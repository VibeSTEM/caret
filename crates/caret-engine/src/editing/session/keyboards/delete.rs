//! 管理 Delete 在不同编辑焦点下的前向删除语义。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, Direction, EditError, Effect};

impl Session {
    /// 命令输入或输入法拥有焦点时不删除正文内容；公式编辑复用领域删除命令。
    pub(super) fn handle_delete(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        self.run(Command::Delete(Direction::Forward))
    }
}
