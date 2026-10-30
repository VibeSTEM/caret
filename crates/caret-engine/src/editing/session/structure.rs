//! 执行 Rust 结构索引上的段落停靠导航与跳出容器。

use super::super::text_map::TextMap;
use super::super::{
    Affinity, Command, Direction, EditError, Effect, Pos, PositionMap, Selection, StoryId,
};
use super::Session;
use super::history::{self, DocumentPatch, SessionHistoryEdit, SessionHistoryState};
use crate::editing::paragraph_ops::{ParagraphLocation, compare_positions, paragraph_at_location};
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, InlineNode, NodeId, Paragraph, Run, SoftBreak,
};
use caret_layout::VisualAffinity;
use caret_structure::{StructureIndex, StructureKind, StructurePosition};
use std::{collections::BTreeMap, sync::Arc};

#[path = "structure/environment_mutations.rs"]
mod environment_mutations;

#[path = "structure/selection.rs"]
mod selection;

impl Session {
    /// 统一提交结构事务的索引、历史与文档代际更新。
    pub(super) fn finalize_structure_edit(
        &mut self,
        updated_paragraph_index: super::super::paragraph_ops::ParagraphIndex,
        patch: DocumentPatch,
        before_selection: Selection,
        before_visual_affinity: VisualAffinity,
        before_state_id: u64,
    ) {
        self.paragraph_index = updated_paragraph_index;
        self.structure_index = Arc::new(StructureIndex::build(&self.document));
        self.text_map_cache.clear();
        self.advance_history_merge_epoch();

        let after_state_id = self.allocate_history_state_id();
        self.record_history_edit(SessionHistoryEdit::new(
            patch,
            SessionHistoryState::new(
                before_selection,
                before_visual_affinity,
                self.pending_inline_style.clone(),
                before_state_id,
            ),
            SessionHistoryState::new(
                self.selection.clone(),
                self.selection_visual_affinity,
                self.pending_inline_style.clone(),
                after_state_id,
            ),
        ));
        self.history_state_id = after_state_id;
        self.advance_document_generation();
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
            history::parent_environment_path(&self.document.body, &location)
        else {
            return Ok(None);
        };
        let Some(environment_index) = sibling_index.checked_sub(1) else {
            return Ok(None);
        };
        let Some(siblings) =
            block_list_by_environment_ids(&self.document.body, &parent_environment_ids)
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
            block_list_by_environment_ids_mut(&mut self.document.body, &parent_environment_ids)
        else {
            return Err(EditError::InvalidPosition);
        };
        siblings.remove(environment_index);

        let before_selection = self.selection.clone();
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
            before_visual_affinity,
            before_state_id,
        );
        Ok(Some(Effect {
            document_changed: true,
            ..Effect::default()
        }))
    }

    /// 在虚拟环境外停靠点输入时，才创建外部段落并把首段文字写入该段。
    pub(super) fn insert_text_at_environment_exit(
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
                    paragraph.inlines.push(InlineNode::Run(Run {
                        text: line.to_owned(),
                        language: None,
                        props: BTreeMap::new(),
                        extra: BTreeMap::new(),
                    }));
                }
                paragraph
            })
            .collect::<Vec<_>>();
        self.insert_paragraphs_at_environment_exit(paragraphs)
    }

    /// 在虚拟环境外停靠点按 Enter 时，创建外部空段落或含软换行的段落。
    pub(super) fn insert_break_at_environment_exit(
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
                .push(InlineNode::SoftBreak(SoftBreak::default()));
        }
        self.insert_paragraphs_at_environment_exit(vec![paragraph])
    }

    fn insert_paragraphs_at_environment_exit(
        &mut self,
        mut paragraphs: Vec<Paragraph>,
    ) -> Result<Effect, EditError> {
        let Some((environment_id, direction)) = self.block_environment_exit.clone() else {
            return Ok(Effect::default());
        };
        let Some((parent_environment_ids, environment_index)) =
            environment_parent_location(&self.document.body, &environment_id)
        else {
            self.block_environment_exit = None;
            return Ok(Effect::default());
        };
        let inline_depth = parent_environment_ids.len() + 1;
        for paragraph in &mut paragraphs {
            let text_len = TextMap::new(paragraph)?.text.len();
            self.apply_pending_style_to_paragraph(paragraph, 0, text_len, inline_depth)?;
        }
        let sibling_index = match direction {
            Direction::Backward => environment_index,
            Direction::Forward => environment_index.saturating_add(1),
        };
        let Some(last_paragraph) = paragraphs.last() else {
            return Ok(Effect::default());
        };
        let final_offset = TextMap::new(last_paragraph)?.text.len();
        let final_paragraph_id = last_paragraph.id.clone();
        let Some(siblings) =
            block_list_by_environment_ids_mut(&mut self.document.body, &parent_environment_ids)
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
                block_list_by_environment_ids_mut(&mut self.document.body, &parent_environment_ids)
            {
                siblings.drain(sibling_index..sibling_index + paragraphs.len());
            }
            return Err(EditError::InvalidDocument(error.to_string()));
        }

        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let position = Pos {
            story: StoryId::Body,
            paragraph_id: final_paragraph_id.clone(),
            offset: final_offset,
            affinity: Affinity::Upstream,
        };
        self.selection = Selection {
            anchor: position.clone(),
            focus: position,
        };
        self.block_environment_exit = None;
        self.selection_visual_affinity = VisualAffinity::Upstream;
        let paragraphs_estimated_bytes = paragraphs
            .iter()
            .map(history::paragraph_estimated_bytes)
            .sum();
        self.finalize_structure_edit(
            super::super::paragraph_ops::build_paragraph_index(&self.document.body),
            DocumentPatch::EnvironmentBoundaryParagraphs {
                parent_environment_ids,
                sibling_index,
                paragraphs,
                paragraphs_estimated_bytes,
            },
            before_selection,
            before_visual_affinity,
            before_state_id,
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
                if let Some(adjacent_id) = self.structure_index.adjacent_paragraph(
                    &self.selection.focus.paragraph_id,
                    *direction == Direction::Forward,
                ) {
                    let is_repeating_exit = self.block_environment_exit.as_ref().is_some_and(
                        |(active_id, active_direction)| {
                            active_id == &environment.id && active_direction == direction
                        },
                    );
                    if is_repeating_exit {
                        let story = self
                            .structure_index
                            .title_environment_id(adjacent_id)
                            .cloned()
                            .map_or(StoryId::Body, StoryId::EnvironmentTitle);
                        let mut next = Pos {
                            story,
                            paragraph_id: adjacent_id.clone(),
                            offset: 0,
                            affinity: Affinity::Downstream,
                        };
                        if *direction == Direction::Backward {
                            next.offset = self.text_map_for(&next)?.text.len();
                            next.affinity = Affinity::Upstream;
                        }
                        self.selection = Selection {
                            anchor: next.clone(),
                            focus: next,
                        };
                        self.selection_visual_affinity = match self.selection.focus.affinity {
                            Affinity::Upstream => VisualAffinity::Upstream,
                            Affinity::Downstream => VisualAffinity::Downstream,
                        };
                        self.block_environment_exit = None;
                        return Ok(Effect {
                            selection_changed: true,
                            ..Effect::default()
                        });
                    }
                }
                if let Some((active_environment_id, active_direction)) =
                    self.block_environment_exit.clone()
                    && active_environment_id == environment.id
                {
                    if active_direction == *direction {
                        return Ok(Effect::default());
                    }
                    self.block_environment_exit = None;
                    self.selection.focus.affinity = match direction {
                        Direction::Forward => Affinity::Downstream,
                        Direction::Backward => Affinity::Upstream,
                    };
                    self.selection.anchor = self.selection.focus.clone();
                    self.selection_visual_affinity = match self.selection.focus.affinity {
                        Affinity::Upstream => VisualAffinity::Upstream,
                        Affinity::Downstream => VisualAffinity::Downstream,
                    };
                    return Ok(Effect {
                        selection_changed: true,
                        ..Effect::default()
                    });
                }
                self.block_environment_exit = Some((environment.id.clone(), *direction));
                self.selection.focus.affinity = match direction {
                    Direction::Backward => Affinity::Upstream,
                    Direction::Forward => Affinity::Downstream,
                };
                self.selection.anchor = self.selection.focus.clone();
                self.selection_visual_affinity = match self.selection.focus.affinity {
                    Affinity::Upstream => VisualAffinity::Upstream,
                    Affinity::Downstream => VisualAffinity::Downstream,
                };
                return Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                });
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
        let story = self
            .structure_index
            .title_environment_id(&target_id)
            .cloned()
            .map_or(StoryId::Body, StoryId::EnvironmentTitle);
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
        self.block_environment_exit = None;
        self.selected_block_environment = None;
        self.selected_block_environments.clear();
        self.structure_selection = None;
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
            block_list_by_environment_ids_mut(&mut self.document.body, &parent_environment_ids)
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
                block_list_by_environment_ids_mut(&mut self.document.body, &parent_environment_ids)
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
                block_list_by_environment_ids_mut(&mut self.document.body, &parent_environment_ids)
            {
                siblings.remove(insert_at);
            }
            return Err(EditError::InvalidPosition);
        };

        let before_selection = self.selection.clone();
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
        self.finalize_structure_edit(
            paragraph_index,
            DocumentPatch::Paragraphs {
                before: Vec::new(),
                after: vec![state],
                merge_group: None,
                merge_epoch: self.history_merge_epoch,
            },
            before_selection,
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

    /// 将选区逐级扩展到当前段落、标题范围或外层块环境。
    fn expand_structure_selection(&mut self) -> Result<Effect, EditError> {
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

        let (current_start, current_end) = if compare_positions(
            &self.paragraph_index,
            &self.selection.anchor,
            &self.selection.focus,
        )?
        .is_gt()
        {
            (&self.selection.focus, &self.selection.anchor)
        } else {
            (&self.selection.anchor, &self.selection.focus)
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
            let contains_current =
                !compare_positions(&self.paragraph_index, &start, current_start)?.is_gt()
                    && !compare_positions(&self.paragraph_index, &end, current_end)?.is_lt();
            let expands_current = start != *current_start || end != *current_end;
            if !contains_current || !expands_current {
                continue;
            }

            let forward = !compare_positions(
                &self.paragraph_index,
                &self.selection.anchor,
                &self.selection.focus,
            )?
            .is_gt();
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

fn block_list_by_environment_ids<'a>(
    blocks: &'a [Block],
    environment_ids: &[NodeId],
) -> Option<&'a [Block]> {
    let Some((environment_id, rest)) = environment_ids.split_first() else {
        return Some(blocks);
    };
    let environment = blocks.iter().find_map(|block| match block {
        Block::Environment(environment) if &environment.id == environment_id => Some(environment),
        _ => None,
    })?;
    block_list_by_environment_ids(&environment.children, rest)
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
            Block::ExplicitBreak(_) | Block::Opaque(_) => return Ok(true),
            _ => {}
        }
    }
    Ok(false)
}

