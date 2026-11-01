//! Align 源码与行列公式树之间的 cell 范围映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::Align;
use std::ops::Range;

/// 按解析器的顶层分隔规则计算每个 cell 在完整 Align 源码中的范围。
pub(super) fn cell_source_ranges(align: &Align) -> Option<Vec<Vec<Range<usize>>>> {
    let source = &align.source;
    let body = align_body_range(source)?;
    let parsed_rows = split_cell_ranges(source, body)?;
    if parsed_rows.len() != align.rows.len()
        || parsed_rows
            .iter()
            .zip(&align.rows)
            .any(|(ranges, row)| ranges.len() != row.cells.len())
    {
        return None;
    }

    align
        .rows
        .iter()
        .zip(parsed_rows)
        .map(|(row, ranges)| {
            row.cells
                .iter()
                .zip(ranges)
                .map(|(cell, range)| {
                    if cell.source.is_empty() {
                        let start = skip_trivia_bounded(source, range.start, range.end);
                        return Some(start..start);
                    }
                    locate_cell_source(source, range, &cell.source)
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect()
}

/// 使用唯一的分隔扫描器计算源码片段的行列范围。
pub(super) fn split_cell_ranges(
    source: &str,
    body: Range<usize>,
) -> Option<Vec<Vec<Range<usize>>>> {
    let body_start = body.start;
    let body_end = body.end;
    let mut parsed_rows = Vec::<Vec<Range<usize>>>::new();
    let mut row = Vec::new();
    let mut cell_start = body_start;
    let mut cursor = body_start;
    let mut brace_depth = 0usize;
    let mut environment_depth = 0usize;
    let mut row_terminated = false;
    while cursor < body_end {
        let remaining = &source[cursor..body_end];
        let Some(character) = remaining.chars().next() else {
            break;
        };
        if character == '%' {
            cursor += remaining
                .find('\n')
                .map_or(remaining.len(), |newline| newline + 1);
            continue;
        }
        if character == '\\' {
            if brace_depth == 0 && environment_depth == 0 && remaining.starts_with("\\\\") {
                row.push(cell_start..cursor);
                parsed_rows.push(std::mem::take(&mut row));
                cursor += 2;
                cell_start = cursor;
                row_terminated = true;
                continue;
            }
            row_terminated = false;
            if let Some((command, after_command)) = control_word_at(source, cursor)
                && matches!(command, "begin" | "end")
                && let Some((_, after_group)) =
                    braced_group_at(source, skip_trivia(source, after_command))
            {
                if command == "begin" {
                    environment_depth += 1;
                } else {
                    environment_depth = environment_depth.saturating_sub(1);
                }
                cursor = after_group;
                continue;
            }
            let Some(escaped) = remaining[1..].chars().next() else {
                break;
            };
            if escaped.is_ascii_alphabetic() {
                let word_end = remaining
                    .char_indices()
                    .skip(1)
                    .take_while(|(_, ch)| ch.is_ascii_alphabetic())
                    .last()
                    .map_or(1, |(index, ch)| index + ch.len_utf8());
                cursor += word_end;
            } else {
                cursor += 1 + escaped.len_utf8();
            }
            continue;
        }
        if !character.is_whitespace() {
            row_terminated = false;
        }
        match character {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '&' if brace_depth == 0 && environment_depth == 0 => {
                row.push(cell_start..cursor);
                cursor += 1;
                cell_start = cursor;
                continue;
            }
            _ => {}
        }
        cursor += character.len_utf8();
    }
    if !row_terminated {
        row.push(cell_start..body_end);
        parsed_rows.push(row);
    }

    Some(parsed_rows)
}

/// 在完整 Align 源码中返回 aligned 环境体，不把注释、分组或嵌套环境中的结束标记当作外层结束。
pub(super) fn align_body_range(source: &str) -> Option<Range<usize>> {
    let mut cursor = skip_trivia(source, 0);
    let (command, after_command) = control_word_at(source, cursor)?;
    if command != "begin" {
        return None;
    }
    cursor = skip_trivia(source, after_command);
    let (environment, after_group) = braced_group_at(source, cursor)?;
    if environment != "aligned" {
        return None;
    }
    let body_start = after_group;
    cursor = body_start;
    let mut brace_depth = 0usize;
    let mut environment_depth = 0usize;
    while cursor < source.len() {
        let character = source[cursor..].chars().next()?;
        if character == '%' {
            cursor = skip_comment(source, cursor);
            continue;
        }
        if character == '\\' {
            if let Some((command, after_command)) = control_word_at(source, cursor)
                && matches!(command, "begin" | "end")
            {
                let group_start = skip_trivia(source, after_command);
                if let Some((environment, after_group)) = braced_group_at(source, group_start) {
                    if brace_depth == 0 && environment == "aligned" {
                        if command == "begin" {
                            environment_depth += 1;
                        } else if environment_depth == 0 {
                            return Some(body_start..cursor);
                        } else {
                            environment_depth -= 1;
                        }
                    } else if brace_depth == 0 && command == "begin" {
                        environment_depth += 1;
                    } else if brace_depth == 0 && command == "end" {
                        environment_depth = environment_depth.saturating_sub(1);
                    }
                    cursor = after_group;
                    continue;
                }
            }
            cursor += 1;
            if let Some(escaped) = source.get(cursor..).and_then(|rest| rest.chars().next())
                && !escaped.is_ascii_alphabetic()
            {
                cursor += escaped.len_utf8();
                continue;
            }
            while cursor < source.len() {
                let next = source[cursor..].chars().next()?;
                if !next.is_ascii_alphabetic() {
                    break;
                }
                cursor += next.len_utf8();
            }
            continue;
        }
        match character {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
        cursor += character.len_utf8();
    }
    None
}

fn locate_cell_source(source: &str, range: Range<usize>, cell: &str) -> Option<Range<usize>> {
    let start = skip_trivia_bounded(source, range.start, range.end);
    if source[start..range.end].starts_with(cell) {
        Some(start..start + cell.len())
    } else {
        None
    }
}

fn skip_trivia(source: &str, mut cursor: usize) -> usize {
    loop {
        while let Some(character) = source[cursor..].chars().next()
            && character.is_whitespace()
        {
            cursor += character.len_utf8();
        }
        if source[cursor..].starts_with('%') {
            cursor = skip_comment(source, cursor);
        } else {
            return cursor;
        }
    }
}

pub(super) fn skip_trivia_bounded(source: &str, mut cursor: usize, end: usize) -> usize {
    loop {
        while cursor < end {
            let Some(character) = source[cursor..end].chars().next() else {
                break;
            };
            if !character.is_whitespace() {
                break;
            }
            cursor += character.len_utf8();
        }
        if cursor < end && source[cursor..end].starts_with('%') {
            cursor = skip_comment(source, cursor).min(end);
        } else {
            return cursor;
        }
    }
}

fn skip_comment(source: &str, cursor: usize) -> usize {
    source[cursor..]
        .find('\n')
        .map_or(source.len(), |offset| cursor + offset + 1)
}

fn control_word_at(source: &str, cursor: usize) -> Option<(&str, usize)> {
    let tail = source.get(cursor..)?.strip_prefix('\\')?;
    let length = tail.bytes().take_while(u8::is_ascii_alphabetic).count();
    (length > 0).then(|| (&tail[..length], cursor + length + 1))
}

fn braced_group_at(source: &str, cursor: usize) -> Option<(&str, usize)> {
    source.get(cursor..)?.strip_prefix('{')?;
    let after_open = cursor + 1;
    let close = source[after_open..].find('}')? + after_open;
    Some((source[after_open..close].trim(), close + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_and_cells_ignore_comment_markers_and_noncanonical_spacing() {
        let Ok(caret_math::ParsedMath::Align(mut align)) = caret_math::parse(
            r"\begin{ aligned }\text{abc}\end{ aligned }",
            caret_math::MathInputKind::Align,
        ) else {
            panic!("canonical Align source should parse to an Align tree");
        };
        align.source =
            "\\begin { aligned } % \\end{aligned} \\text{abc}\n\\text{abc}\\end { aligned }"
                .to_owned();

        let Some(body) = align_body_range(&align.source) else {
            panic!("outer aligned body should be found");
        };
        assert_eq!(
            &align.source[body.clone()],
            " % \\end{aligned} \\text{abc}\n\\text{abc}"
        );
        let Some(ranges) = cell_source_ranges(&align) else {
            panic!("cell ranges should match the parsed tree");
        };
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].len(), 1);
        assert_eq!(&align.source[ranges[0][0].clone()], r"\text{abc}");
        assert_eq!(
            ranges[0][0].start,
            align.source.rfind(r"\text{abc}").unwrap_or(usize::MAX)
        );
    }

    #[test]
    fn escaped_delimiters_do_not_change_wrapper_or_cell_scanning() {
        let Ok(caret_math::ParsedMath::Align(mut align)) = caret_math::parse(
            r"\begin{aligned}\text{a\%b\{c\}}\end{aligned}",
            caret_math::MathInputKind::Align,
        ) else {
            panic!("escaped Align text should parse");
        };
        align.source = r"\begin{aligned}\text{a\%b\{c\}}\end{aligned}".to_owned();

        let Some(body) = align_body_range(&align.source) else {
            panic!("escaped delimiters must not hide the wrapper end");
        };
        assert_eq!(&align.source[body], r"\text{a\%b\{c\}}");
        let Some(ranges) = cell_source_ranges(&align) else {
            panic!("escaped cell range should match the parsed tree");
        };
        assert_eq!(&align.source[ranges[0][0].clone()], r"\text{a\%b\{c\}}");
    }

    #[test]
    fn trailing_row_break_matches_parser_row_shape() {
        let Ok(caret_math::ParsedMath::Align(align)) = caret_math::parse(
            r"\begin{aligned}x\\\end{aligned}",
            caret_math::MathInputKind::Align,
        ) else {
            panic!("trailing row break should parse as one Align row");
        };

        let Some(ranges) = cell_source_ranges(&align) else {
            panic!("scanner row shape should match parser row shape");
        };
        assert_eq!(ranges.len(), align.rows.len());
        assert_eq!(ranges[0].len(), align.rows[0].cells.len());
    }
}
