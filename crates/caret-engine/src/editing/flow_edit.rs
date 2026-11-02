//! 在宿主提供的物理路径上编辑共享 TextFlow 内容。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::ops::Range;

use caret_doc::{FlowContent, Run, TextFlow};

use super::EditError;
use super::flow_map::FlowPath;

/// 按拼接 Run 文本中的 UTF-8 字节区间替换内容，并保留可表达的 Run 属性。
pub(super) fn replace_text_flow_range(
    text: &mut TextFlow,
    range: Range<usize>,
    inserted: &str,
) -> Result<(), EditError> {
    let length = text.runs.iter().map(|run| run.text.len()).sum::<usize>();
    if range.start > range.end
        || range.end > length
        || !text_boundary(&text.runs, range.start)
        || !text_boundary(&text.runs, range.end)
    {
        return Err(EditError::InvalidPosition);
    }

    if range.is_empty() && inserted.is_empty() {
        return Ok(());
    }
    let insertion = (!inserted.is_empty())
        .then(|| insertion_template_index(&text.runs, range.start))
        .flatten();
    let mut offset = 0;
    for run in &mut text.runs {
        let end = offset + run.text.len();
        let overlap_start = range.start.max(offset);
        let overlap_end = range.end.min(end);
        if overlap_start < overlap_end {
            let local_start = overlap_start - offset;
            let local_end = overlap_end - offset;
            if run.text.get(local_start..local_end).is_none() {
                return Err(EditError::InvalidPosition);
            }
            run.text.replace_range(local_start..local_end, "");
        }
        offset = end;
    }
    if inserted.is_empty() {
        return Ok(());
    }
    if let Some((index, local_offset)) = insertion {
        let run = text.runs.get_mut(index).ok_or(EditError::InvalidPosition)?;
        let local_offset = local_offset.min(run.text.len());
        run.text.insert_str(local_offset, inserted);
    } else {
        text.runs.insert(
            0,
            Run {
                text: inserted.to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            },
        );
    }
    Ok(())
}

/// 规范化改动位置附近兼容的 Run 与相邻 TextFlow。
pub(super) fn normalize_text_flow_neighborhood<T>(
    contents: &mut Vec<FlowContent<T>>,
    index: usize,
    run_index: usize,
) {
    if !matches!(contents.get(index), Some(FlowContent::Text(_))) {
        return;
    }
    if let Some(FlowContent::Text(flow)) = contents.get_mut(index) {
        normalize_runs(&mut flow.runs, run_index);
    }

    let mut current = index;
    while current > 0 && can_merge_text_flows(contents, current - 1, current) {
        let FlowContent::Text(next) = contents.remove(current) else {
            return;
        };
        let FlowContent::Text(previous) = &mut contents[current - 1] else {
            return;
        };
        let boundary = previous.runs.len().saturating_sub(1);
        previous.runs.extend(next.runs);
        current -= 1;
        normalize_runs(&mut previous.runs, boundary);
    }
    while current + 1 < contents.len() && can_merge_text_flows(contents, current, current + 1) {
        let FlowContent::Text(next) = contents.remove(current + 1) else {
            return;
        };
        let FlowContent::Text(current_flow) = &mut contents[current] else {
            return;
        };
        let boundary = current_flow.runs.len().saturating_sub(1);
        current_flow.runs.extend(next.runs);
        normalize_runs(&mut current_flow.runs, boundary);
    }
}

pub(super) fn text_flow_run_at_path_mut<'a, T>(
    contents: &'a mut Vec<FlowContent<T>>,
    path: &FlowPath,
) -> Option<&'a mut Run> {
    let run_index = path.run_index?;
    let (content_index, ancestors) = path.content_indices.split_last()?;
    let siblings = content_list_at_path_mut(contents, ancestors)?;
    let FlowContent::Text(text) = siblings.get_mut(*content_index)? else {
        return None;
    };
    text.runs.get_mut(run_index)
}

pub(super) fn text_flow_at_path_mut<'a, T>(
    contents: &'a mut Vec<FlowContent<T>>,
    path: &FlowPath,
) -> Option<&'a mut TextFlow> {
    let (content_index, ancestors) = path.content_indices.split_last()?;
    let siblings = content_list_at_path_mut(contents, ancestors)?;
    let FlowContent::Text(text) = siblings.get_mut(*content_index)? else {
        return None;
    };
    Some(text)
}

