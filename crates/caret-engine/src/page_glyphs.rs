//! 按分页片段提取同一 Parley 布局的字形资源和页面局部基线坐标。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::NodeId;
use caret_layout::{
    GlyphFontData, GlyphRasterizer, GlyphSynthesis, ParleyParagraphLayout,
    PositionedParagraphDrawItem, RasterizedGlyphKey,
};
use caret_math::MathLayoutSnapshot;
use std::{collections::HashMap, ops::Range, sync::Arc};

use crate::DocumentLayoutSnapshot;

mod math_raster;
use math_raster::{rasterize_math_object, rasterize_page_glyph};

/// 同一布局代际中一页的只读字形快照；字体数据按 Blob 共享。
#[derive(Debug, Clone, PartialEq)]
pub struct PageGlyphSnapshot {
    pub session_id: Option<NodeId>,
    pub document_generation: u64,
    pub font_generation: u64,
    pub page_index: usize,
    pub runs: Vec<PageGlyphRun>,
    pub decorations: Vec<PageGlyphDecoration>,
    pub(crate) math_objects: Vec<PageMathObject>,
    pub(crate) draw_order: Vec<PageDrawItem>,
}

/// 正文 run 与行内公式按 Parley 视觉顺序交错绘制。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageDrawItem {
    GlyphRun(usize),
    MathObject(usize),
}

/// 位于页面坐标中的公式快照，并保留该公式正文塑形句柄所属的局部资源表。
#[derive(Debug, Clone)]
pub(crate) struct PageMathObject {
    pub(crate) id: NodeId,
    /// 行内公式是所属段落；显示公式没有段落，这里是公式块 ID。
    pub(crate) paragraph_id: NodeId,
    pub(crate) source_bytes: Range<usize>,
    pub(crate) line_index: usize,
    pub(crate) display_block: bool,
    pub(crate) line_clip_y_pt: Option<[f64; 2]>,
    pub(crate) x_pt: f64,
    pub(crate) baseline_y_pt: f64,
    pub(crate) layout: Arc<MathLayoutSnapshot>,
    pub(crate) text_resources: Arc<HashMap<u64, Arc<ParleyParagraphLayout>>>,
}

impl PartialEq for PageMathObject {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.paragraph_id == other.paragraph_id
            && self.source_bytes == other.source_bytes
            && self.line_index == other.line_index
            && self.display_block == other.display_block
            && self.line_clip_y_pt == other.line_clip_y_pt
            && self.x_pt == other.x_pt
            && self.baseline_y_pt == other.baseline_y_pt
            && self.layout == other.layout
            && Arc::ptr_eq(&self.text_resources, &other.text_resources)
    }
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
    pub commands: Vec<PageGlyphDrawCommand>,
}

/// 一页中按视觉顺序排列的栅格图元。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageGlyphDrawCommand {
    Glyph(PageGlyphPlacement),
    Rectangle(PageGlyphDecoration),
    Line(PageGlyphLine),
}

