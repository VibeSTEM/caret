//! 测量 Parley 段落布局、命中测试与字形栅格缓存的 CPU 基线。

use caret_doc::{DocumentSettings, NodeId, Paragraph, ParagraphContent, Run, TextFlow};
use caret_layout::{
    FontCatalog, FontLineMetrics, GlyphRasterOptions, GlyphRasterizer, LineSpacing,
    ParagraphLayoutOptions, ParagraphLayouter, ResolvedDocumentTextSettings, ShapingStyle,
    VisualAffinity,
};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::collections::BTreeMap;
use std::hint::black_box;

const BASE_FONT_SIZE_CENTIPOINTS: u16 = 1200;
const PAGE_WIDTH_POINTS: f32 = 468.0;
const LATIN_FRAGMENT: &str =
    "Caret paragraphs preserve source text while shaping multilingual text into visual lines. ";
const MIXED_FRAGMENT: &str = "Caret 编辑器文字 shaping العربية Ελληνικά Кириллица 👩‍🔬。";

fn text_flow(run: Run) -> ParagraphContent {
    ParagraphContent::Text(TextFlow {
        runs: vec![run],
        source_range: None,
        extra: BTreeMap::new(),
    })
}

fn paragraph(text: String, language: Option<&str>) -> Paragraph {
    Paragraph {
        id: NodeId::generate(),
        language: language.map(str::to_owned),
        layout: None,
        inlines: vec![text_flow(Run {
            text,
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }
}

fn document_settings() -> ResolvedDocumentTextSettings {
    ResolvedDocumentTextSettings::from_document_settings(&DocumentSettings::default())
        .unwrap_or_else(|error| panic!("文档字体设置无效：{error}"))
}

fn layout_options() -> ParagraphLayoutOptions {
    let metrics = FontLineMetrics {
        ascent_pt: 9.6,
        descent_pt: 2.4,
        leading_pt: 0.0,
    };
    ParagraphLayoutOptions {
        width_pt: PAGE_WIDTH_POINTS,
        spacing: LineSpacing::Natural,
        line_grid_metrics: metrics,
        line_grid_font_size_centipoints: BASE_FONT_SIZE_CENTIPOINTS,
        first_line_indent_chars: None,
    }
}

fn benchmark_paragraph_layout(criterion: &mut Criterion) {
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("无法读取系统字体目录：{error}"));
    let settings = document_settings();
    let style = ShapingStyle::new(BASE_FONT_SIZE_CENTIPOINTS);
    let options = layout_options();
    let mut group = criterion.benchmark_group("rust/paragraph_layout");
    for (name, fragment, repeats, language) in [
        ("latin_8k", LATIN_FRAGMENT, 96, Some("en-US")),
        ("mixed_scripts_8k", MIXED_FRAGMENT, 96, None),
        ("space_dense_100k", "ab cd ", 16666, Some("en-US")),
        ("latin_spaces_100k", LATIN_FRAGMENT, 1152, Some("en-US")),
    ] {
        let text = fragment.repeat(repeats);
        let source = paragraph(text, language);
        group.throughput(Throughput::Bytes(
            source
                .inlines
                .iter()
                .map(|node| match node {
                    ParagraphContent::Text(flow) => {
                        flow.runs.iter().map(|run| run.text.len() as u64).sum()
                    }
                    _ => 0,
                })
                .sum(),
        ));
        group.bench_with_input(
            BenchmarkId::new("shape_and_layout", name),
            &source,
            |bencher, paragraph| {
                let mut layouter = ParagraphLayouter::new();
                bencher.iter(|| {
                    let result = layouter.layout(
                        &mut catalog,
                        black_box(paragraph),
                        &settings,
                        &style,
                        options,
                    );
                    black_box(result.unwrap_or_else(|error| panic!("段落布局失败：{error}")))
                });
            },
        );
    }
    group.finish();
}

fn benchmark_hit_testing(criterion: &mut Criterion) {
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("无法读取系统字体目录：{error}"));
    let mut layouter = ParagraphLayouter::new();
    let settings = document_settings();
    let paragraph = paragraph(MIXED_FRAGMENT.repeat(64), None);
    let (_, layout) = layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::new(BASE_FONT_SIZE_CENTIPOINTS),
            layout_options(),
        )
        .unwrap_or_else(|error| panic!("命中测试基准布局失败：{error}"));
    let line_count = layout.paragraph.lines.len();
    let line_index = line_count / 2;
    let top = layout.parley_line_tops_pt[line_index];
    let height = layout.parley_line_heights_pt[line_index];
    let y = top + height * 0.5;
    let mut sample = 0usize;
    criterion.bench_function("rust/hit_test/mixed_script_paragraph", |bencher| {
        bencher.iter(|| {
            sample = (sample + 1) % 64;
            black_box(layout.cursor_from_point(240.0 + sample as f32, y))
        });
    });
    criterion.bench_function("rust/caret_geometry/mixed_script_paragraph", |bencher| {
        let byte_index = layout
            .grapheme_boundaries
            .get(layout.grapheme_boundaries.len() / 2)
            .copied()
            .unwrap_or(0);
        let cursor = layout
            .cursor_at(byte_index, VisualAffinity::Downstream)
            .unwrap_or_else(|| panic!("光标几何基准位置无效"));
        bencher.iter(|| black_box(layout.cursor_geometry(cursor, 1.0)));
    });

    let (font, glyph_id, font_size, normalized_coords, synthesis) = layout
        .layout
        .lines()
        .flat_map(|line| line.items())
        .find_map(|item| match item {
            parley::PositionedLayoutItem::GlyphRun(run) => {
                let glyph = run.clone().positioned_glyphs().next()?;
                let normalized_coords = run
                    .run()
                    .normalized_coords()
                    .iter()
                    .map(|coordinate| coordinate.to_bits())
                    .collect::<Vec<_>>();
                Some((
                    run.run().font().clone(),
                    glyph.id,
                    run.run().font_size(),
                    normalized_coords,
                    run.run().synthesis(),
                ))
            }
            parley::PositionedLayoutItem::InlineBox(_) => None,
        })
        .unwrap_or_else(|| panic!("字形栅格基准没有可测字形"));
    let raster_options = GlyphRasterOptions {
        pixels_per_em: font_size * 96.0 / 72.0,
        pixel_phase: [0.0, 0.0],
        normalized_coords: &normalized_coords,
        synthesis,
        foreground: [28, 28, 28, 255],
    };
    let mut warm_rasterizer = GlyphRasterizer::default();
    warm_rasterizer
        .rasterize(&font, glyph_id, raster_options)
        .unwrap_or_else(|| panic!("Swash 无法栅格化基准字形"));
    criterion.bench_function("rust/glyph_raster/cache_hit", |bencher| {
        bencher.iter(|| black_box(warm_rasterizer.rasterize(&font, glyph_id, raster_options)));
    });
    criterion.bench_function("rust/glyph_raster/cache_miss", |bencher| {
        bencher.iter(|| {
            let mut rasterizer = GlyphRasterizer::default();
            black_box(rasterizer.rasterize(&font, glyph_id, raster_options))
        });
    });
}

criterion_group!(benches, benchmark_paragraph_layout, benchmark_hit_testing);
criterion_main!(benches);
