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
            let scaled_text_height_pt = text_height_pt * f32::from(percent) / 100.0;
            if objects.is_empty() {
                scaled_text_height_pt
            } else {
                scaled_text_height_pt.max(content_height_pt)
            }
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
    let baseline_pt = match spacing {
        LineSpacing::Exactly { .. } if content_overflows => height_pt - descent_pt,
        _ if content_overflows => ascent_pt,
        _ => {
            let extra_space_pt = (height_pt - content_height_pt).max(0.0);
            ascent_pt + extra_space_pt / 2.0
        }
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
        clip_ink_to_line_height: matches!(spacing, LineSpacing::Exactly { .. }),
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
        object_with_ascent_descent(height_pt * 0.8, height_pt * 0.2)
    }

    fn object_with_ascent_descent(ascent_pt: f32, descent_pt: f32) -> InlineObjectMetrics {
        InlineObjectMetrics {
            source_bytes: Range { start: 0, end: 1 },
            advance_pt: ascent_pt + descent_pt,
            ascent_pt,
            descent_pt,
            ink_bounds: InkBounds {
                left: 0.0,
                top: -ascent_pt,
                right: ascent_pt + descent_pt,
                bottom: descent_pt,
            },
        }
    }

    #[test]
    fn natural_height_uses_max_font_metrics_and_centers_leading()
    -> Result<(), Box<dyn std::error::Error>> {
        let result = calculate_line_metrics(
            &[metrics(8.0, 2.0, 2.0), metrics(7.0, 3.0, 1.0)],
            &[],
            metrics(8.0, 2.0, 0.0),
            LineSpacing::Natural,
        )?;

        assert_eq!(result.height_pt, 13.0);
        assert_eq!(result.baseline_pt, 9.0);
        assert!(!result.content_metrics_overflow_line_box);
        assert!(!result.clip_ink_to_line_height);
        Ok(())
    }

    #[test]
    fn multiple_keeps_combined_text_and_object_metrics_from_overlapping()
    -> Result<(), Box<dyn std::error::Error>> {
        let result = calculate_line_metrics(
            &[metrics(8.0, 2.0, 0.0)],
            &[object(20.0)],
            metrics(8.0, 2.0, 0.0),
            LineSpacing::Multiple { percent: 50 },
        )?;

        assert_eq!(result.height_pt, 20.0);
        assert_eq!(result.baseline_pt, 16.0);
        assert!(!result.content_metrics_overflow_line_box);
        Ok(())
    }

    #[test]
    fn multiple_keeps_text_descenders_below_a_tall_inline_object()
    -> Result<(), Box<dyn std::error::Error>> {
        let result = calculate_line_metrics(
            &[metrics(8.0, 5.0, 0.0)],
            &[object_with_ascent_descent(20.0, 0.0)],
            metrics(8.0, 5.0, 0.0),
            LineSpacing::Multiple { percent: 115 },
        )?;

        assert_eq!(result.height_pt, 25.0);
        assert_eq!(result.baseline_pt, 20.0);
        assert!(!result.content_metrics_overflow_line_box);
        Ok(())
    }

    #[test]
    fn multiple_scales_text_and_reports_metrics_overflow_below_natural_height()
    -> Result<(), Box<dyn std::error::Error>> {
        let result = calculate_line_metrics(
            &[metrics(8.0, 2.0, 0.0)],
            &[],
            metrics(8.0, 2.0, 0.0),
            LineSpacing::Multiple { percent: 50 },
        )?;

        assert_eq!(result.height_pt, 5.0);
        assert_eq!(result.baseline_pt, 8.0);
        assert!(result.content_metrics_overflow_line_box);
        Ok(())
    }

    #[test]
    fn at_least_uses_the_largest_of_content_objects_and_requested_minimum()
    -> Result<(), Box<dyn std::error::Error>> {
        let result = calculate_line_metrics(
            &[metrics(8.0, 2.0, 1.0)],
            &[object(20.0)],
            metrics(8.0, 2.0, 0.0),
            LineSpacing::AtLeast {
                min_centipoints: 2500,
            },
        )?;

        assert_eq!(result.height_pt, 25.0);
        assert_eq!(result.baseline_pt, 18.5);
        assert!(!result.content_metrics_overflow_line_box);
        assert!(!result.clip_ink_to_line_height);
        Ok(())
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
    fn exactly_keeps_fixed_height_and_reports_ink_overflow()
    -> Result<(), Box<dyn std::error::Error>> {
        let result = calculate_line_metrics(
            &[metrics(10.0, 4.0, 1.0)],
            &[],
            metrics(10.0, 4.0, 0.0),
            LineSpacing::Exactly { centipoints: 500 },
        )?;

        assert_eq!(result.height_pt, 5.0);
        assert_eq!(result.baseline_pt, 1.0);
        assert!(result.content_metrics_overflow_line_box);
        assert!(result.clip_ink_to_line_height);
        Ok(())
    }

    #[test]
    fn exactly_centers_content_when_fixed_line_box_exceeds_content()
    -> Result<(), Box<dyn std::error::Error>> {
        let result = calculate_line_metrics(
            &[metrics(8.0, 2.0, 0.0)],
            &[],
            metrics(8.0, 2.0, 0.0),
            LineSpacing::Exactly { centipoints: 1600 },
        )?;

        assert_eq!(result.height_pt, 16.0);
        assert_eq!(result.baseline_pt, 11.0);
        assert!(!result.content_metrics_overflow_line_box);
        assert!(result.clip_ink_to_line_height);
        Ok(())
    }

    #[test]
    fn object_ink_bounds_are_reported_separately_from_font_metrics()
    -> Result<(), Box<dyn std::error::Error>> {
        let result = calculate_line_metrics(
            &[metrics(8.0, 2.0, 0.0)],
            &[object(20.0)],
            metrics(8.0, 2.0, 0.0),
            LineSpacing::Exactly { centipoints: 500 },
        )?;

        assert!(result.content_metrics_overflow_line_box);
        assert!(result.object_ink_overflows_line_box);
        assert!(result.clip_ink_to_line_height);
        Ok(())
    }

    #[test]
    fn empty_line_uses_measured_paragraph_mark_metrics() -> Result<(), Box<dyn std::error::Error>> {
        let result =
            calculate_line_metrics(&[], &[], metrics(9.0, 3.0, 1.0), LineSpacing::Natural)?;

        assert_eq!(result.height_pt, 13.0);
        assert_eq!(result.baseline_pt, 9.5);
        Ok(())
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

    #[test]
    fn malformed_inline_object_geometry_is_rejected_before_line_layout() {
        let mut invalid_range = object(10.0);
        invalid_range.source_bytes = 2..2;
        let mut invalid_advance = object(10.0);
        invalid_advance.advance_pt = f32::INFINITY;
        let mut inverted_ink = object(10.0);
        inverted_ink.ink_bounds.left = inverted_ink.ink_bounds.right + 1.0;
        let mut non_finite_ink = object(10.0);
        non_finite_ink.ink_bounds.top = f32::NAN;

        for invalid in [invalid_range, invalid_advance, inverted_ink, non_finite_ink] {
            let Err(error) = calculate_line_metrics(
                &[metrics(8.0, 2.0, 0.0)],
                &[invalid],
                metrics(8.0, 2.0, 0.0),
                LineSpacing::Natural,
            ) else {
                panic!("invalid object must not produce a line box");
            };
            assert_eq!(error, LineMetricsError::InvalidObjectMetrics);
            assert_eq!(error.to_string(), "layout.invalid_object_metrics");
        }
    }
}
