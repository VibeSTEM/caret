//! 数学 TextFlow 树事务中的源码范围重定位。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::super::flow_edit::replace_text_flow_range;
use super::super::super::EditError;
use super::super::MathTextFlowPath;
use super::validate_text_flow_edit;
use caret_doc::{MathContent, MathFlow, MathNode, MathSourceRange, TextFlowSourceMapEntry};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::editing::session::math_edit) struct EncodedMathTextPiece {
    pub(super) display_range: Range<usize>,
    pub(super) source_range: Range<usize>,
}

/// 直接替换数学 TextFlow，并以同一源码编辑范围重定位树内临时映射。
pub(in crate::editing::session::math_edit) fn apply_math_text_tree_edit(
    tree: &mut caret_doc::MathTree,
    path: &MathTextFlowPath,
    range: Range<usize>,
    inserted: &str,
    source_edit: Range<usize>,
    encoded: &str,
    encoded_pieces: &[EncodedMathTextPiece],
) -> Result<(), EditError> {
    let source_edit_end = source_edit
        .start
        .checked_add(encoded.len())
        .ok_or(EditError::InvalidPosition)?;
    {
        let text = path.resolve_mut(tree).ok_or(EditError::InvalidPosition)?;
        update_text_flow_source_map(
            text,
            range.clone(),
            source_edit.clone(),
            encoded.len(),
            encoded_pieces,
        )?;
        validate_text_flow_edit(text, range.clone(), !inserted.is_empty())?;
        replace_text_flow_range(text, range, inserted)?;
    }
    update_math_flow_ranges(
        &mut tree.root,
        &[],
        path.steps(),
        &source_edit,
        &(source_edit.start..source_edit_end),
    )?;
    Ok(())
}

