//! 将文本编辑命令应用到 Rust 会话并更新布局。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::DocumentViewRuntime;
use caret_engine::{
    Command, Direction, DocumentLayoutDeltaView, DocumentLayoutSnapshot, EditorKey,
    SessionCheckpoint, VisualAffinity,
};

pub(super) struct DocumentEditCheckpoint {
    session: SessionCheckpoint,
    snapshot: Option<DocumentLayoutSnapshot>,
    affinity: VisualAffinity,
    preferred_x_pt: Option<f32>,
    composition_id: Option<u64>,
    next_composition_id: u64,
    preedit_cursor_position_utf16: usize,
    composition_preview_session: Option<caret_engine::Session>,
    composition_snapshot: Option<DocumentLayoutSnapshot>,
    composition_range: Option<caret_engine::Selection>,
    last_interaction_context: Option<(u64, caret_doc::NodeId, bool)>,
}

impl DocumentViewRuntime {
    /// 将 Qt 编辑器按键意图交给 Session 裁决，并按结果发布布局或交互快照。
    pub fn handle_editor_key_json(&mut self, key: &str) -> Result<String, String> {
        let key = match key {
            "escape" => EditorKey::Escape,
            "enter" => EditorKey::Enter,
            "tab" => EditorKey::Tab,
            "shift-enter" => EditorKey::ShiftEnter,
            "mod-backspace" => EditorKey::ModBackspace,
            _ => return Err("interaction.unknown_editor_key".to_owned()),
        };
        if key == EditorKey::Enter
            && is_font_picker_request(&self.session, &Command::AcceptCommandLine)
        {
            self.command_line_command_json("accept", String::new())?;
            return self.interaction_state_json();
        }
        let command = Command::EditorKey(key);
        if is_editor_key_document_edit(&self.session, key) {
            let error_context = command.id();
            return self.run_edit_command(command, error_context);
        }

        let error_context = command.id();
        let effect = self
            .session
            .run(command)
            .map_err(|error| format!("{error_context}: {error}"))?;
        if effect.composition_changed {
            self.composition_id = None;
            self.composition_preview_session = None;
            self.composition_snapshot = None;
            self.composition_range = None;
            return self.layout_snapshot_json_after_composition_cancel();
        }
        if effect.selection_changed {
            self.anchor_visual_affinity = self.session.selection_visual_affinity();
            self.preferred_visual_x_pt = None;
        }
        if effect.document_changed {
            return self.layout_snapshot_json();
        }
        self.interaction_state_json()
    }

    /// 更新 Rust 命令查询；只有确认可执行候选时才生成文档布局快照。
    pub fn command_line_command_json(
        &mut self,
        action: &str,
        text: String,
    ) -> Result<String, String> {
        let command = match action {
            "begin" => Command::BeginCommandLine,
            "type" => Command::TypeCommandLine(text),
            "backspace" => Command::BackspaceCommandLine,
            "cursor-left" => Command::MoveCommandLineCursor(Direction::Backward),
            "cursor-right" => Command::MoveCommandLineCursor(Direction::Forward),
            "previous" => Command::MoveCommandLineSelection(Direction::Backward),
            "next" => Command::MoveCommandLineSelection(Direction::Forward),
            "select" => Command::SelectCommandLineCandidate(
                text.parse()
                    .map_err(|_| "cmdline.invalid_candidate_index")?,
            ),
            "cancel" => Command::CancelCommandLine,
            "tab" => Command::TabCommandLine,
            "accept" => Command::AcceptCommandLine,
            _ => return Err("cmdline.unknown_action".to_owned()),
        };
        if is_font_picker_request(&self.session, &command) {
            let font_families = self.layout.font_family_names();
            let font_targets = font_target_choices(self.layout.font_script_choices());
            self.session
                .run_command_line_with_font_choices(command, font_families, font_targets)
                .map_err(|error| format!("cmdline.{action}: {error}"))?;
            return self.interaction_state_json();
        }
        if is_command_line_document_edit(&self.session, &command) {
            return self.run_edit_command(command, &format!("cmdline.{action}"));
        }
        let effect = self
            .session
            .run(command)
            .map_err(|error| format!("cmdline.{action}: {error}"))?;
        if effect.composition_changed {
            self.composition_id = None;
            self.composition_preview_session = None;
            self.composition_snapshot = None;
            self.composition_range = None;
            return self.layout_snapshot_json_after_composition_cancel();
        }
        self.interaction_state_json()
    }

