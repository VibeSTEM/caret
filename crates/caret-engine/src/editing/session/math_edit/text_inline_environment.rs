//! 将数学 TextFlow 格式命令提交为宿主 MathTree 事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::flow_format::{InheritedInlineStyle, text_flow_inline_format_state};
use super::super::history::{DocumentPatch, MathEditPosition, SessionHistoryEdit};
use super::super::{EditError, Effect};
use super::{
    MathContent, MathEdit, MathEditBranch, MathTextFlowPath, MathTextFlowPathStep,
    MathTextPosition, Session,
};
use crate::editing::flow_format::InlineFormatChange;
use crate::editing::session::history::SessionHistoryState;
use caret_doc::{InlineEnvironmentKind, MathTree};

impl Session {
    pub(super) fn toggle_math_text_inline_environment(
        &mut self,
        kind: InlineEnvironmentKind,
    ) -> Result<Effect, EditError> {
        if !matches!(
            kind,
            InlineEnvironmentKind::Bold | InlineEnvironmentKind::Italic
        ) {
            return Err(EditError::UnsupportedInlineFormat);
        }
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }

        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let inherited_base =
            InheritedInlineStyle::from_theme(self.document.global_settings.theme.as_ref())
                .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
        let MathEditBranch::Text {
            path,
            anchor,
            focus_affinity,
            anchor_visual_affinity,
            visual_affinity,
        } = edit.branch.clone()
        else {
            return Err(EditError::UnsupportedInlineFormat);
        };
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let focus = MathTextPosition::new(path.clone(), edit.cursor, focus_affinity);
        let (island, range) = super::math_text_selection_range(tree, &path, &anchor, edit.cursor)
            .ok_or(EditError::InvalidPosition)?;
        if range.is_empty() {
            let inherited =
                inherited_environment_value_with_style(tree, &path, kind, inherited_base.clone())
                    .ok_or(EditError::InvalidPosition)?;
            let current = self.pending_inline_style.get(kind).unwrap_or(inherited);
            return self.set_pending_math_environment(&edit, kind, !current);
        }

        let mut any_text = false;
        let mut any_disabled = false;
        for (candidate, flow_range) in island.paths.iter().zip(&island.ranges) {
            let start = range.start.max(flow_range.start);
            let end = range.end.min(flow_range.end);
            if start >= end {
                continue;
            }
            let local_range = start - flow_range.start..end - flow_range.start;
            let text = candidate
                .resolve_structurally(tree)
                .ok_or(EditError::InvalidPosition)?;
            let inherited =
                inherited_math_text_style_with_style(tree, candidate, inherited_base.clone())
                    .ok_or(EditError::InvalidPosition)?;
            let (contains_text, contains_disabled) =
                text_flow_inline_format_state(text, local_range, kind, &inherited)?;
            any_text |= contains_text;
            any_disabled |= contains_disabled;
        }
        if !any_text {
            return Ok(Effect::default());
        }
        let enabled = any_disabled;
        let mut updated_tree = tree.clone();
        let (next_anchor, next_focus) =
            super::text_branch::format_tree_edit::apply_math_text_format_range_with_style(
                &mut updated_tree,
                &anchor,
                &focus,
                InlineFormatChange::State(kind, enabled),
                inherited_base,
            )?;
        updated_tree
            .validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let next_path = next_focus.path.clone();
        self.commit_math_text_tree(
            edit,
            updated_tree,
            super::text_branch::MathTextTreeCommit {
                branch: super::MathEditBranch::Text {
                    path: next_path.clone(),
                    anchor: next_anchor,
                    focus_affinity: next_focus.affinity,
                    anchor_visual_affinity,
                    visual_affinity,
                },
                align_cell: next_path.align_cell(),
                cursor: next_focus.offset,
                merge_group: None,
                pending_style: super::text_branch::PendingStylePolicy::Clear,
                selection_changed: true,
            },
        )
    }

    pub(super) fn set_math_text_font_family(&mut self, family: &str) -> Result<Effect, EditError> {
        if family.trim().is_empty() {
            return Err(EditError::InvalidFontFamily);
        }
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }

        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let inherited_base =
            InheritedInlineStyle::from_theme(self.document.global_settings.theme.as_ref())
                .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
        let MathEditBranch::Text {
            path,
            anchor,
            focus_affinity,
            anchor_visual_affinity,
            visual_affinity,
        } = edit.branch.clone()
        else {
            return Err(EditError::UnsupportedInlineFormat);
        };
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let focus = MathTextPosition::new(path.clone(), edit.cursor, focus_affinity);
        let (_, range) = super::math_text_selection_range(tree, &path, &anchor, edit.cursor)
            .ok_or(EditError::InvalidPosition)?;
        if range.is_empty() {
            let inherited =
                inherited_math_text_style_with_style(tree, &path, inherited_base.clone())
                    .map(|style| style.font_family)
                    .ok_or(EditError::InvalidPosition)?;
            let current = self
                .pending_inline_style
                .active_font_family()
                .or(inherited.as_deref());
            if current == Some(family) {
                return Ok(Effect::default());
            }
            return self.set_pending_math_text_font_family(&edit, family);
        }

        let mut updated_tree = tree.clone();
        let (next_anchor, next_focus) =
            super::text_branch::format_tree_edit::apply_math_text_format_range_with_style(
                &mut updated_tree,
                &anchor,
                &focus,
                InlineFormatChange::FontFamily(family),
                inherited_base,
            )?;
        if updated_tree == *tree {
            return Ok(Effect::default());
        }
        updated_tree
            .validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let next_path = next_focus.path.clone();
        self.commit_math_text_tree(
            edit,
            updated_tree,
            super::text_branch::MathTextTreeCommit {
                branch: super::MathEditBranch::Text {
                    path: next_path.clone(),
                    anchor: next_anchor,
                    focus_affinity: next_focus.affinity,
                    anchor_visual_affinity,
                    visual_affinity,
                },
                align_cell: next_path.align_cell(),
                cursor: next_focus.offset,
                merge_group: None,
                pending_style: super::text_branch::PendingStylePolicy::Clear,
                selection_changed: true,
            },
        )
    }

    fn set_pending_math_text_font_family(
        &mut self,
        edit: &MathEdit,
        family: &str,
    ) -> Result<Effect, EditError> {
        if self.pending_inline_style.active_font_family() == Some(family) {
            return Ok(Effect::default());
        }
        let before_pending = self.pending_inline_style.clone();
        let before_selection = self.selection.clone();
        let before_affinity = self.selection_visual_affinity;
        let state_id = self.history_state_id;
        let before_math = super::history_position(edit);
        self.pending_inline_style.set_font_family(family.to_owned());
        self.advance_history_merge_epoch();
        let after_math = MathEditPosition::with_edit_state(edit, edit.cursor, edit.branch.clone());
        let before = SessionHistoryState::new(
            before_selection.clone(),
            before_affinity,
            before_pending,
            state_id,
        )
        .with_math_edit(Some(before_math));
        let after = SessionHistoryState::new(
            before_selection,
            before_affinity,
            self.pending_inline_style.clone(),
            state_id,
        )
        .with_math_edit(Some(after_math));
        self.record_history_edit(SessionHistoryEdit::new(
            DocumentPatch::PendingStyle,
            before,
            after,
        ));
        Ok(Effect::default())
    }

    fn set_pending_math_environment(
        &mut self,
        edit: &MathEdit,
        kind: InlineEnvironmentKind,
        enabled: bool,
    ) -> Result<Effect, EditError> {
        if self.pending_inline_style.get(kind) == Some(enabled) {
            return Ok(Effect::default());
        }
        let before_pending = self.pending_inline_style.clone();
        let before_selection = self.selection.clone();
        let before_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let before_math = super::history_position(edit);
        self.pending_inline_style.set(kind, enabled)?;
        self.advance_history_merge_epoch();
        let after_math = MathEditPosition::with_edit_state(edit, edit.cursor, edit.branch.clone());
        let before = SessionHistoryState::new(
            before_selection.clone(),
            before_affinity,
            before_pending,
            before_state_id,
        )
        .with_math_edit(Some(before_math));
        let after = SessionHistoryState::new(
            before_selection,
            before_affinity,
            self.pending_inline_style.clone(),
            before_state_id,
        )
        .with_math_edit(Some(after_math));
        self.record_history_edit(SessionHistoryEdit::new(
            DocumentPatch::PendingStyle,
            before,
            after,
        ));
        Ok(Effect::default())
    }
}

