//! 表格结构与 Tab/Shift+Tab 键的上下文仲裁。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

impl Session {
    /// 命令输入、组合输入或公式编辑优先于表格键；只有普通文档焦点才执行表格结构操作。
    pub(super) fn handle_table_key(&mut self, key: EditorKey) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() || self.math_edit.is_some() {
            return Ok(Effect::default());
        }
        match key {
            EditorKey::ShiftTab => {
                if let Some(effect) = self.table_tab_backward()? {
                    Ok(effect)
                } else {
                    Ok(Effect::default())
                }
            }
            EditorKey::InsertTable => self.insert_table(),
            EditorKey::TableInsertRow(direction) => self.insert_table_row(direction),
            EditorKey::TableDeleteRow => self.delete_table_row(),
            EditorKey::TableInsertColumn(direction) => self.insert_table_column(direction),
            EditorKey::TableDeleteColumn => self.delete_table_column(),
            _ => Ok(Effect::default()),
        }
    }
}
