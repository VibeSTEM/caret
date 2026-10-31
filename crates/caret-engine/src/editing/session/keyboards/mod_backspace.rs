//! 按平台抽象修饰键与退格键移除当前行内或块环境并保留内容。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, EditError, Effect};

impl Session {
    /// 优先解除光标所在的行内环境，否则解除当前块环境并保留其内容。
    pub(super) fn handle_mod_backspace(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return self.run(Command::RemoveBlockEnvironmentPreservingContent);
        }
        if self.selection.anchor == self.selection.focus
            && let Some(environment) = self.current_inline_environment()?
        {
            return self.run(Command::RemoveInlineEnvironmentPreservingContent(
                environment.environment_id,
            ));
        }
        if self.selection.anchor == self.selection.focus
            && self.current_inline_underline_range()?.is_some()
        {
            return self.run(Command::RemoveRunUnderlinePreservingContent);
        }
        self.run(Command::RemoveBlockEnvironmentPreservingContent)
    }
}
