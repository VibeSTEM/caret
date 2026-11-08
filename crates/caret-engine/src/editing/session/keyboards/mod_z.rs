//! 管理 Mod+Z 与 Mod+Shift+Z 在公式草稿和文档历史间的撤销重做语义。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, EditError, Effect};

impl Session {
    /// 先由数学编辑分支处理本地源码草稿历史，再回退到文档历史。
    pub(super) fn handle_mod_z(&mut self, redo: bool) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        self.run(if redo { Command::Redo } else { Command::Undo })
    }
}
