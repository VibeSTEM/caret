//! 将 Run 属性格式命令适配到数学树中的 TextFlow。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::environment::inline_adapter::{
    RunPropertyEnvironment, run_property_for_key, run_property_for_kind, set_run_property_value,
};
use super::super::history::{self, DocumentPatch, MathEditPosition, SessionHistoryEdit};
use super::super::{Affinity, EditError, Effect};
use super::{
    MathEdit, MathEditBranch, MathEditTarget, Session, block_list, find_block_path,
    find_inline_math_mut, history_position,
};
use crate::editing::session::history::SessionHistoryState;
use crate::editing::session::math_edit::path::MathTextFlowPath;
use caret_doc::{Block, MathTree, Run, TextFlow};

impl Session {
    pub(super) fn toggle_math_text_run_property(
        &mut self,
        kind: crate::editing::EnvironmentKind,
    ) -> Result<Effect, EditError> {
        let descriptor = run_property_for_kind(kind)
            .copied()
            .ok_or(EditError::UnsupportedInlineFormat)?;
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text { path, anchor } = edit.branch.clone() else {
            return Err(EditError::UnsupportedInlineFormat);
        };
        let range = anchor.min(edit.cursor)..anchor.max(edit.cursor);
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let (text, _) = path
            .resolve_in_projection(tree, &edit.source_projection)
            .ok_or(EditError::InvalidPosition)?;
        let text = (*text).clone();
        let display = text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        if range.end > display.len()
            || !display.is_char_boundary(range.start)
            || !display.is_char_boundary(range.end)
            || !super::is_grapheme_boundary(&display, range.start)
            || !super::is_grapheme_boundary(&display, range.end)
        {
            return Err(EditError::InvalidPosition);
        }

        if range.is_empty() {
            return self.set_pending_math_text_property(
                &edit,
                &path,
                descriptor,
                &text,
                range.start,
            );
        }

        let any_disabled = text_range_has_disabled_property(&text, &range, descriptor)?;
        let mut updated = text.clone();
        edit_text_flow_property(&mut updated, range, descriptor, Some(any_disabled))?;
        if updated == text {
            return Ok(Effect::default());
        }
        self.commit_math_text_flow(edit, &path, updated)
    }

