//! 执行字素删除与相邻段落合并。

use super::super::paragraph_ops::{
    ParagraphMergeResult, merge_adjacent_paragraph, update_index_after_merge,
};
use super::super::text_map::TextMap;
use super::super::{Affinity, Direction, EditError, Effect, Pos, PositionMap, Selection, StoryId};
use super::Session;

impl Session {
    pub(super) fn delete(&mut self, direction: Direction) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let (anchor, focus) = self.edit_range()?;
        if anchor.paragraph_id != focus.paragraph_id {
            return self.delete_cross_paragraph_selection(anchor, focus);
        }
        let paragraph = self.paragraph_for(&anchor)?;
        let map = TextMap::new(paragraph)?;
        map.validate_pos(&anchor)?;
        map.validate_pos(&focus)?;
        let (from, to) = if anchor.offset != focus.offset {
            (
                anchor.offset.min(focus.offset),
                anchor.offset.max(focus.offset),
            )
        } else {
            match direction {
                Direction::Backward => (map.previous_boundary(anchor.offset), anchor.offset),
                Direction::Forward => (anchor.offset, map.next_boundary(anchor.offset)),
            }
        };
        if from == to {
            let location = self
                .paragraph_index
                .get(&anchor.paragraph_id)
                .cloned()
                .ok_or_else(|| EditError::UnknownParagraph(anchor.paragraph_id.to_string()))?;
            if anchor.offset == 0
                && direction == Direction::Backward
                && matches!(&anchor.story, StoryId::Body)
                && let Some(ParagraphMergeResult {
                    left_id,
                    right_id,
                    left_len,
                    result_boundaries,
                }) = merge_adjacent_paragraph(
                    &mut self.document.body,
                    &location,
                    Direction::Backward,
                )?
            {
                update_index_after_merge(
                    &self.document.body,
                    &mut self.paragraph_index,
                    &left_id,
                    &right_id,
                );
                let position_map = PositionMap::ParagraphMerge {
                    left_id,
                    right_id,
                    left_len,
                    result_boundaries,
                };
                let pos = position_map.map(&anchor);
                self.selection = Selection {
                    anchor: pos.clone(),
                    focus: pos,
                };
                self.text_map_cache.clear();
                return Ok(Effect {
                    document_changed: true,
                    selection_changed: true,
                    composition_changed: false,
                    changed_paragraph_id: None,
                    position_map: Some(position_map),
                });
            }
            if anchor.offset == map.text.len()
                && direction == Direction::Forward
                && matches!(&anchor.story, StoryId::Body)
                && let Some(ParagraphMergeResult {
                    left_id,
                    right_id,
                    left_len,
                    result_boundaries,
                }) = merge_adjacent_paragraph(
                    &mut self.document.body,
                    &location,
                    Direction::Forward,
                )?
            {
                update_index_after_merge(
                    &self.document.body,
                    &mut self.paragraph_index,
                    &left_id,
                    &right_id,
                );
                let position_map = PositionMap::ParagraphMerge {
                    left_id,
                    right_id,
                    left_len,
                    result_boundaries,
                };
                let pos = position_map.map(&anchor);
                self.selection = Selection {
                    anchor: pos.clone(),
                    focus: pos,
                };
                self.text_map_cache.clear();
                return Ok(Effect {
                    document_changed: true,
                    selection_changed: true,
                    composition_changed: false,
                    changed_paragraph_id: None,
                    position_map: Some(position_map),
                });
            }
            return Ok(Effect::default());
        }
        if anchor.offset == focus.offset {
            map.delete_range(self.paragraph_for_mut(&anchor)?, from, to)?;
        } else {
            let mut updated = paragraph.clone();
            map.delete_range(&mut updated, from, to)?;
            self.replace_paragraph(&anchor, updated)?;
        }
        self.text_map_cache.clear();
        let result_map = self.text_map_for(&Pos {
            offset: from,
            ..anchor.clone()
        })?;
        let caret_offset = result_map.snap_to_grapheme_boundary(from, Affinity::Upstream);
        let result_boundaries = result_map.grapheme_boundaries.clone();
        let collapse_affinity = if anchor.offset <= focus.offset {
            anchor.affinity
        } else {
            focus.affinity
        };
        let paragraph_id = anchor.paragraph_id.clone();
        let pos = Pos {
            offset: caret_offset,
            affinity: collapse_affinity,
            ..anchor
        };
        self.selection = Selection {
            anchor: pos.clone(),
            focus: pos,
        };
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            composition_changed: false,
            changed_paragraph_id: None,
            position_map: Some(PositionMap::TextEdit {
                paragraph_id,
                start: from,
                end: to,
                inserted_len: 0,
                snap_graphemes: true,
                result_boundaries,
            }),
        })
    }
}
