//! 管理文档布局快照的缓存、复用与 composition 增量。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::DocumentViewRuntime;
use caret_doc::NodeId;
use caret_engine::{DocumentLayoutDeltaView, DocumentLayoutSnapshot, Selection, Session};

impl DocumentViewRuntime {
    pub(super) fn composition_preview_delta_for(
        &self,
        updated: &DocumentLayoutSnapshot,
        range: &Selection,
    ) -> Option<(NodeId, usize, u64)> {
        let selection = self.session.selection();
        if selection.anchor.paragraph_id != selection.focus.paragraph_id {
            return None;
        }
        if range.anchor.paragraph_id != range.focus.paragraph_id {
            return None;
        }
        let previous = self.last_snapshot.as_ref()?;
        let paragraph_id = range.anchor.paragraph_id.clone();
        let mut page_start_index = first_changed_page_index(previous, updated, &paragraph_id)?;
        if let Some(previous_preview) = self.composition_snapshot.as_ref() {
            let preview_page_start =
                first_changed_page_index(previous_preview, updated, &paragraph_id)?;
            page_start_index = page_start_index.min(preview_page_start);
        }
        Some((paragraph_id, page_start_index, previous.document_generation))
    }

    pub(super) fn current_snapshot(&mut self) -> Result<&DocumentLayoutSnapshot, String> {
        if self.composition_preview_session.is_some() {
            return self
                .composition_snapshot
                .as_ref()
                .ok_or_else(|| "ime.composition_layout_unavailable".to_owned());
        }
        self.base_snapshot()
    }

    pub(super) fn relayout_reusing_paragraph(
        &mut self,
        previous: &DocumentLayoutSnapshot,
        changed_paragraph_id: &NodeId,
    ) -> Result<(), String> {
        let snapshot = self
            .layout
            .layout_session_reusing(&self.session, previous, changed_paragraph_id)
            .map_err(|error| error.to_string())?;
        self.last_snapshot = Some(snapshot);
        Ok(())
    }

    pub(super) fn base_snapshot(&mut self) -> Result<&DocumentLayoutSnapshot, String> {
        let font_generation = self.layout.font_generation();
        let current = self
            .last_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.is_current_for(&self.session, font_generation));
        if !current {
            self.last_snapshot = Some(
                self.layout
                    .layout_session(&self.session)
                    .map_err(|error| error.to_string())?,
            );
        }
        self.last_snapshot
            .as_ref()
            .ok_or_else(|| "layout.snapshot_unavailable".to_owned())
    }

    pub(super) fn composition_snapshot_for(
        &mut self,
        preview_session: &Session,
        composition_range: &Selection,
    ) -> Result<DocumentLayoutSnapshot, String> {
        self.base_snapshot()?;
        let changed_paragraph_id = composition_range.anchor.paragraph_id.clone();
        let snapshot = self
            .layout
            .layout_composition_preview_reusing(
                &self.session,
                preview_session,
                self.last_snapshot
                    .as_ref()
                    .ok_or_else(|| "layout.snapshot_unavailable".to_owned())?,
                &changed_paragraph_id,
            )
            .map_err(|error| error.to_string())?;
        Ok(snapshot)
    }
}

pub(super) fn first_changed_page_index(
    previous: &DocumentLayoutSnapshot,
    current: &DocumentLayoutSnapshot,
    paragraph_id: &NodeId,
) -> Option<usize> {
    DocumentLayoutDeltaView::first_affected_page_index(previous, current, paragraph_id)
}