    /// 在 Rust Session 中撤销最近一次可撤销编辑并发布更新后的布局。
    pub fn undo_json(&mut self) -> Result<String, String> {
        self.restore_history_json(false)
    }

    /// 在 Rust Session 中重做最近一次可重做编辑并发布更新后的布局。
    pub fn redo_json(&mut self) -> Result<String, String> {
        self.restore_history_json(true)
    }

    fn restore_history_json(&mut self, redo: bool) -> Result<String, String> {
        let command = if redo { Command::Redo } else { Command::Undo };
        let context = if redo { "history.redo" } else { "history.undo" };
        let checkpoint = self.edit_checkpoint();
        let previous = checkpoint.snapshot.clone();
        let rollback_snapshot = previous.clone();
        let effect = match self.session.run(command) {
            Ok(effect) => effect,
            Err(error) => {
                self.rollback_failed_layout_edit(checkpoint);
                return Err(format!("{context}: {error}"));
            }
        };
        self.anchor_visual_affinity = self.session.selection_visual_affinity();
        self.preferred_visual_x_pt = None;

        let (Some(previous), Some(paragraph_id)) = (previous, effect.changed_paragraph_id) else {
            self.last_snapshot = None;
            return match self.layout_snapshot_json() {
                Ok(snapshot) => Ok(snapshot),
                Err(error) => {
                    let mut checkpoint = checkpoint;
                    checkpoint.snapshot = rollback_snapshot;
                    self.rollback_failed_layout_edit(checkpoint);
                    Err(error)
                }
            };
        };
        let base_generation = previous.document_generation;
        let updated =
            match self
                .layout
                .layout_session_reusing(&self.session, &previous, &paragraph_id)
            {
                Ok(updated) => updated,
                Err(error) => {
                    let mut checkpoint = checkpoint;
                    checkpoint.snapshot = Some(previous);
                    self.rollback_failed_layout_edit(checkpoint);
                    return Err(error.to_string());
                }
            };
        let page_start_index = caret_engine::DocumentLayoutDeltaView::first_affected_page_index(
            &previous,
            &updated,
            &paragraph_id,
        );
        self.last_snapshot = Some(updated);
        let result = match page_start_index {
            Some(page_start_index) => self.layout_snapshot_json_with_delta(Some((
                &paragraph_id,
                page_start_index,
                base_generation,
            ))),
            None => self.layout_snapshot_json(),
        };
        if result.is_err() {
            let mut checkpoint = checkpoint;
            checkpoint.snapshot = rollback_snapshot;
            self.rollback_failed_layout_edit(checkpoint);
        }
        result
    }

    /// 将 Qt 直接输入的文本交给 Rust Session，再返回新布局快照。
    pub fn insert_text_json(&mut self, text: String) -> Result<String, String> {
        self.insert_text_with_response(text, false)
    }

    /// 为 Qt 普通单段输入返回完整分页和当前段落布局的增量快照。
    pub fn insert_text_delta_json(&mut self, text: String) -> Result<String, String> {
        self.insert_text_with_response(text, true)
    }

