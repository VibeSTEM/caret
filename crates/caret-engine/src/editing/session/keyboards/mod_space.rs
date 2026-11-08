//! 管理 Mod+Space 逐级扩大选区的编辑焦点优先级。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, EditError, Effect};

impl Session {
    /// 仅在正文焦点执行结构选区扩展。
    pub(super) fn handle_mod_space(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() || self.math_edit.is_some() {
            return Ok(Effect::default());
        }
        self.run(Command::ExpandStructureSelection)
    }
}
