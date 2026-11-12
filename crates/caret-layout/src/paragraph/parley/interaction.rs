//! 将段落布局映射为视觉光标、命中测试和交互几何。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::ops::Range;

use crate::shaping::is_mandatory_break_character;

use super::{
    HitAffinity, ParagraphHit, ParleyParagraphLayout, TextIslandLayout, VisualAffinity,
    VisualCursor,
};
use parley::{Layout, PositionedLayoutItem};

type ParleySelectionRects = Vec<(parley::BoundingBox, usize)>;
type ParleySelectionResult = (ParleySelectionRects, Range<usize>);

impl ParleyParagraphLayout {
    fn underline_top_pt(baseline: f32, offset: f32, line_top: f32, adjustment: f32) -> f64 {
        f64::from(baseline) - f64::from(offset) - f64::from(line_top) + f64::from(adjustment)
    }

    /// 从已塑形的 glyph run 提取持久下划线几何，不包含输入法预编辑装饰。
    pub fn underline_geometry_for_line(
        &self,
        line_index: usize,
    ) -> Vec<(parley::BoundingBox, Option<[u8; 4]>)> {
        let mut rectangles: Vec<(parley::BoundingBox, Option<[u8; 4]>)> = Vec::new();
        let Some(line) = self.layout.get(line_index) else {
            return rectangles;
        };
        let Some(raw_top) = self.parley_line_tops_pt.get(line_index).copied() else {
            return rectangles;
        };
        let Some(adjustment) = self.baseline_adjustments_pt.get(line_index).copied() else {
            return rectangles;
        };
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let style = glyph_run.style();
            let Some(decoration) = style.underline.as_ref() else {
                continue;
            };
            let metrics = glyph_run.run().font_metrics();
            let offset = decoration.offset.unwrap_or(metrics.underline_offset);
            if !offset.is_finite() {
                continue;
            }
            let font_size = glyph_run.run().font_size();
            if !font_size.is_finite() || font_size <= 0.0 {
                continue;
            }
            let thickness = decoration
                .size
                .unwrap_or(metrics.underline_size)
                .is_finite()
                .then_some(decoration.size.unwrap_or(metrics.underline_size))
                .filter(|value| *value > 0.0)
                .unwrap_or(font_size / 18.0);
            let x0 = f64::from(glyph_run.offset());
            let x1 = x0 + f64::from(glyph_run.advance());
            // 先转为 f64 再消去全局基线，避免长文档中的大坐标吞掉字体偏移。
            let y0 = Self::underline_top_pt(glyph_run.baseline(), offset, raw_top, adjustment);
            let y1 = y0 + f64::from(thickness);
            if !x0.is_finite()
                || !x1.is_finite()
                || !y0.is_finite()
                || !y1.is_finite()
                || x1 <= x0
                || y1 <= y0
            {
                continue;
            }
            let foreground = (decoration.brush != [0, 0, 0, 0]).then_some(decoration.brush);
            let bounds = parley::BoundingBox::new(x0, y0, x1, y1);
            if let Some((previous, previous_foreground)) = rectangles.last_mut()
                && *previous_foreground == foreground
                && (previous.x1 - bounds.x0).abs() < 0.01
                && (previous.y0 - bounds.y0).abs() < 0.01
                && (previous.y1 - bounds.y1).abs() < 0.01
            {
                previous.x1 = bounds.x1;
            } else {
                rectangles.push((bounds, foreground));
            }
        }
        rectangles
    }

    /// 字节偏移必须恰好落在扩展字素簇边界；非法位置不静默吸附。
    pub fn cursor_at(&self, byte_index: usize, affinity: VisualAffinity) -> Option<VisualCursor> {
        cursor_at_grapheme_boundary(&self.grapheme_boundaries, byte_index, affinity)
    }

    /// 将视觉光标侧映射为数学树端点的逻辑邻接侧。
    pub fn logical_affinity_at(
        &self,
        byte_index: usize,
        affinity: VisualAffinity,
    ) -> Option<HitAffinity> {
        logical_affinity_for_cursor(
            &self.layout,
            &self.grapheme_boundaries,
            byte_index,
            affinity,
        )
    }

    /// 按视觉顺序移动，并跳过 Parley 在 Run 内部暴露的字素停靠点。
    pub fn move_visual(&self, cursor: VisualCursor, forward: bool) -> Option<VisualCursor> {
        move_visual_cursor(&self.layout, &self.grapheme_boundaries, cursor, forward)
    }

    /// 在相邻视觉行按期望 x 命中，返回更新后的视觉光标和可继续复用的 x。
    pub fn move_vertical(
        &self,
        cursor: VisualCursor,
        line_delta: isize,
        preferred_x: Option<f32>,
    ) -> Option<(VisualCursor, f32)> {
        if line_delta == 0 {
            return None;
        }
        let (bounds, line_index) = self.cursor_geometry_with_line(cursor, 0.0)?;
        let x = preferred_x.unwrap_or(bounds.x0 as f32);
        if !x.is_finite() {
            return None;
        }
        let target_line = line_index.checked_add_signed(line_delta)?;
        let hit = self.cursor_from_point_on_line(x, target_line)?;
        Some((hit.cursor, x))
    }

    /// 将光标移到当前视觉行首尾，并将 Parley 的结果校验到扩展字素边界。
    pub fn move_to_visual_line_edge(
        &self,
        cursor: VisualCursor,
        end: bool,
    ) -> Option<VisualCursor> {
        move_to_visual_line_edge(&self.layout, &self.grapheme_boundaries, cursor, end)
    }

    /// 点击命中时将内部字素停靠点吸附到视觉距离最近的完整字素边缘。
    pub fn cursor_from_point(&self, x: f32, y: f32) -> Option<ParagraphHit> {
        if self.layout.text_len() == 0 {
            return self
                .cursor_at(0, VisualAffinity::Downstream)
                .map(|cursor| ParagraphHit {
                    cursor,
                    logical_affinity: HitAffinity::Downstream,
                });
        }
        let (line_index, _) = self.raw_line_y(y)?;
        self.cursor_from_point_on_line(x, line_index)
    }

    /// 在已选定的 Caret 行内按 x 坐标命中，避免行高差异将命中带到相邻 Parley 行。
    pub fn cursor_from_point_on_line(&self, x: f32, line_index: usize) -> Option<ParagraphHit> {
        if !x.is_finite() {
            return None;
        }
        if self.layout.text_len() == 0 {
            return self
                .cursor_at(0, VisualAffinity::Downstream)
                .map(|cursor| ParagraphHit {
                    cursor,
                    logical_affinity: HitAffinity::Downstream,
                });
        }
        let raw_top = *self.parley_line_tops_pt.get(line_index)?;
        let raw_height = *self.parley_line_heights_pt.get(line_index)?;
        if !raw_top.is_finite() || !raw_height.is_finite() || raw_height <= 0.0 {
            return None;
        }
        let raw_y = raw_top + raw_height / 2.0;
        if let Some((source_range, logical_start)) = self.inline_object_at_point(x, line_index) {
            let byte_index = if logical_start {
                source_range.start
            } else {
                source_range.end
            };
            let affinity = if logical_start {
                VisualAffinity::Downstream
            } else {
                VisualAffinity::Upstream
            };
            let cursor = self.cursor_at(byte_index, affinity)?;
            return Some(ParagraphHit {
                cursor,
                logical_affinity: if logical_start {
                    HitAffinity::Downstream
                } else {
                    HitAffinity::Upstream
                },
            });
        }
        parley_hit_from_point(&self.layout, &self.grapheme_boundaries, x, raw_y)
    }

    /// 返回字素安全的 Parley selection 几何，并映射到 Caret 自定义行框。
    pub fn selection_geometry(
        &self,
        range: Range<usize>,
    ) -> Option<Vec<(parley::BoundingBox, usize)>> {
        let (mut rectangles, normalized_range) =
            parley_selection_geometry(&self.layout, &self.grapheme_boundaries, range)?;
        let start = normalized_range.start;
        let end = normalized_range.end;
        for (bounds, line_index) in &mut rectangles {
            if let (Some(line_top), Some(line)) = (
                self.line_box_tops_pt.get(*line_index),
                self.paragraph.lines.get(*line_index),
            ) {
                // 选区背景使用共享行框，而不是 Parley 按实际字形返回的 run 边界。
                bounds.y0 = f64::from(*line_top);
                bounds.y1 = bounds.y0 + f64::from(line.metrics.height_pt);
            }
        }
        // 行内对象的推进由 Parley 折进其后首个字素簇；选区从对象任一侧开始或结束时，
        // 折叠会让矩形覆盖对象框或把对象宽度膨胀进相邻文本。统一按对象盒规整，
        // 得到与平台字素划分无关的稳定选区几何。
        //
        // 折叠伪影只可能出现在对象盒被选中的一侧，因此保留哪一侧必须按“该侧是否真有
        // 被选中的文本”判断。这里用逻辑偏移 + 对象所在 run 的 RTL 方向换算视觉侧，
        // 而不使用选区首尾插入点的跨行全局 x 范围：自动换行的多行选区末行通常较短，
        // 全局 end_x 会小于中间行的行尾，从而错误丢弃对象右侧的合法文本段。
        //   LTR: 逻辑前=视觉左、逻辑后=视觉右
        //   RTL: 逻辑前=视觉右、逻辑后=视觉左
        let mut object_boxes = Vec::with_capacity(self.inline_object_source_ranges.len());
        for (id, object_range) in &self.inline_object_source_ranges {
            let Some((line_index, inline_box)) = self.positioned_inline_box(*id) else {
                continue;
            };
            let (Some(line_top), Some(line)) = (
                self.line_box_tops_pt.get(line_index),
                self.paragraph.lines.get(line_index),
            ) else {
                continue;
            };
            let rtl = self.inline_object_is_rtl(*id, object_range);
            let starts_before = start < object_range.start;
            let ends_after = end > object_range.end;
            object_boxes.push(InlineObjectSelectionBox {
                line_index,
                x0: f64::from(inline_box.x),
                x1: f64::from(inline_box.x + inline_box.width),
                y0: f64::from(*line_top),
                y1: f64::from(*line_top + line.metrics.height_pt),
                selected: start <= object_range.start && object_range.end <= end,
                keep_left: if rtl { ends_after } else { starts_before },
                keep_right: if rtl { starts_before } else { ends_after },
            });
        }
        Some(normalize_inline_object_selection_rectangles(
            rectangles,
            &object_boxes,
        ))
    }

    /// 返回可见文字的选区几何；段落及软换行符本身不产生高亮矩形。
    pub fn selection_text_geometry(
        &self,
        text: &str,
        range: Range<usize>,
    ) -> Option<Vec<(parley::BoundingBox, usize)>> {
        selection_geometry_excluding_breaks(
            &self.layout,
            &self.grapheme_boundaries,
            text,
            range,
            |range| self.selection_geometry(range),
        )
    }

    /// 预编辑下划线按视觉 run 的基线和字体度量定位，y 相对 Caret 行顶。
    pub fn composition_text_geometry(
        &self,
        text: &str,
        range: Range<usize>,
    ) -> Option<Vec<(parley::BoundingBox, usize)>> {
        let selection = self.selection_text_geometry(text, range)?;
        parley_composition_geometry(&self.layout, &selection, |line_index, baseline, offset| {
            let raw_top = f64::from(*self.parley_line_tops_pt.get(line_index)?);
            let adjustment = f64::from(*self.baseline_adjustments_pt.get(line_index)?);
            Some(f64::from(baseline) - f64::from(offset) - raw_top + adjustment)
        })
    }

    /// 返回与自定义行框 baseline/溢出语义对齐的插入点矩形。
    pub fn cursor_geometry(&self, cursor: VisualCursor, width_pt: f32) -> parley::BoundingBox {
        self.cursor_geometry_with_line(cursor, width_pt)
            .map(|(bounds, _)| bounds)
            .unwrap_or_else(|| parley_cursor(&self.layout, cursor).geometry(&self.layout, width_pt))
    }

    /// 返回插入点矩形和所属的 Caret 行索引。
    pub fn cursor_geometry_with_line(
        &self,
        cursor: VisualCursor,
        width_pt: f32,
    ) -> Option<(parley::BoundingBox, usize)> {
        if let Some((line_index, edge_x)) = self.inline_object_cursor_edge(cursor) {
            let line = self.layout.get(line_index)?;
            let metrics = line.metrics();
            let raw_top = metrics.block_min_coord.min(metrics.content_block_min_coord);
            let raw_bottom = metrics.block_max_coord.max(metrics.content_block_max_coord);
            let offset = f64::from(
                self.line_top_offsets_pt.get(line_index)?
                    + self.baseline_adjustments_pt.get(line_index)?,
            );
            return Some((
                parley::BoundingBox::new(
                    f64::from(edge_x),
                    f64::from(raw_top) + offset,
                    f64::from(edge_x + width_pt),
                    f64::from(raw_bottom) + offset,
                ),
                line_index,
            ));
        }
        let cursor = parley_cursor(&self.layout, cursor);
        if self.layout.text_len() == 0
            && let Some(line) = self.paragraph.lines.first()
        {
            let caret_x = f64::from(self.empty_line_indent_pt);
            return Some((
                parley::BoundingBox::new(caret_x, 0.0, caret_x, f64::from(line.metrics.height_pt)),
                0,
            ));
        }
        let (mut bounds, line_index) =
            cursor_geometry_on_logical_cluster(&self.layout, cursor, width_pt).unwrap_or_else(
                || {
                    let bounds = cursor.geometry(&self.layout, width_pt);
                    let cursor_mid_y = ((bounds.y0 + bounds.y1) / 2.0) as f32;
                    let line_index = self
                        .parley_line_tops_pt
                        .partition_point(|top| *top <= cursor_mid_y)
                        .saturating_sub(1)
                        .min(self.parley_line_tops_pt.len().saturating_sub(1));
                    (bounds, line_index)
                },
            );
        if self.parley_line_tops_pt.is_empty() {
            if let Some(line) = self.paragraph.lines.first() {
                return Some((
                    parley::BoundingBox::new(
                        0.0,
                        0.0,
                        f64::from(width_pt),
                        f64::from(line.metrics.height_pt),
                    ),
                    0,
                ));
            }
            return Some((bounds, line_index));
        }
        if let (Some(top_offset), Some(baseline_adjustment)) = (
            self.line_top_offsets_pt.get(line_index),
            self.baseline_adjustments_pt.get(line_index),
        ) {
            let offset = f64::from(*top_offset + *baseline_adjustment);
            bounds.y0 += offset;
            bounds.y1 += offset;
        }
        Some((bounds, line_index))
    }

    fn inline_object_at_point(&self, x: f32, line_index: usize) -> Option<(Range<usize>, bool)> {
        let line = self.layout.get(line_index)?;
        for item in line.items() {
            let PositionedLayoutItem::InlineBox(inline_box) = item else {
                continue;
            };
            if x < inline_box.x || x > inline_box.x + inline_box.width {
                continue;
            }
            let source_range = self
                .inline_object_source_ranges
                .iter()
                .find(|(id, _)| *id == inline_box.id)
                .map(|(_, range)| range.clone())?;
            let rtl = self.inline_object_is_rtl(inline_box.id, &source_range);
            let visual_left = x < inline_box.x + inline_box.width / 2.0;
            return Some((source_range, visual_left != rtl));
        }
        None
    }

    fn positioned_inline_box(&self, id: u64) -> Option<(usize, parley::PositionedInlineBox)> {
        for (line_index, line) in self.layout.lines().enumerate() {
            for item in line.items() {
                if let PositionedLayoutItem::InlineBox(inline_box) = item
                    && inline_box.id == id
                {
                    return Some((line_index, inline_box));
                }
            }
        }
        None
    }

    fn inline_object_cursor_edge(&self, cursor: VisualCursor) -> Option<(usize, f32)> {
        let byte_index = cursor.byte_index();
        let object = match cursor.affinity() {
            VisualAffinity::Downstream => self
                .inline_object_source_ranges
                .iter()
                .find(|(_, range)| range.start == byte_index),
            VisualAffinity::Upstream => self
                .inline_object_source_ranges
                .iter()
                .find(|(_, range)| range.end == byte_index),
        }?;
        let (id, range) = object;
        let (line_index, inline_box) = self.positioned_inline_box(*id)?;
        let rtl = self.inline_object_is_rtl(*id, range);
        let logical_start = cursor.affinity() == VisualAffinity::Downstream;
        let visual_left = logical_start != rtl;
        Some((
            line_index,
            if visual_left {
                inline_box.x
            } else {
                inline_box.x + inline_box.width
            },
        ))
    }

    fn inline_object_is_rtl(&self, id: u64, source_range: &Range<usize>) -> bool {
        for line in self.layout.lines() {
            let contains_object = line.items().any(|item| {
                matches!(item, PositionedLayoutItem::InlineBox(inline_box) if inline_box.id == id)
            });
            if !contains_object {
                continue;
            }
            if let Some(run) = line.runs().find(|run| {
                let range = run.text_range();
                range.start <= source_range.start && source_range.end <= range.end
            }) {
                return run.is_rtl();
            }
            return self.layout.is_rtl();
        }
        self.layout.is_rtl()
    }

    fn raw_line_y(&self, y: f32) -> Option<(usize, f32)> {
        if !y.is_finite() {
            return None;
        }
        let line_index = self
            .line_box_tops_pt
            .partition_point(|top| *top <= y)
            .saturating_sub(1)
            .min(self.line_box_tops_pt.len().checked_sub(1)?);
        let top = *self.line_box_tops_pt.get(line_index)?;
        let bottom = top + self.paragraph.lines.get(line_index)?.metrics.height_pt;
        if y < top || y > bottom {
            return None;
        }
        let raw_top = *self.parley_line_tops_pt.get(line_index)?;
        let raw_height = *self.parley_line_heights_pt.get(line_index)?;
        if !raw_top.is_finite() || !raw_height.is_finite() || raw_height <= 0.0 {
            return None;
        }
        Some((line_index, raw_top + raw_height / 2.0))
    }
}

