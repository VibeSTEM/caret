//! 提交上下键使用的布局候选；按键状态优先级由 `up_arrow.rs` 和 `down_arrow.rs` 管理。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::Session;
use crate::editing::{
    EditError, Effect, MathEditBranch, NavigationCandidate, NavigationGeneration, NavigationIntent,
    Selection,
};

impl Session {
    /// 提交当前数学焦点下的上下键候选。
    pub(in crate::editing::session::keyboards) fn commit_math_vertical_navigation_candidate(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
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
                None => Ok(Effect::default()),
                Some(NavigationCandidate::Document { .. }) => Err(EditError::InvalidPosition),
            },
            MathEditBranch::Tree { .. } | MathEditBranch::Source => match candidate {
                Some(NavigationCandidate::Document { generation, .. }) => {
                    self.validate_navigation_generation(&generation)?;
                    Ok(Effect::default())
                }
                Some(NavigationCandidate::MathText { .. }) => Err(EditError::InvalidPosition),
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
                None => Ok(Effect::default()),
            },
        }
    }

    /// 提交普通文档布局生成的上下导航候选。
    pub(in crate::editing::session::keyboards) fn commit_document_vertical_navigation_candidate(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
    ) -> Result<Effect, EditError> {
        if self.math_edit.is_some() {
            return Err(EditError::InvalidPosition);
        }
        match candidate {
            Some(NavigationCandidate::Document {
                focus,
                visual_affinity,
                generation,
            }) => {
                self.validate_navigation_generation(&generation)?;
                let anchor = if extend {
                    self.structure_selection
                        .as_ref()
                        .map(|selection| selection.anchor.clone())
                        .unwrap_or_else(|| self.selection.anchor.clone())
                } else {
                    focus.clone()
                };
                self.set_navigation_selection_with_direction(
                    Selection { anchor, focus },
                    visual_affinity,
                    NavigationIntent::Vertical,
                    None,
                )
            }
            Some(
                NavigationCandidate::MathText { .. }
                | NavigationCandidate::MathTextHit { .. }
                | NavigationCandidate::MathTreeHit { .. },
            ) => Err(EditError::InvalidPosition),
            None => Ok(Effect::default()),
        }
    }

    fn validate_math_tree_hit(
        &self,
        hit: &crate::PageMathStructureHit,
        generation: &NavigationGeneration,
    ) -> Result<(), EditError> {
        let Some(current) = &self.math_edit else {
            return Err(EditError::InvalidPosition);
        };
        if current.target != hit.target
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
        let Some(current) = &self.math_edit else {
            return Err(EditError::InvalidPosition);
        };
        if current.target != hit.target
            || hit.session_id.as_ref() != Some(&generation.session_id)
            || hit.document_generation != generation.document_generation
            || hit.font_generation != generation.font_generation
        {
            return Err(EditError::InvalidPosition);
        }
        Ok(())
    }
}
