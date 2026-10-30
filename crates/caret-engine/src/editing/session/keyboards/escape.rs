//! 按唯一优先级裁决编辑器 Escape，避免多个交互层重复消费按键。

use super::super::Session;
use crate::editing::{Command, EditError, Effect};

impl Session {
    /// 按命令输入、输入法组字、选区和结构的顺序处理 Escape。
    pub(super) fn handle_escape(&mut self) -> Result<Effect, EditError> {
        if self.command_line.is_some() {
            let had_preedit = self
                .command_line
                .as_ref()
                .is_some_and(|command_line| !command_line.preedit.is_empty());
            let mut effect = self.run_command_line(Command::CancelCommandLine)?;
            effect.composition_changed = had_preedit;
            return Ok(effect);
        }
        if let Some(composition) = &self.composition {
            return self.cancel_composition(composition.id);
        }
        if self.selected_block_environment.take().is_some() {
            self.selected_block_environments.clear();
            self.structure_selection = None;
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        if self.selection.anchor != self.selection.focus {
            self.advance_history_merge_epoch();
            self.selection.anchor = self.selection.focus.clone();
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        self.run_structure(Command::ExitStructure)
    }
}