impl TextIslandLayout {
    /// 将文本岛局部坐标命中到 UTF-8 字节边界，并保留 Parley 的视觉侧与逻辑侧。
    pub fn cursor_from_point(&self, x_pt: f32, y_pt: f32) -> Option<ParagraphHit> {
        if !x_pt.is_finite() || !y_pt.is_finite() {
            return None;
        }
        if self.layout.text_len() == 0 {
            return Some(ParagraphHit {
                cursor: VisualCursor {
                    byte_index: 0,
                    affinity: VisualAffinity::Downstream,
                },
                logical_affinity: HitAffinity::Downstream,
            });
        }
        parley_hit_from_point(&self.layout, &self.grapheme_boundaries, x_pt, y_pt)
    }

    /// 字节偏移必须落在扩展字素簇边界；非法位置不静默吸附。
    pub fn cursor_at(&self, byte_index: usize, affinity: VisualAffinity) -> Option<VisualCursor> {
        cursor_at_grapheme_boundary(&self.grapheme_boundaries, byte_index, affinity)
    }

    /// 在同一文本岛内按 Parley 视觉顺序移动到下一个字素停靠点。
    pub fn move_visual(&self, cursor: VisualCursor, forward: bool) -> Option<VisualCursor> {
        self.cursor_at(cursor.byte_index, cursor.affinity)?;
        move_visual_cursor(&self.layout, &self.grapheme_boundaries, cursor, forward)
    }

