//! 按平台抽象修饰键与退格键移除当前行内或块环境并保留内容。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, EditError, Effect, EnvironmentSource};

impl Session {
    /// 按环境能力解除光标所在最内层可解除环境：行内环境优先，公式等原子环境跳过。
    pub(super) fn handle_mod_backspace(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return self.run(Command::RemoveBlockEnvironmentPreservingContent);
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
