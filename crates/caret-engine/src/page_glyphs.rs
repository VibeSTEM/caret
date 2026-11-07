//! 按分页片段提取同一 Parley 布局的字形资源和页面局部基线坐标。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::NodeId;
use caret_layout::{
    GlyphFontData, GlyphRasterOptions, GlyphRasterizer, GlyphSynthesis, PositionedGlyphLayoutItem,
    RasterizedGlyph, RasterizedGlyphKey,
};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::DocumentLayoutSnapshot;

/// 同一布局代际中一页的只读字形快照；字体数据按 Blob 共享。
#[derive(Debug, Clone, PartialEq)]
pub struct PageGlyphSnapshot {
    pub session_id: Option<NodeId>,
    pub document_generation: u64,
    pub font_generation: u64,
    pub page_index: usize,
    pub runs: Vec<PageGlyphRun>,
    pub decorations: Vec<PageGlyphDecoration>,
    pub shapes: Vec<PageMathShape>,
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
    pub paint_order: u64,
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
    pub decorations: Vec<PageGlyphDecoration>,
    pub shapes: Vec<PageMathShape>,
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
        let decorations = page
            .decorations
            .iter()
            .map(|decoration| PageGlyphDecoration {
                color: Some(decoration.color.unwrap_or(default_foreground)),
                ..*decoration
            })
            .collect();
        let shapes = page
            .shapes
            .iter()
            .map(|shape| {
                let mut shape = *shape;
                if shape.stroke_width_pt > 0.0 {
                    shape.stroke = Some(shape.stroke.unwrap_or(default_foreground));
                }
                shape
            })
            .collect();
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
                    paint_order: run.paint_order,
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
            decorations,
            shapes,
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
    pub paint_order: u64,
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

/// 页面中的字符装饰矩形，坐标使用页面局部 pt。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGlyphDecoration {
    pub paint_order: u64,
    pub x_pt: f64,
    pub y_pt: f64,
    pub width_pt: f64,
    pub height_pt: f64,
    pub color: Option<[u8; 4]>,
    pub line_clip_y_pt: Option<[f64; 2]>,
}

