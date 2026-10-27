//! 执行纯文本粘贴及多段落替换事务。

use super::super::paragraph_ops::{build_paragraph_index, replace_paragraph_with_many};
use super::super::text_map::{TextMap, split_inline_nodes};
use super::super::{Affinity, EditError, Effect, Pos, PositionMap, Selection, StoryId};
use super::Session;

impl Session {
    pub(super) fn paste_text(&mut self, text: &str) -> Result<Effect, EditError> {
        self.paste_text_at(text, None)
    }

    fn paste_text_at(
        &mut self,
        text: &str,
        cut: Option<(usize, Affinity)>,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if !normalized.contains('\n') {
            return self.insert_text(&normalized);
        }

        let (anchor, focus) = self.edit_range()?;
        if anchor.paragraph_id != focus.paragraph_id {
            let (start_position, _, _, _) = self.ordered_cross_paragraph_range(&anchor, &focus)?;
            let mut merge = self.delete_cross_paragraph_selection(anchor, focus)?;
            let paste = self.paste_text_at(
                &normalized,
                Some((start_position.offset, start_position.affinity)),
            )?;
            merge.document_changed |= paste.document_changed;
            merge.selection_changed |= paste.selection_changed;
            merge.position_map = match (merge.position_map.take(), paste.position_map) {
                (Some(first), Some(second)) => Some(PositionMap::Composite(vec![first, second])),
                (first, second) => first.or(second),
            };
            return Ok(merge);
        }
        let (start_position, end_position) = if anchor.offset <= focus.offset {
            (&anchor, &focus)
        } else {
            (&focus, &anchor)
        };
        let start = cut.map_or(start_position.offset, |(offset, _)| offset);
        let end = cut.map_or(end_position.offset, |(offset, _)| offset);
        let start_affinity = cut.map_or(start_position.affinity, |(_, affinity)| affinity);
        let location = self
            .paragraph_index
            .get(&anchor.paragraph_id)
            .cloned()
            .ok_or_else(|| EditError::UnknownParagraph(anchor.paragraph_id.to_string()))?;
        if location.is_title || !matches!(anchor.story, StoryId::Body) {
            return Err(EditError::UnsupportedNewline);
        }

        let original = self.paragraph_for(&anchor)?.clone();
        if !original.extra.is_empty() {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let mut current = original;
        let original_map = TextMap::new(&current)?;
        if cut.is_none() {
            original_map.validate_pos(start_position)?;
            original_map.validate_pos(end_position)?;
        } else if start > original_map.text.len() || !original_map.text.is_char_boundary(start) {
            return Err(EditError::InvalidPosition);
        }
        if start < end {
            original_map.delete_range(&mut current, start, end)?;
        }
        let first_line = normalized.split('\n').next().unwrap_or_default();
        let map = TextMap::new(&current)?;
        map.insert_text(&mut current, start, start_affinity, first_line)?;
        let result_map = TextMap::new(&current)?;
        let mut position_maps = vec![PositionMap::TextEdit {
            paragraph_id: anchor.paragraph_id.clone(),
            start,
            end,
            inserted_len: first_line.len(),
            snap_graphemes: true,
            result_boundaries: result_map.grapheme_boundaries.clone(),
        }];

        let mut paragraphs = Vec::new();
        let mut current_offset = start + first_line.len();
        for line in normalized.split('\n').skip(1) {
            if !current.extra.is_empty() {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            let (left, right) =
                split_inline_nodes(std::mem::take(&mut current.inlines), current_offset)?;
            current.inlines = left;
            paragraphs.push(current);

            let right_id = caret_doc::NodeId::generate();
            let mut next = paragraphs
                .last()
                .cloned()
                .ok_or(EditError::InvalidPosition)?;
            next.id = right_id.clone();
            next.inlines = right;
            next.extra.clear();
            let source_id = paragraphs
                .last()
                .ok_or(EditError::InvalidPosition)?
                .id
                .clone();
            position_maps.push(PositionMap::ParagraphSplit {
                source_id,
                right_id,
                split_offset: current_offset,
                source_story: anchor.story.clone(),
                right_story: StoryId::Body,
            });
            current = next;

            let next_map = TextMap::new(&current)?;
            next_map.insert_text(&mut current, 0, Affinity::Downstream, line)?;
            let inserted_map = TextMap::new(&current)?;
            if !line.is_empty() {
                position_maps.push(PositionMap::TextEdit {
                    paragraph_id: current.id.clone(),
                    start: 0,
                    end: 0,
                    inserted_len: line.len(),
                    snap_graphemes: true,
                    result_boundaries: inserted_map.grapheme_boundaries.clone(),
                });
            }
            current_offset = line.len();
        }
        let caret_offset =
            TextMap::new(&current)?.snap_to_grapheme_boundary(current_offset, Affinity::Downstream);
        let last_id = current.id.clone();
        paragraphs.push(current);
        if !replace_paragraph_with_many(&mut self.document.body, &location, &mut paragraphs) {
            return Err(EditError::UnknownParagraph(anchor.paragraph_id.to_string()));
        }
        self.paragraph_index = build_paragraph_index(&self.document.body);
        self.text_map_cache.clear();
        let position = Pos {
            story: StoryId::Body,
            paragraph_id: last_id,
            offset: caret_offset,
            affinity: Affinity::Downstream,
        };
        self.selection = Selection {
            anchor: position.clone(),
            focus: position,
        };
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            position_map: Some(PositionMap::Composite(position_maps)),
            ..Effect::default()
        })
    }
}
