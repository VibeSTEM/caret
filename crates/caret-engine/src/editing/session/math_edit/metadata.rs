//! 在公式源码编辑后匹配未变节点，保留稳定 ID 与未知字段。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::collections::{HashMap, HashSet, VecDeque};

use caret_doc::{Align, Block, MathContent, MathFlow, MathNode, MathTree};

pub(super) fn preserve_display_math_metadata(
    original: &Block,
    updated: &mut Block,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
) -> bool {
    match (original, updated) {
        (Block::Equation(original), Block::Equation(updated)) => {
            updated.extra.clone_from(&original.extra);
            preserve_math_tree_metadata_for_edit(
                &original.math_tree,
                &mut updated.math_tree,
                old_edit,
                new_edit,
            )
        }
        (Block::Align(original), Block::Align(updated)) => {
            let prefix_len = super::ALIGN_BEGIN.len();
            let old_full_edit = old_edit.start + prefix_len..old_edit.end + prefix_len;
            let new_full_edit = new_edit.start + prefix_len..new_edit.end + prefix_len;
            preserve_align_metadata(original, updated, &old_full_edit, &new_full_edit)
        }
        _ => false,
    }
}

/// 用源码位置匹配 Align 行和单元格，避免相同内容的单元格因顺序变化而交换身份。
pub(super) fn preserve_align_metadata(
    previous: &Align,
    updated: &mut Align,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
) -> bool {
    updated.extra.clone_from(&previous.extra);
    let previous_cells = align_cell_source_ranges(previous);
    let updated_cells = align_cell_source_ranges(updated);
    let previous_rows = align_row_source_ranges(&previous_cells);
    let updated_rows = align_row_source_ranges(&updated_cells);
    let prefix_end = old_edit.start;
    let previous_tail = old_edit.end;
    let updated_tail = new_edit.end;

    let changed_previous_rows = previous_rows
        .iter()
        .enumerate()
        .filter(|(_, range)| range_intersects_edit(range, old_edit))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let changed_updated_rows = updated_rows
        .iter()
        .filter(|range| range_intersects_edit(range, new_edit))
        .count();
    if (previous.rows.len() != updated.rows.len()
        || changed_previous_rows.len() != changed_updated_rows)
        && changed_previous_rows
            .iter()
            .any(|index| row_has_metadata(&previous.rows[*index]))
    {
        return false;
    }

    let mut old_rows_by_mapped_range = HashMap::<(usize, usize), VecDeque<usize>>::new();
    for (row_index, range) in previous_rows.iter().enumerate() {
        if let Some(mapped) = map_unchanged_range(range, prefix_end, previous_tail, updated_tail) {
            old_rows_by_mapped_range
                .entry(mapped)
                .or_default()
                .push_back(row_index);
        }
    }
    let mut row_mapping = vec![None; updated.rows.len()];
    let mut matched_rows = HashSet::new();
    for (row_index, range) in updated_rows.iter().enumerate() {
        let Some(candidates) = old_rows_by_mapped_range.get_mut(&(range.start, range.end)) else {
            continue;
        };
        while let Some(previous_index) = candidates.pop_front() {
            if matched_rows.insert(previous_index) {
                row_mapping[row_index] = Some(previous_index);
                break;
            }
        }
    }
    map_changed_rows(
        &previous_rows,
        &updated_rows,
        old_edit,
        new_edit,
        &mut row_mapping,
        &mut matched_rows,
    );
    for (row_index, new_row) in updated.rows.iter_mut().enumerate() {
        if let Some(old_row) = row_mapping[row_index].and_then(|index| previous.rows.get(index)) {
            new_row.id.clone_from(&old_row.id);
            new_row.extra.clone_from(&old_row.extra);
        }
    }
    let mut metadata_preserved = previous
        .rows
        .iter()
        .enumerate()
        .all(|(index, row)| matched_rows.contains(&index) || !row_has_metadata(row));

    let mut old_cells_by_mapped_range = HashMap::<(usize, usize), VecDeque<(usize, usize)>>::new();
    for (row_index, row) in previous_cells.iter().enumerate() {
        for (cell_index, range) in row.iter().enumerate() {
            if let Some(mapped) =
                map_unchanged_range(range, prefix_end, previous_tail, updated_tail)
            {
                old_cells_by_mapped_range
                    .entry(mapped)
                    .or_default()
                    .push_back((row_index, cell_index));
            }
        }
    }
    let mut matched_cells = HashSet::new();
    for (row_index, row) in updated.rows.iter_mut().enumerate() {
        for (cell_index, cell) in row.cells.iter_mut().enumerate() {
            let range = &updated_cells[row_index][cell_index];
            let Some(candidates) = old_cells_by_mapped_range.get_mut(&(range.start, range.end))
            else {
                continue;
            };
            while let Some((old_row, old_cell)) = candidates.pop_front() {
                if previous.rows[old_row].cells[old_cell].source == cell.source
                    && matched_cells.insert((old_row, old_cell))
                {
                    let old_cell_range = &previous_cells[old_row][old_cell];
                    let old_local_edit = old_cell_range.end..old_cell_range.end;
                    let new_local_edit = cell.source.len()..cell.source.len();
                    metadata_preserved &= preserve_math_tree_metadata_for_edit(
                        &previous.rows[old_row].cells[old_cell],
                        cell,
                        &old_local_edit,
                        &new_local_edit,
                    );
                    break;
                }
            }
        }
    }

    let old_changed_cells = changed_cell_candidates(&previous_cells, old_edit);
    let new_changed_cells = changed_cell_candidates(&updated_cells, new_edit);
    if old_changed_cells.len() == 1 && new_changed_cells.len() == 1 {
        let (old_row, old_cell) = old_changed_cells[0];
        let (new_row, new_cell) = new_changed_cells[0];
        if !matched_cells.contains(&(old_row, old_cell)) {
            let old_cell_range = &previous_cells[old_row][old_cell];
            let new_cell_range = &updated_cells[new_row][new_cell];
            let old_local_edit = local_edit_range(
                old_edit,
                old_cell_range,
                previous.rows[old_row].cells[old_cell].source.len(),
            );
            let new_local_edit = local_edit_range(
                new_edit,
                new_cell_range,
                updated.rows[new_row].cells[new_cell].source.len(),
            );
            let preserved = preserve_math_tree_metadata_for_edit(
                &previous.rows[old_row].cells[old_cell],
                &mut updated.rows[new_row].cells[new_cell],
                &old_local_edit,
                &new_local_edit,
            );
            metadata_preserved &= preserved;
            if preserved {
                matched_cells.insert((old_row, old_cell));
            }
        }
    }
    for (row_index, row) in previous.rows.iter().enumerate() {
        for (cell_index, cell) in row.cells.iter().enumerate() {
            if !matched_cells.contains(&(row_index, cell_index)) && tree_has_metadata(cell) {
                metadata_preserved = false;
            }
        }
    }
    metadata_preserved
}

