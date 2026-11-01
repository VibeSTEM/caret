//! 公式源码停靠点及 Align 行、单元格位置映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::ops::Range;

use super::super::super::grapheme::GraphemeMap;
use super::{MathEdit, MathEditTarget};

pub(super) fn math_input_body_range(edit: &MathEdit) -> Option<Range<usize>> {
    let kind = match &edit.target {
        MathEditTarget::Inline { .. } => Some(caret_math::MathInputKind::Inline),
        MathEditTarget::Block { align: false, .. } => Some(caret_math::MathInputKind::Equation),
        MathEditTarget::Block { align: true, .. } => None,
    };
    kind.map_or_else(
        || Some(0..edit.source.len()),
        |kind| caret_math::input_body_range(&edit.source, kind).ok(),
    )
}

/// 对齐源码中顶层的 `\\` 与 `&` 切分出单元格，返回光标所在 (行, 单元格, 去首空白后的单元格内偏移)。
pub(super) fn align_cell_at(source: &str, cursor: usize) -> Option<(usize, usize, usize)> {
    if cursor > source.len() || !source.is_char_boundary(cursor) {
        return None;
    }
    let rows = super::align_source::split_cell_ranges(source, 0..source.len())?;
    rows.iter().enumerate().find_map(|(row, cells)| {
        cells.iter().enumerate().find_map(|(cell, range)| {
            if cursor < range.start || cursor > range.end {
                return None;
            }
            let cell_source_start =
                super::align_source::skip_trivia_bounded(source, range.start, cursor);
            Some((row, cell, cursor.saturating_sub(cell_source_start)))
        })
    })
}

pub(super) fn previous_grapheme_start(source: &str, cursor: usize) -> Option<usize> {
    GraphemeMap::previous_in(source, cursor)
}

pub(super) fn next_grapheme_end(source: &str, cursor: usize) -> Option<usize> {
    GraphemeMap::next_in(source, cursor)
}

pub(super) fn previous_math_caret_stop(source: &str, cursor: usize) -> Option<usize> {
    let prefix = source.get(..cursor)?;
    if prefix.ends_with("\\\\") {
        return Some(cursor - 2);
    }
    if prefix.ends_with("}{") {
        return Some(cursor - 2);
    }
    let Some(command_start) = prefix.rfind('\\') else {
        return previous_grapheme_start(source, cursor);
    };
    let suffix = prefix.get(command_start + 1..)?;
    let escaped = !source[..command_start]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count()
        .is_multiple_of(2);
    if !escaped && !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Some(command_start);
    }
    previous_grapheme_start(source, cursor)
}

pub(super) fn next_math_caret_stop(source: &str, cursor: usize) -> Option<usize> {
    let suffix = source.get(cursor..)?;
    if suffix.starts_with("\\\\") {
        return Some(cursor + 2);
    }
    if suffix.starts_with("}{") {
        return Some(cursor + 2);
    }
    if suffix.starts_with('\\') {
        let mut end = cursor + 1;
        while source
            .as_bytes()
            .get(end)
            .is_some_and(u8::is_ascii_alphabetic)
        {
            end += 1;
        }
        if end > cursor + 1 {
            return Some(end);
        }
        return next_grapheme_end(source, cursor);
    }
    if suffix.starts_with('}') {
        return next_grapheme_end(source, cursor);
    }
    next_grapheme_end(source, cursor)
}
