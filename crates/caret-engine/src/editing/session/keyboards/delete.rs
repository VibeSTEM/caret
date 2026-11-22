//! 管理 Delete 在不同编辑焦点下的前向删除语义。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, Direction, EditError, Effect};

impl Session {
    /// 组合输入或命令行占有焦点时不删除正文；其他焦点交给共享领域删除事务。
    pub(super) fn handle_delete(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        self.run(Command::Delete(Direction::Forward))
    }
}

impl Session {
    pub(super) fn handle_delete_table_delete_row(&mut self) -> Result<Effect, EditError> {
        if !super::helpers::table::table_cell_keyboard_available(self) {
            return Ok(Effect::default());
        }
        self.delete_table_row()
    }

    pub(super) fn handle_delete_table_delete_column(&mut self) -> Result<Effect, EditError> {
        if !super::helpers::table::table_cell_keyboard_available(self) {
            return Ok(Effect::default());
        }
        self.delete_table_column()
    }
}
