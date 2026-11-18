//! 对公式布局项计算命中、光标与选区几何。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;
use crate::layout::shared::math_row_id_for_host;

/// 零推进数学文本保持 caret 几何不变，只扩展命中区的最小页面高度。
const ZERO_ADVANCE_MATH_HIT_HEIGHT_PT: f64 = 2.25;

pub(crate) struct MathTextItemGeometry<'a> {
    pub(crate) object_x_pt: f64,
    pub(crate) object_baseline_y_pt: f64,
    pub(crate) projection: &'a MathPageProjection,
    pub(crate) items: &'a [caret_math::layout::MathItem],
    pub(crate) sidecars: &'a HashMap<u64, MathTextSidecar>,
}

pub(crate) fn math_text_caret_in_items(
    geometry: &MathTextItemGeometry<'_>,
    target: &MathEditTarget,
    position: &MathTextPosition,
    visual_affinity: VisualAffinity,
) -> Option<(PageCaretGeometry, u16)> {
    for item in geometry.items {
        let caret_math::layout::MathItem::TextIsland {
            token,
            x,
            baseline_y,
            ..
        } = item
        else {
            continue;
        };
        let Some(sidecar) = geometry.sidecars.get(token) else {
            continue;
        };
        if &sidecar.target != target {
            continue;
        }
        let Some(offset) = sidecar.offset_at_position(position) else {
            continue;
        };
        let island_baseline_y_pt = geometry.object_baseline_y_pt + baseline_y;
        let (caret_x_pt, text_baseline_y_pt) = if sidecar.layout.text.is_empty() {
            sidecar.empty_caret_metrics?;
            (0.0, island_baseline_y_pt)
        } else {
            let (bounds, line_index) = sidecar.layout.cursor_geometry_with_line(
                sidecar.layout.cursor_at(offset, visual_affinity)?,
                0.0,
            )?;
            let first_baseline = sidecar.layout.layout.lines().next()?.metrics().baseline;
            let line_baseline = sidecar.layout.layout.get(line_index)?.metrics().baseline;
            (
                bounds.x0,
                island_baseline_y_pt + f64::from(line_baseline - first_baseline),
            )
        };
        let island_x_pt = geometry.object_x_pt + *x + caret_x_pt;
        let caret = geometry
            .projection
            .caret_at_math_baseline(island_x_pt, text_baseline_y_pt);
        return Some((
            caret,
            sidecar.font_size_centipoints_at(offset, visual_affinity),
        ));
    }
    None
}

pub(super) fn math_text_selection_in_items(
    geometry: &MathTextItemGeometry<'_>,
    target: &MathEditTarget,
    anchor: &MathTextPosition,
    focus: &MathTextPosition,
    composition: bool,
) -> Option<Vec<PageSelectionRect>> {
    for item in geometry.items {
        let caret_math::layout::MathItem::TextIsland {
            token,
            x,
            baseline_y,
            ..
        } = item
        else {
            continue;
        };
        let Some(sidecar) = geometry.sidecars.get(token) else {
            continue;
        };
        if &sidecar.target != target {
            continue;
        }
        let Some(anchor_offset) = sidecar.offset_at_position(anchor) else {
            continue;
        };
        let Some(focus_offset) = sidecar.offset_at_position(focus) else {
            continue;
        };
        let range = anchor_offset.min(focus_offset)..anchor_offset.max(focus_offset);
        let rectangles = if composition {
            sidecar.layout.composition_geometry(range)?
        } else {
            sidecar.layout.selection_geometry(range)?
        };
        if rectangles.is_empty() {
            return Some(Vec::new());
        }
        let line_baseline = sidecar.layout.layout.lines().next()?.metrics().baseline;
        let island_x_pt = geometry.object_x_pt + *x;
        let island_baseline_y_pt = geometry.object_baseline_y_pt + baseline_y;
        return Some(
            rectangles
                .into_iter()
                .map(|(bounds, _line_index)| PageSelectionRect {
                    page_index: geometry.projection.page_index,
                    x0_pt: island_x_pt + bounds.x0,
                    y0_pt: island_baseline_y_pt + bounds.y0 - f64::from(line_baseline),
                    x1_pt: island_x_pt + bounds.x1,
                    y1_pt: island_baseline_y_pt + bounds.y1 - f64::from(line_baseline),
                })
                .collect(),
        );
    }
    None
}

