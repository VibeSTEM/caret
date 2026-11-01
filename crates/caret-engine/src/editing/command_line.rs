//! 保存临时命令查询、候选顺序及其当前高亮项。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod font_picker;

use super::command_registry::{self, CommandCandidate};
use super::position::Pos;
use crate::EditError;
use caret_doc::DocumentFontTarget;
use font_picker::FontPicker;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLineMode {
    Command,
    FontFamily,
    DefaultFontTarget,
    DefaultFontScope,
    DefaultFontLanguage,
    DefaultFontFamily,
    /// 公式编辑中的子命令；查询区分大小写，以区分 `\\gamma` 与 `\\Gamma`。
    Math,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    pub at: Pos,
    pub query: String,
    pub cursor: usize,
    pub selected: usize,
    pub mode: CommandLineMode,
    font_picker: Option<FontPicker>,
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
            font_picker: None,
            preedit: String::new(),
            preedit_cursor_utf16: 0,
        }
    }

    /// 公式编辑中打开的子命令行。
    pub fn math(at: Pos) -> Self {
        Self {
            mode: CommandLineMode::Math,
            ..Self::new(at)
        }
    }

    pub fn font_family_picker(&self, font_families: Vec<String>) -> Self {
        Self {
            at: self.at.clone(),
            query: String::new(),
            cursor: 0,
            selected: 0,
            mode: CommandLineMode::FontFamily,
            font_picker: Some(FontPicker::local_family(font_families)),
            preedit: String::new(),
            preedit_cursor_utf16: 0,
        }
    }

    pub fn default_font_target_picker(
        &self,
        font_targets: Vec<(String, caret_doc::DocumentFontTarget)>,
        font_families: Vec<String>,
    ) -> Self {
        Self {
            at: self.at.clone(),
            query: String::new(),
            cursor: 0,
            selected: 0,
            mode: CommandLineMode::DefaultFontTarget,
            font_picker: Some(FontPicker::default_targets(font_targets, font_families)),
            preedit: String::new(),
            preedit_cursor_utf16: 0,
        }
    }

    pub fn default_font_family_picker(&self, target: caret_doc::DocumentFontTarget) -> Self {
        Self {
            at: self.at.clone(),
            query: String::new(),
            cursor: 0,
            selected: 0,
            mode: CommandLineMode::DefaultFontFamily,
            font_picker: self
                .font_picker
                .as_ref()
                .map(|picker| picker.default_family(target)),
            preedit: String::new(),
            preedit_cursor_utf16: 0,
        }
    }

    pub fn default_font_scope_picker(&self, script: String) -> Self {
        let mut next = self.clone();
        if let Some(picker) = &mut next.font_picker {
            picker.set_script(script);
        }
        next.mode = CommandLineMode::DefaultFontScope;
        next.query.clear();
        next.cursor = 0;
        next.selected = 0;
        next.preedit.clear();
        next.preedit_cursor_utf16 = 0;
        next
    }

    pub fn default_font_language_picker(&self) -> Self {
        let mut next = self.clone();
        next.mode = CommandLineMode::DefaultFontLanguage;
        next.query.clear();
        next.cursor = 0;
        next.selected = 0;
        next.preedit.clear();
        next.preedit_cursor_utf16 = 0;
        next
    }

    pub fn return_to_default_font_scope(&mut self) {
        if !matches!(
            self.mode,
            CommandLineMode::DefaultFontLanguage | CommandLineMode::DefaultFontFamily
        ) {
            return;
        }
        if let Some(picker) = &mut self.font_picker {
            picker.take_target();
        }
        self.mode = CommandLineMode::DefaultFontScope;
        self.query.clear();
        self.cursor = 0;
        self.selected = 0;
    }

    pub fn return_to_default_font_language(&mut self) {
        if self.mode != CommandLineMode::DefaultFontFamily {
            return;
        }
        let language = self
            .font_picker
            .as_mut()
            .and_then(FontPicker::take_target)
            .and_then(|target| match target {
                DocumentFontTarget::ScriptLanguage { language, .. } => Some(language),
                _ => None,
            });
        self.mode = CommandLineMode::DefaultFontLanguage;
        self.query = language.unwrap_or_default();
        self.cursor = self.query.len();
        self.selected = 0;
    }

    pub fn return_to_default_font_targets(&mut self) {
        if !matches!(
            self.mode,
            CommandLineMode::DefaultFontFamily
                | CommandLineMode::DefaultFontScope
                | CommandLineMode::DefaultFontLanguage
        ) {
            return;
        }
        self.font_picker
            .as_mut()
            .and_then(FontPicker::return_to_targets);
        self.mode = CommandLineMode::DefaultFontTarget;
        self.query.clear();
        self.cursor = 0;
        self.selected = 0;
        self.preedit.clear();
        self.preedit_cursor_utf16 = 0;
    }

    pub fn update_preedit(&mut self, preedit: String, cursor_utf16: usize) {
        self.preedit_cursor_utf16 = cursor_utf16.min(preedit.encode_utf16().count());
        self.preedit = preedit;
    }

    pub fn prefix(&self) -> String {
        match self.mode {
            CommandLineMode::Command | CommandLineMode::Math => "\\".to_owned(),
            CommandLineMode::FontFamily => "\\font ".to_owned(),
            CommandLineMode::DefaultFontTarget => "\\defaultfont ".to_owned(),
            CommandLineMode::DefaultFontScope => format!(
                "\\defaultfont {} ",
                self.font_picker
                    .as_ref()
                    .map_or_else(String::new, FontPicker::script)
            ),
            CommandLineMode::DefaultFontLanguage => format!(
                "\\defaultfont {}: ",
                self.font_picker
                    .as_ref()
                    .map_or_else(String::new, FontPicker::script)
            ),
            CommandLineMode::DefaultFontFamily => format!(
                "\\defaultfont {} ",
                self.font_picker
                    .as_ref()
                    .map_or_else(String::new, FontPicker::target_prefix)
            ),
        }
    }

    pub fn is_font_picker(&self) -> bool {
        matches!(
            self.mode,
            CommandLineMode::FontFamily
                | CommandLineMode::DefaultFontTarget
                | CommandLineMode::DefaultFontScope
                | CommandLineMode::DefaultFontLanguage
                | CommandLineMode::DefaultFontFamily
        )
    }

    pub fn target_for_candidate(
        &self,
        candidate: &CommandCandidate,
    ) -> Option<caret_doc::DocumentFontTarget> {
        self.font_picker.as_ref()?.target_for_candidate(candidate)
    }

    pub fn selected_default_font_target(&self) -> Option<DocumentFontTarget> {
        self.font_picker.as_ref()?.target().cloned()
    }

    pub fn enter_font_target(&mut self, target: caret_doc::DocumentFontTarget) {
        let next = self.default_font_family_picker(target);
        *self = next;
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
        if self.is_font_picker() {
            return self.font_picker.as_ref().map_or_else(Vec::new, |picker| {
                picker.candidates(self.mode, &self.query, &mut is_available)
            });
        }

        let source = if self.mode == CommandLineMode::Math {
            command_registry::math_candidates
        } else {
            command_registry::candidates
        };
        let mut candidates = Vec::new();
        if self.query.is_empty() {
            for candidate in source() {
                let mut candidate = candidate;
                candidate.available &= is_available(&candidate);
                if candidate.available {
                    candidates.push(candidate);
                }
            }
            return candidates;
        }
        if self.mode == CommandLineMode::Math {
            // 先列出区分大小写的别名前缀匹配，再列出不区分大小写的名称匹配。
            let query = self.query.clone();
            let lowercase = query.to_lowercase();
            append_matches(
                &mut candidates,
                source,
                |candidate| candidate.alias[1..].starts_with(&query),
                &mut is_available,
            );
            append_matches(
                &mut candidates,
                source,
                |candidate| candidate.label.to_lowercase().contains(&lowercase),
                &mut is_available,
            );
            return candidates;
        }
        let query = self.query.to_ascii_lowercase();
        append_matches(
            &mut candidates,
            source,
            |candidate| candidate.alias[1..].starts_with(&query),
            &mut is_available,
        );
        append_matches(
            &mut candidates,
            source,
            |candidate| candidate.command_id.starts_with(&query),
            &mut is_available,
        );
        append_matches(
            &mut candidates,
            source,
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
            CommandLineMode::Math
                if text
                    .chars()
                    .all(|character| character.is_ascii_alphabetic()) =>
            {
                text.to_owned()
            }
            CommandLineMode::DefaultFontTarget | CommandLineMode::DefaultFontLanguage
                if !text.chars().any(char::is_control) =>
            {
                text.to_owned()
            }
            CommandLineMode::FontFamily | CommandLineMode::DefaultFontFamily
                if !text.chars().any(char::is_control) =>
            {
                text.to_owned()
            }
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
    source: fn() -> Vec<CommandCandidate>,
    mut matches_query: impl FnMut(&CommandCandidate) -> bool,
    mut is_available: impl FnMut(&CommandCandidate) -> bool,
) {
    for candidate in source() {
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
