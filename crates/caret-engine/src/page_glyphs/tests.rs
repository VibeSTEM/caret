//! 验证页面字形快照、裁剪与栅格缓存。

use caret_doc::{
    Block, Document, InlineEnvironment, InlineEnvironmentKind, InlineNode, NodeId, Paragraph,
    ParagraphLineSpacing, Run,
};
use caret_layout::{FontCatalog, PositionedGlyphLayoutItem, ShapingStyle};

use crate::{DocumentLayoutService, DocumentLayoutSnapshot};

use super::{PageGlyphError, PageGlyphRasterizer};

fn text_run(text: &str, language: Option<&str>) -> InlineNode {
    InlineNode::Run(Run {
        text: text.to_owned(),
        language: language.map(str::to_owned),
        props: Default::default(),
        extra: Default::default(),
    })
}

fn document_with_text(text: &str) -> Document {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should contain a paragraph");
    };
    paragraph.inlines = vec![text_run(text, Some("en"))];
    document
}

fn document_with_multilingual_paragraphs() -> Document {
    let mut document = Document::blank();
    let Block::Paragraph(english) = document.body.remove(0) else {
        panic!("blank document should contain a paragraph");
    };
    let mut arabic = Paragraph::empty(NodeId::generate());
    arabic.language = Some("ar".to_owned());
    arabic.inlines = vec![text_run("العربية", None)];
    document.body = vec![Block::Paragraph(english), Block::Paragraph(arabic)];
    document
}

fn layout(document: &Document) -> DocumentLayoutSnapshot {
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    DocumentLayoutService::new()
        .layout(document, 17, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("document layout failed: {error}"))
}

#[test]
fn paragraph_glyphs_keep_one_anchor_across_mixed_scripts() {
    for text in [
        "English！",
        "！English",
        "English中文",
        "中文English",
        "العربية",
    ] {
        let snapshot = layout(&document_with_text(text));
        let page = snapshot
            .page_glyphs(0)
            .unwrap_or_else(|error| panic!("glyph extraction failed for {text:?}: {error}"));
        let first_baseline = page
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .next()
            .unwrap_or_else(|| panic!("paragraph has no glyphs: {text:?}"))
            .baseline_y_pt;
        assert!(
            page.runs
                .iter()
                .flat_map(|run| &run.glyphs)
                .all(|glyph| glyph.baseline_y_pt == first_baseline),
            "mixed scripts changed the paragraph baseline for {text:?}"
        );
    }

    let multilingual = layout(&document_with_multilingual_paragraphs());
    assert_eq!(multilingual.paragraphs.len(), 2);
}

#[test]
fn blank_page_has_no_glyphs_and_invalid_page_is_rejected() {
    let snapshot = layout(&Document::blank());
    let page = snapshot
        .page_glyphs(0)
        .unwrap_or_else(|error| panic!("blank page glyphs failed: {error}"));
    assert!(page.runs.is_empty());
    assert_eq!(page.page_index, 0);
    assert_eq!(page.document_generation, 17);
    assert_eq!(page.font_generation, snapshot.font_generation);
    assert_eq!(page.session_id, snapshot.session_id);
    assert_eq!(
        snapshot.page_glyphs(snapshot.pages.len()),
        Err(PageGlyphError::InvalidPageIndex)
    );
}

#[test]
fn natural_line_spacing_does_not_add_a_line_clip() {
    let snapshot = layout(&document_with_text("A tall line"));
    let page = snapshot
        .page_glyphs(0)
        .unwrap_or_else(|error| panic!("natural line glyphs failed: {error}"));
    assert!(!page.runs.is_empty());
    assert!(page.runs.iter().all(|run| run.line_clip_y_pt.is_none()));
}

