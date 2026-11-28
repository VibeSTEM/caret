//! 执行 Rust 结构索引上的段落停靠导航与跳出容器。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::text_map::TextMap;
use super::super::{
    Affinity, Command, Direction, DocumentRangeEndpoint, EditError, Effect, HostCaretPosition, Pos,
    PositionMap, Selection, StoryId,
};
use super::Session;
use super::history::{self, DocumentEditSnapshot, DocumentPatch, SessionHistoryState};
use crate::editing::paragraph_ops::BlockContainerPath;
use crate::editing::paragraph_ops::{
    block_list_in_container, compare_positions, paragraph_at_location,
    paragraph_parent_container_path, paragraph_parent_path,
};
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, NodeId, Paragraph, Run, SoftBreak, TextFlow,
};
use caret_layout::VisualAffinity;
use caret_structure::{
    ContainerStep, StructureIndex, StructureKind, StructurePosition, find_document_node,
};
use std::{collections::BTreeMap, sync::Arc};

#[path = "structure/environment_mutations.rs"]
mod environment_mutations;

#[path = "structure/selection.rs"]
mod selection;

pub(super) struct StructureEditBeforeState {
    inline_range: Option<Box<super::StructureSelection>>,
    selection: Selection,
    anchor_visual_affinity: VisualAffinity,
    visual_affinity: VisualAffinity,
    state_id: u64,
    host_caret_position: Option<HostCaretPosition<Pos>>,
}

fn block_range(block_id: NodeId, forward: bool) -> (DocumentRangeEndpoint, DocumentRangeEndpoint) {
    let (anchor_side, focus_side) = if forward {
        (Direction::Backward, Direction::Forward)
    } else {
        (Direction::Forward, Direction::Backward)
    };
    (
        DocumentRangeEndpoint::BlockEdge {
            block_id: block_id.clone(),
            side: anchor_side,
        },
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: focus_side,
        },
    )
}

impl Session {
    /// 统一提交结构事务的索引、历史与文档代际更新。
    pub(super) fn finalize_structure_edit(
        &mut self,
        updated_paragraph_index: super::super::paragraph_ops::ParagraphIndex,
        patch: DocumentPatch,
        before_selection: Selection,
        before_anchor_visual_affinity: VisualAffinity,
        before_visual_affinity: VisualAffinity,
        before_state_id: u64,
    ) {
        self.finalize_structure_edit_with_boundary(
            updated_paragraph_index,
            patch,
            StructureEditBeforeState {
                inline_range: None,
                selection: before_selection,
                anchor_visual_affinity: before_anchor_visual_affinity,
                visual_affinity: before_visual_affinity,
                state_id: before_state_id,
                host_caret_position: None,
            },
        );
    }

    /// 记录结构编辑前的选区和宿主停靠，供历史准确恢复。
    pub(super) fn capture_structure_edit_before_state(&self) -> StructureEditBeforeState {
        StructureEditBeforeState {
            inline_range: self
                .structure_selection
                .as_ref()
                .filter(|range| range.anchor.is_inline_math() || range.focus.is_inline_math())
                .cloned()
                .map(Box::new),
            selection: self.selection.clone(),
            anchor_visual_affinity: self.selection_anchor_visual_affinity,
            visual_affinity: self.selection_visual_affinity,
            state_id: self.history_state_id,
            host_caret_position: self.host_caret_position.clone(),
        }
    }

    pub(super) fn finalize_structure_edit_with_boundary(
        &mut self,
        updated_paragraph_index: super::super::paragraph_ops::ParagraphIndex,
        patch: DocumentPatch,
        before: StructureEditBeforeState,
    ) {
        self.paragraph_index = updated_paragraph_index;
        self.structure_index = Arc::new(StructureIndex::build(&self.document));
        self.text_map_cache.clear();
        self.advance_history_merge_epoch();

        let mut before_state = SessionHistoryState::new(
            before.selection,
            before.visual_affinity,
            self.pending_inline_style.clone(),
            before.state_id,
        )
        .with_anchor_visual_affinity(before.anchor_visual_affinity)
        .with_host_caret_position(before.host_caret_position);
        before_state.inline_range = before.inline_range;
        self.commit_document_edit(patch, before_state, DocumentEditSnapshot::Body);
    }

