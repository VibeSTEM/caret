//! 计算行框高度、基线位置以及固定行距下的溢出状态。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::DocumentUnits;
use std::ops::Range;

/// 有限的行内字形 ink bounds，坐标相对其所属基线，单位为 pt。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InkBounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// 独立于文本占位字形的行内对象固有度量。
#[derive(Debug, Clone, PartialEq)]
pub struct InlineObjectMetrics {
    pub source_bytes: Range<usize>,
    pub advance_pt: f32,
    pub ascent_pt: f32,
    pub descent_pt: f32,
    pub ink_bounds: InkBounds,
}

/// 行距的持久化语义；长度用百分之一 pt 表示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineSpacing {
    Natural,
    Multiple { percent: u16 },
    AtLeast { min_centipoints: u32 },
    Exactly { centipoints: u32 },
}

/// 行内所有塑形字体的最大字体度量。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontLineMetrics {
    pub ascent_pt: f32,
    pub descent_pt: f32,
    pub leading_pt: f32,
}

/// 已解析的行框与 baseline 度量。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMetrics {
    pub height_pt: f32,
    pub baseline_pt: f32,
    pub content_metrics_overflow_line_box: bool,
    pub object_ink_overflows_line_box: bool,
    /// 绘制器应把 ink 的垂直范围裁切到相对行顶的 `0..height_pt`；不裁左右悬垂。
    /// Exactly 保持固定行推进，并使用这个垂直裁切区间。
    pub clip_ink_to_line_height: bool,
}

/// 行度量无效时拒绝构造布局结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineMetricsError {
    InvalidFontMetrics,
    InvalidObjectMetrics,
    InvalidLineSpacing,
}

/// 把文档行距转换成共享网格使用的行距语义。
pub fn row_spacing_for_line_spacing(spacing: LineSpacing) -> caret_grid_layout::RowSpacing {
    match spacing {
        LineSpacing::Natural => caret_grid_layout::RowSpacing::Natural,
        LineSpacing::Multiple { percent } => caret_grid_layout::RowSpacing::Multiple {
            percent: f64::from(percent),
        },
        LineSpacing::AtLeast { min_centipoints } => caret_grid_layout::RowSpacing::AtLeast {
            minimum_height: DocumentUnits::centipoints_to_points(min_centipoints),
        },
        LineSpacing::Exactly { centipoints } => caret_grid_layout::RowSpacing::Exactly {
            height: DocumentUnits::centipoints_to_points(centipoints),
        },
    }
}

/// 为需与正文基线一致的结构化行内容解析行距；固定行距维持裁切，其他策略从已决正文行框扩展。
pub fn row_spacing_for_content_line_spacing(
    spacing: LineSpacing,
    body_height_pt: f32,
    body_baseline_pt: f32,
) -> Result<caret_grid_layout::RowSpacing, LineMetricsError> {
    if !body_height_pt.is_finite() || body_height_pt <= 0.0 {
        return Err(LineMetricsError::InvalidFontMetrics);
    }
    match spacing {
        LineSpacing::Exactly { .. } => Ok(row_spacing_for_line_spacing(spacing)),
        _ if !body_baseline_pt.is_finite() || body_baseline_pt < 0.0 => {
            Err(LineMetricsError::InvalidFontMetrics)
        }
        _ => Ok(caret_grid_layout::RowSpacing::ExpandToFit {
            base_height: f64::from(body_height_pt),
            base_baseline: f64::from(body_baseline_pt),
        }),
    }
}

impl std::fmt::Display for LineMetricsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidFontMetrics => "layout.invalid_font_metrics",
            Self::InvalidObjectMetrics => "layout.invalid_object_metrics",
            Self::InvalidLineSpacing => "layout.invalid_line_spacing",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LineMetricsError {}

/// 根据文字、对象及行距语义计算行框和基线，不将对象高度按倍数缩小。
pub fn calculate_line_metrics(
    fonts: &[FontLineMetrics],
    objects: &[InlineObjectMetrics],
    empty_mark: FontLineMetrics,
    spacing: LineSpacing,
) -> Result<LineMetrics, LineMetricsError> {
    for font in fonts {
        validate_font_metrics(*font)?;
    }
    let line_grid = if fonts.is_empty() {
        empty_mark
    } else {
        FontLineMetrics {
            ascent_pt: fonts.iter().map(|font| font.ascent_pt).fold(0.0, f32::max),
            descent_pt: fonts.iter().map(|font| font.descent_pt).fold(0.0, f32::max),
            leading_pt: fonts.iter().map(|font| font.leading_pt).fold(0.0, f32::max),
        }
    };
    calculate_line_metrics_with_grid(line_grid, &[], objects, spacing)
}

