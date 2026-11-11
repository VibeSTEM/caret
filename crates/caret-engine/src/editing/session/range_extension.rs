//! 按文档顺序扩展结构选区焦点，并派生块节点覆盖。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::{
    Affinity, Direction, DocumentRangeEndpoint, EditError, Effect, HostCaretPosition, Pos,
    Selection, Session, StructureSelection,
};
use super::document_position::{visual_affinity_for, visual_affinity_for_block_edge};
use super::math_edit::find_block_path;
use crate::editing::StoryId;
use caret_doc::{Block, NodeId};
use caret_structure::{DocumentNodeKind, block_list_by_environment_ids, find_document_node};

impl Session {
    /// 将结构选区焦点按一个文档单元向指定方向移动。
    ///
    /// 普通段落内按字素边界移动；跨越块序列时，块环境与 Equation/Align 作为一个
    /// 原子对象进入范围。整个调用先完成排序、覆盖与落点校验，再一次性提交状态。
    pub(super) fn extend_structure_selection(
        &mut self,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Ok(Effect::default());
        }
        self.ensure_math_edit_revision_current()?;
        let current = self.current_range_focus_endpoint();
        let anchor = match &self.structure_selection {
            Some(selection) => selection.anchor.clone(),
            None if self.selection.anchor == self.selection.focus => current.clone(),
            None => DocumentRangeEndpoint::Text(self.selection.anchor.clone()),
        };
        let Some(focus) = self.range_focus_step(&current, direction)? else {
            return Ok(Effect::default());
        };
        let (anchor, focus) = self.normalize_environment_range(anchor, focus)?;
        let selection = StructureSelection { anchor, focus };
        // 先派生覆盖列表：空覆盖范围绝不能安装成结构选区，否则隐藏范围会吞掉按键。
        let (start, end) =
            match self.compare_range_endpoints(&selection.anchor, &selection.focus)? {
                std::cmp::Ordering::Greater => (&selection.focus, &selection.anchor),
                _ => (&selection.anchor, &selection.focus),
            };
        let (environments, math_blocks) = self.covered_block_nodes(start, end)?;
        if environments.is_empty() && math_blocks.is_empty() {
            return self.extend_to_empty_coverage(selection);
        }
        // 落点先验证，随后由统一写入入口派生覆盖列表；失败时选区与历史不变。
        let (caret, host_caret_position) = self.park_at_range_endpoint(&selection.focus)?;
        let focus_visual_affinity = match &selection.focus {
            DocumentRangeEndpoint::Text(position) => visual_affinity_for(position.affinity),
            DocumentRangeEndpoint::BlockEdge { side, .. } => visual_affinity_for_block_edge(*side),
        };
        let previous_selection = self.structure_selection.clone();
        let previous_environments = self.selected_block_environments.clone();
        let previous_math_blocks = self.selected_math_blocks.clone();
        self.install_structure_selection(selection)?;
        let changed = self.structure_selection != previous_selection
            || self.selected_block_environments != previous_environments
            || self.selected_math_blocks != previous_math_blocks;
        self.selection = Selection {
            anchor: caret.clone(),
            focus: caret,
        };
        self.host_caret_position = host_caret_position;
        self.selection_visual_affinity = focus_visual_affinity;
        self.pending_inline_style.clear();
        if changed {
            self.advance_history_merge_epoch();
        }
        Ok(Effect {
            selection_changed: changed,
            ..Effect::default()
        })
    }

    /// 空覆盖范围不建立结构选区：文本对保留普通选区，块外侧端点重新停靠并清除结构状态。
    fn extend_to_empty_coverage(
        &mut self,
        selection: StructureSelection,
    ) -> Result<Effect, EditError> {
        let previous_selection = self.selection.clone();
        let previous_visual_affinity = self.selection_visual_affinity;
        let had_structure = self.has_structure_selection();
        if had_structure {
            self.clear_structure_selection_state();
        }
        match &selection.focus {
            DocumentRangeEndpoint::Text(focus) => {
                let anchor = match &selection.anchor {
                    DocumentRangeEndpoint::Text(anchor) => anchor.clone(),
                    DocumentRangeEndpoint::BlockEdge { .. } => focus.clone(),
                };
                self.selection = Selection {
                    anchor,
                    focus: focus.clone(),
                };
                self.host_caret_position = None;
                self.selection_visual_affinity = visual_affinity_for(focus.affinity);
            }
            DocumentRangeEndpoint::BlockEdge { block_id, side } => {
                let (caret, host) = self.park_beside_block_host(block_id, *side)?;
                self.selection = Selection {
                    anchor: caret.clone(),
                    focus: caret,
                };
                self.host_caret_position = Some(host);
                self.selection_visual_affinity = visual_affinity_for_block_edge(*side);
            }
        }
        self.pending_inline_style.clear();
        let changed = had_structure
            || previous_selection != self.selection
            || previous_visual_affinity != self.selection_visual_affinity;
        if changed {
            self.advance_history_merge_epoch();
        }
        Ok(Effect {
            selection_changed: changed,
            ..Effect::default()
        })
    }

    /// 当前扩展焦点：优先使用结构选区焦点，其次块外侧停靠，最后普通文本焦点。
    fn current_range_focus_endpoint(&self) -> DocumentRangeEndpoint {
        if let Some(selection) = &self.structure_selection {
            return selection.focus.clone();
        }
        if let Some((block_id, direction)) = self.block_boundary_exit() {
            return DocumentRangeEndpoint::BlockEdge {
                block_id,
                side: direction,
            };
        }
        DocumentRangeEndpoint::Text(self.selection.focus.clone())
    }

    fn range_focus_step(
        &mut self,
        focus: &DocumentRangeEndpoint,
        direction: Direction,
    ) -> Result<Option<DocumentRangeEndpoint>, EditError> {
        match focus {
            DocumentRangeEndpoint::Text(position) => {
                self.text_range_focus_step(position, direction)
            }
            DocumentRangeEndpoint::BlockEdge { block_id, side } => {
                self.block_edge_range_focus_step(block_id, *side, direction)
            }
        }
    }

    fn text_range_focus_step(
        &mut self,
        position: &Pos,
        direction: Direction,
    ) -> Result<Option<DocumentRangeEndpoint>, EditError> {
        self.text_map_for(position)?.validate_pos(position)?;
        let (offset, affinity) = {
            let map = self.text_map_for(position)?;
            let offset = match direction {
                Direction::Backward => map.previous_boundary(position.offset),
                Direction::Forward => map.next_boundary(position.offset),
            };
            let affinity = if map.has_environment_boundary(offset) {
                match direction {
                    Direction::Backward => Affinity::Downstream,
                    Direction::Forward => Affinity::Upstream,
                }
            } else {
                position.affinity
            };
            (offset, affinity)
        };
        if offset != position.offset {
            return Ok(Some(DocumentRangeEndpoint::Text(Pos {
                offset,
                affinity,
                ..position.clone()
            })));
        }
        self.paragraph_unit_step(position, direction)
    }

    /// 从段落边界跨到相邻块单元；没有相邻块时只越过嵌套环境的父边界。
    fn paragraph_unit_step(
        &self,
        position: &Pos,
        direction: Direction,
    ) -> Result<Option<DocumentRangeEndpoint>, EditError> {
        if let Some(environment_id) = self
            .structure_index
            .title_environment_id(&position.paragraph_id)
            .cloned()
        {
            return match direction {
                Direction::Backward => Ok(Some(block_edge(environment_id, Direction::Backward))),
                // 标题属于环境本体：从标题末尾向前直接选中整个环境，不进入子块。
                Direction::Forward => Ok(Some(block_edge(environment_id, Direction::Forward))),
            };
        }
        if position.story != StoryId::Body {
            return Err(EditError::InvalidPosition);
        }
        let (parent_environment_ids, sibling_index) =
            find_block_path(&self.document.body, &position.paragraph_id)
                .ok_or(EditError::InvalidPosition)?;
        let siblings = block_list_by_environment_ids(&self.document.body, &parent_environment_ids)
            .ok_or(EditError::InvalidPosition)?;
        match direction {
            Direction::Forward => {
                self.unit_start_after(siblings, &parent_environment_ids, sibling_index)
            }
            Direction::Backward => {
                self.unit_end_before(siblings, &parent_environment_ids, sibling_index)
            }
        }
    }

    /// 环境是原子单元：焦点停在环境外侧边界而锚点在其内部时，把锚点扩到
    /// 环境另一侧边界，使整个环境进入结构范围。
    ///
    /// 这与块宿主协议一致：范围触及环境边界即提升为整个环境，而不是留下只覆盖
    /// 环境一部分、无法作为对象删除或复制的半开范围。
    fn normalize_environment_range(
        &self,
        anchor: DocumentRangeEndpoint,
        focus: DocumentRangeEndpoint,
    ) -> Result<(DocumentRangeEndpoint, DocumentRangeEndpoint), EditError> {
        let DocumentRangeEndpoint::BlockEdge { block_id, side } = &focus else {
            return Ok((anchor, focus));
        };
        let location =
            find_document_node(&self.document, block_id).ok_or(EditError::InvalidPosition)?;
        if location.kind != DocumentNodeKind::Environment
            || !self.endpoint_is_inside_environment(&anchor, block_id)
        {
            return Ok((anchor, focus));
        }
        let anchor = match side {
            Direction::Forward => block_edge(block_id.clone(), Direction::Backward),
            Direction::Backward => block_edge(block_id.clone(), Direction::Forward),
        };
        Ok((anchor, focus))
    }

    /// 块外侧边界作为焦点时按单元继续移动；反向键先覆盖当前节点本身。
    fn block_edge_range_focus_step(
        &self,
        block_id: &NodeId,
        side: Direction,
        direction: Direction,
    ) -> Result<Option<DocumentRangeEndpoint>, EditError> {
        let location =
            find_document_node(&self.document, block_id).ok_or(EditError::InvalidPosition)?;
        if !location.kind.is_range_block() {
            return Err(EditError::InvalidPosition);
        }
        let (parent_environment_ids, sibling_index) =
            find_block_path(&self.document.body, block_id).ok_or(EditError::InvalidPosition)?;
        let siblings = block_list_by_environment_ids(&self.document.body, &parent_environment_ids)
            .ok_or(EditError::InvalidPosition)?;
        match (side, direction) {
            // 从节点外前向外后移动时整块进入范围，不展开内部子块。
            (Direction::Backward, Direction::Forward) => {
                Ok(Some(block_edge(block_id.clone(), Direction::Forward)))
            }
            (Direction::Forward, Direction::Backward) => {
                Ok(Some(block_edge(block_id.clone(), Direction::Backward)))
            }
            (Direction::Forward, Direction::Forward) => {
                self.unit_start_after(siblings, &parent_environment_ids, sibling_index)
            }
            (Direction::Backward, Direction::Backward) => {
                self.unit_end_before(siblings, &parent_environment_ids, sibling_index)
            }
        }
    }

    /// 返回指定同级下标之后的第一个可导航块起点；到末尾时越过父环境后边界。
    fn unit_start_after(
        &self,
        siblings: &[Block],
        parent_environment_ids: &[NodeId],
        sibling_index: usize,
    ) -> Result<Option<DocumentRangeEndpoint>, EditError> {
        let mut cursor = sibling_index + 1;
        while let Some(block) = siblings.get(cursor) {
            if matches!(block, Block::ExplicitBreak(_) | Block::Opaque(_)) {
                cursor += 1;
                continue;
            }
            return Ok(self.block_unit_start(block));
        }
        Ok(parent_environment_ids
            .last()
            .cloned()
            .map(|environment_id| block_edge(environment_id, Direction::Forward)))
    }

    /// 返回指定同级下标之前的第一个可导航块终点；到开头时越过父环境前边界。
    fn unit_end_before(
        &self,
        siblings: &[Block],
        parent_environment_ids: &[NodeId],
        sibling_index: usize,
    ) -> Result<Option<DocumentRangeEndpoint>, EditError> {
        let mut cursor = sibling_index.checked_sub(1);
        while let Some(index) = cursor {
            let block = siblings.get(index).ok_or(EditError::InvalidPosition)?;
            if matches!(block, Block::ExplicitBreak(_) | Block::Opaque(_)) {
                cursor = index.checked_sub(1);
                continue;
            }
            return Ok(self.block_unit_end(block));
        }
        Ok(parent_environment_ids
            .last()
            .cloned()
            .map(|environment_id| block_edge(environment_id, Direction::Backward)))
    }

    /// 块作为范围单元被向前进入时的焦点：复杂块用外后边界，段落用真实文本起点。
    fn block_unit_start(&self, block: &Block) -> Option<DocumentRangeEndpoint> {
        let (_, start, end) = self.block_range_unit(block)?;
        Some(match block {
            Block::Paragraph(_) => start,
            _ => end,
        })
    }

    /// 块作为范围单元被向后越出时的焦点：复杂块用外前边界，段落用真实文本终点。
    fn block_unit_end(&self, block: &Block) -> Option<DocumentRangeEndpoint> {
        let (_, start, end) = self.block_range_unit(block)?;
        Some(match block {
            Block::Paragraph(_) => end,
            _ => start,
        })
    }

    /// 判断段落同方向紧邻的块是否是 Equation/Align。
    pub(super) fn paragraph_beside_block_math(
        &self,
        position: &Pos,
        direction: Direction,
    ) -> Result<bool, EditError> {
        let Some((parent_environment_ids, sibling_index)) =
            find_block_path(&self.document.body, &position.paragraph_id)
        else {
            return Ok(false);
        };
        let siblings = block_list_by_environment_ids(&self.document.body, &parent_environment_ids)
            .ok_or(EditError::InvalidPosition)?;
        let neighbour = match direction {
            Direction::Forward => sibling_index.checked_add(1),
            Direction::Backward => sibling_index.checked_sub(1),
        };
        Ok(matches!(
            neighbour.and_then(|index| siblings.get(index)),
            Some(Block::Equation(_) | Block::Align(_))
        ))
    }

    /// 结构选区端点落点：文本端点保持原位置，块外侧边界复用共享宿主停靠策略。
    fn park_at_range_endpoint(
        &mut self,
        focus: &DocumentRangeEndpoint,
    ) -> Result<(Pos, Option<HostCaretPosition<Pos>>), EditError> {
        if let DocumentRangeEndpoint::Text(position) = focus {
            self.text_map_for(position)?.validate_pos(position)?;
            return Ok((position.clone(), None));
        }
        let DocumentRangeEndpoint::BlockEdge { block_id, side } = focus else {
            return Err(EditError::InvalidPosition);
        };
        let (caret, host) = self.park_beside_block_host(block_id, *side)?;
        Ok((caret, Some(host)))
    }
}

fn block_edge(block_id: NodeId, side: Direction) -> DocumentRangeEndpoint {
    DocumentRangeEndpoint::BlockEdge { block_id, side }
}

#[cfg(test)]
#[path = "range_extension/tests.rs"]
mod tests;
