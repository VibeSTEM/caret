//! 管理 Left 及 Shift+Left 的上下文分派和导航候选提交。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{
    Command, Direction, EditError, Effect, MathEditBranch, NavigationCandidate, NavigationIntent,
};

impl Session {
    pub(super) fn handle_left_arrow(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Ok(Effect::default());
        }
        self.ensure_math_edit_revision_current()?;
        if self.command_line.is_some() {
            return self.run(Command::MoveCommandLineCursor(Direction::Backward));
        }
        if let Some(edit) = self.math_edit.as_ref() {
            return match &edit.branch {
                MathEditBranch::Text { .. } => match candidate {
                    Some(NavigationCandidate::MathText {
                        focus,
                        visual_affinity,
                        generation,
                    }) => self.commit_math_text_navigation_candidate(
                        focus,
                        visual_affinity,
                        generation,
                        extend,
                    ),
                    None => {
                        let context = self
                            .math_text_input_context()
                            .ok_or(EditError::InvalidPosition)?;
                        if context.focus_utf16 != 0 {
                            return Ok(Effect::default());
                        }
                        if !extend {
                            self.run(Command::MoveCaret(Direction::Backward))
                        } else if self.math_text_at_host_edge(Direction::Backward)? {
                            self.convert_math_host_to_structure_range(Direction::Backward)
                        } else {
                            // Align 中间 cell 的文本岛边缘：沿用本地移动语义，不伪造整块范围。
                            self.run(Command::MoveCaret(Direction::Backward))
                        }
                    }
                    Some(_) => Err(EditError::InvalidPosition),
                },
                MathEditBranch::Tree { .. } | MathEditBranch::Source => {
                    if let Some(candidate) = candidate {
                        match candidate {
                            NavigationCandidate::Document { generation, .. } => {
                                self.validate_navigation_generation(&generation)?;
                            }
                            NavigationCandidate::MathText { .. }
                            | NavigationCandidate::MathTextHit { .. }
                            | NavigationCandidate::MathTreeHit { .. } => {
                                return Err(EditError::InvalidPosition);
                            }
                        }
                    }
                    match &edit.branch {
                        MathEditBranch::Tree { .. } if extend => {
                            self.extend_math_tree_selection(Direction::Backward)
                        }
                        MathEditBranch::Tree { .. } => {
                            self.run(Command::MoveCaret(Direction::Backward))
                        }
                        // 源码分支没有本地选区；扩选沿用非扩选的光标移动（已知限制）。
                        MathEditBranch::Source => self.run(Command::MoveCaret(Direction::Backward)),
                        // 文本分支已由外层处理；FFI 路径不制造 panic，改为可恢复错误。
                        MathEditBranch::Text { .. } => Err(EditError::InvalidPosition),
                    }
                }
            };
        }
        if !extend
            && self.empty_inline_boundary_candidate_matches(candidate.as_ref())?
            && let Some(effect) = self.move_empty_inline_host_boundary(Direction::Backward)?
        {
            return Ok(effect);
        }
        if !extend
            && candidate.is_none()
            && matches!(
                self.host_caret_position.as_ref(),
                Some(
                    crate::editing::HostCaretPosition::Before {
                        kind: crate::editing::HostCaretKind::Inline,
                        ..
                    } | crate::editing::HostCaretPosition::After {
                        kind: crate::editing::HostCaretKind::Inline,
                        ..
                    }
                )
            )
        {
            return self.run(Command::MoveCaret(Direction::Backward));
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
                NavigationIntent::Horizontal,
                Some(Direction::Backward),
                extend,
            ),
            Some(
                NavigationCandidate::MathText { .. }
                | NavigationCandidate::MathTextHit { .. }
                | NavigationCandidate::MathTreeHit { .. },
            ) => Err(EditError::InvalidPosition),
            None => self.complete_horizontal_document_edge(Direction::Backward, extend),
        }
    }
}
