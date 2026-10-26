//! 验证 Unicode 边界、段落塑形、字体回退与目录刷新。

use crate::{
    FontCatalog, FontLineMetrics, InkBounds, InlineObjectMetrics, LineSpacing,
    ResolvedDocumentTextSettings, ShapingStyle, TextShaper, UNICODE_DATA_VERSION,
    UnicodeBoundaries, layout_shaped_paragraph,
};
use caret_doc::{
    DocumentSettings, InlineEnvironment, InlineEnvironmentKind, InlineNode, NodeId, Paragraph, Run,
};
use parley_engine::{Analysis, AnalysisOptions, Analyzer, BaseDirection};
use std::collections::BTreeMap;

#[test]
fn unicode_boundaries_keep_original_graphemes_and_cover_utf8_text() {
    let text = "e\u{301} 👩‍👩‍👧‍👦\n中文";
    let boundaries = UnicodeBoundaries::new(text);

    assert_eq!(UNICODE_DATA_VERSION, "17.0.0");
    assert!(boundaries.grapheme.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(boundaries.grapheme.first(), Some(&0));
    assert_eq!(boundaries.grapheme.last(), Some(&text.len()));
    assert!(boundaries.grapheme.contains(&"e\u{301}".len()));
    assert!(boundaries.line_break.contains(&0));
    assert!(boundaries.line_break.contains(&text.len()));
}

#[test]
fn unicode_line_breaks_include_complex_script_dictionary_opportunities() {
    let text = "ภาษาไทยภาษาไทย";
    let boundaries = UnicodeBoundaries::new_with_language(text, Some("th-TH"));

    assert!(boundaries.line_break.len() > 2);
    assert_eq!(boundaries.line_break.first(), Some(&0));
    assert_eq!(boundaries.line_break.last(), Some(&text.len()));
}

#[test]
fn unicode_line_breaks_apply_english_default_and_chinese_locale_rules() {
    let english = "A short scientific paper.";
    let english_boundaries = UnicodeBoundaries::new_with_language(english, Some("en-US"));
    assert!(english_boundaries.line_break.contains(&"A ".len()));
    assert!(english_boundaries.line_break.contains(&english.len()));

    let chinese = "你好，世界。";
    let chinese_boundaries = UnicodeBoundaries::new_with_language(chinese, Some("zh-Hans"));
    let before_comma = "你好".len();
    let after_comma = "你好，".len();
    let before_period = "你好，世界".len();
    assert!(!chinese_boundaries.line_break.contains(&before_comma));
    assert!(chinese_boundaries.line_break.contains(&after_comma));
    assert!(!chinese_boundaries.line_break.contains(&before_period));
    assert!(chinese_boundaries.line_break.contains(&chinese.len()));

    let opening_punctuation = "甲（乙";
    let after_opening = "甲（".len();
    let opening_boundaries =
        UnicodeBoundaries::new_with_language(opening_punctuation, Some("zh-CN"));
    assert!(!opening_boundaries.line_break.contains(&after_opening));
}

#[test]
fn chinese_kinsoku_keeps_mandatory_unicode_breaks() {
    let text = "a\u{2028}，b";
    let boundary_after_line_separator = "a\u{2028}".len();
    let boundaries = UnicodeBoundaries::new_with_language(text, Some("zh"));

    assert!(
        boundaries
            .line_break
            .contains(&boundary_after_line_separator)
    );
}

#[test]
fn paragraph_layout_wraps_at_unicode_breaks_and_keeps_source_ranges_contiguous() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let mut shaper = TextShaper::new();
    let source = "one two three four";
    let shaped = match shaper.shape(&mut catalog, source, &ShapingStyle::default()) {
        Ok(shaped) => shaped.clone(),
        Err(error) => panic!("paragraph shaping failed: {error}"),
    };
    let width = shaped
        .text
        .runs()
        .iter()
        .map(|run| run.advance)
        .sum::<f32>()
        * 0.55;
    let layout = match layout_shaped_paragraph(
        source,
        &shaped,
        width,
        LineSpacing::Natural,
        FontLineMetrics {
            ascent_pt: 9.6,
            descent_pt: 2.4,
            leading_pt: 0.0,
        },
        &[],
    ) {
        Ok(layout) => layout,
        Err(error) => panic!("paragraph layout failed: {error}"),
    };

    assert!(layout.lines.len() > 1);
    assert!(layout.lines.iter().all(|line| !line.overflows_width));
    assert_eq!(
        layout.lines.first().map(|line| line.source_bytes.start),
        Some(0)
    );
    assert_eq!(
        layout.lines.last().map(|line| line.source_bytes.end),
        Some(source.len())
    );
    assert!(layout.lines.windows(2).all(|lines| {
        lines[0].consumed_bytes.end == lines[1].source_bytes.start
            && shaped
                .line_breaks
                .binary_search(&lines[0].source_bytes.end)
                .is_ok()
            && shaped
                .grapheme_boundaries
                .binary_search(&lines[0].source_bytes.end)
                .is_ok()
    }));
}