/// 页面中的线段及居中笔画厚度，坐标单位为 pt。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGlyphLine {
    pub x1_pt: f64,
    pub y1_pt: f64,
    pub x2_pt: f64,
    pub y2_pt: f64,
    pub thickness_pt: f64,
    pub color: [u8; 4],
    pub line_clip_y_pt: Option<[f64; 2]>,
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
        let mut commands = Vec::new();
        let mut image_indexes = HashMap::<RasterizedGlyphKey, usize>::new();
        for item in &page.draw_order {
            match *item {
                PageDrawItem::GlyphRun(index) => {
                    let run = page
                        .runs
                        .get(index)
                        .ok_or(PageGlyphError::InvalidGlyphSource)?;
                    for glyph in &run.glyphs {
                        let foreground = run.foreground.unwrap_or(default_foreground);
                        let placement = rasterize_page_glyph(
                            &mut self.glyphs,
                            &mut images,
                            &mut image_indexes,
                            &run.font,
                            glyph.glyph_id,
                            run.font_size_pt * pixels_per_pt,
                            &run.normalized_coords,
                            run.synthesis,
                            foreground,
                            glyph.x_pt,
                            glyph.baseline_y_pt,
                            page_origin_device_px,
                            px_per_pt,
                            run.line_clip_y_pt,
                        )?;
                        commands.push(PageGlyphDrawCommand::Glyph(placement));
                    }
                }
                PageDrawItem::MathObject(index) => {
                    let object = page
                        .math_objects
                        .get(index)
                        .ok_or(PageGlyphError::InvalidGlyphSource)?;
                    rasterize_math_object(
                        &mut self.glyphs,
                        &mut images,
                        &mut image_indexes,
                        &mut commands,
                        object,
                        pixels_per_pt,
                        page_origin_device_px,
                        px_per_pt,
                        default_foreground,
                    )?;
                }
            }
        }
        for decoration in &page.decorations {
            commands.push(PageGlyphDrawCommand::Rectangle(PageGlyphDecoration {
                color: Some(decoration.color.unwrap_or(default_foreground)),
                ..*decoration
            }));
        }
        Ok(RasterizedPageGlyphs {
            session_id: page.session_id.clone(),
            document_generation: page.document_generation,
            font_generation: page.font_generation,
            page_index: page.page_index,
            images,
            commands,
        })
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
    MissingDisplayMath,
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
            Self::MissingDisplayMath => "layout.glyphs.missing_display_math",
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
        let mut math_objects = Vec::new();
        let mut draw_order = Vec::new();
        let mut pieces = Vec::with_capacity(page.layout.fragments.len() + page.layout.blocks.len());
        for fragment in &page.layout.fragments {
            pieces.push((fragment.y_offset_pt, PagePiece::Fragment(fragment)));
        }
        for block in &page.layout.blocks {
            pieces.push((block.y_offset_pt, PagePiece::Block(block)));
        }
        pieces.sort_by(|left, right| left.0.total_cmp(&right.0));
        for (_, piece) in pieces {
            match piece {
                PagePiece::Fragment(fragment) => append_paragraph_fragment(
                    self,
                    page,
                    fragment,
                    &mut runs,
                    &mut decorations,
                    &mut math_objects,
                    &mut draw_order,
                )?,
                PagePiece::Block(block) => {
                    append_display_math(self, page, block, &mut math_objects, &mut draw_order)?
                }
            }
        }
        Ok(PageGlyphSnapshot {
            session_id: self.published_session_id().cloned(),
            document_generation: self.published_document_generation(),
            font_generation: self.font_generation,
            page_index,
            runs,
            decorations,
            math_objects,
            draw_order,
        })
    }
}

