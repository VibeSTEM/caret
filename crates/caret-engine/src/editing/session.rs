//! 管理编辑 Session 状态并执行输入、选区和 IME 命令。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod clipboard;
mod command_line;
mod composition;
mod current_environment;
mod formatting;
mod history;
mod inline_content;
mod keyboards;
mod math_edit;
mod navigation;
mod paragraph_edit;
mod paragraph_formatting;
mod range_edit;
mod selection;
mod structure;
mod text_edit;

use super::paragraph_ops::{
    ParagraphIndex, build_paragraph_index, first_paragraph, paragraph_at_location,
    paragraph_at_location_mut,
};
use super::text_map::TextMap;
use super::{
    Affinity, Command, CommandLine, Direction, EditError, EditorKey, Effect, InputComposition,
    MathDirection, MathFocus, Pos, PositionMap, Selection, StoryId,
};
use caret_doc::{Document, NodeId, PageGeometry, Paragraph};
use caret_layout::VisualAffinity;
use caret_structure::StructureIndex;
use std::{collections::VecDeque, sync::Arc};
use undo::History;

use self::formatting::PendingInlineStyle;
use self::history::{DocumentPatch, SessionHistoryEdit, SessionHistoryState};

const TEXT_MAP_CACHE_CAPACITY: usize = 2;

/// 保存跨越多个正文 Story 的结构选区锚点与焦点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureSelection {
    pub anchor: Pos,
    pub focus: Pos,
}

#[derive(Debug)]
pub struct Session {
    pub(super) document: Document,
    pub(super) selection: Selection,
    selection_visual_affinity: VisualAffinity,
    session_id: NodeId,
    document_generation: u64,
    composition: Option<InputComposition>,
    pending_inline_style: PendingInlineStyle,
    math_focus: Option<MathFocus>,
    math_input_buffer: Option<String>,
    math_input_error: bool,
    command_line: Option<CommandLine>,
    block_environment_exit: Option<(NodeId, Direction)>,
    selected_block_environment: Option<NodeId>,
    selected_block_environments: Vec<NodeId>,
    structure_selection: Option<StructureSelection>,
    last_committed_composition: Option<u64>,
    text_map_cache: VecDeque<(NodeId, StoryId, TextMap)>,
    paragraph_index: ParagraphIndex,
    structure_index: Arc<StructureIndex>,
    edit_history: History<SessionHistoryEdit>,
    history_state_id: u64,
    next_history_state_id: u64,
    history_estimated_bytes: usize,
    history_merge_epoch: u64,
}