    /// 在相邻视觉行按期望 x 移动，并返回可复用的目标 x。
    pub fn move_vertical(
        &self,
        cursor: VisualCursor,
        line_delta: isize,
        preferred_x: Option<f32>,
    ) -> Option<(VisualCursor, f32)> {
        if line_delta == 0 {
            return None;
        }
        let raw = parley_cursor(&self.layout, cursor);
        let (current_bounds, current_line) =
            cursor_geometry_on_logical_cluster(&self.layout, raw, 0.0)?;
        let target_line = current_line.checked_add_signed(line_delta)?;
        let line = self.layout.get(target_line)?;
        let metrics = line.metrics();
        let x = preferred_x.unwrap_or(current_bounds.x0 as f32);
        if !x.is_finite() {
            return None;
        }
        let y = (metrics.block_min_coord + metrics.block_max_coord) * 0.5;
        let hit = parley_hit_from_point(&self.layout, &self.grapheme_boundaries, x, y)?;
        Some((hit.cursor, x))
    }

    /// 将光标移到当前视觉行首尾，并校验结果落在扩展字素边界。
    pub fn move_to_visual_line_edge(
        &self,
        cursor: VisualCursor,
        end: bool,
    ) -> Option<VisualCursor> {
        move_to_visual_line_edge(&self.layout, &self.grapheme_boundaries, cursor, end)
    }

