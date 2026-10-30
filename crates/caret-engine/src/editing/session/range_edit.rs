//! 执行跨段选区合并、删除与文本替换事务。

use super::super::paragraph_ops::{
    ParagraphLocation, ParagraphRangeMergeResult, build_paragraph_index, merge_paragraph_range,
};
use super::super::text_map::TextMap;
use super::super::{Affinity, EditError, Effect, Pos, PositionMap, Selection, StoryId};
use super::Session;

pub(super) struct MergedCrossParagraphRange {
    pub start: Pos,
    pub end: Pos,
    pub result: ParagraphRangeMergeResult,
}

impl Session {
    /// 删除同一正文容器中跨多个段落的选区并原子合并段落。
    pub(super) fn delete_cross_paragraph_selection(
        &mut self,
        anchor: Pos,
        focus: Pos,
    ) -> Result<Effect, EditError> {
        let MergedCrossParagraphRange { start, end, result } =
            self.merge_cross_paragraph_range(&anchor, &focus)?;
        let result_map = self.text_map_for(&Pos {
            paragraph_id: result.paragraph_id.clone(),
            offset: result.start_offset,
            ..start.clone()
        })?;
        let caret_offset =
            result_map.snap_to_grapheme_boundary(result.start_offset, Affinity::Upstream);
        let position = Pos {
            story: StoryId::Body,
            paragraph_id: result.paragraph_id.clone(),
            offset: caret_offset,
            affinity: start.affinity,
        };
        self.selection = Selection {
            anchor: position.clone(),
            focus: position,
        };
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            position_map: Some(PositionMap::ParagraphRangeMerge {
                paragraph_id: result.paragraph_id,
                end_paragraph_id: end.paragraph_id,
                removed_ids: result.removed_ids,
                start_offset: result.start_offset,
                end_offset: result.end_offset,
                snap_graphemes: true,
                result_boundaries: result.result_boundaries,
            }),
            ..Effect::default()
        })
    }

    /// 用文本替换跨段选区，并保留合并时原始 UTF-8 切口。
    pub(super) fn insert_text_across_paragraphs(
        &mut self,
        anchor: Pos,
        focus: Pos,
        text: &str,
    ) -> Result<Effect, EditError> {
        if text.is_empty() {
            return self.delete_cross_paragraph_selection(anchor, focus);
        }
        let MergedCrossParagraphRange { start, end, result } =
            self.merge_cross_paragraph_range(&anchor, &focus)?;
        let target = Pos {
            paragraph_id: result.paragraph_id.clone(),
            offset: result.start_offset,
            ..start.clone()
        };
        let mut paragraph = self.paragraph_for(&target)?.clone();
        TextMap::new(&paragraph)?.insert_text(
            &mut paragraph,
            result.start_offset,
            start.affinity,
            text,
        )?;
        self.replace_paragraph(&target, paragraph)?;
        self.apply_pending_style_to_inserted_text(
            &result.paragraph_id,
            result.start_offset,
            result.start_offset + text.len(),
        )?;
        let affinity = if self.pending_inline_style.active_formats().next().is_some() {
            Affinity::Upstream
        } else {
            Affinity::Downstream
        };
        self.text_map_cache.clear();
        let result_map = self.text_map_for(&target)?;
        let caret_offset = result_map
            .snap_to_grapheme_boundary(result.start_offset + text.len(), Affinity::Downstream);
        let result_boundaries = result_map.grapheme_boundaries.clone();
        let position = Pos {
            offset: caret_offset,
            affinity,
            ..target.clone()
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
                    inserted_len: text.len(),
                    snap_graphemes: true,
                    result_boundaries,
                },
            ])),
            ..Effect::default()
        })
    }

    /// 按稳定段落位置排序并验证选区只能覆盖同一正文容器。
    pub(super) fn ordered_cross_paragraph_range(
        &self,
        anchor: &Pos,
        focus: &Pos,
    ) -> Result<(Pos, ParagraphLocation, Pos, ParagraphLocation), EditError> {
        if anchor.story != StoryId::Body || focus.story != StoryId::Body {
            return Err(EditError::CrossParagraphSelection);
        }
        let anchor_location = self
            .paragraph_index
            .get(&anchor.paragraph_id)
            .cloned()
            .ok_or_else(|| EditError::UnknownParagraph(anchor.paragraph_id.to_string()))?;
        let focus_location = self
            .paragraph_index
            .get(&focus.paragraph_id)
            .cloned()
            .ok_or_else(|| EditError::UnknownParagraph(focus.paragraph_id.to_string()))?;
        TextMap::new(self.paragraph_for(anchor)?)?.validate_pos(anchor)?;
        TextMap::new(self.paragraph_for(focus)?)?.validate_pos(focus)?;
        if anchor_location.block_path == focus_location.block_path {
            return Err(EditError::CrossParagraphSelection);
        }
        Ok(if anchor_location.block_path < focus_location.block_path {
            (
                anchor.clone(),
                anchor_location,
                focus.clone(),
                focus_location,
            )
        } else {
            (
                focus.clone(),
                focus_location,
                anchor.clone(),
                anchor_location,
            )
        })
    }

    /// 合并并校验跨段编辑范围，同时更新段落索引和文本映射缓存。
    pub(super) fn merge_cross_paragraph_range(
        &mut self,
        anchor: &Pos,
        focus: &Pos,
    ) -> Result<MergedCrossParagraphRange, EditError> {
        let (start, start_location, end, end_location) =
            self.ordered_cross_paragraph_range(anchor, focus)?;
        let result = merge_paragraph_range(
            &mut self.document.body,
            &start_location,
            start.offset,
            &end_location,
            end.offset,
        )?;
        self.paragraph_index = build_paragraph_index(&self.document.body);
        self.text_map_cache.clear();
        Ok(MergedCrossParagraphRange { start, end, result })
    }
}