/// 按树内单元格源码的文档顺序，在 Align 原始源码中定位其字节范围。
fn align_cell_source_ranges(align: &Align) -> Vec<Vec<std::ops::Range<usize>>> {
    let source = &align.source;
    let body_start = source
        .find("\\begin{aligned}")
        .map_or(0, |start| start + "\\begin{aligned}".len());
    let body_end = source[body_start..]
        .find("\\end{aligned}")
        .map_or(source.len(), |offset| body_start + offset);
    let mut parsed_rows = Vec::<Vec<std::ops::Range<usize>>>::new();
    let mut row = Vec::new();
    let mut cell_start = body_start;
    let mut cursor = body_start;
    let mut brace_depth = 0usize;
    let mut environment_depth = 0usize;
    while cursor < body_end {
        let remaining = &source[cursor..body_end];
        let Some(character) = remaining.chars().next() else {
            break;
        };
        if character == '%' {
            cursor += remaining
                .find('\n')
                .map_or(remaining.len(), |newline| newline + 1);
            continue;
        }
        if character == '\\' {
            if brace_depth == 0 && environment_depth == 0 && remaining.starts_with("\\\\") {
                row.push(cell_start..cursor);
                parsed_rows.push(std::mem::take(&mut row));
                cursor += 2;
                cell_start = cursor;
                continue;
            }
            if let Some((command, after_command)) = align_control_word(remaining)
                && matches!(command, "begin" | "end")
                && let Some((name, after_group)) = control_group(&remaining[after_command..])
            {
                if command == "begin" {
                    environment_depth += 1;
                } else {
                    environment_depth = environment_depth.saturating_sub(1);
                }
                cursor += after_command + after_group;
                let _ = name;
                continue;
            }
            let Some(escaped) = remaining[1..].chars().next() else {
                break;
            };
            if escaped.is_ascii_alphabetic() {
                let word_end = remaining
                    .char_indices()
                    .skip(1)
                    .take_while(|(_, ch)| ch.is_ascii_alphabetic())
                    .last()
                    .map_or(1, |(index, ch)| index + ch.len_utf8());
                cursor += word_end;
            } else {
                cursor += 1 + escaped.len_utf8();
            }
            continue;
        }
        match character {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '&' if brace_depth == 0 && environment_depth == 0 => {
                row.push(cell_start..cursor);
                cursor += 1;
                cell_start = cursor;
                continue;
            }
            _ => {}
        }
        cursor += character.len_utf8();
    }
    row.push(cell_start..body_end);
    parsed_rows.push(row);

    align
        .rows
        .iter()
        .zip(parsed_rows)
        .map(|(row, ranges)| {
            row.cells
                .iter()
                .zip(ranges)
                .map(|(cell, range)| {
                    if cell.source.is_empty() {
                        return range.start..range.start;
                    }
                    source[range.clone()]
                        .find(&cell.source)
                        .map(|offset| {
                            let start = range.start + offset;
                            start..start + cell.source.len()
                        })
                        .unwrap_or(range)
                })
                .collect()
        })
        .collect()
}