/// 数学排版产生的线段或矩形；坐标使用页面局部 pt，颜色缺省时由画布前景色补齐。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageMathShape {
    pub paint_order: u64,
    pub kind: PageMathShapeKind,
    pub x_pt: f64,
    pub y_pt: f64,
    pub dx_pt: f64,
    pub dy_pt: f64,
    pub width_pt: f64,
    pub height_pt: f64,
    pub fill: Option<[u8; 4]>,
    pub stroke: Option<[u8; 4]>,
    pub stroke_width_pt: f64,
    pub line_clip_y_pt: Option<[f64; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PageMathShapeKind {
    Line = 0,
    Rectangle = 1,
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
    MissingMathBlock,
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
            Self::MissingMathBlock => "layout.glyphs.missing_math_block",
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
        let mut decorations = Vec::new();
        let mut shapes = Vec::new();
        let mut next_paint_order = 0_u64;
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
                for (bounds, color) in paragraph.layout.underline_geometry_for_line(line_index) {
                    let x_pt = page.margin_left_pt + bounds.x0;
                    let y_pt = page_line_top_pt + bounds.y0;
                    let width_pt = bounds.x1 - bounds.x0;
                    let height_pt = bounds.y1 - bounds.y0;
                    if !x_pt.is_finite()
                        || !y_pt.is_finite()
                        || !width_pt.is_finite()
                        || !height_pt.is_finite()
                        || width_pt <= 0.0
                        || height_pt <= 0.0
                    {
                        return Err(PageGlyphError::InvalidLineGeometry);
                    }
                    decorations.push(PageGlyphDecoration {
                        paint_order: next_paint_order,
                        x_pt,
                        y_pt,
                        width_pt,
                        height_pt,
                        color,
                        line_clip_y_pt,
                    });
                    next_paint_order += 1;
                }
                for item in line.items() {
                    let PositionedGlyphLayoutItem::GlyphRun(glyph_run) = &item else {
                        let PositionedGlyphLayoutItem::InlineBox(inline_box) = item else {
                            return Err(PageGlyphError::UnsupportedInlineBox);
                        };
                        let math_layout = paragraph
                            .inline_math_layouts
                            .get(
                                usize::try_from(inline_box.id)
                                    .map_err(|_| PageGlyphError::UnsupportedInlineBox)?,
                            )
                            .ok_or(PageGlyphError::UnsupportedInlineBox)?;
                        let math_text_layouts = paragraph
                            .inline_math_text_layouts
                            .get(
                                usize::try_from(inline_box.id)
                                    .map_err(|_| PageGlyphError::UnsupportedInlineBox)?,
                            )
                            .ok_or(PageGlyphError::UnsupportedInlineBox)?;
                        let index = usize::try_from(inline_box.id)
                            .map_err(|_| PageGlyphError::UnsupportedInlineBox)?;
                        let math_id = paragraph
                            .inline_math_ids
                            .get(index)
                            .ok_or(PageGlyphError::UnsupportedInlineBox)?;
                        let projection = self
                            .math_projection(page_index, math_id)
                            .ok_or(PageGlyphError::UnsupportedInlineBox)?;
                        let object_baseline_y_pt = projection.baseline_y_pt;
                        let object_x_pt = projection.object_x_pt;
                        if !object_baseline_y_pt.is_finite() || !object_x_pt.is_finite() {
                            return Err(PageGlyphError::InvalidGlyphSource);
                        }
                        append_math_items(
                            &math_layout.items,
                            math_text_layouts,
                            MathItemHost {
                                paragraph_id: &paragraph.id,
                                line_index,
                                line_clip_y_pt: projection.drawing_clip(),
                                object_x_pt,
                                baseline_y_pt: object_baseline_y_pt,
                            },
                            MathItemOutput {
                                runs: &mut runs,
                                decorations: &mut decorations,
                                shapes: &mut shapes,
                            },
                            &mut next_paint_order,
                        )?;
                        continue;
                    };
                    // 先消去 Parley 全局行顶，再在 f64 中加入局部 glyph y；避免超长段落
                    // 的全局 f32 baseline 吞掉组合附标的局部偏移。
                    let run_baseline_y_pt = page_line_top_pt + f64::from(glyph_run.baseline())
                        - raw_top_pt
                        + baseline_adjustment_pt;
                    if let Some(run) = page_run_from_positioned_glyphs(
                        &item,
                        PageGlyphRunOrigin {
                            x_pt: page.margin_left_pt,
                            baseline_y_pt: run_baseline_y_pt,
                            paint_order: next_paint_order,
                            paragraph_id: &paragraph.id,
                            line_index,
                            line_clip_y_pt,
                            foreground: None,
                        },
                    )? {
                        runs.push(run);
                        next_paint_order += 1;
                    }
                }
                page_line_top_pt += line_height_pt;
            }
        }
        for fragment in &page.layout.math_fragments {
            let layout = self
                .math_blocks
                .get(&fragment.math_id)
                .ok_or(PageGlyphError::MissingMathBlock)?;
            let text_layouts = self
                .math_block_text_layouts
                .get(&fragment.math_id)
                .ok_or(PageGlyphError::MissingMathBlock)?;
            let projection = self
                .math_projection(page_index, &fragment.math_id)
                .ok_or(PageGlyphError::MissingMathBlock)?;
            let object_x_pt = projection.object_x_pt;
            let baseline_y_pt = projection.baseline_y_pt;
            if !object_x_pt.is_finite()
                || !baseline_y_pt.is_finite()
                || !fragment.width_pt.is_finite()
                || !fragment.height_pt.is_finite()
            {
                return Err(PageGlyphError::InvalidLineGeometry);
            }
            append_math_items(
                &layout.items,
                text_layouts,
                MathItemHost {
                    paragraph_id: &fragment.math_id,
                    line_index: 0,
                    line_clip_y_pt: projection.drawing_clip(),
                    object_x_pt,
                    baseline_y_pt,
                },
                MathItemOutput {
                    runs: &mut runs,
                    decorations: &mut decorations,
                    shapes: &mut shapes,
                },
                &mut next_paint_order,
            )?;
        }
        Ok(PageGlyphSnapshot {
            session_id: self.session_id.clone(),
            document_generation: self.document_generation,
            font_generation: self.font_generation,
            page_index,
            runs,
            decorations,
            shapes,
        })
    }
}