#[test]
fn rasterized_page_deduplicates_images_and_preserves_device_phase() {
    let snapshot = layout(&document_with_text("A"));
    let mut page = snapshot
        .page_glyphs(0)
        .unwrap_or_else(|error| panic!("glyph extraction failed: {error}"));
    let repeated = page.runs[0].glyphs[0];
    page.runs[0].glyphs.push(repeated);
    let page_origin = [0.25, 0.75];
    let px_per_pt = 1.25;
    let mut rasterizer = PageGlyphRasterizer::default();
    let rasterized = rasterizer
        .rasterize(&page, px_per_pt, page_origin, [32, 32, 32, 255])
        .unwrap_or_else(|error| panic!("page glyph rasterization failed: {error}"));

    assert_eq!(rasterized.images.len(), 1);
    assert_eq!(rasterized.placements.len(), 2);
    assert_eq!(rasterized.placements[0].image_index, 0);
    assert_eq!(rasterized.placements[1].image_index, 0);
    assert_eq!(
        rasterized.placements[0].pixel_phase,
        rasterized.placements[1].pixel_phase
    );
    assert_eq!(
        rasterized.placements[0].pixel_phase[0],
        (page_origin[0] + repeated.x_pt * px_per_pt).rem_euclid(1.0) as f32
    );
    assert!(rasterized.images[0].pixels.len() > 4);
    assert!(rasterized.images[0].pixels.as_chunks::<4>().1.is_empty());
    assert_eq!(rasterized.document_generation, page.document_generation);
    assert_eq!(rasterized.font_generation, page.font_generation);
    assert_eq!(rasterized.page_index, page.page_index);

    assert_eq!(
        rasterizer.rasterize(&page, f64::NAN, page_origin, [0, 0, 0, 255]),
        Err(PageGlyphError::InvalidRasterGeometry)
    );
}

#[test]
fn near_integer_device_origins_keep_phases_inside_the_valid_interval() {
    let snapshot = layout(&document_with_text("A"));
    let page = snapshot
        .page_glyphs(0)
        .unwrap_or_else(|error| panic!("glyph extraction failed: {error}"));
    let mut rasterizer = PageGlyphRasterizer::default();
    for origin_x in [-1.0e-9, 1.0e-9, -1.000_000_001, 1.000_000_001] {
        let rasterized = rasterizer
            .rasterize(&page, 1.0, [origin_x, 0.0], [32, 32, 32, 255])
            .unwrap_or_else(|error| panic!("near integer phase failed: {error}"));
        assert!(!rasterized.placements.is_empty());
        assert!(rasterized.placements.iter().all(|placement| {
            placement
                .pixel_phase
                .iter()
                .all(|phase| (0.0..1.0).contains(phase))
        }));
    }
}

#[test]
fn page_deduplication_survives_global_raster_cache_eviction() {
    let snapshot = layout(&document_with_text("A"));
    let mut page = snapshot
        .page_glyphs(0)
        .unwrap_or_else(|error| panic!("glyph extraction failed: {error}"));
    let first = page.runs[0].glyphs[0];
    let mut glyphs = Vec::with_capacity(8194);
    for phase_index in 0..8193 {
        let mut glyph = first;
        glyph.x_pt = f64::from(phase_index) / 8193.0;
        glyphs.push(glyph);
    }
    glyphs.push(first);
    page.runs[0].glyphs = glyphs;
    let mut rasterizer = PageGlyphRasterizer::default();
    let rasterized = rasterizer
        .rasterize(&page, 1.0, [0.0, 0.0], [32, 32, 32, 255])
        .unwrap_or_else(|error| panic!("page glyph rasterization failed: {error}"));

    assert_eq!(rasterized.placements.len(), 8194);
    assert_eq!(rasterized.images.len(), 8193);
    assert_eq!(
        rasterized
            .placements
            .first()
            .map(|placement| placement.image_index),
        rasterized
            .placements
            .last()
            .map(|placement| placement.image_index)
    );
}

