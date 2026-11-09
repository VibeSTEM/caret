//! 共享拥有逻辑行模型的块级退格仲裁策略。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

/// 逻辑行首退格的共享决策；具体块适配器负责判空、删除事务和焦点映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BackspaceAction {
    /// 删除整行，适用于所有 cell 都为空且元数据可安全移除的行。
    RemoveEmptyRow,
    /// 保持非空行及其焦点不变，避免推测跨 cell 的合并方式。
    IgnoreNonEmptyRowStart,
    /// 将行内退格交给当前 cell 的编辑器。
    DelegateToCell,
}

/// 以整行内容和焦点边界统一分类多行块的 Backspace 意图。
pub(super) fn backspace_action(row_is_empty: bool, at_row_start: bool) -> BackspaceAction {
    if row_is_empty {
        BackspaceAction::RemoveEmptyRow
    } else if at_row_start {
        BackspaceAction::IgnoreNonEmptyRowStart
    } else {
        BackspaceAction::DelegateToCell
    }
}