fn align_control_word(source: &str) -> Option<(&str, usize)> {
    let tail = source.strip_prefix('\\')?;
    let length = tail.bytes().take_while(u8::is_ascii_alphabetic).count();
    (length > 0).then(|| (&tail[..length], length + 1))
}

fn control_group(source: &str) -> Option<(&str, usize)> {
    let source = source.trim_start();
    let skipped = source.len() - source.trim_start().len();
    let body = source.strip_prefix('{')?;
    let close = body.find('}')?;
    Some((&body[..close], skipped + close + 2))
}

fn align_row_source_ranges(cells: &[Vec<std::ops::Range<usize>>]) -> Vec<std::ops::Range<usize>> {
    cells
        .iter()
        .map(|row| match (row.first(), row.last()) {
            (Some(first), Some(last)) => first.start..last.end,
            _ => 0..0,
        })
        .collect()
}

fn map_unchanged_range(
    range: &std::ops::Range<usize>,
    prefix_end: usize,
    previous_tail: usize,
    updated_tail: usize,
) -> Option<(usize, usize)> {
    if range.end <= prefix_end
        && !(range.is_empty() && range.start == prefix_end && prefix_end == previous_tail)
    {
        return Some((range.start, range.end));
    }
    if range.start >= previous_tail
        && !(range.is_empty() && range.start == previous_tail && prefix_end == previous_tail)
    {
        let shift = updated_tail as isize - previous_tail as isize;
        return Some((
            range.start.checked_add_signed(shift)?,
            range.end.checked_add_signed(shift)?,
        ));
    }
    None
}

fn changed_cell_candidates(
    cells: &[Vec<std::ops::Range<usize>>],
    edit: &std::ops::Range<usize>,
) -> Vec<(usize, usize)> {
    cells
        .iter()
        .enumerate()
        .flat_map(|(row, cells)| {
            cells
                .iter()
                .enumerate()
                .filter(|(_, range)| {
                    if edit.is_empty() {
                        range.start <= edit.start && edit.start <= range.end
                    } else {
                        range.start < edit.end && edit.start < range.end
                    }
                })
                .map(move |(cell, _)| (row, cell))
        })
        .collect()
}

