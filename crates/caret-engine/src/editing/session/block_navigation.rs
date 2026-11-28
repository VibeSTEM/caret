//! 按文档顺序协调块外侧停靠、相邻块和宿主入口。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::HostCaretTransition;
use super::document_position::visual_affinity_for;
use super::{
    Affinity, Direction, DocumentRangeEndpoint, EditError, Effect, HostCaretKind,
    HostCaretPosition, Pos, Selection, Session, TextMap,
};
use crate::{NavigationCandidate, NavigationGeneration};
use caret_doc::{Block, BlockEnvironment, NodeId};
use caret_structure::{
    BlockContainerPath, ContainerStep, DocumentNodeKind, TableCellPath, block_list_by_container,
    find_block_node,
};

enum BlockVerticalStep {
    Enter,
    AdjacentBoundary(DocumentRangeEndpoint),
    CellBoundary(TableCellPath),
    ParentBoundary(NodeId),
}

impl Session {
    /// 判断上下键是否应先走结构步进。反向扩选从已覆盖块的另一侧收缩。
    pub fn block_boundary_uses_vertical_step(&self, direction: Direction, extend: bool) -> bool {
        if extend
            && let Some((_, DocumentRangeEndpoint::BlockEdge { block_id, side })) =
                self.structure_selection_range()
            && direction != *side
            && (self.selected_block_environments().contains(block_id)
                || self.selected_atomic_blocks().contains(block_id))
        {
            return true;
        }
        self.block_vertical_step(direction).is_some()
    }

    fn block_vertical_step(&self, direction: Direction) -> Option<BlockVerticalStep> {
        let (block_id, side) = self.block_boundary_exit()?;
        let location = find_block_node(&self.document.body, &block_id)?;
        if direction != side {
            return (location.kind == DocumentNodeKind::Table).then_some(BlockVerticalStep::Enter);
        }
        if let Some(block) = self.block_beside_block(&block_id, direction).ok()? {
            let entry = self.block_sequence_entry(std::slice::from_ref(block), direction)?;
            return matches!(entry, DocumentRangeEndpoint::BlockEdge { .. })
                .then_some(BlockVerticalStep::AdjacentBoundary(entry));
        }
        let cell = location.container.innermost_cell()?;
        match location.container.last_step()? {
            ContainerStep::Environment(id) => {
                if direction == Direction::Backward && self.block_environment_title(id).is_some() {
                    return None;
                }
                Some(BlockVerticalStep::ParentBoundary(id.clone()))
            }
            ContainerStep::TableCell(_) => Some(BlockVerticalStep::CellBoundary(cell.clone())),
        }
    }

    pub(super) fn move_vertically_at_block_boundary(
        &mut self,
        direction: Direction,
        extend: bool,
    ) -> Result<Option<Effect>, EditError> {
        let Some(step) = self.block_vertical_step(direction) else {
            return Ok(None);
        };
        if extend {
            return self.extend_structure_selection(direction).map(Some);
        }
        match step {
            BlockVerticalStep::AdjacentBoundary(entry) => self.commit_block_entry(entry).map(Some),
            BlockVerticalStep::Enter => self.move_from_block_boundary_stop(direction),
            BlockVerticalStep::ParentBoundary(id) => {
                self.park_at_block_edge(id, direction).map(Some)
            }
            BlockVerticalStep::CellBoundary(cell) => self
                .move_across_table_cell_boundary(
                    &cell,
                    super::table::TableArrowAxis::Vertical,
                    direction,
                )
                .map(Some),
        }
    }

