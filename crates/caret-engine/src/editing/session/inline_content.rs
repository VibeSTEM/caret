//! 处理 Paragraph 内的文本、软换行与行内公式插入。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::text_map::{OBJECT_REPLACEMENT, TextMap};
use super::super::{Affinity, EditError, Effect, Pos, PositionMap, Selection};
use super::Session;

impl Session {
    pub(super) fn insert_text(&mut self, text: &str) -> Result<Effect, EditError> {
        if text.contains(['\n', '\r']) {
            return Err(EditError::UnsupportedNewline);
        }
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let (anchor, focus) = self.edit_range()?;
        if anchor.paragraph_id != focus.paragraph_id {
            return self.insert_text_across_paragraphs(anchor, focus, text);
        }
        if text.is_empty() && anchor.offset == focus.offset {
            return Ok(Effect::default());
        }
        let start = if anchor.offset <= focus.offset {
            &anchor
        } else {
            &focus
        };
        let insert_offset = anchor.offset.min(focus.offset);
        let mut paragraph = self.paragraph_for(&anchor)?.clone();
        if anchor.offset == focus.offset {
            let map = TextMap::new(&paragraph)?;
            map.validate_pos(&anchor)?;
            map.insert_text(&mut paragraph, insert_offset, start.affinity, text)?;
        } else {
            let map = TextMap::new(&paragraph)?;
            map.validate_pos(&anchor)?;
            map.validate_pos(&focus)?;
            map.delete_range(
                &mut paragraph,
                anchor.offset.min(focus.offset),
                anchor.offset.max(focus.offset),
            )?;
            let updated_map = TextMap::new(&paragraph)?;
            updated_map.insert_text(&mut paragraph, insert_offset, start.affinity, text)?;
        }
        if !text.is_empty() {
            self.apply_pending_style_to_paragraph(
                &mut paragraph,
                insert_offset,
                insert_offset + text.len(),
                self.inline_depth_for_paragraph(&anchor.paragraph_id)?,
            )?;
        }
        self.replace_paragraph(&anchor, paragraph)?;
        let paragraph_id = anchor.paragraph_id.clone();
        let replaced_start = anchor.offset.min(focus.offset);
        let replaced_end = anchor.offset.max(focus.offset);
        self.text_map_cache.clear();
        let result_map = self.text_map_for(&Pos {
            offset: insert_offset + text.len(),
            ..start.clone()
        })?;
        let position_offset =
            result_map.snap_to_grapheme_boundary(insert_offset + text.len(), Affinity::Downstream);
        let result_boundaries = result_map.grapheme_boundaries.clone();
        let affinity = if self.pending_inline_style.is_active() {
            Affinity::Upstream
        } else {
            start.affinity
        };
        let new_pos = Pos {
            offset: position_offset,
            affinity,
            ..anchor
        };
        self.selection = Selection {
            anchor: new_pos.clone(),
            focus: new_pos,
        };
        Ok(Effect {
            document_changed: !text.is_empty() || anchor.offset != focus.offset,
            selection_changed: true,
            composition_changed: false,
            changed_paragraph_id: None,
            position_map: Some(PositionMap::TextEdit {
                paragraph_id,
                start: replaced_start,
                end: replaced_end,
                inserted_len: text.len(),
                snap_graphemes: true,
                result_boundaries,
            }),
        })
    }

