//! 管理 Down 及 Shift+Down 的上下文分派和导航候选提交。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{
    Command, Direction, EditError, Effect, NavigationCandidate, NavigationIntent,
};

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
        self.route_navigation_key(
            NavigationIntent::Vertical,
            Some(Direction::Forward),
            candidate,
            extend,
        )
    }
}
