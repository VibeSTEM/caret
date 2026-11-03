//! 公式源码停靠点及 Align 行、单元格位置映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::ops::Range;

use super::super::super::grapheme::GraphemeMap;
use super::{MathEdit, MathEditTarget};

pub(super) fn math_input_body_range(edit: &MathEdit) -> Option<Range<usize>> {
    match &edit.target {
        MathEditTarget::Block { align: true, .. } => Some(0..edit.source().len()),
        MathEditTarget::Inline { .. } | MathEditTarget::Block { align: false, .. } => {
            Some(edit.source_projection.body_range())
        }
    }
}

/// 使用解析器生成的导航范围映射 Align 光标到行列及 cell 内偏移。
pub(super) fn align_cell_at(
    projection: &caret_math::MathSourceMap<'_>,
    cursor: usize,
) -> Option<(usize, usize, usize)> {
    let source = projection.edit_source();
    if cursor > source.len() || !source.is_char_boundary(cursor) {
        return None;
    }
    let base = projection.edit_range().start;
    let rows = projection.align()?.cell_navigation_ranges();
    rows.iter().enumerate().find_map(|(row, cells)| {
        cells.iter().enumerate().find_map(|(cell, range)| {
            let start = range.start.checked_sub(base)?;
            let end = range.end.checked_sub(base)?;
            if cursor < start || cursor > end {
                return None;
            }
            let cell_source_start = super::align_source::skip_trivia_bounded(source, start, cursor);
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