impl Session {
    pub fn new(document: Document) -> Result<Self, EditError> {
        document
            .validate()
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
        let (paragraph, story) = first_paragraph(&document.body)
            .ok_or_else(|| EditError::InvalidDocument("document has no paragraph".to_owned()))?;
        let pos = Pos {
            story,
            paragraph_id: paragraph.id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        let structure_index = Arc::new(StructureIndex::build(&document));
        Ok(Self {
            paragraph_index: build_paragraph_index(&document.body),
            structure_index,
            document,
            selection: Selection {
                anchor: pos.clone(),
                focus: pos,
            },
            selection_visual_affinity: VisualAffinity::Downstream,
            session_id: NodeId::generate(),
            document_generation: 0,
            composition: None,
            pending_inline_style: PendingInlineStyle::default(),
            math_focus: None,
            math_input_buffer: None,
            math_input_error: false,
            command_line: None,
            block_environment_exit: None,
            selected_block_environment: None,
            selected_block_environments: Vec::new(),
            structure_selection: None,
            last_committed_composition: None,
            text_map_cache: VecDeque::new(),
            edit_history: history::history(),
            history_state_id: 0,
            next_history_state_id: 1,
            history_estimated_bytes: 0,
            history_merge_epoch: 0,
        })
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    /// 当前打开会话的唯一标识，用于区分被新 Session 替换的同代文档快照。
    pub fn session_id(&self) -> &NodeId {
        &self.session_id
    }

    /// 文档内容或页面几何每成功变更一次便递增；异步布局结果用它拒绝过时代际。
    pub fn document_generation(&self) -> u64 {
        self.document_generation
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// 当前公式焦点；数学输入路由到该 MathPos，正文 Selection 保持在公式对象边界。
    pub fn math_focus(&self) -> Option<&MathFocus> {
        self.math_focus.as_ref()
    }

    /// 当前尚未组成有效公式语法的原始输入；错误输入保留到继续输入、退格或 Escape。
    pub fn pending_math_input(&self) -> Option<&str> {
        self.math_input_buffer.as_deref()
    }

    /// 暂存数学输入当前是否为 parser 拒绝的完整语法；原文仍可通过 `pending_math_input` 读取。
    pub fn math_input_error(&self) -> bool {
        self.math_input_error
    }

    /// 按稳定段落 ID 返回 Rust 索引中的正文或环境标题归属。
    pub fn story_for_paragraph(&self, paragraph_id: &NodeId) -> Option<StoryId> {
        self.paragraph_index
            .get(paragraph_id)
            .map(|location| location.story.clone())
    }

    /// 当前文档派生的结构索引；不写入文档，不触发布局。
    pub fn structure_index(&self) -> &StructureIndex {
        &self.structure_index
    }

    /// 当前焦点的视觉侧；不属于 UTF-8 逻辑位置，也不改变文档 generation。
    pub fn selection_visual_affinity(&self) -> VisualAffinity {
        self.selection_visual_affinity
    }

    pub fn composition(&self) -> Option<&InputComposition> {
        self.composition.as_ref()
    }

    /// 当前临时命令查询；此状态不属于文档修订或布局缓存。
    pub fn command_line(&self) -> Option<&CommandLine> {
        self.command_line.as_ref()
    }

    /// 当前光标是否停在块环境外侧，而不为导航插入可见空段落。
    pub fn block_environment_exit(&self) -> Option<(NodeId, Direction)> {
        self.block_environment_exit.clone()
    }

    /// 返回当前以完整页内几何表示的块环境结构选区。
    pub fn selected_block_environment(&self) -> Option<&NodeId> {
        self.selected_block_environment.as_ref()
    }

    /// 返回当前结构选区覆盖的块环境，按文档顺序排列。
    pub fn selected_block_environments(&self) -> &[NodeId] {
        &self.selected_block_environments
    }

    /// 返回跨 Story 结构选区的原始锚点，用于连续 Shift 导航。
    pub fn structure_selection_anchor(&self) -> Option<&Pos> {
        self.structure_selection
            .as_ref()
            .map(|selection| &selection.anchor)
    }

    /// 返回结构选区两端，用于生成跨段、跨 Story 的可见文本选区。
    pub fn structure_selection(&self) -> Option<(&Pos, &Pos)> {
        self.structure_selection
            .as_ref()
            .map(|selection| (&selection.anchor, &selection.focus))
    }

    /// 将普通文本选区无法跨越的环境边界表示为结构选区投影。
    pub fn select_block_environment(
        &mut self,
        environment_id: NodeId,
    ) -> Result<Effect, EditError> {
        let exists = self
            .structure_index
            .structures()
            .iter()
            .any(|entry| entry.id == environment_id && entry.kind.is_block_environment());
        if !exists {
            return Err(EditError::InvalidPosition);
        }
        let changed = self.selected_block_environment.as_ref() != Some(&environment_id);
        self.selected_block_environment = Some(environment_id);
        self.selected_block_environments =
            self.selected_block_environment.iter().cloned().collect();
        self.structure_selection = Some(StructureSelection {
            anchor: self.selection.focus.clone(),
            focus: self.selection.focus.clone(),
        });
        self.block_environment_exit = None;
        Ok(Effect {
            selection_changed: changed,
            ..Effect::default()
        })
    }

    /// 当前文档历史状态标识，用于把异步保存结果关联到实际保存点。
    pub fn history_state_id(&self) -> u64 {
        self.history_state_id
    }

    pub fn can_undo(&self) -> bool {
        self.edit_history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.edit_history.can_redo()
    }

    /// 在异步保存捕获修订前切断输入合并，保留可精确返回的保存位置。
    pub fn begin_save_checkpoint(&mut self) {
        self.advance_history_merge_epoch();
    }

    /// 执行唯一命令入口；可恢复校验在替换文档数据前完成。
    pub fn run(&mut self, command: Command) -> Result<Effect, EditError> {
        let inserted_text = match &command {
            Command::InsertText(text) | Command::PasteText(text) => Some(text.as_str()),
            Command::InsertOneCharacterWidthSpace => Some("\u{2003}"),
            Command::InsertTwoCharacterWidthSpace => Some("\u{2003}\u{2003}"),
            _ => None,
        };
        if self.math_focus.is_some() {
            if matches!(&command, Command::EditorKey(EditorKey::Escape))
                && (self.math_input_buffer.take().is_some() || self.math_input_error)
            {
                self.math_input_error = false;
                return Ok(Effect {
                    math_input_changed: true,
                    ..Effect::default()
                });
            }
            if matches!(&command, Command::Undo | Command::Redo)
                && (self.math_input_buffer.take().is_some() || self.math_input_error)
            {
                self.math_input_error = false;
                return Ok(Effect {
                    math_input_changed: true,
                    ..Effect::default()
                });
            }
            match &command {
                Command::InsertText(text) => {
                    return self.run(Command::TypeMathText(text.clone()));
                }
                Command::PasteText(text) => {
                    return self.run(Command::PasteMathText(text.clone()));
                }
                Command::InsertOneCharacterWidthSpace => {
                    return self.run(Command::TypeMathText("\u{2003}".to_owned()));
                }
                Command::InsertTwoCharacterWidthSpace => {
                    return self.run(Command::TypeMathText("\u{2003}\u{2003}".to_owned()));
                }
                Command::MoveCaret(direction) => {
                    return self.move_math_caret(match direction {
                        Direction::Backward => MathDirection::Left,
                        Direction::Forward => MathDirection::Right,
                    });
                }
                Command::InsertMathText(_)
                | Command::TypeMathText(_)
                | Command::PasteMathText(_)
                | Command::InsertMathFraction
                | Command::InsertMathRadical
                | Command::MoveMathCaret(_)
                | Command::ExitMath
                | Command::Undo
                | Command::Redo
                | Command::EditorKey(EditorKey::Escape) => {}
                Command::Delete(Direction::Backward) if self.math_input_buffer.is_some() => {
                    return self.backspace_math_input();
                }
                Command::UpdateComposition { .. }
                | Command::CommitComposition { .. }
                | Command::CancelComposition { .. }
                | Command::SetSelection(_)
                | Command::Delete(_)
                | Command::MoveStructure(_)
                | Command::ExitBlockEnvironment(_)
                | Command::RemoveBlockEnvironmentPreservingContent
                | Command::RemoveInlineEnvironmentPreservingContent(_)
                | Command::ExpandStructureSelection
                | Command::ExitStructure
                | Command::CreateBlockEnvironment(_)
                | Command::ToggleInlineFormat(_)
                | Command::ToggleRunUnderline
                | Command::SetInlineFontFamily(_)
                | Command::SetParagraphFirstLineIndentChars(_)
                | Command::SetDefaultFontFamily(_)
                | Command::SetDocumentFontFamily { .. }
                | Command::EditorKey(_)
                | Command::SetPageGeometry(_)
                | Command::InsertSoftBreak
                | Command::InsertParagraphBreak
                | Command::InsertInlineMath
                | Command::BeginCommandLine
                | Command::TypeCommandLine(_)
                | Command::BackspaceCommandLine
                | Command::MoveCommandLineCursor(_)
                | Command::MoveCommandLineSelection(_)
                | Command::SelectCommandLineCandidate(_)
                | Command::TabCommandLine
                | Command::AcceptCommandLine
                | Command::CancelCommandLine => return Err(EditError::UnsupportedMathTarget),
            }
        }
        if let Command::MoveMathCaret(direction) = &command {
            return self.move_math_caret(*direction);
        }
        if let Some(text) = inserted_text
            && self.block_environment_exit.is_some()
        {
            return self.insert_text_at_environment_exit(text);
        }
        if matches!(&command, Command::Delete(Direction::Backward))
            && !self.selected_block_environments.is_empty()
            && (self.selected_block_environments.len() > 1
                || self
                    .structure_selection
                    .as_ref()
                    .is_some_and(|selection| selection.anchor != selection.focus))
        {
            return self
                .delete_selected_structure_range()?
                .ok_or(EditError::InvalidPosition);
        }
        if matches!(&command, Command::Delete(Direction::Backward))
            && self.selected_block_environments.len() == 1
        {
            return self.delete_selected_block_environment();
        }
        if matches!(&command, Command::Delete(Direction::Backward))
            && self.selected_block_environments.len() > 1
        {
            return Ok(Effect::default());
        }
        if let Command::EditorKey(key) = &command {
            return self.run_editor_key(*key);
        }
        if command == Command::ExitMath {
            return self.exit_math();
        }
        if let Command::CommitComposition { id, text } = &command
            && self.block_environment_exit.is_some()
        {
            return self.commit_composition(*id, text);
        }
        if self.block_environment_exit.is_some() {
            match command {
                Command::InsertParagraphBreak => {
                    return self.insert_break_at_environment_exit(false);
                }
                Command::InsertSoftBreak => return self.insert_break_at_environment_exit(true),
                _ => {}
            }
        }
        self.selected_block_environment = None;
        self.selected_block_environments.clear();
        self.structure_selection = None;
        if self.command_line.is_some() && matches!(&command, Command::Delete(Direction::Backward)) {
            return self.run_command_line(Command::BackspaceCommandLine);
        }
        if self.block_environment_exit.is_some() && matches!(&command, Command::Delete(_)) {
            let (environment_id, exit_direction) = self
                .block_environment_exit
                .clone()
                .ok_or(EditError::InvalidPosition)?;
            let deletes_adjacent_environment = matches!(
                (exit_direction, &command),
                (Direction::Forward, Command::Delete(Direction::Backward))
                    | (Direction::Backward, Command::Delete(Direction::Forward))
            );
            if deletes_adjacent_environment
                && !structure::environment_has_content_at(&self.document.body, &environment_id)?
            {
                self.selected_block_environment = Some(environment_id);
                return self.delete_selected_block_environment();
            }
            return Ok(Effect::default());
        }
        if matches!(&command, Command::Delete(Direction::Backward))
            && self.selection.anchor == self.selection.focus
            && self.selection.focus.offset == 0
            && self.composition.is_none()
            && self.command_line.is_none()
        {
            let position = self.selection.focus.clone();
            if let Some(effect) = self.unwrap_focused_block_environment(true)? {
                return Ok(effect);
            }
            if let Some(effect) = self.delete_empty_environment_before(&position)? {
                return Ok(effect);
            }
        }
        if matches!(
            &command,
            Command::MoveStructure(_)
                | Command::ExitBlockEnvironment(_)
                | Command::RemoveBlockEnvironmentPreservingContent
                | Command::ExpandStructureSelection
                | Command::ExitStructure
        ) {
            return self.run_structure(command);
        }
        if let Command::CreateBlockEnvironment(kind) = command {
            return self.create_block_environment(kind);
        }
        if matches!(
            &command,
            Command::BeginCommandLine
                | Command::TypeCommandLine(_)
                | Command::BackspaceCommandLine
                | Command::MoveCommandLineCursor(_)
                | Command::MoveCommandLineSelection(_)
                | Command::SelectCommandLineCandidate(_)
                | Command::TabCommandLine
                | Command::AcceptCommandLine
                | Command::CancelCommandLine
        ) {
            return self.run_command_line(command);
        }
        if command == Command::Undo {
            let effect = self.restore_history(false)?;
            self.command_line = None;
            return Ok(effect);
        }
        if command == Command::Redo {
            let effect = self.restore_history(true)?;
            self.command_line = None;
            return Ok(effect);
        }
        let before_selection = self.selection.clone();
        let cancels_command_line_preedit = self
            .command_line
            .as_ref()
            .is_some_and(|line| !line.preedit.is_empty());
        let before_visual_affinity = self.selection_visual_affinity;
        let before_pending_style = self.pending_inline_style.clone();
        let before_math_focus = self.math_focus.clone();
        let before_state_id = self.history_state_id;
        let adjacent_delete_paragraph = match &command {
            Command::Delete(direction) if before_selection.anchor == before_selection.focus => {
                self.adjacent_paragraph_for_delete(*direction)?
            }
            _ => None,
        };
        let edit_group = history::edit_group(&command);
        let breaks_history_merge = edit_group.is_none();
        let resets_pending_style =
            matches!(&command, Command::MoveCaret(_) | Command::SetSelection(_));
        let mut before_paragraph_states = if is_paragraph_edit(&command) {
            let mut ids = history::before_selection_ids(&self.paragraph_index, &self.selection);
            if let Command::Delete(_) = &command
                && self.selection.anchor == self.selection.focus
                && let Some(adjacent_id) = adjacent_delete_paragraph.clone()
            {
                ids.push(adjacent_id);
            }
            history::paragraph_states_for_ids(&self.document, &self.paragraph_index, ids)
        } else {
            Vec::new()
        };
        let before_page_geometry = matches!(&command, Command::SetPageGeometry(_))
            .then(|| {
                self.document
                    .page_sections
                    .first()
                    .and_then(|section| section.geometry().ok())
            })
            .flatten();
        let mut before_latin_font_family = None;
        let before_document_font_family = match &command {
            Command::SetDefaultFontFamily(_) => {
                before_latin_font_family = Some(history::LatinFontFamilyState::capture(
                    &self.document.global_settings,
                ));
                None
            }
            Command::SetDocumentFontFamily { target, .. } => {
                let target = target
                    .normalized()
                    .map_err(|_| EditError::InvalidFontFamily)?;
                if target == caret_doc::DocumentFontTarget::Script("Latn".to_owned()) {
                    before_latin_font_family = Some(history::LatinFontFamilyState::capture(
                        &self.document.global_settings,
                    ));
                    None
                } else {
                    Some((
                        target.clone(),
                        self.document
                            .global_settings
                            .font_family(&target)
                            .map(str::to_owned),
                    ))
                }
            }
            _ => None,
        };
        let mut effect = match command {
            Command::InsertText(text) => self.insert_text(&text),
            Command::InsertInlineMath => self.insert_inline_math(),
            Command::InsertMathText(text) => self.insert_math_text(&text, false),
            Command::TypeMathText(text) => self.insert_math_text(&text, true),
            Command::PasteMathText(text) => self.insert_math_text(&text, false),
            Command::InsertMathFraction => {
                self.insert_math_edit(crate::math_edit::MathEdit::InsertFraction)
            }
            Command::InsertMathRadical => {
                self.insert_math_edit(crate::math_edit::MathEdit::InsertRadical)
            }
            Command::ExitMath => unreachable!("math exit returned before editing dispatch"),
            Command::MoveMathCaret(_) => {
                unreachable!("math navigation returned before editing dispatch")
            }
            Command::InsertOneCharacterWidthSpace => self.insert_text("\u{2003}"),
            Command::InsertTwoCharacterWidthSpace => self.insert_text("\u{2003}\u{2003}"),
            Command::PasteText(text) => self.paste_text(&text),
            Command::InsertSoftBreak => self.insert_soft_break(),
            Command::InsertParagraphBreak => self.split_paragraph(),
            Command::Delete(direction) => self.delete(direction),
            Command::MoveCaret(direction) => self.move_caret(direction),
            Command::MoveStructure(_)
            | Command::ExitBlockEnvironment(_)
            | Command::RemoveBlockEnvironmentPreservingContent
            | Command::ExpandStructureSelection
            | Command::ExitStructure => {
                unreachable!("structure commands returned above")
            }
            Command::CreateBlockEnvironment(_) => {
                unreachable!("structure edit commands returned above")
            }
            Command::ToggleInlineFormat(kind) => self.toggle_inline_format(kind),
            Command::ToggleRunUnderline => self.toggle_run_underline(),
            Command::RemoveInlineEnvironmentPreservingContent(environment_id) => {
                self.remove_inline_environment_preserving_content(&environment_id)
            }
            Command::SetInlineFontFamily(family) => self.set_inline_font_family(&family),
            Command::SetParagraphFirstLineIndentChars(chars) => {
                self.set_paragraph_first_line_indent_chars(chars)
            }
            Command::SetDefaultFontFamily(family) => self.set_default_font_family(family),
            Command::SetDocumentFontFamily { target, family } => {
                self.set_document_font_family(&target, family)
            }
            Command::EditorKey(_) => unreachable!("editor keys returned before editing dispatch"),
            Command::SetSelection(selection) => self.set_selection(selection),
            Command::UpdateComposition { id, preedit } => self.update_composition(id, preedit),
            Command::CommitComposition { id, text } => self.commit_composition(id, &text),
            Command::CancelComposition { id } => self.cancel_composition(id),
            Command::SetPageGeometry(geometry) => self.set_page_geometry(geometry),
            Command::BeginCommandLine
            | Command::TypeCommandLine(_)
            | Command::BackspaceCommandLine
            | Command::MoveCommandLineCursor(_)
            | Command::MoveCommandLineSelection(_)
            | Command::SelectCommandLineCandidate(_)
            | Command::TabCommandLine
            | Command::AcceptCommandLine
            | Command::CancelCommandLine => unreachable!("command-line commands returned above"),
            Command::Undo | Command::Redo => unreachable!("history commands returned above"),
        }?;
        effect.composition_changed |= cancels_command_line_preedit;
        effect.selection_changed |= before_math_focus != self.math_focus;
        self.command_line = None;
        if effect.selection_changed && resets_pending_style {
            self.pending_inline_style.clear();
        }
        if breaks_history_merge {
            self.advance_history_merge_epoch();
        }
        if effect.selection_changed {
            self.selection_visual_affinity = match self.selection.focus.affinity {
                Affinity::Upstream => VisualAffinity::Upstream,
                Affinity::Downstream => VisualAffinity::Downstream,
            };
        }
        if effect.document_changed {
            if effect
                .position_map
                .as_ref()
                .is_some_and(position_map_changes_paragraph_structure)
            {
                self.structure_index = Arc::new(StructureIndex::build(&self.document));
            }
            let after_state_id = self.allocate_history_state_id();
            let patch = if let Some(before) = before_page_geometry {
                let after = self
                    .document
                    .page_sections
                    .first()
                    .and_then(|section| section.geometry().ok())
                    .ok_or_else(|| {
                        EditError::InvalidDocument("document has no PageSection".to_owned())
                    })?;
                DocumentPatch::PageGeometry { before, after }
            } else if let Some(before) = before_latin_font_family {
                DocumentPatch::LatinFontFamily {
                    before,
                    after: history::LatinFontFamilyState::capture(&self.document.global_settings),
                }
            } else if let Some((target, before)) = before_document_font_family {
                DocumentPatch::DocumentFontFamily {
                    before,
                    after: self
                        .document
                        .global_settings
                        .font_family(&target)
                        .map(str::to_owned),
                    target,
                }
            } else {
                let mut ids = before_paragraph_states
                    .iter()
                    .map(|state| state.id.clone())
                    .collect::<Vec<_>>();
                if let Some(map) = &effect.position_map {
                    history::state_ids_from_position_map(map, &mut ids);
                }
                let after_paragraph_states =
                    history::paragraph_states_for_ids(&self.document, &self.paragraph_index, ids);
                if after_paragraph_states.is_empty() {
                    return Err(EditError::InvalidDocument(
                        "changed document command did not produce a history patch".to_owned(),
                    ));
                }
                DocumentPatch::Paragraphs {
                    before: std::mem::take(&mut before_paragraph_states),
                    after: after_paragraph_states,
                    merge_group: edit_group,
                    merge_epoch: self.history_merge_epoch,
                }
            };
            effect.changed_paragraph_id = patch.single_changed_paragraph_id();
            let edit = SessionHistoryEdit::new(
                patch,
                SessionHistoryState::with_math_focus(
                    before_selection,
                    before_math_focus,
                    before_visual_affinity,
                    before_pending_style,
                    before_state_id,
                ),
                SessionHistoryState::with_math_focus(
                    self.selection.clone(),
                    self.math_focus.clone(),
                    self.selection_visual_affinity,
                    self.pending_inline_style.clone(),
                    after_state_id,
                ),
            );
            self.record_history_edit(edit);
            self.history_state_id = after_state_id;
            if let Some(next) = self.document_generation.checked_add(1) {
                self.document_generation = next;
            } else {
                self.session_id = NodeId::generate();
                self.document_generation = 0;
            }
        }
        Ok(effect)
    }

    fn record_history_edit(&mut self, edit: SessionHistoryEdit) {
        let latest_edit_bytes = edit.estimated_bytes();
        let transition = self.edit_history.edit(&mut self.document, edit.clone());
        self.history_state_id = transition.state_id;
        self.history_estimated_bytes = history::estimated_history_bytes(&self.edit_history);
        if self.history_estimated_bytes > history::history_memory_limit() {
            self.edit_history.clear();
            self.history_estimated_bytes = 0;
            if latest_edit_bytes <= history::history_memory_limit() {
                let transition = self.edit_history.edit(&mut self.document, edit);
                self.history_state_id = transition.state_id;
                self.history_estimated_bytes = history::estimated_history_bytes(&self.edit_history);
                if self.history_estimated_bytes > history::history_memory_limit() {
                    self.edit_history.clear();
                    self.history_estimated_bytes = 0;
                }
            }
        }
    }

    pub(super) fn advance_document_generation(&mut self) {
        if let Some(next) = self.document_generation.checked_add(1) {
            self.document_generation = next;
        } else {
            self.session_id = NodeId::generate();
            self.document_generation = 0;
        }
    }

    fn advance_history_merge_epoch(&mut self) {
        if let Some(next) = self.history_merge_epoch.checked_add(1) {
            self.history_merge_epoch = next;
        } else {
            self.edit_history.clear();
            self.history_estimated_bytes = 0;
            self.history_merge_epoch = 0;
        }
    }

    pub(super) fn allocate_history_state_id(&mut self) -> u64 {
        let state_id = self.next_history_state_id;
        if let Some(next) = state_id.checked_add(1) {
            self.next_history_state_id = next;
        } else {
            self.edit_history.clear();
            self.history_estimated_bytes = 0;
            self.next_history_state_id = 1;
        }
        state_id
    }

    fn restore_history(&mut self, redo: bool) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let transition = if redo {
            self.edit_history.redo(&mut self.document)
        } else {
            self.edit_history.undo(&mut self.document)
        };
        let Some(transition) = transition else {
            return Ok(Effect::default());
        };
        let (
            selection,
            math_focus,
            visual_affinity,
            pending_style,
            state_id,
            changed_paragraph_id,
            document_changed,
        ) = history::restore_history_transition(transition);
        let selection_changed = self.selection != selection || self.math_focus != math_focus;
        self.selection = selection;
        self.math_focus = math_focus;
        self.selection_visual_affinity = visual_affinity;
        self.pending_inline_style = pending_style;
        self.selected_block_environment = None;
        self.block_environment_exit = None;
        self.history_state_id = state_id;
        self.advance_history_merge_epoch();
        if document_changed {
            self.paragraph_index = build_paragraph_index(&self.document.body);
            self.structure_index = Arc::new(StructureIndex::build(&self.document));
            self.text_map_cache.clear();
            if let Some(next) = self.document_generation.checked_add(1) {
                self.document_generation = next;
            } else {
                self.session_id = NodeId::generate();
                self.document_generation = 0;
            }
        }
        Ok(Effect {
            document_changed,
            selection_changed,
            changed_paragraph_id,
            ..Effect::default()
        })
    }

    fn adjacent_paragraph_for_delete(
        &mut self,
        direction: Direction,
    ) -> Result<Option<NodeId>, EditError> {
        let position = self.selection.focus.clone();
        let at_boundary = match direction {
            Direction::Backward => position.offset == 0,
            Direction::Forward => position.offset == self.text_map_for(&position)?.text.len(),
        };
        Ok(at_boundary
            .then(|| {
                history::include_adjacent_paragraph_id(
                    &self.document,
                    &self.paragraph_index,
                    &position.paragraph_id,
                    direction,
                )
            })
            .flatten())
    }

    fn set_page_geometry(&mut self, geometry: PageGeometry) -> Result<Effect, EditError> {
        geometry
            .validate()
            .map_err(|error| EditError::InvalidPageGeometry(error.to_string()))?;
        let section =
            self.document.page_sections.first_mut().ok_or_else(|| {
                EditError::InvalidDocument("document has no PageSection".to_owned())
            })?;
        let current = section
            .geometry()
            .map_err(|error| EditError::InvalidPageGeometry(error.to_string()))?;
        if current == geometry {
            return Ok(Effect::default());
        }
        section
            .set_geometry(geometry)
            .map_err(|error| EditError::InvalidPageGeometry(error.to_string()))?;
        Ok(Effect {
            document_changed: true,
            ..Effect::default()
        })
    }

    pub(super) fn paragraph_for(&self, pos: &Pos) -> Result<&Paragraph, EditError> {
        let location = self
            .paragraph_index
            .get(&pos.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))?;
        if location.story != pos.story {
            return Err(EditError::WrongStory);
        }
        paragraph_at_location(&self.document.body, location)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))
    }

    fn paragraph_for_mut(&mut self, pos: &Pos) -> Result<&mut Paragraph, EditError> {
        let location = self
            .paragraph_index
            .get(&pos.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))?;
        if location.story != pos.story {
            return Err(EditError::WrongStory);
        }
        paragraph_at_location_mut(&mut self.document.body, location)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))
    }

    pub(super) fn text_map_for(&mut self, pos: &Pos) -> Result<&TextMap, EditError> {
        if let Some(index) = self
            .text_map_cache
            .iter()
            .position(|(paragraph_id, story, _)| {
                paragraph_id == &pos.paragraph_id && story == &pos.story
            })
        {
            if index > 0 {
                let entry = self
                    .text_map_cache
                    .remove(index)
                    .ok_or(EditError::InvalidPosition)?;
                self.text_map_cache.push_front(entry);
            }
        } else {
            let paragraph = self.paragraph_for(pos)?;
            let map = TextMap::new(paragraph)?;
            self.text_map_cache
                .push_front((pos.paragraph_id.clone(), pos.story.clone(), map));
            if self.text_map_cache.len() > TEXT_MAP_CACHE_CAPACITY {
                self.text_map_cache.pop_back();
            }
        }
        self.text_map_cache
            .front()
            .map(|(_, _, map)| map)
            .ok_or(EditError::InvalidPosition)
    }

    pub(super) fn replace_paragraph(
        &mut self,
        pos: &Pos,
        paragraph: Paragraph,
    ) -> Result<(), EditError> {
        let location = self
            .paragraph_index
            .get(&pos.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))?;
        if location.story != pos.story {
            return Err(EditError::WrongStory);
        }
        *paragraph_at_location_mut(&mut self.document.body, location)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))? = paragraph;
        Ok(())
    }
}