    /// 将视觉 caret 侧映射为公式树端点的逻辑邻接侧。
    pub fn logical_affinity_at(
        &self,
        byte_index: usize,
        affinity: VisualAffinity,
    ) -> Option<HitAffinity> {
        logical_affinity_for_cursor(
            &self.layout,
            &self.grapheme_boundaries,
            byte_index,
            affinity,
        )
    }

    /// 返回文本岛局部坐标中的插入点；非法 UTF-8 或非字素边界位置不吸附。
    pub fn cursor_geometry(
        &self,
        cursor: VisualCursor,
        width_pt: f32,
    ) -> Option<parley::BoundingBox> {
        self.cursor_geometry_with_line(cursor, width_pt)
            .map(|(bounds, _)| bounds)
    }

    /// 返回插入点矩形和所属视觉行，供数学文本岛映射到宿主行基线。
    pub fn cursor_geometry_with_line(
        &self,
        cursor: VisualCursor,
        width_pt: f32,
    ) -> Option<(parley::BoundingBox, usize)> {
        if !width_pt.is_finite()
            || width_pt < 0.0
            || self
                .grapheme_boundaries
                .binary_search(&cursor.byte_index)
                .is_err()
        {
            return None;
        }
        let raw = parley_cursor(&self.layout, cursor);
        if self.layout.text_len() == 0 {
            return Some((raw.geometry(&self.layout, width_pt), 0));
        }
        let (bounds, line_index) = cursor_geometry_on_logical_cluster(&self.layout, raw, width_pt)
            .unwrap_or_else(|| {
                let bounds = raw.geometry(&self.layout, width_pt);
                let midpoint = ((bounds.y0 + bounds.y1) / 2.0) as f32;
                let index = self
                    .layout
                    .lines()
                    .position(|line| {
                        let metrics = line.metrics();
                        metrics.block_min_coord <= midpoint && midpoint <= metrics.block_max_coord
                    })
                    .unwrap_or(0);
                (bounds, index)
            });
        Some((bounds, line_index))
    }

    /// 以稳定文本偏移和视觉侧查询插入点，不静默吸附非法偏移。
    pub fn cursor_geometry_at(
        &self,
        byte_index: usize,
        affinity: VisualAffinity,
        width_pt: f32,
    ) -> Option<parley::BoundingBox> {
        self.cursor_geometry(self.cursor_at(byte_index, affinity)?, width_pt)
    }

    /// 返回文本岛局部坐标中的字素安全选区矩形。
    pub fn selection_geometry(
        &self,
        range: Range<usize>,
    ) -> Option<Vec<(parley::BoundingBox, usize)>> {
        selection_geometry_excluding_breaks(
            &self.layout,
            &self.grapheme_boundaries,
            &self.text,
            range,
            |range| {
                parley_selection_geometry(&self.layout, &self.grapheme_boundaries, range)
                    .map(|(rectangles, _)| rectangles)
            },
        )
    }

    /// 按文本岛中实际字体下划线度量生成输入法预编辑装饰。
    pub fn composition_geometry(
        &self,
        range: Range<usize>,
    ) -> Option<Vec<(parley::BoundingBox, usize)>> {
        let selection = self.selection_geometry(range)?;
        parley_composition_geometry(&self.layout, &selection, |_, baseline, offset| {
            Some(f64::from(baseline) - f64::from(offset))
        })
    }
}

fn cursor_at_grapheme_boundary(
    grapheme_boundaries: &[usize],
    byte_index: usize,
    affinity: VisualAffinity,
) -> Option<VisualCursor> {
    grapheme_boundaries
        .binary_search(&byte_index)
        .ok()
        .map(|_| VisualCursor {
            byte_index,
            affinity,
        })
}

