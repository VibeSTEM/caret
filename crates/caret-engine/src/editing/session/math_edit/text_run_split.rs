//! 按显示字节边界切分数学 TextFlow 的 Run。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{Run, TextFlow};
use std::ops::Range;

/// 将选区前后的 Run 分开，并拒绝无法安全复制扩展字段的切分。
pub(super) fn split_text_flow_runs(
    text: &TextFlow,
    range: &Range<usize>,
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

/// 将选区切成前、中、后三段，供行内格式包装事务使用。
pub(super) fn split_text_flow_runs_for_format(
    text: &TextFlow,
    range: &Range<usize>,
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
        for (slot, part) in [
            (0usize, offset..end.min(range.start)),
            (1usize, offset.max(range.start)..end.min(range.end)),
            (2usize, offset.max(range.end)..end),
        ] {
            if part.start < part.end {
                pieces.push((slot, part.start - offset..part.end - offset));
            }
        }
        if !run.extra.is_empty() && pieces.len() > 1 {
            return None;
        }
        for (slot, part) in pieces {
            let fragment = run_fragment(run, part);
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

fn run_fragment(run: &Run, range: Range<usize>) -> Run {
    let mut fragment = run.clone();
    fragment.text = run.text[range].to_owned();
    fragment
}

pub(super) fn text_flow_boundary(runs: &[Run], position: usize) -> bool {
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
