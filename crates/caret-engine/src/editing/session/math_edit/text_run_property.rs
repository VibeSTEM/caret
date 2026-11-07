//! 将 Run 属性格式命令适配到数学树中的 TextFlow。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::environment::inline_adapter::{
    RunPropertyEnvironment, run_property_for_key, run_property_for_kind,
};
use super::super::super::flow_format::{format_flow_run_property, run_property_range_state};
use super::super::history::{DocumentPatch, MathEditPosition, SessionHistoryEdit};
use super::super::{Affinity, EditError, Effect};
use super::{
    MathEdit, MathEditBranch, MathPositionAffinity, Session, history_position,
    math_text_selection_range,
};
use crate::editing::session::history::SessionHistoryState;
use caret_doc::{Block, FlowContent, MathNode, MathTree, TextFlow};

impl Session {
    pub(super) fn toggle_math_text_run_property(
        &mut self,
        kind: crate::editing::EnvironmentKind,
    ) -> Result<Effect, EditError> {
        let descriptor = run_property_for_kind(kind)
            .copied()
            .ok_or(EditError::UnsupportedInlineFormat)?;
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
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
        let (island, range) = math_text_selection_range(tree, &path, &anchor, edit.cursor)
            .ok_or(EditError::InvalidPosition)?;

        if range.is_empty() {
            let text = path
                .resolve_structurally(tree)
                .cloned()
                .ok_or(EditError::InvalidPosition)?;
            return self.set_pending_math_text_property(&edit, descriptor, &text, edit.cursor);
        }

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
            if text_range_has_disabled_property(text, &local_range, descriptor)? {
                any_disabled = true;
                break;
            }
        }
        let mut updated_tree = tree.clone();
        for (candidate, flow_range) in island.paths.iter().zip(&island.ranges).rev() {
            let start = range.start.max(flow_range.start);
            let end = range.end.min(flow_range.end);
            if start >= end {
                continue;
            }
            let local_range = start - flow_range.start..end - flow_range.start;
            let text = candidate
                .resolve_structurally(&updated_tree)
                .cloned()
                .ok_or(EditError::InvalidPosition)?;
            let mut updated = text;
            edit_text_flow_property(&mut updated, local_range, descriptor, Some(any_disabled))?;
            candidate
                .resolve_structurally_mut(&mut updated_tree)
                .ok_or(EditError::InvalidPosition)?
                .clone_from(&updated);
        }
        updated_tree
            .validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let cursor = edit.cursor;
        self.commit_math_text_tree(
            edit,
            updated_tree,
            super::text_branch::MathTextTreeCommit {
                branch: super::MathEditBranch::Text {
                    path: path.clone(),
                    anchor,
                    focus_affinity,
                    anchor_visual_affinity,
                    visual_affinity,
                },
                align_cell: path.align_cell(),
                cursor,
                merge_group: None,
                pending_style: super::text_branch::PendingStylePolicy::Preserve,
                selection_changed: false,
            },
        )
    }

    fn set_pending_math_text_property(
        &mut self,
        edit: &MathEdit,
        descriptor: RunPropertyEnvironment,
        text: &TextFlow,
        offset: usize,
    ) -> Result<Effect, EditError> {
        let affinity = match &edit.branch {
            MathEditBranch::Text { focus_affinity, .. } => match focus_affinity {
                MathPositionAffinity::Upstream => Affinity::Upstream,
                MathPositionAffinity::Downstream => Affinity::Downstream,
            },
            MathEditBranch::Source | MathEditBranch::Tree { .. } => {
                return Err(EditError::InvalidPosition);
            }
        };
        let inherited = super::super::super::flow_edit::insertion_template_with_affinity(
            &text.runs, offset, affinity,
        )
        .and_then(|run| run.props.get(descriptor.key))
        .is_some_and(|value| (descriptor.is_active)(Some(value)));
        let current = self
            .pending_inline_style
            .run_property(descriptor)
            .unwrap_or(inherited);
        let before_pending = self.pending_inline_style.clone();
        let enabled = !current;
        if self.pending_inline_style.run_property(descriptor) == Some(enabled) {
            return Ok(Effect::default());
        }
        let before_selection = self.selection.clone();
        let before_affinity = self.selection_visual_affinity;
        let state_id = self.history_state_id;
        let before_math = history_position(edit);
        self.pending_inline_style
            .set_run_property(descriptor, enabled);
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
}

pub(super) fn apply_pending_run_properties(
    text: &mut TextFlow,
    start: usize,
    inserted_len: usize,
    properties: &[(&'static str, bool)],
) -> Result<(), EditError> {
    if inserted_len == 0 {
        return Ok(());
    }
    let end = start
        .checked_add(inserted_len)
        .ok_or(EditError::InvalidPosition)?;
    for (key, enabled) in properties {
        let descriptor = run_property_for_key(key).ok_or(EditError::UnsupportedInlineFormat)?;
        edit_text_flow_property(text, start..end, *descriptor, Some(*enabled))?;
    }
    Ok(())
}

pub(super) fn block_math_tree_mut(
    block: &mut Block,
    cell: Option<(usize, usize)>,
) -> Option<&mut MathTree> {
    match (block, cell) {
        (Block::Equation(equation), None) => Some(&mut equation.math_tree),
        (Block::Align(align), Some((row, column))) => {
            align.rows.get_mut(row)?.cells.get_mut(column)
        }
        _ => None,
    }
}

fn text_range_has_disabled_property(
    text: &TextFlow,
    range: &std::ops::Range<usize>,
    descriptor: RunPropertyEnvironment,
) -> Result<bool, EditError> {
    let contents = [FlowContent::<MathNode>::Text(text.clone())];
    let (any_text, any_disabled) = run_property_range_state(&contents, range.clone(), descriptor)?;
    if !any_text {
        return Err(EditError::InvalidPosition);
    }
    Ok(any_disabled)
}

fn edit_text_flow_property(
    text: &mut TextFlow,
    range: std::ops::Range<usize>,
    descriptor: RunPropertyEnvironment,
    value: Option<bool>,
) -> Result<(), EditError> {
    let contents = [FlowContent::<MathNode>::Text(text.clone())];
    let mut updated = format_flow_run_property(&contents, range, descriptor, value)?;
    let [FlowContent::Text(updated)] = updated.as_mut_slice() else {
        return Err(EditError::InvalidPosition);
    };
    text.clone_from(updated);
    Ok(())
}