    /// 在块环境后的空段落段首退格时，仅移除内容为空的环境。
    pub(super) fn delete_empty_environment_before(
        &mut self,
        position: &Pos,
    ) -> Result<Option<Effect>, EditError> {
        if position.story != StoryId::Body || position.offset != 0 {
            return Ok(None);
        }
        let Some(location) = self.paragraph_index.get(&position.paragraph_id).cloned() else {
            return Ok(None);
        };
        let Some((parent_environment_ids, sibling_index)) =
            paragraph_parent_path(&self.document.body, &location)
        else {
            return Ok(None);
        };
        let Some(environment_index) = sibling_index.checked_sub(1) else {
            return Ok(None);
        };
        let Some(siblings) = block_list_by_parent_ids(&self.document.body, &parent_environment_ids)
        else {
            return Err(EditError::InvalidPosition);
        };
        let Some(Block::Environment(environment)) = siblings.get(environment_index) else {
            return Ok(None);
        };
        if environment_has_content(environment)? {
            return Ok(None);
        }
        let environment = environment.clone();
        let environment_estimated_bytes = history::environment_estimated_bytes(&environment);
        let Some(siblings) =
            block_list_by_parent_ids_mut(&mut self.document.body, &parent_environment_ids)
        else {
            return Err(EditError::InvalidPosition);
        };
        siblings.remove(environment_index);

        let before_selection = self.selection.clone();
        let before_anchor_visual_affinity = self.selection_anchor_visual_affinity;
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        self.finalize_structure_edit(
            super::super::paragraph_ops::build_paragraph_index(&self.document.body),
            DocumentPatch::EnvironmentDeletion {
                parent_environment_ids,
                sibling_index: environment_index,
                environment: Box::new(environment),
                environment_estimated_bytes,
                fallback_paragraph: None,
            },
            before_selection.clone(),
            before_anchor_visual_affinity,
            before_visual_affinity,
            before_state_id,
        );
        Ok(Some(Effect {
            document_changed: true,
            ..Effect::default()
        }))
    }

