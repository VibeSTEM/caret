//! 执行 Rust 结构索引上的段落停靠导航与跳出容器。

use super::super::{
    Affinity, Command, Direction, EditError, Effect, Pos, PositionMap, Selection, StoryId,
};
use super::Session;
use super::history::{self, DocumentPatch, SessionHistoryEdit};
use crate::editing::paragraph_ops::{ParagraphLocation, paragraph_at_location};
use caret_doc::{Block, BlockEnvironment, BlockEnvironmentKind, NodeId};
use caret_layout::VisualAffinity;
use caret_structure::{StructureIndex, StructureKind, StructurePosition};
use std::{collections::BTreeMap, sync::Arc};

impl Session {
    /// 将当前段落按容器语义原子转换为 Section 或可命名的 Custom 环境。
    pub(super) fn create_block_environment(
        &mut self,
        kind: BlockEnvironmentKind,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        if self.selection.anchor != self.selection.focus {
            return Err(EditError::InvalidStructureSelection);
        }
        let position = self.selection.focus.clone();
        if position.story != StoryId::Body {
            return Err(EditError::WrongStory);
        }
        let location = self
            .paragraph_index
            .get(&position.paragraph_id)
            .cloned()
            .ok_or_else(|| EditError::UnknownParagraph(position.paragraph_id.to_string()))?;
        let paragraph = paragraph_at_location(&self.document.body, &location)
            .ok_or_else(|| EditError::UnknownParagraph(position.paragraph_id.to_string()))?
            .clone();
        let (parent_environment_ids, sibling_index) =
            history::parent_environment_path(&self.document.body, &location)
                .ok_or(EditError::InvalidPosition)?;
        let environment_id = NodeId::generate();
        let environment = BlockEnvironment {
            id: environment_id.clone(),
            kind,
            definition_id: None,
            props: BTreeMap::new(),
            title: (kind == BlockEnvironmentKind::Section).then(|| paragraph.clone()),
            children: if kind == BlockEnvironmentKind::Custom {
                vec![Block::Paragraph(paragraph.clone())]
            } else {
                Vec::new()
            },
            extra: BTreeMap::new(),
        };
        let replacement = Block::Environment(environment.clone());
        if !replace_block_at_location(&mut self.document.body, &location, replacement.clone()) {
            return Err(EditError::UnknownParagraph(
                position.paragraph_id.to_string(),
            ));
        }
        if let Err(error) = self.document.validate() {
            replace_block_at_location(
                &mut self.document.body,
                &location,
                Block::Paragraph(paragraph.clone()),
            );
            return Err(EditError::InvalidDocument(error.to_string()));
        }

        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let position_map =
            (kind == BlockEnvironmentKind::Section).then(|| PositionMap::StoryChange {
                paragraph_id: position.paragraph_id.clone(),
                from: StoryId::Body,
                to: StoryId::EnvironmentTitle(environment_id.clone()),
            });
        let next_position = if kind == BlockEnvironmentKind::Section {
            Pos {
                story: StoryId::EnvironmentTitle(environment_id.clone()),
                ..position
            }
        } else {
            position
        };
        self.selection = Selection {
            anchor: next_position.clone(),
            focus: next_position,
        };
        let selection_changed = before_selection != self.selection;
        self.selection_visual_affinity = match self.selection.focus.affinity {
            Affinity::Upstream => VisualAffinity::Upstream,
            Affinity::Downstream => VisualAffinity::Downstream,
        };
        self.paragraph_index =
            super::super::paragraph_ops::build_paragraph_index(&self.document.body);
        self.structure_index = Arc::new(StructureIndex::build(&self.document));
        self.text_map_cache.clear();
        self.advance_history_merge_epoch();

        let paragraph_estimated_bytes = history::paragraph_estimated_bytes(&paragraph);
        let patch = DocumentPatch::EnvironmentCreation {
            parent_environment_ids,
            sibling_index,
            paragraph: Box::new(paragraph),
            paragraph_estimated_bytes,
            environment_id,
            kind,
        };
        let after_state_id = self.allocate_history_state_id();
        let edit = SessionHistoryEdit::new(
            patch,
            before_selection,
            self.selection.clone(),
            before_visual_affinity,
            self.selection_visual_affinity,
            before_state_id,
            after_state_id,
        );
        self.record_history_edit(edit);
        self.history_state_id = after_state_id;
        if let Some(next) = self.document_generation.checked_add(1) {
            self.document_generation = next;
        } else {
            self.session_id = NodeId::generate();
            self.document_generation = 0;
        }
        Ok(Effect {
            document_changed: true,
            selection_changed,
            position_map,
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
            Command::ExitStructure => self.structure_exit_target(),
            _ => (None, false),
        };
        let Some(target_id) = target_id else {
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

    fn structure_exit_target(&self) -> (Option<caret_doc::NodeId>, bool) {
        let stack = self.structure_index.at(&StructurePosition {
            paragraph_id: self.selection.focus.paragraph_id.clone(),
            offset_utf8: self.selection.focus.offset,
        });
        let Some(container) = stack
            .iter()
            .rev()
            .find(|entry| matches!(entry.kind, StructureKind::Section | StructureKind::Custom))
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