fn parley_hit_from_point(
    layout: &Layout<[u8; 4]>,
    grapheme_boundaries: &[usize],
    x: f32,
    y: f32,
) -> Option<ParagraphHit> {
    use parley::editing::Cursor;

    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let raw = Cursor::from_point(layout, x, y);
    let raw_index = raw.index();
    let original_affinity = match raw.affinity() {
        parley::Affinity::Upstream => VisualAffinity::Upstream,
        parley::Affinity::Downstream => VisualAffinity::Downstream,
    };
    let (byte_index, exact_boundary) =
        snap_grapheme_boundary(grapheme_boundaries, raw_index, raw.affinity())?;
    let affinity = if exact_boundary {
        normalize_visual_affinity(
            layout,
            grapheme_boundaries,
            byte_index,
            original_affinity,
            f64::from(x),
        )?
    } else {
        original_affinity
    };
    let logical_affinity = if exact_boundary && affinity != original_affinity {
        logical_affinity_for_cursor(layout, grapheme_boundaries, byte_index, affinity)?
    } else {
        hit_affinity(raw.affinity())
    };
    Some(ParagraphHit {
        cursor: VisualCursor {
            byte_index,
            affinity,
        },
        logical_affinity,
    })
}

fn selection_geometry_excluding_breaks(
    layout: &Layout<[u8; 4]>,
    grapheme_boundaries: &[usize],
    text: &str,
    range: Range<usize>,
    mut selection_geometry: impl FnMut(Range<usize>) -> Option<Vec<(parley::BoundingBox, usize)>>,
) -> Option<Vec<(parley::BoundingBox, usize)>> {
    text.get(range.clone())?;
    let range = grapheme_safe_selection_range(layout, grapheme_boundaries, range)?;
    text.get(range.clone())?;
    let mut rectangles = Vec::new();
    let mut text_start = range.start;
    for (relative_offset, character) in text[range.clone()].char_indices() {
        if !is_mandatory_break_character(character) {
            continue;
        }
        let break_start = range.start + relative_offset;
        if text_start < break_start {
            rectangles.extend(selection_geometry(text_start..break_start)?);
        }
        text_start = break_start + character.len_utf8();
    }
    if text_start < range.end {
        rectangles.extend(selection_geometry(text_start..range.end)?);
    }
    Some(rectangles)
}

fn parley_composition_geometry(
    layout: &Layout<[u8; 4]>,
    selection: &[(parley::BoundingBox, usize)],
    mut underline_top: impl FnMut(usize, f32, f32) -> Option<f64>,
) -> Option<Vec<(parley::BoundingBox, usize)>> {
    let mut result: Vec<(parley::BoundingBox, usize)> = Vec::new();
    for (bounds, line_index) in selection {
        let line = layout.get(*line_index)?;
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run_x0 = f64::from(glyph_run.offset());
            let run_x1 = run_x0 + f64::from(glyph_run.advance());
            let x0 = bounds.x0.max(run_x0);
            let x1 = bounds.x1.min(run_x1);
            if x1 <= x0 {
                continue;
            }
            let metrics = glyph_run.run().font_metrics();
            let thickness = f64::from(if metrics.underline_size > 0.0 {
                metrics.underline_size
            } else {
                glyph_run.run().font_size() / 18.0
            });
            let y0 = underline_top(*line_index, glyph_run.baseline(), metrics.underline_offset)?;
            if !y0.is_finite() || !thickness.is_finite() || thickness <= 0.0 {
                continue;
            }
            if let Some((previous, previous_line)) = result.last_mut()
                && *previous_line == *line_index
                && (previous.x1 - x0).abs() < 0.01
                && (previous.y0 - y0).abs() < 0.01
                && (previous.y1 - (y0 + thickness)).abs() < 0.01
            {
                previous.x1 = x1;
            } else {
                result.push((
                    parley::BoundingBox::new(x0, y0, x1, y0 + thickness),
                    *line_index,
                ));
            }
        }
    }
    Some(result)
}

fn snap_grapheme_boundary(
    boundaries: &[usize],
    index: usize,
    affinity: parley::Affinity,
) -> Option<(usize, bool)> {
    match boundaries.binary_search(&index) {
        Ok(boundary_index) => Some((*boundaries.get(boundary_index)?, true)),
        Err(insertion_index) => {
            let boundary_index = match affinity {
                parley::Affinity::Downstream => insertion_index.checked_sub(1)?,
                parley::Affinity::Upstream => insertion_index,
            };
            Some((*boundaries.get(boundary_index)?, false))
        }
    }
}

fn parley_cursor(layout: &Layout<[u8; 4]>, cursor: VisualCursor) -> parley::editing::Cursor {
    parley::editing::Cursor::from_byte_index(
        layout,
        cursor.byte_index,
        match cursor.affinity {
            VisualAffinity::Upstream => parley::Affinity::Upstream,
            VisualAffinity::Downstream => parley::Affinity::Downstream,
        },
    )
}

fn visual_cursor(cursor: parley::editing::Cursor) -> VisualCursor {
    VisualCursor {
        byte_index: cursor.index(),
        affinity: match cursor.affinity() {
            parley::Affinity::Upstream => VisualAffinity::Upstream,
            parley::Affinity::Downstream => VisualAffinity::Downstream,
        },
    }
}

fn move_visual_cursor(
    layout: &Layout<[u8; 4]>,
    grapheme_boundaries: &[usize],
    cursor: VisualCursor,
    forward: bool,
) -> Option<VisualCursor> {
    let origin = parley_cursor(layout, cursor);
    if grapheme_boundaries
        .binary_search(&cursor.byte_index)
        .is_err()
    {
        return None;
    }
    let mut candidate = origin;
    for _ in 0..=layout.text_len().saturating_add(1) {
        candidate = if forward {
            candidate.next_visual(layout)
        } else {
            candidate.previous_visual(layout)
        };
        if candidate == origin {
            return None;
        }
        if grapheme_boundaries
            .binary_search(&candidate.index())
            .is_ok()
        {
            return Some(visual_cursor(candidate));
        }
    }
    None
}