#[test]
fn paragraph_layout_keeps_a_trailing_soft_break_as_an_empty_line() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let mut shaper = TextShaper::new();
    let source = "line\n";
    let shaped = match shaper.shape(&mut catalog, source, &ShapingStyle::default()) {
        Ok(shaped) => shaped.clone(),
        Err(error) => panic!("paragraph shaping failed: {error}"),
    };
    let layout = match layout_shaped_paragraph(
        source,
        &shaped,
        500.0,
        LineSpacing::Natural,
        FontLineMetrics {
            ascent_pt: 9.6,
            descent_pt: 2.4,
            leading_pt: 0.0,
        },
        &[],
    ) {
        Ok(layout) => layout,
        Err(error) => panic!("paragraph layout failed: {error}"),
    };

    assert_eq!(layout.lines.len(), 2);
    assert_eq!(layout.lines[0].source_bytes, 0..4);
    assert_eq!(layout.lines[0].consumed_bytes, 0..5);
    assert_eq!(layout.lines[1].source_bytes, 5..5);
    assert_eq!(layout.lines[1].consumed_bytes, 5..5);
}

#[test]
fn empty_space_and_tab_only_lines_keep_finite_positive_metrics() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let mut shaper = TextShaper::new();
    let empty_mark_metrics = FontLineMetrics {
        ascent_pt: 9.6,
        descent_pt: 2.4,
        leading_pt: 0.0,
    };

    for source in ["", "   ", "\t\t"] {
        let shaped = match shaper.shape(&mut catalog, source, &ShapingStyle::default()) {
            Ok(shaped) => shaped.clone(),
            Err(error) => panic!("paragraph shaping failed for {source:?}: {error}"),
        };
        let layout = match layout_shaped_paragraph(
            source,
            &shaped,
            500.0,
            LineSpacing::Natural,
            empty_mark_metrics,
            &[],
        ) {
            Ok(layout) => layout,
            Err(error) => panic!("paragraph layout failed for {source:?}: {error}"),
        };

        assert!(!layout.lines.is_empty(), "source {source:?}");
        assert!(layout.total_height_pt.is_finite() && layout.total_height_pt > 0.0);
        assert!(
            layout
                .lines
                .iter()
                .all(|line| line.metrics.height_pt.is_finite() && line.metrics.height_pt > 0.0)
        );
    }
}

#[test]
fn inline_object_metrics_replace_placeholder_cluster_advance_and_font_height() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let mut shaper = TextShaper::new();
    let source = "x";
    let shaped = match shaper.shape(&mut catalog, source, &ShapingStyle::default()) {
        Ok(shaped) => shaped.clone(),
        Err(error) => panic!("paragraph shaping failed: {error}"),
    };
    let object = InlineObjectMetrics {
        source_bytes: 0..1,
        advance_pt: 30.0,
        ascent_pt: 16.0,
        descent_pt: 4.0,
        ink_bounds: InkBounds {
            left: 0.0,
            top: -16.0,
            right: 30.0,
            bottom: 4.0,
        },
    };
    let layout = match layout_shaped_paragraph(
        source,
        &shaped,
        10.0,
        LineSpacing::Natural,
        FontLineMetrics {
            ascent_pt: 9.6,
            descent_pt: 2.4,
            leading_pt: 0.0,
        },
        &[object],
    ) {
        Ok(layout) => layout,
        Err(error) => panic!("paragraph layout failed: {error}"),
    };

    assert_eq!(layout.lines[0].advance_pt, 30.0);
    assert!(layout.lines[0].overflows_width);
    assert!(layout.lines[0].metrics.height_pt >= 20.0);
}