    pub(super) fn insert_soft_break(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let (anchor, focus) = self.edit_range()?;
        if anchor.paragraph_id != focus.paragraph_id {
            let super::range_edit::MergedCrossParagraphRange { start, end, result } =
                self.merge_cross_paragraph_range(&anchor, &focus)?;
            let target = Pos {
                paragraph_id: result.paragraph_id.clone(),
                offset: result.start_offset,
                ..start.clone()
            };
            let mut paragraph = self.paragraph_for(&target)?.clone();
            TextMap::new(&paragraph)?.insert_soft_break(
                &mut paragraph,
                result.start_offset,
                start.affinity,
            )?;
            self.replace_paragraph(&target, paragraph)?;
            self.text_map_cache.clear();
            let result_map = self.text_map_for(&target)?;
            let caret_offset =
                result_map.snap_to_grapheme_boundary(result.start_offset + 1, Affinity::Downstream);
            let result_boundaries = result_map.grapheme_boundaries.clone();
            let position = Pos {
                offset: caret_offset,
                affinity: Affinity::Downstream,
                ..target.clone()
            };
            self.selection = Selection {
                anchor: position.clone(),
                focus: position,
            };
            return Ok(Effect {
                document_changed: true,
                selection_changed: true,
                position_map: Some(PositionMap::Composite(vec![
                    PositionMap::ParagraphRangeMerge {
                        paragraph_id: result.paragraph_id.clone(),
                        end_paragraph_id: end.paragraph_id,
                        removed_ids: result.removed_ids,
                        start_offset: result.start_offset,
                        end_offset: result.end_offset,
                        snap_graphemes: false,
                        result_boundaries: result.result_boundaries,
                    },
                    PositionMap::TextEdit {
                        paragraph_id: result.paragraph_id,
                        start: result.start_offset,
                        end: result.start_offset,
                        inserted_len: 1,
                        snap_graphemes: true,
                        result_boundaries,
                    },
                ])),
                ..Effect::default()
            });
        }
        let mut paragraph = self.paragraph_for(&anchor)?.clone();
        let map = TextMap::new(&paragraph)?;
        map.validate_pos(&anchor)?;
        map.validate_pos(&focus)?;
        let offset = anchor.offset.min(focus.offset);
        if anchor.offset != focus.offset {
            map.delete_range(
                &mut paragraph,
                anchor.offset.min(focus.offset),
                anchor.offset.max(focus.offset),
            )?;
        }
        let affinity = if anchor.offset <= focus.offset {
            anchor.affinity
        } else {
            focus.affinity
        };
        let paragraph_id = anchor.paragraph_id.clone();
        let replaced_start = anchor.offset.min(focus.offset);
        let replaced_end = anchor.offset.max(focus.offset);
        TextMap::new(&paragraph)?.insert_soft_break(&mut paragraph, offset, affinity)?;
        self.replace_paragraph(&anchor, paragraph)?;
        self.text_map_cache.clear();
        let result_boundaries = TextMap::new(self.paragraph_for(&anchor)?)?.grapheme_boundaries;
        let pos = Pos {
            offset: offset + 1,
            affinity: anchor.affinity,
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
                start: replaced_start,
                end: replaced_end,
                inserted_len: 1,
                snap_graphemes: true,
                result_boundaries,
            }),
        })
    }

    /// 解析 LaTeX 源文本并以 U+FFFC 原子对象插入；解析失败时正文不变。
    pub(super) fn insert_inline_math(&mut self, source: &str) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let caret_math::ParsedMath::Tree(math_tree) =
            caret_math::parse(source, caret_math::MathInputKind::Inline)
                .map_err(|error| EditError::InvalidMath(error.to_string()))?
        else {
            return Err(EditError::InvalidMath("expected inline math".to_owned()));
        };
        let (anchor, focus) = self.edit_range()?;
        if anchor.paragraph_id != focus.paragraph_id {
            return Err(EditError::CrossParagraphSelection);
        }
        let mut paragraph = self.paragraph_for(&anchor)?.clone();
        let map = TextMap::new(&paragraph)?;
        map.validate_pos(&anchor)?;
        map.validate_pos(&focus)?;
        let offset = anchor.offset.min(focus.offset);
        let end = anchor.offset.max(focus.offset);
        if offset != end {
            map.delete_range(&mut paragraph, offset, end)?;
        }
        let affinity = if anchor.offset <= focus.offset {
            anchor.affinity
        } else {
            focus.affinity
        };
        let node = caret_doc::InlineNode::Math(caret_doc::InlineMath {
            id: caret_doc::NodeId::generate(),
            math_tree,
            extra: Default::default(),
        });
        TextMap::new(&paragraph)?.insert_atomic(&mut paragraph, offset, affinity, node)?;
        let paragraph_id = anchor.paragraph_id.clone();
        self.replace_paragraph(&anchor, paragraph)?;
        self.text_map_cache.clear();
        let inserted_len = OBJECT_REPLACEMENT.len_utf8();
        let result_boundaries = TextMap::new(self.paragraph_for(&anchor)?)?.grapheme_boundaries;
        let pos = Pos {
            offset: offset + inserted_len,
            affinity: Affinity::Upstream,
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
                start: offset,
                end,
                inserted_len,
                snap_graphemes: true,
                result_boundaries,
            }),
        })
    }
}