#[test]
fn paged_glyphs_reset_baselines_and_visit_only_fragment_lines() {
    let mut document = document_with_text(&"A\n".repeat(150));
    document.global_settings.paragraph_layout.line_spacing = ParagraphLineSpacing::Exactly {
        centipoints: 1000,
        extra: Default::default(),
    };
    let snapshot = layout(&document);
    assert!(snapshot.pages.len() >= 3);
    let paragraph = &snapshot.paragraphs[0];
    let source_count: usize = paragraph
        .layout
        .layout
        .lines()
        .flat_map(|line| line.items())
        .filter_map(|item| match item {
            PositionedGlyphLayoutItem::GlyphRun(run) => Some(run.positioned_glyphs().count()),
            PositionedGlyphLayoutItem::InlineBox(_) => None,
        })
        .sum();
    let mut extracted_count = 0;
    for (page_index, page_layout) in snapshot.pages.iter().enumerate() {
        let page = snapshot
            .page_glyphs(page_index)
            .unwrap_or_else(|error| panic!("paged glyphs failed: {error}"));
        assert!(!page.runs.is_empty());
        assert!(page.runs.iter().all(|run| {
            run.line_clip_y_pt.is_some()
                && page_layout.layout.fragments.iter().any(|fragment| {
                    fragment.paragraph_id == run.paragraph_id
                        && fragment.line_range.contains(&run.line_index)
                })
        }));
        let first_run = &page.runs[0];
        let fragment = &page_layout.layout.fragments[0];
        let caret_line = &paragraph.layout.paragraph.lines[first_run.line_index];
        let expected_baseline = page_layout.margin_top_pt
            + f64::from(fragment.y_offset_pt)
            + f64::from(caret_line.metrics.baseline_pt);
        assert!(
            (first_run.glyphs[0].baseline_y_pt - expected_baseline).abs() < 0.0001,
            "page {page_index} baseline mismatch"
        );
        if page_index > 0 {
            assert!(first_run.line_index > 0);
        }
        extracted_count += page.runs.iter().map(|run| run.glyphs.len()).sum::<usize>();
    }
    assert_eq!(extracted_count, source_count);
}

