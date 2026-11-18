//! 计算公式文本岛的插入点、选区与键盘导航。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::items::{MathTextItemGeometry, math_text_caret_in_items, math_text_selection_in_items};
use super::*;

#[derive(Clone, Copy)]
enum MathTextMoveRequest {
    Visual(bool),
    Vertical(isize, Option<f32>),
    LineEdge(bool),
}

impl MathTextSidecar {
    pub(super) fn font_size_centipoints_at(
        &self,
        offset: usize,
        visual_affinity: VisualAffinity,
    ) -> u16 {
        let mut upstream = None;
        let mut downstream = None;
        for line in self.layout.layout.lines() {
            for item in line.items() {
                let caret_layout::PositionedGlyphLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let range = glyph_run.run().text_range();
                let Some(size) = font_size_centipoints(glyph_run.run().font_size()) else {
                    continue;
                };
                if range.start < offset && offset < range.end {
                    return size;
                }
                if range.end == offset {
                    upstream = Some(size);
                }
                if range.start == offset {
                    downstream = Some(size);
                }
            }
        }
        match visual_affinity {
            VisualAffinity::Upstream => upstream.or(downstream),
            VisualAffinity::Downstream => downstream.or(upstream),
        }
        .unwrap_or(self.fallback_font_size_centipoints)
    }

    pub(super) fn offset_at_position(&self, position: &MathTextPosition) -> Option<usize> {
        let downstream = position.affinity == MathPositionAffinity::Downstream;
        let matches_position = |span: &&MathTextSourceSpan| {
            if span.path != position.path || position.offset < span.flow_offset {
                return false;
            }
            position.offset
                <= span
                    .flow_offset
                    .saturating_add(span.range.end - span.range.start)
        };
        let span = if downstream {
            self.source_spans
                .iter()
                .find(matches_position)
                .filter(|span| {
                    position.offset
                        < span
                            .flow_offset
                            .saturating_add(span.range.end.saturating_sub(span.range.start))
                })
                .or_else(|| {
                    self.source_spans.iter().find(|span| {
                        span.path == position.path
                            && span.range.start == span.range.end
                            && span.flow_offset == position.offset
                    })
                })
                .or_else(|| self.source_spans.iter().rev().find(matches_position))
        } else {
            self.source_spans
                .iter()
                .rev()
                .find(matches_position)
                .filter(|span| position.offset > span.flow_offset)
                .or_else(|| {
                    self.source_spans.iter().rev().find(|span| {
                        span.path == position.path
                            && span.range.start == span.range.end
                            && span.flow_offset == position.offset
                    })
                })
                .or_else(|| self.source_spans.iter().find(matches_position))
        }?;
        span.range
            .start
            .checked_add(position.offset.checked_sub(span.flow_offset)?)
    }

    pub(crate) fn position_at_offset(
        &self,
        offset: usize,
        affinity: MathPositionAffinity,
    ) -> Option<MathTextPosition> {
        let downstream = affinity == MathPositionAffinity::Downstream;
        let span = if downstream {
            self.source_spans
                .iter()
                .find(|span| span.range.start <= offset && offset < span.range.end)
                .or_else(|| {
                    self.source_spans.iter().find(|span| {
                        span.range.start == span.range.end && span.range.start == offset
                    })
                })
                .or_else(|| {
                    self.source_spans
                        .last()
                        .filter(|span| span.range.end == offset)
                })
        } else {
            self.source_spans
                .iter()
                .rev()
                .find(|span| span.range.start < offset && offset <= span.range.end)
                .or_else(|| {
                    self.source_spans.iter().rev().find(|span| {
                        span.range.start == span.range.end && span.range.start == offset
                    })
                })
                .or_else(|| {
                    self.source_spans
                        .first()
                        .filter(|span| span.range.start == offset)
                })
        }?;
        let local_offset = span
            .flow_offset
            .checked_add(offset.checked_sub(span.range.start)?)?;
        Some(MathTextPosition::new(
            span.path.clone(),
            local_offset,
            affinity,
        ))
    }
}

impl DocumentLayoutSnapshot {
    /// 根据已提交 TextFlow 位置返回同代文本岛 Parley 几何中的数学光标。
    pub fn math_text_caret(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
    ) -> Option<(PageCaretGeometry, u16)> {
        self.math_text_caret_with_clipping(target, position, visual_affinity, true)
    }

    /// 返回未按宿主行框裁切的数学 caret，用于保留多行文本的视觉顺序。
    pub fn math_text_caret_geometry_for_visual_order(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
    ) -> Option<PageCaretGeometry> {
        self.math_text_caret_with_clipping(target, position, visual_affinity, false)
            .map(|(caret, _)| caret)
    }

