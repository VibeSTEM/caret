//! 执行命令面板状态迁移，并把可用候选分派给编辑命令。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::StoryId;
use super::Session;
use crate::editing::command_registry::MATH_INSERT_COMMAND_ID;
use crate::editing::{
    Command, CommandCandidate, CommandLine, CommandLineMode, Direction, EditError, Effect,
};
use caret_doc::BlockEnvironmentKind;

impl Session {
    /// 接受 `\\font` 时打开由布局字体目录填充的字体族候选模式。
    pub fn run_command_line_with_font_families(
        &mut self,
        command: Command,
        font_families: Vec<String>,
    ) -> Result<Effect, EditError> {
        self.run_command_line_with_font_choices(command, font_families, Vec::new())
    }

    pub fn run_command_line_with_font_choices(
        &mut self,
        command: Command,
        font_families: Vec<String>,
        font_targets: Vec<(String, caret_doc::DocumentFontTarget)>,
    ) -> Result<Effect, EditError> {
        let accepts_candidate = command == Command::AcceptCommandLine
            || matches!(&command, Command::TypeCommandLine(text) if text == "\\");
        let opens_font_picker = self.command_line.as_ref().is_some_and(|line| {
            let command_line_has_query = !line.query.is_empty();
            accepts_candidate
                && line.mode == CommandLineMode::Command
                && line.preedit.is_empty()
                && (!matches!(&command, Command::TypeCommandLine(_)) || command_line_has_query)
                && line
                    .selected_candidate_with(|candidate| {
                        self.command_candidate_available(candidate)
                    })
                    .is_some_and(|candidate| {
                        matches!(
                            candidate.command_id,
                            "text.format.font" | "document.default_font"
                        )
                    })
        });

        if opens_font_picker {
            if let Some(command_line) = &self.command_line {
                let selected = command_line.selected_candidate_with(|candidate| {
                    self.command_candidate_available(candidate)
                });
                let picker = if selected
                    .as_ref()
                    .is_some_and(|candidate| candidate.command_id == "document.default_font")
                {
                    command_line.default_font_target_picker(font_targets, font_families.clone())
                } else {
                    command_line.font_family_picker(font_families.clone())
                };
                self.command_line = Some(picker);
            }
            return Ok(Effect::default());
        }

        self.run_command_line(command)
    }