#[test]
fn unicode_visual_runs_keep_exact_font_instances_and_glyph_order() {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should contain a paragraph");
    };
    paragraph.inlines = vec![
        text_run("Latin e\u{301} ", Some("en")),
        InlineNode::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: Default::default(),
            children: vec![text_run("אבג 中文 😀", Some("he"))],
            extra: Default::default(),
        }),
    ];
    let snapshot = layout(&document);
    let page = snapshot
        .page_glyphs(0)
        .unwrap_or_else(|error| panic!("mixed glyphs failed: {error}"));
    assert!(page.runs.len() > 1);
    let source = &snapshot.paragraphs[0].layout.layout;
    let expected_runs = source
        .lines()
        .enumerate()
        .flat_map(|(line_index, line)| {
            line.items().filter_map(move |item| match item {
                PositionedGlyphLayoutItem::GlyphRun(run) => Some((line_index, run)),
                PositionedGlyphLayoutItem::InlineBox(_) => None,
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(page.runs.len(), expected_runs.len());
    for (actual, (line_index, expected)) in page.runs.iter().zip(expected_runs) {
        let source_run = expected.run();
        assert_eq!(actual.line_index, line_index);
        assert_eq!(actual.font.data.id(), source_run.font().data.id());
        assert_eq!(actual.font.index, source_run.font().index);
        assert_eq!(actual.font_size_pt, source_run.font_size());
        assert_eq!(actual.synthesis, source_run.synthesis());
        assert_eq!(actual.foreground, None);
        assert_eq!(
            actual.normalized_coords,
            source_run
                .normalized_coords()
                .iter()
                .map(|coordinate| coordinate.to_bits())
                .collect::<Vec<_>>()
        );
        let source_glyphs = expected.positioned_glyphs().collect::<Vec<_>>();
        assert_eq!(actual.glyphs.len(), source_glyphs.len());
        for (glyph, source_glyph) in actual.glyphs.iter().zip(source_glyphs) {
            assert_eq!(glyph.glyph_id, source_glyph.id);
            assert_eq!(
                glyph.x_pt,
                snapshot.pages[0].margin_left_pt + f64::from(source_glyph.x)
            );
        }
    }
}

#[test]
fn long_paragraph_keeps_local_combining_glyph_offsets() {
    let mut text = "A\n".repeat(10_000);
    text.push_str("x\u{301}\u{301}\u{301} شَ بِ שָׁ");
    let snapshot = layout(&document_with_text(&text));
    let page_index = snapshot.pages.len() - 1;
    let page = snapshot
        .page_glyphs(page_index)
        .unwrap_or_else(|error| panic!("last-page glyphs failed: {error}"));
    let page_layout = &snapshot.pages[page_index];
    let paragraph = &snapshot.paragraphs[0];
    let line_index = paragraph.layout.paragraph.lines.len() - 1;
    let line = paragraph
        .layout
        .layout
        .get(line_index)
        .unwrap_or_else(|| panic!("last Parley line is missing"));
    let fragment = page_layout
        .layout
        .fragments
        .iter()
        .find(|fragment| fragment.line_range.contains(&line_index))
        .unwrap_or_else(|| panic!("last-page fragment is missing"));
    let line_top_pt = page_layout.margin_top_pt
        + f64::from(fragment.y_offset_pt)
        + paragraph.layout.paragraph.lines[fragment.line_range.start..line_index]
            .iter()
            .map(|line| f64::from(line.metrics.height_pt))
            .sum::<f64>();
    let caret_baseline_pt = f64::from(
        paragraph.layout.paragraph.lines[line_index]
            .metrics
            .baseline_pt,
    );
    let actual_runs = page
        .runs
        .iter()
        .filter(|run| run.line_index == line_index)
        .collect::<Vec<_>>();
    let expected_runs = line
        .items()
        .filter_map(|item| match item {
            PositionedGlyphLayoutItem::GlyphRun(run) => Some(run),
            PositionedGlyphLayoutItem::InlineBox(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(actual_runs.len(), expected_runs.len());
    let mut has_local_y_offset = false;
    for (actual_run, expected_run) in actual_runs.into_iter().zip(expected_runs) {
        let baseline_delta_pt =
            f64::from(expected_run.baseline()) - f64::from(line.metrics().baseline);
        for (actual, local) in actual_run.glyphs.iter().zip(expected_run.glyphs()) {
            has_local_y_offset |= local.y != 0.0;
            let expected_y =
                line_top_pt + caret_baseline_pt + baseline_delta_pt + f64::from(local.y);
            assert!(
                (actual.baseline_y_pt - expected_y).abs() < 0.0001,
                "local glyph offset changed at paragraph line {line_index}"
            );
        }
    }
    assert!(has_local_y_offset);
}

#[test]
fn invalid_fragment_or_glyph_source_rejects_the_page_atomically() {
    let mut snapshot = layout(&document_with_text("A valid source"));
    let original = snapshot.clone();
    snapshot.pages[0].layout.fragments[0].line_range = 0..usize::MAX;
    assert_eq!(
        snapshot.page_glyphs(0),
        Err(PageGlyphError::InvalidLineRange)
    );

    snapshot = original.clone();
    snapshot.pages[0].layout.fragments[0].paragraph_id = NodeId::generate();
    assert_eq!(
        snapshot.page_glyphs(0),
        Err(PageGlyphError::MissingParagraph)
    );

    snapshot = original.clone();
    std::sync::Arc::make_mut(&mut snapshot.paragraphs[0].layout).baseline_adjustments_pt[0] =
        f32::NAN;
    assert_eq!(
        snapshot.page_glyphs(0),
        Err(PageGlyphError::InvalidLineGeometry)
    );

    snapshot = original;
    std::sync::Arc::make_mut(&mut snapshot.paragraphs[0].layout).layout = Default::default();
    assert_eq!(
        snapshot.page_glyphs(0),
        Err(PageGlyphError::InvalidGlyphSource)
    );
}

fn parsed_tree(source: &str, kind: caret_math::MathInputKind) -> caret_math::ParsedMath {
    caret_math::parse(source, kind).unwrap_or_else(|error| panic!("{source}: {error:?}"))
}

fn inline_math(source: &str) -> InlineNode {
    let caret_math::ParsedMath::Tree(math_tree) =
        parsed_tree(source, caret_math::MathInputKind::Inline)
    else {
        panic!("inline math must parse to a tree");
    };
    InlineNode::Math(caret_doc::InlineMath {
        id: NodeId::generate(),
        math_tree,
        extra: Default::default(),
    })
}

fn math_runs(snapshot: &super::PageGlyphSnapshot) -> Vec<&super::PageGlyphRun> {
    let math_font = super::math_font_data();
    snapshot
        .runs
        .iter()
        .filter(|run| run.font == *math_font)
        .collect()
}

#[test]
fn inline_math_occupies_its_box_and_emits_math_glyphs_on_the_text_baseline() {
    let mut document = document_with_text("x");
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should contain a paragraph");
    };
    paragraph.inlines = vec![
        text_run("a ", Some("en")),
        inline_math("\\frac{1}{2}+y"),
        text_run(" b", Some("en")),
    ];
    let snapshot = layout(&document);
    let paragraph = &snapshot.paragraphs[0];
    assert_eq!(paragraph.math_boxes.len(), 1);
    let math = &paragraph.math_boxes[0];
    assert_eq!(math.source_bytes, 2..5);
    assert!(math.layout.width > 10.0);
    let page = snapshot
        .page_glyphs(0)
        .unwrap_or_else(|error| panic!("page glyphs failed: {error}"));
    let runs = math_runs(&page);
    assert!(
        !runs.is_empty(),
        "math glyphs must be emitted with the math font"
    );
    // 分数线作为装饰矩形输出。
    assert!(!page.decorations.is_empty());
    // 占位字符本身不绘制；正文字形 'a' 与 'b' 位于公式盒两侧。
    let text_runs: Vec<_> = page
        .runs
        .iter()
        .filter(|run| run.font != *super::math_font_data())
        .collect();
    let text_glyphs: Vec<_> = text_runs.iter().flat_map(|run| run.glyphs.iter()).collect();
    assert_eq!(text_glyphs.len(), 4, "'a', space, space, 'b'");
    let math_x: Vec<_> = runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.x_pt))
        .collect();
    let min_math_x = math_x.iter().copied().fold(f64::INFINITY, f64::min);
    let max_math_x = math_x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let first_text_x = text_glyphs.first().map_or(0.0, |glyph| glyph.x_pt);
    let last_text_x = text_glyphs.last().map_or(0.0, |glyph| glyph.x_pt);
    assert!(first_text_x < min_math_x);
    assert!(last_text_x > max_math_x);
    // '+' 与 'y' 位于正文基线上（容差 0.01pt）。
    let text_baseline = text_glyphs.first().map_or(0.0, |glyph| glyph.baseline_y_pt);
    let on_baseline = runs
        .iter()
        .flat_map(|run| run.glyphs.iter())
        .filter(|glyph| (glyph.baseline_y_pt - text_baseline).abs() < 1e-2)
        .count();
    assert!(on_baseline >= 2);
}

#[test]
fn equation_and_align_blocks_lay_out_as_centered_display_paragraphs() {
    let mut document = Document::blank();
    let caret_math::ParsedMath::Tree(math_tree) =
        parsed_tree("\\sum_{i=1}^{n} i", caret_math::MathInputKind::Equation)
    else {
        panic!("equation must parse to a tree");
    };
    let equation_id = NodeId::generate();
    document.body.push(Block::Equation(caret_doc::Equation {
        id: equation_id.clone(),
        math_tree,
        extra: Default::default(),
    }));
    let caret_math::ParsedMath::Align(align) = parsed_tree(
        r"\[\begin{aligned} a &= b \\ c &= d \end{aligned}\]",
        caret_math::MathInputKind::Align,
    ) else {
        panic!("align must parse to rows");
    };
    let align_id = align.id.clone();
    document.body.push(Block::Align(align));
    let snapshot = layout(&document);
    let equation = snapshot
        .paragraph_by_id(&equation_id)
        .unwrap_or_else(|| panic!("equation paragraph missing"));
    assert!(equation.display_math);
    assert!(equation.environment_ids.contains(&equation_id));
    let align = snapshot
        .paragraph_by_id(&align_id)
        .unwrap_or_else(|| panic!("align paragraph missing"));
    assert!(align.display_math);
    // 两行对齐公式的盒高大于单行显示公式的行高的一半以上。
    let align_box = &align.math_boxes[0];
    assert!(align_box.layout.ascent + align_box.layout.descent > 20.0);
    let page = snapshot
        .page_glyphs(0)
        .unwrap_or_else(|error| panic!("page glyphs failed: {error}"));
    let page_layout = &snapshot.pages[0];
    let center = page_layout.margin_left_pt
        + (page_layout.width_pt - page_layout.margin_left_pt - page_layout.margin_right_pt) / 2.0;
    for id in [&equation_id, &align_id] {
        let xs: Vec<_> = math_runs(&page)
            .into_iter()
            .filter(|run| run.paragraph_id == *id)
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.x_pt))
            .collect();
        assert!(!xs.is_empty());
        let min = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let max = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(
            min < center && max > center,
            "display math must be centered: {min}..{max} vs {center}"
        );
    }
}