    /// 删除公式块后紧邻的空段落，并将光标放回公式末端。
    pub(super) fn delete_empty_paragraph_after_math(
        &mut self,
        position: &Pos,
    ) -> Result<Option<Effect>, EditError> {
        if position.story != StoryId::Body
            || position.offset != 0
            || self.math_edit.is_some()
            || self.block_boundary_exit().is_some()
            || self.composition.is_some()
            || self.command_line.is_some()
            || self.selection.anchor != self.selection.focus
            || !self.text_map_for(position)?.text.is_empty()
        {
            return Ok(None);
        }
        let Some(location) = self.paragraph_index.get(&position.paragraph_id).cloned() else {
            return Ok(None);
        };
        if location.is_title {
            return Ok(None);
        }
        let Some((parent_ids, paragraph_index)) =
            paragraph_parent_path(&self.document.body, &location)
        else {
            return Ok(None);
        };
        let Some(math_index) = paragraph_index.checked_sub(1) else {
            return Ok(None);
        };
        let siblings = block_list_by_parent_ids(&self.document.body, &parent_ids)
            .ok_or(EditError::InvalidPosition)?;
        if !matches!(
            siblings.get(math_index),
            Some(Block::Equation(_) | Block::Align(_))
        ) {
            return Ok(None);
        }
        let math_id = match siblings.get(math_index) {
            Some(Block::Equation(equation)) => equation.id.clone(),
            Some(Block::Align(align)) => align.id.clone(),
            _ => unreachable!("the preceding sibling was checked as a math block"),
        };
        let Some(edit) = self.math_edit_at_block(&math_id, Direction::Backward)? else {
            return Ok(None);
        };
        let anchor_paragraph = siblings[..paragraph_index]
            .iter()
            .rev()
            .find_map(|block| match block {
                Block::Paragraph(paragraph) => Some((paragraph, Direction::Backward)),
                _ => None,
            })
            .or_else(|| {
                siblings[paragraph_index + 1..]
                    .iter()
                    .find_map(|block| match block {
                        Block::Paragraph(paragraph) => Some((paragraph, Direction::Forward)),
                        _ => None,
                    })
            });
        let selection_position =
            if let Some((anchor_paragraph, anchor_direction)) = anchor_paragraph {
                Pos {
                    story: StoryId::Body,
                    paragraph_id: anchor_paragraph.id.clone(),
                    offset: match anchor_direction {
                        Direction::Backward => TextMap::new(anchor_paragraph)?.text.len(),
                        Direction::Forward => 0,
                    },
                    affinity: match anchor_direction {
                        Direction::Backward => Affinity::Downstream,
                        Direction::Forward => Affinity::Upstream,
                    },
                }
            } else {
                let ordinal = self
                    .structure_index
                    .paragraph_ordinal(&position.paragraph_id)
                    .ok_or(EditError::InvalidPosition)?;
                let previous = (0..ordinal).rev().find_map(|index| {
                    let paragraph_id = self.structure_index.paragraph_id_at(index)?;
                    (paragraph_id != &position.paragraph_id).then_some(paragraph_id)
                });
                let next = (ordinal + 1..self.structure_index.navigation_paragraphs().len())
                    .find_map(|index| self.structure_index.paragraph_id_at(index));
                let nearby = previous
                    .map(|id| (id, Direction::Backward))
                    .or_else(|| next.map(|id| (id, Direction::Forward)));
                let Some((paragraph_id, direction)) = nearby else {
                    return Ok(None);
                };
                let location = self
                    .paragraph_index
                    .get(paragraph_id)
                    .ok_or(EditError::InvalidPosition)?;
                let paragraph = paragraph_at_location(&self.document.body, location)
                    .ok_or(EditError::InvalidPosition)?;
                Pos {
                    story: location.story.clone(),
                    paragraph_id: paragraph_id.clone(),
                    offset: if direction == Direction::Backward {
                        TextMap::new(paragraph)?.text.len()
                    } else {
                        0
                    },
                    affinity: if direction == Direction::Backward {
                        Affinity::Downstream
                    } else {
                        Affinity::Upstream
                    },
                }
            };
        let Some(siblings) = block_list_by_parent_ids_mut(&mut self.document.body, &parent_ids)
        else {
            return Err(EditError::InvalidPosition);
        };
        let removed = siblings.remove(paragraph_index);
        let Block::Paragraph(removed_paragraph) = removed else {
            return Err(EditError::InvalidPosition);
        };
        if let Err(error) = self.document.validate() {
            if let Some(siblings) =
                block_list_by_parent_ids_mut(&mut self.document.body, &parent_ids)
            {
                siblings.insert(paragraph_index, Block::Paragraph(removed_paragraph));
            }
            return Err(EditError::InvalidDocument(error.to_string()));
        }

        let before_selection = self.selection.clone();
        let before_anchor_visual_affinity = self.selection_anchor_visual_affinity;
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let before_block_boundary_exit = self.host_caret_position.clone();
        self.selection = Selection {
            anchor: selection_position.clone(),
            focus: selection_position.clone(),
        };
        self.selection_visual_affinity = match selection_position.affinity {
            Affinity::Upstream => VisualAffinity::Upstream,
            Affinity::Downstream => VisualAffinity::Downstream,
        };
        self.selection_anchor_visual_affinity = self.selection_visual_affinity;
        self.set_math_edit(Some(edit));
        self.clear_host_caret_position();
        self.finalize_structure_edit_with_boundary(
            super::super::paragraph_ops::build_paragraph_index(&self.document.body),
            DocumentPatch::BlockRange {
                container: BlockContainerPath::from_environment_ids(&parent_ids),
                sibling_index: paragraph_index,
                before: vec![Block::Paragraph(removed_paragraph)],
                after: Vec::new(),
            },
            StructureEditBeforeState {
                inline_range: None,
                selection: before_selection,
                anchor_visual_affinity: before_anchor_visual_affinity,
                visual_affinity: before_visual_affinity,
                state_id: before_state_id,
                host_caret_position: before_block_boundary_exit,
            },
        );
        Ok(Some(Effect {
            document_changed: true,
            selection_changed: true,
            changed_paragraph_id: Some(selection_position.paragraph_id),
            ..Effect::default()
        }))
    }

    /// 在虚拟环境外停靠点输入时，才创建外部段落并把首段文字写入该段。
    pub(super) fn insert_text_at_block_boundary(
        &mut self,
        text: &str,
    ) -> Result<Effect, EditError> {
        if text.is_empty() {
            return Ok(Effect::default());
        }
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let paragraphs = normalized
            .split('\n')
            .map(|line| {
                let mut paragraph = Paragraph::empty(NodeId::generate());
                if !line.is_empty() {
                    paragraph
                        .inlines
                        .push(caret_doc::ParagraphContent::Text(TextFlow {
                            runs: vec![Run {
                                text: line.to_owned(),
                                language: None,
                                props: BTreeMap::new(),
                                extra: BTreeMap::new(),
                            }],
                            extra: BTreeMap::new(),
                        }));
                }
                paragraph
            })
            .collect::<Vec<_>>();
        self.insert_paragraphs_at_block_boundary(paragraphs)
    }

