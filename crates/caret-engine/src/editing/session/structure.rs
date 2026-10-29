//! 执行 Rust 结构索引上的段落停靠导航与跳出容器。

use super::super::{Affinity, Command, Direction, EditError, Effect, Pos, Selection, StoryId};
use super::Session;
use caret_structure::{StructureKind, StructurePosition};

impl Session {
    pub(super) fn run_structure(&mut self, command: Command) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        let target_id = match &command {
            Command::MoveStructure(direction) => self
                .structure_index
                .adjacent_paragraph(
                    &self.selection.focus.paragraph_id,
                    *direction == Direction::Forward,
                )
                .cloned(),
            Command::ExitStructure => self.structure_exit_target(),
            _ => None,
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
        if matches!(&command, Command::MoveStructure(Direction::Backward)) {
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
        self.selection_visual_affinity = caret_layout::VisualAffinity::Downstream;
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    fn structure_exit_target(&self) -> Option<caret_doc::NodeId> {
        let stack = self.structure_index.at(&StructurePosition {
            paragraph_id: self.selection.focus.paragraph_id.clone(),
            offset_utf8: self.selection.focus.offset,
        });
        let container = stack
            .iter()
            .rev()
            .find(|entry| matches!(entry.kind, StructureKind::Section | StructureKind::Showcase))?;
        self.structure_index
            .paragraph_id_at(container.range.paragraph_ordinals.end)
            .cloned()
    }
}

#[cfg(test)]
#[path = "structure/tests.rs"]
mod tests;
