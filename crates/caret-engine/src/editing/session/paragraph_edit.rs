//! 执行 Enter 段落拆分及跨段选区结构变更。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::paragraph_ops::{
    build_paragraph_index, insert_paragraph_after, replace_paragraph_with_many,
    split_environment_title, update_index_after_split,
};
use super::super::text_map::{TextMap, split_inline_nodes};
use super::super::{Affinity, EditError, Effect, Pos, PositionMap, Selection, StoryId};
use super::Session;
use caret_doc::NodeId;

impl Session {
    pub(super) fn split_paragraph(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let (anchor, focus) = self.edit_range()?;
        if anchor.paragraph_id != focus.paragraph_id {
            let before_document = self.document.clone();
            let before_selection = self.selection.clone();
            let before_index = self.paragraph_index.clone();
            let before_cache = self.text_map_cache.clone();
            let result = (|| {
                let super::range_edit::MergedCrossParagraphRange { start, end, result } =
                    self.merge_cross_paragraph_range(&anchor, &focus)?;
                let target = Pos {
                    paragraph_id: result.paragraph_id.clone(),
                    offset: result.start_offset,
                    ..start.clone()
                };
                let mut source = self.paragraph_for(&target)?.clone();
                let result_boundaries = TextMap::new(&source)?.grapheme_boundaries;
                let (left, right) =
                    split_inline_nodes(std::mem::take(&mut source.inlines), start.offset)?;
                let mut left_paragraph = source.clone();
                left_paragraph.inlines = left;
                let mut right_paragraph = source;
                right_paragraph.id = NodeId::generate();
                right_paragraph.inlines = right;
                let right_id = right_paragraph.id.clone();
                let location = self
                    .paragraph_index
                    .get(&result.paragraph_id)
                    .cloned()
                    .ok_or_else(|| EditError::UnknownParagraph(result.paragraph_id.to_string()))?;
                let mut replacements = vec![left_paragraph, right_paragraph];
                if !replace_paragraph_with_many(
                    &mut self.document.body,
                    &location,
                    &mut replacements,
                ) {
                    return Err(EditError::UnknownParagraph(result.paragraph_id.to_string()));
                }
                self.paragraph_index = build_paragraph_index(&self.document.body);
                self.text_map_cache.clear();
                let position = Pos {
                    story: StoryId::Body,
                    paragraph_id: right_id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                };
                self.selection = Selection {
                    anchor: position.clone(),
                    focus: position,
                };
                Ok(Effect {
                    document_changed: true,
                    selection_changed: true,
                    position_map: Some(PositionMap::Composite(vec![
                        PositionMap::ParagraphRangeMerge {
                            paragraph_id: result.paragraph_id,
                            end_paragraph_id: end.paragraph_id,
                            removed_ids: result.removed_ids,
                            start_offset: result.start_offset,
                            end_offset: result.end_offset,
                            snap_graphemes: false,
                            result_boundaries,
                        },
                        PositionMap::ParagraphSplit {
                            source_id: target.paragraph_id,
                            right_id,
                            split_offset: start.offset,
                            source_story: StoryId::Body,
                            right_story: StoryId::Body,
                        },
                    ])),
                    ..Effect::default()
                })
            })();
            if result.is_err() {
                self.document = before_document;
                self.selection = before_selection;
                self.paragraph_index = before_index;
                self.text_map_cache = before_cache;
            }
            return result;
        }
        let mut source = self.paragraph_for(&anchor)?.clone();
        let map = TextMap::new(&source)?;
        map.validate_pos(&anchor)?;
        map.validate_pos(&focus)?;
        let split_offset = anchor.offset.min(focus.offset);
        if anchor.offset != focus.offset {
            map.delete_range(
                &mut source,
                anchor.offset.min(focus.offset),
                anchor.offset.max(focus.offset),
            )?;
        }
        let result_map = TextMap::new(&source)?;
        let result_boundaries = result_map.grapheme_boundaries;
        if !source.extra.is_empty() {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let (left, right) = split_inline_nodes(std::mem::take(&mut source.inlines), split_offset)?;
        let mut left_paragraph = source.clone();
        left_paragraph.inlines = left;
        let mut right_paragraph = source;
        right_paragraph.id = NodeId::generate();
        right_paragraph.inlines = right;
        right_paragraph.extra.clear();
        let right_id = right_paragraph.id.clone();
        let source_id = anchor.paragraph_id.clone();
        let source_story = anchor.story.clone();
        let replaced_start = anchor.offset.min(focus.offset);
        let replaced_end = anchor.offset.max(focus.offset);
        let mut left_paragraph = Some(left_paragraph);
        let mut right_paragraph = Some(right_paragraph);
        let location = self
            .paragraph_index
            .get(&anchor.paragraph_id)
            .cloned()
            .ok_or_else(|| EditError::UnknownParagraph(anchor.paragraph_id.to_string()))?;

        match &anchor.story {
            StoryId::Body => {
                if !insert_paragraph_after(
                    &mut self.document.body,
                    &location,
                    &mut left_paragraph,
                    &mut right_paragraph,
                ) {
                    return Err(EditError::UnknownParagraph(anchor.paragraph_id.to_string()));
                }
            }
            StoryId::EnvironmentTitle(_) => {
                if !split_environment_title(
                    &mut self.document.body,
                    &location,
                    &mut left_paragraph,
                    &mut right_paragraph,
                ) {
                    return Err(EditError::UnknownParagraph(anchor.paragraph_id.to_string()));
                }
            }
        }
        self.text_map_cache.clear();
        update_index_after_split(
            &self.document.body,
            &mut self.paragraph_index,
            &right_id,
            &location,
        );
        let pos = Pos {
            story: StoryId::Body,
            paragraph_id: right_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        self.selection = Selection {
            anchor: pos.clone(),
            focus: pos,
        };
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            composition_changed: false,
            math_input_changed: false,
            math_input_error: false,
            changed_paragraph_id: None,
            position_map: Some(if replaced_start == replaced_end {
                PositionMap::ParagraphSplit {
                    source_id,
                    right_id,
                    split_offset,
                    source_story,
                    right_story: StoryId::Body,
                }
            } else {
                PositionMap::Composite(vec![
                    PositionMap::TextEdit {
                        paragraph_id: source_id.clone(),
                        start: replaced_start,
                        end: replaced_end,
                        inserted_len: 0,
                        snap_graphemes: false,
                        result_boundaries,
                    },
                    PositionMap::ParagraphSplit {
                        source_id,
                        right_id,
                        split_offset,
                        source_story,
                        right_story: StoryId::Body,
                    },
                ])
            }),
        })
    }
}