#[test]
fn paragraph_layout_honors_unicode_mandatory_breaks() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let mut shaper = TextShaper::new();
    let empty_mark_metrics = FontLineMetrics {
        ascent_pt: 9.6,
        descent_pt: 2.4,
        leading_pt: 0.0,
    };

    for separator in ["\n", "\r\n", "\u{000c}", "\u{2028}", "\u{2029}"] {
        let source = format!("left{separator}right");
        let break_start = "left".len();
        let break_end = break_start + separator.len();
        let shaped = match shaper.shape(&mut catalog, &source, &ShapingStyle::default()) {
            Ok(shaped) => shaped.clone(),
            Err(error) => panic!("paragraph shaping failed for {separator:?}: {error}"),
        };
        let layout = match layout_shaped_paragraph(
            &source,
            &shaped,
            500.0,
            LineSpacing::Natural,
            empty_mark_metrics,
            &[],
        ) {
            Ok(layout) => layout,
            Err(error) => panic!("paragraph layout failed for {separator:?}: {error}"),
        };

        assert_eq!(layout.lines.len(), 2, "separator {separator:?}");
        assert_eq!(layout.lines[0].source_bytes, 0..break_start);
        assert_eq!(layout.lines[0].consumed_bytes, 0..break_end);
        assert_eq!(layout.lines[1].source_bytes, break_end..source.len());
    }
}

#[test]
fn dictionary_line_breaks_do_not_split_an_extended_grapheme() {
    let text = "\u{0e31}\u{0308}\u{0e31}";
    let boundaries = UnicodeBoundaries::new_with_language(text, Some("th"));

    assert_eq!(boundaries.grapheme, vec![0, text.len()]);
    assert_eq!(boundaries.line_break, vec![0, text.len()]);
}

