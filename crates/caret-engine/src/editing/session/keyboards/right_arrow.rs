//! 管理 Right 及 Shift+Right 的上下文分派和导航候选提交。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Direction, EditError, Effect, NavigationCandidate, NavigationIntent};

impl Session {
    pub(super) fn handle_right_arrow(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
    ) -> Result<Effect, EditError> {
        self.route_navigation_key(
            NavigationIntent::Horizontal,
            Some(Direction::Forward),
            candidate,
            extend,
        )
    }
}