pub(super) fn environment_has_content_at(
    blocks: &[Block],
    environment_id: &NodeId,
) -> Result<bool, EditError> {
    let (parent_ids, sibling_index) =
        environment_parent_location(blocks, environment_id).ok_or(EditError::InvalidPosition)?;
    let siblings =
        block_list_by_environment_ids(blocks, &parent_ids).ok_or(EditError::InvalidPosition)?;
    let Some(Block::Environment(environment)) = siblings.get(sibling_index) else {
        return Err(EditError::InvalidPosition);
    };
    environment_has_content(environment)
}

fn environment_parent_location(
    blocks: &[Block],
    target_id: &NodeId,
) -> Option<(Vec<NodeId>, usize)> {
    fn find(
        blocks: &[Block],
        target_id: &NodeId,
        parent_ids: &mut Vec<NodeId>,
    ) -> Option<(Vec<NodeId>, usize)> {
        for (index, block) in blocks.iter().enumerate() {
            let Block::Environment(environment) = block else {
                continue;
            };
            if &environment.id == target_id {
                return Some((parent_ids.clone(), index));
            }
            parent_ids.push(environment.id.clone());
            let result = find(&environment.children, target_id, parent_ids);
            parent_ids.pop();
            if result.is_some() {
                return result;
            }
        }
        None
    }

    find(blocks, target_id, &mut Vec::new())
}

