//! 管理 `$` 在正文进入行内公式及其他编辑焦点中的语义。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, EditError, Effect};

impl Session {
    /// 正文中的 `$` 创建并聚焦空行内公式；公式或命令输入中按其本地输入语义处理。
    pub(super) fn handle_dollar(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if self.command_line.is_some() {
            return self.run(Command::TypeCommandLine("$".to_owned()));
        }
        if self.math_edit.is_some() {
            return self
                .run_math_edit(&Command::InsertText("$".to_owned()))?
                .ok_or(EditError::InvalidPosition);
        }
        self.begin_inline_math()
    }
}