/// 将基础行距网格与需要扩展行框的字体度量分开计算。
pub fn calculate_line_metrics_with_grid(
    line_grid: FontLineMetrics,
    fonts: &[FontLineMetrics],
    objects: &[InlineObjectMetrics],
    spacing: LineSpacing,
) -> Result<LineMetrics, LineMetricsError> {
    let base = calculate_line_metrics_with_grid_inner(line_grid, fonts, &[], spacing)?;
    let mut metrics = calculate_line_metrics_with_grid_inner(line_grid, fonts, objects, spacing)?;
    if !objects.is_empty() && !matches!(spacing, LineSpacing::Exactly { .. }) {
        let object_ascent = objects
            .iter()
            .map(|object| object.ascent_pt)
            .fold(0.0_f32, f32::max);
        let object_descent = objects
            .iter()
            .map(|object| object.descent_pt)
            .fold(0.0_f32, f32::max);
        let extra_top = (object_ascent - base.baseline_pt).max(0.0);
        let extra_bottom = (object_descent - (base.height_pt - base.baseline_pt)).max(0.0);
        metrics.height_pt = metrics
            .height_pt
            .max(base.height_pt + extra_top + extra_bottom);
        metrics.baseline_pt = base.baseline_pt + extra_top;
        metrics.content_metrics_overflow_line_box = false;
    }
    if !metrics.height_pt.is_finite()
        || metrics.height_pt <= 0.0
        || !metrics.baseline_pt.is_finite()
    {
        return Err(LineMetricsError::InvalidFontMetrics);
    }
    metrics.object_ink_overflows_line_box = objects.iter().any(|object| {
        object.ink_bounds.top < -metrics.baseline_pt
            || object.ink_bounds.bottom > metrics.height_pt - metrics.baseline_pt
    });
    Ok(metrics)
}

/// 用正文行框为结构化内容计算宿主行框，不把内容的有符号基线度量当作字体或文本对象。
pub fn calculate_content_line_metrics_with_grid(
    line_grid: FontLineMetrics,
    content_ascent: f32,
    content_descent: f32,
    spacing: LineSpacing,
) -> Result<LineMetrics, LineMetricsError> {
    if !content_ascent.is_finite() || !content_descent.is_finite() {
        return Err(LineMetricsError::InvalidObjectMetrics);
    }
    let base = calculate_line_metrics_with_grid_inner(line_grid, &[], &[], spacing)?;
    let row_spacing = match spacing {
        LineSpacing::Exactly { .. } => row_spacing_for_line_spacing(spacing),
        _ => caret_grid_layout::RowSpacing::ExpandToFit {
            base_height: f64::from(base.height_pt),
            base_baseline: f64::from(base.baseline_pt),
        },
    };
    let grid = caret_grid_layout::layout_grid(
        &[caret_grid_layout::GridRow::Cells(vec![
            caret_grid_layout::CellMetrics {
                width: 0.0,
                ascent: f64::from(content_ascent),
                descent: f64::from(content_descent),
            },
        ])],
        &caret_grid_layout::GridSpec {
            column_widths: vec![None],
            column_alignments: vec![caret_grid_layout::HorizontalAlignment::Center],
            vertical_rules: Vec::new(),
            horizontal_rules: Vec::new(),
            column_gap: 0.0,
            row_gap: 0.0,
            minimum_row_ascent: 0.0,
            minimum_row_descent: 0.0,
            row_spacing: Some(row_spacing),
            base_row_ascent: f64::from(line_grid.ascent_pt),
            base_row_descent: f64::from(line_grid.descent_pt),
            base_row_leading: f64::from(line_grid.leading_pt),
        },
    )
    .map_err(|_| LineMetricsError::InvalidObjectMetrics)?;
    let row = grid
        .rows
        .first()
        .ok_or(LineMetricsError::InvalidObjectMetrics)?;
    let height = row.ascent + row.descent;
    let baseline = row.ascent;
    let content_overflow = row.content_overflow;
    let ink_overflow =
        f64::from(content_ascent) > baseline || f64::from(content_descent) > height - baseline;
    let height_pt = height as f32;
    let baseline_pt = baseline as f32;
    if !height_pt.is_finite() || !baseline_pt.is_finite() {
        return Err(LineMetricsError::InvalidFontMetrics);
    }
    Ok(LineMetrics {
        height_pt,
        baseline_pt,
        content_metrics_overflow_line_box: content_overflow,
        object_ink_overflows_line_box: ink_overflow,
        clip_ink_to_line_height: row.clip_content,
    })
}

