//! 执行行内格式环境命令并管理当前插入点的待输入格式。

use super::super::paragraph_ops::{compare_positions, paragraph_at_location};
use super::super::session::history::{DocumentPatch, SessionHistoryEdit, SessionHistoryState};
use super::super::text_map::TextMap;
use super::super::{Affinity, EditError, Effect, Pos};
use super::Session;
use caret_doc::{DocumentFontTarget, InlineEnvironmentKind, NodeId, Paragraph};
use inline_tree::{
    apply_font_family_to_range, apply_formats_to_range, effective_format_at, format_spans,
    format_value, wrap_format_range,
};

fn is_collapsed_text_range(anchor: &Pos, focus: &Pos) -> bool {
    anchor.story == focus.story
        && anchor.paragraph_id == focus.paragraph_id
        && anchor.offset == focus.offset
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PendingInlineStyle {
    bold: Option<bool>,
    italic: Option<bool>,
    font_family: Option<String>,
}

impl PendingInlineStyle {
    pub(super) fn clear(&mut self) {
        self.bold = None;
        self.italic = None;
        self.font_family = None;
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

    pub(super) fn active_font_family(&self) -> Option<&str> {
        self.font_family.as_deref()
    }

    pub(super) fn is_active(&self) -> bool {
        self.bold.is_some() || self.italic.is_some() || self.font_family.is_some()
    }
}

impl Session {
    pub(super) fn set_document_font_family(
        &mut self,
        target: &DocumentFontTarget,
        family: Option<String>,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let target = target
            .normalized()
            .map_err(|_| EditError::InvalidFontFamily)?;
        if family
            .as_deref()
            .is_some_and(|family| family.trim().is_empty())
        {
            return Err(EditError::InvalidFontFamily);
        }
        if self.document.global_settings.font_family(&target) == family.as_deref() {
            return Ok(Effect::default());
        }
        self.document
            .global_settings
            .set_font_family(&target, family)
            .map_err(|_| EditError::InvalidFontFamily)?;
        Ok(Effect {
            document_changed: true,
            ..Effect::default()
        })
    }

    pub(super) fn inline_depth_for_paragraph(
        &self,
        paragraph_id: &NodeId,
    ) -> Result<usize, EditError> {
        let location = self
            .paragraph_index
            .get(paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        Ok(location.block_path.len() + usize::from(location.is_title))
    }

    pub(super) fn set_default_font_family(
        &mut self,
        family: Option<String>,
    ) -> Result<Effect, EditError> {
        self.set_document_font_family(&DocumentFontTarget::Script("Latn".to_owned()), family)
    }

    pub(super) fn set_inline_font_family(&mut self, family: &str) -> Result<Effect, EditError> {
        if family.trim().is_empty() {
            return Err(EditError::InvalidFontFamily);
        }
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let (anchor, focus) = self.edit_range()?;
        if is_collapsed_text_range(&anchor, &focus) {
            let before_pending_style = self.pending_inline_style.clone();
            if before_pending_style.font_family.as_deref() == Some(family) {
                return Ok(Effect::default());
            }
            let before_selection = self.selection.clone();
            let before_visual_affinity = self.selection_visual_affinity;
            let before_state_id = self.history_state_id;
            self.pending_inline_style.font_family = Some(family.to_owned());
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
        if start.paragraph_id == end.paragraph_id {
            let location = self
                .paragraph_index
                .get(&start.paragraph_id)
                .ok_or(EditError::InvalidPosition)?;
            let original = paragraph_at_location(&self.document.body, location)
                .ok_or(EditError::InvalidPosition)?;
            let map = TextMap::new(original)?;
            if start.offset >= end.offset || end.offset > map.text.len() {
                return Err(EditError::InvalidPosition);
            }
            map.validate_pos(&start)?;
            map.validate_pos(&end)?;
            let mut paragraph = original.clone();
            apply_font_family_to_range(
                &mut paragraph,
                start.offset,
                end.offset,
                family,
                location.block_path.len() + usize::from(location.is_title),
            )?;
            if paragraph == *original {
                return Ok(Effect::default());
            }
            self.replace_paragraph(&start, paragraph)?;
            self.text_map_cache.clear();
            self.pending_inline_style.clear();
            self.advance_history_merge_epoch();
            return Ok(Effect {
                document_changed: true,
                changed_paragraph_id: Some(start.paragraph_id),
                ..Effect::default()
            });
        }

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

        let mut edits = Vec::with_capacity(end_index - start_index + 1);
        for (index, entry) in paragraphs
            .iter()
            .enumerate()
            .take(end_index + 1)
            .skip(start_index)
        {
            let (paragraph_id, location) = *entry;
            let original = paragraph_at_location(&self.document.body, location)
                .ok_or(EditError::InvalidPosition)?;
            let map = TextMap::new(original)?;
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
            if range_start >= range_end || range_end > map.text.len() {
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
            let mut paragraph = original.clone();
            apply_font_family_to_range(
                &mut paragraph,
                range_start,
                range_end,
                family,
                location.block_path.len() + usize::from(location.is_title),
            )?;
            if paragraph == *original {
                continue;
            }
            edits.push((
                Pos {
                    paragraph_id: paragraph_id.clone(),
                    ..start.clone()
                },
                paragraph,
            ));
        }
        if edits.is_empty() {
            return Ok(Effect::default());
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
        if is_collapsed_text_range(&anchor, &focus) {
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
        if !self.pending_inline_style.is_active() || start >= end {
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
        self.apply_pending_style_to_paragraph(
            &mut paragraph,
            start,
            end,
            self.inline_depth_for_paragraph(paragraph_id)?,
        )?;
        self.replace_paragraph(&position, paragraph)?;
        self.text_map_cache.clear();
        Ok(())
    }

    pub(super) fn apply_pending_style_to_paragraph(
        &self,
        paragraph: &mut Paragraph,
        start: usize,
        end: usize,
        inline_depth: usize,
    ) -> Result<(), EditError> {
        let formats: Vec<_> = self.pending_inline_style.active_formats().collect();
        let font_family = self
            .pending_inline_style
            .active_font_family()
            .map(str::to_owned);
        if (formats.is_empty() && font_family.is_none()) || start >= end {
            return Ok(());
        }
        let map = TextMap::new(paragraph)?;
        let snapped_start = map.snap_to_grapheme_boundary(start, Affinity::Upstream);
        let snapped_end = map.snap_to_grapheme_boundary(end, Affinity::Downstream);
        apply_formats_to_range(paragraph, snapped_start, snapped_end, &formats)?;
        if let Some(family) = font_family {
            apply_font_family_to_range(
                paragraph,
                snapped_start,
                snapped_end,
                &family,
                inline_depth,
            )?;
        }
        Ok(())
    }
}

pub(super) mod inline_tree;

#[cfg(test)]
mod tests;
