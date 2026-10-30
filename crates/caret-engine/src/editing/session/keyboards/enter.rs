//! 按当前编辑上下文处理 Enter 与 Shift+Enter。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, EditError, Effect};

impl Session {
    /// 命令输入期间普通 Enter 接受候选，Shift+Enter 不误改正文；正文复用标准编辑命令。
    pub(super) fn handle_enter(&mut self, soft_break: bool) -> Result<Effect, EditError> {
        if self.command_line.is_some() {
            return if soft_break {
                Ok(Effect::default())
            } else {
                self.run_command_line(Command::AcceptCommandLine)
            };
        }
        self.run(if soft_break {
            Command::InsertSoftBreak
        } else {
            Command::InsertParagraphBreak
        })
    }
}
