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
    pub decorations: Vec<PageGlyphDecoration>,
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
    pub decorations: Vec<PageGlyphDecoration>,
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
            decorations,
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

/// 页面中的字符装饰矩形，坐标使用页面局部 pt。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGlyphDecoration {
    pub x_pt: f64,
    pub y_pt: f64,
    pub width_pt: f64,
    pub height_pt: f64,
    pub color: Option<[u8; 4]>,
    pub line_clip_y_pt: Option<[f64; 2]>,
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
        let mut decorations = Vec::new();
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
                        x_pt,
                        y_pt,
                        width_pt,
                        height_pt,
                        color,
                        line_clip_y_pt,
                    });
                }
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
            decorations,
        })
    }
}

#[cfg(test)]
mod tests;
