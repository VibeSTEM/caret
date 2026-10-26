//! 覆盖 Parley 高层布局、Caret Unicode tailoring、字体偏好及跨 Run 行为。

use std::collections::BTreeMap;

use caret_doc::{
    DocumentSettings, InlineEnvironment, InlineEnvironmentKind, InlineNode, NodeId, Paragraph, Run,
};
use parley_engine::{Analysis, AnalysisOptions, Analyzer, BaseDirection};

use crate::{
    FontCatalog, FontLineMetrics, LineSpacing, ParagraphLayoutOptions, ParagraphLayouter,
    ResolvedDocumentTextSettings, ShapingStyle, UNICODE_DATA_VERSION, UnicodeBoundaries,
    has_empty_line, paragraph_has_empty_line,
};

fn paragraph(text: &str, language: Option<&str>) -> Paragraph {
    Paragraph {
        id: NodeId::generate(),
        language: language.map(str::to_owned),
        layout: None,
        inlines: vec![InlineNode::Run(Run {
            text: text.to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }
}

fn font_catalog() -> FontCatalog {
    FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"))
}

fn document_settings() -> ResolvedDocumentTextSettings {
    ResolvedDocumentTextSettings::from_document_settings(&DocumentSettings::default())
        .unwrap_or_else(|error| panic!("document settings failed: {error}"))
}

fn empty_metrics() -> FontLineMetrics {
    FontLineMetrics {
        ascent_pt: 9.6,
        descent_pt: 2.4,
        leading_pt: 0.0,
    }
}

fn layout_options(width_pt: f32) -> ParagraphLayoutOptions {
    ParagraphLayoutOptions {
        width_pt,
        spacing: LineSpacing::Natural,
        empty_mark_metrics: empty_metrics(),
    }
}

fn glyph_signature(layout: &parley::Layout<[u8; 4]>) -> Vec<(u32, f32, f32, f32)> {
    layout
        .lines()
        .flat_map(|line| {
            line.items().filter_map(|item| match item {
                parley::PositionedLayoutItem::GlyphRun(run) => Some(run),
                parley::PositionedLayoutItem::InlineBox(_) => None,
            })
        })
        .flat_map(|run| run.positioned_glyphs().collect::<Vec<_>>())
        .map(|glyph| (glyph.id, glyph.x, glyph.y, glyph.advance))
        .collect()
}

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
fn locale_tailoring_keeps_english_chinese_and_dictionary_break_rules() {
    let english = "A short scientific paper.";
    let english_boundaries = UnicodeBoundaries::new_with_language(english, Some("en-US"));
    assert!(english_boundaries.line_break.contains(&"A ".len()));
    assert!(english_boundaries.line_break.contains(&english.len()));

    let thai = "ภาษาไทยภาษาไทย";
    let thai_boundaries = UnicodeBoundaries::new_with_language(thai, Some("th-TH"));
    assert!(thai_boundaries.line_break.len() > 2);
    assert_eq!(thai_boundaries.line_break.first(), Some(&0));
    assert_eq!(thai_boundaries.line_break.last(), Some(&thai.len()));
}

#[test]
fn unicode_17_grapheme_and_line_break_conformance_corpora_pass() {
    run_unicode_break_corpus(
        include_str!("../../../tests/fixtures/unicode-17/GraphemeBreakTest.txt"),
        true,
    );
    run_unicode_break_corpus(
        include_str!("../../../tests/fixtures/unicode-17/LineBreakTest.txt"),
        false,
    );
}

#[test]
fn parley_layout_matches_icu_line_ranges_for_english_chinese_and_bidi() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    for (text, language) in [
        ("alpha beta gamma delta epsilon", Some("en")),
        ("你好，世界。再见！", Some("zh-Hans")),
        ("abc אבג 中文 xyz", Some("en")),
    ] {
        let paragraph = paragraph(text, language);
        let (_, layout) = layouter
            .layout(
                &mut catalog,
                &paragraph,
                &settings,
                &ShapingStyle::default(),
                layout_options(48.0),
            )
            .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
        assert!(layout.paragraph.lines.len() > 1);
        assert_eq!(
            layout.grapheme_boundaries.first(),
            Some(&0),
            "grapheme start for {text:?}"
        );
        assert_eq!(
            layout.grapheme_boundaries.last(),
            Some(&text.len()),
            "grapheme end for {text:?}"
        );
        assert!(layout.paragraph.lines.iter().all(|line| {
            text.is_char_boundary(line.source_bytes.start)
                && text.is_char_boundary(line.source_bytes.end)
                && line.metrics.height_pt.is_finite()
                && line.metrics.height_pt > 0.0
        }));
        assert!(
            layout
                .paragraph
                .lines
                .windows(2)
                .all(|lines| { lines[0].consumed_bytes.end == lines[1].source_bytes.start })
        );
    }
}

#[test]
fn run_language_and_paragraph_environment_styles_reach_parley() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let mut paragraph = paragraph("Bold Thai text ภาษาไทย", Some("en"));
    let bold = InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::new(),
        children: vec![InlineNode::Run(Run {
            text: "Bold Thai text ภาษาไทย".to_owned(),
            language: Some("th-TH".to_owned()),
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    };
    paragraph.inlines = vec![InlineNode::Environment(bold)];
    let (text, result) = layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::default(),
            layout_options(120.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    assert_eq!(text, "Bold Thai text ภาษาไทย");
    assert!(result.paragraph.lines.len() > 1);
    assert!(
        result
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .any(|run| run.font_attrs().weight == fontique::FontWeight::BOLD)
    );
}

#[test]
fn missing_font_falls_back_for_mixed_scripts_and_emoji() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let text = "Caret 文稿 العربية Русский Ελληνικά 😀 👩‍👩‍👧‍👦";
    let style = ShapingStyle {
        requested_family: Some("Caret Missing Test Family 8D5A".to_owned()),
        ..ShapingStyle::default()
    };
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph(text, Some("en")),
            &settings,
            &style,
            layout_options(360.0),
        )
        .unwrap_or_else(|error| panic!("fallback shaping failed: {error}"));

    assert!(result.requested_family_missing);
    let glyphs = glyph_signature(&result.layout);
    assert!(!glyphs.is_empty());
    assert!(glyphs.iter().all(|glyph| glyph.0 != 0));
    assert!(
        result
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .any(|run| run.is_rtl())
    );
    assert!(result.paragraph.lines.iter().all(|line| {
        line.advance_pt.is_finite()
            && line.metrics.height_pt.is_finite()
            && line.metrics.height_pt > 0.0
    }));
    assert!(
        result
            .grapheme_boundaries
            .binary_search(&text.len())
            .is_ok()
    );

    for emoji_text in ["😀", "👩‍👩‍👧‍👦"] {
        let (_, emoji_layout) = layouter
            .layout(
                &mut catalog,
                &paragraph(emoji_text, Some("en")),
                &settings,
                &style,
                layout_options(120.0),
            )
            .unwrap_or_else(|error| panic!("emoji layout failed for {emoji_text:?}: {error}"));
        assert_eq!(emoji_layout.grapheme_boundaries, vec![0, emoji_text.len()]);
        let emoji_glyphs = glyph_signature(&emoji_layout.layout);
        assert!(!emoji_glyphs.is_empty());
        assert!(emoji_glyphs.iter().all(|glyph| glyph.0 != 0));
    }
}

#[test]
fn half_point_font_sizes_and_cyrillic_greek_keep_finite_layout_metrics() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let text = "Русский текст · Ελληνικά";
    let half_point = ShapingStyle {
        font_size_centipoints: 50,
        ..ShapingStyle::default()
    };
    let (_, half_point_layout) = layouter
        .layout(
            &mut catalog,
            &paragraph(text, Some("ru")),
            &settings,
            &half_point,
            layout_options(180.0),
        )
        .unwrap_or_else(|error| panic!("0.5 pt layout failed: {error}"));
    assert!(
        half_point_layout
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .all(|run| run.font_size() == 0.5)
    );
    assert!(!glyph_signature(&half_point_layout.layout).is_empty());
    assert!(
        half_point_layout
            .paragraph
            .lines
            .iter()
            .all(|line| { line.metrics.height_pt.is_finite() && line.metrics.height_pt > 0.0 })
    );

    let fractional_point = ShapingStyle {
        font_size_centipoints: 1225,
        ..ShapingStyle::default()
    };
    let (_, fractional_layout) = layouter
        .layout(
            &mut catalog,
            &paragraph("12.25 pt", Some("en")),
            &settings,
            &fractional_point,
            layout_options(180.0),
        )
        .unwrap_or_else(|error| panic!("12.25 pt layout failed: {error}"));
    assert!(
        fractional_layout
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .all(|run| run.font_size() == 12.25)
    );
}

#[test]
fn same_style_run_boundaries_preserve_positioned_glyphs() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let text = "office العربية e\u{301}";
    let mut split = paragraph("", None);
    split.inlines = vec![
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
    ];
    let (_, flat_layout) = layouter
        .layout(
            &mut catalog,
            &paragraph(text, None),
            &settings,
            &ShapingStyle::default(),
            layout_options(480.0),
        )
        .unwrap_or_else(|error| panic!("flat paragraph layout failed: {error}"));
    let (_, split_layout) = layouter
        .layout(
            &mut catalog,
            &split,
            &settings,
            &ShapingStyle::default(),
            layout_options(480.0),
        )
        .unwrap_or_else(|error| panic!("split-run paragraph layout failed: {error}"));

    assert_eq!(
        glyph_signature(&flat_layout.layout),
        glyph_signature(&split_layout.layout)
    );
    assert_eq!(flat_layout.paragraph.lines, split_layout.paragraph.lines);
}

