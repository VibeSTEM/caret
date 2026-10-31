//! 将 Parley 与数学布局统一栅格为页面绘制命令。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{
    PageGlyphDecoration, PageGlyphDrawCommand, PageGlyphError, PageGlyphImage, PageGlyphLine,
    PageGlyphPlacement, PageMathObject,
};
use caret_layout::{
    GlyphFontData, GlyphRasterOptions, GlyphRasterizer, GlyphSynthesis,
    PositionedParagraphDrawItem, RasterizedGlyph, RasterizedGlyphKey, stix_two_math_font_data,
};
use caret_math::MathLayoutItem;
use std::collections::HashMap;

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

#[allow(clippy::too_many_arguments)]
pub(super) fn rasterize_page_glyph(
    rasterizer: &mut GlyphRasterizer,
    images: &mut Vec<PageGlyphImage>,
    image_indexes: &mut HashMap<RasterizedGlyphKey, usize>,
    font: &GlyphFontData,
    glyph_id: u32,
    pixels_per_em: f32,
    normalized_coords: &[i16],
    synthesis: GlyphSynthesis,
    foreground: [u8; 4],
    x_pt: f64,
    baseline_y_pt: f64,
    page_origin_device_px: [f64; 2],
    px_per_pt: f64,
    line_clip_y_pt: Option<[f64; 2]>,
) -> Result<PageGlyphPlacement, PageGlyphError> {
    if !pixels_per_em.is_finite()
        || pixels_per_em <= 0.0
        || !x_pt.is_finite()
        || !baseline_y_pt.is_finite()
        || !valid_clip(line_clip_y_pt)
    {
        return Err(PageGlyphError::InvalidRasterGeometry);
    }
    let device_x = page_origin_device_px[0] + x_pt * px_per_pt;
    let device_y = page_origin_device_px[1] + baseline_y_pt * px_per_pt;
    if !device_x.is_finite() || !device_y.is_finite() {
        return Err(PageGlyphError::InvalidRasterGeometry);
    }
    let phase = [device_phase(device_x), device_phase(device_y)];
    let options = GlyphRasterOptions {
        pixels_per_em,
        pixel_phase: phase,
        normalized_coords,
        synthesis,
        foreground,
    };
    let key = RasterizedGlyphKey::new(font, glyph_id, options)
        .ok_or(PageGlyphError::GlyphRasterizationFailed)?;
    let image_index = if let Some(index) = image_indexes.get(&key) {
        *index
    } else {
        let (key, image) = rasterizer
            .rasterize_with_key(font, glyph_id, options)
            .ok_or(PageGlyphError::GlyphRasterizationFailed)?;
        let index = images.len();
        images.push(page_glyph_image(&image));
        image_indexes.insert(key, index);
        index
    };
    Ok(PageGlyphPlacement {
        image_index,
        x_pt,
        baseline_y_pt,
        pixel_phase: phase,
        line_clip_y_pt,
    })
}

fn valid_clip(clip: Option<[f64; 2]>) -> bool {
    clip.is_none_or(|[top, bottom]| top.is_finite() && bottom.is_finite() && bottom > top)
}

fn math_color(color: Option<[u8; 3]>, fallback: [u8; 4]) -> [u8; 4] {
    color
        .map(|[red, green, blue]| [red, green, blue, 255])
        .unwrap_or(fallback)
}

fn micro_pt(value: i64) -> f64 {
    value as f64 / 1_000_000.0
}

