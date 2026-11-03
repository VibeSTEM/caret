//! 数学 TextFlow 行内格式的直接树事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::EditError;
use super::super::path::content_slot_mut;
use super::super::{MathTextFlowPath, text_run_split::split_text_flow_runs_for_format};
use super::source_map_edit::{rebase_text_map_entry, update_math_flow_ranges};
use caret_doc::{InlineEnvironment, MathContent, MathSourceRange, NodeId, TextFlowSourceMapEntry};
use serde_json::Value;
use std::collections::BTreeMap;
use std::ops::Range;

#[derive(Default)]
struct FormatSourceMapParts {
    left: Vec<TextFlowSourceMapEntry>,
    selected: Vec<TextFlowSourceMapEntry>,
    right: Vec<TextFlowSourceMapEntry>,
}

pub(in crate::editing::session::math_edit) fn apply_math_text_format_tree_edit(
    tree: &mut caret_doc::MathTree,
    path: &MathTextFlowPath,
    format: &super::MathTextFormat,
    source_edit: Range<usize>,
    replacement: &str,
) -> Result<(), EditError> {
    let original = path
        .resolve(tree)
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let old_range = original
        .source_range
        .as_ref()
        .map(|range| range.start as usize..range.end as usize)
        .ok_or(EditError::InvalidPosition)?;
    if source_edit.start < old_range.start
        || source_edit.end > old_range.end
        || source_edit.start > source_edit.end
        || format.selected_source_len != source_edit.len()
    {
        return Err(EditError::InvalidPosition);
    }
    let selected_start_in_flow = source_edit
        .start
        .checked_sub(old_range.start)
        .ok_or(EditError::InvalidPosition)?;
    let prefix_len = format
        .selected_source_start
        .checked_sub(selected_start_in_flow)
        .ok_or(EditError::InvalidPosition)?;
    let selected_end_in_replacement = prefix_len
        .checked_add(format.selected_source_len)
        .ok_or(EditError::InvalidPosition)?;
    if replacement.get(prefix_len..selected_end_in_replacement)
        != tree.source.get(source_edit.clone())
    {
        return Err(EditError::InvalidPosition);
    }
    let selected_source = source_edit
        .start
        .checked_add(prefix_len)
        .ok_or(EditError::InvalidPosition)?
        ..source_edit
            .start
            .checked_add(selected_end_in_replacement)
            .ok_or(EditError::InvalidPosition)?;
    let delta = replacement.len() as isize - source_edit.len() as isize;
    let right_source_start = source_edit
        .end
        .checked_add_signed(delta)
        .ok_or(EditError::InvalidPosition)?;
    let right_source = right_source_start
        ..old_range
            .end
            .checked_add_signed(delta)
            .ok_or(EditError::InvalidPosition)?;

    let display = original
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    if format.display_range.end > display.len()
        || display.get(format.display_range.clone()) != Some(format.selected_text.as_str())
    {
        return Err(EditError::InvalidPosition);
    }
    let (left_runs, selected_runs, right_runs) =
        split_text_flow_runs_for_format(&original, &format.display_range)
            .ok_or(EditError::UnsupportedOpaqueContent)?;
    let has_left = !left_runs.is_empty() || old_range.start < source_edit.start;
    let has_right = !right_runs.is_empty() || source_edit.end < old_range.end;
    let FormatSourceMapParts {
        left: left_map,
        selected: selected_map,
        right: right_map,
    } = split_format_source_map(
        &original,
        &format.display_range,
        prefix_len,
        delta,
        has_left,
        has_right,
    )?;
    let segment_count = 1 + usize::from(has_left) + usize::from(has_right);
    if segment_count > 1
        && (!original.extra.is_empty()
            || original
                .source_range
                .as_ref()
                .is_some_and(|range| !range.extra.is_empty()))
    {
        return Err(EditError::UnsupportedOpaqueContent);
    }

    let mut replacement_contents = Vec::with_capacity(segment_count + 1);
    if has_left {
        replacement_contents.push(MathContent::Text(text_flow_fragment(
            &original,
            left_runs,
            old_range.start..source_edit.start,
            left_map,
            segment_count == 1,
        )?));
    }
    let selected_flow = text_flow_fragment(
        &original,
        selected_runs,
        selected_source.clone(),
        selected_map,
        segment_count == 1,
    )?;
    let mut props = BTreeMap::new();
    if !format.enabled {
        props.insert("enabled".to_owned(), Value::Bool(false));
    }
    replacement_contents.push(MathContent::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind: format.kind,
        definition_id: None,
        source_range: Some(MathSourceRange {
            start: u32::try_from(source_edit.start).map_err(|_| EditError::InvalidPosition)?,
            end: u32::try_from(
                source_edit
                    .end
                    .checked_add_signed(delta)
                    .ok_or(EditError::InvalidPosition)?,
            )
            .map_err(|_| EditError::InvalidPosition)?,
            extra: Default::default(),
        }),
        props,
        children: vec![MathContent::Text(selected_flow)],
        extra: BTreeMap::new(),
    }));
    if has_right {
        replacement_contents.push(MathContent::Text(text_flow_fragment(
            &original,
            right_runs,
            right_source,
            right_map,
            segment_count == 1,
        )?));
    }

    update_math_flow_ranges(
        &mut tree.root,
        &[],
        path.steps(),
        &source_edit,
        &(source_edit.start
            ..source_edit
                .end
                .checked_add_signed(delta)
                .ok_or(EditError::InvalidPosition)?),
    )?;
    let (contents, index) =
        content_slot_mut(&mut tree.root, path.steps()).ok_or(EditError::InvalidPosition)?;
    contents.splice(index..index + 1, replacement_contents);
    tree.source.replace_range(source_edit, replacement);
    Ok(())
}