pub(super) fn content_at_path<'a, T>(
    contents: &'a [FlowContent<T>],
    path: &FlowPath,
) -> Option<&'a FlowContent<T>> {
    let (content_index, ancestors) = path.content_indices.split_last()?;
    content_list_at_path(contents, ancestors)?.get(*content_index)
}

pub(super) fn siblings_at_path_mut<'a, T>(
    contents: &'a mut Vec<FlowContent<T>>,
    path: &FlowPath,
) -> Option<(&'a mut Vec<FlowContent<T>>, usize)> {
    let (content_index, ancestors) = path.content_indices.split_last()?;
    Some((
        content_list_at_path_mut(contents, ancestors)?,
        *content_index,
    ))
}

pub(super) fn content_list_at_path<'a, T>(
    contents: &'a [FlowContent<T>],
    ancestors: &[usize],
) -> Option<&'a [FlowContent<T>]> {
    let Some((index, rest)) = ancestors.split_first() else {
        return Some(contents);
    };
    let FlowContent::Environment(environment) = contents.get(*index)? else {
        return None;
    };
    content_list_at_path(&environment.children, rest)
}

pub(super) fn content_list_at_path_mut<'a, T>(
    contents: &'a mut Vec<FlowContent<T>>,
    ancestors: &[usize],
) -> Option<&'a mut Vec<FlowContent<T>>> {
    let Some((index, rest)) = ancestors.split_first() else {
        return Some(contents);
    };
    let FlowContent::Environment(environment) = contents.get_mut(*index)? else {
        return None;
    };
    content_list_at_path_mut(&mut environment.children, rest)
}

fn can_merge_text_flows<T>(contents: &[FlowContent<T>], left: usize, right: usize) -> bool {
    matches!(
        (contents.get(left), contents.get(right)),
        (Some(FlowContent::Text(left)), Some(FlowContent::Text(right)))
            if left.source_range.is_none()
                && right.source_range.is_none()
                && left.source_map.is_empty()
                && right.source_map.is_empty()
                && left.extra.is_empty()
                && right.extra.is_empty()
    )
}

fn normalize_runs(runs: &mut Vec<Run>, index: usize) {
    if runs.is_empty() || index >= runs.len() {
        return;
    }
    let mut current = index;
    while current > 0 {
        if is_empty_default_run(&runs[current - 1]) {
            runs.remove(current - 1);
            current -= 1;
            continue;
        }
        if !runs_can_merge(&runs[current - 1], &runs[current]) {
            break;
        }
        let next_text = runs[current].text.clone();
        runs[current - 1].text.push_str(&next_text);
        runs.remove(current);
        current -= 1;
    }
    while current + 1 < runs.len() {
        if is_empty_default_run(&runs[current + 1]) {
            runs.remove(current + 1);
            continue;
        }
        if !runs_can_merge(&runs[current], &runs[current + 1]) {
            break;
        }
        let next_text = runs[current + 1].text.clone();
        runs[current].text.push_str(&next_text);
        runs.remove(current + 1);
    }
}

fn is_empty_default_run(run: &Run) -> bool {
    run.text.is_empty() && run.language.is_none() && run.props.is_empty() && run.extra.is_empty()
}

fn runs_can_merge(left: &Run, right: &Run) -> bool {
    left.language == right.language
        && left.props == right.props
        && left.extra.is_empty()
        && right.extra.is_empty()
}

pub(super) fn insertion_template(runs: &[Run], position: usize) -> Option<&Run> {
    insertion_template_index(runs, position).and_then(|(index, _)| runs.get(index))
}

fn insertion_template_index(runs: &[Run], position: usize) -> Option<(usize, usize)> {
    let mut offset = 0;
    let mut preceding = None;
    for (index, run) in runs.iter().enumerate() {
        let end = offset + run.text.len();
        if run.text.is_empty() && offset == position {
            return Some((index, 0));
        }
        if offset <= position && position < end {
            return Some((index, position - offset));
        }
        if end == position {
            preceding = Some((index, run.text.len()));
        }
        offset = end;
    }
    preceding
}

fn text_boundary(runs: &[Run], position: usize) -> bool {
    let mut offset = 0;
    for run in runs {
        if position >= offset && position <= offset + run.text.len() {
            return run.text.is_char_boundary(position - offset);
        }
        offset += run.text.len();
    }
    position == offset
}
