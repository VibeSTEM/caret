//! 表格结构键的共享动作入口；上下文优先级由各 EditorKey 对应 handler 在调用前判定。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::Session;

/// 表格 cell 内的行列/删除键可用：输入上下文干净且焦点确在单元格内。
pub(in crate::editing::session::keyboards) fn table_cell_keyboard_available(
    session: &Session,
) -> bool {
    table_keyboard_context_available(session) && session.table_context().is_some()
}

pub(in crate::editing::session::keyboards) fn table_keyboard_context_available(
    session: &Session,
) -> bool {
    session.composition.is_none() && session.command_line.is_none() && session.math_edit.is_none()
}
