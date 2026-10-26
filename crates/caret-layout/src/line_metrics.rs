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

impl From<parley_engine::FontMetrics> for FontLineMetrics {
    fn from(metrics: parley_engine::FontMetrics) -> Self {
        Self {
            ascent_pt: metrics.ascent,
            descent_pt: metrics.descent,
            leading_pt: metrics.leading,
        }
    }
}

/// 已解析的行框与 baseline 度量。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMetrics {
    pub height_pt: f32,
    pub baseline_pt: f32,
    pub content_metrics_overflow_line_box: bool,
    pub object_ink_overflows_line_box: bool,
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
    let fonts = if fonts.is_empty() {
        std::slice::from_ref(&empty_mark)
    } else {
        fonts
    };
    if fonts.iter().any(|metrics| {
        !metrics.ascent_pt.is_finite()
            || !metrics.descent_pt.is_finite()
            || !metrics.leading_pt.is_finite()
            || metrics.ascent_pt < 0.0
            || metrics.descent_pt < 0.0
            || metrics.leading_pt < 0.0
    }) {
        return Err(LineMetricsError::InvalidFontMetrics);
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

    let font_ascent_pt = fonts
        .iter()
        .map(|metrics| metrics.ascent_pt)
        .fold(0.0_f32, f32::max);
    let font_descent_pt = fonts
        .iter()
        .map(|metrics| metrics.descent_pt)
        .fold(0.0_f32, f32::max);
    let leading_pt = fonts
        .iter()
        .map(|metrics| metrics.leading_pt)
        .fold(0.0_f32, f32::max);
    let text_height_pt = font_ascent_pt + font_descent_pt + leading_pt;
    let ascent_pt = font_ascent_pt.max(
        objects
            .iter()
            .map(|object| object.ascent_pt)
            .fold(0.0, f32::max),
    );
    let descent_pt = font_descent_pt.max(
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
    let required_height_pt = content_height_pt.max(text_height_pt).max(object_height_pt);

    let height_pt = match spacing {
        LineSpacing::Natural => required_height_pt,
        LineSpacing::Multiple { percent } if percent > 0 => {
            (text_height_pt * f32::from(percent) / 100.0).max(object_height_pt)
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
    let content_overflows = height_pt < content_height_pt || height_pt < text_height_pt;
    let baseline_pt = if content_overflows {
        ascent_pt
    } else {
        let extra_space_pt = (height_pt - content_height_pt).max(0.0);
        ascent_pt + extra_space_pt / 2.0
    };
    Ok(LineMetrics {
        height_pt,
        baseline_pt,
        content_metrics_overflow_line_box: content_overflows
            || objects
                .iter()
                .any(|object| object.ascent_pt + object.descent_pt > height_pt),
        object_ink_overflows_line_box: objects.iter().any(|object| {
            object.ink_bounds.top < -baseline_pt
                || object.ink_bounds.bottom > height_pt - baseline_pt
        }),
    })
}

fn valid_ink_bounds(bounds: InkBounds) -> bool {
    [bounds.left, bounds.top, bounds.right, bounds.bottom]
        .into_iter()
        .all(f32::is_finite)
        && bounds.left <= bounds.right
        && bounds.top <= bounds.bottom
}

#[cfg(test)]
mod tests {
    use super::{
        FontLineMetrics, InkBounds, InlineObjectMetrics, LineMetricsError, LineSpacing,
        calculate_line_metrics,
    };
    use std::ops::Range;

    fn metrics(ascent_pt: f32, descent_pt: f32, leading_pt: f32) -> FontLineMetrics {
        FontLineMetrics {
            ascent_pt,
            descent_pt,
            leading_pt,
        }
    }

    fn object(height_pt: f32) -> InlineObjectMetrics {
        InlineObjectMetrics {
            source_bytes: Range { start: 0, end: 1 },
            advance_pt: height_pt,
            ascent_pt: height_pt * 0.8,
            descent_pt: height_pt * 0.2,
            ink_bounds: InkBounds {
                left: 0.0,
                top: -height_pt * 0.8,
                right: height_pt,
                bottom: 0.0,
            },
        }
    }

    #[test]
    fn natural_height_uses_max_font_metrics_and_centers_leading() {
        let result = calculate_line_metrics(
            &[metrics(8.0, 2.0, 2.0), metrics(7.0, 3.0, 1.0)],
            &[],
            metrics(8.0, 2.0, 0.0),
            LineSpacing::Natural,
        )
        .unwrap_or_else(|error| panic!("valid metrics rejected: {error}"));

        assert_eq!(result.height_pt, 13.0);
        assert_eq!(result.baseline_pt, 9.0);
        assert!(!result.content_metrics_overflow_line_box);
    }

    #[test]
    fn multiple_never_scales_an_inline_object_below_its_intrinsic_height() {
        let result = calculate_line_metrics(
            &[metrics(8.0, 2.0, 0.0)],
            &[object(20.0)],
            metrics(8.0, 2.0, 0.0),
            LineSpacing::Multiple { percent: 50 },
        )
        .unwrap_or_else(|error| panic!("valid metrics rejected: {error}"));

        assert_eq!(result.height_pt, 20.0);
        assert_eq!(result.baseline_pt, 16.0);
        assert!(!result.content_metrics_overflow_line_box);
    }

    #[test]
    fn multiple_scales_text_and_reports_metrics_overflow_below_natural_height() {
        let result = calculate_line_metrics(
            &[metrics(8.0, 2.0, 0.0)],
            &[],
            metrics(8.0, 2.0, 0.0),
            LineSpacing::Multiple { percent: 50 },
        )
        .unwrap_or_else(|error| panic!("valid metrics rejected: {error}"));

        assert_eq!(result.height_pt, 5.0);
        assert_eq!(result.baseline_pt, 8.0);
        assert!(result.content_metrics_overflow_line_box);
    }

    #[test]
    fn at_least_uses_the_largest_of_content_objects_and_requested_minimum() {
        let result = calculate_line_metrics(
            &[metrics(8.0, 2.0, 1.0)],
            &[object(20.0)],
            metrics(8.0, 2.0, 0.0),
            LineSpacing::AtLeast {
                min_centipoints: 2500,
            },
        )
        .unwrap_or_else(|error| panic!("valid metrics rejected: {error}"));

        assert_eq!(result.height_pt, 25.0);
        assert_eq!(result.baseline_pt, 18.5);
        assert!(!result.content_metrics_overflow_line_box);
    }

    #[test]
    fn natural_line_height_rejects_zero_or_non_finite_sums() {
        assert_eq!(
            calculate_line_metrics(
                &[metrics(0.0, 0.0, 0.0)],
                &[],
                metrics(0.0, 0.0, 0.0),
                LineSpacing::Natural,
            ),
            Err(LineMetricsError::InvalidFontMetrics)
        );
        assert_eq!(
            calculate_line_metrics(
                &[metrics(f32::MAX, f32::MAX, 0.0)],
                &[],
                metrics(8.0, 2.0, 0.0),
                LineSpacing::Natural,
            ),
            Err(LineMetricsError::InvalidFontMetrics)
        );
    }

    #[test]
    fn exactly_keeps_fixed_height_and_reports_ink_overflow() {
        let result = calculate_line_metrics(
            &[metrics(10.0, 4.0, 1.0)],
            &[],
            metrics(10.0, 4.0, 0.0),
            LineSpacing::Exactly { centipoints: 500 },
        )
        .unwrap_or_else(|error| panic!("valid metrics rejected: {error}"));

        assert_eq!(result.height_pt, 5.0);
        assert_eq!(result.baseline_pt, 10.0);
        assert!(result.content_metrics_overflow_line_box);
    }

    #[test]
    fn object_ink_bounds_are_reported_separately_from_font_metrics() {
        let result = calculate_line_metrics(
            &[metrics(8.0, 2.0, 0.0)],
            &[object(20.0)],
            metrics(8.0, 2.0, 0.0),
            LineSpacing::Exactly { centipoints: 500 },
        )
        .unwrap_or_else(|error| panic!("valid metrics rejected: {error}"));

        assert!(result.content_metrics_overflow_line_box);
        assert!(result.object_ink_overflows_line_box);
    }

    #[test]
    fn empty_line_uses_measured_paragraph_mark_metrics() {
        let result = calculate_line_metrics(&[], &[], metrics(9.0, 3.0, 1.0), LineSpacing::Natural)
            .unwrap_or_else(|error| panic!("valid metrics rejected: {error}"));

        assert_eq!(result.height_pt, 13.0);
        assert_eq!(result.baseline_pt, 9.5);
    }

    #[test]
    fn rejects_zero_multiple_and_non_finite_font_metrics() {
        assert_eq!(
            calculate_line_metrics(
                &[metrics(f32::NAN, 2.0, 0.0)],
                &[],
                metrics(8.0, 2.0, 0.0),
                LineSpacing::Natural,
            ),
            Err(LineMetricsError::InvalidFontMetrics)
        );
        assert_eq!(
            calculate_line_metrics(
                &[metrics(8.0, 2.0, 0.0)],
                &[],
                metrics(8.0, 2.0, 0.0),
                LineSpacing::Multiple { percent: 0 },
            ),
            Err(LineMetricsError::InvalidLineSpacing)
        );
    }
}
