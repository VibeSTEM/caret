//! 统一上下键的状态仲裁并提交布局候选；按键模块提供方向和候选。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::Session;
use crate::editing::{
    Command, Direction, DocumentRangeEndpoint, EditError, Effect, MathEditBranch,
    NavigationCandidate, NavigationGeneration, NavigationIntent, Selection,
};

impl Session {
    /// 共用 Up/Down 的状态路由；按键模块只提供方向和候选。
    pub(in crate::editing::session::keyboards) fn handle_vertical_arrow(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
        structure: bool,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        if structure {
            if self.composition.is_some() || self.command_line.is_some() || self.math_edit.is_some()
            {
                return Ok(Effect::default());
            }
            if self.table_context().is_some() {
                // 表格内纵向结构键只按行移动；到边界且相邻块不可停靠时保持 no-op，
                // 不回退到会被横向表格拦截复用的 MoveStructure。
                return Ok(self
                    .move_table_row(direction)?
                    .unwrap_or_else(Effect::default));
            }
            return self.run(Command::MoveStructure(direction));
        }
        if self.composition.is_some() {
            return Ok(Effect::default());
        }
        self.ensure_math_edit_revision_current()?;
        if self.command_line.is_some() {
            return self.run(Command::MoveCommandLineSelection(direction));
        }
        if self.math_edit.is_some() {
            return self.commit_math_vertical_navigation_candidate(candidate, extend, direction);
        }
        if !extend
            && !self.has_structure_selection()
            && candidate.is_none()
            && let Some(effect) = self.table_arrow_step(
                super::super::super::table::TableArrowAxis::Vertical,
                direction,
            )?
        {
            return Ok(effect);
        }
        if candidate.is_none() {
            return self.complete_vertical_document_edge(direction, extend);
        }
        self.commit_document_vertical_navigation_candidate(candidate, extend, direction)
    }

    /// 提交当前数学焦点下的上下键候选。
    pub(in crate::editing::session::keyboards) fn commit_math_vertical_navigation_candidate(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        let edit = self.math_edit.as_ref().ok_or(EditError::InvalidPosition)?;
        match &edit.branch {
            MathEditBranch::Text { .. } => match candidate {
                Some(NavigationCandidate::MathText {
                    focus,
                    visual_affinity,
                    generation,
                }) => {
                    self.validate_navigation_generation(&generation)?;
                    let context = self
                        .math_text_input_context()
                        .ok_or(EditError::InvalidPosition)?;
                    let (anchor, anchor_visual_affinity) = if extend {
                        (context.anchor, context.anchor_visual_affinity)
                    } else {
                        (focus.clone(), visual_affinity)
                    };
                    self.set_math_text_selection_positions_with_visual_affinities(
                        anchor,
                        focus,
                        anchor_visual_affinity,
                        visual_affinity,
                    )
                }
                Some(NavigationCandidate::MathTextHit { hit, generation }) => {
                    self.validate_navigation_generation(&generation)?;
                    self.validate_math_text_hit(&hit, &generation)?;
                    if extend {
                        return Ok(Effect::default());
                    }
                    self.focus_math_text_hit(&hit, generation.font_generation)
                }
                Some(NavigationCandidate::MathTreeHit { hit, generation }) => {
                    self.validate_navigation_generation(&generation)?;
                    self.validate_math_tree_hit(&hit, &generation)?;
                    if extend {
                        return Ok(Effect::default());
                    }
                    self.focus_math_structure_hit(&hit, generation.font_generation)
                }
                None => self.exit_vertical_math_block(direction, extend),
                Some(
                    NavigationCandidate::BlockEdge { .. }
                    | NavigationCandidate::Document { .. }
                    | NavigationCandidate::InlineMathRangeEdge { .. },
                ) => Err(EditError::InvalidPosition),
            },
            MathEditBranch::Tree { .. } | MathEditBranch::Source => match candidate {
                Some(NavigationCandidate::Document { generation, .. }) => {
                    self.validate_navigation_generation(&generation)?;
                    Ok(Effect::default())
                }
                Some(
                    NavigationCandidate::BlockEdge { .. }
                    | NavigationCandidate::MathText { .. }
                    | NavigationCandidate::InlineMathRangeEdge { .. },
                ) => Err(EditError::InvalidPosition),
                Some(NavigationCandidate::MathTextHit { hit, generation }) => {
                    self.validate_navigation_generation(&generation)?;
                    self.validate_math_text_hit(&hit, &generation)?;
                    if extend {
                        return Ok(Effect::default());
                    }
                    self.focus_math_text_hit(&hit, generation.font_generation)
                }
                Some(NavigationCandidate::MathTreeHit { hit, generation }) => {
                    self.validate_navigation_generation(&generation)?;
                    self.validate_math_tree_hit(&hit, &generation)?;
                    if extend {
                        return Ok(Effect::default());
                    }
                    self.focus_math_structure_hit(&hit, generation.font_generation)
                }
                None => self.exit_vertical_math_block(direction, extend),
            },
        }
    }

