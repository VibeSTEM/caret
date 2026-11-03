//! Math TextFlow 拆分时迁移 Run 与 sourceMap 元数据。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{MathContent, MathFlow, MathTree, Run, TextFlow};

use super::MathTextFlowPath;
use super::metadata::map_unchanged_range;

pub(super) fn prepare_math_text_split_metadata(
    tree: &mut MathTree,
    path: &MathTextFlowPath,
) -> bool {
    let Some(text) = path.resolve_mut(tree) else {
        return false;
    };
    if !text.extra.is_empty()
        || text
            .source_range
            .as_ref()
            .is_some_and(|range| !range.extra.is_empty())
    {
        return false;
    }
    for run in &mut text.runs {
        run.language = None;
        run.props.clear();
        run.extra.clear();
    }
    for entry in &mut text.source_map {
        entry.extra.clear();
        entry.source_range.extra.clear();
    }
    true
}

pub(super) fn prepare_math_text_format_metadata(
    tree: &mut MathTree,
    path: &MathTextFlowPath,
) -> bool {
    let Some(text) = path.resolve_mut(tree) else {
        return false;
    };
    text.extra.clear();
    if let Some(range) = &mut text.source_range {
        range.extra.clear();
    }
    for run in &mut text.runs {
        run.extra.clear();
    }
    for entry in &mut text.source_map {
        entry.extra.clear();
        entry.source_range.extra.clear();
    }
    true
}

pub(super) fn restore_math_text_split_metadata(
    previous: &MathTree,
    updated: &mut MathTree,
    path: &MathTextFlowPath,
    display_range: &std::ops::Range<usize>,
    old_source_edit: &std::ops::Range<usize>,
    new_source_edit: &std::ops::Range<usize>,
) -> bool {
    let Some(old_text) = path.resolve(previous) else {
        return false;
    };
    let Some((left_runs, right_runs)) = split_text_flow_runs(old_text, display_range) else {
        return false;
    };
    let Some(old_range) = old_text
        .source_range
        .as_ref()
        .map(|range| range.start as usize..range.end as usize)
    else {
        return false;
    };
    let left_source = map_unchanged_range(
        &(old_range.start..old_source_edit.start.min(old_range.end)),
        old_source_edit.start,
        old_source_edit.end,
        new_source_edit.end,
    )
    .filter(|range| range.0 < range.1);
    let right_source = map_unchanged_range(
        &(old_source_edit.end.max(old_range.start)..old_range.end),
        old_source_edit.start,
        old_source_edit.end,
        new_source_edit.end,
    )
    .filter(|range| range.0 < range.1);

    match left_source {
        Some(range) => {
            let Some(mut targets) = find_text_flows_by_source_range_mut(&mut updated.root, &range)
            else {
                return false;
            };
            if targets.len() != 1 {
                return false;
            }
            let Some(target) = targets.pop() else {
                return false;
            };
            target.runs = left_runs;
        }
        None if !left_runs.is_empty() => return false,
        None => {}
    }
    match right_source {
        Some(range) => {
            let Some(mut targets) = find_text_flows_by_source_range_mut(&mut updated.root, &range)
            else {
                return false;
            };
            if targets.len() != 1 {
                return false;
            }
            let Some(target) = targets.pop() else {
                return false;
            };
            target.runs = right_runs;
        }
        None if !right_runs.is_empty() => return false,
        None => {}
    }

    restore_text_flow_source_map_extras(
        old_text,
        updated,
        old_source_edit,
        new_source_edit,
        display_range,
        left_source,
        right_source,
    )
}