pub(super) fn hit_math_text_items(
    page_index: usize,
    page_x_pt: f64,
    page_y_pt: f64,
    geometry: &MathTextItemGeometry<'_>,
) -> Option<PageMathTextHit> {
    let mut zero_width_hit = None;
    for item in geometry.items {
        let caret_math::layout::MathItem::TextIsland {
            token,
            x,
            baseline_y,
            ..
        } = item
        else {
            continue;
        };
        let Some(sidecar) = geometry.sidecars.get(token) else {
            continue;
        };
        if !geometry
            .projection
            .contains_y_for_row(page_y_pt, math_row_id_for_host(&sidecar.host))
        {
            continue;
        }
        let island_x_pt = geometry.object_x_pt + x;
        let island_baseline_y_pt = geometry.object_baseline_y_pt + baseline_y;
        if sidecar.layout.text.is_empty() {
            let Some((ascent_pt, descent_pt)) = sidecar.empty_caret_metrics else {
                continue;
            };
            let island_y0_pt = island_baseline_y_pt - f64::from(ascent_pt);
            let island_y1_pt = island_baseline_y_pt + f64::from(descent_pt);
            let (hit_y0_pt, hit_y1_pt) = expand_zero_advance_hit_y(island_y0_pt, island_y1_pt);
            if (page_x_pt - island_x_pt).abs() > 4.0
                || page_y_pt < hit_y0_pt
                || page_y_pt > hit_y1_pt
            {
                continue;
            }
            let Some(position) = sidecar.position_at_offset(0, MathPositionAffinity::Downstream)
            else {
                continue;
            };
            zero_width_hit = Some(PageMathTextHit {
                token: *token,
                session_id: None,
                document_generation: 0,
                font_generation: 0,
                target: sidecar.target.clone(),
                host: sidecar.host.clone(),
                position,
                visual_affinity: VisualAffinity::Downstream,
                is_zero_width: true,
                caret: PageCaretGeometry {
                    page_index,
                    x_pt: island_x_pt,
                    y0_pt: island_y0_pt,
                    y1_pt: island_y1_pt,
                },
            });
            continue;
        }
        let Some(line) = sidecar.layout.layout.lines().next() else {
            continue;
        };
        let metrics = sidecar.layout.metrics;
        if metrics.ascent_pt + metrics.descent_pt <= 0.0 {
            continue;
        }
        let island_y0_pt = island_baseline_y_pt - f64::from(metrics.ascent_pt);
        let island_y1_pt = island_baseline_y_pt + f64::from(metrics.descent_pt);
        let zero_advance = metrics.advance_pt <= 0.0;
        let (hit_y0_pt, hit_y1_pt) = if zero_advance {
            expand_zero_advance_hit_y(island_y0_pt, island_y1_pt)
        } else {
            (island_y0_pt, island_y1_pt)
        };
        let outside_x = if zero_advance {
            (page_x_pt - island_x_pt).abs() > 4.0
        } else {
            page_x_pt < island_x_pt || page_x_pt > island_x_pt + f64::from(metrics.advance_pt)
        };
        if !island_x_pt.is_finite()
            || !island_baseline_y_pt.is_finite()
            || outside_x
            || page_y_pt < hit_y0_pt
            || page_y_pt > hit_y1_pt
        {
            continue;
        }
        let local_x = if zero_advance {
            0.0
        } else {
            (page_x_pt - island_x_pt) as f32
        };
        let local_y =
            (page_y_pt - island_baseline_y_pt + f64::from(line.metrics().baseline)) as f32;
        let Some(hit) = sidecar.layout.cursor_from_point(local_x, local_y) else {
            continue;
        };
        let affinity = match hit.logical_affinity {
            HitAffinity::Upstream => MathPositionAffinity::Upstream,
            HitAffinity::Downstream => MathPositionAffinity::Downstream,
        };
        let Some(position) = sidecar.position_at_offset(hit.cursor.byte_index(), affinity) else {
            continue;
        };
        let Some(caret_bounds) = sidecar.layout.cursor_geometry(hit.cursor, 0.0) else {
            continue;
        };
        let text_hit = PageMathTextHit {
            token: *token,
            session_id: None,
            document_generation: 0,
            font_generation: 0,
            target: sidecar.target.clone(),
            host: sidecar.host.clone(),
            position,
            visual_affinity: hit.cursor.affinity(),
            is_zero_width: zero_advance,
            caret: PageCaretGeometry {
                page_index,
                x_pt: island_x_pt + caret_bounds.x0,
                y0_pt: island_baseline_y_pt + caret_bounds.y0 - f64::from(line.metrics().baseline),
                y1_pt: island_baseline_y_pt + caret_bounds.y1 - f64::from(line.metrics().baseline),
            },
        };
        if zero_advance {
            zero_width_hit = Some(text_hit);
        } else {
            return Some(text_hit);
        }
    }
    zero_width_hit
}

fn expand_zero_advance_hit_y(y0_pt: f64, y1_pt: f64) -> (f64, f64) {
    let extra = (ZERO_ADVANCE_MATH_HIT_HEIGHT_PT - (y1_pt - y0_pt)).max(0.0) / 2.0;
    (y0_pt - extra, y1_pt + extra)
}
