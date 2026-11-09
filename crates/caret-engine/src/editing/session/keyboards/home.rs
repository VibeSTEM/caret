//! 管理 Home 及 Shift+Home 的上下文分派和视觉行首候选提交。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{EditError, Effect, NavigationCandidate, NavigationIntent};

impl Session {
    pub(super) fn handle_home(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Ok(Effect::default());
        }
        self.ensure_math_edit_revision_current()?;
        if self.command_line.is_some() {
            return Ok(Effect::default());
        }
        if self.math_edit.is_some() {
            return self.commit_math_visual_line_edge_candidate(candidate, extend);
        }
        match candidate {
            Some(NavigationCandidate::Document {
                focus,
                visual_affinity,
                generation,
            }) => self.commit_document_navigation_candidate(
                focus,
                visual_affinity,
                generation,
                NavigationIntent::VisualLineEdge,
                None,
                extend,
            ),
            Some(_) => Err(EditError::InvalidPosition),
            None => Ok(Effect::default()),
        }
    }
}