enum PagePiece<'a> {
    Fragment(&'a caret_layout::ParagraphFragment),
    Block(&'a caret_layout::PageBlockFragment),
}

fn append_paragraph_fragment(
    snapshot: &DocumentLayoutSnapshot,
    page: &crate::PageLayoutSnapshot,
    fragment: &caret_layout::ParagraphFragment,
    runs: &mut Vec<PageGlyphRun>,
    decorations: &mut Vec<PageGlyphDecoration>,
    math_objects: &mut Vec<PageMathObject>,
    draw_order: &mut Vec<PageDrawItem>,
) -> Result<(), PageGlyphError> {
    let paragraph = snapshot
        .paragraph_by_id(&fragment.paragraph_id)
        .ok_or(PageGlyphError::MissingParagraph)?;
    let line_range = &fragment.line_range;
    if line_range.start >= line_range.end || line_range.end > paragraph.layout.paragraph.lines.len()
    {
        return Err(PageGlyphError::InvalidLineRange);
    }
    let mut page_line_top_pt = page.margin_top_pt + f64::from(fragment.y_offset_pt);
    let math_objects_by_id = paragraph
        .math_objects
        .iter()
        .map(|math| (math.inline_box_id, math))
        .collect::<HashMap<_, _>>();
    let inline_positions_by_id = paragraph
        .layout
        .positioned_inline_objects
        .iter()
        .map(|position| (position.id, position))
        .collect::<HashMap<_, _>>();
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
        if !line_height_pt.is_finite() || line_height_pt <= 0.0 {
            return Err(PageGlyphError::InvalidLineGeometry);
        }
        // 空段由 Caret 合成不可见行框，Parley 可以没有对应的绘制行。
        if paragraph.text.is_empty() {
            page_line_top_pt += line_height_pt;
            continue;
        }
        let draw_items = paragraph
            .layout
            .positioned_draw_items_for_line(line_index)
            .map_err(|error| match error {
                caret_layout::PositionedParagraphDrawError::InvalidLineGeometry => {
                    PageGlyphError::InvalidLineGeometry
                }
                caret_layout::PositionedParagraphDrawError::InvalidLineIndex
                | caret_layout::PositionedParagraphDrawError::InvalidGlyphRun => {
                    PageGlyphError::InvalidGlyphSource
                }
            })?;
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
        for item in draw_items {
            let PositionedParagraphDrawItem::GlyphRun(glyph_run) = item else {
                let PositionedParagraphDrawItem::InlineObject { id } = item else {
                    return Err(PageGlyphError::InvalidGlyphSource);
                };
                let math = math_objects_by_id
                    .get(&id)
                    .copied()
                    .ok_or(PageGlyphError::UnsupportedInlineBox)?;
                let position = inline_positions_by_id
                    .get(&id)
                    .copied()
                    .filter(|position| position.line_index == line_index)
                    .ok_or(PageGlyphError::InvalidGlyphSource)?;
                let x_pt = page.margin_left_pt + f64::from(position.x_pt);
                let baseline_y_pt =
                    page_line_top_pt + f64::from(position.y_pt) + f64::from(position.baseline_pt);
                if !x_pt.is_finite() || !baseline_y_pt.is_finite() {
                    return Err(PageGlyphError::InvalidLineGeometry);
                }
                let source_bytes = math.source_bytes.clone();
                if source_bytes != position.source_bytes {
                    return Err(PageGlyphError::InvalidGlyphSource);
                }
                let math_index = math_objects.len();
                math_objects.push(PageMathObject {
                    id: math.id.clone(),
                    paragraph_id: paragraph.id.clone(),
                    source_bytes,
                    line_index,
                    display_block: false,
                    line_clip_y_pt,
                    x_pt,
                    baseline_y_pt,
                    layout: Arc::clone(&math.layout),
                    text_resources: Arc::clone(&math.text_resources),
                });
                draw_order.push(PageDrawItem::MathObject(math_index));
                continue;
            };
            let font_size_pt = glyph_run.font_size_pt;
            if !font_size_pt.is_finite() || font_size_pt <= 0.0 || glyph_run.font.data.is_empty() {
                return Err(PageGlyphError::InvalidGlyphSource);
            }
            let mut glyphs = Vec::new();
            // 先消去 Parley 全局行顶，再在 f64 中加入局部 glyph y；避免超长段落
            // 的全局 f32 baseline 吞掉组合附标的局部偏移。
            for glyph in glyph_run.glyphs {
                let x_pt = page.margin_left_pt + f64::from(glyph.x_pt);
                let baseline_y_pt = page_line_top_pt + glyph.baseline_offset_pt;
                if !x_pt.is_finite() || !baseline_y_pt.is_finite() {
                    return Err(PageGlyphError::InvalidGlyphSource);
                }
                glyphs.push(PageGlyph {
                    glyph_id: glyph.glyph_id,
                    x_pt,
                    baseline_y_pt,
                });
            }
            if !glyphs.is_empty() {
                let run_index = runs.len();
                runs.push(PageGlyphRun {
                    paragraph_id: paragraph.id.clone(),
                    line_index,
                    line_clip_y_pt,
                    font: glyph_run.font.clone(),
                    font_size_pt,
                    normalized_coords: glyph_run.normalized_coords,
                    synthesis: glyph_run.synthesis,
                    foreground: glyph_run.foreground,
                    glyphs,
                });
                draw_order.push(PageDrawItem::GlyphRun(run_index));
            }
        }
        page_line_top_pt += line_height_pt;
    }
    Ok(())
}

fn append_display_math(
    snapshot: &DocumentLayoutSnapshot,
    page: &crate::PageLayoutSnapshot,
    block: &caret_layout::PageBlockFragment,
    math_objects: &mut Vec<PageMathObject>,
    draw_order: &mut Vec<PageDrawItem>,
) -> Result<(), PageGlyphError> {
    let math = snapshot
        .display_math
        .iter()
        .find(|math| math.id == block.block_id)
        .ok_or(PageGlyphError::MissingDisplayMath)?;
    let content_width_pt = page.width_pt - page.margin_left_pt - page.margin_right_pt;
    let x_pt = math.origin_x_pt(page.margin_left_pt, content_width_pt);
    let baseline_y_pt =
        page.margin_top_pt + f64::from(block.y_offset_pt) + f64::from(math.ascent_pt);
    if !content_width_pt.is_finite()
        || content_width_pt <= 0.0
        || !x_pt.is_finite()
        || !baseline_y_pt.is_finite()
        || !math.width_pt.is_finite()
        || !math.ascent_pt.is_finite()
        || block.y_offset_pt < 0.0
        || block.height_pt <= 0.0
    {
        return Err(PageGlyphError::InvalidLineGeometry);
    }
    let math_index = math_objects.len();
    math_objects.push(PageMathObject {
        id: math.id.clone(),
        paragraph_id: math.id.clone(),
        source_bytes: 0..0,
        line_index: 0,
        display_block: true,
        line_clip_y_pt: None,
        x_pt,
        baseline_y_pt,
        layout: Arc::clone(&math.layout),
        text_resources: Arc::clone(&math.text_resources),
    });
    draw_order.push(PageDrawItem::MathObject(math_index));
    Ok(())
}

#[cfg(test)]
mod tests;