    fn insert_text_with_response(
        &mut self,
        text: String,
        allow_delta: bool,
    ) -> Result<String, String> {
        self.base_snapshot()?;
        let selection = self.session.selection();
        let reusable_paragraph_id = (self.session.block_environment_exit().is_none()
            && self.session.selected_block_environment().is_none()
            && selection.anchor.paragraph_id == selection.focus.paragraph_id)
            .then(|| selection.focus.paragraph_id.clone());
        let checkpoint = self.edit_checkpoint();
        let previous_snapshot = checkpoint.snapshot.clone();
        if let Err(error) = self.session.run(Command::InsertText(text)) {
            let mut checkpoint = checkpoint;
            checkpoint.snapshot = previous_snapshot;
            self.rollback_failed_layout_edit(checkpoint);
            return Err(format!("text.insert: {error}"));
        }
        self.anchor_visual_affinity = self.session.selection_visual_affinity();
        self.preferred_visual_x_pt = None;
        let mut page_start_index = None;
        let mut base_generation = None;
        if let Some(paragraph_id) = reusable_paragraph_id.as_ref() {
            let Some(previous) = self.last_snapshot.clone() else {
                let mut checkpoint = checkpoint;
                checkpoint.snapshot = previous_snapshot;
                self.rollback_failed_layout_edit(checkpoint);
                return Err("layout.snapshot_unavailable".to_owned());
            };
            base_generation = Some(previous.document_generation);
            let updated =
                match self
                    .layout
                    .layout_session_reusing(&self.session, &previous, paragraph_id)
                {
                    Ok(updated) => updated,
                    Err(error) => {
                        let mut checkpoint = checkpoint;
                        checkpoint.snapshot = previous_snapshot;
                        self.rollback_failed_layout_edit(checkpoint);
                        return Err(error.to_string());
                    }
                };
            page_start_index = DocumentLayoutDeltaView::first_affected_page_index(
                &previous,
                &updated,
                paragraph_id,
            );
            if page_start_index.is_none() {
                let mut checkpoint = checkpoint;
                checkpoint.snapshot = previous_snapshot;
                self.rollback_failed_layout_edit(checkpoint);
                return Err("layout.changed_paragraph_page_unavailable".to_owned());
            }
            self.last_snapshot = Some(updated);
        } else {
            self.last_snapshot = None;
        }
        let result = match reusable_paragraph_id.as_ref().filter(|_| allow_delta) {
            Some(paragraph_id) => match (page_start_index, base_generation) {
                (Some(page_start_index), Some(base_generation)) => self
                    .layout_snapshot_json_with_delta(Some((
                        paragraph_id,
                        page_start_index,
                        base_generation,
                    ))),
                (None, _) => Err("layout.changed_page_unavailable".to_owned()),
                (_, None) => Err("layout.delta_base_generation_unavailable".to_owned()),
            },
            None => self.layout_snapshot_json(),
        };
        if result.is_err() {
            let mut checkpoint = checkpoint;
            checkpoint.snapshot = previous_snapshot;
            self.rollback_failed_layout_edit(checkpoint);
        }
        result
    }

    /// 将纯文本剪贴板内容作为一次原子事务插入，换行会拆为段落。
    pub fn paste_text_json(&mut self, text: String) -> Result<String, String> {
        self.run_edit_command(Command::PasteText(text), "clipboard.paste")
    }

    /// 在 Rust Session 中拆分当前段落并返回新布局。
    pub fn split_paragraph_json(&mut self) -> Result<String, String> {
        self.run_edit_command(Command::InsertParagraphBreak, "text.split_paragraph")
    }

    /// 在当前插入点删除前一个字素；位于段首时与前一段合并。
    pub fn delete_backward_json(&mut self) -> Result<String, String> {
        self.run_edit_command(Command::Delete(Direction::Backward), "text.delete_backward")
    }

    /// 在当前段落内插入软换行，不创建新段落。
    pub fn insert_soft_break_json(&mut self) -> Result<String, String> {
        self.run_edit_command(Command::InsertSoftBreak, "text.soft_break")
    }