    /// 从块外停靠进入本块或同方向相邻块。原子块之间共享规范间隙。
    pub(super) fn move_from_block_boundary_stop(
        &mut self,
        direction: Direction,
    ) -> Result<Option<Effect>, EditError> {
        let Some(position) = self.host_caret_position.clone() else {
            return Ok(None);
        };
        if position.kind() != HostCaretKind::Block {
            return Ok(None);
        }
        let Some((block_id, _)) = position.as_outside() else {
            return Ok(None);
        };
        let location =
            find_block_node(&self.document.body, block_id).ok_or(EditError::InvalidPosition)?;
        if !location.kind.is_range_block() {
            return Ok(None);
        }
        if matches!(
            position.horizontal_transition(direction == Direction::Forward, false, false),
            HostCaretTransition::EnterStart | HostCaretTransition::EnterEnd
        ) {
            return if location.kind.is_atomic_block() {
                self.enter_atomic_block(block_id, location.kind, direction)
                    .map(Some)
            } else {
                let Some(Block::Environment(environment)) =
                    find_block_by_id(&self.document.body, block_id)
                else {
                    return Err(EditError::InvalidPosition);
                };
                let Some(entry) = self.environment_entry(environment, direction) else {
                    return Ok(Some(Effect::default()));
                };
                self.commit_block_entry(entry).map(Some)
            };
        }

        let Some(block) = self.block_beside_block(block_id, direction)? else {
            if let Some(parent) = location.container.title_environment_id() {
                if direction == Direction::Backward
                    && let Some(title) = self.block_environment_title(parent)
                    && let Some(entry) = self.environment_title_entry(title, direction)
                {
                    return self.commit_block_entry(entry).map(Some);
                }
                return self.park_at_block_edge(parent.clone(), direction).map(Some);
            }
            if let Some(cell) = location.container.innermost_cell() {
                return self
                    .move_across_table_cell_boundary(
                        cell,
                        super::table::TableArrowAxis::Horizontal,
                        direction,
                    )
                    .map(Some);
            }
            return Ok(Some(Effect::default()));
        };
        if let Some((id, kind)) = block_identity(block)
            && kind.is_atomic_block()
        {
            if location.kind == DocumentNodeKind::Environment {
                return self
                    .park_at_block_edge(id.clone(), opposite(direction))
                    .map(Some);
            }
            let id = id.clone();
            return self.enter_atomic_block(&id, kind, direction).map(Some);
        }

        let endpoint = match block {
            Block::Environment(environment) => self.environment_entry(environment, direction),
            Block::Paragraph(_) => self
                .paragraph_beside_block(block_id, direction)?
                .map(DocumentRangeEndpoint::Text),
            _ => None,
        };
        if let Some(endpoint) = endpoint {
            return self.commit_block_entry(endpoint).map(Some);
        }
        Ok(Some(Effect::default()))
    }

