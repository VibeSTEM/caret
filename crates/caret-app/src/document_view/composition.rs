//! 将 Qt 输入法生命周期转换为 Rust 文档 composition 命令。

use super::DocumentViewRuntime;
use caret_engine::Command;

impl DocumentViewRuntime {
    /// 将 Qt 输入法预编辑、替换和提交事件交给 Rust Session。
    pub fn input_method_event_json(
        &mut self,
        preedit: String,
        commit: String,
        replacement_start: i32,
        replacement_length: i32,
        cursor_position_utf16: usize,
    ) -> Result<String, String> {
        let had_composition = self.composition_id.is_some();
        let has_commit = !commit.is_empty();
        let has_replacement = replacement_start != 0 || replacement_length != 0;
        let replace_active_composition = had_composition && has_replacement;
        let replacement_without_commit = !had_composition && !has_commit && has_replacement;
        let commit_current = has_commit || replace_active_composition || replacement_without_commit;

        if commit_current {
            if self.composition_id.is_some() && has_replacement {
                self.commit_ime_replacement(
                    replacement_start,
                    replacement_length,
                    &commit,
                    self.composition_id,
                )?;
            } else if let Some(id) = self.composition_id {
                self.session
                    .run(Command::CommitComposition { id, text: commit })
                    .map_err(|error| format!("ime.commit: {error}"))?;
            } else {
                self.commit_ime_replacement(replacement_start, replacement_length, &commit, None)?;
            }
            self.composition_id = None;
            self.composition_preview_session = None;
            self.composition_snapshot = None;
            self.composition_range = None;
            self.anchor_visual_affinity = self.session.selection_visual_affinity();
            self.preferred_visual_x_pt = None;
        }

        if !preedit.is_empty() {
            let replacement_range = if self.composition_id.is_none() && !commit_current {
                Some((replacement_start, replacement_length))
            } else {
                None
            };
            let id = match self.composition_id {
                Some(id) => id,
                None => self.allocate_composition_id()?,
            };
            return match self.install_composition_update(
                id,
                preedit,
                replacement_range,
                cursor_position_utf16,
            ) {
                Ok(response) => Ok(response),
                Err(_) if commit_current => self.layout_snapshot_json(),
                Err(error) => Err(error),
            };
        } else if !commit_current && let Some(id) = self.composition_id {
            self.session
                .run(Command::CancelComposition { id })
                .map_err(|error| format!("ime.cancel: {error}"))?;
            self.composition_id = None;
            self.composition_preview_session = None;
            self.composition_snapshot = None;
            self.composition_range = None;
        }

        self.preedit_cursor_position_utf16 = cursor_position_utf16;
        self.composition_preview_session = None;
        self.composition_snapshot = None;
        self.composition_range = None;
        if commit_current || replacement_length != 0 || had_composition {
            self.layout_snapshot_json()
        } else {
            self.interaction_state_json()
        }
    }

    fn commit_ime_replacement(
        &mut self,
        replacement_start: i32,
        replacement_length: i32,
        text: &str,
        active_composition_id: Option<u64>,
    ) -> Result<(), String> {
        let selection_removal_id = self.allocate_composition_id()?;
        let id = self.allocate_composition_id()?;
        self.session
            .commit_composition_replacement(
                active_composition_id,
                selection_removal_id,
                id,
                replacement_start,
                replacement_length,
                text,
            )
            .map_err(|error| format!("ime.replacement: {error}"))?;
        Ok(())
    }

    fn install_composition_update(
        &mut self,
        id: u64,
        preedit: String,
        replacement_range: Option<(i32, i32)>,
        cursor_position_utf16: usize,
    ) -> Result<String, String> {
        let (candidate, preview, range) = self
            .session
            .composition_update_candidate(id, preedit, replacement_range, cursor_position_utf16)
            .map_err(|error| format!("ime.preview: {error}"))?;
        let snapshot = self
            .composition_snapshot_for(&preview, &range)
            .map_err(|error| format!("ime.preview_layout: {error}"))?;

        self.session = candidate;
        self.composition_id = Some(id);
        self.composition_preview_session = Some(preview);
        self.composition_snapshot = Some(snapshot);
        self.composition_range = Some(range);
        self.preedit_cursor_position_utf16 = cursor_position_utf16;
        self.layout_snapshot_json()
    }

    fn allocate_composition_id(&mut self) -> Result<u64, String> {
        let id = self.next_composition_id;
        self.next_composition_id = id
            .checked_add(1)
            .ok_or_else(|| "ime.composition_id_exhausted".to_owned())?;
        Ok(id)
    }
}
