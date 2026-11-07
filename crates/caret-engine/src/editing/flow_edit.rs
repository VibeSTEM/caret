//! 在宿主提供的物理路径上编辑共享 TextFlow 内容。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::ops::Range;

use caret_doc::{FlowContent, Run, TextFlow};

use super::EditError;
use super::flow_map::FlowPath;
use super::position::Affinity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RangeSplit {
    Left,
    Right,
    Inside(usize),
}

/// 分类按源顺序切开节点，切点上的零长度节点归入左侧。
pub(super) fn classify_range_split(start: usize, end: usize, cut: usize) -> RangeSplit {
    if end <= cut {
        RangeSplit::Left
    } else if start >= cut {
        RangeSplit::Right
    } else {
        RangeSplit::Inside(cut - start)
    }
}

/// 在 UTF-8 字节边界拆分一个 Run；调用方保留宿主自己的空 Run 归属与事务策略。
pub(super) fn split_run_at(run: &Run, offset: usize) -> Result<(Run, Run), EditError> {
    if !run.extra.is_empty() {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    if offset == 0 || offset >= run.text.len() || !run.text.is_char_boundary(offset) {
        return Err(EditError::InvalidPosition);
    }
    let mut left = run.clone();
    let right_text = left.text.split_off(offset);
    let mut right = run.clone();
    right.text = right_text;
    Ok((left, right))
}

pub(super) fn run_fragment(run: &Run, range: Range<usize>) -> Result<Run, EditError> {
    if range.start > range.end
        || range.end > run.text.len()
        || !run.text.is_char_boundary(range.start)
        || !run.text.is_char_boundary(range.end)
    {
        return Err(EditError::InvalidPosition);
    }
    if (range.start > 0 || range.end < run.text.len()) && !run.extra.is_empty() {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    let mut fragment = run.clone();
    fragment.text = run
        .text
        .get(range)
        .ok_or(EditError::InvalidPosition)?
        .to_owned();
    Ok(fragment)
}

pub(super) fn runs_can_merge(left: &Run, right: &Run) -> bool {
    left.language == right.language
        && left.props == right.props
        && left.extra.is_empty()
        && right.extra.is_empty()
}

/// 合并相邻且属性可表达为同一 Run 的内容。
pub(super) fn merge_run_pair(left: &mut Run, right: &Run) -> bool {
    if !runs_can_merge(left, right) {
        return false;
    }
    left.text.push_str(&right.text);
    true
}

/// 在线性时间内合并选区接缝上兼容的非空 Run。
pub(super) fn merge_runs_at_selected_seams(runs: &mut Vec<Run>, range: Range<usize>) {
    if range.start >= range.end {
        return;
    }
    let mut merged = Vec::with_capacity(runs.len());
    let mut offset = 0usize;
    for run in std::mem::take(runs) {
        let seam = offset;
        offset += run.text.len();
        let can_merge = range.start <= seam
            && seam <= range.end
            && !run.text.is_empty()
            && merged
                .last()
                .is_some_and(|previous: &Run| !previous.text.is_empty());
        if can_merge
            && let Some(previous) = merged.last_mut()
            && merge_run_pair(previous, &run)
        {
            continue;
        }
        merged.push(run);
    }
    *runs = merged;
}

pub(super) fn text_flow_len(runs: &[Run]) -> usize {
    runs.iter().map(|run| run.text.len()).sum()
}

/// 按拼接 Run 文本中的 UTF-8 字节区间替换内容，并保留可表达的 Run 属性。
pub(super) fn replace_text_flow_range(
    text: &mut TextFlow,
    range: Range<usize>,
    affinity: Affinity,
    inserted: &str,
) -> Result<(), EditError> {
    let length = text_flow_len(&text.runs);
    if range.start > range.end
        || range.end > length
        || !text_flow_boundary(&text.runs, range.start)
        || !text_flow_boundary(&text.runs, range.end)
    {
        return Err(EditError::InvalidPosition);
    }

    if range.is_empty() && inserted.is_empty() {
        return Ok(());
    }
    let insertion = (!inserted.is_empty())
        .then(|| insertion_template_index(&text.runs, range.start, affinity))
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
        let index = run_insertion_index(&text.runs, range.start)?;
        text.runs.insert(index, default_run(inserted));
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
            if left.extra.is_empty()
                && right.extra.is_empty()
    )
}

fn normalize_runs(runs: &mut Vec<Run>, index: usize) {
    if runs.is_empty() || index >= runs.len() {
        return;
    }
    let anchor = &runs[index];
    let mut start = index;
    while start > 0 {
        let previous = &runs[start - 1];
        if is_empty_default_run(previous) || runs_can_merge(previous, anchor) {
            start -= 1;
        } else {
            break;
        }
    }
    let mut end = index + 1;
    while end < runs.len() {
        let next = &runs[end];
        if is_empty_default_run(next) || runs_can_merge(anchor, next) {
            end += 1;
        } else {
            break;
        }
    }

    let anchor_is_empty_default = is_empty_default_run(anchor);
    let has_nonempty_left_run = runs[start..index].iter().any(|run| !run.text.is_empty());
    let mut normalized: Option<Run> = None;
    for (relative_index, run) in runs.drain(start..end).enumerate() {
        let is_anchor = relative_index == index - start;
        if is_empty_default_run(&run) && !(is_anchor && !has_nonempty_left_run) {
            continue;
        }
        if let Some(previous) = normalized.as_mut()
            && merge_run_pair(previous, &run)
        {
            continue;
        }
        normalized = Some(run);
    }
    if anchor_is_empty_default && !has_nonempty_left_run && normalized.is_none() {
        normalized = Some(Run {
            text: String::new(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        });
    }
    runs.splice(start..start, normalized);
}

/// 在拼接文本偏移处规范化 Run，供不同宿主复用同一兼容合并规则。
pub(super) fn normalize_runs_at_offset(runs: &mut Vec<Run>, position: usize) {
    let mut offset = 0usize;
    for (index, run) in runs.iter().enumerate() {
        let end = offset + run.text.len();
        if position <= end {
            normalize_runs(runs, index);
            return;
        }
        offset = end;
    }
    if position == offset {
        normalize_runs(runs, runs.len().saturating_sub(1));
    }
}

/// 未知 TextFlow 字段可能依赖 Run 边界；只有无扩展的内容流允许合并相邻 Run。
pub(super) fn normalize_text_flow_runs_at_offset(text: &mut TextFlow, position: usize) {
    if text.extra.is_empty() {
        normalize_runs_at_offset(&mut text.runs, position);
    }
}

fn is_empty_default_run(run: &Run) -> bool {
    run.text.is_empty() && run.language.is_none() && run.props.is_empty() && run.extra.is_empty()
}

pub(super) fn insertion_template_with_affinity(
    runs: &[Run],
    position: usize,
    affinity: Affinity,
) -> Option<&Run> {
    insertion_template_index(runs, position, affinity).and_then(|(index, _)| runs.get(index))
}

fn insertion_template_index(
    runs: &[Run],
    position: usize,
    affinity: Affinity,
) -> Option<(usize, usize)> {
    let mut offset = 0;
    let mut preceding = None;
    let mut following = None;
    let mut first_empty = None;
    let mut last_empty = None;
    for (index, run) in runs.iter().enumerate() {
        let end = offset + run.text.len();
        if run.text.is_empty() && offset == position {
            first_empty.get_or_insert((index, 0));
            last_empty = Some((index, 0));
        }
        if offset < position && position < end {
            return Some((index, position - offset));
        }
        if offset == position && !run.text.is_empty() {
            following.get_or_insert((index, 0));
        }
        if end == position && !run.text.is_empty() {
            preceding = Some((index, run.text.len()));
        }
        offset = end;
    }
    match affinity {
        Affinity::Upstream => preceding.or(last_empty),
        Affinity::Downstream => following.or(first_empty),
    }
}

fn run_insertion_index(runs: &[Run], position: usize) -> Result<usize, EditError> {
    let mut offset = 0usize;
    for (index, run) in runs.iter().enumerate() {
        let end = offset
            .checked_add(run.text.len())
            .ok_or(EditError::InvalidPosition)?;
        if position == offset || position < end {
            return Ok(index);
        }
        if position == end {
            return Ok(index + 1);
        }
        offset = end;
    }
    (position == offset)
        .then_some(runs.len())
        .ok_or(EditError::InvalidPosition)
}

fn default_run(text: &str) -> Run {
    Run {
        text: text.to_owned(),
        language: None,
        props: Default::default(),
        extra: Default::default(),
    }
}

pub(super) fn text_flow_boundary(runs: &[Run], position: usize) -> bool {
    let mut offset = 0;
    for run in runs {
        if position >= offset && position <= offset + run.text.len() {
            return run.text.is_char_boundary(position - offset);
        }
        offset += run.text.len();
    }
    position == offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn run(text: &str) -> Run {
        Run {
            text: text.to_owned(),
            language: Some("zh-Hans".to_owned()),
            props: BTreeMap::from([("bold".to_owned(), serde_json::json!(true))]),
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn shared_range_split_keeps_zero_length_items_on_the_left_boundary() {
        assert_eq!(classify_range_split(2, 2, 2), RangeSplit::Left);
        assert_eq!(classify_range_split(2, 4, 2), RangeSplit::Right);
        assert_eq!(classify_range_split(1, 3, 2), RangeSplit::Inside(1));
    }

    #[test]
    fn shared_run_split_preserves_known_metadata_and_requires_utf8_boundaries() {
        let original = run("é😀");
        let (left, right) = split_run_at(&original, "é".len())
            .unwrap_or_else(|error| panic!("UTF-8 boundary should split: {error}"));
        assert_eq!(left.text, "é");
        assert_eq!(right.text, "😀");
        assert_eq!(left.language, original.language);
        assert_eq!(right.props, original.props);
        assert_eq!(left.extra, original.extra);
        assert_eq!(right.extra, original.extra);
        assert_eq!(split_run_at(&original, 1), Err(EditError::InvalidPosition));

        let mut opaque = original;
        opaque
            .extra
            .insert("future".to_owned(), serde_json::json!(true));
        assert_eq!(
            split_run_at(&opaque, "é".len()),
            Err(EditError::UnsupportedOpaqueContent)
        );
    }

    #[test]
    fn shared_run_merge_preserves_only_compatible_metadata() {
        let mut left = run("a");
        assert!(merge_run_pair(&mut left, &run("b")));
        assert_eq!(left.text, "ab");

        let mut different_language = run("a");
        let mut right = run("b");
        right.language = Some("en".to_owned());
        assert!(!merge_run_pair(&mut different_language, &right));
        assert_eq!(different_language.text, "a");

        right.language = different_language.language.clone();
        right
            .extra
            .insert("future".to_owned(), serde_json::json!(1));
        assert!(!merge_run_pair(&mut different_language, &right));
        assert!(runs_can_merge(&run(""), &run("")));
    }

    #[test]
    fn run_normalization_is_linear_and_keeps_the_anchor_placeholder() {
        let mut runs = (0..20_000).map(|_| run("x")).collect::<Vec<_>>();
        runs.push(Run {
            text: "tail".to_owned(),
            language: Some("en".to_owned()),
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        });
        normalize_runs(&mut runs, 10_000);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "x".repeat(20_000));
        assert_eq!(runs[1].text, "tail");

        let mut isolated_anchor = vec![
            Run {
                text: "left".to_owned(),
                language: Some("en".to_owned()),
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
            Run {
                text: String::new(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
            Run {
                text: "right".to_owned(),
                language: None,
                props: BTreeMap::from([("bold".to_owned(), serde_json::json!(true))]),
                extra: BTreeMap::new(),
            },
        ];
        normalize_runs(&mut isolated_anchor, 1);
        assert_eq!(isolated_anchor.len(), 3);
        assert_eq!(isolated_anchor[1].text, "");
        assert_eq!(isolated_anchor[0].text, "left");
        assert_eq!(isolated_anchor[2].text, "right");
    }

    #[test]
    fn text_flow_extensions_preserve_run_boundaries_during_normalization() {
        let mut text = TextFlow {
            runs: vec![run("a"), run("b")],
            extra: BTreeMap::from([("vendor".to_owned(), serde_json::json!(true))]),
        };
        normalize_text_flow_runs_at_offset(&mut text, 1);
        assert_eq!(text.runs.len(), 2);
        assert_eq!(text.runs[0].text, "a");
        assert_eq!(text.runs[1].text, "b");
        assert_eq!(text.extra["vendor"], serde_json::json!(true));

        text.extra.clear();
        normalize_text_flow_runs_at_offset(&mut text, 1);
        assert_eq!(text.runs.len(), 1);
        assert_eq!(text.runs[0].text, "ab");
    }

    #[test]
    fn shared_run_fragment_preserves_metadata_only_for_unsplit_ranges() {
        let mut original = run("éx");
        original
            .extra
            .insert("future".to_owned(), serde_json::json!("keep"));
        assert_eq!(
            run_fragment(&original, 0..original.text.len()),
            Ok(original.clone())
        );
        assert_eq!(
            run_fragment(&original, 0.."é".len()),
            Err(EditError::UnsupportedOpaqueContent)
        );
        assert_eq!(
            run_fragment(&run("éx"), 1..2),
            Err(EditError::InvalidPosition)
        );
    }
}
