//! 将 Tab 映射为候选补全或两个字宽的排版空格命令。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, EditError, Effect};

impl Session {
    /// 命令输入期间交给命令面板补全；正文中插入两个全字宽空格。
    pub(super) fn handle_tab(&mut self) -> Result<Effect, EditError> {
        self.handle_tab_with_font_choices(|| (Vec::new(), Vec::new()))
    }

    pub(super) fn handle_tab_with_font_choices(
        &mut self,
        font_choices: impl FnOnce() -> (Vec<String>, Vec<(String, caret_doc::DocumentFontTarget)>),
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if self.command_line.is_some() {
            return self
                .run_command_line_with_font_choices_lazy(Command::TabCommandLine, font_choices);
        }
        if self.math_edit.is_some() {
            return Ok(Effect::default());
        }
        self.run(Command::InsertTwoCharacterWidthSpace)
    }
}
