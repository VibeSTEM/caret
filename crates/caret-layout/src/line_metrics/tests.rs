//! 验证行高、基线和不同文字脚本的度量。

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
fn exactly_keeps_fixed_height_and_reports_ink_overflow() -> Result<(), Box<dyn std::error::Error>> {
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
    let result = calculate_line_metrics(&[], &[], metrics(9.0, 3.0, 1.0), LineSpacing::Natural)?;

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
