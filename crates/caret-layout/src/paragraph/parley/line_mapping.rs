//! 将 Parley 视觉行映射为分页行框和基线偏移。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use parley::{InlineBox, InlineBoxKind, Layout, PositionedLayoutItem};
use std::ops::Range;

use crate::{
    FontLineMetrics, InkBounds, InlineObjectMetrics, LineMetricsError, LineSpacing,
    ParagraphLayout, ParagraphLayoutError, ParagraphLine, PositionedInlineObject,
    calculate_line_metrics_with_grid,
};

pub(super) fn font_line_metrics(metrics: parley::FontMetrics) -> FontLineMetrics {
    FontLineMetrics {
        ascent_pt: metrics.ascent,
        descent_pt: metrics.descent,
        leading_pt: metrics.leading,
    }
}

pub(super) struct ParagraphLineMapping {
    pub(super) paragraph: ParagraphLayout,
    pub(super) line_top_offsets_pt: Vec<f32>,
    pub(super) line_box_tops_pt: Vec<f32>,
    pub(super) parley_line_tops_pt: Vec<f32>,
    pub(super) parley_line_heights_pt: Vec<f32>,
    pub(super) baseline_adjustments_pt: Vec<f32>,
    pub(super) positioned_inline_objects: Vec<PositionedInlineObject>,
}

