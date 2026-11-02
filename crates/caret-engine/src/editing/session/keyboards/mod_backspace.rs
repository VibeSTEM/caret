//! 按平台抽象修饰键与退格键移除当前行内或块环境并保留内容。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, EditError, Effect, EnvironmentSource};

impl Session {
    /// 优先处理 IME/命令行和数学焦点，再解除光标所在最内层可解除环境。
    pub(super) fn handle_mod_backspace(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return self.run(Command::RemoveBlockEnvironmentPreservingContent);
        }
        if self.command_line.is_some() {
            if self.math_edit().is_some() {
                return Ok(Effect::default());
            }
            return self.run(Command::RemoveBlockEnvironmentPreservingContent);
        }
        if self.math_edit().is_some() {
            return self.handle_math_mod_backspace();
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
