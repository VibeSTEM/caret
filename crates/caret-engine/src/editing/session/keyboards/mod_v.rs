//! 管理 Mod+V 的会话焦点语义，并请求 app 适配层读取系统剪贴板。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{ClipboardAction, EditError, Effect};

impl Session {
    /// 仅在正文焦点接受系统粘贴，避免命令行或公式焦点被外层快捷键抢占。
    pub(super) fn handle_mod_v(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        Ok(Effect {
            clipboard_action: Some(ClipboardAction::Paste),
            ..Effect::default()
        })
    }
}
