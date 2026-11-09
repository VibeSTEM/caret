//! 管理 Backspace 在命令行、公式、输入法和正文状态中的优先级。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, Direction, EditError, Effect};

impl Session {
    /// 先尊重组合输入与命令行焦点，再委托共享领域删除事务处理结构、公式或正文。
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