    /// 在虚拟环境外停靠点按 Enter 时，创建外部空段落或含软换行的段落。
    pub(super) fn insert_break_at_block_boundary(
        &mut self,
        soft_break: bool,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        let mut paragraph = Paragraph::empty(NodeId::generate());
        if soft_break {
            paragraph
                .inlines
                .push(caret_doc::ParagraphContent::SoftBreak(SoftBreak::default()));
        }
        self.insert_paragraphs_at_block_boundary(vec![paragraph])
    }

    fn insert_paragraphs_at_block_boundary(
        &mut self,
        mut paragraphs: Vec<Paragraph>,
    ) -> Result<Effect, EditError> {
        let Some(position) = self.host_caret_position.clone() else {
            return Ok(Effect::default());
        };
        if position.kind() != super::super::HostCaretKind::Block {
            return Ok(Effect::default());
        }
        let Some((block_id, direction)) = position
            .as_outside()
            .map(|(host_id, direction)| (host_id.clone(), direction))
        else {
            return Ok(Effect::default());
        };
        let Some((container, block_index)) =
            super::block_navigation::find_block_container_path(&self.document.body, &block_id)
        else {
            return Err(EditError::InvalidPosition);
        };
        let inline_depth = container.environment_depth() + 1;
        for paragraph in &mut paragraphs {
            let text_len = TextMap::new(paragraph)?.text.len();
            self.apply_pending_style_to_paragraph(paragraph, 0, text_len, inline_depth)?;
        }
        let sibling_index = match direction {
            Direction::Backward => block_index,
            Direction::Forward => block_index.saturating_add(1),
        };
        let Some(last_paragraph) = paragraphs.last() else {
            return Ok(Effect::default());
        };
        let final_offset = TextMap::new(last_paragraph)?.text.len();
        let final_paragraph_id = last_paragraph.id.clone();
        let Some(siblings) =
            caret_structure::block_list_by_container_mut(&mut self.document.body, &container)
        else {
            return Err(EditError::InvalidPosition);
        };
        if sibling_index > siblings.len() {
            return Err(EditError::InvalidPosition);
        }
        siblings.splice(
            sibling_index..sibling_index,
            paragraphs.iter().cloned().map(Block::Paragraph),
        );
        if let Err(error) = self.document.validate() {
            if let Some(siblings) =
                caret_structure::block_list_by_container_mut(&mut self.document.body, &container)
            {
                siblings.drain(sibling_index..sibling_index + paragraphs.len());
            }
            return Err(EditError::InvalidDocument(error.to_string()));
        }

        let before_selection = self.selection.clone();
        let before_anchor_visual_affinity = self.selection_anchor_visual_affinity;
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let before_block_boundary_exit = self.host_caret_position.clone();
        let position = Pos {
            story: crate::editing::paragraph_ops::story_for_container(&container),
            paragraph_id: final_paragraph_id.clone(),
            offset: final_offset,
            affinity: Affinity::Upstream,
        };
        self.selection = Selection {
            anchor: position.clone(),
            focus: position,
        };
        self.clear_host_caret_position();
        self.selection_visual_affinity = VisualAffinity::Upstream;
        self.selection_anchor_visual_affinity = VisualAffinity::Upstream;
        let paragraphs_estimated_bytes = paragraphs
            .iter()
            .map(history::paragraph_estimated_bytes)
            .sum();
        self.finalize_structure_edit_with_boundary(
            super::super::paragraph_ops::build_paragraph_index(&self.document.body),
            DocumentPatch::BlockBoundaryParagraphs {
                container,
                sibling_index,
                paragraphs,
                paragraphs_estimated_bytes,
            },
            StructureEditBeforeState {
                inline_range: None,
                selection: before_selection,
                anchor_visual_affinity: before_anchor_visual_affinity,
                visual_affinity: before_visual_affinity,
                state_id: before_state_id,
                host_caret_position: before_block_boundary_exit,
            },
        );
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            changed_paragraph_id: Some(final_paragraph_id),
            ..Effect::default()
        })
    }

    pub(super) fn run_structure(&mut self, command: Command) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        if let Command::MoveStructure(direction) = &command
            && let Some(effect) = self.move_table_cell(*direction)?
        {
            return Ok(effect);
        }
        if let Command::MoveStructure(direction) = &command
            && self.block_boundary_exit().is_some_and(|(id, _)| {
                self.structure_index
                    .get(&id)
                    .is_some_and(|entry| entry.kind == StructureKind::Table)
            })
            && let Some(effect) = self.move_from_block_boundary_stop(*direction)?
        {
            return Ok(effect);
        }
        if matches!(&command, Command::ExitStructure)
            && self.selection.anchor != self.selection.focus
        {
            self.advance_history_merge_epoch();
            self.selection.anchor = self.selection.focus.clone();
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        if matches!(&command, Command::ExpandStructureSelection) {
            return self.expand_structure_selection();
        }
        if matches!(&command, Command::RemoveBlockEnvironmentPreservingContent) {
            return Ok(self
                .unwrap_focused_block_environment(false)?
                .unwrap_or_default());
        }
        if let Command::ExitBlockEnvironment(direction) = &command {
            if self.selection.anchor != self.selection.focus {
                return Ok(Effect::default());
            }
            let ordinal = self
                .structure_index
                .paragraph_ordinal(&self.selection.focus.paragraph_id)
                .ok_or(EditError::InvalidPosition)?;
            let focus = self.selection.focus.clone();
            let focus_offset = focus.offset;
            let text_len = self.text_map_for(&focus)?.text.len();
            let at_text_boundary = match direction {
                Direction::Forward => focus_offset == text_len,
                Direction::Backward => focus_offset == 0,
            };
            if !at_text_boundary {
                return Ok(Effect::default());
            }
            let environment = self
                .structure_index
                .at(&StructurePosition {
                    paragraph_id: focus.paragraph_id.clone(),
                    offset_utf8: focus.offset,
                })
                .into_iter()
                .rfind(|entry| entry.kind.is_block_environment());
            let Some(environment) = environment else {
                return Ok(Effect::default());
            };
            let at_boundary = match direction {
                Direction::Forward => environment.range.paragraph_ordinals.end == ordinal + 1,
                Direction::Backward => environment.range.paragraph_ordinals.start == ordinal,
            };
            if at_boundary {
                let environment_id = environment.id.clone();
                if self
                    .block_boundary_exit()
                    .is_some_and(|(id, _)| id == environment_id)
                    && let Some(effect) = self.move_from_block_boundary_stop(*direction)?
                {
                    return Ok(effect);
                }
                return Ok(self.park_at_block_environment_edge(environment_id, *direction));
            }
            return Ok(Effect::default());
        }
        let (target_id, move_to_end) = match &command {
            Command::MoveStructure(direction) => (
                self.structure_index
                    .adjacent_paragraph(
                        &self.selection.focus.paragraph_id,
                        *direction == Direction::Forward,
                    )
                    .cloned(),
                *direction == Direction::Backward,
            ),
            Command::ExitBlockEnvironment(_) | Command::RemoveBlockEnvironmentPreservingContent => {
                unreachable!("handled above")
            }
            Command::ExitStructure => self.structure_exit_target(),
            _ => (None, false),
        };
        let Some(target_id) = target_id else {
            if matches!(&command, Command::ExitStructure) {
                return self.create_exit_paragraph(Direction::Forward);
            }
            return Ok(Effect::default());
        };
        // 结构索引的全序也包含表格 cell 段落；目标 Story 必须取段落索引的
        // 容器身份，不能一律投影成 Body，否则会提交跨 Story 位置并报
        // position_story_mismatch。
        let target_story = self
            .paragraph_index
            .get(&target_id)
            .map(|location| location.story.clone());
        if target_story
            .as_ref()
            .is_some_and(|story| matches!(story, StoryId::TableCell { .. }))
            && !matches!(self.selection.focus.story, StoryId::TableCell { .. })
        {
            let direction = match &command {
                Command::MoveStructure(direction) => *direction,
                _ => Direction::Forward,
            };
            if self.park_beside_adjacent_atomic_block(direction, false)? {
                return Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                });
            }
            return Ok(Effect::default());
        }
        let story = target_story.unwrap_or_else(|| {
            self.structure_index
                .title_environment_id(&target_id)
                .cloned()
                .map_or(StoryId::Body, StoryId::EnvironmentTitle)
        });
        let mut target = Pos {
            story,
            paragraph_id: target_id,
            offset: 0,
            affinity: Affinity::Downstream,
        };
        let text_map = self.text_map_for(&target)?;
        if move_to_end {
            target.offset = text_map.text.len();
            target.affinity = Affinity::Upstream;
        }
        if target == self.selection.focus && self.selection.anchor == self.selection.focus {
            return Ok(Effect::default());
        }
        self.advance_history_merge_epoch();
        self.selection = Selection {
            anchor: target.clone(),
            focus: target,
        };
        self.clear_host_caret_position();
        self.clear_structure_selection_state();
        self.selection_visual_affinity = if move_to_end {
            caret_layout::VisualAffinity::Upstream
        } else {
            caret_layout::VisualAffinity::Downstream
        };
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    /// 垂直布局候选已到单元格外时，按环境的首末段落边界停靠，不要求字节光标位于行首尾。
    pub(super) fn exit_block_environment_at_vertical_edge(
        &mut self,
        direction: Direction,
    ) -> Result<Option<Effect>, EditError> {
        if self.selection.anchor != self.selection.focus {
            return Ok(None);
        }
        let focus = self.selection.focus.clone();
        let ordinal = self
            .structure_index
            .paragraph_ordinal(&focus.paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        let environment = self
            .structure_index
            .at(&StructurePosition {
                paragraph_id: focus.paragraph_id.clone(),
                offset_utf8: focus.offset,
            })
            .into_iter()
            .rfind(|entry| entry.kind.is_block_environment());
        let Some(environment) = environment else {
            return Ok(None);
        };
        let at_boundary = match direction {
            Direction::Forward => environment.range.paragraph_ordinals.end == ordinal + 1,
            Direction::Backward => environment.range.paragraph_ordinals.start == ordinal,
        };
        let is_title = self
            .paragraph_index
            .get(&focus.paragraph_id)
            .is_some_and(|location| location.is_title);
        if !at_boundary
            || (direction == Direction::Backward
                && !is_title
                && self.block_environment_title(&environment.id).is_some())
            || self.has_adjacent_navigable_block(&focus.paragraph_id, direction)?
        {
            return Ok(None);
        }
        Ok(Some(self.park_at_block_environment_edge(
            environment.id.clone(),
            direction,
        )))
    }

    fn has_adjacent_navigable_block(
        &self,
        paragraph_id: &NodeId,
        direction: Direction,
    ) -> Result<bool, EditError> {
        let location = self
            .paragraph_index
            .get(paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        let Some((container, index)) =
            paragraph_parent_container_path(&self.document.body, location)
        else {
            return Ok(false);
        };
        let blocks = block_list_in_container(&self.document.body, &container)
            .ok_or(EditError::InvalidPosition)?;
        let neighbour = match direction {
            Direction::Backward => index.checked_sub(1),
            Direction::Forward => index.checked_add(1),
        };
        Ok(neighbour
            .is_some_and(|start| self.find_caret_sibling(blocks, start, direction).is_some()))
    }

    fn park_at_block_environment_edge(
        &mut self,
        environment_id: NodeId,
        direction: Direction,
    ) -> Effect {
        self.set_host_caret_position(Some(HostCaretPosition::outside(environment_id, direction)));
        self.selection.focus.affinity = match direction {
            Direction::Backward => Affinity::Upstream,
            Direction::Forward => Affinity::Downstream,
        };
        self.selection.anchor = self.selection.focus.clone();
        self.selection_visual_affinity = match self.selection.focus.affinity {
            Affinity::Upstream => VisualAffinity::Upstream,
            Affinity::Downstream => VisualAffinity::Downstream,
        };
        Effect {
            selection_changed: true,
            ..Effect::default()
        }
    }

    /// 末尾容器没有外部段落时，在容器后创建一个可继续编辑的正文段落。
    fn create_exit_paragraph(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let stack = self.structure_index.at(&StructurePosition {
            paragraph_id: self.selection.focus.paragraph_id.clone(),
            offset_utf8: self.selection.focus.offset,
        });
        let Some(environment_id) = stack
            .iter()
            .rev()
            .find(|entry| entry.kind.is_block_environment())
            .map(|entry| entry.id.clone())
        else {
            return Ok(Effect::default());
        };
        let Some((parent_environment_ids, sibling_index)) =
            environment_parent_location(&self.document.body, &environment_id)
        else {
            return Err(EditError::InvalidPosition);
        };
        let Some(siblings) =
            block_list_by_parent_ids_mut(&mut self.document.body, &parent_environment_ids)
        else {
            return Err(EditError::InvalidPosition);
        };
        let insert_at = match direction {
            Direction::Backward => sibling_index,
            Direction::Forward => sibling_index.saturating_add(1).min(siblings.len()),
        };
        let paragraph = Paragraph::empty(NodeId::generate());
        siblings.insert(insert_at, Block::Paragraph(paragraph.clone()));
        if let Err(error) = self.document.validate() {
            if let Some(siblings) =
                block_list_by_parent_ids_mut(&mut self.document.body, &parent_environment_ids)
            {
                siblings.remove(insert_at);
            }
            return Err(EditError::InvalidDocument(error.to_string()));
        }

        let paragraph_index =
            super::super::paragraph_ops::build_paragraph_index(&self.document.body);
        let Some(state) =
            history::ParagraphState::capture(&self.document, &paragraph_index, &paragraph.id)
        else {
            if let Some(siblings) =
                block_list_by_parent_ids_mut(&mut self.document.body, &parent_environment_ids)
            {
                siblings.remove(insert_at);
            }
            return Err(EditError::InvalidPosition);
        };

        let before_selection = self.selection.clone();
        let before_anchor_visual_affinity = self.selection_anchor_visual_affinity;
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let next_position = Pos {
            story: StoryId::Body,
            paragraph_id: paragraph.id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        self.selection = Selection {
            anchor: next_position.clone(),
            focus: next_position,
        };
        self.selection_visual_affinity = VisualAffinity::Downstream;
        self.selection_anchor_visual_affinity = VisualAffinity::Downstream;
        self.finalize_structure_edit(
            paragraph_index,
            DocumentPatch::Paragraphs {
                before: Vec::new(),
                after: vec![state],
                merge_group: None,
                merge_epoch: self.history_merge_epoch,
            },
            before_selection,
            before_anchor_visual_affinity,
            before_visual_affinity,
            before_state_id,
        );
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            changed_paragraph_id: Some(paragraph.id),
            ..Effect::default()
        })
    }

    /// 将选区逐级扩展到包含当前范围的段落、标题范围或块宿主。
    fn expand_structure_selection(&mut self) -> Result<Effect, EditError> {
        if let Some(selection) = self.structure_selection.as_ref() {
            return self.expand_existing_structure_selection(selection.focus.clone());
        }
        let focus = self.selection.focus.clone();
        let stack = self.structure_index.at(&StructurePosition {
            paragraph_id: focus.paragraph_id.clone(),
            offset_utf8: focus.offset,
        });
        let location = self
            .paragraph_index
            .get(&focus.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(focus.paragraph_id.to_string()))?;
        let ordinal = self
            .structure_index
            .paragraph_ordinal(&focus.paragraph_id)
            .ok_or(EditError::InvalidPosition)?;

        let ranges = std::iter::once(ordinal..ordinal + 1).chain(
            stack
                .iter()
                .rev()
                .filter(|entry| {
                    entry.paragraph_id.is_none()
                        || matches!(entry.kind, StructureKind::Heading { .. })
                })
                .map(|entry| entry.range.paragraph_ordinals.clone()),
        );

        let forward = !compare_positions(
            &self.document.body,
            &self.paragraph_index,
            &self.selection.anchor,
            &self.selection.focus,
        )?
        .is_gt();
        let (current_start, current_end) = if forward {
            (&self.selection.anchor, &self.selection.focus)
        } else {
            (&self.selection.focus, &self.selection.anchor)
        };

        for range in ranges {
            let Some(start) = self.position_at_structure_ordinal(range.start, false) else {
                continue;
            };
            let Some(end) = self.position_at_structure_ordinal(range.end.saturating_sub(1), true)
            else {
                continue;
            };
            if start.story != location.story || end.story != location.story {
                continue;
            }
            let contains_current = !compare_positions(
                &self.document.body,
                &self.paragraph_index,
                &start,
                current_start,
            )?
            .is_gt()
                && !compare_positions(
                    &self.document.body,
                    &self.paragraph_index,
                    &end,
                    current_end,
                )?
                .is_lt();
            let expands_current = start != *current_start || end != *current_end;
            if !contains_current || !expands_current {
                continue;
            }

            self.selection = if forward {
                Selection {
                    anchor: start,
                    focus: end,
                }
            } else {
                Selection {
                    anchor: end,
                    focus: start,
                }
            };
            self.selection_visual_affinity = if forward {
                VisualAffinity::Upstream
            } else {
                VisualAffinity::Downstream
            };
            self.advance_history_merge_epoch();
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        if let Some(cell) = self.focused_table_cell() {
            let (anchor, focus) = block_range(cell.table_id, forward);
            return self.commit_range_between(anchor, focus, None, true);
        }
        Ok(Effect::default())
    }

    fn expand_existing_structure_selection(
        &mut self,
        focus: DocumentRangeEndpoint,
    ) -> Result<Effect, EditError> {
        let (start, end) = self.ordered_structure_selection()?;
        let forward = &focus == end;
        // 焦点借用的段落可能在选区之外，父链须从实际端点所在的块容器解析。
        let location = find_document_node(&self.document, focus.block_container_node_id())
            .ok_or(EditError::InvalidPosition)?;
        let mut container = location.container;
        while let Some(step) = container.last_step() {
            let block_id = match step {
                ContainerStep::Environment(id) => id,
                ContainerStep::TableCell(cell) => &cell.table_id,
            };
            let (before, after) = block_range(block_id.clone(), true);
            let start_order = self.compare_range_endpoints(&before, start)?;
            let end_order = self.compare_range_endpoints(&after, end)?;
            if start_order.is_le()
                && end_order.is_ge()
                && (start_order.is_lt() || end_order.is_gt())
            {
                let (anchor, focus) = if forward {
                    (before, after)
                } else {
                    (after, before)
                };
                return self.commit_range_between(anchor, focus, None, true);
            }
            container.pop();
        }
        Ok(Effect::default())
    }

    fn position_at_structure_ordinal(&self, ordinal: usize, at_end: bool) -> Option<Pos> {
        let paragraph_id = self.structure_index.paragraph_id_at(ordinal)?;
        let location = self.paragraph_index.get(paragraph_id)?;
        let paragraph = paragraph_at_location(&self.document.body, location)?;
        let text_length = super::super::text_map::TextMap::new(paragraph)
            .ok()?
            .text
            .len();
        Some(Pos {
            story: location.story.clone(),
            paragraph_id: paragraph_id.clone(),
            offset: if at_end { text_length } else { 0 },
            affinity: if at_end {
                Affinity::Upstream
            } else {
                Affinity::Downstream
            },
        })
    }

    fn structure_exit_target(&self) -> (Option<caret_doc::NodeId>, bool) {
        let stack = self.structure_index.at(&StructurePosition {
            paragraph_id: self.selection.focus.paragraph_id.clone(),
            offset_utf8: self.selection.focus.offset,
        });
        let Some(container) = stack
            .iter()
            .rev()
            .find(|entry| entry.kind.is_block_environment())
        else {
            return (None, false);
        };
        let end = container.range.paragraph_ordinals.end;
        if let Some(paragraph_id) = self.structure_index.paragraph_id_at(end) {
            return (Some(paragraph_id.clone()), false);
        }
        let previous = container.range.paragraph_ordinals.start.checked_sub(1);
        (
            previous
                .and_then(|ordinal| self.structure_index.paragraph_id_at(ordinal))
                .cloned(),
            previous.is_some(),
        )
    }
}

fn block_list_by_parent_ids<'a>(
    blocks: &'a [Block],
    environment_ids: &[NodeId],
) -> Option<&'a [Block]> {
    caret_structure::block_list_by_environment_ids(blocks, environment_ids)
}