fn calculate_line_metrics_with_grid_inner(
    line_grid: FontLineMetrics,
    fonts: &[FontLineMetrics],
    objects: &[InlineObjectMetrics],
    spacing: LineSpacing,
) -> Result<LineMetrics, LineMetricsError> {
    validate_font_metrics(line_grid)?;
    validate_line_spacing(spacing)?;
    let mut content_metrics = line_grid;
    for font in fonts {
        validate_font_metrics(*font)?;
        content_metrics.ascent_pt = content_metrics.ascent_pt.max(font.ascent_pt);
        content_metrics.descent_pt = content_metrics.descent_pt.max(font.descent_pt);
        content_metrics.leading_pt = content_metrics.leading_pt.max(font.leading_pt);
    }
    if objects.iter().any(|object| {
        object.source_bytes.start >= object.source_bytes.end
            || !object.advance_pt.is_finite()
            || !object.ascent_pt.is_finite()
            || !object.descent_pt.is_finite()
            || object.advance_pt < 0.0
            || object.ascent_pt < 0.0
            || object.descent_pt < 0.0
            || !valid_ink_bounds(object.ink_bounds)
    }) {
        return Err(LineMetricsError::InvalidObjectMetrics);
    }
    let ascent_pt = content_metrics.ascent_pt.max(
        objects
            .iter()
            .map(|object| object.ascent_pt)
            .fold(0.0, f32::max),
    );
    let descent_pt = content_metrics.descent_pt.max(
        objects
            .iter()
            .map(|object| object.descent_pt)
            .fold(0.0, f32::max),
    );
    let row_spacing = row_spacing_for_line_spacing(spacing);
    let frame = caret_grid_layout::calculate_row_frame_with_content_leading(
        caret_grid_layout::RowMetrics {
            ascent: f64::from(line_grid.ascent_pt),
            descent: f64::from(line_grid.descent_pt),
            leading: f64::from(line_grid.leading_pt),
        },
        caret_grid_layout::RowMetrics {
            ascent: f64::from(ascent_pt),
            descent: f64::from(descent_pt),
            leading: f64::from(content_metrics.leading_pt),
        },
        !objects.is_empty(),
        row_spacing,
    )
    .map_err(|_| LineMetricsError::InvalidFontMetrics)?;
    let height_pt = frame.height as f32;
    let baseline_pt = frame.baseline as f32;
    if !height_pt.is_finite() || height_pt <= 0.0 || !baseline_pt.is_finite() {
        return Err(LineMetricsError::InvalidFontMetrics);
    }
    Ok(LineMetrics {
        height_pt,
        baseline_pt,
        content_metrics_overflow_line_box: frame.content_overflow,
        object_ink_overflows_line_box: objects.iter().any(|object| {
            object.ink_bounds.top < -baseline_pt
                || object.ink_bounds.bottom > height_pt - baseline_pt
        }),
        clip_ink_to_line_height: frame.clip_content,
    })
}

fn validate_line_spacing(spacing: LineSpacing) -> Result<(), LineMetricsError> {
    let valid = match spacing {
        LineSpacing::Natural => true,
        LineSpacing::Multiple { percent } => percent > 0,
        LineSpacing::AtLeast { min_centipoints }
        | LineSpacing::Exactly {
            centipoints: min_centipoints,
        } => min_centipoints > 0,
    };
    if valid {
        Ok(())
    } else {
        Err(LineMetricsError::InvalidLineSpacing)
    }
}

fn validate_font_metrics(metrics: FontLineMetrics) -> Result<(), LineMetricsError> {
    if !metrics.ascent_pt.is_finite()
        || !metrics.descent_pt.is_finite()
        || !metrics.leading_pt.is_finite()
        || metrics.ascent_pt < 0.0
        || metrics.descent_pt < 0.0
        || metrics.leading_pt < 0.0
    {
        return Err(LineMetricsError::InvalidFontMetrics);
    }
    Ok(())
}

fn valid_ink_bounds(bounds: InkBounds) -> bool {
    [bounds.left, bounds.top, bounds.right, bounds.bottom]
        .into_iter()
        .all(f32::is_finite)
        && bounds.left <= bounds.right
        && bounds.top <= bounds.bottom
}

#[cfg(test)]
mod tests;
