//! 验证数学正文 shaping 使用 Parley 回退且事务句柄能随布局快照转移。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{MathInlineBoxMetrics, ParleyMathTextShaper, stix_two_math_font_data};
use crate::{FontCatalog, ParagraphLayouter, ResolvedDocumentTextSettings, ShapingStyle};
use caret_doc::DocumentSettings;
use caret_math::{MathTextShaper, MathTextStyle};
use parley::InlineBoxKind;

fn settings() -> ResolvedDocumentTextSettings {
    ResolvedDocumentTextSettings::from_document_settings(&DocumentSettings::default())
        .unwrap_or_else(|error| panic!("default text settings should resolve: {error}"))
}

#[test]
fn chinese_math_text_uses_parley_and_committed_resources_move_to_snapshot() {
    let mut catalog = FontCatalog::system()
        .unwrap_or_else(|error| panic!("system font catalog should load: {error}"));
    let mut layouter = ParagraphLayouter::new();
    let settings = settings();
    let base_style = ShapingStyle::new(1200);
    let provider = ParleyMathTextShaper::new(&mut catalog, &mut layouter, &settings, &base_style);

    provider
        .begin_layout()
        .unwrap_or_else(|error| panic!("math text transaction should start: {error}"));
    let metrics = provider
        .shape_text("中文", MathTextStyle::Text, 12_000_000)
        .unwrap_or_else(|error| panic!("Chinese should shape with Parley fallback: {error}"));
    assert!(provider.take_resources().is_empty());
    provider.finish_layout(true);

    assert!(metrics.width_micro_pt > 0);
    assert!(metrics.ascent_micro_pt > 0);
    let resources = provider.take_resources();
    assert!(resources.contains_key(&metrics.handle));
}

#[test]
fn failed_math_layout_transaction_releases_its_shaped_runs() {
    let mut catalog = FontCatalog::system()
        .unwrap_or_else(|error| panic!("system font catalog should load: {error}"));
    let mut layouter = ParagraphLayouter::new();
    let settings = settings();
    let base_style = ShapingStyle::new(1200);
    let provider = ParleyMathTextShaper::new(&mut catalog, &mut layouter, &settings, &base_style);

    provider
        .begin_layout()
        .unwrap_or_else(|error| panic!("math text transaction should start: {error}"));
    let shaped = provider
        .shape_text("text", MathTextStyle::Text, 12_000_000)
        .unwrap_or_else(|error| panic!("text should shape: {error}"));
    provider.finish_layout(false);

    assert!(provider.take_resources().is_empty());
    assert!(shaped.handle > 0);
}

#[test]
fn unsupported_math_text_font_styles_fail_without_committing_resources() {
    let mut catalog = FontCatalog::system()
        .unwrap_or_else(|error| panic!("system font catalog should load: {error}"));
    let mut layouter = ParagraphLayouter::new();
    let settings = settings();
    let base_style = ShapingStyle::new(1200);
    let provider = ParleyMathTextShaper::new(&mut catalog, &mut layouter, &settings, &base_style);

    provider
        .begin_layout()
        .unwrap_or_else(|error| panic!("math text transaction should start: {error}"));
    let Err(error) = provider.shape_text("A", MathTextStyle::Cal, 12_000_000) else {
        panic!("unsupported calligraphic mapping must not silently use body text");
    };
    provider.finish_layout(false);

    assert!(error.contains("Cal"));
    assert!(provider.take_resources().is_empty());
}

#[test]
fn math_inline_box_uses_formula_baseline_and_shared_stix_resource() {
    let inline = MathInlineBoxMetrics {
        width_pt: 15.0,
        ascent_pt: 8.0,
        descent_pt: 2.0,
    }
    .inline_box(9, 4)
    .unwrap_or_else(|error| panic!("inline box metrics should be valid: {error}"));

    assert_eq!(inline.id, 9);
    assert_eq!(inline.index, 4);
    assert_eq!(inline.kind, InlineBoxKind::InFlow);
    assert_eq!(inline.baseline, Some(8.0));
    assert!(
        MathInlineBoxMetrics {
            width_pt: 1.0,
            ascent_pt: f32::MAX,
            descent_pt: f32::MAX,
        }
        .inline_box(10, 5)
        .is_err()
    );
    let font = stix_two_math_font_data();
    assert_eq!(font.data.as_ref(), caret_math::MATH_FONT_BYTES);
}
