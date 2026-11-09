//! 管理 Mod+Z 与 Mod+Shift+Z 在公式草稿和文档历史间的撤销重做语义。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{EditError, Effect};

impl Session {
    /// 先由数学编辑分支处理本地源码草稿历史，再回退到文档历史。
    pub(super) fn handle_mod_z(&mut self, redo: bool) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if self.command_line.is_some() {
            return Ok(Effect::default());
        }
        self.handle_history_command(redo)
    }

    /// 共享 Mod+Z、工具栏和直接历史命令的领域恢复；实体按键仍先决定焦点优先级。
    pub(in crate::editing::session) fn handle_history_command(
        &mut self,
        redo: bool,
    ) -> Result<Effect, EditError> {
        if self.command_line.is_none()
            && let Some(edit) = &mut self.math_edit
        {
            let changed = if redo {
                edit.redo_source_draft()?
            } else {
                edit.undo_source_draft()?
            };
            if changed {
                return Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                });
            }
            if edit.source_draft_is_pending() {
                return Err(EditError::InvalidMath(
                    "incomplete math source draft has no local undo step".to_owned(),
                ));
            }
            edit.source_draft = None;
        }
        let effect = self.restore_history(redo)?;
        self.command_line = None;
        Ok(effect)
    }
}
