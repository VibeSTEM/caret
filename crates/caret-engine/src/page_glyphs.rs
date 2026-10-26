//! 按分页片段提取同一 Parley 布局的字形资源和页面局部基线坐标。

use caret_doc::NodeId;
use caret_layout::{
    GlyphFontData, GlyphRasterOptions, GlyphRasterizer, GlyphSynthesis, PositionedGlyphLayoutItem,
    RasterizedGlyph, RasterizedGlyphKey,
};
use std::collections::HashMap;

use crate::DocumentLayoutSnapshot;

/// 同一布局代际中一页的只读字形快照；字体数据按 Blob 共享。
#[derive(Debug, Clone, PartialEq)]
pub struct PageGlyphSnapshot {
    pub session_id: Option<NodeId>,
    pub document_generation: u64,
    pub font_generation: u64,
    pub page_index: usize,
    pub runs: Vec<PageGlyphRun>,
}

/// 同一页的唯一栅格图片；同字形重复使用时只传输一份像素数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageGlyphImage {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub pixel_phase_applied: bool,
    pub pixels: Vec<u8>,
}

/// 页面中某个 glyph 图片的放置位置和可选行框裁切范围。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGlyphPlacement {
    pub image_index: usize,
    pub x_pt: f64,
    pub baseline_y_pt: f64,
    pub pixel_phase: [f32; 2],
    pub line_clip_y_pt: Option<[f64; 2]>,
}

/// 页面栅格快照；图片去重，文本顺序保留在 placements 中。
#[derive(Debug, Clone, PartialEq)]
pub struct RasterizedPageGlyphs {
    pub session_id: Option<NodeId>,
    pub document_generation: u64,
    pub font_generation: u64,
    pub page_index: usize,
    pub images: Vec<PageGlyphImage>,
    pub placements: Vec<PageGlyphPlacement>,
}

/// 持有有界 glyph 缓存的页面栅格化入口。
#[derive(Default)]
pub struct PageGlyphRasterizer {
    glyphs: GlyphRasterizer,
}

impl PageGlyphRasterizer {
    /// 栅格化一页，px_per_pt 含 zoom 与设备像素比，页面原点用窗口设备像素表示。
    pub fn rasterize(
        &mut self,
        page: &PageGlyphSnapshot,
        px_per_pt: f64,
        page_origin_device_px: [f64; 2],
        default_foreground: [u8; 4],
    ) -> Result<RasterizedPageGlyphs, PageGlyphError> {
        if !px_per_pt.is_finite()
            || px_per_pt <= 0.0
            || page_origin_device_px.iter().any(|value| !value.is_finite())
        {
            return Err(PageGlyphError::InvalidRasterGeometry);
        }
        let pixels_per_pt = px_per_pt as f32;
        if !pixels_per_pt.is_finite() || pixels_per_pt <= 0.0 {
            return Err(PageGlyphError::InvalidRasterGeometry);
        }
        let mut images = Vec::new();
        let mut placements = Vec::new();
        let mut image_indexes = HashMap::<RasterizedGlyphKey, usize>::new();
        for run in &page.runs {
            let pixels_per_em = run.font_size_pt * pixels_per_pt;
            let foreground = run.foreground.unwrap_or(default_foreground);
            for glyph in &run.glyphs {
                let device_x = page_origin_device_px[0] + glyph.x_pt * px_per_pt;
                let device_y = page_origin_device_px[1] + glyph.baseline_y_pt * px_per_pt;
                if !device_x.is_finite() || !device_y.is_finite() {
                    return Err(PageGlyphError::InvalidRasterGeometry);
                }
                let phase = [device_phase(device_x), device_phase(device_y)];
                let options = GlyphRasterOptions {
                    pixels_per_em,
                    pixel_phase: phase,
                    normalized_coords: &run.normalized_coords,
                    synthesis: run.synthesis,
                    foreground,
                };
                let key = RasterizedGlyphKey::new(&run.font, glyph.glyph_id, options)
                    .ok_or(PageGlyphError::GlyphRasterizationFailed)?;
                let image_index = if let Some(index) = image_indexes.get(&key) {
                    *index
                } else {
                    let (key, image) = self
                        .glyphs
                        .rasterize_with_key(&run.font, glyph.glyph_id, options)
                        .ok_or(PageGlyphError::GlyphRasterizationFailed)?;
                    let index = images.len();
                    images.push(page_glyph_image(&image));
                    image_indexes.insert(key, index);
                    index
                };
                placements.push(PageGlyphPlacement {
                    image_index,
                    x_pt: glyph.x_pt,
                    baseline_y_pt: glyph.baseline_y_pt,
                    pixel_phase: phase,
                    line_clip_y_pt: run.line_clip_y_pt,
                });
            }
        }
        Ok(RasterizedPageGlyphs {
            session_id: page.session_id.clone(),
            document_generation: page.document_generation,
            font_generation: page.font_generation,
            page_index: page.page_index,
            images,
            placements,
        })
    }
}

