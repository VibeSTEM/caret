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
