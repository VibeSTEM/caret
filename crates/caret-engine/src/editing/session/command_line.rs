//! 执行命令面板状态迁移，并把可用候选分派给编辑命令。

use super::super::StoryId;
use super::Session;
use crate::editing::{Command, CommandCandidate, CommandLine, Direction, EditError, Effect};
use caret_doc::BlockEnvironmentKind;

impl Session {
    /// 当前命令查询在编辑上下文中的候选；领域可用性由 Session 统一裁决。
    pub fn command_line_candidates(&self) -> Vec<CommandCandidate> {
        let Some(command_line) = &self.command_line else {
            return Vec::new();
        };
        command_line.candidates_with(|candidate| self.command_candidate_available(candidate))
    }

    fn command_candidate_available(&self, candidate: &CommandCandidate) -> bool {
        if !candidate.available {
            return false;
        }
        match candidate.command_id {
            "struct.section.create" => self.command_line.as_ref().is_some_and(|line| {
                line.at.story == StoryId::Body
                    && self.selection.anchor == self.selection.focus
                    && self.paragraph_index.contains_key(&line.at.paragraph_id)
            }),
            _ => true,
        }
    }

    pub(super) fn run_command_line(&mut self, command: Command) -> Result<Effect, EditError> {
        match command {
            Command::BeginCommandLine => {
                if self.composition.is_some() {
                    return Ok(Effect::default());
                }
                self.command_line = Some(CommandLine {
                    at: self.selection.focus.clone(),
                    query: String::new(),
                    cursor: 0,
                    selected: 0,
                });
            }
            Command::TypeCommandLine(text) => {
                if text == "\\" {
                    return self.repeat_command_line_prefix();
                }
                if let Some(command_line) = &mut self.command_line {
                    command_line.push_text(&text);
                }
            }
            Command::BackspaceCommandLine => {
                if let Some(command_line) = &mut self.command_line
                    && !command_line.backspace()
                    && command_line.query.is_empty()
                {
                    self.command_line = None;
                }
            }
            Command::MoveCommandLineCursor(direction) => {
                if let Some(command_line) = &mut self.command_line {
                    command_line.move_cursor(direction == Direction::Forward);
                }
            }
            Command::MoveCommandLineSelection(direction) => {
                if let Some(mut command_line) = self.command_line.clone() {
                    command_line
                        .move_selection_with(direction == Direction::Forward, |candidate| {
                            self.command_candidate_available(candidate)
                        });
                    self.command_line = Some(command_line);
                }
            }
            Command::SelectCommandLineCandidate(index) => {
                let candidate_count = self.command_line_candidates().len();
                if let Some(command_line) = &mut self.command_line
                    && index < candidate_count
                {
                    command_line.selected = index;
                }
            }
            Command::TabCommandLine => {
                let Some(command_line) = self.command_line.clone() else {
                    return Ok(Effect::default());
                };
                if command_line.query.is_empty() {
                    self.command_line = None;
                    return match self.run(Command::InsertText("\\".to_owned())) {
                        Ok(effect) => Ok(effect),
                        Err(error) => {
                            self.command_line = Some(command_line);
                            Err(error)
                        }
                    };
                }
                if let Some(candidate) = command_line.selected_candidate_with(|candidate| {
                    self.command_candidate_available(candidate)
                }) && let Some(alias) = candidate.alias.strip_prefix('\\')
                    && let Some(current) = &mut self.command_line
                {
                    current.query = alias.to_owned();
                    current.cursor = current.query.len();
                    current.selected = 0;
                }
            }
            Command::CancelCommandLine => self.command_line = None,
            Command::AcceptCommandLine => return self.accept_command_line(false),
            _ => unreachable!("only command-line commands are handled here"),
        }
        Ok(Effect::default())
    }

    fn repeat_command_line_prefix(&mut self) -> Result<Effect, EditError> {
        let Some(command_line) = self.command_line.clone() else {
            return Ok(Effect::default());
        };
        if command_line.query.is_empty() {
            self.command_line = None;
            return match self.run(Command::InsertText("\\".to_owned())) {
                Ok(effect) => Ok(effect),
                Err(error) => {
                    self.command_line = Some(command_line);
                    Err(error)
                }
            };
        }
        let effect = self.accept_command_line(true)?;
        Ok(effect)
    }

    fn accept_command_line(&mut self, continue_after: bool) -> Result<Effect, EditError> {
        let Some(command_line) = self.command_line.clone() else {
            return Ok(Effect::default());
        };
        let Some(candidate) = command_line
            .selected_candidate_with(|candidate| self.command_candidate_available(candidate))
        else {
            return Ok(Effect::default());
        };
        if !candidate.available {
            return Ok(Effect::default());
        }
        self.command_line = None;
        let effect = match candidate.command_id {
            "text.soft_break" => self.run(Command::InsertSoftBreak),
            "text.split_paragraph" => self.run(Command::InsertParagraphBreak),
            "struct.section.create" => self.run(Command::CreateBlockEnvironment(
                BlockEnvironmentKind::Section,
            )),
            _ => return Ok(Effect::default()),
        };
        let effect = match effect {
            Ok(effect) => effect,
            Err(error) => {
                self.command_line = Some(command_line);
                return Err(error);
            }
        };
        if continue_after {
            self.command_line = Some(CommandLine {
                at: self.selection.focus.clone(),
                query: String::new(),
                cursor: 0,
                selected: 0,
            });
        }
        Ok(effect)
    }
}
