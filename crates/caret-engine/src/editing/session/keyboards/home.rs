//! 管理 Home 及 Shift+Home 的上下文分派和视觉行首候选提交。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Direction, EditError, Effect, NavigationCandidate, NavigationIntent};

impl Session {
    pub(super) fn handle_home(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
    ) -> Result<Effect, EditError> {
        self.route_navigation_key(
            NavigationIntent::VisualLineEdge,
            Some(Direction::Backward),
            candidate,
            extend,
        )
    }
}
