//! 按平台抽象修饰键与退格键移除当前行内或块环境并保留内容。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, EditError, Effect, EnvironmentSource};

impl Session {
    /// 优先处理 IME/命令行和数学焦点，再解除光标所在最内层可解除环境。
    pub(super) fn handle_mod_backspace(&mut self) -> Result<Effect, EditError> {
        // 结构选区由退格/删除作为整体处理，不得把保留的文字光标解释为解除其他块环境。
        if !self.selected_block_environments.is_empty() {
            return Ok(Effect::default());
        }
        if self.composition.is_some() {
            return self.run(Command::RemoveBlockEnvironmentPreservingContent);
        }
        if self.command_line.is_some() {
            if self.math_edit().is_some() {
                return Ok(Effect::default());
            }
            return self.run(Command::RemoveBlockEnvironmentPreservingContent);
        }
        if let Some(edit) = self.math_edit() {
            if edit.source_draft_is_pending() {
                return Err(EditError::InvalidMath(
                    "resolve or cancel the incomplete math source draft before leaving".to_owned(),
                ));
            }
            return self.math_mod_backspace();
        }
        if self.selection.anchor == self.selection.focus
            && let Some(environment) = self
                .current_environment_view()?
                .innermost_unwrappable_inline()
        {
            return match environment.source {
                EnvironmentSource::InlineEnvironment => {
                    let Some(id) = environment.id.clone() else {
                        return Ok(Effect::default());
                    };
                    self.run(Command::RemoveInlineEnvironmentPreservingContent(id))
                }
                EnvironmentSource::RunProperty => self.run(
                    Command::RemoveRunPropertyPreservingContent(environment.kind),
                ),
                EnvironmentSource::InlineMath
                | EnvironmentSource::BlockEnvironment
                | EnvironmentSource::MathBlock => Ok(Effect::default()),
            };
        }
        self.run(Command::RemoveBlockEnvironmentPreservingContent)
    }
}
