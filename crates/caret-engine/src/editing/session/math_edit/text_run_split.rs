//! 按显示字节边界切分数学 TextFlow 的 Run。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use crate::editing::flow_edit::{run_fragment, text_flow_boundary, text_flow_len};
use caret_doc::{Run, TextFlow};
use std::ops::Range;

pub(super) struct TextFlowSplit {
    pub(super) left_runs: Vec<Run>,
    pub(super) right_runs: Vec<Run>,
    pub(super) has_left: bool,
    pub(super) has_right: bool,
}

/// 将选区前后的 Run 分开，并拒绝无法安全复制扩展字段的切分。
pub(super) fn split_text_flow_for_replacement(
    text: &TextFlow,
    range: &Range<usize>,
) -> Option<TextFlowSplit> {
    let length = text_flow_len(&text.runs);
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
            left.push(run_fragment(run, 0..left_end - offset).ok()?);
        }
        let right_start = offset.max(range.end);
        if right_start < end {
            right.push(run_fragment(run, right_start - offset..run.text.len()).ok()?);
        }
        let selected = offset < range.end && range.start < end;
        let split =
            offset < range.start && range.start < end || offset < range.end && range.end < end;
        if !run.extra.is_empty() && (selected || split) {
            return None;
        }
        offset = end;
    }
    let has_left = !left.is_empty() || range.start == 0;
    let has_right = !right.is_empty() || range.end == length || range.is_empty();
    let fragment_count = usize::from(has_left) + usize::from(has_right);
    if fragment_count != 1 && !text.extra.is_empty() {
        return None;
    }

    Some(TextFlowSplit {
        left_runs: left,
        right_runs: right,
        has_left,
        has_right,
    })
}
