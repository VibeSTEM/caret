//! 将带上下文优先级的编辑器按键意图分派到独立行为模块。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod enter;
mod escape;
mod mod_backspace;
mod tab;

use super::Session;
use crate::editing::{EditError, EditorKey, Effect};

impl Session {
    /// 在单一入口根据按键类型分派；每种按键的状态优先级独立维护。
    pub(super) fn run_editor_key(&mut self, key: EditorKey) -> Result<Effect, EditError> {
        match key {
            EditorKey::Escape => self.handle_escape(),
            EditorKey::Enter => self.handle_enter(false),
            EditorKey::Tab => self.handle_tab(),
            EditorKey::ShiftEnter => self.handle_enter(true),
            EditorKey::ModBackspace => self.handle_mod_backspace(),
        }
    }
}
