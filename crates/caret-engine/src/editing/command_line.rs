//! 保存临时命令查询、候选顺序及其当前高亮项。

use super::command_registry::{self, CommandCandidate};
use super::position::Pos;
use crate::EditError;
use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLineMode {
    Command,
    FontFamily,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    pub at: Pos,
    pub query: String,
    pub cursor: usize,
    pub selected: usize,
    pub mode: CommandLineMode,
    pub font_families: Vec<String>,
    pub preedit: String,
    pub preedit_cursor_utf16: usize,
}

impl CommandLine {
    pub fn new(at: Pos) -> Self {
        Self {
            at,
            query: String::new(),
            cursor: 0,
            selected: 0,
            mode: CommandLineMode::Command,
            font_families: Vec::new(),
            preedit: String::new(),
            preedit_cursor_utf16: 0,
        }
    }

    pub fn font_family_picker(&self, font_families: Vec<String>) -> Self {
        Self {
            at: self.at.clone(),
            query: String::new(),
            cursor: 0,
            selected: 0,
            mode: CommandLineMode::FontFamily,
            font_families,
            preedit: String::new(),
            preedit_cursor_utf16: 0,
        }
    }

    pub fn update_preedit(&mut self, preedit: String, cursor_utf16: usize) {
        self.preedit_cursor_utf16 = cursor_utf16.min(preedit.encode_utf16().count());
        self.preedit = preedit;
    }

    pub fn prefix(&self) -> &'static str {
        match self.mode {
            CommandLineMode::Command => "\\",
            CommandLineMode::FontFamily => "\\font ",
        }
    }
}

impl CommandLine {
    pub fn candidates(&self) -> Vec<CommandCandidate> {
        self.candidates_with(|candidate| candidate.available)
    }

    pub fn candidates_with(
        &self,
        mut is_available: impl FnMut(&CommandCandidate) -> bool,
    ) -> Vec<CommandCandidate> {
        if self.mode == CommandLineMode::FontFamily {
            let query = self.query.to_lowercase();
            return self
                .font_families
                .iter()
                .filter(|family| family.to_lowercase().starts_with(&query))
                .map(|family| {
                    let mut candidate = CommandCandidate {
                        alias: Cow::Owned(family.clone()),
                        command_id: "text.format.font_family",
                        label: "Font family",
                        available: true,
                        value: Some(family.clone()),
                    };
                    candidate.available &= is_available(&candidate);
                    candidate
                })
                .filter(|candidate| candidate.available)
                .collect();
        }

        let mut candidates = Vec::new();
        if self.query.is_empty() {
            for candidate in command_registry::candidates() {
                let mut candidate = candidate;
                candidate.available &= is_available(&candidate);
                if candidate.available {
                    candidates.push(candidate);
                }
            }
            return candidates;
        }
        let query = self.query.to_ascii_lowercase();
        append_matches(
            &mut candidates,
            |candidate| candidate.alias[1..].starts_with(&query),
            &mut is_available,
        );
        append_matches(
            &mut candidates,
            |candidate| candidate.command_id.starts_with(&query),
            &mut is_available,
        );
        append_matches(
            &mut candidates,
            |candidate| candidate.label.to_ascii_lowercase().starts_with(&query),
            &mut is_available,
        );
        candidates
    }

    pub fn selected_candidate(&self) -> Option<CommandCandidate> {
        self.candidates().get(self.selected).cloned()
    }

    pub fn selected_candidate_with(
        &self,
        is_available: impl FnMut(&CommandCandidate) -> bool,
    ) -> Option<CommandCandidate> {
        self.candidates_with(is_available)
            .get(self.selected)
            .cloned()
    }

    pub fn push_text(&mut self, text: &str) {
        let text = match self.mode {
            CommandLineMode::Command
                if text.chars().all(|character| character.is_ascii_graphic()) =>
            {
                text.to_ascii_lowercase()
            }
            CommandLineMode::FontFamily if !text.chars().any(char::is_control) => text.to_owned(),
            _ => return,
        };
        if self.query.is_char_boundary(self.cursor) {
            self.query.insert_str(self.cursor, &text);
            self.cursor += text.len();
            self.cursor = round_up_to_grapheme_boundary(&self.query, self.cursor);
            self.selected = 0;
        }
    }