pub(super) fn restore_math_text_format_metadata(
    previous: &MathTree,
    updated: &mut MathTree,
    path: &MathTextFlowPath,
    format: &super::text_branch::MathTextFormat,
    old_source_edit: &std::ops::Range<usize>,
    new_source_edit: &std::ops::Range<usize>,
) -> bool {
    let Some(old_text) = path.resolve(previous) else {
        return false;
    };
    let Some(old_range) = old_text
        .source_range
        .as_ref()
        .map(|range| range.start as usize..range.end as usize)
    else {
        return false;
    };
    let Some((left_runs, selected_runs, right_runs)) =
        split_text_flow_runs_for_format(old_text, &format.display_range)
    else {
        return false;
    };
    let left_old = old_range.start..old_range.start + old_source_edit.start;
    let right_old = old_range.start + old_source_edit.end..old_range.end;
    let left_new = map_unchanged_range(
        &left_old,
        old_source_edit.start,
        old_source_edit.end,
        new_source_edit.end,
    )
    .filter(|range| range.0 < range.1);
    let Some(selected_new_start) = old_range.start.checked_add(format.selected_source_start) else {
        return false;
    };
    let Some(selected_new_end) = selected_new_start.checked_add(format.selected_source_len) else {
        return false;
    };
    let selected_new = selected_new_start..selected_new_end;
    let right_new = map_unchanged_range(
        &right_old,
        old_source_edit.start,
        old_source_edit.end,
        new_source_edit.end,
    )
    .filter(|range| range.0 < range.1);

    if left_new.is_none() && !left_runs.is_empty() || right_new.is_none() && !right_runs.is_empty()
    {
        return false;
    }
    let segment_count = 1 + usize::from(left_new.is_some()) + usize::from(right_new.is_some());
    if segment_count > 1
        && (!old_text.extra.is_empty()
            || old_text
                .source_range
                .as_ref()
                .is_some_and(|range| !range.extra.is_empty()))
    {
        return false;
    }

    if let Some(range) = left_new {
        let Some(mut targets) = find_text_flows_by_source_range_mut(&mut updated.root, &range)
        else {
            return false;
        };
        if targets.len() != 1 {
            return false;
        }
        let Some(target) = targets.pop() else {
            return false;
        };
        target.runs = left_runs;
    }
    {
        let Some(mut targets) = find_text_flows_by_source_range_mut(
            &mut updated.root,
            &(selected_new.start, selected_new.end),
        ) else {
            return false;
        };
        if targets.len() != 1 {
            return false;
        }
        let Some(selected) = targets.pop() else {
            return false;
        };
        selected.runs = selected_runs;
        if segment_count == 1 {
            selected.extra.clone_from(&old_text.extra);
            if let (Some(old), Some(new)) = (&old_text.source_range, &mut selected.source_range) {
                new.extra.clone_from(&old.extra);
            }
        }
    }
    if let Some(range) = right_new {
        let Some(mut targets) = find_text_flows_by_source_range_mut(&mut updated.root, &range)
        else {
            return false;
        };
        if targets.len() != 1 {
            return false;
        }
        let Some(target) = targets.pop() else {
            return false;
        };
        target.runs = right_runs;
    }

    restore_format_source_map_extras(FormatSourceMapRestore {
        previous: old_text,
        updated,
        old_source_edit,
        new_source_edit,
        format,
        left_flow_range: left_new,
        selected_flow_range: Some((selected_new.start, selected_new.end)),
        right_flow_range: right_new,
    })
}

fn split_text_flow_runs(
    text: &TextFlow,
    range: &std::ops::Range<usize>,
) -> Option<(Vec<Run>, Vec<Run>)> {
    let length = text.runs.iter().map(|run| run.text.len()).sum::<usize>();
    if range.start > range.end
        || range.end > length
        || !text_flow_boundary(&text.runs, range.start)
        || !text_flow_boundary(&text.runs, range.end)
    {
        return None;
    }
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut offset = 0;
    for run in &text.runs {
        let end = offset + run.text.len();
        if run.text.is_empty() {
            if run.language.is_some() || !run.props.is_empty() || !run.extra.is_empty() {
                if offset >= range.start && offset <= range.end {
                    return None;
                }
                let destination = if offset < range.start {
                    &mut left
                } else {
                    &mut right
                };
                destination.push(run.clone());
            }
            offset = end;
            continue;
        }
        let left_end = end.min(range.start);
        if offset < left_end {
            left.push(run_fragment(run, 0..left_end - offset));
        }
        let right_start = offset.max(range.end);
        if right_start < end {
            right.push(run_fragment(run, right_start - offset..run.text.len()));
        }
        let selected = offset < range.end && range.start < end;
        let split =
            offset < range.start && range.start < end || offset < range.end && range.end < end;
        if !run.extra.is_empty() && (selected || split) {
            return None;
        }
        offset = end;
    }
    Some((left, right))
}

