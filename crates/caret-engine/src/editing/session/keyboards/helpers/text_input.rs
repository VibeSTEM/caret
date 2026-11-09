//! 在共享输入入口处理命令查询、前缀键和普通文本的目标选择。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::Session;
use crate::editing::{Command, EditError, EditorKey, Effect};

impl Session {
    /// 根据当前焦点把宿主文本送往命令输入、前缀 handler 或普通插入。
    pub(in crate::editing::session) fn run_text_input(
        &mut self,
        text: String,
    ) -> Result<Effect, EditError> {
        self.run_text_input_with_font_choices_inner(text, || (Vec::new(), Vec::new()))
    }

    fn run_text_input_with_font_choices_inner(
        &mut self,
        text: String,
        font_choices: impl FnOnce() -> (Vec<String>, Vec<(String, caret_doc::DocumentFontTarget)>),
    ) -> Result<Effect, EditError> {
        if self.command_line.is_some() {
            return self.run_command_line_with_font_choices_lazy(
                Command::TypeCommandLine(text),
                font_choices,
            );
        }
        if text == "\\" {
            return self.run_editor_key_with_font_choices_inner(EditorKey::Backslash, font_choices);
        }
        if text == "$" && self.math_edit.is_none() {
            return self.run_editor_key_with_font_choices_inner(EditorKey::Dollar, font_choices);
        }
        self.run(Command::InsertText(text))
    }

    /// 应用文本输入适配入口；命令查询字体候选仍由其 handler 选择性加载。
    pub fn run_text_input_with_font_choices_lazy(
        &mut self,
        text: String,
        font_choices: impl FnOnce() -> (Vec<String>, Vec<(String, caret_doc::DocumentFontTarget)>),
    ) -> Result<Effect, EditError> {
        self.run_session_operation(Command::TextInput(text), move |session, command| {
            let Command::TextInput(text) = command else {
                unreachable!("the text adapter always wraps text input")
            };
            session.run_text_input_with_font_choices_inner(text, font_choices)
        })
    }
}
