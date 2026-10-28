//! 将段落布局映射为视觉光标、命中测试和交互几何。

use std::ops::Range;

use super::{HitAffinity, ParagraphHit, ParleyParagraphLayout, VisualAffinity, VisualCursor};
use parley::{Layout, PositionedLayoutItem};

impl ParleyParagraphLayout {
    /// 字节偏移必须恰好落在扩展字素簇边界；非法位置不静默吸附。
    pub fn cursor_at(&self, byte_index: usize, affinity: VisualAffinity) -> Option<VisualCursor> {
        self.grapheme_boundaries
            .binary_search(&byte_index)
            .ok()
            .map(|_| VisualCursor {
                byte_index,
                affinity,
            })
    }

    /// 按视觉顺序移动，并跳过 Parley 在 Run 内部暴露的字素停靠点。
    pub fn move_visual(&self, cursor: VisualCursor, forward: bool) -> Option<VisualCursor> {
        let mut candidate = self.to_parley_cursor(cursor);
        for _ in 0..=self.layout.text_len().saturating_add(1) {
            candidate = if forward {
                candidate.next_visual(&self.layout)
            } else {
                candidate.previous_visual(&self.layout)
            };
            if candidate == self.to_parley_cursor(cursor) {
                return None;
            }
            if self
                .grapheme_boundaries
                .binary_search(&candidate.index())
                .is_ok()
            {
                return Some(Self::from_parley_cursor(candidate));
            }
        }
        None
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
        let selection = parley::editing::Selection::from(self.to_parley_cursor(cursor));
        let moved = if end {
            selection.line_end(&self.layout, false)
        } else {
            selection.line_start(&self.layout, false)
        };
        let target = Self::from_parley_cursor(moved.focus());
        self.grapheme_boundaries
            .binary_search(&target.byte_index)
            .is_ok()
            .then_some(target)
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
        use parley::editing::Cursor;

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
        let raw = Cursor::from_point(&self.layout, x, raw_y);
        let raw_index = raw.index();
        let (byte_index, logical_affinity) = match (
            self.grapheme_boundaries.binary_search(&raw_index),
            raw.affinity(),
        ) {
            (Ok(_), affinity) => {
                let cursor = Self::from_parley_cursor(raw);
                let bounds = self.cursor_geometry(cursor, 0.0);
                let same_position = [parley::Affinity::Upstream, parley::Affinity::Downstream]
                    .into_iter()
                    .filter(|candidate_affinity| *candidate_affinity != affinity)
                    .any(|candidate_affinity| {
                        let other =
                            Cursor::from_byte_index(&self.layout, raw_index, candidate_affinity);
                        let other_bounds = other.geometry(&self.layout, 0.0);
                        (bounds.x0 - other_bounds.x0).abs() <= 0.0001
                    });
                let logical_affinity =
                    if same_position && (f64::from(x) - bounds.x0).abs() <= 0.0001 {
                        HitAffinity::Downstream
                    } else {
                        hit_affinity(affinity)
                    };
                let visual_cursor = if same_position && (f64::from(x) - bounds.x0).abs() <= 0.0001 {
                    Self::from_parley_cursor(Cursor::from_byte_index(
                        &self.layout,
                        raw_index,
                        parley::Affinity::Downstream,
                    ))
                } else {
                    cursor
                };
                return Some(ParagraphHit {
                    cursor: visual_cursor,
                    logical_affinity,
                });
            }
            (Err(index), parley::Affinity::Downstream) => (
                *self.grapheme_boundaries.get(index.saturating_sub(1))?,
                HitAffinity::Downstream,
            ),
            (Err(index), parley::Affinity::Upstream) => {
                (*self.grapheme_boundaries.get(index)?, HitAffinity::Upstream)
            }
        };
        let affinity = match raw.affinity() {
            parley::Affinity::Upstream => VisualAffinity::Upstream,
            parley::Affinity::Downstream => VisualAffinity::Downstream,
        };
        Some(ParagraphHit {
            cursor: self.cursor_at(byte_index, affinity)?,
            logical_affinity,
        })
    }