    /// 应用字体查询的输入法替换范围，并避免将查询光标拆进 Unicode 字素簇。
    pub fn replace_from_ime(
        &mut self,
        replacement_start: i32,
        replacement_length: i32,
        text: &str,
    ) -> Result<(), EditError> {
        if replacement_length < 0 || text.chars().any(char::is_control) {
            return Ok(());
        }
        let cursor_utf16 = self.query[..self.cursor].encode_utf16().count() as i64;
        let start_utf16 = cursor_utf16 + i64::from(replacement_start);
        let end_utf16 = start_utf16 + i64::from(replacement_length);
        let query_length_utf16 = self.query.encode_utf16().count() as i64;
        if start_utf16 < 0 || end_utf16 < start_utf16 || end_utf16 > query_length_utf16 {
            return Err(EditError::InvalidPosition);
        }
        let start = grapheme_byte_offset(&self.query, start_utf16 as usize, false)
            .ok_or(EditError::InvalidPosition)?;
        let end = grapheme_byte_offset(&self.query, end_utf16 as usize, true)
            .ok_or(EditError::InvalidPosition)?;
        self.query.replace_range(start..end, text);
        self.cursor = round_up_to_grapheme_boundary(&self.query, start + text.len());
        self.selected = 0;
        Ok(())
    }

    pub fn backspace(&mut self) -> bool {
        if self.cursor > 0 {
            let Some((start, _)) = self.query[..self.cursor].grapheme_indices(true).next_back()
            else {
                return false;
            };
            self.query.replace_range(start..self.cursor, "");
            self.cursor = start;
            self.selected = 0;
            true
        } else {
            false
        }
    }

    pub fn move_cursor(&mut self, forward: bool) {
        if forward {
            if let Some((offset, _)) = self.query[self.cursor..].grapheme_indices(true).nth(1) {
                self.cursor += offset;
            } else {
                self.cursor = self.query.len();
            }
        } else {
            self.cursor = self.query[..self.cursor]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(offset, _)| offset);
        }
    }

    pub fn move_selection(&mut self, forward: bool) {
        self.move_selection_with(forward, |candidate| candidate.available);
    }

    pub fn move_selection_with(
        &mut self,
        forward: bool,
        is_available: impl FnMut(&CommandCandidate) -> bool,
    ) {
        let count = self.candidates_with(is_available).len();
        if count == 0 {
            self.selected = 0;
        } else if forward {
            self.selected = (self.selected + 1) % count;
        } else {
            self.selected = (self.selected + count - 1) % count;
        }
    }
}

fn round_up_to_grapheme_boundary(text: &str, target: usize) -> usize {
    for (start, grapheme) in text.grapheme_indices(true) {
        let end = start + grapheme.len();
        if target == start || target == end {
            return target;
        }
        if target < end {
            return end;
        }
    }
    text.len()
}

fn grapheme_byte_offset(text: &str, target_utf16: usize, round_up: bool) -> Option<usize> {
    let mut utf16_offset = 0;
    for (byte_offset, grapheme) in text.grapheme_indices(true) {
        if target_utf16 == utf16_offset {
            return Some(byte_offset);
        }
        let next_utf16_offset = utf16_offset + grapheme.encode_utf16().count();
        if target_utf16 < next_utf16_offset {
            return Some(if round_up {
                byte_offset + grapheme.len()
            } else {
                byte_offset
            });
        }
        utf16_offset = next_utf16_offset;
    }
    (target_utf16 == utf16_offset).then_some(text.len())
}

fn append_matches(
    candidates: &mut Vec<CommandCandidate>,
    mut matches_query: impl FnMut(&CommandCandidate) -> bool,
    mut is_available: impl FnMut(&CommandCandidate) -> bool,
) {
    for candidate in command_registry::candidates() {
        if !matches_query(&candidate)
            || candidates
                .iter()
                .any(|matched| matched.alias == candidate.alias)
        {
            continue;
        }
        let mut candidate = candidate;
        candidate.available &= is_available(&candidate);
        candidates.push(candidate);
    }
}
