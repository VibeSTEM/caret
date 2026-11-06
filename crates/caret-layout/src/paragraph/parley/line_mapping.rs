//! 将 Parley 视觉行映射为分页行框和基线偏移。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::DocumentUnits;
use parley::Layout;

use crate::{
    FontLineMetrics, InkBounds, InlineObjectLayout, InlineObjectMetrics, LineMetricsError,
    LineSpacing, ParagraphLayout, ParagraphLayoutError, ParagraphLine,
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
}

pub(super) fn make_paragraph_lines(
    text: &str,
    layout: &Layout<[u8; 4]>,
    spacing: LineSpacing,
    line_grid_metrics: FontLineMetrics,
    line_grid_font_size_centipoints: u16,
    inline_objects: &[InlineObjectLayout],
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
        });
    }
    let lines: Vec<_> = layout.lines().collect();
    if lines.is_empty() {
        return Err(ParagraphLayoutError::InconsistentShaping);
    }
    let object_lines = lines
        .iter()
        .enumerate()
        .flat_map(|(line_index, line)| {
            line.items().filter_map(move |item| match item {
                parley::PositionedLayoutItem::InlineBox(inline_box) => Some((
                    inline_box.id,
                    inline_objects
                        .iter()
                        .find(|object| object.id == inline_box.id)
                        .map(|object| object.source_bytes.clone())?,
                    line_index,
                )),
                parley::PositionedLayoutItem::GlyphRun(_) => None,
            })
        })
        .collect::<Vec<_>>();
    // Parley 对只含超宽对象的行会给出越界的空范围哨兵。将对象的逻辑源码范围归给该行，
    // 并从相邻视觉行移除占位字节，使源码范围与消费范围保持有序且互不重叠。
    let mut source_ranges = Vec::with_capacity(lines.len());
    for (line_index, line) in lines.iter().enumerate() {
        let raw_range = line.text_range();
        let owned_objects = object_lines
            .iter()
            .filter(|(_, _, object_line)| *object_line == line_index)
            .collect::<Vec<_>>();
        let mut source_range = if raw_range.start > text.len() || raw_range.start > raw_range.end {
            let start = owned_objects
                .iter()
                .map(|(_, range, _)| range.start)
                .min()
                .ok_or(ParagraphLayoutError::InconsistentShaping)?;
            let end = owned_objects
                .iter()
                .map(|(_, range, _)| range.end)
                .max()
                .ok_or(ParagraphLayoutError::InconsistentShaping)?;
            start..end
        } else {
            raw_range
        };
        for (_, object_range, object_line) in &object_lines {
            if *object_line == line_index
                || source_range.end <= object_range.start
                || source_range.start >= object_range.end
            {
                continue;
            }
            if source_range.start < object_range.start {
                source_range.end = object_range.start;
            } else if source_range.end > object_range.end {
                source_range.start = object_range.end;
            } else {
                source_range = if line_index < *object_line {
                    object_range.start..object_range.start
                } else {
                    object_range.end..object_range.end
                };
            }
        }
        if source_range.end > text.len()
            || !text.is_char_boundary(source_range.start)
            || !text.is_char_boundary(source_range.end)
        {
            return Err(ParagraphLayoutError::InconsistentShaping);
        }
        source_ranges.push(source_range);
    }
    let mut paragraph_lines = Vec::with_capacity(lines.len());
    let mut line_top_offsets_pt = Vec::with_capacity(lines.len());
    let mut line_box_tops_pt = Vec::with_capacity(lines.len());
    let mut parley_line_tops_pt = Vec::with_capacity(lines.len());
    let mut parley_line_heights_pt = Vec::with_capacity(lines.len());
    let mut baseline_adjustments_pt = Vec::with_capacity(lines.len());
    let mut font_metrics = Vec::new();
    let line_grid_font_size_pt =
        DocumentUnits::centipoints_to_points(u32::from(line_grid_font_size_centipoints)) as f32;
    let mut top_pt = 0.0f32;
    for (index, line) in lines.iter().enumerate() {
        let metrics = line.metrics();
        let source_bytes = source_ranges[index].clone();
        if !metrics.advance.is_finite()
            || !metrics.line_height.is_finite()
            || metrics.line_height <= 0.0
        {
            return Err(ParagraphLayoutError::InconsistentShaping);
        }
        let consumed_end = source_ranges
            .get(index + 1)
            .map_or(text.len(), |next| next.start);
        if consumed_end < source_bytes.end || consumed_end > text.len() {
            return Err(ParagraphLayoutError::InconsistentShaping);
        }
        font_metrics.clear();
        font_metrics.extend(
            line.runs()
                .filter(|run| run.font_size() > line_grid_font_size_pt)
                .map(|run| font_line_metrics(*run.font_metrics())),
        );
        let line_objects = inline_objects
            .iter()
            .filter(|object| {
                source_bytes.start <= object.source_bytes.start
                    && object.source_bytes.end <= source_bytes.end
            })
            .map(|object| InlineObjectMetrics {
                source_bytes: object.source_bytes.clone(),
                advance_pt: object.advance_pt,
                ascent_pt: object.ascent_pt,
                descent_pt: object.descent_pt,
                ink_bounds: InkBounds {
                    left: 0.0,
                    top: -object.ascent_pt,
                    right: object.advance_pt,
                    bottom: object.descent_pt,
                },
            })
            .collect::<Vec<_>>();
        let line_metrics = calculate_line_metrics_with_grid(
            line_grid_metrics,
            &font_metrics,
            &line_objects,
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
        baseline_adjustments_pt.push(line_metrics.baseline_pt - raw_baseline_from_top);
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
    })
}
