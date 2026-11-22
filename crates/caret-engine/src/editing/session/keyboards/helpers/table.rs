//! 表格结构键的共享动作入口；上下文优先级由各 EditorKey 对应 handler 在调用前判定。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::Session;

pub(in crate::editing::session::keyboards) fn table_keyboard_context_available(
    session: &Session,
) -> bool {
    session.composition.is_none() && session.command_line.is_none() && session.math_edit.is_none()
}
