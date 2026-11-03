//! 将 Math TextFlow 选区提升为同级数学节点的直接树事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::EditError;
use super::super::path::content_slot_mut;
use super::super::{
    MathTextFlowPath,
    text_run_split::{split_text_flow_runs, text_flow_boundary},
};
use super::source_map_edit::{rebase_text_map_entry, update_math_flow_ranges};
use caret_doc::{
    MathContent, MathFlow, MathNode, MathSourceRange, Run, TextFlow, TextFlowSourceMapEntry,
};
use std::ops::Range;

#[derive(Default)]
struct SplitSourceMap {
    left: Vec<TextFlowSourceMapEntry>,
    right: Vec<TextFlowSourceMapEntry>,
}

/// 以解析后的插入内容替换文本岛选区，并保留可无损归属的两侧 Run 与映射。
pub(in crate::editing::session::math_edit) fn apply_math_text_split_tree_edit(
    tree: &mut caret_doc::MathTree,
    path: &MathTextFlowPath,
    display_range: Range<usize>,
    source_edit: Range<usize>,
    replacement: &str,
    inserted_contents: &[MathContent],
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
    let display_len = original
        .runs
        .iter()
        .map(|run| run.text.len())
        .sum::<usize>();
    if display_range.start > display_range.end
        || display_range.end > display_len
        || !text_flow_boundary(&original.runs, display_range.start)
        || !text_flow_boundary(&original.runs, display_range.end)
        || source_edit.start < old_range.start
        || source_edit.end > old_range.end
        || source_edit.start > source_edit.end
        || (display_range.is_empty() != source_edit.is_empty())
    {
        return Err(EditError::InvalidPosition);
    }
    if !original.extra.is_empty()
        || original
            .source_range
            .as_ref()
            .is_some_and(|range| !range.extra.is_empty())
    {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    let (left_runs, right_runs) = split_text_flow_runs(&original, &display_range)
        .ok_or(EditError::UnsupportedOpaqueContent)?;
    let delta = replacement.len() as isize - source_edit.len() as isize;
    let new_source_end = source_edit
        .start
        .checked_add(replacement.len())
        .ok_or(EditError::InvalidPosition)?;
    let right_source_end = old_range
        .end
        .checked_add_signed(delta)
        .ok_or(EditError::InvalidPosition)?;
    let has_left = !left_runs.is_empty() || old_range.start < source_edit.start;
    let has_right =
        !right_runs.is_empty() || source_edit.end < old_range.end || display_range.is_empty();
    let source_map = split_source_map(&original, &display_range, delta, has_left, has_right)?;

    let mut contents = Vec::with_capacity(
        usize::from(has_left) + inserted_contents.len() + usize::from(has_right),
    );
    if has_left {
        contents.push(MathContent::Text(text_flow_fragment(
            &original,
            left_runs,
            old_range.start..source_edit.start,
            source_map.left,
        )?));
    }
    let inserted_source_start = source_edit
        .start
        .checked_add(1)
        .ok_or(EditError::InvalidPosition)?;
    for content in inserted_contents {
        let mut content = content.clone();
        shift_content_source_ranges(&mut content, inserted_source_start)?;
        contents.push(content);
    }
    if inserted_contents.is_empty() && replacement == "$$" {
        let mut placeholder = MathContent::Math(MathNode {
            id: caret_doc::NodeId::generate(),
            kind: caret_doc::MathNodeKind::Placeholder,
            text: None,
            attributes: Default::default(),
            source_range: Some(MathSourceRange {
                start: 0,
                end: 0,
                extra: Default::default(),
            }),
            children: Vec::new(),
            extra: Default::default(),
        });
        shift_content_source_ranges(&mut placeholder, inserted_source_start)?;
        contents.push(placeholder);
    }
    if has_right {
        contents.push(MathContent::Text(text_flow_fragment(
            &original,
            right_runs,
            new_source_end..right_source_end,
            source_map.right,
        )?));
    }

    update_math_flow_ranges(
        &mut tree.root,
        &[],
        path.steps(),
        &source_edit,
        &(source_edit.start..new_source_end),
    )?;
    let (parent, index) =
        content_slot_mut(&mut tree.root, path.steps()).ok_or(EditError::InvalidPosition)?;
    parent.splice(index..index + 1, contents);
    Ok(())
}

fn split_source_map(
    original: &TextFlow,
    display_range: &Range<usize>,
    source_delta: isize,
    has_left: bool,
    has_right: bool,
) -> Result<SplitSourceMap, EditError> {
    let mut split = SplitSourceMap::default();
    for entry in &original.source_map {
        let text_range = entry.text_start as usize..entry.text_end as usize;
        let source_range = entry.source_range.start as usize..entry.source_range.end as usize;
        if text_range.is_empty() {
            if display_range.is_empty() && text_range.start == display_range.start && has_right {
                let mapped = rebase_text_map_entry(
                    entry,
                    -isize::try_from(display_range.end).map_err(|_| EditError::InvalidPosition)?,
                    source_delta,
                )?;
                split.right.push(mapped);
            } else if text_range.start <= display_range.start && has_left {
                split.left.push(entry.clone());
            } else if text_range.start >= display_range.end && has_right {
                let mapped = rebase_text_map_entry(
                    entry,
                    -isize::try_from(display_range.end).map_err(|_| EditError::InvalidPosition)?,
                    source_delta,
                )?;
                split.right.push(mapped);
            } else if !entry.extra.is_empty() || !entry.source_range.extra.is_empty() {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            continue;
        }

        let overlap = text_range.start < display_range.end && display_range.start < text_range.end;
        if !overlap {
            if text_range.end <= display_range.start {
                split.left.push(entry.clone());
            } else if text_range.start >= display_range.end {
                let mapped = rebase_text_map_entry(
                    entry,
                    -isize::try_from(display_range.end).map_err(|_| EditError::InvalidPosition)?,
                    source_delta,
                )?;
                split.right.push(mapped);
            }
            continue;
        }

        if !entry.extra.is_empty() || !entry.source_range.extra.is_empty() {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let linear = text_range.len() == source_range.len();
        if !linear && (display_range.start > text_range.start || display_range.end < text_range.end)
        {
            return Err(EditError::InvalidPosition);
        }
        if linear && text_range.start < display_range.start {
            append_map_piece(
                &mut split.left,
                entry,
                text_range.start..display_range.start,
                &source_range,
                0,
                0,
            )?;
        }
        if linear && display_range.end < text_range.end {
            append_map_piece(
                &mut split.right,
                entry,
                display_range.end..text_range.end,
                &source_range,
                -isize::try_from(display_range.end).map_err(|_| EditError::InvalidPosition)?,
                source_delta,
            )?;
        }
    }

    Ok(split)
}

fn append_map_piece(
    target: &mut Vec<TextFlowSourceMapEntry>,
    entry: &TextFlowSourceMapEntry,
    display_piece: Range<usize>,
    source_range: &Range<usize>,
    display_shift: isize,
    source_shift: isize,
) -> Result<(), EditError> {
    let text_range = entry.text_start as usize..entry.text_end as usize;
    let source_start = if text_range.len() == source_range.len() {
        source_range.start + display_piece.start - text_range.start
    } else {
        source_range.start
    };
    let source_end = if text_range.len() == source_range.len() {
        source_range.start + display_piece.end - text_range.start
    } else {
        source_range.end
    };
    let mut mapped = entry.clone();
    mapped.text_start = u32::try_from(
        display_piece
            .start
            .checked_add_signed(display_shift)
            .ok_or(EditError::InvalidPosition)?,
    )
    .map_err(|_| EditError::InvalidPosition)?;
    mapped.text_end = u32::try_from(
        display_piece
            .end
            .checked_add_signed(display_shift)
            .ok_or(EditError::InvalidPosition)?,
    )
    .map_err(|_| EditError::InvalidPosition)?;
    mapped.source_range.start = u32::try_from(
        source_start
            .checked_add_signed(source_shift)
            .ok_or(EditError::InvalidPosition)?,
    )
    .map_err(|_| EditError::InvalidPosition)?;
    mapped.source_range.end = u32::try_from(
        source_end
            .checked_add_signed(source_shift)
            .ok_or(EditError::InvalidPosition)?,
    )
    .map_err(|_| EditError::InvalidPosition)?;
    target.push(mapped);
    Ok(())
}

fn text_flow_fragment(
    original: &TextFlow,
    mut runs: Vec<Run>,
    source_range: Range<usize>,
    mut source_map: Vec<TextFlowSourceMapEntry>,
) -> Result<TextFlow, EditError> {
    if runs.is_empty() {
        if source_range.start != source_range.end {
            return Err(EditError::InvalidPosition);
        }
        runs.push(Run {
            text: String::new(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        });
        if source_map.is_empty() {
            let source_offset =
                u32::try_from(source_range.start).map_err(|_| EditError::InvalidPosition)?;
            source_map.push(TextFlowSourceMapEntry {
                text_start: 0,
                text_end: 0,
                source_range: MathSourceRange {
                    start: source_offset,
                    end: source_offset,
                    extra: Default::default(),
                },
                extra: Default::default(),
            });
        }
    }
    let mut fragment = original.clone();
    fragment.runs = runs;
    fragment.source_range = Some(MathSourceRange {
        start: u32::try_from(source_range.start).map_err(|_| EditError::InvalidPosition)?,
        end: u32::try_from(source_range.end).map_err(|_| EditError::InvalidPosition)?,
        extra: Default::default(),
    });
    fragment.source_map = source_map;
    Ok(fragment)
}

fn shift_content_source_ranges(content: &mut MathContent, offset: usize) -> Result<(), EditError> {
    match content {
        MathContent::Text(text) => {
            shift_source_range(&mut text.source_range, offset)?;
            for entry in &mut text.source_map {
                entry.source_range.start = entry
                    .source_range
                    .start
                    .checked_add(u32::try_from(offset).map_err(|_| EditError::InvalidPosition)?)
                    .ok_or(EditError::InvalidPosition)?;
                entry.source_range.end = entry
                    .source_range
                    .end
                    .checked_add(u32::try_from(offset).map_err(|_| EditError::InvalidPosition)?)
                    .ok_or(EditError::InvalidPosition)?;
            }
        }
        MathContent::Environment(environment) => {
            shift_source_range(&mut environment.source_range, offset)?;
            for child in &mut environment.children {
                shift_content_source_ranges(child, offset)?;
            }
        }
        MathContent::Math(node) => shift_math_node_source_ranges(node, offset)?,
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
    }
    Ok(())
}

fn shift_math_flow_source_ranges(flow: &mut MathFlow, offset: usize) -> Result<(), EditError> {
    shift_source_range(&mut flow.source_range, offset)?;
    for content in &mut flow.content {
        shift_content_source_ranges(content, offset)?;
    }
    Ok(())
}

fn shift_math_node_source_ranges(node: &mut MathNode, offset: usize) -> Result<(), EditError> {
    shift_source_range(&mut node.source_range, offset)?;
    for child in &mut node.children {
        shift_math_flow_source_ranges(child, offset)?;
    }
    Ok(())
}

fn shift_source_range(range: &mut Option<MathSourceRange>, offset: usize) -> Result<(), EditError> {
    let Some(range) = range else {
        return Ok(());
    };
    let offset = u32::try_from(offset).map_err(|_| EditError::InvalidPosition)?;
    range.start = range
        .start
        .checked_add(offset)
        .ok_or(EditError::InvalidPosition)?;
    range.end = range
        .end
        .checked_add(offset)
        .ok_or(EditError::InvalidPosition)?;
    Ok(())
}
