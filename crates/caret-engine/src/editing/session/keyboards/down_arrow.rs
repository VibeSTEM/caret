//! 管理 Down 及 Shift+Down 在输入、命令行、公式结构和正文中的状态优先级。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, Direction, EditError, Effect, NavigationCandidate};

impl Session {
    pub(super) fn handle_down_arrow(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
        structure: bool,
    ) -> Result<Effect, EditError> {
        if structure {
            if self.composition.is_some() || self.command_line.is_some() || self.math_edit.is_some()
            {
                return Ok(Effect::default());
            }
            return self.run(Command::MoveStructure(Direction::Forward));
        }
        if self.composition.is_some() {
            return Ok(Effect::default());
        }
        self.ensure_math_edit_revision_current()?;
        if self.command_line.is_some() {
            return self.run(Command::MoveCommandLineSelection(Direction::Forward));
        }
        if self.math_edit.is_some() {
            return self.commit_math_vertical_navigation_candidate(candidate, extend);
        }
        self.commit_document_vertical_navigation_candidate(candidate, extend)
    }
}