fn block_list_by_environment_ids_mut<'a>(
    blocks: &'a mut Vec<Block>,
    environment_ids: &[NodeId],
) -> Option<&'a mut Vec<Block>> {
    let Some((environment_id, remaining_ids)) = environment_ids.split_first() else {
        return Some(blocks);
    };
    let environment = blocks.iter_mut().find_map(|block| match block {
        Block::Environment(environment) if &environment.id == environment_id => Some(environment),
        _ => None,
    })?;
    block_list_by_environment_ids_mut(&mut environment.children, remaining_ids)
}

fn replace_block_at_location(
    blocks: &mut Vec<Block>,
    location: &ParagraphLocation,
    replacement: Block,
) -> bool {
    fn visit(blocks: &mut Vec<Block>, path: &[usize], replacement: Block) -> bool {
        let Some((&index, parent_path)) = path.split_last() else {
            return false;
        };
        let siblings = if parent_path.is_empty() {
            blocks
        } else {
            let Some(Block::Environment(parent)) = block_mut_at_path(blocks, parent_path) else {
                return false;
            };
            &mut parent.children
        };
        let Some(_) = siblings.get(index) else {
            return false;
        };
        siblings[index] = replacement;
        true
    }

    visit(blocks, &location.block_path, replacement)
}

fn block_mut_at_path<'a>(blocks: &'a mut [Block], path: &[usize]) -> Option<&'a mut Block> {
    let (&index, parent_path) = path.split_last()?;
    if parent_path.is_empty() {
        return blocks.get_mut(index);
    }
    let parent = block_mut_at_path(blocks, parent_path)?;
    let Block::Environment(environment) = parent else {
        return None;
    };
    environment.children.get_mut(index)
}

#[cfg(test)]
#[path = "structure/tests.rs"]
mod tests;