#[allow(clippy::too_many_arguments)]
pub(super) fn rasterize_math_object(
    rasterizer: &mut GlyphRasterizer,
    images: &mut Vec<PageGlyphImage>,
    image_indexes: &mut HashMap<RasterizedGlyphKey, usize>,
    commands: &mut Vec<PageGlyphDrawCommand>,
    object: &PageMathObject,
    pixels_per_pt: f32,
    page_origin_device_px: [f64; 2],
    px_per_pt: f64,
    default_foreground: [u8; 4],
) -> Result<(), PageGlyphError> {
    if !object.x_pt.is_finite()
        || !object.baseline_y_pt.is_finite()
        || !valid_clip(object.line_clip_y_pt)
        || object.layout.font_resource_sha256 != caret_math::MATH_FONT_SHA256
    {
        return Err(PageGlyphError::InvalidGlyphSource);
    }
    let math_font = stix_two_math_font_data();
    for item in &object.layout.items {
        match *item {
            MathLayoutItem::Glyph {
                glyph_id,
                font_size_micro_pt,
                x_micro_pt,
                baseline_y_micro_pt,
                color,
            } => {
                let x_pt = object.x_pt + micro_pt(x_micro_pt);
                let baseline_y_pt = object.baseline_y_pt + micro_pt(baseline_y_micro_pt);
                let placement = rasterize_page_glyph(
                    rasterizer,
                    images,
                    image_indexes,
                    &math_font,
                    u32::from(glyph_id),
                    micro_pt(font_size_micro_pt) as f32 * pixels_per_pt,
                    &[],
                    GlyphSynthesis::default(),
                    math_color(color, default_foreground),
                    x_pt,
                    baseline_y_pt,
                    page_origin_device_px,
                    px_per_pt,
                    object.line_clip_y_pt,
                )?;
                commands.push(PageGlyphDrawCommand::Glyph(placement));
            }
            MathLayoutItem::ExternalText {
                handle,
                x_micro_pt,
                baseline_y_micro_pt,
                color,
            } => {
                let paragraph = object
                    .text_resources
                    .get(&handle)
                    .ok_or(PageGlyphError::InvalidGlyphSource)?;
                if paragraph.paragraph.lines.len() != 1 {
                    return Err(PageGlyphError::InvalidGlyphSource);
                }
                let baseline = f64::from(paragraph.paragraph.lines[0].metrics.baseline_pt);
                if !baseline.is_finite() {
                    return Err(PageGlyphError::InvalidGlyphSource);
                }
                let items = paragraph
                    .positioned_draw_items_for_line(0)
                    .map_err(|_| PageGlyphError::InvalidGlyphSource)?;
                for local_item in items {
                    let PositionedParagraphDrawItem::GlyphRun(run) = local_item else {
                        return Err(PageGlyphError::InvalidGlyphSource);
                    };
                    for glyph in run.glyphs {
                        let placement = rasterize_page_glyph(
                            rasterizer,
                            images,
                            image_indexes,
                            &run.font,
                            glyph.glyph_id,
                            run.font_size_pt * pixels_per_pt,
                            &run.normalized_coords,
                            run.synthesis,
                            color
                                .map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
                                .or(run.foreground)
                                .unwrap_or(default_foreground),
                            object.x_pt + micro_pt(x_micro_pt) + f64::from(glyph.x_pt),
                            object.baseline_y_pt
                                + micro_pt(baseline_y_micro_pt)
                                + glyph.baseline_offset_pt
                                - baseline,
                            page_origin_device_px,
                            px_per_pt,
                            object.line_clip_y_pt,
                        )?;
                        commands.push(PageGlyphDrawCommand::Glyph(placement));
                    }
                }
            }
            MathLayoutItem::Rule {
                x_micro_pt,
                y_micro_pt,
                width_micro_pt,
                height_micro_pt,
                color,
            } => {
                append_math_rectangle(
                    commands,
                    object,
                    x_micro_pt,
                    y_micro_pt,
                    width_micro_pt,
                    height_micro_pt,
                    Some(math_color(color, default_foreground)),
                    None,
                    default_foreground,
                )?;
            }
            MathLayoutItem::Line {
                x1_micro_pt,
                y1_micro_pt,
                x2_micro_pt,
                y2_micro_pt,
                thickness_micro_pt,
                color,
            } => {
                let line = PageGlyphLine {
                    x1_pt: object.x_pt + micro_pt(x1_micro_pt),
                    y1_pt: object.baseline_y_pt + micro_pt(y1_micro_pt),
                    x2_pt: object.x_pt + micro_pt(x2_micro_pt),
                    y2_pt: object.baseline_y_pt + micro_pt(y2_micro_pt),
                    thickness_pt: micro_pt(thickness_micro_pt),
                    color: math_color(color, default_foreground),
                    line_clip_y_pt: object.line_clip_y_pt,
                };
                if ![
                    line.x1_pt,
                    line.y1_pt,
                    line.x2_pt,
                    line.y2_pt,
                    line.thickness_pt,
                ]
                .iter()
                .all(|value| value.is_finite())
                    || line.thickness_pt < 0.0
                {
                    return Err(PageGlyphError::InvalidRasterGeometry);
                }
                if line.thickness_pt > 0.0 {
                    commands.push(PageGlyphDrawCommand::Line(line));
                }
            }
            MathLayoutItem::Rectangle {
                x_micro_pt,
                y_micro_pt,
                width_micro_pt,
                height_micro_pt,
                fill,
                stroke,
            } => append_math_rectangle(
                commands,
                object,
                x_micro_pt,
                y_micro_pt,
                width_micro_pt,
                height_micro_pt,
                fill.map(|color| math_color(Some(color), default_foreground)),
                stroke.map(|(color, thickness)| {
                    (math_color(color, default_foreground), micro_pt(thickness))
                }),
                default_foreground,
            )?,
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_math_rectangle(
    commands: &mut Vec<PageGlyphDrawCommand>,
    object: &PageMathObject,
    x_micro_pt: i64,
    y_micro_pt: i64,
    width_micro_pt: i64,
    height_micro_pt: i64,
    fill: Option<[u8; 4]>,
    stroke: Option<([u8; 4], f64)>,
    _default_foreground: [u8; 4],
) -> Result<(), PageGlyphError> {
    let x_pt = object.x_pt + micro_pt(x_micro_pt);
    let y_pt = object.baseline_y_pt + micro_pt(y_micro_pt);
    let width_pt = micro_pt(width_micro_pt);
    let height_pt = micro_pt(height_micro_pt);
    if ![x_pt, y_pt, width_pt, height_pt]
        .iter()
        .all(|value| value.is_finite())
        || width_pt < 0.0
        || height_pt < 0.0
        || !valid_clip(object.line_clip_y_pt)
    {
        return Err(PageGlyphError::InvalidRasterGeometry);
    }
    if let Some(color) = fill.filter(|_| width_pt > 0.0 && height_pt > 0.0) {
        commands.push(PageGlyphDrawCommand::Rectangle(PageGlyphDecoration {
            x_pt,
            y_pt,
            width_pt,
            height_pt,
            color: Some(color),
            line_clip_y_pt: object.line_clip_y_pt,
        }));
    }
    if let Some((color, thickness)) = stroke {
        if !thickness.is_finite() || thickness < 0.0 {
            return Err(PageGlyphError::InvalidRasterGeometry);
        }
        if thickness > 0.0 && width_pt > thickness && height_pt > thickness {
            let strips = if height_pt <= 2.0 * thickness {
                let split = height_pt / 2.0;
                vec![
                    (x_pt, y_pt, width_pt, split),
                    (x_pt, y_pt + split, width_pt, height_pt - split),
                ]
            } else if width_pt <= 2.0 * thickness {
                let split = width_pt / 2.0;
                vec![
                    (x_pt, y_pt, split, height_pt),
                    (x_pt + split, y_pt, width_pt - split, height_pt),
                ]
            } else {
                vec![
                    (x_pt, y_pt, width_pt, thickness),
                    (x_pt, y_pt + height_pt - thickness, width_pt, thickness),
                    (
                        x_pt,
                        y_pt + thickness,
                        thickness,
                        height_pt - 2.0 * thickness,
                    ),
                    (
                        x_pt + width_pt - thickness,
                        y_pt + thickness,
                        thickness,
                        height_pt - 2.0 * thickness,
                    ),
                ]
            };
            for (x, y, width, height) in strips {
                if width <= 0.0 || height <= 0.0 {
                    continue;
                }
                commands.push(PageGlyphDrawCommand::Rectangle(PageGlyphDecoration {
                    x_pt: x,
                    y_pt: y,
                    width_pt: width,
                    height_pt: height,
                    color: Some(color),
                    line_clip_y_pt: object.line_clip_y_pt,
                }));
            }
        }
    }
    Ok(())
}