    /// 将输入法预编辑和提交文本路由到字体族查询，不触碰正文会话内容。
    pub fn input_method_command_line(
        &mut self,
        preedit: String,
        commit: String,
        replacement_start: i32,
        replacement_length: i32,
        cursor_utf16: usize,
    ) -> Result<(), EditError> {
        let Some(command_line) = &mut self.command_line else {
            return Ok(());
        };
        let has_replacement =
            !commit.is_empty() || replacement_start != 0 || replacement_length != 0;
        let accepts_commit = matches!(
            command_line.mode,
            CommandLineMode::FontFamily
                | CommandLineMode::DefaultFontFamily
                | CommandLineMode::DefaultFontLanguage
        ) || (command_line.mode == CommandLineMode::Command
            && commit.chars().all(|character| character.is_ascii_graphic()));
        if has_replacement && accepts_commit {
            command_line.replace_from_ime(replacement_start, replacement_length, &commit)?;
        }
        command_line.update_preedit(preedit, cursor_utf16);
        Ok(())
    }

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
            "insert.equation" | "insert.align" => self.command_line.as_ref().is_some_and(|line| {
                line.at.story == StoryId::Body
                    && self.selection.anchor == self.selection.focus
                    && self
                        .paragraph_index
                        .get(&line.at.paragraph_id)
                        .is_some_and(|location| !location.is_title)
            }),
            "insert.inline_math" => self.selection.anchor == self.selection.focus,
            _ => true,
        }
    }

    pub(super) fn run_command_line(&mut self, command: Command) -> Result<Effect, EditError> {
        if command == Command::CancelCommandLine {
            let had_preedit = self
                .command_line
                .as_ref()
                .is_some_and(|line| !line.preedit.is_empty());
            self.command_line = None;
            return Ok(Effect {
                composition_changed: had_preedit,
                ..Effect::default()
            });
        }
        if self
            .command_line
            .as_ref()
            .is_some_and(|line| !line.preedit.is_empty())
        {
            return Ok(Effect::default());
        }
        match command {
            Command::BeginCommandLine => {
                if self.composition.is_some() {
                    return Ok(Effect::default());
                }
                let at = self.selection.focus.clone();
                self.command_line = Some(if self.math_edit.is_some() {
                    CommandLine::math(at)
                } else {
                    CommandLine::new(at)
                });
            }
            Command::TypeCommandLine(text) => {
                if text == "\\"
                    && self
                        .command_line
                        .as_ref()
                        .is_none_or(|line| line.mode == CommandLineMode::Command)
                {
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
                    let forward = direction == Direction::Forward;
                    match command_line.mode {
                        CommandLineMode::DefaultFontTarget if forward => {
                            if let Some(candidate) =
                                command_line.candidates().get(command_line.selected)
                                && candidate.command_id == "document.default_font.target"
                                && let Some(target) = command_line.target_for_candidate(candidate)
                                && let caret_doc::DocumentFontTarget::Script(script) = target
                            {
                                *command_line = command_line.default_font_scope_picker(script);
                            }
                        }
                        CommandLineMode::DefaultFontScope if forward => {
                            if let Some(candidate) =
                                command_line.candidates().get(command_line.selected)
                            {
                                match candidate.command_id {
                                    "document.default_font.script" => {
                                        if let Some(target) =
                                            command_line.target_for_candidate(candidate)
                                        {
                                            command_line.enter_font_target(target);
                                        }
                                    }
                                    "document.default_font.script_language_scope" => {
                                        *command_line = command_line.default_font_language_picker();
                                    }
                                    _ => {}
                                }
                            }
                        }
                        CommandLineMode::DefaultFontLanguage if forward => {
                            if let Some(candidate) =
                                command_line.candidates().get(command_line.selected)
                                && candidate.command_id == "document.default_font.language"
                                && let Some(target) = command_line.target_for_candidate(candidate)
                            {
                                command_line.enter_font_target(target);
                            }
                        }
                        CommandLineMode::DefaultFontFamily
                            if !forward && command_line.cursor == 0 =>
                        {
                            if matches!(
                                command_line.selected_default_font_target(),
                                Some(caret_doc::DocumentFontTarget::ScriptLanguage { .. })
                            ) {
                                command_line.return_to_default_font_language();
                            } else {
                                command_line.return_to_default_font_scope();
                            }
                        }
                        CommandLineMode::DefaultFontLanguage
                        | CommandLineMode::DefaultFontScope
                            if !forward && command_line.cursor == 0 =>
                        {
                            command_line.return_to_default_font_targets();
                        }
                        _ => command_line.move_cursor(forward),
                    }
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
                if command_line.is_font_picker() {
                    return self.accept_command_line(false);
                }
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
            Command::CancelCommandLine => unreachable!("command cancellation returned above"),
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
        if !command_line.preedit.is_empty() {
            return Ok(Effect::default());
        }
        if !candidate.available {
            return Ok(Effect::default());
        }
        if command_line.mode == CommandLineMode::DefaultFontTarget {
            if candidate.command_id == "command_line.close_font_picker" {
                self.command_line = None;
                return Ok(Effect::default());
            }
            if candidate.command_id == "document.default_font.target"
                && let Some(caret_doc::DocumentFontTarget::Script(script)) =
                    command_line.target_for_candidate(&candidate)
            {
                let mut next = command_line;
                next = next.default_font_scope_picker(script);
                self.command_line = Some(next);
            }
            return Ok(Effect::default());
        }
        if command_line.mode == CommandLineMode::DefaultFontScope {
            if candidate.command_id == "command_line.close_font_picker" {
                self.command_line = None;
                return Ok(Effect::default());
            }
            let mut next = command_line;
            match candidate.command_id {
                "document.default_font.script" => {
                    if let Some(target) = next.target_for_candidate(&candidate) {
                        next.enter_font_target(target);
                    }
                }
                "document.default_font.script_language_scope" => {
                    next = next.default_font_language_picker();
                }
                _ => {}
            }
            self.command_line = Some(next);
            return Ok(Effect::default());
        }
        if command_line.mode == CommandLineMode::DefaultFontLanguage {
            if candidate.command_id == "command_line.close_font_picker" {
                self.command_line = None;
                return Ok(Effect::default());
            }
            if candidate.command_id == "document.default_font.language"
                && let Some(target) = command_line.target_for_candidate(&candidate)
            {
                let mut next = command_line;
                next.enter_font_target(target);
                self.command_line = Some(next);
            }
            return Ok(Effect::default());
        }
        if command_line.mode == CommandLineMode::DefaultFontFamily {
            if candidate.command_id == "command_line.close_font_picker" {
                self.command_line = None;
                return Ok(Effect::default());
            }
            if candidate.command_id == "document.default_font.family"
                && let (Some(target), Some(family)) = (
                    command_line.selected_default_font_target(),
                    candidate.value.clone(),
                )
            {
                let mut next = command_line;
                let effect = self.run(Command::SetDocumentFontFamily {
                    target,
                    family: Some(family),
                })?;
                next.return_to_default_font_targets();
                self.command_line = Some(next);
                return Ok(effect);
            }
            return Ok(Effect::default());
        }
        self.command_line = None;
        let effect = match candidate.command_id {
            "text.soft_break" => self.run(Command::InsertSoftBreak),
            "text.split_paragraph" => self.run(Command::InsertParagraphBreak),
            "text.insert.one_character_width_space" => {
                self.run(Command::InsertOneCharacterWidthSpace)
            }
            "text.insert.two_character_width_space" => {
                self.run(Command::InsertTwoCharacterWidthSpace)
            }
            "text.format.bold" => self.run(Command::ToggleInlineFormat(
                caret_doc::InlineEnvironmentKind::Bold,
            )),
            "text.format.italic" => self.run(Command::ToggleInlineFormat(
                caret_doc::InlineEnvironmentKind::Italic,
            )),
            "text.format.underline" => self.run(Command::ToggleRunUnderline),
            "text.format.font_family" => candidate
                .value
                .map(|family| self.run(Command::SetInlineFontFamily(family)))
                .unwrap_or_else(|| Ok(Effect::default())),
            "paragraph.first_line_indent.set" => {
                self.run(Command::SetParagraphFirstLineIndentChars(2))
            }
            "struct.section.create" => self.run(Command::CreateBlockEnvironment(
                BlockEnvironmentKind::Section,
            )),
            "insert.inline_math" => self.begin_inline_math(),
            "insert.equation" => self.insert_display_math(false),
            "insert.align" => self.insert_display_math(true),
            id if id == MATH_INSERT_COMMAND_ID => match candidate.value.clone() {
                Some(template) if self.math_edit.is_some() => self.math_insert_template(&template),
                _ => return Ok(Effect::default()),
            },
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
            let at = self.selection.focus.clone();
            self.command_line = Some(if self.math_edit.is_some() {
                CommandLine::math(at)
            } else {
                CommandLine::new(at)
            });
        }
        Ok(effect)
    }
}