/// 克隆创建独立编辑会话，异步布局身份与文档代际从新会话重新开始。
impl Clone for Session {
    fn clone(&self) -> Self {
        Self {
            document: self.document.clone(),
            selection: self.selection.clone(),
            selection_visual_affinity: self.selection_visual_affinity,
            session_id: NodeId::generate(),
            document_generation: 0,
            composition: self.composition.clone(),
            pending_inline_style: self.pending_inline_style.clone(),
            math_focus: self.math_focus.clone(),
            math_input_buffer: self.math_input_buffer.clone(),
            math_input_error: self.math_input_error,
            command_line: self.command_line.clone(),
            block_environment_exit: self.block_environment_exit.clone(),
            selected_block_environment: self.selected_block_environment.clone(),
            selected_block_environments: self.selected_block_environments.clone(),
            structure_selection: self.structure_selection.clone(),
            last_committed_composition: self.last_committed_composition,
            text_map_cache: self.text_map_cache.clone(),
            paragraph_index: self.paragraph_index.clone(),
            structure_index: self.structure_index.clone(),
            edit_history: self.edit_history.clone(),
            history_state_id: self.history_state_id,
            next_history_state_id: self.next_history_state_id,
            history_estimated_bytes: self.history_estimated_bytes,
            history_merge_epoch: self.history_merge_epoch,
        }
    }
}

