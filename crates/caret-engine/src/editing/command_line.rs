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
        self.candidates_with(|candidate| candidate.available)
    }

    pub fn candidates_with(
        &self,
        mut is_available: impl FnMut(&CommandCandidate) -> bool,
    ) -> Vec<CommandCandidate> {
        let mut candidates = Vec::new();
        if self.query.is_empty() {
            for candidate in command_registry::candidates() {
                let mut candidate = *candidate;
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
        self.candidates().get(self.selected).copied()
    }

    pub fn selected_candidate_with(
        &self,
        is_available: impl FnMut(&CommandCandidate) -> bool,
    ) -> Option<CommandCandidate> {
        self.candidates_with(is_available)
            .get(self.selected)
            .copied()
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

fn append_matches(
    candidates: &mut Vec<CommandCandidate>,
    mut matches_query: impl FnMut(&CommandCandidate) -> bool,
    mut is_available: impl FnMut(&CommandCandidate) -> bool,
) {
    for candidate in command_registry::candidates() {
        if !matches_query(candidate)
            || candidates
                .iter()
                .any(|matched| matched.alias == candidate.alias)
        {
            continue;
        }
        let mut candidate = *candidate;
        candidate.available &= is_available(&candidate);
        candidates.push(candidate);
    }
}