fn move_to_visual_line_edge(
    layout: &Layout<[u8; 4]>,
    grapheme_boundaries: &[usize],
    cursor: VisualCursor,
    end: bool,
) -> Option<VisualCursor> {
    if grapheme_boundaries
        .binary_search(&cursor.byte_index)
        .is_err()
    {
        return None;
    }
    let selection = parley::editing::Selection::from(parley_cursor(layout, cursor));
    let moved = if end {
        selection.line_end(layout, false)
    } else {
        selection.line_start(layout, false)
    };
    let cursor = visual_cursor(moved.focus());
    grapheme_boundaries
        .binary_search(&cursor.byte_index)
        .is_ok()
        .then_some(cursor)
}

/// 行内对象在同代排版中的版面盒、选区覆盖状态，以及对象盒左侧/右侧是否落在
/// 选区的视觉范围内（用于区分被选中的文本与 Parley 退化推进的伪影尾段）。
#[derive(Debug, Clone, Copy)]
struct InlineObjectSelectionBox {
    line_index: usize,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    selected: bool,
    keep_left: bool,
    keep_right: bool,
}

/// 将 Parley 选区矩形规整为与平台字素划分无关的对象边界几何。
///
/// Parley 会把行内对象推进折进其后首个字素簇：对象被选中时，相邻文本矩形可能
/// 覆盖对象盒或膨胀进对象宽度；对象未被选中时，后续文本矩形可能回溯到对象框。
/// 因此先从所有矩形裁除每个非零宽对象盒，再仅对被完整选中的对象补一次整盒；
/// 最后合并首尾相接的同段矩形。裁除时只保留确实落在选区视觉范围内的左右文本段，
/// 丢弃对象盒之外由退化推进产生的伪影尾段（例如选区终点就在对象终点时，右侧
/// `[object_x1, object_x1 + next_grapheme]` 并不是真正选中的文本）。零宽对象没有
/// 可见几何，既不裁除也不补盒。
fn normalize_inline_object_selection_rectangles(
    rectangles: Vec<(parley::BoundingBox, usize)>,
    object_boxes: &[InlineObjectSelectionBox],
) -> Vec<(parley::BoundingBox, usize)> {
    let mut rectangles = rectangles;
    for object in object_boxes.iter().filter(|object| object.x1 > object.x0) {
        rectangles = trim_selection_interval(
            rectangles,
            object.line_index,
            object.x0,
            object.x1,
            object.keep_left,
            object.keep_right,
        );
    }
    for object in object_boxes
        .iter()
        .filter(|object| object.selected && object.x1 > object.x0)
    {
        rectangles.push((
            parley::BoundingBox::new(object.x0, object.y0, object.x1, object.y1),
            object.line_index,
        ));
    }
    rectangles.sort_by(|(left, left_line), (right, right_line)| {
        left_line
            .cmp(right_line)
            .then_with(|| left.x0.total_cmp(&right.x0))
    });
    merge_contiguous_selection_rectangles(&mut rectangles);
    rectangles
}

/// 从同一行的选区矩形中裁除指定 x 区间，必要时拆成左右两段；
/// `keep_left`/`keep_right` 为 false 时丢弃对应侧的余段（退化推进伪影）。
fn trim_selection_interval(
    rectangles: Vec<(parley::BoundingBox, usize)>,
    line_index: usize,
    interval_x0: f64,
    interval_x1: f64,
    keep_left: bool,
    keep_right: bool,
) -> Vec<(parley::BoundingBox, usize)> {
    let mut trimmed = Vec::with_capacity(rectangles.len());
    for (bounds, rectangle_line) in rectangles {
        if rectangle_line != line_index || bounds.x1 <= interval_x0 || bounds.x0 >= interval_x1 {
            trimmed.push((bounds, rectangle_line));
            continue;
        }
        if keep_left && bounds.x0 < interval_x0 {
            trimmed.push((
                parley::BoundingBox::new(bounds.x0, bounds.y0, interval_x0, bounds.y1),
                rectangle_line,
            ));
        }
        if keep_right && bounds.x1 > interval_x1 {
            trimmed.push((
                parley::BoundingBox::new(interval_x1, bounds.y0, bounds.x1, bounds.y1),
                rectangle_line,
            ));
        }
    }
    trimmed
}

/// 合并同一行内首尾相接的矩形，保持“一行一段连续选区”的稳定输出。
/// 合并同一行、纵向范围相同且首尾相接的矩形，避免对象盒与相邻文本矩形
/// 在仅有 1e-6 精度差时留下视觉接缝。
fn merge_contiguous_selection_rectangles(rectangles: &mut Vec<(parley::BoundingBox, usize)>) {
    let mut merged: Vec<(parley::BoundingBox, usize)> = Vec::with_capacity(rectangles.len());
    for (bounds, line_index) in rectangles.drain(..) {
        if let Some((previous, previous_line)) = merged.last_mut()
            && *previous_line == line_index
            && (previous.y0 - bounds.y0).abs() < 1e-6
            && (previous.y1 - bounds.y1).abs() < 1e-6
            && (previous.x1 - bounds.x0).abs() < 1e-6
        {
            previous.x1 = bounds.x1;
            continue;
        }
        merged.push((bounds, line_index));
    }
    *rectangles = merged;
}

fn parley_selection_geometry(
    layout: &Layout<[u8; 4]>,
    grapheme_boundaries: &[usize],
    range: Range<usize>,
) -> Option<ParleySelectionResult> {
    let range = grapheme_safe_selection_range(layout, grapheme_boundaries, range)?;
    if range.is_empty() {
        return Some((Vec::new(), range));
    }
    let start = range.start;
    let end = range.end;
    let anchor =
        parley::editing::Cursor::from_byte_index(layout, start, parley::Affinity::Downstream);
    let focus = parley::editing::Cursor::from_byte_index(layout, end, parley::Affinity::Upstream);
    Some((
        parley::editing::Selection::new(anchor, focus).geometry(layout),
        range,
    ))
}

fn grapheme_safe_selection_range(
    layout: &Layout<[u8; 4]>,
    grapheme_boundaries: &[usize],
    range: Range<usize>,
) -> Option<Range<usize>> {
    if range.start > range.end || range.end > layout.text_len() {
        return None;
    }
    if range.is_empty() {
        return grapheme_boundaries
            .binary_search(&range.start)
            .ok()
            .map(|_| range);
    }
    let start_index = grapheme_boundaries
        .partition_point(|boundary| *boundary <= range.start)
        .saturating_sub(1);
    let start = *grapheme_boundaries.get(start_index)?;
    let end_index = grapheme_boundaries.partition_point(|boundary| *boundary < range.end);
    let end = *grapheme_boundaries.get(end_index)?;
    if start == end {
        return Some(start..end);
    }
    Some(start..end)
}