fn split_text_flow_runs_for_format(
    text: &TextFlow,
    range: &std::ops::Range<usize>,
) -> Option<(Vec<Run>, Vec<Run>, Vec<Run>)> {
    let length = text.runs.iter().map(|run| run.text.len()).sum::<usize>();
    if range.start >= range.end
        || range.end > length
        || !text_flow_boundary(&text.runs, range.start)
        || !text_flow_boundary(&text.runs, range.end)
    {
        return None;
    }
    let (mut left, mut selected, mut right) = (Vec::new(), Vec::new(), Vec::new());
    let mut offset = 0;
    for run in &text.runs {
        let end = offset + run.text.len();
        if run.text.is_empty() {
            if offset < range.start {
                left.push(run.clone());
            } else if offset > range.end {
                right.push(run.clone());
            } else if offset > range.start && offset < range.end {
                selected.push(run.clone());
            } else if run.language.is_some() || !run.props.is_empty() || !run.extra.is_empty() {
                return None;
            }
            offset = end;
            continue;
        }

        let mut pieces = Vec::new();
        for (slot, piece) in [
            (0usize, offset..end.min(range.start)),
            (1usize, offset.max(range.start)..end.min(range.end)),
            (2usize, offset.max(range.end)..end),
        ] {
            if piece.start < piece.end {
                pieces.push((slot, piece.start - offset..piece.end - offset));
            }
        }
        if !run.extra.is_empty() && pieces.len() > 1 {
            return None;
        }
        for (slot, piece) in pieces {
            let fragment = run_fragment(run, piece);
            match slot {
                0 => left.push(fragment),
                1 => selected.push(fragment),
                _ => right.push(fragment),
            }
        }
        offset = end;
    }
    Some((left, selected, right))
}

struct FormatSourceMapRestore<'a> {
    previous: &'a TextFlow,
    updated: &'a mut MathTree,
    old_source_edit: &'a std::ops::Range<usize>,
    new_source_edit: &'a std::ops::Range<usize>,
    format: &'a super::text_branch::MathTextFormat,
    left_flow_range: Option<(usize, usize)>,
    selected_flow_range: Option<(usize, usize)>,
    right_flow_range: Option<(usize, usize)>,
}

fn restore_format_source_map_extras(restore: FormatSourceMapRestore<'_>) -> bool {
    let FormatSourceMapRestore {
        previous,
        updated,
        old_source_edit,
        new_source_edit,
        format,
        left_flow_range,
        selected_flow_range,
        right_flow_range,
    } = restore;
    let Some(prefix_len) = format
        .selected_source_start
        .checked_sub(old_source_edit.start)
    else {
        return false;
    };
    for entry in &previous.source_map {
        if entry.extra.is_empty() && entry.source_range.extra.is_empty() {
            continue;
        }
        let display_range = entry.text_start as usize..entry.text_end as usize;
        let (flow_range, display_base, source_range) =
            if display_range.end <= format.display_range.start {
                let range = entry.source_range.start as usize..entry.source_range.end as usize;
                let Some(mapped) = map_unchanged_range(
                    &range,
                    old_source_edit.start,
                    old_source_edit.end,
                    new_source_edit.end,
                ) else {
                    return false;
                };
                (left_flow_range, 0, mapped)
            } else if display_range.start >= format.display_range.end {
                let range = entry.source_range.start as usize..entry.source_range.end as usize;
                let Some(mapped) = map_unchanged_range(
                    &range,
                    old_source_edit.start,
                    old_source_edit.end,
                    new_source_edit.end,
                ) else {
                    return false;
                };
                (right_flow_range, format.display_range.end, mapped)
            } else if format.display_range.start <= display_range.start
                && display_range.end <= format.display_range.end
            {
                let source_start = (entry.source_range.start as usize).checked_add(prefix_len);
                let source_end = (entry.source_range.end as usize).checked_add(prefix_len);
                let (Some(source_start), Some(source_end)) = (source_start, source_end) else {
                    return false;
                };
                (
                    selected_flow_range,
                    format.display_range.start,
                    (source_start, source_end),
                )
            } else {
                return false;
            };
        let Some(flow_range) = flow_range else {
            return false;
        };
        let Some(mut flows) = find_text_flows_by_source_range_mut(&mut updated.root, &flow_range)
        else {
            return false;
        };
        if flows.len() != 1 {
            return false;
        }
        let Some(flow) = flows.pop() else {
            return false;
        };
        let target_start = display_range.start - display_base;
        let target_end = display_range.end - display_base;
        let mut candidates = flow.source_map.iter_mut().filter(|candidate| {
            candidate.text_start as usize == target_start
                && candidate.text_end as usize == target_end
                && candidate.source_range.start as usize == source_range.0
                && candidate.source_range.end as usize == source_range.1
        });
        let Some(target) = candidates.next() else {
            return false;
        };
        if candidates.next().is_some() {
            return false;
        }
        target.extra.clone_from(&entry.extra);
        target
            .source_range
            .extra
            .clone_from(&entry.source_range.extra);
    }
    true
}