fn map_changed_rows(
    previous: &[std::ops::Range<usize>],
    updated: &[std::ops::Range<usize>],
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
    mapping: &mut [Option<usize>],
    matched_previous: &mut HashSet<usize>,
) {
    let old_candidates = previous
        .iter()
        .enumerate()
        .filter(|(index, range)| {
            !matched_previous.contains(index)
                && range.start <= old_edit.start
                && range.end >= old_edit.end
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut updated_candidates = updated
        .iter()
        .enumerate()
        .filter(|(index, range)| {
            mapping[*index].is_none() && range.start <= new_edit.start && range.end >= new_edit.end
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if updated_candidates.is_empty()
        && let Some((index, _)) = updated
            .iter()
            .enumerate()
            .filter(|(index, range)| mapping[*index].is_none() && range.end <= new_edit.start)
            .max_by_key(|(_, range)| range.end)
    {
        updated_candidates.push(index);
    }
    if updated_candidates.is_empty()
        && let Some((index, _)) = updated
            .iter()
            .enumerate()
            .filter(|(index, range)| mapping[*index].is_none() && range.start >= new_edit.end)
            .min_by_key(|(_, range)| range.start)
    {
        updated_candidates.push(index);
    }
    for (old_index, updated_index) in old_candidates.into_iter().zip(updated_candidates) {
        mapping[updated_index] = Some(old_index);
        matched_previous.insert(old_index);
    }
}

/// 仅当未变源码范围唯一对应时复用节点身份；覆盖编辑范围的节点沿用解析器生成的新元数据。
/// 通过源码范围匹配并恢复节点元数据；存在未匹配的未知字段时返回 `false`。
#[cfg(test)]
pub(super) fn preserve_math_tree_metadata(previous: &MathTree, updated: &mut MathTree) -> bool {
    let (prefix_end, previous_tail, updated_tail) =
        unchanged_source_boundaries(&previous.source, &updated.source);
    preserve_math_tree_metadata_for_edit(
        previous,
        updated,
        &(prefix_end..previous_tail),
        &(prefix_end..updated_tail),
    )
}

pub(super) fn preserve_math_tree_metadata_for_edit(
    previous: &MathTree,
    updated: &mut MathTree,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
) -> bool {
    updated.extra.clone_from(&previous.extra);
    preserve_math_flow_metadata(
        &previous.root,
        &mut updated.root,
        old_edit.start,
        old_edit.end,
        new_edit.end,
    )
}

fn range_intersects_edit(range: &std::ops::Range<usize>, edit: &std::ops::Range<usize>) -> bool {
    if edit.is_empty() {
        range.start <= edit.start && edit.start <= range.end
    } else {
        range.start < edit.end && edit.start < range.end
    }
}

fn local_edit_range(
    edit: &std::ops::Range<usize>,
    content: &std::ops::Range<usize>,
    content_len: usize,
) -> std::ops::Range<usize> {
    edit.start.saturating_sub(content.start).min(content_len)
        ..edit.end.saturating_sub(content.start).min(content_len)
}

fn preserve_math_flow_metadata(
    previous: &MathFlow,
    updated: &mut MathFlow,
    prefix_end: usize,
    previous_tail: usize,
    updated_tail: usize,
) -> bool {
    let mut metadata_preserved = true;
    updated.id.clone_from(&previous.id);
    updated.extra.clone_from(&previous.extra);
    if let (Some(previous_range), Some(updated_range)) =
        (&previous.source_range, &mut updated.source_range)
    {
        updated_range.extra.clone_from(&previous_range.extra);
    } else if previous
        .source_range
        .as_ref()
        .is_some_and(|range| !range.extra.is_empty())
    {
        metadata_preserved = false;
    }
    let mut matched_previous = HashSet::new();
    let mut previous_by_mapped_range = HashMap::<(usize, usize), VecDeque<usize>>::new();
    let mut previous_containing_edit = Vec::new();
    for (index, candidate) in previous.content.iter().enumerate() {
        if let Some((start, end)) =
            mapped_content_range(candidate, prefix_end, previous_tail, updated_tail)
        {
            previous_by_mapped_range
                .entry((start, end))
                .or_default()
                .push_back(index);
        }
        if content_source_range(candidate).is_some_and(|range| {
            range.start as usize <= prefix_end && range.end as usize >= previous_tail
        }) {
            previous_containing_edit.push(index);
        }
    }
    for child in &mut updated.content {
        let mapped_match = content_source_range(child).and_then(|range| {
            let expected = (range.start as usize, range.end as usize);
            let candidates = previous_by_mapped_range.get_mut(&expected)?;
            let match_index = candidates.iter().position(|index| {
                !matched_previous.contains(index)
                    && content_kind_matches(&previous.content[*index], child)
            })?;
            let index = candidates.remove(match_index)?;
            Some((index, &previous.content[index]))
        });
        let containing_match = content_source_range(child).and_then(|range| {
            let start = range.start as usize;
            let end = range.end as usize;
            if start > prefix_end || end < updated_tail {
                return None;
            }
            let mut candidates = previous_containing_edit.iter().filter(|index| {
                !matched_previous.contains(*index)
                    && content_kind_matches(&previous.content[**index], child)
            });
            let index = *candidates.next()?;
            candidates
                .next()
                .is_none()
                .then_some((index, &previous.content[index]))
        });
        if let Some((index, matched)) = mapped_match.or(containing_match) {
            matched_previous.insert(index);
            metadata_preserved &= preserve_math_content_metadata(
                matched,
                child,
                prefix_end,
                previous_tail,
                updated_tail,
            );
        }
    }
    for (index, child) in previous.content.iter().enumerate() {
        if !matched_previous.contains(&index) && content_has_metadata(child) {
            metadata_preserved = false;
        }
    }
    metadata_preserved
}

fn preserve_math_content_metadata(
    previous: &MathContent,
    updated: &mut MathContent,
    prefix_end: usize,
    previous_tail: usize,
    updated_tail: usize,
) -> bool {
    match (previous, updated) {
        (MathContent::Math(previous), MathContent::Math(updated)) => {
            preserve_math_node_metadata(previous, updated, prefix_end, previous_tail, updated_tail)
        }
        (MathContent::Text(previous), MathContent::Text(updated)) => {
            let same_text = previous
                .runs
                .iter()
                .flat_map(|run| run.text.chars())
                .eq(updated.runs.iter().flat_map(|run| run.text.chars()));
            if same_text {
                updated.runs.clone_from(&previous.runs);
                updated.extra.clone_from(&previous.extra);
                if let Some(old) = &previous.source_range {
                    let Some(new) = &mut updated.source_range else {
                        return false;
                    };
                    new.extra.clone_from(&old.extra);
                }
                true
            } else {
                previous.extra.is_empty()
                    && previous
                        .source_range
                        .as_ref()
                        .is_none_or(|range| range.extra.is_empty())
                    && previous.runs.iter().all(|run| {
                        run.language.is_none() && run.props.is_empty() && run.extra.is_empty()
                    })
            }
        }
        (MathContent::Environment(previous), MathContent::Environment(updated))
            if previous.kind == updated.kind =>
        {
            updated.id.clone_from(&previous.id);
            updated.extra.clone_from(&previous.extra);
            updated.props.clone_from(&previous.props);
            updated.definition_id.clone_from(&previous.definition_id);
            if let (Some(old), Some(new)) = (&previous.source_range, &mut updated.source_range) {
                new.extra.clone_from(&old.extra);
            }
            (previous
                .source_range
                .as_ref()
                .is_none_or(|range| range.extra.is_empty())
                || updated.source_range.is_some())
                && previous.children.len() == updated.children.len()
                && previous
                    .children
                    .iter()
                    .zip(&mut updated.children)
                    .all(|(old, new)| {
                        preserve_math_content_metadata(
                            old,
                            new,
                            prefix_end,
                            previous_tail,
                            updated_tail,
                        )
                    })
        }
        (MathContent::SoftBreak(previous), MathContent::SoftBreak(updated)) => {
            updated.extra.clone_from(&previous.extra);
            true
        }
        (MathContent::Opaque(_), MathContent::Opaque(_)) => true,
        _ => !content_has_metadata(previous),
    }
}

fn preserve_math_node_metadata(
    previous: &MathNode,
    updated: &mut MathNode,
    prefix_end: usize,
    previous_tail: usize,
    updated_tail: usize,
) -> bool {
    let mut preserved = true;
    if previous.kind == updated.kind {
        updated.id.clone_from(&previous.id);
        updated.extra.clone_from(&previous.extra);
        if let (Some(old), Some(new)) = (&previous.source_range, &mut updated.source_range) {
            new.extra.clone_from(&old.extra);
        } else if previous
            .source_range
            .as_ref()
            .is_some_and(|range| !range.extra.is_empty())
        {
            preserved = false;
        }
    } else if !previous.extra.is_empty() {
        preserved = false;
    }
    if previous.children.len() != updated.children.len() {
        return preserved
            && previous
                .children
                .iter()
                .all(|child| !flow_has_metadata(child));
    }
    for (old, new) in previous.children.iter().zip(&mut updated.children) {
        preserved &= preserve_math_flow_metadata(old, new, prefix_end, previous_tail, updated_tail);
    }
    preserved
}

fn content_source_range(content: &MathContent) -> Option<&caret_doc::MathSourceRange> {
    match content {
        MathContent::Text(text) => text.source_range.as_ref(),
        MathContent::Math(node) => node.source_range.as_ref(),
        MathContent::Environment(environment) => environment.source_range.as_ref(),
        _ => None,
    }
}
fn mapped_content_range(
    content: &MathContent,
    prefix_end: usize,
    previous_tail: usize,
    updated_tail: usize,
) -> Option<(usize, usize)> {
    let range = content_source_range(content)?;
    let (start, end) = (range.start as usize, range.end as usize);
    if end <= prefix_end {
        Some((start, end))
    } else if start >= previous_tail {
        let shift = updated_tail as isize - previous_tail as isize;
        Some((
            start.checked_add_signed(shift)?,
            end.checked_add_signed(shift)?,
        ))
    } else {
        None
    }
}
fn content_kind_matches(left: &MathContent, right: &MathContent) -> bool {
    match (left, right) {
        (MathContent::Text(_), MathContent::Text(_))
        | (MathContent::Environment(_), MathContent::Environment(_))
        | (MathContent::SoftBreak(_), MathContent::SoftBreak(_))
        | (MathContent::Opaque(_), MathContent::Opaque(_)) => true,
        (MathContent::Math(left), MathContent::Math(right)) => left.kind == right.kind,
        _ => false,
    }
}
fn content_has_metadata(content: &MathContent) -> bool {
    match content {
        MathContent::Text(text) => {
            !text.extra.is_empty()
                || text
                    .source_range
                    .as_ref()
                    .is_some_and(|range| !range.extra.is_empty())
                || text.runs.iter().any(|run| {
                    run.language.is_some() || !run.props.is_empty() || !run.extra.is_empty()
                })
        }
        MathContent::Math(node) => node_has_metadata(node),
        MathContent::Environment(environment) => {
            environment.definition_id.is_some()
                || !environment.props.is_empty()
                || !environment.extra.is_empty()
                || environment
                    .source_range
                    .as_ref()
                    .is_some_and(|range| !range.extra.is_empty())
                || environment.children.iter().any(content_has_metadata)
        }
        MathContent::SoftBreak(value) => !value.extra.is_empty(),
        MathContent::Opaque(_) => true,
    }
}
fn flow_has_metadata(flow: &MathFlow) -> bool {
    !flow.extra.is_empty()
        || flow
            .source_range
            .as_ref()
            .is_some_and(|range| !range.extra.is_empty())
        || flow.content.iter().any(content_has_metadata)
}

fn tree_has_metadata(tree: &MathTree) -> bool {
    !tree.extra.is_empty() || flow_has_metadata(&tree.root)
}

fn row_has_metadata(row: &caret_doc::MathAlignRow) -> bool {
    !row.extra.is_empty() || row.cells.iter().any(tree_has_metadata)
}

fn node_has_metadata(node: &MathNode) -> bool {
    !node.extra.is_empty()
        || node
            .source_range
            .as_ref()
            .is_some_and(|range| !range.extra.is_empty())
        || node.children.iter().any(flow_has_metadata)
}

#[cfg(test)]
pub(super) fn unchanged_source_boundaries(previous: &str, updated: &str) -> (usize, usize, usize) {
    let mut prefix_end = 0;
    for (old, new) in previous.chars().zip(updated.chars()) {
        if old != new {
            break;
        }
        prefix_end += old.len_utf8();
    }
    let mut previous_tail = previous.len();
    let mut updated_tail = updated.len();
    let mut old_chars = previous[prefix_end..].chars().rev();
    let mut new_chars = updated[prefix_end..].chars().rev();
    loop {
        match (old_chars.next(), new_chars.next()) {
            (Some(old), Some(new)) if old == new => {
                previous_tail -= old.len_utf8();
                updated_tail -= new.len_utf8();
            }
            _ => break,
        }
    }
    (prefix_end, previous_tail, updated_tail)
}