fn split_format_source_map(
    original: &caret_doc::TextFlow,
    display_range: &Range<usize>,
    prefix_len: usize,
    source_delta: isize,
    has_left: bool,
    has_right: bool,
) -> Result<FormatSourceMapParts, EditError> {
    let mut result = FormatSourceMapParts::default();
    for entry in &original.source_map {
        let text_range = entry.text_start as usize..entry.text_end as usize;
        let source_range = entry.source_range.start as usize..entry.source_range.end as usize;
        if text_range.is_empty() {
            if text_range.start < display_range.start
                || (text_range.start == display_range.start && has_left)
            {
                result.left.push(entry.clone());
            } else if text_range.start > display_range.end
                || (text_range.start == display_range.end && has_right)
            {
                let mut shifted = rebase_text_map_entry(entry, 0, source_delta)?;
                shifted.text_start = u32::try_from(text_range.start - display_range.end)
                    .map_err(|_| EditError::InvalidPosition)?;
                shifted.text_end = shifted.text_start;
                result.right.push(shifted);
            } else {
                let mut selected = entry.clone();
                selected.text_start = u32::try_from(text_range.start - display_range.start)
                    .map_err(|_| EditError::InvalidPosition)?;
                selected.text_end = selected.text_start;
                selected.source_range.start = u32::try_from(
                    source_range
                        .start
                        .checked_add(prefix_len)
                        .ok_or(EditError::InvalidPosition)?,
                )
                .map_err(|_| EditError::InvalidPosition)?;
                selected.source_range.end = u32::try_from(
                    source_range
                        .end
                        .checked_add(prefix_len)
                        .ok_or(EditError::InvalidPosition)?,
                )
                .map_err(|_| EditError::InvalidPosition)?;
                result.selected.push(selected);
            }
            continue;
        }
        let linear = text_range.len() == source_range.len();
        let mut pieces = Vec::new();
        for (slot, part) in [
            (
                0usize,
                text_range.start..text_range.end.min(display_range.start),
            ),
            (
                1usize,
                text_range.start.max(display_range.start)..text_range.end.min(display_range.end),
            ),
            (
                2usize,
                text_range.start.max(display_range.end)..text_range.end,
            ),
        ] {
            if part.start < part.end {
                pieces.push((slot, part));
            }
        }
        if !linear && pieces.len() > 1 {
            return Err(EditError::InvalidPosition);
        }
        if pieces.len() > 1 && (!entry.extra.is_empty() || !entry.source_range.extra.is_empty()) {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        for (slot, part) in pieces {
            let mapped_start = if linear {
                source_range.start + part.start - text_range.start
            } else {
                source_range.start
            };
            let mapped_end = if linear {
                source_range.start + part.end - text_range.start
            } else {
                source_range.end
            };
            let (display_base, source_shift, target) = match slot {
                0 => (0, 0, &mut result.left),
                1 => (
                    display_range.start,
                    prefix_len as isize,
                    &mut result.selected,
                ),
                _ => (display_range.end, source_delta, &mut result.right),
            };
            let mut mapped = entry.clone();
            mapped.text_start =
                u32::try_from(part.start - display_base).map_err(|_| EditError::InvalidPosition)?;
            mapped.text_end =
                u32::try_from(part.end - display_base).map_err(|_| EditError::InvalidPosition)?;
            mapped.source_range.start = u32::try_from(
                mapped_start
                    .checked_add_signed(source_shift)
                    .ok_or(EditError::InvalidPosition)?,
            )
            .map_err(|_| EditError::InvalidPosition)?;
            mapped.source_range.end = u32::try_from(
                mapped_end
                    .checked_add_signed(source_shift)
                    .ok_or(EditError::InvalidPosition)?,
            )
            .map_err(|_| EditError::InvalidPosition)?;
            target.push(mapped);
        }
    }
    Ok(result)
}

fn text_flow_fragment(
    original: &caret_doc::TextFlow,
    runs: Vec<caret_doc::Run>,
    source_range: Range<usize>,
    source_map: Vec<TextFlowSourceMapEntry>,
    preserve_flow_extra: bool,
) -> Result<caret_doc::TextFlow, EditError> {
    let mut text = original.clone();
    text.runs = runs;
    text.source_range = Some(MathSourceRange {
        start: u32::try_from(source_range.start).map_err(|_| EditError::InvalidPosition)?,
        end: u32::try_from(source_range.end).map_err(|_| EditError::InvalidPosition)?,
        extra: if preserve_flow_extra {
            original
                .source_range
                .as_ref()
                .map(|range| range.extra.clone())
                .unwrap_or_default()
        } else {
            Default::default()
        },
    });
    text.source_map = source_map;
    if !preserve_flow_extra {
        text.extra.clear();
    }
    Ok(text)
}