fn device_phase(position: f64) -> f32 {
    let phase = position.rem_euclid(1.0) as f32;
    phase.min(f32::from_bits(1.0_f32.to_bits() - 1))
}

fn page_glyph_image(image: &RasterizedGlyph) -> PageGlyphImage {
    PageGlyphImage {
        width: image.width,
        height: image.height,
        left: image.left,
        top: image.top,
        pixel_phase_applied: image.pixel_phase_applied,
        pixels: image.pixels.clone(),
    }
}

/// 同一字体实例与绘制样式的视觉字形序列。
#[derive(Debug, Clone, PartialEq)]
pub struct PageGlyphRun {
    pub paragraph_id: NodeId,
    pub line_index: usize,
    /// Exactly 行距要求逐行裁切时的页面局部 y 范围；还需与页面裁切取交集。
    pub line_clip_y_pt: Option<[f64; 2]>,
    pub font: GlyphFontData,
    pub font_size_pt: f32,
    /// Parley 字体实例的 F2Dot14 规范化轴坐标原始位值。
    pub normalized_coords: Vec<i16>,
    pub synthesis: GlyphSynthesis,
    /// None 表示当前模型未声明颜色，绘制默认值由界面主题提供。
    pub foreground: Option<[u8; 4]>,
    pub glyphs: Vec<PageGlyph>,
}

/// 字形 ID 和页面局部 pt 坐标；顺序与 Parley 的视觉顺序相同。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGlyph {
    pub glyph_id: u32,
    pub x_pt: f64,
    /// 字形原点的基线 y，包含该 glyph 相对 run baseline 的局部偏移。
    pub baseline_y_pt: f64,
}

/// 页面或字形来源无效时拒绝整份页面输出，不发布部分字形。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageGlyphError {
    InvalidPageIndex,
    MissingParagraph,
    InvalidLineRange,
    InvalidLineGeometry,
    InvalidGlyphSource,
    InvalidRasterGeometry,
    GlyphRasterizationFailed,
    UnsupportedInlineBox,
}

impl std::fmt::Display for PageGlyphError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPageIndex => "layout.glyphs.invalid_page_index",
            Self::MissingParagraph => "layout.glyphs.missing_paragraph",
            Self::InvalidLineRange => "layout.glyphs.invalid_line_range",
            Self::InvalidLineGeometry => "layout.glyphs.invalid_line_geometry",
            Self::InvalidGlyphSource => "layout.glyphs.invalid_source",
            Self::InvalidRasterGeometry => "layout.glyphs.invalid_raster_geometry",
            Self::GlyphRasterizationFailed => "layout.glyphs.rasterization_failed",
            Self::UnsupportedInlineBox => "layout.glyphs.unsupported_inline_box",
        })
    }
}

impl std::error::Error for PageGlyphError {}

