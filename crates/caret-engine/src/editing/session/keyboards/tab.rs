//! 根据焦点将 Tab 映射为公式占位符导航、候选补全或排版空格。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, EditError, Effect};

impl Session {
    /// 公式焦点由 Session 优先处理；命令输入期间交给命令面板补全，正文中插入两个全字宽空格。
    pub(super) fn handle_tab(&mut self) -> Result<Effect, EditError> {
        if self.command_line.is_some() {
            return self.run_command_line(Command::TabCommandLine);
        }
        self.run(Command::InsertTwoCharacterWidthSpace)
    }
}