fn is_paragraph_edit(command: &Command) -> bool {
    matches!(
        command,
        Command::InsertText(_)
            | Command::InsertInlineMath
            | Command::InsertMathText(_)
            | Command::TypeMathText(_)
            | Command::PasteMathText(_)
            | Command::InsertMathFraction
            | Command::InsertMathRadical
            | Command::InsertOneCharacterWidthSpace
            | Command::InsertTwoCharacterWidthSpace
            | Command::PasteText(_)
            | Command::InsertSoftBreak
            | Command::InsertParagraphBreak
            | Command::ToggleInlineFormat(_)
            | Command::ToggleRunUnderline
            | Command::RemoveInlineEnvironmentPreservingContent(_)
            | Command::SetInlineFontFamily(_)
            | Command::SetParagraphFirstLineIndentChars(_)
            | Command::Delete(_)
            | Command::CommitComposition { .. }
    )
}

fn position_map_changes_paragraph_structure(position_map: &PositionMap) -> bool {
    match position_map {
        PositionMap::Composite(maps) => maps.iter().any(position_map_changes_paragraph_structure),
        PositionMap::ParagraphSplit { .. }
        | PositionMap::ParagraphMerge { .. }
        | PositionMap::ParagraphRangeMerge { .. }
        | PositionMap::StoryChange { .. } => true,
        PositionMap::TextEdit { .. } => false,
    }
}

#[cfg(test)]
mod tests;