impl DocumentLayoutSnapshot {
    /// 从指定页面引用的行提取字形，不重新塑形或扫描其他页面。
    ///
    /// 页面索引、分页片段或字形来源无效时返回错误；不裁切允许溢出行框的字形，页面裁切
    /// 由绘制层执行。返回的字体 Blob 可跨快照共享，调用方不得用 Qt 再次匹配字体。
    pub fn page_glyphs(&self, page_index: usize) -> Result<PageGlyphSnapshot, PageGlyphError> {
        let page = self
            .pages
            .get(page_index)
            .ok_or(PageGlyphError::InvalidPageIndex)?;
        if !page.margin_left_pt.is_finite()
            || !page.margin_top_pt.is_finite()
            || page.margin_left_pt < 0.0
            || page.margin_top_pt < 0.0
        {
            return Err(PageGlyphError::InvalidLineGeometry);
        }
        let mut runs = Vec::new();
        for fragment in &page.layout.fragments {
            let paragraph = self
                .paragraph_by_id(&fragment.paragraph_id)
                .ok_or(PageGlyphError::MissingParagraph)?;
            let line_range = &fragment.line_range;
            if line_range.start >= line_range.end
                || line_range.end > paragraph.layout.paragraph.lines.len()
            {
                return Err(PageGlyphError::InvalidLineRange);
            }
            let mut page_line_top_pt = page.margin_top_pt + f64::from(fragment.y_offset_pt);
            if !page_line_top_pt.is_finite() || fragment.y_offset_pt < 0.0 {
                return Err(PageGlyphError::InvalidLineGeometry);
            }
            for line_index in line_range.clone() {
                let caret_line = &paragraph.layout.paragraph.lines[line_index];
                let line_height_pt = f64::from(caret_line.metrics.height_pt);
                let line_clip_y_pt = caret_line
                    .metrics
                    .clip_ink_to_line_height
                    .then_some([page_line_top_pt, page_line_top_pt + line_height_pt]);
                let raw_top_pt = f64::from(
                    *paragraph
                        .layout
                        .parley_line_tops_pt
                        .get(line_index)
                        .ok_or(PageGlyphError::InvalidLineGeometry)?,
                );
                let baseline_adjustment_pt = f64::from(
                    *paragraph
                        .layout
                        .baseline_adjustments_pt
                        .get(line_index)
                        .ok_or(PageGlyphError::InvalidLineGeometry)?,
                );
                if !line_height_pt.is_finite()
                    || line_height_pt <= 0.0
                    || !raw_top_pt.is_finite()
                    || !baseline_adjustment_pt.is_finite()
                {
                    return Err(PageGlyphError::InvalidLineGeometry);
                }
                // 空段由 Caret 合成不可见行框，Parley 可以没有对应的绘制行。
                if paragraph.text.is_empty() {
                    page_line_top_pt += line_height_pt;
                    continue;
                }
                let line = paragraph
                    .layout
                    .layout
                    .get(line_index)
                    .ok_or(PageGlyphError::InvalidGlyphSource)?;
                for item in line.items() {
                    let PositionedGlyphLayoutItem::GlyphRun(glyph_run) = item else {
                        return Err(PageGlyphError::UnsupportedInlineBox);
                    };
                    let run = glyph_run.run();
                    let font_size_pt = run.font_size();
                    if !font_size_pt.is_finite()
                        || font_size_pt <= 0.0
                        || run.font().data.is_empty()
                    {
                        return Err(PageGlyphError::InvalidGlyphSource);
                    }
                    let mut glyphs = Vec::new();
                    // 先消去 Parley 全局行顶，再在 f64 中加入局部 glyph y；避免超长段落
                    // 的全局 f32 baseline 吞掉组合附标的局部偏移。
                    let run_baseline_y_pt = page_line_top_pt + f64::from(glyph_run.baseline())
                        - raw_top_pt
                        + baseline_adjustment_pt;
                    for (glyph, local_glyph) in
                        glyph_run.positioned_glyphs().zip(glyph_run.glyphs())
                    {
                        let x_pt = page.margin_left_pt + f64::from(glyph.x);
                        let baseline_y_pt = run_baseline_y_pt + f64::from(local_glyph.y);
                        if !x_pt.is_finite() || !baseline_y_pt.is_finite() {
                            return Err(PageGlyphError::InvalidGlyphSource);
                        }
                        glyphs.push(PageGlyph {
                            glyph_id: glyph.id,
                            x_pt,
                            baseline_y_pt,
                        });
                    }
                    if !glyphs.is_empty() {
                        runs.push(PageGlyphRun {
                            paragraph_id: paragraph.id.clone(),
                            line_index,
                            line_clip_y_pt,
                            font: run.font().clone(),
                            font_size_pt,
                            normalized_coords: run
                                .normalized_coords()
                                .iter()
                                .map(|coordinate| coordinate.to_bits())
                                .collect(),
                            synthesis: run.synthesis(),
                            foreground: (glyph_run.style().brush != [0, 0, 0, 0])
                                .then_some(glyph_run.style().brush),
                            glyphs,
                        });
                    }
                }
                page_line_top_pt += line_height_pt;
            }
        }
        Ok(PageGlyphSnapshot {
            session_id: self.session_id.clone(),
            document_generation: self.document_generation,
            font_generation: self.font_generation,
            page_index,
            runs,
        })
    }
}

#[cfg(test)]
mod tests {
    use caret_doc::{
        Block, Document, InlineEnvironment, InlineEnvironmentKind, InlineNode, NodeId,
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

    fn layout(document: &Document) -> DocumentLayoutSnapshot {
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
        DocumentLayoutService::new()
            .layout(document, 17, &mut catalog, &ShapingStyle::default())
            .unwrap_or_else(|error| panic!("document layout failed: {error}"))
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
        snapshot.paragraphs[0].layout.baseline_adjustments_pt[0] = f32::NAN;
        assert_eq!(
            snapshot.page_glyphs(0),
            Err(PageGlyphError::InvalidLineGeometry)
        );

        snapshot = original;
        snapshot.paragraphs[0].layout.layout = Default::default();
        assert_eq!(
            snapshot.page_glyphs(0),
            Err(PageGlyphError::InvalidGlyphSource)
        );
    }
}