fn run_fragment(run: &Run, range: std::ops::Range<usize>) -> Run {
    let mut fragment = run.clone();
    fragment.text = run.text[range].to_owned();
    fragment
}

fn text_flow_boundary(runs: &[Run], position: usize) -> bool {
    let mut offset = 0;
    for run in runs {
        let end = offset + run.text.len();
        if position >= offset && position <= end {
            return run.text.is_char_boundary(position - offset);
        }
        offset = end;
    }
    position == offset
}

fn find_text_flows_by_source_range_mut<'a>(
    flow: &'a mut MathFlow,
    range: &(usize, usize),
) -> Option<Vec<&'a mut TextFlow>> {
    let mut matches = Vec::new();
    collect_text_flows_by_source_range_mut(flow, range, &mut matches);
    (!matches.is_empty()).then_some(matches)
}

fn collect_text_flows_by_source_range_mut<'a>(
    flow: &'a mut MathFlow,
    range: &(usize, usize),
    matches: &mut Vec<&'a mut TextFlow>,
) {
    collect_text_flows_in_contents_mut(&mut flow.content, range, matches);
}

fn collect_text_flows_in_contents_mut<'a>(
    contents: &'a mut [MathContent],
    range: &(usize, usize),
    matches: &mut Vec<&'a mut TextFlow>,
) {
    for content in contents {
        match content {
            MathContent::Text(text)
                if text
                    .source_range
                    .as_ref()
                    .map(|range| (range.start as usize, range.end as usize))
                    == Some(*range) =>
            {
                matches.push(text);
            }
            MathContent::Environment(environment) => {
                collect_text_flows_in_contents_mut(&mut environment.children, range, matches);
            }
            MathContent::Math(node) => {
                for child in &mut node.children {
                    collect_text_flows_by_source_range_mut(child, range, matches);
                }
            }
            MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
        }
    }
}

fn restore_text_flow_source_map_extras(
    previous: &TextFlow,
    updated: &mut MathTree,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
    display_edit: &std::ops::Range<usize>,
    left_flow_range: Option<(usize, usize)>,
    right_flow_range: Option<(usize, usize)>,
) -> bool {
    for entry in &previous.source_map {
        if entry.extra.is_empty() && entry.source_range.extra.is_empty() {
            continue;
        }
        let source_range = entry.source_range.start as usize..entry.source_range.end as usize;
        if source_range.start < old_edit.end && old_edit.start < source_range.end {
            return false;
        }
        let Some(mapped) =
            map_unchanged_range(&source_range, old_edit.start, old_edit.end, new_edit.end)
        else {
            return false;
        };
        let display_range = entry.text_start as usize..entry.text_end as usize;
        let (flow_range, target_range) = if display_range.end <= display_edit.start {
            (left_flow_range, display_range)
        } else if display_range.start >= display_edit.end {
            (
                right_flow_range,
                display_range.start - display_edit.end..display_range.end - display_edit.end,
            )
        } else {
            return false;
        };
        let Some(flow_range) = flow_range else {
            return false;
        };
        let Some(mut targets) = find_text_flows_by_source_range_mut(&mut updated.root, &flow_range)
        else {
            return false;
        };
        if targets.len() != 1 {
            return false;
        }
        let Some(target) = targets.pop() else {
            return false;
        };
        let mut entries = target.source_map.iter_mut().filter(|candidate| {
            candidate.text_start as usize == target_range.start
                && candidate.text_end as usize == target_range.end
                && candidate.source_range.start as usize == mapped.0
                && candidate.source_range.end as usize == mapped.1
        });
        let Some(target_entry) = entries.next() else {
            return false;
        };
        if entries.next().is_some() {
            return false;
        }
        target_entry.extra.clone_from(&entry.extra);
        target_entry
            .source_range
            .extra
            .clone_from(&entry.source_range.extra);
    }
    true
}