    fn enter_atomic_block(
        &mut self,
        block_id: &NodeId,
        kind: DocumentNodeKind,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        if kind == DocumentNodeKind::Table {
            return self.enter_table_at_edge(block_id, direction);
        }
        let edit = self
            .math_edit_at_block(block_id, direction)?
            .ok_or(EditError::InvalidPosition)?;
        self.clear_host_caret_position();
        self.set_math_edit(Some(edit));
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    pub(super) fn park_at_block_edge(
        &mut self,
        block_id: NodeId,
        side: Direction,
    ) -> Result<Effect, EditError> {
        self.commit_range_focus(
            DocumentRangeEndpoint::BlockEdge { block_id, side },
            None,
            false,
        )
    }

    pub(super) fn adjacent_atomic_block(
        &self,
        position: &Pos,
        direction: Direction,
    ) -> Result<Option<(&NodeId, DocumentNodeKind)>, EditError> {
        let location = self
            .paragraph_index
            .get(&position.paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        let (siblings, index) = if location.is_title {
            let siblings = block_list_by_container(&self.document.body, &location.container)
                .ok_or(EditError::InvalidPosition)?;
            (siblings, (direction == Direction::Forward).then_some(0))
        } else {
            let (container, index) =
                find_block_container_path(&self.document.body, &position.paragraph_id)
                    .ok_or(EditError::InvalidPosition)?;
            let siblings = block_list_by_container(&self.document.body, &container)
                .ok_or(EditError::InvalidPosition)?;
            let candidate = match direction {
                Direction::Backward => index.checked_sub(1),
                Direction::Forward => index.checked_add(1),
            };
            (siblings, candidate)
        };
        Ok(index
            .and_then(|index| self.find_caret_sibling(siblings, index, direction))
            .map(|(_, block)| block)
            .and_then(block_identity)
            .filter(|(_, kind)| kind.is_atomic_block()))
    }

    /// 布局确认段落已到对应视觉边缘后，返回相邻原子块的远侧选区端点。
    /// 文档顺序由 Session 判定，候选在提交时仍需校验布局代际。
    pub fn adjacent_atomic_selection_candidate(
        &self,
        direction: Direction,
        generation: NavigationGeneration,
    ) -> Result<Option<NavigationCandidate>, EditError> {
        Ok(self
            .adjacent_atomic_block(&self.selection.focus, direction)?
            .map(|(block_id, _)| NavigationCandidate::BlockEdge {
                block_id: block_id.clone(),
                side: direction,
                generation,
            }))
    }

    /// 先停在相邻原子块的近侧。水平入口要求到达文本边界，纵向入口由布局识别视觉行边界。
    pub(super) fn park_beside_adjacent_atomic_block(
        &mut self,
        direction: Direction,
        require_text_edge: bool,
    ) -> Result<bool, EditError> {
        if self.selection.anchor != self.selection.focus
            || self.has_structure_selection()
            || self.host_caret_position.as_ref().is_some_and(|host| {
                host.kind() != HostCaretKind::Inline || host.outside_direction() != Some(direction)
            })
        {
            return Ok(false);
        }
        let focus = self.selection.focus.clone();
        if require_text_edge {
            let edge = match direction {
                Direction::Backward => 0,
                Direction::Forward => self.text_map_for(&focus)?.text.len(),
            };
            if focus.offset != edge {
                return Ok(false);
            }
        }
        let Some((block_id, _)) = self.adjacent_atomic_block(&focus, direction)? else {
            return Ok(false);
        };
        self.park_at_block_edge(block_id.clone(), opposite(direction))
            .map(|effect| effect.selection_changed)
    }

    pub(super) fn commit_block_entry(
        &mut self,
        endpoint: DocumentRangeEndpoint,
    ) -> Result<Effect, EditError> {
        let DocumentRangeEndpoint::Text(position) = endpoint else {
            return self.commit_range_focus(endpoint, None, false);
        };
        let empty_inline_host = self
            .text_map_for(&position)?
            .empty_inline_environment_stack_at(position.offset, position.affinity)
            .last()
            .and_then(|span| span.id.clone());
        let visual_affinity = visual_affinity_for(position.affinity);
        if self.selection.anchor == position
            && self.selection.focus == position
            && self.host_caret_position.is_none()
        {
            return Ok(Effect::default());
        }
        self.selection = Selection {
            anchor: position.clone(),
            focus: position.clone(),
        };
        self.selection_visual_affinity = visual_affinity;
        self.selection_anchor_visual_affinity = visual_affinity;
        self.set_host_caret_position(empty_inline_host.map(|host_id| match position.affinity {
            Affinity::Upstream => HostCaretPosition::Before {
                host_id,
                kind: HostCaretKind::Inline,
            },
            Affinity::Downstream => HostCaretPosition::After {
                host_id,
                kind: HostCaretKind::Inline,
            },
        }));
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    fn environment_entry(
        &self,
        environment: &BlockEnvironment,
        direction: Direction,
    ) -> Option<DocumentRangeEndpoint> {
        let title = || self.environment_title_entry(environment.title.as_ref()?, direction);
        let children = || self.block_sequence_entry(&environment.children, direction);
        match direction {
            Direction::Forward => title().or_else(children),
            Direction::Backward => children().or_else(title),
        }
    }

    /// 普通方向移动忽略没有可进入内容的块环境；其他块由结构身份保证可停靠。
    pub(super) fn find_caret_sibling<'a>(
        &self,
        siblings: &'a [Block],
        start: usize,
        direction: Direction,
    ) -> Option<(usize, &'a Block)> {
        find_navigable_sibling(siblings, start, direction, |block| match block {
            Block::Environment(environment) => {
                self.environment_entry(environment, direction).is_some()
            }
            _ => true,
        })
    }

    pub(super) fn block_environment_title(
        &self,
        environment_id: &NodeId,
    ) -> Option<&caret_doc::Paragraph> {
        let Block::Environment(environment) =
            find_block_by_id(&self.document.body, environment_id)?
        else {
            return None;
        };
        environment.title.as_ref()
    }

    fn environment_title_entry(
        &self,
        title: &caret_doc::Paragraph,
        direction: Direction,
    ) -> Option<DocumentRangeEndpoint> {
        let (offset, affinity) = match direction {
            Direction::Forward => (0, Affinity::Downstream),
            Direction::Backward => (TextMap::new(title).ok()?.text.len(), Affinity::Upstream),
        };
        Some(DocumentRangeEndpoint::Text(Pos {
            story: self.story_for_paragraph(&title.id)?,
            paragraph_id: title.id.clone(),
            offset,
            affinity,
        }))
    }

    pub(super) fn block_sequence_entry(
        &self,
        blocks: &[Block],
        direction: Direction,
    ) -> Option<DocumentRangeEndpoint> {
        let entry = |block: &Block| {
            if let Block::Environment(environment) = block {
                return self.environment_entry(environment, direction);
            }
            let (_, start, end) = self.block_range_unit(block)?;
            Some(if direction == Direction::Forward {
                start
            } else {
                end
            })
        };
        match direction {
            Direction::Forward => blocks.iter().find_map(entry),
            Direction::Backward => blocks.iter().rev().find_map(entry),
        }
    }

    fn block_beside_block(
        &self,
        block_id: &NodeId,
        direction: Direction,
    ) -> Result<Option<&Block>, EditError> {
        let (container, index) = find_block_container_path(&self.document.body, block_id)
            .ok_or(EditError::InvalidPosition)?;
        let siblings = block_list_by_container(&self.document.body, &container)
            .ok_or(EditError::InvalidPosition)?;
        let neighbour = match direction {
            Direction::Forward => index.checked_add(1),
            Direction::Backward => index.checked_sub(1),
        };
        Ok(neighbour
            .and_then(|index| self.find_caret_sibling(siblings, index, direction))
            .map(|(_, block)| block))
    }

    /// 为块外侧停靠提供相邻段落锚点，供文本输入与 caret 投影借用。
    pub(super) fn paragraph_beside_block(
        &self,
        block_id: &NodeId,
        direction: Direction,
    ) -> Result<Option<Pos>, EditError> {
        let Some(Block::Paragraph(paragraph)) = self.block_beside_block(block_id, direction)?
        else {
            return Ok(None);
        };
        let (offset, affinity) = match direction {
            Direction::Forward => (0, Affinity::Upstream),
            Direction::Backward => (TextMap::new(paragraph)?.text.len(), Affinity::Downstream),
        };
        Ok(Some(Pos {
            story: self
                .story_for_paragraph(&paragraph.id)
                .ok_or(EditError::InvalidPosition)?,
            paragraph_id: paragraph.id.clone(),
            offset,
            affinity,
        }))
    }
}

/// 查找同级序列中最近的可导航块；显式分页与未知块不产生键盘停靠点。
pub(super) fn find_navigable_sibling(
    siblings: &[Block],
    start: usize,
    direction: Direction,
    mut has_entry: impl FnMut(&Block) -> bool,
) -> Option<(usize, &Block)> {
    let mut cursor = Some(start);
    while let Some(index) = cursor {
        let block = siblings.get(index)?;
        if block_identity(block).is_some() && has_entry(block) {
            return Some((index, block));
        }
        cursor = match direction {
            Direction::Forward => index.checked_add(1),
            Direction::Backward => index.checked_sub(1),
        };
    }
    None
}

fn opposite(direction: Direction) -> Direction {
    match direction {
        Direction::Forward => Direction::Backward,
        Direction::Backward => Direction::Forward,
    }
}

/// 在块树中查找块 ID，返回真实容器路径与同级下标；环境与表格单元格统一寻址。
pub(super) fn find_block_container_path(
    blocks: &[Block],
    id: &NodeId,
) -> Option<(BlockContainerPath, usize)> {
    let location = caret_structure::find_block_node(blocks, id)?;
    if location.kind != caret_structure::DocumentNodeKind::Paragraph
        && !location.kind.is_range_block()
    {
        return None;
    }
    Some((location.container, location.sibling_index?))
}

pub(super) fn find_block_by_id<'a>(blocks: &'a [Block], id: &NodeId) -> Option<&'a Block> {
    let (container, index) = find_block_container_path(blocks, id)?;
    caret_structure::block_list_by_container(blocks, &container)?.get(index)
}

pub(super) fn block_identity(block: &Block) -> Option<(&NodeId, DocumentNodeKind)> {
    Some(match block {
        Block::Paragraph(value) => (&value.id, DocumentNodeKind::Paragraph),
        Block::Environment(value) => (&value.id, DocumentNodeKind::Environment),
        Block::Equation(value) => (&value.id, DocumentNodeKind::Equation),
        Block::Align(value) => (&value.id, DocumentNodeKind::Align),
        Block::Table(value) => (&value.id, DocumentNodeKind::Table),
        Block::ExplicitBreak(_) | Block::Opaque(_) => return None,
    })
}