#[test]
fn run_language_enables_dictionary_breaks_over_paragraph_language() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let paragraph = Paragraph {
        id: node_id("localized-run-paragraph"),
        language: Some("en".to_owned()),
        inlines: vec![InlineNode::Run(Run {
            text: "ภาษาไทยภาษาไทย".to_owned(),
            language: Some("th-TH".to_owned()),
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let mut shaper = TextShaper::new();
    let output = match shaper.shape_paragraph(
        &mut catalog,
        &paragraph,
        &ShapingStyle {
            language: Some("en".to_owned()),
            ..ShapingStyle::default()
        },
    ) {
        Ok(output) => output,
        Err(error) => panic!("run-local dictionary shaping failed: {error}"),
    };

    assert!(output.line_breaks.len() > 2);
}

#[test]
fn document_language_is_used_when_paragraph_has_no_override() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let paragraph = Paragraph {
        id: node_id("document-language-paragraph"),
        language: None,
        inlines: vec![InlineNode::Run(Run {
            text: "ภาษาไทยภาษาไทย".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let settings = DocumentSettings {
        default_language: Some("th-TH".to_owned()),
        default_font_family: Some("Caret Missing Preference Font".to_owned()),
        ..DocumentSettings::default()
    };
    let resolved_settings = ResolvedDocumentTextSettings::from_document_settings(&settings)
        .unwrap_or_else(|error| panic!("document settings should resolve: {error}"));
    let mut shaper = TextShaper::new();
    let output = match shaper.shape_paragraph_with_resolved_settings(
        &mut catalog,
        &paragraph,
        &resolved_settings,
        &ShapingStyle::default(),
    ) {
        Ok(output) => output,
        Err(error) => panic!("document-language shaping failed: {error}"),
    };

    assert!(output.line_breaks.len() > 2);
    assert!(output.requested_family_missing);
    assert!(!output.actual_families.is_empty());
}

#[test]
fn font_preferences_use_full_run_language_even_when_shaping_language_is_narrower() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let paragraph = Paragraph {
        id: node_id("variant-language-font-preference"),
        language: None,
        inlines: vec![InlineNode::Run(Run {
            text: "Slovenščina".to_owned(),
            language: Some("sl-rozaj".to_owned()),
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let settings = DocumentSettings {
        language_script_font_families: BTreeMap::from([(
            "Latn".to_owned(),
            BTreeMap::from([("sl-rozaj".to_owned(), "Missing Slovenian Font".to_owned())]),
        )]),
        ..DocumentSettings::default()
    };
    let resolved_settings = ResolvedDocumentTextSettings::from_document_settings(&settings)
        .unwrap_or_else(|error| panic!("document settings should resolve: {error}"));
    let mut shaper = TextShaper::new();
    let output = match shaper.shape_paragraph_with_resolved_settings(
        &mut catalog,
        &paragraph,
        &resolved_settings,
        &ShapingStyle::default(),
    ) {
        Ok(output) => output,
        Err(error) => panic!("language-specific font preference failed: {error}"),
    };

    assert!(output.requested_family_missing);
    assert!(!output.actual_families.is_empty());
}

#[test]
fn paragraph_reshaping_rejects_empty_ranges_inside_utf8_characters() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let paragraph = Paragraph {
        id: node_id("utf8-range-paragraph"),
        language: None,
        inlines: vec![InlineNode::Run(Run {
            text: "中".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let mut shaper = TextShaper::new();

    assert!(matches!(
        shaper.shape_paragraph_range(
            &mut catalog,
            &paragraph,
            &ShapingStyle::default(),
            1..1,
            BaseDirection::Ltr,
        ),
        Err(crate::ShapingError::InvalidTextRange)
    ));
}

#[test]
fn dictionary_breaks_stay_inside_runs_with_the_matching_language() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let first_run = "ภาษาไทย ";
    let second_run = "ภาษาไทย";
    let second_start = first_run.len();
    let second_end = second_start + second_run.len();
    let paragraph = Paragraph {
        id: node_id("mixed-language-break-paragraph"),
        language: Some("en".to_owned()),
        inlines: vec![
            InlineNode::Run(Run {
                text: first_run.to_owned(),
                language: Some("th".to_owned()),
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }),
            InlineNode::Run(Run {
                text: second_run.to_owned(),
                language: Some("en".to_owned()),
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }),
        ],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let mut shaper = TextShaper::new();
    let output = match shaper.shape_paragraph(
        &mut catalog,
        &paragraph,
        &ShapingStyle {
            language: Some("en".to_owned()),
            ..ShapingStyle::default()
        },
    ) {
        Ok(output) => output,
        Err(error) => panic!("mixed-language shaping failed: {error}"),
    };

    assert!(
        output
            .line_breaks
            .iter()
            .any(|boundary| *boundary > 0 && *boundary < second_start)
    );
    assert!(
        !output
            .line_breaks
            .iter()
            .any(|boundary| *boundary > second_start && *boundary < second_end)
    );
}

#[test]
fn unicode_17_grapheme_break_conformance_corpus() {
    run_unicode_break_corpus(
        include_str!("../../../tests/fixtures/unicode-17/GraphemeBreakTest.txt"),
        true,
    );
}

#[test]
fn unicode_17_line_break_conformance_corpus() {
    run_unicode_break_corpus(
        include_str!("../../../tests/fixtures/unicode-17/LineBreakTest.txt"),
        false,
    );
}

#[test]
fn unicode_17_bidi_character_conformance_corpus() {
    let mut analyzer = Analyzer::new();
    let mut analysis = Analysis::new();
    for (line_number, line) in
        include_str!("../../../tests/fixtures/unicode-17/BidiCharacterTest.txt")
            .lines()
            .enumerate()
    {
        let Some(test_case) = line.split('#').next() else {
            continue;
        };
        if test_case.trim().is_empty() {
            continue;
        }
        let fields: Vec<_> = test_case.split(';').map(str::trim).collect();
        if fields.len() != 5 {
            panic!("invalid bidi corpus line {}", line_number + 1);
        }
        let mut text = String::new();
        for codepoint in fields[0].split_ascii_whitespace() {
            let scalar = match u32::from_str_radix(codepoint, 16)
                .ok()
                .and_then(char::from_u32)
            {
                Some(scalar) => scalar,
                None => panic!("invalid bidi codepoint on line {}", line_number + 1),
            };
            text.push(scalar);
        }
        let base_direction = match fields[1] {
            "0" => BaseDirection::Ltr,
            "1" => BaseDirection::Rtl,
            "2" => BaseDirection::Auto,
            _ => panic!(
                "invalid bidi paragraph direction on line {}",
                line_number + 1
            ),
        };
        let expected_paragraph_level = match fields[2].parse::<u8>() {
            Ok(level) => level,
            Err(_) => panic!("invalid bidi paragraph level on line {}", line_number + 1),
        };
        analyzer.analyze(
            &text,
            &AnalysisOptions {
                base_direction,
                ..AnalysisOptions::default()
            },
            &mut analysis,
        );
        assert_eq!(
            analysis.paragraph_level().to_u8(),
            expected_paragraph_level,
            "paragraph level mismatch on bidi corpus line {}",
            line_number + 1,
        );
        let actual_levels: Vec<_> = if analysis.bidi_levels().is_empty() {
            vec![0; text.chars().count()]
        } else {
            analysis
                .bidi_levels()
                .iter()
                .map(|level| level.to_u8())
                .collect()
        };
        let expected_levels: Vec<_> = fields[3].split_ascii_whitespace().collect();
        assert_eq!(
            actual_levels.len(),
            expected_levels.len(),
            "bidi level count mismatch on corpus line {}",
            line_number + 1,
        );
        for (actual, expected) in actual_levels.iter().zip(expected_levels.iter().copied()) {
            if expected != "x" {
                let expected = match expected.parse::<u8>() {
                    Ok(level) => level,
                    Err(_) => panic!("invalid bidi level on line {}", line_number + 1),
                };
                assert_eq!(
                    *actual,
                    expected,
                    "resolved bidi level mismatch on corpus line {}",
                    line_number + 1,
                );
            }
        }
        let mut actual_visual_order: Vec<_> = expected_levels
            .iter()
            .enumerate()
            .filter_map(|(index, expected)| (*expected != "x").then_some(index))
            .collect();
        reorder_by_bidi_levels(&mut actual_visual_order, &actual_levels);
        let expected_visual_order: Vec<_> = fields[4]
            .split_ascii_whitespace()
            .filter_map(|index| index.parse::<usize>().ok())
            .collect();
        assert_eq!(
            actual_visual_order,
            expected_visual_order,
            "bidi visual order mismatch on corpus line {}",
            line_number + 1,
        );
    }
}

fn reorder_by_bidi_levels(visual_order: &mut [usize], levels: &[u8]) {
    let Some(max_level) = visual_order.iter().map(|index| levels[*index]).max() else {
        return;
    };
    let Some(min_odd_level) = visual_order
        .iter()
        .map(|index| levels[*index])
        .filter(|level| level % 2 == 1)
        .min()
    else {
        return;
    };
    for level in (min_odd_level..=max_level).rev() {
        let mut start = 0;
        while start < visual_order.len() {
            if levels[visual_order[start]] < level {
                start += 1;
                continue;
            }
            let mut end = start + 1;
            while end < visual_order.len() && levels[visual_order[end]] >= level {
                end += 1;
            }
            visual_order[start..end].reverse();
            start = end;
        }
    }
}

#[test]
fn missing_requested_family_is_reported_while_fallback_shapes_mixed_direction_text() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let mut shaper = TextShaper::new();
    let text = "Caret 文稿 العربية Русский e\u{301} 👩‍👩‍👧‍👦";
    let output = match shaper.shape(
        &mut catalog,
        text,
        &ShapingStyle {
            requested_family: Some("Caret Missing Test Family 8D5A".to_owned()),
            ..ShapingStyle::default()
        },
    ) {
        Ok(output) => output,
        Err(error) => panic!("shape failed: {error}"),
    };

    assert!(output.requested_family_missing);
    assert!(!output.actual_families.is_empty());
    assert!(!output.text.shaped_clusters().is_empty());
    assert_eq!(
        output.cluster_bytes.len(),
        output.text.shaped_clusters().len()
    );
    assert!(output.cluster_bytes.iter().all(|range| {
        range.start <= range.end
            && range.end <= text.len()
            && text.is_char_boundary(range.start)
            && text.is_char_boundary(range.end)
    }));
    assert!(output.text.runs().iter().any(|run| run.bidi_level.is_rtl()));
}

#[test]
fn half_point_font_sizes_and_cyrillic_greek_shape_with_finite_metrics() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let mut shaper = TextShaper::new();
    let output = match shaper.shape(
        &mut catalog,
        "Русский текст · Ελληνικά",
        &ShapingStyle {
            font_size_centipoints: 50,
            ..ShapingStyle::default()
        },
    ) {
        Ok(output) => output,
        Err(error) => panic!("sub-point Cyrillic shaping failed: {error}"),
    };

    assert!(!output.text.shaped_clusters().is_empty());
    assert!(output.text.runs().iter().all(|run| {
        run.font_size == 0.5 && run.advance.is_finite() && run.font_metrics.ascent.is_finite()
    }));
    assert!(
        output
            .cluster_bytes
            .iter()
            .all(|range| range.end <= "Русский текст · Ελληνικά".len())
    );

    let quarter_size = match shaper.shape(
        &mut catalog,
        "12.25 pt",
        &ShapingStyle {
            font_size_centipoints: 1225,
            ..ShapingStyle::default()
        },
    ) {
        Ok(output) => output,
        Err(error) => panic!("fractional point-size shaping failed: {error}"),
    };
    assert!(
        quarter_size
            .text
            .runs()
            .iter()
            .all(|run| run.font_size == 12.25)
    );
}

#[test]
fn emoji_and_zwj_family_use_emoji_fonts_with_nonzero_glyphs() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let mut shaper = TextShaper::new();
    let emoji = match shaper.shape(&mut catalog, "😀", &ShapingStyle::default()) {
        Ok(output) => output.clone(),
        Err(error) => panic!("emoji shaping failed: {error}"),
    };
    let family_text = "👩‍👩‍👧‍👦";
    let family = match shaper.shape(&mut catalog, family_text, &ShapingStyle::default()) {
        Ok(output) => output.clone(),
        Err(error) => panic!("emoji ZWJ shaping failed: {error}"),
    };

    for shaped in [&emoji, &family] {
        let mut glyph_ids = Vec::new();
        for cluster in shaped.text.shaped_clusters() {
            if cluster.has_inline_glyph() {
                glyph_ids.push(cluster.glyph_offset);
            } else {
                let start = cluster.glyph_offset as usize;
                let end = start + usize::from(cluster.glyph_len());
                glyph_ids.extend(
                    shaped.text.glyphs()[start..end]
                        .iter()
                        .map(|glyph| glyph.id),
                );
            }
        }
        assert!(!glyph_ids.is_empty());
        assert!(glyph_ids.iter().all(|glyph_id| *glyph_id != 0));
        assert!(
            shaped
                .actual_families
                .iter()
                .any(|family| family.to_lowercase().contains("emoji"))
        );
    }
    assert!(
        family
            .text
            .shaped_clusters()
            .iter()
            .all(|cluster| cluster.advance.is_finite())
    );
    assert_eq!(
        family.cluster_bytes.last().map(|range| range.end),
        Some(family_text.len())
    );

    let explicit_family = match shaper.shape(
        &mut catalog,
        "☺️",
        &ShapingStyle {
            requested_family: Some("Arial".to_owned()),
            ..ShapingStyle::default()
        },
    ) {
        Ok(output) => output.clone(),
        Err(error) => panic!("emoji presentation with an explicit family failed: {error}"),
    };
    assert!(
        explicit_family
            .actual_families
            .iter()
            .any(|family| family.to_lowercase().contains("emoji"))
    );
}

#[test]
fn explicit_font_refresh_replaces_catalog_generation() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let old_generation = catalog.generation();
    let old_families = catalog.families().len();

    let refreshed_generation = match catalog.refresh() {
        Ok(generation) => generation,
        Err(error) => panic!("font refresh failed: {error}"),
    };

    assert_eq!(refreshed_generation, old_generation + 1);
    assert!(!catalog.families().is_empty());
    assert_eq!(catalog.generation(), refreshed_generation);
    assert!(old_families > 0);
}

#[test]
fn paragraph_shaping_resolves_nested_bold_and_font_environments() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let first_family = catalog.families()[0].name.clone();
    let paragraph = Paragraph {
        id: node_id("paragraph"),
        language: Some("en".to_owned()),
        inlines: vec![
            InlineNode::Run(Run {
                text: "plain ".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }),
            InlineNode::Environment(InlineEnvironment {
                id: node_id("font-environment"),
                kind: InlineEnvironmentKind::Font,
                definition_id: None,
                props: BTreeMap::from([(
                    "fontFamily".to_owned(),
                    serde_json::Value::String(first_family),
                )]),
                children: vec![InlineNode::Environment(InlineEnvironment {
                    id: node_id("bold-environment"),
                    kind: InlineEnvironmentKind::Bold,
                    definition_id: None,
                    props: BTreeMap::new(),
                    children: vec![InlineNode::Run(Run {
                        text: "bold".to_owned(),
                        language: None,
                        props: BTreeMap::new(),
                        extra: BTreeMap::new(),
                    })],
                    extra: BTreeMap::new(),
                })],
                extra: BTreeMap::new(),
            }),
        ],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let mut shaper = TextShaper::new();
    let output = match shaper.shape_paragraph(&mut catalog, &paragraph, &ShapingStyle::default()) {
        Ok(output) => output,
        Err(error) => panic!("paragraph shaping failed: {error}"),
    };

    assert_eq!(output.text.characters().len(), "plain bold".chars().count());
    assert!(
        output.text.characters()[..6]
            .iter()
            .all(|character| character.style_index == 0)
    );
    assert!(
        output.text.characters()[6..]
            .iter()
            .all(|character| character.style_index == 1)
    );
    assert!(!output.actual_families.is_empty());
    assert!(!output.requested_family_missing);
}

#[test]
fn same_style_run_boundaries_do_not_change_shaping() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let source = "office العربية e\u{301}";
    let paragraph = Paragraph {
        id: node_id("split-run-paragraph"),
        language: None,
        inlines: vec![
            InlineNode::Run(Run {
                text: "office العربية e".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }),
            InlineNode::Run(Run {
                text: "\u{301}".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }),
        ],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let mut shaper = TextShaper::new();
    let flat = match shaper.shape(&mut catalog, source, &ShapingStyle::default()) {
        Ok(output) => output.clone(),
        Err(error) => panic!("flat text shaping failed: {error}"),
    };
    let split = match shaper.shape_paragraph(&mut catalog, &paragraph, &ShapingStyle::default()) {
        Ok(output) => output.clone(),
        Err(error) => panic!("split run shaping failed: {error}"),
    };

    assert_eq!(flat.cluster_bytes, split.cluster_bytes);
    assert_eq!(flat.text, split.text);
}

#[test]
fn invalid_font_size_is_rejected_before_shaping() {
    let mut catalog = match FontCatalog::system() {
        Ok(catalog) => catalog,
        Err(error) => panic!("font enumeration failed: {error}"),
    };
    let mut shaper = TextShaper::new();

    let result = shaper.shape(
        &mut catalog,
        "text",
        &ShapingStyle {
            font_size_centipoints: 0,
            ..ShapingStyle::default()
        },
    );
    assert!(matches!(result, Err(crate::ShapingError::InvalidFontSize)));
}

fn node_id(value: &str) -> NodeId {
    match NodeId::parse(value) {
        Ok(id) => id,
        Err(error) => panic!("invalid test node ID: {error}"),
    }
}

fn run_unicode_break_corpus(corpus: &str, grapheme: bool) {
    for (line_number, line) in corpus.lines().enumerate() {
        let Some(test_case) = line.split('#').next() else {
            continue;
        };
        if test_case.trim().is_empty() {
            continue;
        }
        let mut text = String::new();
        let mut expected = Vec::new();
        for token in test_case.split_ascii_whitespace() {
            match token {
                "÷" => expected.push(text.len()),
                "×" => {}
                codepoint => {
                    let scalar = match u32::from_str_radix(codepoint, 16)
                        .ok()
                        .and_then(char::from_u32)
                    {
                        Some(scalar) => scalar,
                        None => panic!("invalid Unicode scalar on corpus line {}", line_number + 1),
                    };
                    text.push(scalar);
                }
            }
        }
        if expected.first().copied() != Some(0) {
            expected.insert(0, 0);
        }
        let boundaries = UnicodeBoundaries::new(&text);
        let actual = if grapheme {
            &boundaries.grapheme
        } else {
            &boundaries.line_break
        };
        assert_eq!(
            actual,
            &expected,
            "Unicode 17 {} break mismatch on corpus line {}: {line}",
            if grapheme { "grapheme" } else { "line" },
            line_number + 1,
        );
    }
}
