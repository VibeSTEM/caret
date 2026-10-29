//! 将文本编辑命令应用到 Rust 会话并更新布局。
use super::DocumentViewRuntime;
use caret_engine::{Command, Direction, DocumentLayoutDeltaView, EditorKey};

impl DocumentViewRuntime {
    /// 将 Qt 编辑器按键意图交给 Session 裁决，并按结果发布布局或交互快照。
    pub fn handle_editor_key_json(&mut self, key: &str) -> Result<String, String> {
        let key = match key {
            "escape" => EditorKey::Escape,
            "enter" => EditorKey::Enter,
            "shift-enter" => EditorKey::ShiftEnter,
            "mod-backspace" => EditorKey::ModBackspace,
            _ => return Err("interaction.unknown_editor_key".to_owned()),
        };
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
        if is_command_line_document_edit(&self.session, &command) {
            return self.run_edit_command(command, &format!("cmdline.{action}"));
        }
        self.session
            .run(command)
            .map_err(|error| format!("cmdline.{action}: {error}"))?;
        self.command_line_state_json()
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
        let effect = self
            .session
            .run(command)
            .map_err(|error| format!("{context}: {error}"))?;
        let previous = self.last_snapshot.take();
        self.anchor_visual_affinity = self.session.selection_visual_affinity();
        self.preferred_visual_x_pt = None;

        let (Some(previous), Some(paragraph_id)) = (previous, effect.changed_paragraph_id) else {
            self.last_snapshot = None;
            return self.layout_snapshot_json();
        };
        let base_generation = previous.document_generation;
        let updated = self
            .layout
            .layout_session_reusing(&self.session, &previous, &paragraph_id)
            .map_err(|error| error.to_string())?;
        let page_start_index = caret_engine::DocumentLayoutDeltaView::first_affected_page_index(
            &previous,
            &updated,
            &paragraph_id,
        );
        self.last_snapshot = Some(updated);
        match page_start_index {
            Some(page_start_index) => self.layout_snapshot_json_with_delta(Some((
                &paragraph_id,
                page_start_index,
                base_generation,
            ))),
            None => self.layout_snapshot_json(),
        }
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
        self.session
            .run(Command::InsertText(text))
            .map_err(|error| format!("text.insert: {error}"))?;
        self.anchor_visual_affinity = self.session.selection_visual_affinity();
        self.preferred_visual_x_pt = None;
        let mut page_start_index = None;
        let mut base_generation = None;
        if let Some(paragraph_id) = reusable_paragraph_id.as_ref() {
            let previous = self
                .last_snapshot
                .as_ref()
                .ok_or_else(|| "layout.snapshot_unavailable".to_owned())?;
            base_generation = Some(previous.document_generation);
            let updated = self
                .layout
                .layout_session_reusing(&self.session, previous, paragraph_id)
                .map_err(|error| error.to_string())?;
            page_start_index = DocumentLayoutDeltaView::first_affected_page_index(
                previous,
                &updated,
                paragraph_id,
            );
            if page_start_index.is_none() {
                return Err("layout.changed_paragraph_page_unavailable".to_owned());
            }
            self.last_snapshot = Some(updated);
        } else {
            self.last_snapshot = None;
        }
        match reusable_paragraph_id.as_ref().filter(|_| allow_delta) {
            Some(paragraph_id) => self.layout_snapshot_json_with_delta(Some((
                paragraph_id,
                page_start_index.ok_or_else(|| "layout.changed_page_unavailable".to_owned())?,
                base_generation
                    .ok_or_else(|| "layout.delta_base_generation_unavailable".to_owned())?,
            ))),
            None => self.layout_snapshot_json(),
        }
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
        let effect = self
            .session
            .run(command)
            .map_err(|error| format!("{error_context}: {error}"))?;
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
            let previous = self
                .last_snapshot
                .as_ref()
                .ok_or_else(|| "layout.snapshot_unavailable".to_owned())?;
            let base_generation = previous.document_generation;
            let updated = self
                .layout
                .layout_session_reusing(&self.session, previous, paragraph_id)
                .map_err(|error| error.to_string())?;
            if delete_is_paragraph_local {
                let page_start_index = DocumentLayoutDeltaView::first_affected_page_index(
                    previous,
                    &updated,
                    paragraph_id,
                )
                .ok_or_else(|| "layout.changed_paragraph_page_unavailable".to_owned())?;
                delete_delta = Some((paragraph_id, page_start_index, base_generation));
            }
            self.last_snapshot = Some(updated);
        }
        self.layout_snapshot_json_with_delta(delete_delta)
    }
}

fn is_command_line_document_edit(session: &caret_engine::Session, command: &Command) -> bool {
    let Some(line) = session.command_line() else {
        return false;
    };
    let candidates = session.command_line_candidates();
    let selected = candidates.get(line.selected).copied();
    match command {
        Command::TypeCommandLine(text) if text == "\\" => {
            line.query.is_empty() || selected.is_some_and(|candidate| candidate.available)
        }
        Command::TabCommandLine => line.query.is_empty(),
        Command::AcceptCommandLine => selected.is_some_and(|candidate| candidate.available),
        _ => false,
    }
}

fn is_editor_key_document_edit(session: &caret_engine::Session, key: EditorKey) -> bool {
    match key {
        EditorKey::Enter if session.command_line().is_some() => {
            is_command_line_document_edit(session, &Command::AcceptCommandLine)
        }
        EditorKey::Enter => true,
        EditorKey::ShiftEnter => session.command_line().is_none(),
        EditorKey::ModBackspace => session.command_line().is_none(),
        EditorKey::Escape => false,
    }
}
