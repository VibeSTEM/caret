//! 管理 End 及 Shift+End 的上下文分派和视觉行尾候选提交。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Direction, EditError, Effect, NavigationCandidate, NavigationIntent};

impl Session {
    pub(super) fn handle_end(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
    ) -> Result<Effect, EditError> {
        self.route_navigation_key(
            NavigationIntent::VisualLineEdge,
            Some(Direction::Forward),
            candidate,
            extend,
        )
    }
}
