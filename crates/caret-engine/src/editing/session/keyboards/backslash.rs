//! 管理反斜线在命令输入和公式焦点中的命令前缀语义。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, EditError, Effect};

impl Session {
    /// 在正文启动命令输入；已有命令输入时将反斜线作为查询文本。
    pub(super) fn handle_backslash(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if self.command_line.is_some() {
            return self.run(Command::TypeCommandLine("\\".to_owned()));
        }
        self.run(Command::BeginCommandLine)
    }
}
