//! 保存临时命令查询、候选顺序及其当前高亮项。

use super::command_registry::{self, CommandCandidate};
use super::position::Pos;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    pub at: Pos,
    pub query: String,
    pub cursor: usize,
    pub selected: usize,
}

impl CommandLine {
    pub fn candidates(&self) -> Vec<CommandCandidate> {
        if self.query.is_empty() {
            return command_registry::candidates().to_vec();
        }
        let query = self.query.to_ascii_lowercase();
        let mut matches = Vec::new();
        for candidate in command_registry::candidates() {
            if candidate.alias[1..].starts_with(&query) {
                matches.push(*candidate);
            }
        }
        for candidate in command_registry::candidates() {
            if !matches.contains(candidate) && candidate.command_id.starts_with(&query) {
                matches.push(*candidate);
            }
        }
        for candidate in command_registry::candidates() {
            if !matches.contains(candidate)
                && candidate.label.to_ascii_lowercase().starts_with(&query)
            {
                matches.push(*candidate);
            }
        }
        matches
    }

    pub fn selected_candidate(&self) -> Option<CommandCandidate> {
        self.candidates().get(self.selected).copied()
    }

    pub fn push_text(&mut self, text: &str) {
        if text.chars().all(|character| character.is_ascii_graphic()) {
            let text = text.to_ascii_lowercase();
            self.query.insert_str(self.cursor, &text);
            self.cursor += text.len();
            self.selected = 0;
        }
    }

    pub fn backspace(&mut self) -> bool {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.query.remove(self.cursor);
            self.selected = 0;
            true
        } else {
            false
        }
    }

    pub fn move_cursor(&mut self, forward: bool) {
        if forward {
            self.cursor = (self.cursor + 1).min(self.query.len());
        } else {
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    pub fn move_selection(&mut self, forward: bool) {
        let count = self.candidates().len();
        if count == 0 {
            self.selected = 0;
        } else if forward {
            self.selected = (self.selected + 1) % count;
        } else {
            self.selected = (self.selected + count - 1) % count;
        }
    }
}
