//! 管理 Down 及 Shift+Down 在输入、命令行、公式结构和正文中的状态优先级。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Direction, EditError, Effect, NavigationCandidate};

impl Session {
    pub(super) fn handle_down_arrow(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
        structure: bool,
    ) -> Result<Effect, EditError> {
        self.handle_vertical_arrow(candidate, extend, structure, Direction::Forward)
    }
}

impl Session {
    /// Alt+Down：在单元格下方插入行。
    pub(super) fn handle_down_arrow_table_insert_row(&mut self) -> Result<Effect, EditError> {
        if !super::helpers::table::table_keyboard_context_available(self) {
            return Ok(Effect::default());
        }
        self.insert_table_row(Direction::Forward)
    }
}
