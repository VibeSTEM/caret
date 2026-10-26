//! 覆盖 Parley 高层布局、Caret Unicode tailoring、字体偏好及跨 Run 行为。

use std::collections::BTreeMap;

use caret_doc::{
    DocumentSettings, InlineEnvironment, InlineEnvironmentKind, InlineNode, NodeId, Paragraph, Run,
};
use parley_engine::{Analysis, AnalysisOptions, Analyzer, BaseDirection};

use crate::{
    FontCatalog, FontLineMetrics, GlyphRasterOptions, GlyphRasterizer, LineSpacing,
    ParagraphLayoutOptions, ParagraphLayouter, ResolvedDocumentTextSettings, ShapingStyle,
    UNICODE_DATA_VERSION, UnicodeBoundaries, has_empty_line, paragraph_has_empty_line,
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
fn swash_rasterizes_the_font_face_and_glyph_selected_by_parley() {
    let mut catalog = font_catalog();
    let mut layouter = ParagraphLayouter::new();
    let (_, layout) = layouter
        .layout(
            &mut catalog,
            &paragraph("Raster", Some("en")),
            &document_settings(),
            &ShapingStyle::default(),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    let (run, glyph) = layout
        .layout
        .lines()
        .flat_map(|line| line.items())
        .find_map(|item| match item {
            parley::PositionedLayoutItem::GlyphRun(run) => {
                let glyph = run.clone().positioned_glyphs().next();
                glyph.map(|glyph| (run, glyph))
            }
            parley::PositionedLayoutItem::InlineBox(_) => None,
        })
        .unwrap_or_else(|| panic!("Parley layout should contain a glyph"));
    let normalized_coords: Vec<_> = run
        .run()
        .normalized_coords()
        .iter()
        .map(|coordinate| coordinate.to_bits())
        .collect();
    let mut rasterizer = GlyphRasterizer::default();
    let bitmap = rasterizer
        .rasterize(
            run.run().font(),
            glyph.id,
            GlyphRasterOptions {
                pixels_per_em: run.run().font_size() * 96.0 / 72.0,
                pixel_phase: [0.0, 0.0],
                normalized_coords: &normalized_coords,
                synthesis: run.run().synthesis(),
                foreground: [28, 28, 28, 255],
            },
        )
        .unwrap_or_else(|| panic!("Swash should rasterize Parley's glyph"));
    let cached_bitmap = rasterizer
        .rasterize(
            run.run().font(),
            glyph.id,
            GlyphRasterOptions {
                pixels_per_em: run.run().font_size() * 96.0 / 72.0,
                pixel_phase: [0.0, 0.0],
                normalized_coords: &normalized_coords,
                synthesis: run.run().synthesis(),
                foreground: [28, 28, 28, 255],
            },
        )
        .unwrap_or_else(|| panic!("cached glyph should remain available"));
    assert!(std::sync::Arc::ptr_eq(&bitmap, &cached_bitmap));
    assert!(bitmap.width > 0);
    assert!(bitmap.height > 0);
    assert!(
        bitmap
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0)
    );
    let pixel_count = usize::try_from(bitmap.width)
        .unwrap_or_else(|error| panic!("glyph width is invalid: {error}"))
        .checked_mul(
            usize::try_from(bitmap.height)
                .unwrap_or_else(|error| panic!("glyph height is invalid: {error}")),
        )
        .unwrap_or_else(|| panic!("glyph size overflows"));
    assert_eq!(bitmap.pixels.len(), pixel_count * 4);
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
        .cursor_at(0, crate::VisualAffinity::Downstream)
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
    let mut cursor = result
        .cursor_at(0, crate::VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("paragraph start is not a visual cursor"));
    let mut visited = Vec::new();
    for _ in 0..text.len() * 4 {
        assert!(
            result
                .grapheme_boundaries
                .binary_search(&cursor.byte_index())
                .is_ok()
        );
        if visited.contains(&(cursor.byte_index(), cursor.affinity())) {
            break;
        }
        visited.push((cursor.byte_index(), cursor.affinity()));
        let Some(next) = result.move_visual(cursor, true) else {
            break;
        };
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

    assert!(
        result
            .cursor_at(1, crate::VisualAffinity::Downstream)
            .is_none()
    );
    assert!(
        result
            .cursor_at("e\u{301}".len(), crate::VisualAffinity::Downstream)
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
        let hit = result
            .cursor_from_point(x as f32, y)
            .unwrap_or_else(|| panic!("hit at x={x} did not resolve to a cursor"));
        assert!(
            result
                .grapheme_boundaries
                .binary_search(&hit.cursor.byte_index())
                .is_ok(),
            "hit returned a non-grapheme byte offset {}",
            hit.cursor.byte_index()
        );
    }
}

#[test]
fn exact_ltr_cluster_boundary_prefers_logical_downstream() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph("ab", Some("en")),
            &settings,
            &ShapingStyle::default(),
            layout_options(160.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    let upstream = result
        .cursor_at(1, crate::VisualAffinity::Upstream)
        .unwrap_or_else(|| panic!("upstream boundary cursor missing"));
    let downstream = result
        .cursor_at(1, crate::VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("downstream boundary cursor missing"));
    let upstream_rect = result.cursor_geometry(upstream, 1.0);
    let downstream_rect = result.cursor_geometry(downstream, 1.0);
    assert_eq!(upstream_rect.x0, downstream_rect.x0);
    let hit = result
        .cursor_from_point_on_line(upstream_rect.x0 as f32, 0)
        .unwrap_or_else(|| panic!("exact boundary hit did not resolve"));
    assert_eq!(hit.cursor.byte_index(), 1);
    assert_eq!(hit.cursor.affinity(), crate::VisualAffinity::Downstream);
    assert_eq!(hit.logical_affinity, crate::HitAffinity::Downstream);
}

#[test]
fn hit_testing_preserves_logical_cluster_side_for_ltr_and_rtl_at_small_sizes() {
    for font_size_centipoints in [50, 1200] {
        let mut catalog = font_catalog();
        let settings = document_settings();
        let style = ShapingStyle {
            font_size_centipoints,
            ..ShapingStyle::default()
        };
        let mut layouter = ParagraphLayouter::new();
        for (text, language, expected_left, expected_right) in [
            (
                "abc",
                "en",
                crate::HitAffinity::Upstream,
                crate::HitAffinity::Downstream,
            ),
            (
                "אבג",
                "he",
                crate::HitAffinity::Downstream,
                crate::HitAffinity::Upstream,
            ),
        ] {
            let (_, result) = layouter
                .layout(
                    &mut catalog,
                    &paragraph(text, Some(language)),
                    &settings,
                    &style,
                    layout_options(160.0),
                )
                .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
            let upstream = result
                .cursor_at(2, crate::VisualAffinity::Upstream)
                .unwrap_or_else(|| panic!("upstream boundary cursor missing"));
            let downstream = result
                .cursor_at(2, crate::VisualAffinity::Downstream)
                .unwrap_or_else(|| panic!("downstream boundary cursor missing"));
            let upstream_x = result.cursor_geometry(upstream, 0.0).x0;
            let downstream_x = result.cursor_geometry(downstream, 0.0).x0;
            assert_eq!(upstream_x, downstream_x);
            let epsilon = f32::from(font_size_centipoints) * 0.0001;
            let cases = [
                ((upstream_x - f64::from(epsilon)) as f32, expected_left),
                ((upstream_x + f64::from(epsilon)) as f32, expected_right),
                (upstream_x as f32, crate::HitAffinity::Downstream),
            ];
            for (x, expected_affinity) in cases {
                let hit = result
                    .cursor_from_point_on_line(x, 0)
                    .unwrap_or_else(|| panic!("hit at x={x} did not resolve"));
                assert_eq!(
                    hit.cursor.byte_index(),
                    2,
                    "text={text:?}, size={font_size_centipoints}"
                );
                assert_eq!(
                    hit.logical_affinity, expected_affinity,
                    "text={text:?}, size={font_size_centipoints}, x={x}"
                );
            }
        }
    }
}

#[test]
fn hit_testing_keeps_visual_sides_at_bidi_boundaries() {
    for font_size_centipoints in [50, 1200] {
        let mut catalog = font_catalog();
        let settings = document_settings();
        let style = ShapingStyle {
            font_size_centipoints,
            ..ShapingStyle::default()
        };
        let mut layouter = ParagraphLayouter::new();
        let (_, result) = layouter
            .layout(
                &mut catalog,
                &paragraph("aאבb", Some("en")),
                &settings,
                &style,
                layout_options(160.0),
            )
            .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
        let upstream = result
            .cursor_at(1, crate::VisualAffinity::Upstream)
            .unwrap_or_else(|| panic!("upstream bidi cursor missing"));
        let downstream = result
            .cursor_at(1, crate::VisualAffinity::Downstream)
            .unwrap_or_else(|| panic!("downstream bidi cursor missing"));
        let upstream_x = result.cursor_geometry(upstream, 0.0).x0;
        let downstream_x = result.cursor_geometry(downstream, 0.0).x0;
        assert_ne!(upstream_x, downstream_x);
        let epsilon = f32::from(font_size_centipoints) * 0.0001;
        for (x, expected_visual) in [
            (
                (upstream_x - f64::from(epsilon)) as f32,
                crate::VisualAffinity::Upstream,
            ),
            (
                (downstream_x - f64::from(epsilon)) as f32,
                crate::VisualAffinity::Downstream,
            ),
        ] {
            let hit = result
                .cursor_from_point_on_line(x, 0)
                .unwrap_or_else(|| panic!("bidi hit at x={x} did not resolve"));
            assert_eq!(hit.cursor.byte_index(), 1);
            assert_eq!(hit.cursor.affinity(), expected_visual);
        }
    }
}

#[test]
fn multiline_bidi_hit_and_cursor_geometry_stay_on_the_selected_line() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph("abc אבג def", Some("en")),
            &settings,
            &ShapingStyle::default(),
            layout_options(24.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    assert!(result.paragraph.lines.len() > 1);

    let hit = result
        .cursor_from_point_on_line(0.0, 1)
        .unwrap_or_else(|| panic!("second-line bidi hit did not resolve"));
    assert_eq!(hit.cursor.byte_index(), 10);
    assert_eq!(hit.logical_affinity, crate::HitAffinity::Upstream);

    let bounds = result.cursor_geometry(hit.cursor, 1.0);
    let second_line_top = f64::from(result.line_box_tops_pt[1]);
    let second_line_bottom =
        second_line_top + f64::from(result.paragraph.lines[1].metrics.height_pt);
    let cursor_mid_y = (bounds.y0 + bounds.y1) / 2.0;
    assert!(
        (second_line_top..=second_line_bottom).contains(&cursor_mid_y),
        "cursor y={cursor_mid_y} is outside second line {second_line_top}..{second_line_bottom}"
    );
}

#[test]
fn hard_line_break_caret_geometry_moves_to_the_following_or_trailing_line() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let cases = [
        ("line\n", 5, 1),
        ("a\nb", 2, 1),
        ("a\r\nb", 3, 1),
        ("\n\n", 2, 2),
        ("אבג\nדהו", 7, 1),
    ];
    for (text, offset, expected_line) in cases {
        let mut layouter = ParagraphLayouter::new();
        let (_, result) = layouter
            .layout(
                &mut catalog,
                &paragraph(text, Some("en")),
                &settings,
                &ShapingStyle::default(),
                layout_options(160.0),
            )
            .unwrap_or_else(|error| panic!("Parley layout failed for {text:?}: {error}"));
        assert!(result.paragraph.lines.len() > expected_line);
        let cursor = result
            .cursor_at(offset, crate::VisualAffinity::Upstream)
            .unwrap_or_else(|| panic!("cursor at {offset} missing for {text:?}"));
        let bounds = result.cursor_geometry(cursor, 1.0);
        let parley_line = result
            .layout
            .lines()
            .nth(expected_line)
            .unwrap_or_else(|| panic!("Parley line {expected_line} missing for {text:?}"));
        let parley_line_metrics = parley_line.metrics();
        let expected_x = if text.starts_with('א') {
            parley_line_metrics.offset + parley_line_metrics.advance
        } else {
            parley_line_metrics.offset
        };
        assert_eq!(bounds.x0, f64::from(expected_x), "{text:?}");
        let expected_top = f64::from(result.line_box_tops_pt[expected_line]);
        let expected_bottom =
            expected_top + f64::from(result.paragraph.lines[expected_line].metrics.height_pt);
        let cursor_mid_y = (bounds.y0 + bounds.y1) / 2.0;
        assert!(
            (expected_top..=expected_bottom).contains(&cursor_mid_y),
            "{text:?}: cursor y={cursor_mid_y} is outside expected line {expected_line} ({expected_top}..{expected_bottom})"
        );
    }
}

#[test]
fn hit_testing_snaps_combining_cluster_to_its_logical_edges() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let text = "e\u{301}";
    let mut split = paragraph("", Some("en"));
    split.inlines = vec![
        InlineNode::Run(Run {
            text: "e".to_owned(),
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
    let mut layouter = ParagraphLayouter::new();
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &split,
            &settings,
            &ShapingStyle {
                font_size_centipoints: 50,
                ..ShapingStyle::default()
            },
            layout_options(160.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    assert_eq!(text, "e\u{301}");
    let start = result
        .cursor_at(0, crate::VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("cluster start cursor missing"));
    let end = result
        .cursor_at(text.len(), crate::VisualAffinity::Upstream)
        .unwrap_or_else(|| panic!("cluster end cursor missing"));
    let start_x = result.cursor_geometry(start, 0.0).x0;
    let end_x = result.cursor_geometry(end, 0.0).x0;
    let epsilon = (end_x - start_x).abs() * 0.1;
    let start_hit = result
        .cursor_from_point_on_line((start_x + epsilon) as f32, 0)
        .unwrap_or_else(|| panic!("cluster start hit did not resolve"));
    let end_hit = result
        .cursor_from_point_on_line((end_x - epsilon) as f32, 0)
        .unwrap_or_else(|| panic!("cluster end hit did not resolve"));
    assert_eq!(start_hit.cursor.byte_index(), 0);
    assert_eq!(start_hit.logical_affinity, crate::HitAffinity::Downstream);
    assert_eq!(end_hit.cursor.byte_index(), text.len());
    assert_eq!(end_hit.logical_affinity, crate::HitAffinity::Upstream);
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
    assert!(
        result
            .cursor_at(1, crate::VisualAffinity::Downstream)
            .is_none()
    );
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