struct MathItemHost<'a> {
    paragraph_id: &'a NodeId,
    line_index: usize,
    line_clip_y_pt: Option<[f64; 2]>,
    object_x_pt: f64,
    baseline_y_pt: f64,
}

struct MathItemOutput<'a> {
    runs: &'a mut Vec<PageGlyphRun>,
    decorations: &'a mut Vec<PageGlyphDecoration>,
    shapes: &'a mut Vec<PageMathShape>,
}

fn append_math_items(
    items: &[caret_math::layout::MathItem],
    text_layouts: &HashMap<u64, crate::layout::MathTextSidecar>,
    host: MathItemHost<'_>,
    output: MathItemOutput<'_>,
    next_paint_order: &mut u64,
) -> Result<(), PageGlyphError> {
    for item in items {
        let color_bytes = |color: Option<caret_math::layout::MathColor>| {
            color.map(|color| [color.red, color.green, color.blue, color.alpha])
        };
        match item {
            caret_math::layout::MathItem::Glyph {
                glyph,
                x,
                y,
                size,
                color,
            } => {
                let font_size_pt = *size as f32;
                let x_pt = host.object_x_pt + x;
                let baseline_y_pt = host.baseline_y_pt + y;
                if !font_size_pt.is_finite()
                    || font_size_pt <= 0.0
                    || !x_pt.is_finite()
                    || !baseline_y_pt.is_finite()
                {
                    return Err(PageGlyphError::InvalidGlyphSource);
                }
                output.runs.push(PageGlyphRun {
                    paint_order: *next_paint_order,
                    paragraph_id: host.paragraph_id.clone(),
                    line_index: host.line_index,
                    line_clip_y_pt: host.line_clip_y_pt,
                    font: math_font_data(),
                    font_size_pt,
                    normalized_coords: Vec::new(),
                    synthesis: GlyphSynthesis::default(),
                    foreground: color_bytes(*color),
                    glyphs: vec![PageGlyph {
                        glyph_id: u32::from(*glyph),
                        x_pt,
                        baseline_y_pt,
                    }],
                });
                *next_paint_order += 1;
            }
            caret_math::layout::MathItem::Rule {
                x,
                y,
                width,
                height,
                color,
            } => {
                let x_pt = host.object_x_pt + x;
                let y_pt = host.baseline_y_pt + y;
                if !x_pt.is_finite()
                    || !y_pt.is_finite()
                    || !width.is_finite()
                    || !height.is_finite()
                    || *width <= 0.0
                    || *height <= 0.0
                {
                    return Err(PageGlyphError::InvalidGlyphSource);
                }
                output.decorations.push(PageGlyphDecoration {
                    paint_order: *next_paint_order,
                    x_pt,
                    y_pt,
                    width_pt: *width,
                    height_pt: *height,
                    color: color_bytes(*color),
                    line_clip_y_pt: host.line_clip_y_pt,
                });
                *next_paint_order += 1;
            }
            caret_math::layout::MathItem::Line {
                x,
                y,
                dx,
                dy,
                thickness,
                color,
            } => {
                let x_pt = host.object_x_pt + x;
                let y_pt = host.baseline_y_pt + y;
                if !x_pt.is_finite()
                    || !y_pt.is_finite()
                    || !dx.is_finite()
                    || !dy.is_finite()
                    || !thickness.is_finite()
                    || *thickness <= 0.0
                {
                    return Err(PageGlyphError::InvalidGlyphSource);
                }
                output.shapes.push(PageMathShape {
                    paint_order: *next_paint_order,
                    kind: PageMathShapeKind::Line,
                    x_pt,
                    y_pt,
                    dx_pt: *dx,
                    dy_pt: *dy,
                    width_pt: 0.0,
                    height_pt: 0.0,
                    fill: None,
                    stroke: color_bytes(*color),
                    stroke_width_pt: *thickness,
                    line_clip_y_pt: host.line_clip_y_pt,
                });
                *next_paint_order += 1;
            }
            caret_math::layout::MathItem::Rectangle {
                x,
                y,
                width,
                height,
                fill,
                stroke,
                stroke_width,
            } => {
                let x_pt = host.object_x_pt + x;
                let y_pt = host.baseline_y_pt + y;
                if !x_pt.is_finite()
                    || !y_pt.is_finite()
                    || !width.is_finite()
                    || !height.is_finite()
                    || *width <= 0.0
                    || *height <= 0.0
                    || !stroke_width.is_finite()
                    || *stroke_width < 0.0
                {
                    return Err(PageGlyphError::InvalidGlyphSource);
                }
                output.shapes.push(PageMathShape {
                    paint_order: *next_paint_order,
                    kind: PageMathShapeKind::Rectangle,
                    x_pt,
                    y_pt,
                    dx_pt: 0.0,
                    dy_pt: 0.0,
                    width_pt: *width,
                    height_pt: *height,
                    fill: color_bytes(*fill),
                    stroke: color_bytes(*stroke),
                    stroke_width_pt: *stroke_width,
                    line_clip_y_pt: host.line_clip_y_pt,
                });
                *next_paint_order += 1;
            }
            caret_math::layout::MathItem::TextIsland {
                token,
                x,
                baseline_y,
                color,
                ..
            } => {
                let sidecar = text_layouts
                    .get(token)
                    .ok_or(PageGlyphError::InvalidGlyphSource)?;
                if sidecar.layout.text.is_empty() {
                    continue;
                }
                let mut lines = sidecar.layout.layout.lines();
                let first_line = lines.next().ok_or(PageGlyphError::InvalidGlyphSource)?;
                let first_baseline = first_line.metrics().baseline;
                let island_baseline = host.baseline_y_pt + baseline_y;
                let math_color = color_bytes(*color).or_else(|| color_bytes(sidecar.color));
                for line in std::iter::once(first_line).chain(lines) {
                    for positioned in line.items() {
                        let caret_layout::PositionedGlyphLayoutItem::GlyphRun(glyph_run) =
                            &positioned
                        else {
                            continue;
                        };
                        let baseline_y_pt = island_baseline + f64::from(glyph_run.baseline())
                            - f64::from(first_baseline);
                        let run = glyph_run.run();
                        let foreground = math_color.or_else(|| {
                            (glyph_run.style().brush != [0, 0, 0, 0])
                                .then_some(glyph_run.style().brush)
                        });
                        if let Some(decoration) = glyph_run.style().underline.as_ref() {
                            let font_metrics = run.font_metrics();
                            let offset = decoration.offset.unwrap_or(font_metrics.underline_offset);
                            let font_size = run.font_size();
                            let thickness = decoration
                                .size
                                .unwrap_or(font_metrics.underline_size)
                                .is_finite()
                                .then_some(decoration.size.unwrap_or(font_metrics.underline_size))
                                .filter(|value| *value > 0.0)
                                .unwrap_or(font_size / 18.0);
                            let x_pt = host.object_x_pt + x + f64::from(glyph_run.offset());
                            let y_pt = baseline_y_pt - f64::from(offset);
                            let width_pt = f64::from(glyph_run.advance());
                            if !width_pt.is_finite() || width_pt < 0.0 {
                                return Err(PageGlyphError::InvalidGlyphSource);
                            }
                            if width_pt > 0.0 {
                                if !offset.is_finite()
                                    || !thickness.is_finite()
                                    || !x_pt.is_finite()
                                    || !y_pt.is_finite()
                                    || thickness <= 0.0
                                {
                                    return Err(PageGlyphError::InvalidGlyphSource);
                                }
                                output.decorations.push(PageGlyphDecoration {
                                    paint_order: *next_paint_order,
                                    x_pt,
                                    y_pt,
                                    width_pt,
                                    height_pt: f64::from(thickness),
                                    color: math_color.or_else(|| {
                                        (decoration.brush != [0, 0, 0, 0])
                                            .then_some(decoration.brush)
                                    }),
                                    line_clip_y_pt: host.line_clip_y_pt,
                                });
                                *next_paint_order += 1;
                            }
                        }
                        if let Some(run) = page_run_from_positioned_glyphs(
                            &positioned,
                            PageGlyphRunOrigin {
                                x_pt: host.object_x_pt + x,
                                baseline_y_pt,
                                paint_order: *next_paint_order,
                                paragraph_id: host.paragraph_id,
                                line_index: host.line_index,
                                line_clip_y_pt: host.line_clip_y_pt,
                                foreground,
                            },
                        )? {
                            output.runs.push(run);
                            *next_paint_order += 1;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

struct PageGlyphRunOrigin<'a> {
    x_pt: f64,
    baseline_y_pt: f64,
    paint_order: u64,
    paragraph_id: &'a NodeId,
    line_index: usize,
    line_clip_y_pt: Option<[f64; 2]>,
    foreground: Option<[u8; 4]>,
}

/// 将正文或数学文本岛的同类 Parley glyph run 转为页面快照。
fn page_run_from_positioned_glyphs(
    item: &PositionedGlyphLayoutItem<'_, [u8; 4]>,
    origin: PageGlyphRunOrigin<'_>,
) -> Result<Option<PageGlyphRun>, PageGlyphError> {
    let PositionedGlyphLayoutItem::GlyphRun(glyph_run) = item else {
        return Err(PageGlyphError::UnsupportedInlineBox);
    };
    let run = glyph_run.run();
    let font_size_pt = run.font_size();
    if !font_size_pt.is_finite() || font_size_pt <= 0.0 || run.font().data.is_empty() {
        return Err(PageGlyphError::InvalidGlyphSource);
    }
    let glyphs = glyph_run
        .positioned_glyphs()
        .zip(glyph_run.glyphs())
        .map(|(positioned_glyph, local_glyph)| PageGlyph {
            glyph_id: positioned_glyph.id,
            x_pt: origin.x_pt + f64::from(positioned_glyph.x),
            baseline_y_pt: origin.baseline_y_pt + f64::from(local_glyph.y),
        })
        .collect::<Vec<_>>();
    if glyphs
        .iter()
        .any(|glyph| !glyph.x_pt.is_finite() || !glyph.baseline_y_pt.is_finite())
    {
        return Err(PageGlyphError::InvalidGlyphSource);
    }
    if glyphs.is_empty() {
        return Ok(None);
    }
    Ok(Some(PageGlyphRun {
        paint_order: origin.paint_order,
        paragraph_id: origin.paragraph_id.clone(),
        line_index: origin.line_index,
        line_clip_y_pt: origin.line_clip_y_pt,
        font: run.font().clone(),
        font_size_pt,
        normalized_coords: run
            .normalized_coords()
            .iter()
            .map(|coordinate| coordinate.to_bits())
            .collect(),
        synthesis: run.synthesis(),
        foreground: origin.foreground.or_else(|| {
            (glyph_run.style().brush != [0, 0, 0, 0]).then_some(glyph_run.style().brush)
        }),
        glyphs,
    }))
}

fn math_font_data() -> GlyphFontData {
    static FONT: OnceLock<GlyphFontData> = OnceLock::new();
    FONT.get_or_init(|| {
        let data: Arc<dyn AsRef<[u8]> + Send + Sync> =
            Arc::new(caret_font_roles::default_math_font());
        GlyphFontData::new(fontique::Blob::new(data), 0)
    })
    .clone()
}

#[cfg(test)]
mod tests;