    fn run_edit_command(
        &mut self,
        command: Command,
        error_context: &str,
    ) -> Result<String, String> {
        let selection = self.session.selection();
        let reusable_paragraph_id = (self.session.selected_block_environment().is_none()
            && selection.anchor.paragraph_id == selection.focus.paragraph_id)
            .then(|| selection.focus.paragraph_id.clone());
        let delete_is_paragraph_local = matches!(&command, Command::Delete(Direction::Backward))
            && reusable_paragraph_id.is_some()
            && (selection.anchor.offset != selection.focus.offset || selection.focus.offset > 0);
        let can_reuse_layout = matches!(
            &command,
            Command::InsertParagraphBreak | Command::InsertSoftBreak
        ) || matches!(&command, Command::PasteText(_))
            && reusable_paragraph_id.is_some()
            || delete_is_paragraph_local
            || is_command_line_document_edit(&self.session, &command)
            || matches!(&command, Command::EditorKey(key) if is_editor_key_document_edit(&self.session, *key));
        if can_reuse_layout {
            self.base_snapshot()?;
        }
        let checkpoint = self.edit_checkpoint();
        let previous_snapshot = checkpoint.snapshot.clone();
        let effect = match self.session.run(command) {
            Ok(effect) => effect,
            Err(error) => {
                let mut checkpoint = checkpoint;
                checkpoint.snapshot = previous_snapshot;
                self.rollback_failed_layout_edit(checkpoint);
                return Err(format!("{error_context}: {error}"));
            }
        };
        if effect.composition_changed {
            self.composition_id = None;
            self.composition_preview_session = None;
            self.composition_snapshot = None;
            self.composition_range = None;
        }
        self.anchor_visual_affinity = self.session.selection_visual_affinity();
        self.preferred_visual_x_pt = None;
        let mut delete_delta = None;
        if can_reuse_layout && let Some(paragraph_id) = reusable_paragraph_id.as_ref() {
            let Some(previous) = self.last_snapshot.clone() else {
                let mut checkpoint = checkpoint;
                checkpoint.snapshot = previous_snapshot;
                self.rollback_failed_layout_edit(checkpoint);
                return Err("layout.snapshot_unavailable".to_owned());
            };
            let base_generation = previous.document_generation;
            let updated =
                match self
                    .layout
                    .layout_session_reusing(&self.session, &previous, paragraph_id)
                {
                    Ok(updated) => updated,
                    Err(error) => {
                        let mut checkpoint = checkpoint;
                        checkpoint.snapshot = previous_snapshot;
                        self.rollback_failed_layout_edit(checkpoint);
                        return Err(error.to_string());
                    }
                };
            if delete_is_paragraph_local {
                let Some(page_start_index) = DocumentLayoutDeltaView::first_affected_page_index(
                    &previous,
                    &updated,
                    paragraph_id,
                ) else {
                    let mut checkpoint = checkpoint;
                    checkpoint.snapshot = previous_snapshot;
                    self.rollback_failed_layout_edit(checkpoint);
                    return Err("layout.changed_paragraph_page_unavailable".to_owned());
                };
                delete_delta = Some((paragraph_id, page_start_index, base_generation));
            }
            self.last_snapshot = Some(updated);
        }
        let result = self.layout_snapshot_json_with_delta(delete_delta);
        if result.is_err() {
            let mut checkpoint = checkpoint;
            checkpoint.snapshot = previous_snapshot;
            self.rollback_failed_layout_edit(checkpoint);
        }
        result
    }

    pub(super) fn edit_checkpoint(&mut self) -> DocumentEditCheckpoint {
        DocumentEditCheckpoint {
            session: self.session.checkpoint(),
            snapshot: self.last_snapshot.clone(),
            affinity: self.anchor_visual_affinity,
            preferred_x_pt: self.preferred_visual_x_pt,
            composition_id: self.composition_id,
            next_composition_id: self.next_composition_id,
            preedit_cursor_position_utf16: self.preedit_cursor_position_utf16,
            composition_preview_session: self.composition_preview_session.clone(),
            composition_snapshot: self.composition_snapshot.clone(),
            composition_range: self.composition_range.clone(),
            last_interaction_context: self.last_interaction_context.clone(),
        }
    }