    /// 公式内部没有垂直候选时按宿主边界退出。
    ///
    /// 块级公式保留外侧停靠语义，但扩选仍需要布局候选，因此 Block + Shift 保持
    /// no-op。InlineMath 没有块外侧停靠：垂直边缘先按局部边界退到 U+FFFC 前/后侧；
    /// 扩选也先退出，由 app 在同一次按键内继续文档纵向导航，避免静默吞键。
    fn exit_vertical_math_block(
        &mut self,
        direction: Direction,
        extend: bool,
    ) -> Result<Effect, EditError> {
        let Some(edit) = self.math_edit() else {
            return Ok(Effect::default());
        };
        if extend && matches!(&edit.target, crate::MathEditTarget::Block { .. }) {
            return Ok(Effect::default());
        }
        self.exit_math_edit_for_navigation(direction)
    }

    /// 提交普通文档布局生成的上下导航候选。
    pub(in crate::editing::session::keyboards) fn commit_document_vertical_navigation_candidate(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        if self.math_edit.is_some() {
            return Err(EditError::InvalidPosition);
        }
        let from_block = self.block_boundary_exit().is_some()
            || self
                .structure_selection_range()
                .is_some_and(|(_, focus)| matches!(focus, DocumentRangeEndpoint::BlockEdge { .. }));
        match candidate {
            Some(NavigationCandidate::BlockEdge {
                block_id,
                side,
                generation,
            }) => {
                self.validate_navigation_generation(&generation)?;
                self.commit_range_focus(
                    DocumentRangeEndpoint::BlockEdge { block_id, side },
                    None,
                    extend,
                )
            }
            Some(NavigationCandidate::MathTextHit { hit, generation }) if from_block => {
                self.validate_navigation_generation(&generation)?;
                self.validate_math_text_hit(&hit, &generation)?;
                if extend {
                    return self.extend_vertical_math_target(&hit.target, direction);
                }
                self.focus_math_text_hit(&hit, generation.font_generation)
            }
            Some(NavigationCandidate::MathTreeHit { hit, generation }) if from_block => {
                self.validate_navigation_generation(&generation)?;
                self.validate_math_tree_hit(&hit, &generation)?;
                if extend {
                    return self.extend_vertical_math_target(&hit.target, direction);
                }
                self.focus_math_structure_hit(&hit, generation.font_generation)
            }
            Some(NavigationCandidate::Document {
                focus,
                visual_affinity,
                generation,
            }) => {
                self.validate_navigation_generation(&generation)?;
                if extend
                    && let Some((anchor, _)) = self.structure_selection_range()
                    && anchor.is_inline_math()
                {
                    return self.install_inline_math_selection(
                        crate::StructureSelection {
                            anchor: anchor.clone(),
                            focus: DocumentRangeEndpoint::Text(focus),
                        },
                        self.selection_anchor_visual_affinity(),
                        visual_affinity,
                    );
                }
                if extend
                    && (from_block
                        || self.structure_selection_range().is_some_and(|(anchor, _)| {
                            matches!(anchor, DocumentRangeEndpoint::BlockEdge { .. })
                        }))
                {
                    return self.commit_range_focus(
                        DocumentRangeEndpoint::Text(focus),
                        Some(visual_affinity),
                        true,
                    );
                }
                let anchor = if extend {
                    self.structure_selection_text_anchor()
                        .unwrap_or_else(|| self.selection.anchor.clone())
                } else {
                    focus.clone()
                };
                self.set_navigation_selection_with_direction(
                    Selection { anchor, focus },
                    visual_affinity,
                    NavigationIntent::Vertical,
                    Some(direction),
                )
            }
            Some(
                NavigationCandidate::MathText { .. }
                | NavigationCandidate::InlineMathRangeEdge { .. }
                | NavigationCandidate::MathTextHit { .. }
                | NavigationCandidate::MathTreeHit { .. },
            ) => Err(EditError::InvalidPosition),
            None => Ok(Effect::default()),
        }
    }

    /// 文档扩选触及显示公式内容时由 Session 按现有范围协议纳入该宿主。
    fn extend_vertical_math_target(
        &mut self,
        target: &crate::MathEditTarget,
        side: Direction,
    ) -> Result<Effect, EditError> {
        let crate::MathEditTarget::Block { block_id, .. } = target else {
            return Err(EditError::InvalidPosition);
        };
        self.commit_range_focus(
            DocumentRangeEndpoint::BlockEdge {
                block_id: block_id.clone(),
                side,
            },
            None,
            true,
        )
    }

    fn validate_math_tree_hit(
        &self,
        hit: &crate::PageMathStructureHit,
        generation: &NavigationGeneration,
    ) -> Result<(), EditError> {
        if self
            .math_edit
            .as_ref()
            .is_some_and(|current| current.target != hit.target)
            || hit.session_id.as_ref() != Some(&generation.session_id)
            || hit.document_generation != generation.document_generation
            || hit.font_generation != generation.font_generation
        {
            return Err(EditError::InvalidPosition);
        }
        Ok(())
    }

    fn validate_math_text_hit(
        &self,
        hit: &crate::PageMathTextHit,
        generation: &NavigationGeneration,
    ) -> Result<(), EditError> {
        if self
            .math_edit
            .as_ref()
            .is_some_and(|current| current.target != hit.target)
            || hit.session_id.as_ref() != Some(&generation.session_id)
            || hit.document_generation != generation.document_generation
            || hit.font_generation != generation.font_generation
        {
            return Err(EditError::InvalidPosition);
        }
        Ok(())
    }
}