#[test]
fn font_environment_and_nested_bold_reach_parley_style_ranges() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let family = catalog.families()[0].name.clone();
    let paragraph = Paragraph {
        id: NodeId::generate(),
        language: Some("en".to_owned()),
        layout: None,
        inlines: vec![
            InlineNode::Run(Run {
                text: "plain ".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }),
            InlineNode::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Font,
                definition_id: None,
                props: BTreeMap::from([(
                    "fontFamily".to_owned(),
                    serde_json::Value::String(family),
                )]),
                children: vec![InlineNode::Environment(InlineEnvironment {
                    id: NodeId::generate(),
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
    let mut layouter = ParagraphLayouter::new();
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::default(),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("nested style layout failed: {error}"));
    assert!(!result.requested_family_missing);
    assert!(
        result
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .any(|run| {
                run.text_range().start >= "plain ".len()
                    && run.font_attrs().weight == fontique::FontWeight::BOLD
            })
    );
}

#[test]
fn paragraph_layouter_rebuilds_parley_context_after_font_refresh() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let paragraph = paragraph("font cache refresh", Some("en"));
    let options = layout_options(240.0);
    layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::default(),
            options,
        )
        .unwrap_or_else(|error| panic!("initial Parley layout failed: {error}"));
    let previous_generation = catalog.generation();
    let refreshed_generation = catalog
        .refresh()
        .unwrap_or_else(|error| panic!("font catalog refresh failed: {error}"));
    assert!(refreshed_generation > previous_generation);
    layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::default(),
            options,
        )
        .unwrap_or_else(|error| panic!("Parley layout after refresh failed: {error}"));
}

