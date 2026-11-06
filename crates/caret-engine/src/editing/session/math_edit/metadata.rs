//! 在公式源码编辑后匹配未变节点，保留稳定 ID 与未知字段。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::collections::{HashMap, HashSet, VecDeque};

use caret_doc::{Align, Block, MathContent, MathFlow, MathNode, MathTree, TextFlow};
use caret_math::{MathContentSourceMap, MathFlowSourceMap, MathSourceMap, MathTreeSourceMap};

pub(super) fn preserve_display_math_metadata(
    original: &Block,
    updated: &mut Block,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
    previous_source: &MathSourceMap<'_>,
    updated_source: &MathSourceMap<'_>,
) -> bool {
    let mut candidate = updated.clone();
    if !preserve_display_math_metadata_inner(
        original,
        &mut candidate,
        old_edit,
        new_edit,
        previous_source,
        updated_source,
    ) {
        return false;
    }
    *updated = candidate;
    true
}

fn preserve_display_math_metadata_inner(
    original: &Block,
    updated: &mut Block,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
    previous_source: &MathSourceMap<'_>,
    updated_source: &MathSourceMap<'_>,
) -> bool {
    match (original, updated) {
        (Block::Equation(original), Block::Equation(updated)) => {
            updated.extra.clone_from(&original.extra);
            let (Some(previous_map), Some(updated_map)) =
                (previous_source.tree(), updated_source.tree())
            else {
                return false;
            };
            let (Some(old_edit), Some(new_edit)) = (
                edit_range_in_source(previous_source, old_edit),
                edit_range_in_source(updated_source, new_edit),
            ) else {
                return false;
            };
            preserve_math_tree_metadata_for_edit(
                &original.math_tree,
                &mut updated.math_tree,
                &old_edit,
                &new_edit,
                previous_map,
                updated_map,
            )
        }
        (Block::Align(original), Block::Align(updated)) => {
            let Some(old_full_edit) = edit_range_in_source(previous_source, old_edit) else {
                return false;
            };
            let Some(new_full_edit) = edit_range_in_source(updated_source, new_edit) else {
                return false;
            };
            preserve_align_metadata(
                original,
                updated,
                &old_full_edit,
                &new_full_edit,
                previous_source,
                updated_source,
            )
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
    previous_source: &MathSourceMap<'_>,
    updated_source: &MathSourceMap<'_>,
) -> bool {
    let mut candidate = updated.clone();
    if !preserve_align_metadata_inner(
        previous,
        &mut candidate,
        old_edit,
        new_edit,
        previous_source,
        updated_source,
    ) {
        return false;
    }
    *updated = candidate;
    true
}

fn preserve_align_metadata_inner(
    previous: &Align,
    updated: &mut Align,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
    previous_source: &MathSourceMap<'_>,
    updated_source: &MathSourceMap<'_>,
) -> bool {
    let (Some(previous_align_map), Some(updated_align_map)) =
        (previous_source.align(), updated_source.align())
    else {
        return false;
    };
    // 暂无按编辑映射迁移扩展的接口，拒绝未知归属以免扩展落到其他节点。
    if previous.rows.iter().enumerate().any(|(row_index, row)| {
        row.cells.iter().enumerate().any(|(cell_index, _)| {
            previous_align_map
                .cell_tree(row_index, cell_index)
                .is_none_or(|map| flow_map_has_extra(map.root()))
        })
    }) || updated.rows.iter().enumerate().any(|(row_index, row)| {
        row.cells.iter().enumerate().any(|(cell_index, _)| {
            updated_align_map
                .cell_tree(row_index, cell_index)
                .is_none_or(|map| flow_map_has_extra(map.root()))
        })
    }) {
        return false;
    }
    updated.extra.clone_from(&previous.extra);
    let Some(mut previous_cells) = align_cell_source_ranges(previous, previous_source) else {
        return false;
    };
    let Some(mut updated_cells) = align_cell_source_ranges(updated, updated_source) else {
        return false;
    };
    let Some(mut previous_navigation_cells) = align_cell_navigation_ranges(previous_source) else {
        return false;
    };
    let Some(mut updated_navigation_cells) = align_cell_navigation_ranges(updated_source) else {
        return false;
    };
    let previous_base = previous_source.edit_range().start;
    let updated_base = updated_source.edit_range().start;
    if !shift_cell_ranges(&mut previous_cells, previous_base)
        || !shift_cell_ranges(&mut updated_cells, updated_base)
        || !shift_cell_ranges(&mut previous_navigation_cells, previous_base)
        || !shift_cell_ranges(&mut updated_navigation_cells, updated_base)
    {
        return false;
    }
    let (Some(old_start), Some(old_end), Some(new_start), Some(new_end)) = (
        old_edit.start.checked_sub(previous_base),
        old_edit.end.checked_sub(previous_base),
        new_edit.start.checked_sub(updated_base),
        new_edit.end.checked_sub(updated_base),
    ) else {
        return false;
    };
    let old_edit = old_start..old_end;
    let new_edit = new_start..new_end;
    let previous_rows = align_row_navigation_ranges(&previous_navigation_cells);
    let updated_rows = align_row_navigation_ranges(&updated_navigation_cells);
    let prefix_end = old_edit.start;
    let previous_tail = old_edit.end;
    let updated_tail = new_edit.end;

    let changed_previous_rows = previous_rows
        .iter()
        .enumerate()
        .filter(|(_, range)| range_intersects_edit(range, &old_edit))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let changed_updated_rows = updated_rows
        .iter()
        .filter(|range| range_intersects_edit(range, &new_edit))
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
        &old_edit,
        &new_edit,
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
    let mut matched_updated_cells = HashSet::new();
    for (row_index, row) in updated.rows.iter_mut().enumerate() {
        for (cell_index, cell) in row.cells.iter_mut().enumerate() {
            let range = &updated_cells[row_index][cell_index];
            let Some(candidates) = old_cells_by_mapped_range.get_mut(&(range.start, range.end))
            else {
                continue;
            };
            while let Some((old_row, old_cell)) = candidates.pop_front() {
                let old_source = previous_source
                    .edit_source()
                    .get(previous_cells[old_row][old_cell].clone());
                let new_source = updated_source
                    .edit_source()
                    .get(updated_cells[row_index][cell_index].clone());
                if old_source.is_some()
                    && old_source == new_source
                    && matched_cells.insert((old_row, old_cell))
                {
                    matched_updated_cells.insert((row_index, cell_index));
                    let old_cell_range = &previous_cells[old_row][old_cell];
                    let old_local_edit = old_cell_range.end..old_cell_range.end;
                    let new_local_edit = updated_cells[row_index][cell_index].len()
                        ..updated_cells[row_index][cell_index].len();
                    let Some(old_absolute) =
                        offset_range(&old_local_edit, previous_base + old_cell_range.start)
                    else {
                        return false;
                    };
                    let Some(new_absolute) = offset_range(
                        &new_local_edit,
                        updated_base + updated_cells[row_index][cell_index].start,
                    ) else {
                        return false;
                    };
                    let (Some(old_map), Some(new_map)) = (
                        previous_align_map.cell_tree(old_row, old_cell),
                        updated_align_map.cell_tree(row_index, cell_index),
                    ) else {
                        return false;
                    };
                    metadata_preserved &= preserve_math_tree_metadata_for_edit(
                        &previous.rows[old_row].cells[old_cell],
                        cell,
                        &old_absolute,
                        &new_absolute,
                        old_map,
                        new_map,
                    );
                    break;
                }
            }
        }
    }

    let old_changed_cells = changed_cell_candidates(&previous_navigation_cells, &old_edit);
    // 空 cell 的语义范围可能是注释之后的零宽点；导航范围才能将注释前的编辑归回该 cell。
    let new_changed_cells = changed_cell_candidates(&updated_navigation_cells, &new_edit);
    if old_changed_cells.len() == 1 && new_changed_cells.len() == 1 {
        let (old_row, old_cell) = old_changed_cells[0];
        let (new_row, new_cell) = new_changed_cells[0];
        if !matched_cells.contains(&(old_row, old_cell))
            && !matched_updated_cells.contains(&(new_row, new_cell))
        {
            let old_cell_range = &previous_cells[old_row][old_cell];
            let new_cell_range = &updated_cells[new_row][new_cell];
            let old_local_edit = local_edit_range(&old_edit, old_cell_range, old_cell_range.len());
            let new_local_edit = local_edit_range(&new_edit, new_cell_range, new_cell_range.len());
            let Some(old_absolute) =
                offset_range(&old_local_edit, previous_base + old_cell_range.start)
            else {
                return false;
            };
            let Some(new_absolute) =
                offset_range(&new_local_edit, updated_base + new_cell_range.start)
            else {
                return false;
            };
            let (Some(old_map), Some(new_map)) = (
                previous_align_map.cell_tree(old_row, old_cell),
                updated_align_map.cell_tree(new_row, new_cell),
            ) else {
                return false;
            };
            let preserved = preserve_math_tree_metadata_for_edit(
                &previous.rows[old_row].cells[old_cell],
                &mut updated.rows[new_row].cells[new_cell],
                &old_absolute,
                &new_absolute,
                old_map,
                new_map,
            );
            metadata_preserved &= preserved;
            if preserved {
                matched_cells.insert((old_row, old_cell));
                matched_updated_cells.insert((new_row, new_cell));
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
pub(super) fn align_cell_source_ranges(
    align: &Align,
    source: &MathSourceMap<'_>,
) -> Option<Vec<Vec<std::ops::Range<usize>>>> {
    let source_map = source.align()?;
    if align.rows.len() != source_map.cell_navigation_ranges().len() {
        return None;
    }
    align
        .rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            if row.cells.len() != source_map.cell_navigation_ranges()[row_index].len() {
                return None;
            }
            (0..row.cells.len())
                .map(|cell_index| source_map.cell_range(row_index, cell_index))
                .collect()
        })
        .collect()
}

fn align_cell_navigation_ranges(
    source: &MathSourceMap<'_>,
) -> Option<Vec<Vec<std::ops::Range<usize>>>> {
    Some(source.align()?.cell_navigation_ranges().to_vec())
}

fn shift_cell_ranges(cells: &mut [Vec<std::ops::Range<usize>>], base: usize) -> bool {
    cells.iter_mut().flatten().all(|range| {
        let (Some(start), Some(end)) = (range.start.checked_sub(base), range.end.checked_sub(base))
        else {
            return false;
        };
        range.start = start;
        range.end = end;
        true
    })
}

fn edit_range_in_source(
    source: &MathSourceMap<'_>,
    edit: &std::ops::Range<usize>,
) -> Option<std::ops::Range<usize>> {
    if edit.start > edit.end || edit.end > source.edit_source().len() {
        return None;
    }
    Some(
        source.edit_range().start.checked_add(edit.start)?
            ..source.edit_range().start.checked_add(edit.end)?,
    )
}

fn offset_range(range: &std::ops::Range<usize>, base: usize) -> Option<std::ops::Range<usize>> {
    Some(base.checked_add(range.start)?..base.checked_add(range.end)?)
}

fn align_row_navigation_ranges(
    cells: &[Vec<std::ops::Range<usize>>],
) -> Vec<std::ops::Range<usize>> {
    cells
        .iter()
        .map(|row| {
            let start = row.iter().map(|range| range.start).min().unwrap_or(0);
            let end = row.iter().map(|range| range.end).max().unwrap_or(start);
            start..end
        })
        .collect()
}

pub(super) fn map_unchanged_range(
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

/// 按未变源码范围匹配节点，并在可唯一对应时沿祖先路径恢复身份；未知字段无法无歧义迁移时返回 `false`。
#[cfg(test)]
pub(super) fn preserve_math_tree_metadata(previous: &MathTree, updated: &mut MathTree) -> bool {
    let previous_source = caret_math::serialize_tree(previous);
    let updated_source = caret_math::serialize_tree(updated);
    let (Ok(old), Ok(new)) = (
        caret_math::parse_projection(&previous_source, caret_math::MathInputKind::Inline),
        caret_math::parse_projection(&updated_source, caret_math::MathInputKind::Inline),
    ) else {
        return false;
    };
    let (Some(old_map), Some(new_map)) = (old.source_map().tree(), new.source_map().tree()) else {
        return false;
    };
    let (prefix_end, previous_tail, updated_tail) =
        unchanged_source_boundaries(&previous_source, &updated_source);
    preserve_math_tree_metadata_for_edit(
        previous,
        updated,
        &(prefix_end..previous_tail),
        &(prefix_end..updated_tail),
        old_map,
        new_map,
    )
}

/// 根据完整源码的绝对编辑范围匹配持久节点；映射须由调用方验证其树版本与源码。
/// 无法确定扩展归属时拒绝修改，并保持 `updated` 不变。
pub(super) fn preserve_math_tree_metadata_for_edit(
    previous: &MathTree,
    updated: &mut MathTree,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
    previous_map: &MathTreeSourceMap,
    updated_map: &MathTreeSourceMap,
) -> bool {
    if old_edit.start > old_edit.end
        || new_edit.start > new_edit.end
        || flow_map_has_extra(previous_map.root())
        || flow_map_has_extra(updated_map.root())
    {
        return false;
    }
    let mut candidate = updated.clone();
    candidate.extra.clone_from(&previous.extra);
    if !preserve_math_flow_metadata(
        &previous.root,
        &mut candidate.root,
        previous_map.root(),
        updated_map.root(),
        old_edit,
        new_edit,
    ) {
        return false;
    }
    *updated = candidate;
    true
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
    previous_map: &MathFlowSourceMap,
    updated_map: &MathFlowSourceMap,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
) -> bool {
    updated.id.clone_from(&previous.id);
    updated.extra.clone_from(&previous.extra);
    preserve_math_content_list_metadata(
        &previous.content,
        &mut updated.content,
        previous_map.content(),
        updated_map.content(),
        old_edit,
        new_edit,
    )
}

fn preserve_math_content_list_metadata(
    previous: &[MathContent],
    updated: &mut [MathContent],
    previous_map: &[MathContentSourceMap],
    updated_map: &[MathContentSourceMap],
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
) -> bool {
    if previous.len() != previous_map.len() || updated.len() != updated_map.len() {
        return false;
    }
    let mut metadata_preserved = true;
    let mut matched_previous = HashSet::new();
    let mut previous_by_mapped_range = HashMap::<(usize, usize), VecDeque<usize>>::new();
    let mut previous_containing_edit = Vec::new();
    for (index, map) in previous_map.iter().enumerate() {
        if let Some(range) = map.range() {
            if let Some(mapped) = map_tree_range(&range, old_edit, new_edit) {
                previous_by_mapped_range
                    .entry(mapped)
                    .or_default()
                    .push_back(index);
            }
            if range.start <= old_edit.start && range.end >= old_edit.end {
                previous_containing_edit.push(index);
            }
        }
    }
    for (child, map) in updated.iter_mut().zip(updated_map) {
        let mapped_match = map.range().and_then(|range| {
            let candidates = previous_by_mapped_range.get_mut(&(range.start, range.end))?;
            let match_index = candidates.iter().position(|index| {
                !matched_previous.contains(index) && content_kind_matches(&previous[*index], child)
            })?;
            let index = candidates.remove(match_index)?;
            Some(index)
        });
        let containing_match = map.range().and_then(|range| {
            if range.start > new_edit.start || range.end < new_edit.end {
                return None;
            }
            let mut candidates = previous_containing_edit.iter().filter(|index| {
                !matched_previous.contains(*index)
                    && content_kind_matches(&previous[**index], child)
            });
            let index = *candidates.next()?;
            candidates.next().is_none().then_some(index)
        });
        if let Some(index) = mapped_match.or(containing_match) {
            matched_previous.insert(index);
            metadata_preserved &= preserve_math_content_metadata(
                &previous[index],
                child,
                &previous_map[index],
                map,
                old_edit,
                new_edit,
            );
        }
    }
    for (index, child) in previous.iter().enumerate() {
        if !matched_previous.contains(&index) && content_has_metadata(child) {
            metadata_preserved = false;
        }
    }
    metadata_preserved
}

fn preserve_math_content_metadata(
    previous: &MathContent,
    updated: &mut MathContent,
    previous_map: &MathContentSourceMap,
    updated_map: &MathContentSourceMap,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
) -> bool {
    match (previous, updated, previous_map, updated_map) {
        (
            MathContent::Math(previous),
            MathContent::Math(updated),
            MathContentSourceMap::Math {
                operands: old_operands,
                ..
            },
            MathContentSourceMap::Math {
                operands: new_operands,
                ..
            },
        ) => preserve_math_node_metadata(
            previous,
            updated,
            old_operands,
            new_operands,
            old_edit,
            new_edit,
        ),
        (
            MathContent::Text(previous),
            MathContent::Text(updated),
            MathContentSourceMap::Text(_),
            MathContentSourceMap::Text(_),
        ) => {
            let same_text = previous
                .runs
                .iter()
                .flat_map(|run| run.text.chars())
                .eq(updated.runs.iter().flat_map(|run| run.text.chars()));
            if same_text {
                updated.runs.clone_from(&previous.runs);
                updated.extra.clone_from(&previous.extra);
                true
            } else {
                previous.extra.is_empty()
                    && previous.runs.iter().all(|run| {
                        run.language.is_none() && run.props.is_empty() && run.extra.is_empty()
                    })
            }
        }
        (
            MathContent::Environment(previous),
            MathContent::Environment(updated),
            MathContentSourceMap::Environment {
                children: old_children,
                ..
            },
            MathContentSourceMap::Environment {
                children: new_children,
                ..
            },
        ) if previous.kind == updated.kind => {
            updated.id.clone_from(&previous.id);
            updated.extra.clone_from(&previous.extra);
            updated.props.clone_from(&previous.props);
            updated.definition_id.clone_from(&previous.definition_id);
            preserve_math_content_list_metadata(
                &previous.children,
                &mut updated.children,
                old_children,
                new_children,
                old_edit,
                new_edit,
            )
        }
        (
            MathContent::SoftBreak(previous),
            MathContent::SoftBreak(updated),
            MathContentSourceMap::Other,
            MathContentSourceMap::Other,
        ) => {
            updated.extra.clone_from(&previous.extra);
            true
        }
        (
            MathContent::Opaque(_),
            MathContent::Opaque(_),
            MathContentSourceMap::Other,
            MathContentSourceMap::Other,
        ) => true,
        _ => false,
    }
}

fn preserve_math_node_metadata(
    previous: &MathNode,
    updated: &mut MathNode,
    previous_map: &[MathFlowSourceMap],
    updated_map: &[MathFlowSourceMap],
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
) -> bool {
    if previous.children.len() != previous_map.len() || updated.children.len() != updated_map.len()
    {
        return false;
    }
    let mut preserved = true;
    if previous.kind == updated.kind {
        updated.id.clone_from(&previous.id);
        updated.extra.clone_from(&previous.extra);
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
    for ((old, old_map), (new, new_map)) in previous
        .children
        .iter()
        .zip(previous_map)
        .zip(updated.children.iter_mut().zip(updated_map))
    {
        preserved &= preserve_math_flow_metadata(old, new, old_map, new_map, old_edit, new_edit);
    }
    preserved
}

fn map_tree_range(
    range: &std::ops::Range<usize>,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
) -> Option<(usize, usize)> {
    let (from, to) = if range.end <= old_edit.start
        && !(range.is_empty() && range.start == old_edit.start && old_edit.is_empty())
    {
        (old_edit.start, new_edit.start)
    } else if range.start >= old_edit.end
        && !(range.is_empty() && range.start == old_edit.end && old_edit.is_empty())
    {
        (old_edit.end, new_edit.end)
    } else {
        return None;
    };
    let translate = |position: usize| {
        if to >= from {
            position.checked_add(to - from)
        } else {
            position.checked_sub(from - to)
        }
    };
    Some((translate(range.start)?, translate(range.end)?))
}

fn flow_map_has_extra(flow: &MathFlowSourceMap) -> bool {
    !flow.range_extra().is_empty() || flow.content().iter().any(content_map_has_extra)
}

fn content_map_has_extra(content: &MathContentSourceMap) -> bool {
    match content {
        MathContentSourceMap::Text(text) => {
            !text.range_extra().is_empty()
                || text.segments().iter().any(|segment| {
                    !segment.extra().is_empty() || !segment.source_extra().is_empty()
                })
        }
        MathContentSourceMap::Math {
            range, operands, ..
        } => !range.extra().is_empty() || operands.iter().any(flow_map_has_extra),
        MathContentSourceMap::Environment { range, children } => {
            !range.extra().is_empty() || children.iter().any(content_map_has_extra)
        }
        MathContentSourceMap::Other => false,
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
                || text.runs.iter().any(|run| {
                    run.language.is_some() || !run.props.is_empty() || !run.extra.is_empty()
                })
        }
        MathContent::Math(node) => node_has_metadata(node),
        MathContent::Environment(environment) => {
            environment.definition_id.is_some()
                || !environment.props.is_empty()
                || !environment.extra.is_empty()
                || environment.children.iter().any(content_has_metadata)
        }
        MathContent::SoftBreak(value) => !value.extra.is_empty(),
        MathContent::Opaque(_) => true,
    }
}
fn flow_has_metadata(flow: &MathFlow) -> bool {
    !flow.extra.is_empty() || flow.content.iter().any(content_has_metadata)
}

fn tree_has_metadata(tree: &MathTree) -> bool {
    !tree.extra.is_empty() || flow_has_metadata(&tree.root)
}

/// 检查降级为源码文本时无法由正文 Run 或其宿主包装表达的字段。
pub(super) fn can_materialize_source_as_text(tree: &MathTree) -> bool {
    let Some(parsed) = crate::math_validation::parse_tree_with_same_semantics(
        tree,
        caret_math::MathInputKind::Inline,
    ) else {
        return false;
    };
    tree.extra.is_empty() && flow_source_is_textual(&tree.root, &parsed.root)
}

fn flow_source_is_textual(flow: &MathFlow, parsed: &MathFlow) -> bool {
    flow.extra.is_empty() && content_sequence_source_is_textual(&flow.content, &parsed.content)
}

fn content_sequence_source_is_textual(contents: &[MathContent], parsed: &[MathContent]) -> bool {
    let (mut content_index, mut parsed_index) = (0, 0);
    while content_index < contents.len() && parsed_index < parsed.len() {
        if matches!(contents[content_index], MathContent::Text(_))
            && matches!(parsed[parsed_index], MathContent::Text(_))
        {
            let content_end = text_sequence_end(contents, content_index);
            let parsed_end = text_sequence_end(parsed, parsed_index);
            let content_sequence = &contents[content_index..content_end];
            let parsed_sequence = &parsed[parsed_index..parsed_end];
            if !content_sequence.iter().all(|content| match content {
                MathContent::Text(text) => text_flow_is_plain(text),
                _ => false,
            }) || !parsed_sequence.iter().all(|content| match content {
                MathContent::Text(text) => text_flow_is_plain(text),
                _ => false,
            }) || text_sequence(content_sequence) != text_sequence(parsed_sequence)
            {
                return false;
            }
            content_index = content_end;
            parsed_index = parsed_end;
            continue;
        }
        if !content_source_is_textual(&contents[content_index], &parsed[parsed_index]) {
            return false;
        }
        content_index += 1;
        parsed_index += 1;
    }
    content_index == contents.len() && parsed_index == parsed.len()
}

fn text_sequence_end(contents: &[MathContent], start: usize) -> usize {
    start
        + contents[start..]
            .iter()
            .take_while(|content| matches!(content, MathContent::Text(_)))
            .count()
}

fn text_sequence(contents: &[MathContent]) -> String {
    contents
        .iter()
        .filter_map(|content| match content {
            MathContent::Text(text) => Some(
                text.runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

fn content_source_is_textual(content: &MathContent, parsed: &MathContent) -> bool {
    match (content, parsed) {
        (MathContent::Text(text), MathContent::Text(_)) => text_flow_is_plain(text),
        (MathContent::Math(node), MathContent::Math(parsed_node)) => {
            node.extra.is_empty()
                && node.children.len() == parsed_node.children.len()
                && node
                    .children
                    .iter()
                    .zip(&parsed_node.children)
                    .all(|(flow, parsed)| flow_source_is_textual(flow, parsed))
        }
        (MathContent::Environment(environment), MathContent::Environment(parsed_environment)) => {
            environment.definition_id.is_none()
                && environment.props == parsed_environment.props
                && environment.extra.is_empty()
                && content_sequence_source_is_textual(
                    &environment.children,
                    &parsed_environment.children,
                )
        }
        (MathContent::SoftBreak(value), MathContent::SoftBreak(_)) => value.extra.is_empty(),
        (MathContent::Opaque(_), _) | (_, MathContent::Opaque(_)) => false,
        _ => false,
    }
}

fn text_flow_is_plain(text: &TextFlow) -> bool {
    text.extra.is_empty()
        && text
            .runs
            .iter()
            .all(|run| run.language.is_none() && run.props.is_empty() && run.extra.is_empty())
}

fn row_has_metadata(row: &caret_doc::MathAlignRow) -> bool {
    !row.extra.is_empty() || row.cells.iter().any(tree_has_metadata)
}

fn node_has_metadata(node: &MathNode) -> bool {
    !node.extra.is_empty() || node.children.iter().any(flow_has_metadata)
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

#[cfg(test)]
mod tests {
    use super::preserve_math_tree_metadata_for_edit;
    use caret_doc::MathContent;
    use caret_math::{MathInputKind, ProjectedMath, parse_projection};

    #[test]
    fn metadata_follows_unchanged_source_after_insertion() {
        let (ProjectedMath::Tree(mut previous), old_map) =
            parse_projection("a+a", MathInputKind::Inline)
                .unwrap_or_else(|_| panic!("valid previous formula"))
                .into_parts()
        else {
            panic!("expected tree")
        };
        let (ProjectedMath::Tree(mut updated), new_map) =
            parse_projection("a+a+a", MathInputKind::Inline)
                .unwrap_or_else(|_| panic!("valid updated formula"))
                .into_parts()
        else {
            panic!("expected tree")
        };
        let MathContent::Math(original) = &mut previous.root.content[0] else {
            panic!("expected node")
        };
        original.extra.insert("vendor".into(), serde_json::json!(1));
        let original_id = original.id.clone();
        assert!(preserve_math_tree_metadata_for_edit(
            &previous,
            &mut updated,
            &(0..0),
            &(0..2),
            old_map.tree().unwrap_or_else(|| panic!("old map")),
            new_map.tree().unwrap_or_else(|| panic!("new map")),
        ));
        let MathContent::Math(original) = &updated.root.content[2] else {
            panic!("expected node")
        };
        assert_eq!(original.id, original_id);
        assert_eq!(original.extra["vendor"], 1);
    }

    #[test]
    fn rejected_metadata_mapping_keeps_candidate_unchanged() {
        let (ProjectedMath::Tree(mut previous), old_map) =
            parse_projection("a+b", MathInputKind::Inline)
                .unwrap_or_else(|_| panic!("valid previous formula"))
                .into_parts()
        else {
            panic!("expected tree")
        };
        let source = r"\frac{a}{b}";
        let (ProjectedMath::Tree(mut updated), new_map) =
            parse_projection(source, MathInputKind::Inline)
                .unwrap_or_else(|_| panic!("valid updated formula"))
                .into_parts()
        else {
            panic!("expected tree")
        };
        let MathContent::Math(original) = &mut previous.root.content[0] else {
            panic!("expected node")
        };
        original.extra.insert("vendor".into(), serde_json::json!(1));
        let before = updated.clone();
        assert!(!preserve_math_tree_metadata_for_edit(
            &previous,
            &mut updated,
            &(0..3),
            &(0..source.len()),
            old_map.tree().unwrap_or_else(|| panic!("old map")),
            new_map.tree().unwrap_or_else(|| panic!("new map")),
        ));
        assert_eq!(updated, before);
    }

    #[test]
    fn align_cell_metadata_survives_shift_in_complete_source() {
        let old = r"\begin{aligned}a&b\end{aligned}";
        let new = r"\begin{aligned}aa&b\end{aligned}";
        let (ProjectedMath::Align(mut previous), old_map) =
            parse_projection(old, MathInputKind::Align)
                .unwrap_or_else(|_| panic!("valid previous align"))
                .into_parts()
        else {
            panic!("expected align")
        };
        let (ProjectedMath::Align(mut updated), new_map) =
            parse_projection(new, MathInputKind::Align)
                .unwrap_or_else(|_| panic!("valid updated align"))
                .into_parts()
        else {
            panic!("expected align")
        };
        let previous_tree = &mut previous.rows[0].cells[1];
        let MathContent::Math(original) = &mut previous_tree.root.content[0] else {
            panic!("expected node")
        };
        original.extra.insert("vendor".into(), serde_json::json!(1));
        let original_id = original.id.clone();
        let old_cell = old_map.align().unwrap_or_else(|| panic!("old align map"));
        let new_cell = new_map.align().unwrap_or_else(|| panic!("new align map"));
        let old_end = old_cell
            .cell_range(0, 1)
            .unwrap_or_else(|| panic!("old cell"))
            .end;
        let new_end = new_cell
            .cell_range(0, 1)
            .unwrap_or_else(|| panic!("new cell"))
            .end;
        assert!(preserve_math_tree_metadata_for_edit(
            previous_tree,
            &mut updated.rows[0].cells[1],
            &(old_end..old_end),
            &(new_end..new_end),
            old_cell
                .cell_tree(0, 1)
                .unwrap_or_else(|| panic!("old tree map")),
            new_cell
                .cell_tree(0, 1)
                .unwrap_or_else(|| panic!("new tree map")),
        ));
        let MathContent::Math(original) = &updated.rows[0].cells[1].root.content[0] else {
            panic!("expected node")
        };
        assert_eq!(original.id, original_id);
        assert_eq!(original.extra["vendor"], 1);
    }
}
