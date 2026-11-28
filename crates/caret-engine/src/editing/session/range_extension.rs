//! 按文档顺序扩展结构选区焦点，并派生块节点覆盖。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::{
    Affinity, Direction, DocumentRangeEndpoint, EditError, Effect, Pos, Selection, Session,
    StructureSelection,
};
use super::block_navigation::{find_block_container_path, find_navigable_sibling};
use super::document_position::{visual_affinity_for, visual_affinity_for_block_edge};
use caret_doc::{Block, NodeId};
use caret_structure::{DocumentNodeKind, block_list_by_container, find_document_node};

impl Session {
    /// 将结构选区焦点按一个文档单元向指定方向移动。
    ///
    /// 普通段落内按字素边界移动；跨越块序列时，复杂块作为一个
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
        let Some(focus) = self.range_focus_step(&current, direction)? else {
            return Ok(Effect::default());
        };
        self.commit_range_focus(focus, None, true)
    }

    /// 提交已解析的端点，布局扩选与文档顺序扩选共用锚点、覆盖及折叠规则。
    pub(super) fn commit_range_focus(
        &mut self,
        focus: DocumentRangeEndpoint,
        visual: Option<crate::VisualAffinity>,
        extend: bool,
    ) -> Result<Effect, EditError> {
        let anchor = match &self.structure_selection {
            _ if !extend => focus.clone(),
            Some(selection) => selection.anchor.clone(),
            None if self.selection.anchor == self.selection.focus => {
                self.current_range_focus_endpoint()
            }
            None => DocumentRangeEndpoint::Text(self.selection.anchor.clone()),
        };
        self.commit_range_between(anchor, focus, visual, extend)
    }

    pub(super) fn commit_range_between(
        &mut self,
        anchor: DocumentRangeEndpoint,
        focus: DocumentRangeEndpoint,
        visual: Option<crate::VisualAffinity>,
        extend: bool,
    ) -> Result<Effect, EditError> {
        // 跨越 Section 边界时，区域按整棵环境覆盖，几何焦点仍停在候选视觉行。
        let visual_focus = if visual.is_some() && extend {
            Some(self.park_at_range_endpoint(&focus)?)
        } else {
            None
        };
        let selection = if visual_focus.is_some() {
            let ancestor_ids = |endpoint: &DocumentRangeEndpoint| {
                find_document_node(&self.document, endpoint.block_container_node_id())
                    .map(|location| location.parent_environment_ids)
                    .ok_or(EditError::InvalidPosition)
            };
            let anchor_parents = ancestor_ids(&anchor)?;
            let focus_parents = ancestor_ids(&focus)?;
            let common = anchor_parents
                .iter()
                .zip(&focus_parents)
                .take_while(|(a, b)| a == b)
                .count();
            let crossed = anchor_parents[common..]
                .iter()
                .chain(&focus_parents[common..])
                .cloned()
                .collect::<Vec<_>>();
            self.normalized_environment_range(anchor, focus, &crossed)?
        } else {
            let (anchor, focus) = self.normalize_environment_range(anchor, focus)?;
            StructureSelection { anchor, focus }
        };
        if selection.anchor.is_inline_math() || selection.focus.is_inline_math() {
            return self.install_inline_math_selection(
                selection,
                self.selection_anchor_visual_affinity,
                visual.unwrap_or(self.selection_visual_affinity),
            );
        }
        // 先派生覆盖列表：空覆盖范围绝不能安装成结构选区，否则隐藏范围会吞掉按键。
        // 结构选区安装入口还有同一空覆盖守卫；这里提前判断是为了回落到普通选区，
        // 而不是让安装失败。
        let (start, end) =
            match self.compare_range_endpoints(&selection.anchor, &selection.focus)? {
                std::cmp::Ordering::Greater => (&selection.focus, &selection.anchor),
                _ => (&selection.anchor, &selection.focus),
            };
        let (environments, math_blocks) = self.covered_block_nodes(start, end)?;
        if environments.is_empty() && math_blocks.is_empty() {
            return self.extend_to_empty_coverage(selection, visual);
        }
        // 落点先验证，随后由统一写入入口派生覆盖列表；失败时选区与历史不变。
        let (caret, host_caret_position) = match visual_focus {
            Some(parked) => parked,
            None => self.park_at_range_endpoint(&selection.focus)?,
        };
        let focus_visual_affinity = visual.unwrap_or_else(|| match &selection.focus {
            DocumentRangeEndpoint::InlineMathTree { position, .. } => match position.affinity {
                crate::MathPositionAffinity::Upstream => crate::VisualAffinity::Upstream,
                crate::MathPositionAffinity::Downstream => crate::VisualAffinity::Downstream,
            },
            DocumentRangeEndpoint::InlineMathText { position, .. } => match position.affinity {
                crate::MathPositionAffinity::Upstream => crate::VisualAffinity::Upstream,
                crate::MathPositionAffinity::Downstream => crate::VisualAffinity::Downstream,
            },
            DocumentRangeEndpoint::Text(position) => visual_affinity_for(position.affinity),
            DocumentRangeEndpoint::BlockEdge { side, .. } => visual_affinity_for_block_edge(*side),
        });
        let previous_selection = self.structure_selection.clone();
        let previous_environments = self.selected_block_environments.clone();
        let previous_math_blocks = self.selected_atomic_blocks.clone();
        self.install_structure_selection(selection)?;
        let changed = self.structure_selection != previous_selection
            || self.selected_block_environments != previous_environments
            || self.selected_atomic_blocks != previous_math_blocks;
        self.selection = Selection {
            anchor: caret.clone(),
            focus: caret,
        };
        self.set_host_caret_position(host_caret_position);
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
        visual: Option<crate::VisualAffinity>,
    ) -> Result<Effect, EditError> {
        let previous_selection = self.selection.clone();
        let previous_visual_affinity = self.selection_visual_affinity;
        let had_structure = self.has_structure_selection();
        let previous_host = self.host_caret_position.clone();
        match &selection.focus {
            DocumentRangeEndpoint::InlineMathTree { .. }
            | DocumentRangeEndpoint::InlineMathText { .. } => {
                return Err(EditError::InvalidPosition);
            }
            DocumentRangeEndpoint::Text(focus) => {
                self.text_map_for(focus)?.validate_pos(focus)?;
                let anchor = match &selection.anchor {
                    DocumentRangeEndpoint::InlineMathTree { .. }
                    | DocumentRangeEndpoint::InlineMathText { .. } => {
                        return Err(EditError::InvalidPosition);
                    }
                    DocumentRangeEndpoint::Text(anchor) => anchor.clone(),
                    DocumentRangeEndpoint::BlockEdge { .. } => self
                        .text_endpoint_clamped(&selection.anchor)
                        .unwrap_or_else(|| focus.clone()),
                };
                if anchor.story != focus.story {
                    return Ok(Effect::default());
                }
                self.selection = Selection {
                    anchor,
                    focus: focus.clone(),
                };
                self.clear_host_caret_position();
                self.selection_visual_affinity =
                    visual.unwrap_or_else(|| visual_affinity_for(focus.affinity));
            }
            DocumentRangeEndpoint::BlockEdge { block_id, side } => {
                let (caret, host) = self.park_beside_block_host(block_id, *side)?;
                self.selection = Selection {
                    anchor: caret.clone(),
                    focus: caret,
                };
                self.set_host_caret_position(Some(host));
                self.selection_visual_affinity = visual_affinity_for_block_edge(*side);
            }
        }
        self.pending_inline_style.clear();
        self.clear_structure_selection_state();
        let changed = had_structure
            || previous_host != self.host_caret_position
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
    pub(super) fn current_range_focus_endpoint(&self) -> DocumentRangeEndpoint {
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
            DocumentRangeEndpoint::InlineMathTree { .. }
            | DocumentRangeEndpoint::InlineMathText { .. } => Ok(None),
            DocumentRangeEndpoint::Text(position) => {
                self.text_range_focus_step(position, direction)
            }
            DocumentRangeEndpoint::BlockEdge { block_id, side } => {
                self.block_edge_range_focus_step(block_id, *side, direction)
            }
        }
    }

    /// 只读地返回焦点按一个文档单元前进后的端点，供按键仲裁判断候选是否跳过
    /// 紧邻的复杂块；不安装结构选区、不改变 Session 状态。
    pub(in crate::editing::session) fn next_range_focus_step(
        &mut self,
        endpoint: &DocumentRangeEndpoint,
        direction: Direction,
    ) -> Result<Option<DocumentRangeEndpoint>, EditError> {
        self.range_focus_step(endpoint, direction)
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
        let (container, sibling_index) =
            find_block_container_path(&self.document.body, &position.paragraph_id)
                .ok_or(EditError::InvalidPosition)?;
        let parent = container.title_environment_id();
        let siblings = block_list_by_container(&self.document.body, &container)
            .ok_or(EditError::InvalidPosition)?;
        match direction {
            Direction::Forward => self.unit_start_after(siblings, parent, sibling_index),
            Direction::Backward => self.unit_end_before(siblings, parent, sibling_index),
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
        let sibling_index = location.sibling_index.ok_or(EditError::InvalidPosition)?;
        let parent = location.container.title_environment_id();
        let siblings = block_list_by_container(&self.document.body, &location.container)
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
                self.unit_start_after(siblings, parent, sibling_index)
            }
            (Direction::Backward, Direction::Backward) => {
                self.unit_end_before(siblings, parent, sibling_index)
            }
        }
    }

    /// 返回指定同级下标之后的第一个可导航块起点；到末尾时越过父环境后边界。
    fn unit_start_after(
        &self,
        siblings: &[Block],
        parent: Option<&NodeId>,
        sibling_index: usize,
    ) -> Result<Option<DocumentRangeEndpoint>, EditError> {
        let block =
            find_navigable_sibling(siblings, sibling_index + 1, Direction::Forward, |_| true)
                .map(|(_, block)| block);
        if let Some(block) = block {
            return Ok(self.block_unit_start(block));
        }
        Ok(parent
            .cloned()
            .map(|environment_id| block_edge(environment_id, Direction::Forward)))
    }

    /// 返回指定同级下标之前的第一个可导航块终点；到开头时越过父环境前边界。
    fn unit_end_before(
        &self,
        siblings: &[Block],
        parent: Option<&NodeId>,
        sibling_index: usize,
    ) -> Result<Option<DocumentRangeEndpoint>, EditError> {
        if let Some((_, block)) = sibling_index.checked_sub(1).and_then(|index| {
            find_navigable_sibling(siblings, index, Direction::Backward, |_| true)
        }) {
            return Ok(self.block_unit_end(block));
        }
        Ok(parent
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
}

fn block_edge(block_id: NodeId, side: Direction) -> DocumentRangeEndpoint {
    DocumentRangeEndpoint::BlockEdge { block_id, side }
}

#[cfg(test)]
#[path = "range_extension/tests.rs"]
mod tests;
