//! 执行命令面板状态迁移，并把可用候选分派给编辑命令。

use super::Session;
use crate::editing::{Command, CommandLine, Direction, EditError, Effect};
use caret_doc::BlockEnvironmentKind;

impl Session {
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
                if let Some(command_line) = &mut self.command_line {
                    command_line.move_selection(direction == Direction::Forward);
                }
            }
            Command::SelectCommandLineCandidate(index) => {
                if let Some(command_line) = &mut self.command_line
                    && index < command_line.candidates().len()
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
                if let Some(candidate) = command_line.selected_candidate()
                    && let Some(alias) = candidate.alias.strip_prefix('\\')
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
        let Some(candidate) = command_line.selected_candidate() else {
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
            "struct.custom.create" => self.run(Command::CreateBlockEnvironment(
                BlockEnvironmentKind::Custom,
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
