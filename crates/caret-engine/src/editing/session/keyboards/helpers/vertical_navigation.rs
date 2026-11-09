//! 统一上下键的状态仲裁并提交布局候选；按键模块提供方向和候选。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::Session;
use crate::editing::{
    Command, Direction, EditError, Effect, MathEditBranch, NavigationCandidate,
    NavigationGeneration, NavigationIntent, Selection,
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
        if candidate.is_none() {
            return self.complete_vertical_document_edge(direction, extend);
        }
        self.commit_document_vertical_navigation_candidate(candidate, extend)
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
                None => self.exit_vertical_math_block(direction, extend),
            },
        }
    }

    fn exit_vertical_math_block(
        &mut self,
        direction: Direction,
        extend: bool,
    ) -> Result<Effect, EditError> {
        if extend
            || !self
                .math_edit()
                .is_some_and(|edit| matches!(&edit.target, crate::MathEditTarget::Block { .. }))
        {
            return Ok(Effect::default());
        }
        self.exit_math_edit_for_navigation(direction)
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
