//! 将带上下文优先级的编辑器按键意图分派到独立行为模块。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod backslash;
mod backspace;
mod delete;
mod dollar;
mod down_arrow;
mod end;
mod enter;
mod escape;
mod helpers;
mod home;
mod left_arrow;
mod mod_backspace;
mod mod_c;
mod mod_space;
mod mod_v;
mod mod_z;
mod right_arrow;
mod tab;
mod up_arrow;

use super::Session;
use crate::editing::{EditError, EditorKey, Effect};

impl Session {
    /// 编辑器键的应用入口；需要字体候选的键仍由对应 handler 决定何时取字体目录。
    pub fn run_editor_key_with_font_choices_lazy(
        &mut self,
        key: EditorKey,
        font_choices: impl FnOnce() -> (Vec<String>, Vec<(String, caret_doc::DocumentFontTarget)>),
    ) -> Result<Effect, EditError> {
        self.run_session_operation(
            crate::editing::Command::EditorKey(key),
            move |session, command| {
                let crate::editing::Command::EditorKey(key) = command else {
                    unreachable!("the keyboard adapter always wraps an editor key")
                };
                session.run_editor_key_with_font_choices_inner(key, font_choices)
            },
        )
    }

    pub(super) fn run_editor_key_with_font_choices_inner(
        &mut self,
        key: EditorKey,
        font_choices: impl FnOnce() -> (Vec<String>, Vec<(String, caret_doc::DocumentFontTarget)>),
    ) -> Result<Effect, EditError> {
        match key {
            EditorKey::Enter => self.handle_enter_with_font_choices(false, font_choices),
            EditorKey::ShiftEnter => self.handle_enter_with_font_choices(true, font_choices),
            EditorKey::Tab => self.handle_tab_with_font_choices(font_choices),
            _ => self.run_editor_key(key),
        }
    }

    /// 在单一入口根据按键类型分派；每种按键的状态优先级独立维护。
    pub(super) fn run_editor_key(&mut self, key: EditorKey) -> Result<Effect, EditError> {
        match key {
            EditorKey::Escape => self.handle_escape(),
            EditorKey::Enter => self.handle_enter(false),
            EditorKey::Tab => self.handle_tab(),
            EditorKey::ShiftEnter => self.handle_enter(true),
            EditorKey::Backspace => self.handle_backspace(),
            EditorKey::Delete => self.handle_delete(),
            EditorKey::Backslash => self.handle_backslash(),
            EditorKey::Dollar => self.handle_dollar(),
            EditorKey::ModC => self.handle_mod_c(),
            EditorKey::ModV => self.handle_mod_v(),
            EditorKey::ModZ { redo } => self.handle_mod_z(redo),
            EditorKey::ModSpace => self.handle_mod_space(),
            EditorKey::ModBackspace => self.handle_mod_backspace(),
            EditorKey::LeftArrow { candidate, extend } => self.handle_left_arrow(candidate, extend),
            EditorKey::RightArrow { candidate, extend } => {
                self.handle_right_arrow(candidate, extend)
            }
            EditorKey::UpArrow {
                candidate,
                extend,
                structure,
            } => self.handle_up_arrow(candidate, extend, structure),
            EditorKey::DownArrow {
                candidate,
                extend,
                structure,
            } => self.handle_down_arrow(candidate, extend, structure),
            EditorKey::Home { candidate, extend } => self.handle_home(candidate, extend),
            EditorKey::End { candidate, extend } => self.handle_end(candidate, extend),
        }
    }
}

#[cfg(test)]
mod tests;
