//! 管理 Backspace 在命令行、公式、输入法和正文状态中的优先级。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, Direction, EditError, Effect};

impl Session {
    /// 删除当前焦点拥有者的前一个输入单元。
    pub(super) fn handle_backspace(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Ok(Effect::default());
        }
        if self.command_line.is_some() {
            return self.run(Command::BackspaceCommandLine);
        }
        self.run(Command::Delete(Direction::Backward))
    }
}
