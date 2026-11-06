//! 将数学 TextFlow 格式命令提交为宿主 MathTree 事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::history::{DocumentPatch, MathEditPosition, SessionHistoryEdit};
use super::super::{EditError, Effect};
use super::text_branch::apply_math_text_environment_tree_range;
use super::{
    MathContent, MathEdit, MathEditBranch, MathPositionAffinity, MathTextFlowPath,
    MathTextFlowPathStep, MathTextPosition, Session,
};
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
        let MathEditBranch::Text { path, anchor, .. } = edit.branch.clone() else {
            return Err(EditError::UnsupportedInlineFormat);
        };
        if anchor.path != path {
            return Err(EditError::UnsupportedInlineFormat);
        }
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let text = path
            .resolve_structurally(tree)
            .ok_or(EditError::InvalidPosition)?;
        let display = text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        let range = anchor.offset.min(edit.cursor)..anchor.offset.max(edit.cursor);
        if range.end > display.len()
            || !display.is_char_boundary(range.start)
            || !display.is_char_boundary(range.end)
            || !super::math_text_offsets_are_boundaries(tree, &path, &[range.start, range.end])
        {
            return Err(EditError::InvalidPosition);
        }

        let inherited =
            inherited_environment_value(tree, &path, kind).ok_or(EditError::InvalidPosition)?;
        let current = self.pending_inline_style.get(kind).unwrap_or(inherited);
        let enabled = !current;
        if range.is_empty() {
            return self.set_pending_math_environment(&edit, &path, kind, enabled);
        }

        let mut updated_tree = tree.clone();
        let selected_path = apply_math_text_environment_tree_range(
            &mut updated_tree,
            &path,
            range.clone(),
            kind,
            enabled,
        )?;
        updated_tree
            .validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let selected_len = range.end - range.start;
        let (next_anchor, next_cursor) = if anchor.offset <= edit.cursor {
            (0, selected_len)
        } else {
            (selected_len, 0)
        };
        self.commit_math_text_tree(
            edit,
            updated_tree,
            super::text_branch::MathTextTreeCommit {
                branch: super::MathEditBranch::Text {
                    path: selected_path.clone(),
                    anchor: MathTextPosition::new(
                        selected_path.clone(),
                        next_anchor,
                        MathPositionAffinity::Downstream,
                    ),
                    focus_affinity: MathPositionAffinity::Downstream,
                    anchor_visual_affinity: crate::VisualAffinity::Downstream,
                    visual_affinity: crate::VisualAffinity::Downstream,
                },
                align_cell: selected_path.align_cell(),
                cursor: next_cursor,
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
        let MathEditBranch::Text { path, anchor, .. } = edit.branch.clone() else {
            return Err(EditError::UnsupportedInlineFormat);
        };
        if anchor.path != path {
            return Err(EditError::UnsupportedInlineFormat);
        }
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let text = path
            .resolve_structurally(tree)
            .ok_or(EditError::InvalidPosition)?;
        let display = text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        let range = anchor.offset.min(edit.cursor)..anchor.offset.max(edit.cursor);
        if range.end > display.len()
            || !display.is_char_boundary(range.start)
            || !display.is_char_boundary(range.end)
            || !super::math_text_offsets_are_boundaries(tree, &path, &[range.start, range.end])
        {
            return Err(EditError::InvalidPosition);
        }

        let inherited =
            inherited_math_text_font_family(tree, &path).ok_or(EditError::InvalidPosition)?;
        if range.is_empty() {
            let current = self
                .pending_inline_style
                .active_font_family()
                .or(inherited.as_deref());
            if current == Some(family) {
                return Ok(Effect::default());
            }
            return self.set_pending_math_text_font_family(&edit, &path, family);
        }

        let mut updated_tree = tree.clone();
        let selected_path = super::text_branch::apply_math_text_font_family_tree_range(
            &mut updated_tree,
            &path,
            range.clone(),
            family,
        )?;
        if updated_tree == *tree {
            return Ok(Effect::default());
        }
        updated_tree
            .validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let selected_len = range.end - range.start;
        let (next_anchor, next_cursor) = if anchor.offset <= edit.cursor {
            (0, selected_len)
        } else {
            (selected_len, 0)
        };
        self.commit_math_text_tree(
            edit,
            updated_tree,
            super::text_branch::MathTextTreeCommit {
                branch: super::MathEditBranch::Text {
                    path: selected_path.clone(),
                    anchor: MathTextPosition::new(
                        selected_path.clone(),
                        next_anchor,
                        MathPositionAffinity::Downstream,
                    ),
                    focus_affinity: MathPositionAffinity::Downstream,
                    anchor_visual_affinity: crate::VisualAffinity::Downstream,
                    visual_affinity: crate::VisualAffinity::Downstream,
                },
                align_cell: selected_path.align_cell(),
                cursor: next_cursor,
                merge_group: None,
                pending_style: super::text_branch::PendingStylePolicy::Clear,
                selection_changed: true,
            },
        )
    }

    fn set_pending_math_text_font_family(
        &mut self,
        edit: &MathEdit,
        path: &MathTextFlowPath,
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
        let after_math = MathEditPosition::with_edit_state(
            edit,
            edit.cursor,
            MathEditBranch::Text {
                path: path.clone(),
                anchor: MathTextPosition::new(
                    path.clone(),
                    edit.cursor,
                    MathPositionAffinity::Downstream,
                ),
                focus_affinity: MathPositionAffinity::Downstream,
                anchor_visual_affinity: crate::VisualAffinity::Downstream,
                visual_affinity: crate::VisualAffinity::Downstream,
            },
        );
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
        path: &MathTextFlowPath,
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
        let after_math = MathEditPosition::with_edit_state(
            edit,
            edit.cursor,
            MathEditBranch::Text {
                path: path.clone(),
                anchor: MathTextPosition::new(
                    path.clone(),
                    edit.cursor,
                    MathPositionAffinity::Downstream,
                ),
                focus_affinity: MathPositionAffinity::Downstream,
                anchor_visual_affinity: crate::VisualAffinity::Downstream,
                visual_affinity: crate::VisualAffinity::Downstream,
            },
        );
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

pub(super) fn inherited_environment_value(
    tree: &MathTree,
    path: &MathTextFlowPath,
    kind: InlineEnvironmentKind,
) -> Option<bool> {
    fn in_flow(
        flow: &caret_doc::MathFlow,
        steps: &[MathTextFlowPathStep],
        kind: InlineEnvironmentKind,
        inherited: bool,
    ) -> Option<bool> {
        let MathTextFlowPathStep::Content(index) = steps.first()? else {
            return None;
        };
        in_content(flow.content.get(*index)?, &steps[1..], kind, inherited)
    }

    fn in_content(
        content: &MathContent,
        steps: &[MathTextFlowPathStep],
        kind: InlineEnvironmentKind,
        inherited: bool,
    ) -> Option<bool> {
        match content {
            MathContent::Text(_) if steps.is_empty() => Some(inherited),
            MathContent::Environment(environment) => {
                let MathTextFlowPathStep::EnvironmentChild(index) = steps.first()? else {
                    return None;
                };
                let inherited = if environment.kind == kind {
                    environment
                        .props
                        .get("enabled")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true)
                } else {
                    inherited
                };
                in_content(
                    environment.children.get(*index)?,
                    &steps[1..],
                    kind,
                    inherited,
                )
            }
            MathContent::Math(node) => {
                let MathTextFlowPathStep::Operand(index) = steps.first()? else {
                    return None;
                };
                in_flow(node.children.get(*index)?, &steps[1..], kind, inherited)
            }
            _ => None,
        }
    }

    in_flow(&tree.root, path.steps(), kind, false)
}

pub(super) fn inherited_math_text_font_family(
    tree: &MathTree,
    path: &MathTextFlowPath,
) -> Option<Option<String>> {
    fn in_flow(
        flow: &caret_doc::MathFlow,
        steps: &[MathTextFlowPathStep],
        inherited: Option<String>,
    ) -> Option<Option<String>> {
        let MathTextFlowPathStep::Content(index) = steps.first()? else {
            return None;
        };
        in_content(flow.content.get(*index)?, &steps[1..], inherited)
    }

    fn in_content(
        content: &MathContent,
        steps: &[MathTextFlowPathStep],
        inherited: Option<String>,
    ) -> Option<Option<String>> {
        match content {
            MathContent::Text(_) if steps.is_empty() => Some(inherited),
            MathContent::Environment(environment) => {
                let MathTextFlowPathStep::EnvironmentChild(index) = steps.first()? else {
                    return None;
                };
                let inherited = if environment.kind == InlineEnvironmentKind::Font {
                    environment
                        .props
                        .get("fontFamily")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .or(inherited)
                } else {
                    inherited
                };
                in_content(environment.children.get(*index)?, &steps[1..], inherited)
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

    in_flow(&tree.root, path.steps(), None)
}