fn logical_affinity_for_cursor(
    layout: &Layout<[u8; 4]>,
    grapheme_boundaries: &[usize],
    byte_index: usize,
    affinity: VisualAffinity,
) -> Option<HitAffinity> {
    if grapheme_boundaries.binary_search(&byte_index).is_err() {
        return None;
    }
    let cursor = parley::editing::Cursor::from_byte_index(
        layout,
        byte_index,
        match affinity {
            VisualAffinity::Upstream => parley::Affinity::Upstream,
            VisualAffinity::Downstream => parley::Affinity::Downstream,
        },
    );
    let bounds = cursor_geometry_on_logical_cluster(layout, cursor, 0.0)
        .map_or_else(|| cursor.geometry(layout, 0.0), |(bounds, _)| bounds);
    let opposite = match cursor.affinity() {
        parley::Affinity::Upstream => parley::Affinity::Downstream,
        parley::Affinity::Downstream => parley::Affinity::Upstream,
    };
    let other = parley::editing::Cursor::from_byte_index(layout, byte_index, opposite);
    let other_bounds = cursor_geometry_on_logical_cluster(layout, other, 0.0)
        .map_or_else(|| other.geometry(layout, 0.0), |(bounds, _)| bounds);
    Some(
        if (bounds.x0 - other_bounds.x0).abs() <= 0.0001
            && (bounds.y0 - other_bounds.y0).abs() <= 0.0001
        {
            HitAffinity::Downstream
        } else {
            hit_affinity(cursor.affinity())
        },
    )
}

fn normalize_visual_affinity(
    layout: &Layout<[u8; 4]>,
    grapheme_boundaries: &[usize],
    byte_index: usize,
    affinity: VisualAffinity,
    hit_x: f64,
) -> Option<VisualAffinity> {
    if grapheme_boundaries.binary_search(&byte_index).is_err() || !hit_x.is_finite() {
        return None;
    }
    let cursor = parley_cursor(
        layout,
        VisualCursor {
            byte_index,
            affinity,
        },
    );
    let bounds = cursor_geometry_on_logical_cluster(layout, cursor, 0.0)
        .map_or_else(|| cursor.geometry(layout, 0.0), |(bounds, _)| bounds);
    let opposite_affinity = match cursor.affinity() {
        parley::Affinity::Upstream => parley::Affinity::Downstream,
        parley::Affinity::Downstream => parley::Affinity::Upstream,
    };
    let opposite = parley::editing::Cursor::from_byte_index(layout, byte_index, opposite_affinity);
    let opposite_bounds = cursor_geometry_on_logical_cluster(layout, opposite, 0.0)
        .map_or_else(|| opposite.geometry(layout, 0.0), |(bounds, _)| bounds);
    let same_visual_position = (bounds.x0 - opposite_bounds.x0).abs() <= 0.0001
        && (bounds.y0 - opposite_bounds.y0).abs() <= 0.0001;
    Some(
        if same_visual_position && (hit_x - bounds.x0).abs() <= 0.0001 {
            VisualAffinity::Downstream
        } else {
            affinity
        },
    )
}

fn cursor_geometry_on_logical_cluster(
    layout: &Layout<[u8; 4]>,
    cursor: parley::editing::Cursor,
    width: f32,
) -> Option<(parley::BoundingBox, usize)> {
    let [upstream, downstream] = cursor.logical_clusters(layout);
    if upstream.is_some_and(|cluster| cluster.is_hard_line_break()) {
        if let Some(cluster) = downstream {
            return cluster_cursor_geometry(cluster, cluster.is_rtl(), width);
        }
        let line_index = layout.len().checked_sub(1)?;
        let line = layout.get(line_index)?;
        let metrics = line.metrics();
        return Some((
            parley::BoundingBox::new(
                f64::from(metrics.offset),
                f64::from(metrics.block_min_coord.min(metrics.content_block_min_coord)),
                f64::from(metrics.offset + width),
                f64::from(metrics.block_max_coord.max(metrics.content_block_max_coord)),
            ),
            line_index,
        ));
    }
    let (cluster, at_end) = match cursor.affinity() {
        parley::Affinity::Upstream => {
            let cluster = upstream.or(downstream)?;
            (cluster, !cluster.is_rtl())
        }
        parley::Affinity::Downstream => {
            let cluster = downstream.or(upstream)?;
            (cluster, cluster.is_rtl())
        }
    };
    cluster_cursor_geometry(cluster, at_end, width)
}

fn cluster_cursor_geometry(
    cluster: parley::layout::Cluster<'_, [u8; 4]>,
    at_end: bool,
    width: f32,
) -> Option<(parley::BoundingBox, usize)> {
    let mut x = cluster.visual_offset()?;
    if at_end {
        x += cluster.advance();
    }
    let line = cluster.line();
    let metrics = line.metrics();
    let bounds = parley::BoundingBox::new(
        f64::from(x),
        f64::from(metrics.block_min_coord.min(metrics.content_block_min_coord)),
        f64::from(x + width),
        f64::from(metrics.block_max_coord.max(metrics.content_block_max_coord)),
    );
    Some((bounds, cluster.path().line_index()))
}