pub(super) fn make_paragraph_lines(
    text: &str,
    layout: &Layout<[u8; 4]>,
    spacing: LineSpacing,
    line_grid_metrics: FontLineMetrics,
    line_grid_font_size_centipoints: u16,
    inline_boxes: &[InlineBox],
) -> Result<ParagraphLineMapping, ParagraphLayoutError> {
    let line_grid = calculate_line_metrics_with_grid(line_grid_metrics, &[], &[], spacing)?;
    if text.is_empty() {
        // 空段落与有文字的段落共用行网格，避免输入前后行高发生跳变。
        let metrics = line_grid;
        return Ok(ParagraphLineMapping {
            paragraph: ParagraphLayout {
                lines: vec![ParagraphLine {
                    source_bytes: 0..0,
                    consumed_bytes: 0..0,
                    advance_pt: 0.0,
                    overflows_width: false,
                    metrics,
                }],
                total_height_pt: metrics.height_pt,
            },
            line_top_offsets_pt: vec![0.0],
            line_box_tops_pt: vec![0.0],
            parley_line_tops_pt: vec![0.0],
            parley_line_heights_pt: vec![0.0],
            baseline_adjustments_pt: vec![0.0],
            positioned_inline_objects: Vec::new(),
        });
    }
    let lines: Vec<_> = layout.lines().collect();
    if lines.is_empty() {
        return Err(ParagraphLayoutError::InconsistentShaping);
    }
    let inline_box_by_id = inline_boxes
        .iter()
        .map(|inline_box| (inline_box.id, inline_box))
        .collect::<std::collections::HashMap<_, _>>();
    let mut paragraph_lines = Vec::with_capacity(lines.len());
    let mut line_top_offsets_pt = Vec::with_capacity(lines.len());
    let mut line_box_tops_pt = Vec::with_capacity(lines.len());
    let mut parley_line_tops_pt = Vec::with_capacity(lines.len());
    let mut parley_line_heights_pt = Vec::with_capacity(lines.len());
    let mut baseline_adjustments_pt = Vec::with_capacity(lines.len());
    let mut positioned_inline_objects = Vec::new();
    let mut font_metrics = Vec::new();
    let mut object_metrics = Vec::new();
    let line_grid_font_size_pt = f32::from(line_grid_font_size_centipoints) / 100.0;
    let mut top_pt = 0.0f32;
    for (index, line) in lines.iter().enumerate() {
        let metrics = line.metrics();
        let source_bytes = line.text_range();
        if source_bytes.start > source_bytes.end
            || source_bytes.end > text.len()
            || !text.is_char_boundary(source_bytes.start)
            || !text.is_char_boundary(source_bytes.end)
            || !metrics.advance.is_finite()
            || !metrics.line_height.is_finite()
            || metrics.line_height <= 0.0
        {
            return Err(ParagraphLayoutError::InconsistentShaping);
        }
        let consumed_end = lines
            .get(index + 1)
            .map_or(text.len(), |next| next.text_range().start);
        if consumed_end < source_bytes.end || consumed_end > text.len() {
            return Err(ParagraphLayoutError::InconsistentShaping);
        }
        font_metrics.clear();
        font_metrics.extend(
            line.runs()
                .filter(|run| run.font_size() > line_grid_font_size_pt)
                .map(|run| font_line_metrics(*run.font_metrics())),
        );
        object_metrics.clear();
        let mut line_inline_objects = Vec::new();
        if !inline_box_by_id.is_empty() {
            for item in line.items() {
                let PositionedLayoutItem::InlineBox(positioned) = item else {
                    continue;
                };
                if positioned.kind != InlineBoxKind::InFlow {
                    continue;
                }
                let Some(source) = inline_box_by_id.get(&positioned.id) else {
                    return Err(ParagraphLayoutError::InconsistentShaping);
                };
                let ascent_pt = positioned.baseline.unwrap_or(positioned.height);
                if !ascent_pt.is_finite() || ascent_pt < 0.0 || ascent_pt > positioned.height {
                    return Err(ParagraphLayoutError::InvalidInlineBox);
                }
                let descent_pt = positioned.height - ascent_pt;
                let Some(source_bytes) = inline_box_source_range(text, source.index) else {
                    return Err(ParagraphLayoutError::InvalidInlineBox);
                };
                object_metrics.push(InlineObjectMetrics {
                    source_bytes: source_bytes.clone(),
                    advance_pt: positioned.width,
                    ascent_pt,
                    descent_pt,
                    ink_bounds: InkBounds {
                        left: 0.0,
                        top: -ascent_pt,
                        right: positioned.width,
                        bottom: descent_pt,
                    },
                });
                line_inline_objects.push((
                    positioned.id,
                    source_bytes,
                    positioned.x,
                    positioned.width,
                    positioned.height,
                    ascent_pt,
                    positioned.y,
                ));
            }
        }
        let line_metrics = calculate_line_metrics_with_grid(
            line_grid_metrics,
            &font_metrics,
            &object_metrics,
            spacing,
        )
        .map_err(|error| match error {
            LineMetricsError::InvalidFontMetrics
            | LineMetricsError::InvalidObjectMetrics
            | LineMetricsError::InvalidLineSpacing => {
                ParagraphLayoutError::InvalidLineMetrics(error)
            }
        })?;
        let raw_top = metrics.block_min_coord;
        let raw_baseline_from_top = metrics.baseline - raw_top;
        if !raw_top.is_finite() || !raw_baseline_from_top.is_finite() {
            return Err(ParagraphLayoutError::InconsistentShaping);
        }
        line_top_offsets_pt.push(top_pt - raw_top);
        line_box_tops_pt.push(top_pt);
        parley_line_tops_pt.push(raw_top);
        parley_line_heights_pt.push(metrics.line_height);
        let baseline_adjustment_pt = line_metrics.baseline_pt - raw_baseline_from_top;
        baseline_adjustments_pt.push(baseline_adjustment_pt);
        for (id, source_bytes, x_pt, width_pt, height_pt, baseline_pt, parley_y_pt) in
            line_inline_objects
        {
            let positioned = PositionedInlineObject {
                id,
                source_bytes,
                line_index: index,
                x_pt,
                // 当前盒子采用基线对齐；从 Caret 最终行基线推导行局部 y，避免相减两个较大的
                // 段落级 Parley f32 坐标而损失精度。
                y_pt: line_metrics.baseline_pt - baseline_pt,
                width_pt,
                height_pt,
                baseline_pt,
            };
            if [
                positioned.x_pt,
                positioned.y_pt,
                positioned.width_pt,
                positioned.height_pt,
                positioned.baseline_pt,
                parley_y_pt,
            ]
            .iter()
            .any(|value| !value.is_finite())
            {
                return Err(ParagraphLayoutError::InvalidInlineBox);
            }
            positioned_inline_objects.push(positioned);
        }
        paragraph_lines.push(ParagraphLine {
            source_bytes: source_bytes.clone(),
            consumed_bytes: source_bytes.start..consumed_end,
            advance_pt: metrics.advance,
            overflows_width: metrics.advance > layout.layout_max_advance(),
            metrics: line_metrics,
        });
        top_pt += line_metrics.height_pt;
    }
    Ok(ParagraphLineMapping {
        paragraph: ParagraphLayout {
            lines: paragraph_lines,
            total_height_pt: top_pt,
        },
        line_top_offsets_pt,
        line_box_tops_pt,
        parley_line_tops_pt,
        parley_line_heights_pt,
        baseline_adjustments_pt,
        positioned_inline_objects,
    })
}

fn inline_box_source_range(text: &str, index: usize) -> Option<Range<usize>> {
    text.get(index..)?
        .starts_with('\u{fffc}')
        .then_some(index..index + '\u{fffc}'.len_utf8())
}
