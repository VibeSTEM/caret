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
        let previous_layout = if commit_current {
            Some(self.base_snapshot()?.clone())
        } else {
            None
        };
        let selection_before = self.session.selection().clone();
        let composition_range_before = self.composition_range.clone();
        let selection_is_single_paragraph =
            selection_before.anchor.paragraph_id == selection_before.focus.paragraph_id;
        let reusable_paragraph_id = selection_is_single_paragraph.then(|| {
            composition_range_before
                .as_ref()
                .filter(|range| range.anchor.paragraph_id == range.focus.paragraph_id)
                .map(|range| range.anchor.paragraph_id.clone())
                .unwrap_or_else(|| selection_before.focus.paragraph_id.clone())
        });
        let previous_preview_page_start = previous_layout.as_ref().and_then(|previous| {
            let preview = self.composition_snapshot.as_ref()?;
            let paragraph_id = reusable_paragraph_id.as_ref()?;
            super::layout::first_changed_page_index(previous, preview, paragraph_id)
        });
        let mut committed_delta = None;

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

        if let (Some(previous), Some(paragraph_id)) =
            (previous_layout.as_ref(), reusable_paragraph_id.as_ref())
        {
            self.relayout_reusing_paragraph(previous, paragraph_id)?;
            let updated = self
                .last_snapshot
                .as_ref()
                .ok_or_else(|| "layout.snapshot_unavailable".to_owned())?;
            let page_start_index =
                super::layout::first_changed_page_index(previous, updated, paragraph_id).map(
                    |page_start_index| {
                        previous_preview_page_start.map_or(page_start_index, |preview_start| {
                            page_start_index.min(preview_start)
                        })
                    },
                );
            if let Some(page_start_index) = page_start_index {
                committed_delta = Some((
                    paragraph_id.clone(),
                    page_start_index,
                    previous.document_generation,
                ));
            }
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
            if let Some((paragraph_id, page_start_index, base_generation)) = committed_delta {
                self.layout_snapshot_json_with_delta(Some((
                    &paragraph_id,
                    page_start_index,
                    base_generation,
                )))
            } else {
                self.layout_snapshot_json()
            }
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
        let delta = self.composition_preview_delta_for(&snapshot, &range);

        self.session.install_composition_candidate(candidate);
        self.composition_id = Some(id);
        self.composition_preview_session = Some(preview);
        self.composition_snapshot = Some(snapshot);
        self.composition_range = Some(range);
        self.preedit_cursor_position_utf16 = cursor_position_utf16;
        match delta {
            Some((paragraph_id, page_start_index, base_generation)) => self
                .layout_snapshot_json_with_delta(Some((
                    &paragraph_id,
                    page_start_index,
                    base_generation,
                ))),
            None => self.layout_snapshot_json(),
        }
    }

    fn allocate_composition_id(&mut self) -> Result<u64, String> {
        let id = self.next_composition_id;
        self.next_composition_id = id
            .checked_add(1)
            .ok_or_else(|| "ime.composition_id_exhausted".to_owned())?;
        Ok(id)
    }
}