    fn set_pending_math_text_property(
        &mut self,
        edit: &MathEdit,
        path: &MathTextFlowPath,
        descriptor: RunPropertyEnvironment,
        text: &TextFlow,
        offset: usize,
    ) -> Result<Effect, EditError> {
        let inherited = super::super::super::flow_edit::insertion_template(&text.runs, offset)
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
        let after_math = MathEditPosition::with_edit_state(
            edit,
            edit.cursor,
            MathEditBranch::Text {
                path: path.clone(),
                anchor: edit.cursor,
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

    fn commit_math_text_flow(
        &mut self,
        edit: MathEdit,
        path: &MathTextFlowPath,
        updated: TextFlow,
    ) -> Result<Effect, EditError> {
        let before_math = history_position(&edit);
        match &edit.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let story = self
                    .story_for_paragraph(paragraph_id)
                    .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
                let position = super::super::Pos {
                    story,
                    paragraph_id: paragraph_id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                };
                let mut paragraph = self.paragraph_for(&position)?.clone();
                let math = find_inline_math_mut(&mut paragraph.inlines, math_id)
                    .ok_or(EditError::InvalidPosition)?;
                path.resolve_mut_in_projection(&mut math.math_tree, &edit.source_projection)
                    .ok_or(EditError::InvalidPosition)?
                    .clone_from(&updated);
                let before = history::paragraph_states_for_ids(
                    &self.document,
                    &self.paragraph_index,
                    [paragraph_id.clone()],
                );
                self.replace_paragraph(&position, paragraph)?;
                self.text_map_cache.clear();
                let after = history::paragraph_states_for_ids(
                    &self.document,
                    &self.paragraph_index,
                    [paragraph_id.clone()],
                );
                self.record_math_history(
                    DocumentPatch::Paragraphs {
                        before,
                        after,
                        merge_group: None,
                        merge_epoch: self.history_merge_epoch,
                    },
                    (self.selection.clone(), self.selection_visual_affinity),
                    (self.selection.clone(), self.selection_visual_affinity),
                    Some(before_math),
                    Some(history_position(&edit)),
                );
            }
            MathEditTarget::Block { block_id: id, .. } => {
                let (parent_ids, index) =
                    find_block_path(&self.document.body, id).ok_or(EditError::InvalidPosition)?;
                let blocks = block_list(&mut self.document.body, &parent_ids)?;
                let slot = blocks.get_mut(index).ok_or(EditError::InvalidPosition)?;
                let before = slot.clone();
                let mut next = before.clone();
                let tree = block_math_tree_mut(&mut next, path.align_cell())
                    .ok_or(EditError::InvalidPosition)?;
                path.resolve_mut_in_projection(tree, &edit.source_projection)
                    .ok_or(EditError::InvalidPosition)?
                    .clone_from(&updated);
                *slot = next.clone();
                self.text_map_cache.clear();
                self.record_math_history(
                    DocumentPatch::BlockRange {
                        parent_environment_ids: parent_ids,
                        sibling_index: index,
                        before: vec![before],
                        after: vec![next],
                    },
                    (self.selection.clone(), self.selection_visual_affinity),
                    (self.selection.clone(), self.selection_visual_affinity),
                    Some(before_math),
                    Some(history_position(&edit)),
                );
            }
        }
        Ok(Effect {
            document_changed: true,
            ..Effect::default()
        })
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
    let mut offset = 0;
    let mut any_text = false;
    let mut any_disabled = false;
    for run in &text.runs {
        let end = offset + run.text.len();
        if offset < range.end && end > range.start {
            any_text = true;
            any_disabled |= !run
                .props
                .get(descriptor.key)
                .is_some_and(|value| (descriptor.is_active)(Some(value)));
        }
        offset = end;
    }
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
    if range.start >= range.end {
        return Ok(());
    }
    let mut runs = text.runs.clone();
    split_runs_at(&mut runs, range.start)?;
    split_runs_at(&mut runs, range.end)?;
    let mut offset = 0;
    let mut selected_start = None;
    let mut selected_end = None;
    for (index, run) in runs.iter().enumerate() {
        let end = offset + run.text.len();
        if offset < range.end && end > range.start {
            selected_start.get_or_insert(index);
            selected_end = Some(index + 1);
        }
        offset = end;
    }
    let start_index = selected_start.ok_or(EditError::InvalidPosition)?;
    let selected_end = selected_end.ok_or(EditError::InvalidPosition)?;
    if start_index >= selected_end {
        return Err(EditError::InvalidPosition);
    }
    for run in &mut runs[start_index..selected_end] {
        set_run_property_value(run, descriptor, value)?;
    }
    let merge_end = (selected_end + 1).min(runs.len());
    merge_compatible_runs(&mut runs, start_index.saturating_sub(1), merge_end);
    text.runs = runs;
    Ok(())
}

fn split_runs_at(runs: &mut Vec<Run>, offset: usize) -> Result<usize, EditError> {
    let mut current = 0usize;
    for index in 0..runs.len() {
        let end = current + runs[index].text.len();
        if offset == current {
            return Ok(index);
        }
        if offset == end {
            current = end;
            continue;
        }
        if current < offset && offset < end {
            if !runs[index].extra.is_empty() {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            let local = offset - current;
            if !runs[index].text.is_char_boundary(local) {
                return Err(EditError::InvalidPosition);
            }
            let mut right = runs[index].clone();
            right.text = runs[index].text.split_off(local);
            runs.insert(index + 1, right);
            return Ok(index + 1);
        }
        current = end;
    }
    (offset == current)
        .then_some(runs.len())
        .ok_or(EditError::InvalidPosition)
}

fn merge_compatible_runs(runs: &mut Vec<Run>, start: usize, end: usize) {
    let mut merged: Vec<Run> = Vec::with_capacity(end - start);
    for run in runs.drain(start..end) {
        if let Some(previous) = merged.last_mut()
            && previous.language == run.language
            && previous.props == run.props
            && previous.extra.is_empty()
            && run.extra.is_empty()
        {
            previous.text.push_str(&run.text);
        } else {
            merged.push(run);
        }
    }
    runs.splice(start..start, merged);
}
