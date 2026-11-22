//! 管理 Alt+T 插入表格；与 076/080 的逐键 handler 结构一致。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{EditError, Effect};

impl Session {
    pub(super) fn handle_insert_table(&mut self) -> Result<Effect, EditError> {
        if !super::helpers::table::table_keyboard_context_available(self)
            || !self.can_insert_table()
        {
            return Ok(Effect::default());
        }
        self.insert_table()
    }
}
