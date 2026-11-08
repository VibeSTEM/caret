//! 按当前编辑上下文处理 Enter 与 Shift+Enter。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, Direction, EditError, Effect, MathEditTarget};

impl Session {
    /// 命令输入期间普通 Enter 接受候选，Shift+Enter 不误改正文；正文复用标准编辑命令。
    pub(super) fn handle_enter(&mut self, soft_break: bool) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if self.command_line.is_some() {
            return if soft_break {
                Ok(Effect::default())
            } else {
                self.run_command_line(Command::AcceptCommandLine)
            };
        }
        if let Some(edit) = self.math_edit.as_ref() {
            if soft_break {
                return Ok(Effect::default());
            }
            if matches!(edit.target, MathEditTarget::Block { align: true, .. })
                && !self.math_edit_is_text_branch()
            {
                return if self.math_edit_is_tree_branch() {
                    self.math_tree_align_break(true)
                } else {
                    self.math_insert_raw(
                        super::super::math_edit::ALIGN_ROW_BREAK,
                        super::super::math_edit::ALIGN_ROW_BREAK.len(),
                    )
                };
            }
            if self.math_edit_is_text_branch() {
                return Err(EditError::UnsupportedNewline);
            }
            return self.exit_math_edit(Direction::Forward, true);
        }
        self.run(if soft_break {
            Command::InsertSoftBreak
        } else {
            Command::InsertParagraphBreak
        })
    }
}
