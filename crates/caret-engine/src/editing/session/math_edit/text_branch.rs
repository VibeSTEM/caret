//! 数学 TextFlow 分支的源码映射、Run 操作与文本编辑事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::flow_edit::insertion_template;
use super::super::super::grapheme::GraphemeMap;
use super::super::history::EditGroup;
use super::super::{Affinity, Direction, EditError, Effect};
use super::Session;
use super::navigation::{next_grapheme_end, previous_grapheme_start};
use super::{MathEditBranch, MathTextFlowPath};
use caret_doc::{InlineEnvironmentKind, MathContent};

mod format_tree_edit;
mod source_map_edit;
mod split_tree_edit;
pub(super) use format_tree_edit::apply_math_text_format_tree_edit;
pub(super) use source_map_edit::{EncodedMathTextPiece, apply_math_text_tree_edit};
pub(super) use split_tree_edit::apply_math_text_split_tree_edit;
use std::ops::Range;

#[derive(Debug, Clone)]
pub(super) struct MathTextEdit {
    pub(super) path: MathTextFlowPath,
    pub(super) range: Range<usize>,
    pub(super) inserted: String,
    pub(super) source_edit: Range<usize>,
    pub(super) source_replacement_len: usize,
    pub(super) encoded_pieces: Vec<EncodedMathTextPiece>,
    pub(super) snap_forward: bool,
    pub(super) removes_entire_flow: bool,
    pub(super) split_math_entry: bool,
    pub(super) split_math_contents: Option<Vec<MathContent>>,
    pub(super) format: Option<MathTextFormat>,
}

#[derive(Debug, Clone)]
pub(super) struct MathTextFormat {
    pub(super) kind: InlineEnvironmentKind,
    pub(super) enabled: bool,
    pub(super) selected_source_start: usize,
    pub(super) selected_source_len: usize,
    pub(super) selected_text: String,
    pub(super) display_range: Range<usize>,
    pub(super) selection_reversed: bool,
}

pub(super) fn text_source_boundary_from_map(
    text: &caret_doc::TextFlow,
    source_map: &caret_math::MathTextSourceMap,
    source_base: usize,
    offset: usize,
    affinity: Affinity,
) -> Option<usize> {
    let text_len = text
        .runs
        .iter()
        .try_fold(0usize, |length, run| length.checked_add(run.text.len()))?;
    if offset > text_len {
        return None;
    }
    let mut upstream = None;
    let mut downstream = None;
    for segment in source_map.segments() {
        let display = segment.display();
        let host_source = segment.source()?;
        let source_start = host_source.start.checked_sub(source_base)?;
        let source_end = host_source.end.checked_sub(source_base)?;
        if offset == display.start {
            downstream = Some(source_start);
        }
        if offset == display.end {
            upstream = Some(source_end);
        } else if display.start < offset
            && offset < display.end
            && display.len() == host_source.len()
        {
            return source_start.checked_add(offset - display.start);
        }
    }
    match affinity {
        Affinity::Upstream => upstream.or(downstream),
        Affinity::Downstream => downstream.or(upstream),
    }
}

pub(super) fn text_source_range_from_map(
    source_map: &caret_math::MathTextSourceMap,
    source_base: usize,
    range: Range<usize>,
) -> Option<Range<usize>> {
    if range.start >= range.end {
        return None;
    }
    let mut mapped = range.start;
    let mut source_range = None;
    for segment in source_map.segments() {
        let display = segment.display();
        let overlap_start = range.start.max(display.start);
        let overlap_end = range.end.min(display.end);
        if overlap_start >= overlap_end {
            continue;
        }
        if overlap_start != mapped {
            return None;
        }
        let host_source = segment.source()?;
        let source_start = host_source.start.checked_sub(source_base)?;
        let source_end = host_source.end.checked_sub(source_base)?;
        let linear = display.len() == host_source.len();
        if !linear && (overlap_start != display.start || overlap_end != display.end) {
            return None;
        }
        let mapped_source_start = if linear {
            source_start.checked_add(overlap_start - display.start)?
        } else {
            source_start
        };
        if source_range
            .as_ref()
            .is_some_and(|previous: &Range<usize>| previous.end != mapped_source_start)
        {
            return None;
        }
        let mapped_source_end = if linear {
            source_start.checked_add(overlap_end - display.start)?
        } else {
            source_end
        };
        match &mut source_range {
            Some(current) => current.end = mapped_source_end,
            None => source_range = Some(mapped_source_start..mapped_source_end),
        }
        mapped = overlap_end;
    }
    (mapped == range.end)
        .then_some(source_range?)
        .filter(|source_range| source_range.start < source_range.end)
}