#[test]
fn empty_and_trailing_break_lines_keep_positive_metrics() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let mut empty = paragraph("", Some("en"));
    empty.inlines.clear();
    let (_, empty_layout) = layouter
        .layout(
            &mut catalog,
            &empty,
            &settings,
            &ShapingStyle::default(),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("empty paragraph layout failed: {error}"));
    assert!(has_empty_line(""));
    assert_eq!(empty_layout.paragraph.lines.len(), 1);
    assert!(empty_layout.paragraph.total_height_pt > 0.0);
    let empty_cursor = empty_layout
        .cursor_at(0, parley::Affinity::Downstream)
        .unwrap_or_else(|| panic!("empty paragraph start is not a valid cursor"));
    let empty_cursor_geometry = empty_layout.cursor_geometry(empty_cursor, 1.0);
    assert_eq!(empty_cursor_geometry.x0, 0.0);
    assert_eq!(
        empty_cursor_geometry.y1 - empty_cursor_geometry.y0,
        f64::from(empty_layout.paragraph.lines[0].metrics.height_pt)
    );

    let trailing = paragraph("line\n", Some("en"));
    let (text, result) = layouter
        .layout(
            &mut catalog,
            &trailing,
            &settings,
            &ShapingStyle::default(),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("trailing break layout failed: {error}"));
    assert_eq!(text, "line\n");
    assert_eq!(result.paragraph.lines.len(), 2);
    assert_eq!(
        result.paragraph.lines[1].source_bytes,
        text.len()..text.len()
    );
}

