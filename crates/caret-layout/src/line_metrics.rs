//! 计算行框高度、基线位置以及固定行距下的溢出状态。

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
    validate_font_metrics(line_grid)?;
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
    let font_height_pt = content_metrics.ascent_pt + content_metrics.descent_pt;
    let natural_text_height_pt = font_height_pt + content_metrics.leading_pt;
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
    let content_height_pt = ascent_pt + descent_pt;
    let object_height_pt = objects
        .iter()
        .map(|object| object.ascent_pt + object.descent_pt)
        .fold(0.0_f32, f32::max);
    let required_height_pt = content_height_pt
        .max(natural_text_height_pt)
        .max(object_height_pt);
    if !required_height_pt.is_finite() || required_height_pt <= 0.0 {
        return Err(LineMetricsError::InvalidFontMetrics);
    }
    let height_pt = match spacing {
        LineSpacing::Natural => required_height_pt,
        LineSpacing::Multiple { percent } if percent > 0 => {
            let grid_height_pt =
                (line_grid.ascent_pt + line_grid.descent_pt + line_grid.leading_pt)
                    * f32::from(percent)
                    / 100.0;
            let object_floor_pt = if objects.is_empty() {
                0.0
            } else {
                content_height_pt.max(object_height_pt)
            };
            let larger_font_floor_pt = if fonts.is_empty() {
                0.0
            } else {
                required_height_pt
            };
            grid_height_pt
                .max(larger_font_floor_pt)
                .max(object_floor_pt)
        }
        LineSpacing::AtLeast { min_centipoints } => {
            required_height_pt.max(min_centipoints as f32 * 0.01)
        }
        LineSpacing::Exactly { centipoints } if centipoints > 0 => centipoints as f32 * 0.01,
        _ => return Err(LineMetricsError::InvalidLineSpacing),
    };
    if !height_pt.is_finite() || height_pt <= 0.0 {
        return Err(LineMetricsError::InvalidFontMetrics);
    }
    let content_overflows = height_pt < required_height_pt;
    let baseline_pt = if matches!(spacing, LineSpacing::Exactly { .. }) {
        let grid_content_height_pt = line_grid.ascent_pt + line_grid.descent_pt;
        let grid_overflows = height_pt < grid_content_height_pt + line_grid.leading_pt;
        if grid_overflows {
            height_pt - line_grid.descent_pt
        } else {
            line_grid.ascent_pt + (height_pt - grid_content_height_pt).max(0.0) / 2.0
        }
    } else if content_overflows {
        ascent_pt
    } else {
        ascent_pt + (height_pt - content_height_pt).max(0.0) / 2.0
    };
    Ok(LineMetrics {
        height_pt,
        baseline_pt,
        content_metrics_overflow_line_box: content_overflows,
        object_ink_overflows_line_box: objects.iter().any(|object| {
            object.ink_bounds.top < -baseline_pt
                || object.ink_bounds.bottom > height_pt - baseline_pt
        }),
        clip_ink_to_line_height: matches!(spacing, LineSpacing::Exactly { .. }),
    })
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