#[cfg(test)]
pub(super) fn inherited_environment_value(
    tree: &MathTree,
    path: &MathTextFlowPath,
    kind: InlineEnvironmentKind,
) -> Option<bool> {
    inherited_math_text_style(tree, path)?.state(kind)
}

fn inherited_environment_value_with_style(
    tree: &MathTree,
    path: &MathTextFlowPath,
    kind: InlineEnvironmentKind,
    inherited: InheritedInlineStyle,
) -> Option<bool> {
    inherited_math_text_style_with_style(tree, path, inherited)?.state(kind)
}

#[cfg(test)]
pub(super) fn inherited_math_text_style(
    tree: &MathTree,
    path: &MathTextFlowPath,
) -> Option<InheritedInlineStyle> {
    inherited_math_text_style_with_style(tree, path, InheritedInlineStyle::default())
}

fn inherited_math_text_style_with_style(
    tree: &MathTree,
    path: &MathTextFlowPath,
    inherited: InheritedInlineStyle,
) -> Option<InheritedInlineStyle> {
    fn in_flow(
        flow: &caret_doc::MathFlow,
        steps: &[MathTextFlowPathStep],
        inherited: InheritedInlineStyle,
    ) -> Option<InheritedInlineStyle> {
        let MathTextFlowPathStep::Content(index) = steps.first()? else {
            return None;
        };
        in_content(flow.content.get(*index)?, &steps[1..], inherited)
    }

    fn in_content(
        content: &MathContent,
        steps: &[MathTextFlowPathStep],
        inherited: InheritedInlineStyle,
    ) -> Option<InheritedInlineStyle> {
        match content {
            MathContent::Text(_) if steps.is_empty() => Some(inherited),
            MathContent::Environment(environment) => {
                let MathTextFlowPathStep::EnvironmentChild(index) = steps.first()? else {
                    return None;
                };
                in_content(
                    environment.children.get(*index)?,
                    &steps[1..],
                    inherited.with_environment(environment),
                )
            }
            MathContent::Math(node) => {
                let MathTextFlowPathStep::Operand(index) = steps.first()? else {
                    return None;
                };
                in_flow(node.children.get(*index)?, &steps[1..], inherited)
            }
            _ => None,
        }
    }

    in_flow(&tree.root, path.steps(), inherited)
}

#[cfg(test)]
pub(super) fn inherited_math_text_font_family(
    tree: &MathTree,
    path: &MathTextFlowPath,
) -> Option<Option<String>> {
    Some(inherited_math_text_style(tree, path)?.font_family)
}

pub(super) fn inherited_math_text_font_family_with_style(
    tree: &MathTree,
    path: &MathTextFlowPath,
    inherited: InheritedInlineStyle,
) -> Option<Option<String>> {
    Some(inherited_math_text_style_with_style(tree, path, inherited)?.font_family)
}
