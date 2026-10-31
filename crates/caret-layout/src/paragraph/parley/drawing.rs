//! 将 Parley 行项目转换为可跨 engine/FFI 消费的稳定页面绘制输入。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::ParleyParagraphLayout;
use crate::{GlyphFontData, GlyphSynthesis};
use parley::PositionedLayoutItem;

/// Parley 字形在段落局部坐标中的位置和基线偏移。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalGlyph {
    /// 按所属字体面解释的字形 ID。
    pub glyph_id: u32,
    /// 段落左侧起算的 x 坐标，单位为 pt。
    pub x_pt: f32,
    /// 相对 Caret 行顶的基线 y 坐标，单位为 pt，使用 f64 保留长段落的局部偏移。
    pub baseline_offset_pt: f64,
}

/// 不依赖 Parley `GlyphRun` 的字体实例和 glyph 序列投影。
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGlyphRun {
    /// Parley 实际选中的字体 Blob 与 face 索引。
    pub font: GlyphFontData,
    /// Parley 实际使用的字号，单位为 pt。
    pub font_size_pt: f32,
    /// 可变字体轴的 F2Dot14 原始值。
    pub normalized_coords: Vec<i16>,
    /// Parley 为该字体实例选择的合成参数。
    pub synthesis: GlyphSynthesis,
    /// 当前 run 的前景色；`None` 表示使用调用方默认色。
    pub foreground: Option<[u8; 4]>,
    /// glyph 的视觉顺序、段落局部 x 与行局部基线偏移。
    pub glyphs: Vec<LocalGlyph>,
}

/// 保持视觉顺序的段落行绘制输入。
#[derive(Debug, Clone, PartialEq)]
pub enum PositionedParagraphDrawItem {
    /// 一个带有确切字体实例的文字 run。
    GlyphRun(PositionedGlyphRun),
    /// 行内对象的位置标识；坐标由对应 `PositionedInlineObject` 提供。
    InlineObject { id: u64 },
}

/// 行项目或字体实例无法映射为稳定的 glyph 输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionedParagraphDrawError {
    /// 请求的行超出 Parley 布局。
    InvalidLineIndex,
    /// 行顶或 Caret 基线映射缺失或不是有限值。
    InvalidLineGeometry,
    /// glyph run 缺少有效字体或包含非法坐标。
    InvalidGlyphRun,
}

impl ParleyParagraphLayout {
    /// 读取一行的有序绘制项；基线偏移已改为行框局部坐标，便于精确放置正文资源。
    pub fn positioned_draw_items_for_line(
        &self,
        line_index: usize,
    ) -> Result<Vec<PositionedParagraphDrawItem>, PositionedParagraphDrawError> {
        let line = self
            .layout
            .get(line_index)
            .ok_or(PositionedParagraphDrawError::InvalidLineIndex)?;
        let raw_top = *self
            .parley_line_tops_pt
            .get(line_index)
            .ok_or(PositionedParagraphDrawError::InvalidLineGeometry)?;
        let baseline_adjustment = *self
            .baseline_adjustments_pt
            .get(line_index)
            .ok_or(PositionedParagraphDrawError::InvalidLineGeometry)?;
        if !raw_top.is_finite() || !baseline_adjustment.is_finite() {
            return Err(PositionedParagraphDrawError::InvalidLineGeometry);
        }

        line.items()
            .map(|item| match item {
                PositionedLayoutItem::GlyphRun(glyph_run) => {
                    let run = glyph_run.run();
                    let font_size_pt = run.font_size();
                    if !font_size_pt.is_finite()
                        || font_size_pt <= 0.0
                        || run.font().data.is_empty()
                    {
                        return Err(PositionedParagraphDrawError::InvalidGlyphRun);
                    }
                    let baseline = f64::from(glyph_run.baseline()) - f64::from(raw_top)
                        + f64::from(baseline_adjustment);
                    if !baseline.is_finite() {
                        return Err(PositionedParagraphDrawError::InvalidGlyphRun);
                    }
                    let glyphs = glyph_run
                        .positioned_glyphs()
                        .zip(glyph_run.glyphs())
                        .map(|(glyph, local_glyph)| {
                            let baseline_offset_pt = baseline + f64::from(local_glyph.y);
                            if !glyph.x.is_finite() || !baseline_offset_pt.is_finite() {
                                return Err(PositionedParagraphDrawError::InvalidGlyphRun);
                            }
                            Ok(LocalGlyph {
                                glyph_id: glyph.id,
                                x_pt: glyph.x,
                                baseline_offset_pt,
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(PositionedParagraphDrawItem::GlyphRun(PositionedGlyphRun {
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
                    }))
                }
                PositionedLayoutItem::InlineBox(inline_box) => {
                    Ok(PositionedParagraphDrawItem::InlineObject { id: inline_box.id })
                }
            })
            .collect()
    }
}