    pub(super) fn rollback_failed_layout_edit(&mut self, checkpoint: DocumentEditCheckpoint) {
        let restored = self.session.restore_checkpoint(checkpoint.session);
        debug_assert!(restored, "edit checkpoint belongs to this Session");
        self.last_snapshot = checkpoint.snapshot;
        self.anchor_visual_affinity = checkpoint.affinity;
        self.preferred_visual_x_pt = checkpoint.preferred_x_pt;
        self.composition_id = checkpoint.composition_id;
        self.next_composition_id = checkpoint.next_composition_id;
        self.preedit_cursor_position_utf16 = checkpoint.preedit_cursor_position_utf16;
        self.composition_preview_session = checkpoint.composition_preview_session;
        self.composition_snapshot = checkpoint.composition_snapshot;
        self.composition_range = checkpoint.composition_range;
        self.last_interaction_context = checkpoint.last_interaction_context;
        if restored {
            let generation = self.session.document_generation();
            if let Some(snapshot) = self.last_snapshot.as_mut() {
                snapshot.session_id = Some(self.session.session_id().clone());
                snapshot.document_generation = generation;
            }
            if let Some(snapshot) = self.composition_snapshot.as_mut() {
                if let Some(preview) = self.composition_preview_session.as_ref() {
                    snapshot.session_id = Some(preview.session_id().clone());
                    snapshot.document_generation = preview.document_generation();
                }
                snapshot.publication_session_id = Some(self.session.session_id().clone());
                snapshot.publication_generation = Some(generation);
            }
        }
    }
}

fn is_command_line_document_edit(session: &caret_engine::Session, command: &Command) -> bool {
    let Some(line) = session.command_line() else {
        return false;
    };
    if !line.preedit.is_empty() {
        return false;
    }
    let candidates = session.command_line_candidates();
    let selected = candidates.get(line.selected);
    if matches!(
        line.mode,
        caret_engine::CommandLineMode::DefaultFontTarget
            | caret_engine::CommandLineMode::DefaultFontScope
            | caret_engine::CommandLineMode::DefaultFontLanguage
    ) {
        return false;
    }
    if matches!(
        line.mode,
        caret_engine::CommandLineMode::FontFamily
            | caret_engine::CommandLineMode::DefaultFontFamily
    ) {
        return matches!(
            command,
            Command::AcceptCommandLine | Command::TabCommandLine
        ) && selected.is_some_and(|candidate| {
            candidate.available
                && matches!(
                    candidate.command_id,
                    "text.format.font_family" | "document.default_font.family"
                )
        });
    }
    match command {
        Command::TypeCommandLine(text) if text == "\\" => {
            line.query.is_empty()
                || selected.is_some_and(|candidate| {
                    candidate.available
                        && !matches!(
                            candidate.command_id,
                            "text.format.font" | "document.default_font"
                        )
                })
        }
        Command::TabCommandLine => line.query.is_empty(),
        Command::AcceptCommandLine => selected.is_some_and(|candidate| {
            candidate.available
                && !matches!(
                    candidate.command_id,
                    "text.format.font" | "document.default_font"
                )
        }),
        _ => false,
    }
}

fn is_font_picker_request(session: &caret_engine::Session, command: &Command) -> bool {
    let Some(line) = session.command_line() else {
        return false;
    };
    let accepts_candidate = matches!(command, Command::AcceptCommandLine)
        || matches!(command, Command::TypeCommandLine(text) if text == "\\" && !line.query.is_empty());
    if line.mode != caret_engine::CommandLineMode::Command || !accepts_candidate {
        return false;
    }
    session
        .command_line_candidates()
        .get(line.selected)
        .is_some_and(|candidate| {
            candidate.available
                && matches!(
                    candidate.command_id,
                    "text.format.font" | "document.default_font"
                )
        })
}

fn font_target_choices(
    scripts: Vec<(String, String)>,
) -> Vec<(String, caret_doc::DocumentFontTarget)> {
    let mut scripts = scripts;
    scripts.sort_by_key(|(tag, _)| (tag != "Latn", tag.clone()));
    scripts
        .into_iter()
        .map(|(tag, name)| {
            (
                format!("{tag} · {name}"),
                caret_doc::DocumentFontTarget::Script(tag),
            )
        })
        .collect()
}

fn is_editor_key_document_edit(session: &caret_engine::Session, key: EditorKey) -> bool {
    match key {
        EditorKey::Enter if session.command_line().is_some() => {
            is_command_line_document_edit(session, &Command::AcceptCommandLine)
        }
        EditorKey::Tab => {
            session.command_line().is_none()
                || is_command_line_document_edit(session, &Command::TabCommandLine)
        }
        EditorKey::Enter => true,
        EditorKey::ShiftEnter => session.command_line().is_none(),
        EditorKey::ModBackspace => session.command_line().is_none(),
        EditorKey::Escape => false,
    }
}