fn environment_has_content(environment: &BlockEnvironment) -> Result<bool, EditError> {
    if let Some(title) = &environment.title
        && !super::super::text_map::TextMap::new(title)?.text.is_empty()
    {
        return Ok(true);
    }
    for child in &environment.children {
        match child {
            Block::Paragraph(paragraph)
                if !super::super::text_map::TextMap::new(paragraph)?
                    .text
                    .is_empty() =>
            {
                return Ok(true);
            }
            Block::Environment(child) if environment_has_content(child)? => return Ok(true),
            Block::Equation(_)
            | Block::Align(_)
            | Block::Table(_)
            | Block::ExplicitBreak(_)
            | Block::Opaque(_) => {
                return Ok(true);
            }
            Block::Paragraph(_) | Block::Environment(_) => {}
        }
    }
    Ok(false)
}

pub(super) fn environment_has_content_at(
    blocks: &[Block],
    environment_id: &NodeId,
) -> Result<bool, EditError> {
    let block = super::block_navigation::find_block_by_id(blocks, environment_id)
        .ok_or(EditError::InvalidPosition)?;
    match block {
        Block::Environment(environment) => environment_has_content(environment),
        _ => Ok(true),
    }
}

fn environment_parent_location(
    blocks: &[Block],
    target_id: &NodeId,
) -> Option<(Vec<NodeId>, usize)> {
    let location = caret_structure::find_block_node(blocks, target_id)?;
    (location.kind == caret_structure::DocumentNodeKind::Environment)
        .then_some((location.parent_environment_ids, location.sibling_index?))
}

fn block_list_by_parent_ids_mut<'a>(
    blocks: &'a mut Vec<Block>,
    environment_ids: &[NodeId],
) -> Option<&'a mut Vec<Block>> {
    caret_structure::block_list_by_environment_ids_mut(blocks, environment_ids)
}

#[cfg(test)]
#[path = "structure/tests.rs"]
mod tests;