pub(super) fn is_grapheme_boundary(text: &str, offset: usize) -> bool {
    GraphemeMap::is_boundary_in(text, offset)
}

pub(super) fn snap_text_cursor(text: &str, offset: usize, forward: bool) -> Option<usize> {
    GraphemeMap::snap_in(
        text,
        offset,
        if forward {
            Affinity::Downstream
        } else {
            Affinity::Upstream
        },
    )
}

/// 拒绝无法安全迁移到新源码位置的 Run 扩展字段。
pub(super) fn validate_text_flow_edit(
    text: &caret_doc::TextFlow,
    range: Range<usize>,
    inserts: bool,
) -> Result<(), EditError> {
    let mut offset = 0;
    for run in &text.runs {
        let end = offset + run.text.len();
        let overlaps = range.start < range.end && offset < range.end && range.start < end;
        if overlaps && !run.extra.is_empty() {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        if run.text.is_empty()
            && range.start <= offset
            && offset <= range.end
            && !run.extra.is_empty()
        {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        offset = end;
    }
    if inserts
        && insertion_template(&text.runs, range.start).is_some_and(|run| !run.extra.is_empty())
    {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    Ok(())
}

impl Session {
    /// 将文本岛选区替换成源码分支入口，并保留其前后文本岛。
    pub(super) fn math_text_enter_math(
        &mut self,
        initial_source: &str,
    ) -> Result<Effect, EditError> {
        let body = caret_math::input_body_range(initial_source, caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let initial_source = initial_source.get(body).ok_or(EditError::InvalidPosition)?;
        let split_math_contents =
            match caret_math::parse(initial_source, caret_math::MathInputKind::Inline)
                .map_err(|error| EditError::InvalidMath(error.to_string()))?
            {
                caret_math::ParsedMath::Tree(tree) => tree.root.content,
                caret_math::ParsedMath::Align(_) => return Err(EditError::InvalidPosition),
            };
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text { path, anchor } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let (text, text_map) = path
            .resolve_in_projection(tree, &edit.source_projection)
            .ok_or(EditError::InvalidPosition)?;
        let range = (*anchor).min(edit.cursor)..(*anchor).max(edit.cursor);
        let source_edit = if range.is_empty() {
            let cursor = text_source_boundary_from_map(
                text,
                text_map,
                edit.source_projection.edit_range().start,
                range.start,
                Affinity::Downstream,
            )
            .and_then(|offset| path.source_offset(offset))
            .ok_or(EditError::InvalidPosition)?;
            cursor..cursor
        } else {
            let display = text
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>();
            if range.end > display.len()
                || !is_grapheme_boundary(&display, range.start)
                || !is_grapheme_boundary(&display, range.end)
            {
                return Err(EditError::InvalidPosition);
            }
            text_source_range_from_map(
                text_map,
                edit.source_projection.edit_range().start,
                range.clone(),
            )
            .and_then(|mapped| {
                Some(path.source_offset(mapped.start)?..path.source_offset(mapped.end)?)
            })
            .ok_or(EditError::InvalidPosition)?
        };
        let local_source_edit = source_edit;
        let source_edit = path
            .host_offset(local_source_edit.start)
            .and_then(|start| {
                path.host_offset(local_source_edit.end)
                    .map(|end| start..end)
            })
            .ok_or(EditError::InvalidPosition)?;
        let mut replacement = String::with_capacity(initial_source.len() + 2);
        replacement.push('$');
        replacement.push_str(initial_source);
        replacement.push('$');
        let cursor = source_edit
            .start
            .checked_add(1 + initial_source.len())
            .ok_or(EditError::InvalidPosition)?;
        let mut source = edit.source().to_owned();
        source.replace_range(source_edit.clone(), &replacement);
        self.write_math_source_with_metadata(
            source,
            cursor,
            None,
            source_edit,
            None,
            Some(MathTextEdit {
                path: path.clone(),
                range,
                inserted: String::new(),
                source_edit: local_source_edit,
                source_replacement_len: replacement.len(),
                encoded_pieces: Vec::new(),
                snap_forward: false,
                removes_entire_flow: false,
                split_math_entry: true,
                split_math_contents: Some(split_math_contents),
                format: None,
            }),
        )
    }

    /// 将文本岛中的直接输入映射到可逆源码区间，并复用公式解析与历史事务。
    pub(super) fn math_text_insert(&mut self, inserted: &str) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text { path, anchor } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        if inserted.is_empty() && anchor == &edit.cursor {
            return Ok(Effect::default());
        }
        if inserted
            .chars()
            .any(|character| matches!(character, '\n' | '\r'))
        {
            return Err(EditError::UnsupportedNewline);
        }
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let (text, text_map) = path
            .resolve_in_projection(tree, &edit.source_projection)
            .ok_or(EditError::InvalidPosition)?;
        let range = (*anchor).min(edit.cursor)..(*anchor).max(edit.cursor);
        let (local_source_range, removes_entire_flow) = if range.is_empty() {
            let cursor = text_source_boundary_from_map(
                text,
                text_map,
                edit.source_projection.edit_range().start,
                range.start,
                Affinity::Downstream,
            )
            .and_then(|offset| path.source_offset(offset))
            .ok_or(EditError::InvalidPosition)?;
            (cursor..cursor, false)
        } else {
            let display = text
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>();
            if range.end > display.len()
                || !is_grapheme_boundary(&display, range.start)
                || !is_grapheme_boundary(&display, range.end)
            {
                return Err(EditError::InvalidPosition);
            }
            (
                text_source_range_from_map(
                    text_map,
                    edit.source_projection.edit_range().start,
                    range.clone(),
                )
                .and_then(|mapped| {
                    Some(path.source_offset(mapped.start)?..path.source_offset(mapped.end)?)
                })
                .ok_or(EditError::InvalidPosition)?,
                range.start == 0 && range.end == display.len() && inserted.is_empty(),
            )
        };
        let source_range = path
            .host_offset(local_source_range.start)
            .and_then(|start| {
                path.host_offset(local_source_range.end)
                    .map(|end| start..end)
            })
            .ok_or(EditError::InvalidPosition)?;
        let encoded_len = encoded_math_text_len(inserted);
        let projected_len = edit
            .source_projection
            .source()
            .len()
            .checked_sub(source_range.len())
            .and_then(|len| len.checked_add(encoded_len))
            .ok_or(EditError::InvalidPosition)?;
        if projected_len > caret_math::MAX_MATH_SOURCE_BYTES {
            return Err(EditError::InvalidMath(
                caret_math::MathParseError::InputTooLong {
                    len: projected_len,
                    max: caret_math::MAX_MATH_SOURCE_BYTES,
                }
                .to_string(),
            ));
        }
        let mut source = edit.source().to_owned();
        let (encoded, encoded_pieces) = encode_math_text_with_pieces(inserted);
        source.replace_range(source_range.clone(), &encoded);
        let next_cursor = range.start + inserted.len();
        self.write_math_source_with_metadata(
            source,
            next_cursor,
            Some(EditGroup::InsertText),
            source_range.clone(),
            Some((path.clone(), next_cursor)),
            Some(MathTextEdit {
                path: path.clone(),
                range,
                inserted: inserted.to_owned(),
                source_edit: local_source_range,
                source_replacement_len: encoded.len(),
                encoded_pieces,
                snap_forward: true,
                removes_entire_flow,
                split_math_entry: false,
                split_math_contents: None,
                format: None,
            }),
        )
    }

    /// 删除文本岛内一个 grapheme cluster；非精确源码映射不执行修改。
    pub(super) fn math_text_delete(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text { path, anchor } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let (text, text_map) = path
            .resolve_in_projection(tree, &edit.source_projection)
            .ok_or(EditError::InvalidPosition)?;
        let display = text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        let range = if anchor != &edit.cursor {
            Some((*anchor).min(edit.cursor)..(*anchor).max(edit.cursor))
        } else {
            match direction {
                Direction::Backward => {
                    previous_grapheme_start(&display, edit.cursor).map(|start| start..edit.cursor)
                }
                Direction::Forward => {
                    next_grapheme_end(&display, edit.cursor).map(|end| edit.cursor..end)
                }
            }
        };
        let Some(range) = range else {
            return Ok(Effect::default());
        };
        let local_source_range = text_source_range_from_map(
            text_map,
            edit.source_projection.edit_range().start,
            range.clone(),
        )
        .and_then(|mapped| Some(path.source_offset(mapped.start)?..path.source_offset(mapped.end)?))
        .ok_or(EditError::InvalidPosition)?;
        let source_range = path
            .host_offset(local_source_range.start)
            .and_then(|start| {
                path.host_offset(local_source_range.end)
                    .map(|end| start..end)
            })
            .ok_or(EditError::InvalidPosition)?;
        let mut source = edit.source().to_owned();
        source.replace_range(source_range.clone(), "");
        self.write_math_source_with_metadata(
            source,
            range.start,
            Some(match direction {
                Direction::Backward => EditGroup::DeleteBackward,
                Direction::Forward => EditGroup::DeleteForward,
            }),
            source_range.clone(),
            Some((path.clone(), range.start)),
            Some(MathTextEdit {
                path: path.clone(),
                range: range.clone(),
                inserted: String::new(),
                source_edit: local_source_range,
                source_replacement_len: 0,
                encoded_pieces: Vec::new(),
                snap_forward: false,
                removes_entire_flow: range.start == 0 && range.end == display.len(),
                split_math_entry: false,
                split_math_contents: None,
                format: None,
            }),
        )
    }
}

fn encode_math_text_with_pieces(text: &str) -> (String, Vec<EncodedMathTextPiece>) {
    let mut encoded = String::with_capacity(text.len());
    let mut pieces = Vec::with_capacity(text.chars().count());
    let mut display_offset = 0;
    for character in text.chars() {
        let source_start = encoded.len();
        match character {
            '\\' => encoded.push_str(r"\textchar{5C}"),
            '~' => encoded.push_str(r"\textchar{7E}"),
            '^' => encoded.push_str(r"\textchar{5E}"),
            character if character.is_whitespace() => {
                use std::fmt::Write as _;
                let _ = write!(encoded, r"\textchar{{{:X}}}", character as u32);
            }
            '{' | '}' | '%' | '#' | '$' | '&' | '_' => {
                encoded.push('\\');
                encoded.push(character);
            }
            _ => encoded.push(character),
        }
        let display_end = display_offset + character.len_utf8();
        pieces.push(EncodedMathTextPiece {
            display_range: display_offset..display_end,
            source_range: source_start..encoded.len(),
        });
        display_offset = display_end;
    }
    (encoded, pieces)
}

fn encoded_math_text_len(text: &str) -> usize {
    text.chars()
        .map(|character| match character {
            '\\' => r"\textchar{5C}".len(),
            '~' => r"\textchar{7E}".len(),
            '^' => r"\textchar{5E}".len(),
            character if character.is_whitespace() => {
                let mut digits = 1;
                let mut codepoint = character as u32;
                while codepoint >= 16 {
                    codepoint /= 16;
                    digits += 1;
                }
                r"\textchar{".len() + digits + 1
            }
            '{' | '}' | '%' | '#' | '$' | '&' | '_' => 2,
            character => character.len_utf8(),
        })
        .sum()
}
