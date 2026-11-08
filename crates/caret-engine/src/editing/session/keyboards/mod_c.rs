//! 管理 Mod+C 的会话焦点语义，并请求 app 适配层写入系统剪贴板。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{ClipboardAction, EditError, Effect};

impl Session {
    /// 仅在正文拥有焦点且存在文本或结构选区时请求复制。
    pub(super) fn handle_mod_c(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        if self.math_edit.is_some() {
            let Some(context) = self.math_text_input_context() else {
                return Ok(Effect::default());
            };
            if context.anchor_utf16 == context.focus_utf16 {
                return Ok(Effect::default());
            }
            return Ok(Effect {
                clipboard_action: Some(ClipboardAction::Copy),
                ..Effect::default()
            });
        }
        if self.selection.anchor == self.selection.focus
            && self.selected_block_environments.is_empty()
            && self.structure_selection.is_none()
        {
            return Ok(Effect::default());
        }
        Ok(Effect {
            clipboard_action: Some(ClipboardAction::Copy),
            ..Effect::default()
        })
    }
}