fn update_text_flow_source_map(
    text: &mut caret_doc::TextFlow,
    display_edit: Range<usize>,
    source_edit: Range<usize>,
    inserted_source_len: usize,
    inserted_pieces: &[EncodedMathTextPiece],
) -> Result<(), EditError> {
    let inserted_text_len = inserted_pieces
        .iter()
        .map(|piece| piece.display_range.end - piece.display_range.start)
        .sum::<usize>();
    let source_delta = inserted_source_len as isize - source_edit.len() as isize;
    let text_delta = inserted_text_len as isize - display_edit.len() as isize;
    let mut updated = Vec::with_capacity(text.source_map.len() + inserted_pieces.len());
    for entry in &text.source_map {
        let text_range = entry.text_start as usize..entry.text_end as usize;
        let source_range = entry.source_range.start as usize..entry.source_range.end as usize;
        if text_range.is_empty() {
            if display_edit.is_empty() && text_range.start == display_edit.start {
                if !entry.extra.is_empty() || !entry.source_range.extra.is_empty() {
                    return Err(EditError::UnsupportedOpaqueContent);
                }
                continue;
            }
            if display_range_precedes(&text_range, &display_edit) {
                updated.push(entry.clone());
            } else if display_range_follows(&text_range, &display_edit) {
                updated.push(rebase_text_map_entry(entry, text_delta, source_delta)?);
            } else if !entry.extra.is_empty() || !entry.source_range.extra.is_empty() {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            continue;
        }

        let linear = text_range.len() == source_range.len();
        let overlap = text_range.start < display_edit.end && display_edit.start < text_range.end;
        if display_edit.is_empty() {
            if text_range.end <= display_edit.start {
                updated.push(entry.clone());
            } else if text_range.start >= display_edit.start {
                updated.push(rebase_text_map_entry(entry, text_delta, source_delta)?);
            } else if linear {
                append_text_map_piece(
                    &mut updated,
                    entry,
                    text_range.start..display_edit.start,
                    &source_range,
                    0,
                    0,
                )?;
                append_text_map_piece(
                    &mut updated,
                    entry,
                    display_edit.start..text_range.end,
                    &source_range,
                    text_delta,
                    source_delta,
                )?;
            } else {
                return Err(EditError::InvalidPosition);
            }
        } else if !overlap {
            if text_range.end <= display_edit.start {
                updated.push(entry.clone());
            } else {
                updated.push(rebase_text_map_entry(entry, text_delta, source_delta)?);
            }
        } else if !linear {
            if display_edit.start > text_range.start || display_edit.end < text_range.end {
                return Err(EditError::InvalidPosition);
            }
            if !entry.extra.is_empty() || !entry.source_range.extra.is_empty() {
                return Err(EditError::UnsupportedOpaqueContent);
            }
        } else {
            let left_end = display_edit.start.min(text_range.end);
            if text_range.start < left_end {
                append_text_map_piece(
                    &mut updated,
                    entry,
                    text_range.start..left_end,
                    &source_range,
                    0,
                    0,
                )?;
            }
            let right_start = display_edit.end.max(text_range.start);
            if right_start < text_range.end {
                append_text_map_piece(
                    &mut updated,
                    entry,
                    right_start..text_range.end,
                    &source_range,
                    text_delta,
                    source_delta,
                )?;
            }
            if overlap && (!entry.extra.is_empty() || !entry.source_range.extra.is_empty()) {
                return Err(EditError::UnsupportedOpaqueContent);
            }
        }
    }

    for piece in inserted_pieces {
        let display_start = display_edit
            .start
            .checked_add(piece.display_range.start)
            .ok_or(EditError::InvalidPosition)?;
        let display_end = display_edit
            .start
            .checked_add(piece.display_range.end)
            .ok_or(EditError::InvalidPosition)?;
        let source_start = source_edit
            .start
            .checked_add(piece.source_range.start)
            .ok_or(EditError::InvalidPosition)?;
        let source_end = source_edit
            .start
            .checked_add(piece.source_range.end)
            .ok_or(EditError::InvalidPosition)?;
        updated.push(TextFlowSourceMapEntry {
            text_start: u32::try_from(display_start).map_err(|_| EditError::InvalidPosition)?,
            text_end: u32::try_from(display_end).map_err(|_| EditError::InvalidPosition)?,
            source_range: MathSourceRange {
                start: u32::try_from(source_start).map_err(|_| EditError::InvalidPosition)?,
                end: u32::try_from(source_end).map_err(|_| EditError::InvalidPosition)?,
                extra: Default::default(),
            },
            extra: Default::default(),
        });
    }
    updated.sort_by_key(|entry| (entry.text_start, entry.text_end));
    if updated.is_empty() {
        let offset = u32::try_from(source_edit.start).map_err(|_| EditError::InvalidPosition)?;
        updated.push(TextFlowSourceMapEntry {
            text_start: 0,
            text_end: 0,
            source_range: MathSourceRange {
                start: offset,
                end: offset,
                extra: Default::default(),
            },
            extra: Default::default(),
        });
    }
    text.source_map = updated;
    Ok(())
}

fn display_range_precedes(range: &Range<usize>, edit: &Range<usize>) -> bool {
    range.end <= edit.start
}

fn display_range_follows(range: &Range<usize>, edit: &Range<usize>) -> bool {
    range.start >= edit.end
}

pub(super) fn rebase_text_map_entry(
    entry: &TextFlowSourceMapEntry,
    text_delta: isize,
    source_delta: isize,
) -> Result<TextFlowSourceMapEntry, EditError> {
    let mut updated = entry.clone();
    updated.text_start = updated
        .text_start
        .checked_add_signed(
            text_delta
                .try_into()
                .map_err(|_| EditError::InvalidPosition)?,
        )
        .ok_or(EditError::InvalidPosition)?;
    updated.text_end = updated
        .text_end
        .checked_add_signed(
            text_delta
                .try_into()
                .map_err(|_| EditError::InvalidPosition)?,
        )
        .ok_or(EditError::InvalidPosition)?;
    updated.source_range.start = updated
        .source_range
        .start
        .checked_add_signed(
            source_delta
                .try_into()
                .map_err(|_| EditError::InvalidPosition)?,
        )
        .ok_or(EditError::InvalidPosition)?;
    updated.source_range.end = updated
        .source_range
        .end
        .checked_add_signed(
            source_delta
                .try_into()
                .map_err(|_| EditError::InvalidPosition)?,
        )
        .ok_or(EditError::InvalidPosition)?;
    Ok(updated)
}

fn append_text_map_piece(
    output: &mut Vec<TextFlowSourceMapEntry>,
    original: &TextFlowSourceMapEntry,
    display_range: Range<usize>,
    original_source_range: &Range<usize>,
    display_delta: isize,
    source_delta: isize,
) -> Result<(), EditError> {
    if display_range.is_empty() {
        return Ok(());
    }
    if display_range != (original.text_start as usize..original.text_end as usize)
        && (!original.extra.is_empty() || !original.source_range.extra.is_empty())
    {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    let source_start = original_source_range
        .start
        .checked_add(display_range.start - original.text_start as usize)
        .and_then(|offset| offset.checked_add_signed(source_delta))
        .ok_or(EditError::InvalidPosition)?;
    let source_end = original_source_range
        .start
        .checked_add(display_range.end - original.text_start as usize)
        .and_then(|offset| offset.checked_add_signed(source_delta))
        .ok_or(EditError::InvalidPosition)?;
    let text_start = display_range
        .start
        .checked_add_signed(display_delta)
        .ok_or(EditError::InvalidPosition)?;
    let text_end = display_range
        .end
        .checked_add_signed(display_delta)
        .ok_or(EditError::InvalidPosition)?;
    let mut entry = original.clone();
    entry.text_start = u32::try_from(text_start).map_err(|_| EditError::InvalidPosition)?;
    entry.text_end = u32::try_from(text_end).map_err(|_| EditError::InvalidPosition)?;
    entry.source_range.start =
        u32::try_from(source_start).map_err(|_| EditError::InvalidPosition)?;
    entry.source_range.end = u32::try_from(source_end).map_err(|_| EditError::InvalidPosition)?;
    output.push(entry);
    Ok(())
}

pub(super) fn update_math_flow_ranges(
    flow: &mut MathFlow,
    flow_path: &[crate::editing::session::math_edit::MathTextFlowPathStep],
    target_path: &[crate::editing::session::math_edit::MathTextFlowPathStep],
    old_edit: &Range<usize>,
    new_edit: &Range<usize>,
) -> Result<(), EditError> {
    rebase_math_source_range(
        &mut flow.source_range,
        old_edit,
        new_edit,
        is_path_prefix(flow_path, target_path),
    )?;
    for (index, content) in flow.content.iter_mut().enumerate() {
        let mut content_path = flow_path.to_vec();
        content_path.push(crate::editing::session::math_edit::MathTextFlowPathStep::Content(index));
        update_math_content_ranges(content, &content_path, target_path, old_edit, new_edit)?;
    }
    Ok(())
}

fn update_math_content_ranges(
    content: &mut MathContent,
    content_path: &[crate::editing::session::math_edit::MathTextFlowPathStep],
    target_path: &[crate::editing::session::math_edit::MathTextFlowPathStep],
    old_edit: &Range<usize>,
    new_edit: &Range<usize>,
) -> Result<(), EditError> {
    match content {
        MathContent::Text(text) => {
            let target = content_path == target_path;
            rebase_math_source_range(&mut text.source_range, old_edit, new_edit, target)?;
            if !target {
                for entry in &mut text.source_map {
                    rebase_text_map_source_range(
                        &mut entry.source_range,
                        old_edit,
                        new_edit,
                        false,
                    )?;
                }
            }
        }
        MathContent::Math(node) => {
            rebase_math_source_range(
                &mut node.source_range,
                old_edit,
                new_edit,
                is_path_prefix(content_path, target_path),
            )?;
            update_math_node_ranges(node, content_path, target_path, old_edit, new_edit)?;
        }
        MathContent::Environment(environment) => {
            rebase_math_source_range(
                &mut environment.source_range,
                old_edit,
                new_edit,
                is_path_prefix(content_path, target_path),
            )?;
            for (index, child) in environment.children.iter_mut().enumerate() {
                let mut child_path = content_path.to_vec();
                child_path.push(
                    crate::editing::session::math_edit::MathTextFlowPathStep::EnvironmentChild(
                        index,
                    ),
                );
                update_math_content_ranges(child, &child_path, target_path, old_edit, new_edit)?;
            }
        }
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
    }
    Ok(())
}

fn update_math_node_ranges(
    node: &mut MathNode,
    node_path: &[crate::editing::session::math_edit::MathTextFlowPathStep],
    target_path: &[crate::editing::session::math_edit::MathTextFlowPathStep],
    old_edit: &Range<usize>,
    new_edit: &Range<usize>,
) -> Result<(), EditError> {
    for (index, flow) in node.children.iter_mut().enumerate() {
        let mut flow_path = node_path.to_vec();
        flow_path.push(crate::editing::session::math_edit::MathTextFlowPathStep::Operand(index));
        update_math_flow_ranges(flow, &flow_path, target_path, old_edit, new_edit)?;
    }
    Ok(())
}

fn is_path_prefix(
    prefix: &[crate::editing::session::math_edit::MathTextFlowPathStep],
    path: &[crate::editing::session::math_edit::MathTextFlowPathStep],
) -> bool {
    path.starts_with(prefix)
}

fn rebase_math_source_range(
    range: &mut Option<MathSourceRange>,
    old_edit: &Range<usize>,
    new_edit: &Range<usize>,
    contains_edit: bool,
) -> Result<(), EditError> {
    let Some(range) = range else {
        return Ok(());
    };
    let old = range.start as usize..range.end as usize;
    let delta = new_edit.end as isize - old_edit.end as isize;
    let updated = if contains_edit {
        if old.start > old_edit.start || old.end < old_edit.end {
            return Err(EditError::InvalidPosition);
        }
        old.start
            ..old
                .end
                .checked_add_signed(delta)
                .ok_or(EditError::InvalidPosition)?
    } else if old_edit.is_empty() {
        if old.start >= old_edit.start {
            old.start
                .checked_add_signed(delta)
                .ok_or(EditError::InvalidPosition)?
                ..old
                    .end
                    .checked_add_signed(delta)
                    .ok_or(EditError::InvalidPosition)?
        } else if old.end <= old_edit.start {
            old
        } else {
            old.start
                ..old
                    .end
                    .checked_add_signed(delta)
                    .ok_or(EditError::InvalidPosition)?
        }
    } else if old.end <= old_edit.start {
        old
    } else if old.start >= old_edit.end {
        old.start
            .checked_add_signed(delta)
            .ok_or(EditError::InvalidPosition)?
            ..old
                .end
                .checked_add_signed(delta)
                .ok_or(EditError::InvalidPosition)?
    } else {
        return Err(EditError::InvalidPosition);
    };
    range.start = u32::try_from(updated.start).map_err(|_| EditError::InvalidPosition)?;
    range.end = u32::try_from(updated.end).map_err(|_| EditError::InvalidPosition)?;
    Ok(())
}

fn rebase_text_map_source_range(
    range: &mut MathSourceRange,
    old_edit: &Range<usize>,
    new_edit: &Range<usize>,
    contains_edit: bool,
) -> Result<(), EditError> {
    let mut value = Some(range.clone());
    rebase_math_source_range(&mut value, old_edit, new_edit, contains_edit)?;
    *range = value.ok_or(EditError::InvalidPosition)?;
    Ok(())
}
