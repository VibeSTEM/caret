//! 执行行内格式环境命令并管理当前插入点的待输入格式。

use super::super::paragraph_ops::{compare_positions, paragraph_at_location};
use super::super::session::history::{DocumentPatch, SessionHistoryEdit, SessionHistoryState};
use super::super::text_map::TextMap;
use super::super::{Affinity, EditError, Effect, Pos};
use super::Session;
use caret_doc::{InlineEnvironmentKind, NodeId};
use inline_tree::{
    apply_formats_to_range, effective_format_at, format_spans, format_value, wrap_format_range,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PendingInlineStyle {
    bold: Option<bool>,
    italic: Option<bool>,
}

impl PendingInlineStyle {
    pub(super) fn clear(&mut self) {
        self.bold = None;
        self.italic = None;
    }

    fn get(&self, kind: InlineEnvironmentKind) -> Option<bool> {
        match kind {
            InlineEnvironmentKind::Bold => self.bold,
            InlineEnvironmentKind::Italic => self.italic,
            InlineEnvironmentKind::Font => None,
        }
    }

    fn set(&mut self, kind: InlineEnvironmentKind, value: bool) -> Result<(), EditError> {
        match kind {
            InlineEnvironmentKind::Bold => self.bold = Some(value),
            InlineEnvironmentKind::Italic => self.italic = Some(value),
            InlineEnvironmentKind::Font => return Err(EditError::UnsupportedInlineFormat),
        }
        Ok(())
    }

    pub(super) fn active_formats(
        &self,
    ) -> impl Iterator<Item = (InlineEnvironmentKind, bool)> + '_ {
        [
            self.bold.map(|value| (InlineEnvironmentKind::Bold, value)),
            self.italic
                .map(|value| (InlineEnvironmentKind::Italic, value)),
        ]
        .into_iter()
        .flatten()
    }
}

impl Session {
    pub(super) fn toggle_inline_format(
        &mut self,
        kind: InlineEnvironmentKind,
    ) -> Result<Effect, EditError> {
        if matches!(kind, InlineEnvironmentKind::Font) {
            return Err(EditError::UnsupportedInlineFormat);
        }
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }

        let (anchor, focus) = self.edit_range()?;
        if anchor == focus {
            let position = self.selection.focus.clone();
            let inherited = effective_format_at(
                &self.paragraph_for(&position)?.inlines,
                position.offset,
                position.affinity,
                kind,
            )?;
            let current = self.pending_inline_style.get(kind).unwrap_or(inherited);
            let before_pending_style = self.pending_inline_style.clone();
            let before_selection = self.selection.clone();
            let before_visual_affinity = self.selection_visual_affinity;
            let before_state_id = self.history_state_id;
            self.pending_inline_style.set(kind, !current)?;
            self.advance_history_merge_epoch();
            let after_state_id = self.allocate_history_state_id();
            self.record_history_edit(SessionHistoryEdit::new(
                DocumentPatch::PendingStyle,
                SessionHistoryState::new(
                    before_selection,
                    before_visual_affinity,
                    before_pending_style,
                    before_state_id,
                ),
                SessionHistoryState::new(
                    self.selection.clone(),
                    self.selection_visual_affinity,
                    self.pending_inline_style.clone(),
                    after_state_id,
                ),
            ));
            self.history_state_id = after_state_id;
            return Ok(Effect::default());
        }

