//! 提交导航 handler 已判定目标的候选，不选择按键状态优先级。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::Session;
use crate::editing::{
    Command, Direction, EditError, Effect, MathEditBranch, MathTextPosition, NavigationCandidate,
    NavigationGeneration, NavigationIntent, Pos, Selection,
};
use caret_layout::VisualAffinity;

impl Session {
    /// 验证候选所属的 Session 和文档代际；字体代际由 app 布局服务先行校验。
    pub(in crate::editing::session::keyboards) fn validate_navigation_generation(
        &self,
        generation: &NavigationGeneration,
    ) -> Result<(), EditError> {
        if &generation.session_id != self.session_id()
            || generation.document_generation != self.document_generation()
        {
            return Err(EditError::InvalidPosition);
        }
        Ok(())
    }

    /// 提交正文候选；锚点根据调用方已选择的扩展状态构造。
    pub(in crate::editing::session::keyboards) fn commit_document_navigation_candidate(
        &mut self,
        focus: Pos,
        visual_affinity: VisualAffinity,
        generation: NavigationGeneration,
        intent: NavigationIntent,
        direction: Option<Direction>,
        extend: bool,
    ) -> Result<Effect, EditError> {
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
            intent,
            direction,
        )
    }

    /// 提交 MathText 候选并保留树内逻辑锚点与视觉亲和性。
    pub(in crate::editing::session::keyboards) fn commit_math_text_navigation_candidate(
        &mut self,
        focus: MathTextPosition,
        visual_affinity: VisualAffinity,
        generation: NavigationGeneration,
        extend: bool,
    ) -> Result<Effect, EditError> {
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

    /// 提交 Home/End 候选；调用方已处理 composition、命令行和 MathEdit 优先级。
    pub(in crate::editing::session::keyboards) fn commit_math_visual_line_edge_candidate(
        &mut self,
        candidate: Option<NavigationCandidate>,
        extend: bool,
    ) -> Result<Effect, EditError> {
        let branch = &self
            .math_edit
            .as_ref()
            .ok_or(EditError::InvalidPosition)?
            .branch;
        match (branch, candidate) {
            (
                MathEditBranch::Text { .. },
                Some(NavigationCandidate::MathText {
                    focus,
                    visual_affinity,
                    generation,
                }),
            ) => self.commit_math_text_navigation_candidate(
                focus,
                visual_affinity,
                generation,
                extend,
            ),
            (MathEditBranch::Text { .. }, None) => Ok(Effect::default()),
            (MathEditBranch::Text { .. }, Some(_)) => Err(EditError::InvalidPosition),
            (
                MathEditBranch::Tree { .. } | MathEditBranch::Source,
                Some(NavigationCandidate::Document { generation, .. }),
            ) => {
                self.validate_navigation_generation(&generation)?;
                Ok(Effect::default())
            }
            (MathEditBranch::Tree { .. } | MathEditBranch::Source, Some(_)) => {
                Err(EditError::InvalidPosition)
            }
            (MathEditBranch::Tree { .. } | MathEditBranch::Source, None) => Ok(Effect::default()),
        }
    }

    /// 完成正文水平导航的空候选边缘动作；按键 handler 负责调用时机。
    pub(in crate::editing::session::keyboards) fn complete_horizontal_document_edge(
        &mut self,
        direction: Direction,
        extend: bool,
    ) -> Result<Effect, EditError> {
        if extend && let Some(environment_id) = self.block_environment_at(&self.selection.focus) {
            self.select_block_environment(environment_id)?;
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        if self.enter_adjacent_math_if_present(direction)? {
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        self.run(Command::ExitBlockEnvironment(direction))
    }
}
