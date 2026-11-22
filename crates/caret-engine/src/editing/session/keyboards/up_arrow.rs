//! 管理 Up 及 Shift+Up 在输入、命令行、公式结构和正文中的状态优先级。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Direction, EditError, Effect, NavigationCandidate};

impl Session {
    pub(super) fn handle_up_arrow(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
        structure: bool,
    ) -> Result<Effect, EditError> {
        self.handle_vertical_arrow(candidate, extend, structure, Direction::Backward)
    }
}

impl Session {
    /// Alt+Up：在单元格上方插入行；修饰差异归本按键。
    pub(super) fn handle_up_arrow_table_insert_row(&mut self) -> Result<Effect, EditError> {
        if !super::helpers::table::table_cell_keyboard_available(self) {
            return Ok(Effect::default());
        }
        self.insert_table_row(Direction::Backward)
    }
}