        let ordering = compare_positions(&self.paragraph_index, &anchor, &focus)?;
        let (start, end) = if ordering.is_le() {
            (anchor, focus)
        } else {
            (focus, anchor)
        };
        let mut paragraphs: Vec<_> = self
            .paragraph_index
            .iter()
            .filter(|(_, location)| location.story == start.story)
            .collect();
        paragraphs.sort_by(|(_, left), (_, right)| {
            left.block_path
                .cmp(&right.block_path)
                .then_with(|| left.is_title.cmp(&right.is_title).reverse())
        });
        let start_index = paragraphs
            .iter()
            .position(|(id, _)| *id == &start.paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        let end_index = paragraphs
            .iter()
            .position(|(id, _)| *id == &end.paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        if start_index > end_index {
            return Err(EditError::InvalidPosition);
        }

        let mut ranges = Vec::with_capacity(end_index - start_index + 1);
        let mut any_disabled = false;
        let mut any_content = false;
        for (index, entry) in paragraphs
            .iter()
            .enumerate()
            .take(end_index + 1)
            .skip(start_index)
        {
            let (paragraph_id, location) = *entry;
            let paragraph = paragraph_at_location(&self.document.body, location)
                .ok_or(EditError::InvalidPosition)?;
            let map = TextMap::new(paragraph)?;
            let range_start = if index == start_index {
                start.offset
            } else {
                0
            };
            let range_end = if index == end_index {
                end.offset
            } else {
                map.text.len()
            };
            if range_start > range_end || range_end > map.text.len() {
                return Err(EditError::InvalidPosition);
            }
            if range_start == range_end {
                continue;
            }
            map.validate_pos(&Pos {
                offset: range_start,
                ..start.clone()
            })?;
            map.validate_pos(&Pos {
                paragraph_id: paragraph_id.clone(),
                offset: range_end,
                ..start.clone()
            })?;
            let spans = format_spans(&paragraph.inlines)?;
            for span in spans
                .iter()
                .filter(|span| span.start < range_end && span.end > range_start)
            {
                any_content = true;
                any_disabled |= !format_value(span, kind);
            }
            ranges.push((
                paragraph_id.clone(),
                location.clone(),
                range_start,
                range_end,
            ));
        }
        if !any_content {
            return Ok(Effect::default());
        }
        let enabled = any_disabled;

        // 先在段落副本中完成所有切分；任何一处失败都不会留下部分格式修改。
        let mut edits = Vec::with_capacity(ranges.len());
        for (paragraph_id, location, range_start, range_end) in ranges {
            let mut paragraph = paragraph_at_location(&self.document.body, &location)
                .ok_or(EditError::InvalidPosition)?
                .clone();
            wrap_format_range(
                &mut paragraph.inlines,
                range_start,
                range_end,
                kind,
                enabled,
            )?;
            edits.push((
                Pos {
                    paragraph_id,
                    ..start.clone()
                },
                paragraph,
            ));
        }
        for (position, paragraph) in edits {
            self.replace_paragraph(&position, paragraph)?;
        }
        self.text_map_cache.clear();
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();
        Ok(Effect {
            document_changed: true,
            changed_paragraph_id: (start.paragraph_id == end.paragraph_id)
                .then_some(start.paragraph_id),
            ..Effect::default()
        })
    }

    pub(super) fn apply_pending_style_to_inserted_text(
        &mut self,
        paragraph_id: &NodeId,
        start: usize,
        end: usize,
    ) -> Result<(), EditError> {
        let formats: Vec<_> = self.pending_inline_style.active_formats().collect();
        if formats.is_empty() || start >= end {
            return Ok(());
        }
        let position = Pos {
            story: self
                .paragraph_index
                .get(paragraph_id)
                .ok_or(EditError::InvalidPosition)?
                .story
                .clone(),
            paragraph_id: paragraph_id.clone(),
            offset: start,
            affinity: Affinity::Downstream,
        };
        let mut paragraph = self.paragraph_for(&position)?.clone();
        let map = TextMap::new(&paragraph)?;
        let snapped_start = map.snap_to_grapheme_boundary(start, Affinity::Upstream);
        let snapped_end = map.snap_to_grapheme_boundary(end, Affinity::Downstream);
        apply_formats_to_range(&mut paragraph, snapped_start, snapped_end, &formats)?;
        self.replace_paragraph(&position, paragraph)?;
        self.text_map_cache.clear();
        Ok(())
    }
}

pub(super) mod inline_tree;

#[cfg(test)]
mod tests;