    /// 返回字素安全的 Parley selection 几何，并映射到 Caret 自定义行框。
    pub fn selection_geometry(
        &self,
        range: Range<usize>,
    ) -> Option<Vec<(parley::BoundingBox, usize)>> {
        if range.start > range.end || range.end > self.layout.text_len() {
            return None;
        }
        let start_index = self
            .grapheme_boundaries
            .partition_point(|boundary| *boundary <= range.start)
            .saturating_sub(1);
        let start = *self.grapheme_boundaries.get(start_index)?;
        let end_index = self
            .grapheme_boundaries
            .partition_point(|boundary| *boundary < range.end);
        let end = *self.grapheme_boundaries.get(end_index)?;
        if start == end {
            return Some(Vec::new());
        }
        let anchor = parley::editing::Cursor::from_byte_index(
            &self.layout,
            start,
            parley::Affinity::Downstream,
        );
        let focus =
            parley::editing::Cursor::from_byte_index(&self.layout, end, parley::Affinity::Upstream);
        Some(
            parley::editing::Selection::new(anchor, focus)
                .geometry(&self.layout)
                .into_iter()
                .map(|(mut bounds, line_index)| {
                    if let (Some(top_offset), Some(line)) = (
                        self.line_top_offsets_pt.get(line_index),
                        self.paragraph.lines.get(line_index),
                    ) {
                        bounds.y0 += f64::from(*top_offset);
                        bounds.y1 = bounds.y0 + f64::from(line.metrics.height_pt);
                    }
                    (bounds, line_index)
                })
                .collect(),
        )
    }

    /// 返回可见文字的选区几何；段落及软换行符本身不产生高亮矩形。
    pub fn selection_text_geometry(
        &self,
        text: &str,
        range: Range<usize>,
    ) -> Option<Vec<(parley::BoundingBox, usize)>> {
        if range.start > range.end
            || range.end > text.len()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
            || range.end > self.layout.text_len()
        {
            return None;
        }

        let mut rectangles = Vec::new();
        let mut text_start = range.start;
        for (relative_offset, character) in text[range.clone()].char_indices() {
            if character != '\n' {
                continue;
            }
            let break_start = range.start + relative_offset;
            if text_start < break_start {
                rectangles.extend(self.selection_geometry(text_start..break_start)?);
            }
            text_start = break_start + character.len_utf8();
        }
        if text_start < range.end {
            rectangles.extend(self.selection_geometry(text_start..range.end)?);
        }
        Some(rectangles)
    }

    /// 预编辑下划线按视觉 run 的基线和字体度量定位，y 相对 Caret 行顶。
    pub fn composition_text_geometry(
        &self,
        text: &str,
        range: Range<usize>,
    ) -> Option<Vec<(parley::BoundingBox, usize)>> {
        let selection = self.selection_text_geometry(text, range)?;
        let mut result = Vec::new();
        for (bounds, line_index) in selection {
            let line = self.layout.get(line_index)?;
            let raw_top = f64::from(*self.parley_line_tops_pt.get(line_index)?);
            let adjustment = f64::from(*self.baseline_adjustments_pt.get(line_index)?);
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let x0 = bounds.x0.max(f64::from(glyph_run.offset()));
                let x1 = bounds
                    .x1
                    .min(f64::from(glyph_run.offset() + glyph_run.advance()));
                if x1 <= x0 {
                    continue;
                }
                let metrics = glyph_run.run().font_metrics();
                let thickness = f64::from(if metrics.underline_size > 0.0 {
                    metrics.underline_size
                } else {
                    glyph_run.run().font_size() / 18.0
                });
                let y0 = f64::from(glyph_run.baseline() - metrics.underline_offset) - raw_top
                    + adjustment;
                if y0.is_finite() && thickness.is_finite() && thickness > 0.0 {
                    result.push((
                        parley::BoundingBox::new(x0, y0, x1, y0 + thickness),
                        line_index,
                    ));
                }
            }
        }
        Some(result)
    }

    /// 返回与自定义行框 baseline/溢出语义对齐的插入点矩形。
    pub fn cursor_geometry(&self, cursor: VisualCursor, width_pt: f32) -> parley::BoundingBox {
        self.cursor_geometry_with_line(cursor, width_pt)
            .map(|(bounds, _)| bounds)
            .unwrap_or_else(|| {
                self.to_parley_cursor(cursor)
                    .geometry(&self.layout, width_pt)
            })
    }

    /// 返回插入点矩形和所属的 Caret 行索引。
    pub fn cursor_geometry_with_line(
        &self,
        cursor: VisualCursor,
        width_pt: f32,
    ) -> Option<(parley::BoundingBox, usize)> {
        let cursor = self.to_parley_cursor(cursor);
        if self.layout.text_len() == 0
            && let Some(line) = self.paragraph.lines.first()
        {
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

    fn to_parley_cursor(&self, cursor: VisualCursor) -> parley::editing::Cursor {
        parley::editing::Cursor::from_byte_index(
            &self.layout,
            cursor.byte_index,
            match cursor.affinity {
                VisualAffinity::Upstream => parley::Affinity::Upstream,
                VisualAffinity::Downstream => parley::Affinity::Downstream,
            },
        )
    }

    fn from_parley_cursor(cursor: parley::editing::Cursor) -> VisualCursor {
        VisualCursor {
            byte_index: cursor.index(),
            affinity: match cursor.affinity() {
                parley::Affinity::Upstream => VisualAffinity::Upstream,
                parley::Affinity::Downstream => VisualAffinity::Downstream,
            },
        }
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
