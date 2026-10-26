//! 评估 Parley 高层布局与 Caret 的 Unicode、字体和视觉光标规则能否兼容。

use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use icu_properties::{CodePointMapData, props::LineBreak as LineBreakClass};
use parley::{FontContext, LayoutContext, StyleProperty, editing::Cursor};

use crate::{
    FontCatalog, FontLineMetrics, LineSpacing, ShapingStyle, TextShaper, UnicodeBoundaries,
    layout_shaped_paragraph,
};

fn font_context(catalog: &FontCatalog) -> FontContext {
    FontContext {
        collection: catalog.collection.clone(),
        source_cache: catalog.source_cache.clone(),
    }
}

fn line_break_override(text: &str, boundaries: &UnicodeBoundaries) -> (Vec<bool>, usize) {
    let mut decisions = Vec::new();
    let mut byte_offset = 0;
    let mut previous = None;
    for current in text.chars() {
        if let Some(before) = previous
            && !is_mandatory(before)
            && !is_mandatory(current)
        {
            byte_offset += before.len_utf8();
            decisions.push(boundaries.line_break.binary_search(&byte_offset).is_ok());
        } else if let Some(before) = previous {
            byte_offset += before.len_utf8();
        }
        previous = Some(current);
    }
    let callback_count = decisions.len();
    (decisions, callback_count)
}

fn is_mandatory(character: char) -> bool {
    matches!(
        CodePointMapData::<LineBreakClass>::new().get(character),
        LineBreakClass::MandatoryBreak
            | LineBreakClass::CarriageReturn
            | LineBreakClass::LineFeed
            | LineBreakClass::NextLine
    )
}

fn build_layout(
    catalog: &FontCatalog,
    text: &str,
    language: Option<&str>,
    width: f32,
    style_range: Option<Range<usize>>,
) -> (parley::Layout<()>, usize, usize) {
    let boundaries = UnicodeBoundaries::new_with_language(text, language);
    let (decisions, pair_count) = line_break_override(text, &boundaries);
    let cursor = Arc::new(AtomicUsize::new(0));
    let callback_cursor = Arc::clone(&cursor);
    let override_fn = move |_| {
        let index = callback_cursor.fetch_add(1, Ordering::Relaxed);
        decisions.get(index).copied()
    };
    let mut font_context = font_context(catalog);
    let mut layout_context = LayoutContext::new();
    let mut builder = layout_context.ranged_builder(&mut font_context, text, 1.0, true);
    builder.push_default(StyleProperty::FontSize(12.0));
    if let Some(range) = style_range {
        builder.push(StyleProperty::FontSize(14.0), range);
    }
    builder.set_line_break_override(Some(&override_fn));
    let mut layout = builder.build(text);
    layout.break_all_lines(Some(width));
    (layout, cursor.load(Ordering::Relaxed), pair_count)
}

#[test]
fn high_level_parley_layout_accepts_carets_language_break_opportunities() {
    let mut catalog = FontCatalog::system().unwrap_or_else(|error| panic!("font catalog: {error}"));
    for (text, language) in [
        ("alpha beta gamma delta", Some("en")),
        ("你好，世界。再见！", Some("zh-Hans")),
    ] {
        let (layout, callbacks, expected_callbacks) =
            build_layout(&catalog, text, language, 48.0, None);
        assert_eq!(callbacks, expected_callbacks);
        assert!(layout.lines().count() > 1);
        assert!(layout.lines().all(|line| {
            let range = line.text_range();
            text.is_char_boundary(range.start) && text.is_char_boundary(range.end)
        }));

        let mut shaper = TextShaper::new();
        let shaped = shaper
            .shape(
                &mut catalog,
                text,
                &ShapingStyle {
                    language: language.map(str::to_owned),
                    ..ShapingStyle::default()
                },
            )
            .unwrap_or_else(|error| panic!("Caret shaping failed: {error}"))
            .clone();
        let caret_layout = layout_shaped_paragraph(
            text,
            &shaped,
            48.0,
            LineSpacing::Natural,
            FontLineMetrics {
                ascent_pt: 9.6,
                descent_pt: 2.4,
                leading_pt: 0.0,
            },
            &[],
        )
        .unwrap_or_else(|error| panic!("Caret line breaking failed: {error}"));
        let parley_ranges: Vec<_> = layout.lines().map(|line| line.text_range()).collect();
        let caret_ranges: Vec<_> = caret_layout
            .lines
            .iter()
            .map(|line| line.source_bytes.clone())
            .collect();
        assert_eq!(
            parley_ranges, caret_ranges,
            "line ranges differ for {language:?}"
        );
    }
}

#[test]
fn high_level_parley_visual_cursors_return_utf8_grapheme_boundaries() {
    let catalog = FontCatalog::system().unwrap_or_else(|error| panic!("font catalog: {error}"));
    let text = "abc אבג 中文";
    let (layout, callbacks, expected_callbacks) =
        build_layout(&catalog, text, Some("en"), 240.0, None);
    assert_eq!(callbacks, expected_callbacks);

    let boundaries = UnicodeBoundaries::new(text).grapheme;
    let mut cursor = Cursor::from_byte_index(&layout, 0, parley::Affinity::Downstream);
    let mut visited = Vec::new();
    for _ in 0..text.len() * 4 {
        assert!(boundaries.binary_search(&cursor.index()).is_ok());
        let state = (cursor.index(), cursor.affinity());
        if visited.contains(&state) {
            break;
        }
        visited.push(state);
        let next = cursor.next_visual(&layout);
        if next == cursor {
            break;
        }
        cursor = next;
    }
    assert!(visited.len() > 2);
    assert_eq!(
        cursor.index(),
        text.len(),
        "visual cursor did not reach the end"
    );
    for x in (0..240).step_by(8) {
        let hit = Cursor::from_point(&layout, x as f32, 6.0);
        assert!(boundaries.binary_search(&hit.index()).is_ok());
    }
}

#[test]
fn high_level_parley_probe_observes_cross_run_combining_mark_cursor_stops() {
    let catalog = FontCatalog::system().unwrap_or_else(|error| panic!("font catalog: {error}"));
    let text = "e\u{301}x";
    let (layout, callbacks, expected_callbacks) =
        build_layout(&catalog, text, Some("en"), 240.0, Some(1..3));
    assert_eq!(callbacks, expected_callbacks);
    let boundaries = UnicodeBoundaries::new(text).grapheme;
    let mut cursor = Cursor::from_byte_index(&layout, 0, parley::Affinity::Downstream);
    let mut observed_non_grapheme_stop = false;
    for _ in 0..=boundaries.len() * 2 {
        observed_non_grapheme_stop |= boundaries.binary_search(&cursor.index()).is_err();
        let next = cursor.next_visual(&layout);
        if next == cursor {
            break;
        }
        cursor = next;
    }
    assert!(
        observed_non_grapheme_stop,
        "recheck the cross-run grapheme adapter if Parley changes this behavior"
    );
}
