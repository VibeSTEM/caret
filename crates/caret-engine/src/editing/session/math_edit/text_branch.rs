//! 数学 TextFlow 分支的源码映射、Run 操作与文本编辑事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::grapheme::GraphemeMap;
use super::super::history::EditGroup;
use super::super::{Affinity, Direction, EditError, Effect};
use super::Session;
use super::navigation::{next_grapheme_end, previous_grapheme_start};
use super::{MathEditBranch, MathTextFlowPath};
use std::ops::Range;

#[derive(Debug, Clone)]
pub(super) struct MathTextEdit {
    pub(super) path: MathTextFlowPath,
    pub(super) range: Range<usize>,
    pub(super) inserted: String,
    pub(super) source_edit: Range<usize>,
    pub(super) source_replacement_len: usize,
    pub(super) snap_forward: bool,
    pub(super) removes_entire_flow: bool,
}

fn text_source_boundary(text: &caret_doc::TextFlow, offset: usize) -> Option<usize> {
    let text_len = text.runs.iter().map(|run| run.text.len()).sum::<usize>();
    if offset > text_len {
        return None;
    }
    text.source_map.iter().find_map(|entry| {
        let text_start = entry.text_start as usize;
        let text_end = entry.text_end as usize;
        let source_start = entry.source_range.start as usize;
        let source_end = entry.source_range.end as usize;
        if offset == text_start {
            Some(source_start)
        } else if offset == text_end {
            Some(source_end)
        } else if text_start < offset
            && offset < text_end
            && text_end - text_start == source_end - source_start
        {
            Some(source_start + offset - text_start)
        } else {
            None
        }
    })
}

fn text_source_range(text: &caret_doc::TextFlow, range: Range<usize>) -> Option<Range<usize>> {
    if range.start >= range.end {
        return None;
    }
    let source_start = text_source_boundary(text, range.start)?;
    let source_end = text_source_boundary(text, range.end)?;
    let mut mapped = range.start;
    let mut previous_source_end = None;
    for entry in &text.source_map {
        let text_start = entry.text_start as usize;
        let text_end = entry.text_end as usize;
        let overlap_start = range.start.max(text_start);
        let overlap_end = range.end.min(text_end);
        if overlap_start >= overlap_end {
            continue;
        }
        if overlap_start != mapped {
            return None;
        }
        let source_start = entry.source_range.start as usize;
        let source_end = entry.source_range.end as usize;
        let linear = text_end - text_start == source_end - source_start;
        if !linear && (overlap_start != text_start || overlap_end != text_end) {
            return None;
        }
        let mapped_source_start = if linear {
            source_start + overlap_start - text_start
        } else {
            source_start
        };
        if previous_source_end.is_some_and(|end| end != mapped_source_start) {
            return None;
        }
        previous_source_end = Some(if linear {
            source_start + overlap_end - text_start
        } else {
            source_end
        });
        mapped = overlap_end;
    }
    let exact_source_start = text_source_boundary(text, range.start)?;
    let exact_source_end = text_source_boundary(text, range.end)?;
    (mapped == range.end
        && source_start == exact_source_start
        && source_end == exact_source_end
        && exact_source_start < exact_source_end)
        .then_some(exact_source_start..exact_source_end)
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

pub(super) fn replace_text_flow_range(
    text: &mut caret_doc::TextFlow,
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

    let template = insertion_template(&text.runs, range.start)
        .cloned()
        .unwrap_or(caret_doc::Run {
            text: String::new(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        });

    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut offset = 0;
    for run in &text.runs {
        let end = offset + run.text.len();
        if run.text.is_empty() {
            if offset <= range.end {
                before.push(run.clone());
            } else {
                after.push(run.clone());
            }
        } else if offset < range.start {
            let prefix_end = end.min(range.start);
            if prefix_end > offset {
                let mut fragment = run.clone();
                fragment.text = run.text[..prefix_end - offset].to_owned();
                before.push(fragment);
            }
        } else if range.start < range.end && offset >= range.start && end <= range.end {
            let mut placeholder = run.clone();
            placeholder.text.clear();
            before.push(placeholder);
        }
        offset = end;
    }
    if !inserted.is_empty() {
        let mut inserted_run = template;
        inserted_run.text = inserted.to_owned();
        before.push(inserted_run);
    }
    let mut offset = 0;
    for run in &text.runs {
        let end = offset + run.text.len();
        if !run.text.is_empty() && end > range.end {
            let suffix_start = offset.max(range.end);
            if suffix_start < end {
                let mut fragment = run.clone();
                fragment.text = run.text[suffix_start - offset..].to_owned();
                after.push(fragment);
            }
        }
        offset = end;
    }
    before.extend(after);
    text.runs = before;
    Ok(())
}

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

fn insertion_template(runs: &[caret_doc::Run], position: usize) -> Option<&caret_doc::Run> {
    let mut offset = 0;
    let mut preceding = None;
    for run in runs {
        let end = offset + run.text.len();
        if run.text.is_empty() && offset == position {
            return Some(run);
        }
        if offset <= position && position < end {
            return Some(run);
        }
        if end == position {
            preceding = Some(run);
        }
        offset = end;
    }
    preceding
}

fn text_boundary(runs: &[caret_doc::Run], position: usize) -> bool {
    let mut offset = 0;
    for run in runs {
        if position >= offset && position <= offset + run.text.len() {
            return run.text.is_char_boundary(position - offset);
        }
        offset += run.text.len();
    }
    position == offset
}

impl Session {
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
        let text = path.resolve(tree).ok_or(EditError::InvalidPosition)?;
        let range = (*anchor).min(edit.cursor)..(*anchor).max(edit.cursor);
        let (local_source_range, removes_entire_flow) = if range.is_empty() {
            let cursor =
                text_source_boundary(text, range.start).ok_or(EditError::InvalidPosition)?;
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
                text_source_range(text, range.clone()).ok_or(EditError::InvalidPosition)?,
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
        let mut source = edit.source.clone();
        let encoded = encode_math_text(inserted);
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
                snap_forward: true,
                removes_entire_flow,
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
        let text = path.resolve(tree).ok_or(EditError::InvalidPosition)?;
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
        let local_source_range =
            text_source_range(text, range.clone()).ok_or(EditError::InvalidPosition)?;
        let source_range = path
            .host_offset(local_source_range.start)
            .and_then(|start| {
                path.host_offset(local_source_range.end)
                    .map(|end| start..end)
            })
            .ok_or(EditError::InvalidPosition)?;
        let mut source = edit.source.clone();
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
                snap_forward: false,
                removes_entire_flow: range.start == 0 && range.end == display.len(),
            }),
        )
    }
}

fn encode_math_text(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for character in text.chars() {
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
    }
    encoded
}