    pub(super) fn math_text_caret_with_clipping(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        clip: bool,
    ) -> Option<(PageCaretGeometry, u16)> {
        for projection in self.math_projections_for_target(target) {
            let Some((layout, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            let geometry = MathTextItemGeometry {
                object_x_pt: projection.object_x_pt,
                object_baseline_y_pt: projection.baseline_y_pt,
                projection,
                items: &layout.items,
                sidecars,
            };
            if let Some((caret, size)) =
                math_text_caret_in_items(&geometry, target, position, visual_affinity)
            {
                return if clip {
                    projection
                        .clip_caret_for_row(caret, self.math_text_row_id(target, position))
                        .map(|caret| (caret, size))
                } else {
                    Some((caret, size))
                };
            }
        }
        None
    }

    /// 根据已提交 TextFlow 位置返回数学光标页面几何。
    pub fn math_text_caret_geometry(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
    ) -> Option<PageCaretGeometry> {
        self.math_text_caret(target, position, visual_affinity)
            .map(|(caret, _)| caret)
    }

    pub(crate) fn math_text_host(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
    ) -> Option<MathPositionHost> {
        for projection in self.math_projections_for_target(target) {
            let Some((_, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            for sidecar in sidecars.values() {
                if &sidecar.target == target && sidecar.offset_at_position(position).is_some() {
                    return Some(sidecar.host.clone());
                }
            }
        }
        None
    }

    /// 返回同一数学文本岛中端点之间、由该代 Parley 布局生成的选区矩形。
    pub fn math_text_selection_geometry(
        &self,
        target: &MathEditTarget,
        anchor: &MathTextPosition,
        focus: &MathTextPosition,
    ) -> Option<Vec<PageSelectionRect>> {
        self.math_text_range_geometry(target, anchor, focus, false)
    }

    /// 返回数学文本输入法预编辑范围的字体下划线几何。
    pub fn math_text_composition_geometry(
        &self,
        target: &MathEditTarget,
        anchor: &MathTextPosition,
        focus: &MathTextPosition,
    ) -> Option<Vec<PageSelectionRect>> {
        self.math_text_range_geometry(target, anchor, focus, true)
    }

    pub(super) fn math_text_range_geometry(
        &self,
        target: &MathEditTarget,
        anchor: &MathTextPosition,
        focus: &MathTextPosition,
        composition: bool,
    ) -> Option<Vec<PageSelectionRect>> {
        for projection in self.math_projections_for_target(target) {
            let Some((layout, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            let geometry = MathTextItemGeometry {
                object_x_pt: projection.object_x_pt,
                object_baseline_y_pt: projection.baseline_y_pt,
                projection,
                items: &layout.items,
                sidecars,
            };
            if let Some(rectangles) =
                math_text_selection_in_items(&geometry, target, anchor, focus, composition)
            {
                return Some(
                    rectangles
                        .into_iter()
                        .filter_map(|mut rect| {
                            (rect.y0_pt, rect.y1_pt) = projection.clip_range_for_row(
                                rect.y0_pt,
                                rect.y1_pt,
                                self.math_text_row_id(target, anchor),
                            )?;
                            Some(rect)
                        })
                        .collect(),
                );
            }
        }
        None
    }

    /// 在同一公式文本岛内按 Parley 视觉顺序移动数学光标。
    pub fn move_math_text_visual(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        forward: bool,
    ) -> Option<(MathTextPosition, VisualAffinity)> {
        self.move_math_text_cursor(
            target,
            position,
            visual_affinity,
            MathTextMoveRequest::Visual(forward),
        )
        .map(|(position, affinity, _)| (position, affinity))
    }

    /// 在同一公式文本岛内按保留的目标 x 上下移动数学光标。
    pub fn move_math_text_vertical(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        line_delta: isize,
        preferred_x: Option<f32>,
    ) -> Option<(MathTextPosition, VisualAffinity, f32)> {
        let (position, affinity, preferred_x) = self.move_math_text_cursor(
            target,
            position,
            visual_affinity,
            MathTextMoveRequest::Vertical(line_delta, preferred_x),
        )?;
        Some((position, affinity, preferred_x?))
    }

    pub(crate) fn adjacent_math_text_hit(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        line_delta: isize,
        preferred_x: Option<f32>,
    ) -> Option<PageMathTextHit> {
        if line_delta == 0 {
            return None;
        }
        let current = self.math_text_caret_geometry(target, position, visual_affinity)?;
        let current_host = self.math_text_host(target, position)?;
        let (current_path, current_offset) =
            MathTreePosition::sequence_boundary_from_text_path(position.path.steps()).ok()?;
        let preferred_x = preferred_x.unwrap_or(current.x_pt as f32);
        if !preferred_x.is_finite() {
            return None;
        }
        let current_y = (current.y0_pt + current.y1_pt) / 2.0;
        let mut best: Option<((usize, f64, f64), PageMathTextHit)> = None;
        for projection in self.math_projections_for_target(target) {
            let Some((layout, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            for item in &layout.items {
                let caret_math::layout::MathItem::TextIsland {
                    token,
                    x,
                    baseline_y,
                    ..
                } = item
                else {
                    continue;
                };
                let Some(sidecar) = sidecars.get(token) else {
                    continue;
                };
                if &sidecar.target != target {
                    continue;
                }
                if sidecar.layout.text.is_empty() {
                    let Some(next_position) =
                        sidecar.position_at_offset(0, MathPositionAffinity::Downstream)
                    else {
                        continue;
                    };
                    if !math_navigation::math_text_candidate_is_vertical_neighbor(
                        &current_host,
                        current_path.steps(),
                        current_offset,
                        &sidecar.host,
                        &next_position.path,
                    ) {
                        continue;
                    }
                    let Some((caret, _)) = math_text_caret_in_items(
                        &MathTextItemGeometry {
                            object_x_pt: projection.object_x_pt,
                            object_baseline_y_pt: projection.baseline_y_pt,
                            projection,
                            items: &layout.items,
                            sidecars,
                        },
                        target,
                        &next_position,
                        visual_affinity,
                    ) else {
                        continue;
                    };
                    let Some((clip_top, clip_bottom)) = projection.clip_range_for_row(
                        caret.y0_pt,
                        caret.y1_pt,
                        math_row_id_for_host(&sidecar.host),
                    ) else {
                        continue;
                    };
                    let center_y = (clip_top + clip_bottom) / 2.0;
                    let is_ahead = if line_delta > 0 {
                        (projection.page_index, center_y) > (current.page_index, current_y)
                    } else {
                        (projection.page_index, center_y) < (current.page_index, current_y)
                    };
                    let key = (
                        projection.page_index.abs_diff(current.page_index),
                        if projection.page_index == current.page_index {
                            (center_y - current_y).abs()
                        } else {
                            0.0
                        },
                        (caret.x_pt - f64::from(preferred_x)).abs(),
                    );
                    if !is_ahead
                        || sidecar
                            .position_at_offset(0, MathPositionAffinity::Downstream)
                            .as_ref()
                            == Some(position)
                        || best.as_ref().is_some_and(|(best_key, _)| key >= *best_key)
                    {
                        continue;
                    }
                    best = Some((
                        key,
                        PageMathTextHit {
                            token: *token,
                            session_id: self.session_id.clone(),
                            document_generation: self.document_generation,
                            font_generation: self.font_generation,
                            target: target.clone(),
                            host: sidecar.host.clone(),
                            position: next_position,
                            visual_affinity,
                            is_zero_width: true,
                            caret: PageCaretGeometry {
                                page_index: projection.page_index,
                                x_pt: caret.x_pt,
                                y0_pt: clip_top,
                                y1_pt: clip_bottom,
                            },
                        },
                    ));
                    continue;
                }
                let Some(first_line) = sidecar.layout.layout.lines().next() else {
                    continue;
                };
                let island_x = projection.object_x_pt + *x;
                let island_baseline = projection.baseline_y_pt + *baseline_y;
                for line in sidecar.layout.layout.lines() {
                    let metrics = line.metrics();
                    let local_y = (metrics.block_min_coord + metrics.block_max_coord) / 2.0;
                    let local_x = (f64::from(preferred_x) - island_x) as f32;
                    let Some(hit) = sidecar.layout.cursor_from_point(local_x, local_y) else {
                        continue;
                    };
                    let logical_affinity = match hit.logical_affinity {
                        HitAffinity::Upstream => MathPositionAffinity::Upstream,
                        HitAffinity::Downstream => MathPositionAffinity::Downstream,
                    };
                    let Some(next_position) =
                        sidecar.position_at_offset(hit.cursor.byte_index(), logical_affinity)
                    else {
                        continue;
                    };
                    if !math_navigation::math_text_candidate_is_vertical_neighbor(
                        &current_host,
                        current_path.steps(),
                        current_offset,
                        &sidecar.host,
                        &next_position.path,
                    ) {
                        continue;
                    }
                    let Some((bounds, line_index)) =
                        sidecar.layout.cursor_geometry_with_line(hit.cursor, 0.0)
                    else {
                        continue;
                    };
                    let Some(selected_line) = sidecar.layout.layout.get(line_index) else {
                        continue;
                    };
                    let selected_baseline = island_baseline
                        + f64::from(
                            selected_line.metrics().baseline - first_line.metrics().baseline,
                        );
                    let caret =
                        projection.caret_at_math_baseline(island_x + bounds.x0, selected_baseline);
                    let Some((clip_top, clip_bottom)) = projection.clip_range_for_row(
                        caret.y0_pt,
                        caret.y1_pt,
                        math_row_id_for_host(&sidecar.host),
                    ) else {
                        continue;
                    };
                    let center_y = (clip_top + clip_bottom) / 2.0;
                    let is_forward = line_delta > 0;
                    let is_ahead = if is_forward {
                        (projection.page_index, center_y) > (current.page_index, current_y)
                    } else {
                        (projection.page_index, center_y) < (current.page_index, current_y)
                    };
                    if !is_ahead {
                        continue;
                    }
                    let key = (
                        projection.page_index.abs_diff(current.page_index),
                        if projection.page_index == current.page_index {
                            (center_y - current_y).abs()
                        } else {
                            0.0
                        },
                        (caret.x_pt - f64::from(preferred_x)).abs(),
                    );
                    if sidecar
                        .position_at_offset(hit.cursor.byte_index(), logical_affinity)
                        .as_ref()
                        == Some(position)
                        || best.as_ref().is_some_and(|(best_key, _)| key >= *best_key)
                    {
                        continue;
                    }
                    let candidate = PageMathTextHit {
                        token: *token,
                        session_id: self.session_id.clone(),
                        document_generation: self.document_generation,
                        font_generation: self.font_generation,
                        target: target.clone(),
                        host: sidecar.host.clone(),
                        position: next_position,
                        visual_affinity: hit.cursor.affinity(),
                        is_zero_width: sidecar.layout.metrics.advance_pt <= 0.0,
                        caret: PageCaretGeometry {
                            page_index: projection.page_index,
                            x_pt: caret.x_pt,
                            y0_pt: clip_top,
                            y1_pt: clip_bottom,
                        },
                    };
                    best = Some((key, candidate));
                }
            }
        }
        best.map(|(_, hit)| hit)
    }

    /// 将同一公式文本岛内的数学光标移到视觉行首尾。
    pub fn move_math_text_line_edge(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        end: bool,
    ) -> Option<(MathTextPosition, VisualAffinity)> {
        self.move_math_text_cursor(
            target,
            position,
            visual_affinity,
            MathTextMoveRequest::LineEdge(end),
        )
        .map(|(position, affinity, _)| (position, affinity))
    }

    fn move_math_text_cursor(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        movement: MathTextMoveRequest,
    ) -> Option<(MathTextPosition, VisualAffinity, Option<f32>)> {
        for sidecar in self
            .math_block_text_layouts
            .values()
            .flat_map(HashMap::values)
            .chain(
                self.paragraphs
                    .iter()
                    .flat_map(|paragraph| paragraph.inline_math_text_layouts.iter())
                    .flat_map(HashMap::values),
            )
        {
            if &sidecar.target != target {
                continue;
            }
            let Some(offset) = sidecar.offset_at_position(position) else {
                continue;
            };
            let Some(cursor) = sidecar.layout.cursor_at(offset, visual_affinity) else {
                continue;
            };
            let vertical_origin_x = if matches!(movement, MathTextMoveRequest::Vertical(_, _)) {
                let local_x = sidecar.layout.cursor_geometry(cursor, 0.0)?.x0;
                let page_x = self
                    .math_text_caret_geometry(target, position, visual_affinity)?
                    .x_pt;
                Some((page_x - local_x) as f32)
            } else {
                None
            };
            let Some((next_cursor, preferred_x)) = (match movement {
                MathTextMoveRequest::Visual(forward) => sidecar
                    .layout
                    .move_visual(cursor, forward)
                    .map(|cursor| (cursor, None)),
                MathTextMoveRequest::Vertical(line_delta, preferred_x) => sidecar
                    .layout
                    .move_vertical(
                        cursor,
                        line_delta,
                        preferred_x.map(|x| x - vertical_origin_x.unwrap_or(0.0)),
                    )
                    .map(|(cursor, preferred_x)| {
                        (cursor, Some(preferred_x + vertical_origin_x.unwrap_or(0.0)))
                    }),
                MathTextMoveRequest::LineEdge(end) => sidecar
                    .layout
                    .move_to_visual_line_edge(cursor, end)
                    .map(|cursor| (cursor, None)),
            }) else {
                continue;
            };
            let logical_affinity = match sidecar
                .layout
                .logical_affinity_at(next_cursor.byte_index(), next_cursor.affinity())?
            {
                HitAffinity::Upstream => MathPositionAffinity::Upstream,
                HitAffinity::Downstream => MathPositionAffinity::Downstream,
            };
            let next_position =
                sidecar.position_at_offset(next_cursor.byte_index(), logical_affinity)?;
            return Some((next_position, next_cursor.affinity(), preferred_x));
        }
        None
    }
}