#[test]
fn paragraph_empty_line_scan_handles_split_crlf_without_copying_text() {
    let mut paragraph = paragraph("", Some("en"));
    paragraph.inlines = vec![
        InlineNode::Run(Run {
            text: "first\r".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
        InlineNode::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Italic,
            definition_id: None,
            props: BTreeMap::new(),
            children: vec![InlineNode::Run(Run {
                text: "\nsecond".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        }),
    ];
    assert!(!paragraph_has_empty_line(&paragraph));
    paragraph
        .inlines
        .push(InlineNode::SoftBreak(caret_doc::SoftBreak {
            extra: BTreeMap::new(),
        }));
    assert!(paragraph_has_empty_line(&paragraph));
}

#[test]
fn parley_visual_cursor_moves_through_utf8_and_bidi_grapheme_boundaries() {
    use parley::editing::Cursor;

    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let text = "abc אבג 中文 e\u{301} 👩‍👩‍👧‍👦";
    let paragraph = paragraph(text, Some("en"));
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::default(),
            layout_options(320.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    let mut cursor = Cursor::from_byte_index(&result.layout, 0, parley::Affinity::Downstream);
    let mut visited = Vec::new();
    for _ in 0..text.len() * 4 {
        assert!(
            result
                .grapheme_boundaries
                .binary_search(&cursor.index())
                .is_ok()
        );
        if visited.contains(&(cursor.index(), cursor.affinity())) {
            break;
        }
        visited.push((cursor.index(), cursor.affinity()));
        let next = cursor.next_visual(&result.layout);
        if next == cursor {
            break;
        }
        cursor = next;
    }
    assert!(visited.len() > 2);
}

#[test]
fn caret_and_selection_geometry_never_split_extended_graphemes() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let text = "e\u{301} 👩‍👩‍👧‍👦";
    let paragraph = paragraph(text, Some("en"));
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::default(),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));

    assert!(result.cursor_at(1, parley::Affinity::Downstream).is_none());
    assert!(
        result
            .cursor_at("e\u{301}".len(), parley::Affinity::Downstream)
            .is_some()
    );
    let snapped = result
        .selection_geometry(1..2)
        .unwrap_or_else(|| panic!("valid selection was rejected"));
    let full_cluster = result
        .selection_geometry(0.."e\u{301}".len())
        .unwrap_or_else(|| panic!("valid grapheme selection was rejected"));
    assert!(!snapped.is_empty());
    assert_eq!(snapped, full_cluster);

    let line = result
        .layout
        .lines()
        .next()
        .unwrap_or_else(|| panic!("nonempty paragraph has no layout line"));
    let y = line.metrics().block_min_coord + line.metrics().line_height / 2.0;
    for x in (0..240).step_by(3) {
        let cursor = result
            .cursor_from_point(x as f32, y)
            .unwrap_or_else(|| panic!("hit at x={x} did not resolve to a cursor"));
        assert!(
            result
                .grapheme_boundaries
                .binary_search(&cursor.index())
                .is_ok(),
            "hit returned a non-grapheme byte offset {}",
            cursor.index()
        );
    }
}

#[test]
fn grapheme_boundaries_cross_run_and_environment_boundaries() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let mut paragraph = paragraph("", Some("en"));
    paragraph.inlines = vec![
        InlineNode::Run(Run {
            text: "e".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
        InlineNode::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: BTreeMap::new(),
            children: vec![InlineNode::Run(Run {
                text: "\u{301}".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        }),
    ];
    let (text, result) = layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::default(),
            layout_options(160.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    assert_eq!(text, "e\u{301}");
    assert_eq!(result.grapheme_boundaries, vec![0, text.len()]);
    assert!(result.cursor_at(1, parley::Affinity::Downstream).is_none());
    assert_eq!(result.paragraph.lines.len(), 1);
}

#[test]
fn unicode_17_bidi_character_corpus_checks_levels_and_visual_order() {
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
            _ => panic!("invalid bidi direction on line {}", line_number + 1),
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
            line_number + 1
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
            line_number + 1
        );
        for (actual, expected) in actual_levels.iter().zip(expected_levels.iter().copied()) {
            if expected != "x" {
                let expected = expected.parse::<u8>().unwrap_or_else(|error| {
                    panic!("invalid bidi level on line {}: {error}", line_number + 1)
                });
                assert_eq!(
                    *actual,
                    expected,
                    "resolved bidi level mismatch on corpus line {}",
                    line_number + 1
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
            line_number + 1
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
                codepoint => text.push(
                    char::from_u32(u32::from_str_radix(codepoint, 16).unwrap_or_else(|error| {
                        panic!(
                            "invalid scalar {codepoint:?} on line {}: {error}",
                            line_number + 1
                        )
                    }))
                    .unwrap_or_else(|| panic!("invalid scalar on line {}", line_number + 1)),
                ),
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
            "Unicode 17 {} mismatch on line {}: {line}",
            if grapheme { "grapheme" } else { "line break" },
            line_number + 1
        );
    }
}