fn hit_affinity(affinity: parley::Affinity) -> HitAffinity {
    match affinity {
        parley::Affinity::Upstream => HitAffinity::Upstream,
        parley::Affinity::Downstream => HitAffinity::Downstream,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InlineObjectSelectionBox, ParleyParagraphLayout, merge_contiguous_selection_rectangles,
        normalize_inline_object_selection_rectangles, trim_selection_interval,
    };
    use parley::BoundingBox;

    fn rectangle(x0: f64, x1: f64) -> (BoundingBox, usize) {
        (BoundingBox::new(x0, 0.0, x1, 20.0), 0)
    }

    fn bounds(rectangles: &[(BoundingBox, usize)]) -> Vec<(f64, f64)> {
        rectangles
            .iter()
            .map(|(bounds, _)| (bounds.x0, bounds.x1))
            .collect()
    }

    fn object_box(selected: bool, keep_left: bool, keep_right: bool) -> InlineObjectSelectionBox {
        InlineObjectSelectionBox {
            line_index: 0,
            x0: 12.0,
            x1: 42.0,
            y0: 0.0,
            y1: 20.0,
            selected,
            keep_left,
            keep_right,
        }
    }

    #[test]
    fn trim_selection_interval_splits_and_drops_overlapping_rectangles() {
        let trimmed =
            trim_selection_interval(vec![rectangle(0.0, 50.0)], 0, 20.0, 40.0, true, true);
        assert_eq!(bounds(&trimmed), [(0.0, 20.0), (40.0, 50.0)]);

        let partial =
            trim_selection_interval(vec![rectangle(10.0, 30.0)], 0, 20.0, 40.0, true, true);
        assert_eq!(bounds(&partial), [(10.0, 20.0)]);

        let dropped =
            trim_selection_interval(vec![rectangle(25.0, 35.0)], 0, 20.0, 40.0, true, true);
        assert!(dropped.is_empty());

        // 侧向保留开关用于丢弃对象盒之外、不在选区视觉范围内的退化推进伪影。
        let kept_left_only =
            trim_selection_interval(vec![rectangle(0.0, 50.0)], 0, 20.0, 40.0, true, false);
        assert_eq!(bounds(&kept_left_only), [(0.0, 20.0)]);
        let kept_right_only =
            trim_selection_interval(vec![rectangle(0.0, 50.0)], 0, 20.0, 40.0, false, true);
        assert_eq!(bounds(&kept_right_only), [(40.0, 50.0)]);
        let dropped_both =
            trim_selection_interval(vec![rectangle(0.0, 50.0)], 0, 20.0, 40.0, false, false);
        assert!(dropped_both.is_empty());
    }

    #[test]
    fn merge_contiguous_selection_rectangles_joins_adjacent_same_line_pieces_only() {
        let mut rectangles = vec![
            rectangle(0.0, 12.0),
            rectangle(12.0, 42.0),
            rectangle(45.0, 60.0),
        ];
        merge_contiguous_selection_rectangles(&mut rectangles);
        assert_eq!(bounds(&rectangles), [(0.0, 42.0), (45.0, 60.0)]);

        // 不同行不能合并，即使 x 区间首尾相接。
        let mut different_lines = vec![rectangle(0.0, 12.0), rectangle(12.0, 42.0)];
        different_lines[1].1 = 1;
        merge_contiguous_selection_rectangles(&mut different_lines);
        assert_eq!(bounds(&different_lines), [(0.0, 12.0), (12.0, 42.0)]);
    }

    #[test]
    fn duplicate_platform_rectangles_collapse_to_one_object_interval() {
        // 模拟另一平台 Parley 输出：对象矩形与其后字素矩形都从对象左缘开始，
        // 其中后者被折进了对象推进，看起来比对象宽一个字素。选区终点就在对象终点
        // （keep_right=false），对象盒之后的尾段是退化伪影，必须整体丢弃。
        let rectangles = normalize_inline_object_selection_rectangles(
            vec![rectangle(12.0, 42.0), rectangle(12.0, 47.76)],
            &[object_box(true, false, false)],
        );
        assert_eq!(bounds(&rectangles), [(12.0, 42.0)]);
    }

    #[test]
    fn selected_object_with_following_selected_text_keeps_the_legitimate_tail() {
        // 选区确实延伸到对象之后（keep_right=true）：尾段是已选中的后一字素，
        // 归一化后应与对象盒合并成连续高亮，而不是被裁掉。
        let rectangles = normalize_inline_object_selection_rectangles(
            vec![rectangle(12.0, 42.0), rectangle(12.0, 47.76)],
            &[object_box(true, false, true)],
        );
        assert_eq!(bounds(&rectangles), [(12.0, 47.76)]);
    }

    #[test]
    fn unselected_object_artifact_is_trimmed_from_following_text_rectangles() {
        // 选区起点在对象之后：Parley 把对象推进折进后续字素，矩形回溯到对象左缘。
        let rectangles = normalize_inline_object_selection_rectangles(
            vec![rectangle(12.0, 60.0)],
            &[object_box(false, false, true)],
        );
        assert_eq!(bounds(&rectangles), [(42.0, 60.0)]);
    }

    #[test]
    fn zero_width_object_neither_trims_nor_adds_rectangles() {
        let rectangles = normalize_inline_object_selection_rectangles(
            vec![rectangle(12.0, 60.0)],
            &[InlineObjectSelectionBox {
                line_index: 0,
                x0: 12.0,
                x1: 12.0,
                y0: 0.0,
                y1: 20.0,
                selected: true,
                keep_left: true,
                keep_right: true,
            }],
        );
        assert_eq!(bounds(&rectangles), [(12.0, 60.0)]);
    }

    #[test]
    fn object_only_selection_drops_left_artifact_and_keeps_the_object_box() {
        // 镜像场景：选区只包含对象，平台把对象推进折进左侧字素导致矩形左溢；
        // keep_left=false 丢弃左侧伪影，只保留一次对象盒。
        let rectangles = normalize_inline_object_selection_rectangles(
            vec![rectangle(0.0, 40.0)],
            &[object_box(true, false, false)],
        );
        assert_eq!(bounds(&rectangles), [(12.0, 42.0)]);
    }

    #[test]
    fn selection_covering_text_before_object_keeps_the_left_text_piece() {
        let rectangles = normalize_inline_object_selection_rectangles(
            vec![rectangle(0.0, 40.0)],
            &[object_box(true, true, false)],
        );
        assert_eq!(bounds(&rectangles), [(0.0, 42.0)]);
    }

    #[test]
    fn underline_font_offset_survives_large_document_baselines() {
        let baseline = 1_000_000.0_f32;
        let offset = 0.03125_f32;
        let top = 999_988.0_f32;
        let actual = ParleyParagraphLayout::underline_top_pt(baseline, offset, top, 0.0);
        assert!((actual - 11.96875).abs() < 0.001);
        assert_ne!(
            f64::from(baseline - offset - top),
            actual,
            "the regression fixture must expose f32 cancellation"
        );
    }
}
