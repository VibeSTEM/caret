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

    /// 仅在布局候选确实停留于当前逻辑/视觉边界时运行空行内宿主迁移。
    pub(in crate::editing::session::keyboards) fn empty_inline_boundary_candidate_matches(
        &mut self,
        candidate: Option<&NavigationCandidate>,
    ) -> Result<bool, EditError> {
        match candidate {
            None => Ok(true),
            Some(NavigationCandidate::Document {
                focus,
                visual_affinity,
                generation,
            }) => {
                self.validate_navigation_generation(generation)?;
                let current_focus = self.selection.focus.clone();
                let current_visual_affinity = self.selection_visual_affinity;
                let map = self.text_map_for(focus)?;
                map.validate_pos(focus)?;
                if focus.paragraph_id != current_focus.paragraph_id
                    || focus.story != current_focus.story
                {
                    return Ok(false);
                }
                Ok(if focus.offset == current_focus.offset {
                    *visual_affinity == current_visual_affinity
                } else {
                    map.has_empty_inline_environment_between(focus.offset, current_focus.offset)
                })
            }
            Some(_) => Ok(false),
        }
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
        let retained_empty_inline_host = if !extend
            && focus.paragraph_id == self.selection.focus.paragraph_id
            && focus.story == self.selection.focus.story
            && focus.offset == self.selection.focus.offset
        {
            let position = self.host_caret_position.clone();
            let map = self.text_map_for(&focus)?;
            position.filter(|position| {
                position.kind() == crate::editing::HostCaretKind::Inline
                    && map.is_empty_inline_environment(focus.offset, position.host_id())
            })
        } else {
            None
        };
        if !extend
            && self.selection.anchor == self.selection.focus
            && focus == self.selection.focus
            && visual_affinity != self.selection_visual_affinity
        {
            let changed = self.selection_visual_affinity != visual_affinity
                || self.selection_anchor_visual_affinity != visual_affinity;
            self.selection_visual_affinity = visual_affinity;
            self.selection_anchor_visual_affinity = visual_affinity;
            return Ok(Effect {
                selection_changed: changed,
                ..Effect::default()
            });
        }
        let anchor = if extend {
            self.structure_selection
                .as_ref()
                .map(|selection| selection.anchor.clone())
                .unwrap_or_else(|| self.selection.anchor.clone())
        } else {
            focus.clone()
        };
        let result = self.set_navigation_selection_with_direction(
            Selection {
                anchor,
                focus: focus.clone(),
            },
            visual_affinity,
            intent,
            direction,
        );
        if result.is_ok()
            && let Some(position) = retained_empty_inline_host
            && self.math_edit.is_none()
            && self.selection.focus == focus
            && self
                .host_caret_position
                .as_ref()
                .is_none_or(|current| current == &position)
        {
            self.host_caret_position = Some(position);
        }
        result
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
        if !extend && let Some(effect) = self.move_from_math_block_boundary_stop(direction)? {
            return Ok(effect);
        }
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

    /// 完成正文视觉列的上下边缘；若相邻块是显示公式，复用同一块邻接入口。
    pub(in crate::editing::session::keyboards) fn complete_vertical_document_edge(
        &mut self,
        direction: Direction,
        extend: bool,
    ) -> Result<Effect, EditError> {
        if !extend && let Some(effect) = self.move_from_math_block_boundary_stop(direction)? {
            return Ok(effect);
        }
        if !extend && self.enter_adjacent_block_math_if_present(direction)? {
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        Ok(Effect::default())
    }
}
