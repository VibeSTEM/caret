//! 公式源码停靠点及 Align 行、单元格位置映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

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
    let mut row = 0;
    let mut cell = 0;
    let mut cell_start = 0;
    let mut depth = 0usize;
    let mut environments = Vec::<&str>::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() && index < cursor {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'%' => {
                index = source[index..]
                    .find('\n')
                    .map_or(bytes.len(), |offset| index + offset);
                continue;
            }
            b'\\' if bytes.get(index + 1) == Some(&b'\\') => {
                if depth == 0 && environments.is_empty() {
                    if index + 2 > cursor {
                        break;
                    }
                    row += 1;
                    cell = 0;
                    cell_start = index + 2;
                }
                index += 2;
                continue;
            }
            b'\\' => {
                let command_start = index + 1;
                let mut command_end = command_start;
                while bytes.get(command_end).is_some_and(u8::is_ascii_alphabetic) {
                    command_end += 1;
                }
                if command_end == command_start {
                    index = (index + 2).min(bytes.len());
                    continue;
                }
                let command = &source[command_start..command_end];
                if matches!(command, "begin" | "end") {
                    let mut name_start = command_end;
                    while bytes.get(name_start).is_some_and(u8::is_ascii_whitespace) {
                        name_start += 1;
                    }
                    if bytes.get(name_start) == Some(&b'{')
                        && let Some(relative_end) = source[name_start + 1..].find('}')
                    {
                        let name_end = name_start + 1 + relative_end;
                        let name = &source[name_start + 1..name_end];
                        if command == "begin" {
                            environments.push(name);
                        } else if environments.last().copied() == Some(name) {
                            environments.pop();
                        }
                        index = name_end + 1;
                        continue;
                    }
                }
                index = command_end;
                continue;
            }
            b'&' if depth == 0 && environments.is_empty() => {
                cell += 1;
                cell_start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    let cell_text = source.get(cell_start..cursor.max(cell_start))?;
    let leading = cell_text.len() - cell_text.trim_start().len();
    Some((row, cell, cursor.saturating_sub(cell_start + leading)))
}

pub(super) fn previous_grapheme_start(source: &str, cursor: usize) -> Option<usize> {
    source
        .get(..cursor)?
        .grapheme_indices(true)
        .next_back()
        .map(|(start, _)| start)
}

pub(super) fn next_grapheme_end(source: &str, cursor: usize) -> Option<usize> {
    source
        .get(cursor..)?
        .graphemes(true)
        .next()
        .map(|grapheme| cursor + grapheme.len())
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
        let mut end = cursor;
        while source.as_bytes().get(end) == Some(&b'}') {
            end += 1;
        }
        return (end < source.len()).then_some(end);
    }
    next_grapheme_end(source, cursor)
}
