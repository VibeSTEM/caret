//! 单字形片段、字形伸展（预制变体与组装件）、轴心对齐与 MATH 角落字距。
//
// SPDX-License-Identifier: Apache-2.0
//
// 本文件移植自 Typst：
//   仓库：https://github.com/typst/typst
//   原路径：crates/typst-layout/src/math/fragment/glyph.rs
//   提交：9dfd3a08500b7896045f907433cf7b4b02434fad（v0.15.1）
//   版权：Copyright 2023 Typst Project Developers，Apache License 2.0
//   修改：改写为 Caret 的 read-fonts 字体封装与左上角帧模型；去掉 ssty/flac 字形替换、
//   字体回退与 span 记录；组装件直接展开为逐字形定位项。

use read_fonts::tables::math::MathConstant;

use super::context::{Context, Style};
use super::font::{Assembly, AssemblyPart, Axis, Corner};
use super::frame::{Fragment, Frame, GlyphSource, Item, MathColor, Point};
use crate::symbols::AtomClass;

/// 组装件中延伸段的最大重复次数，防止病态字体导致无限循环。
const MAX_REPEATS: usize = 1024;

/// 由字符建立字形片段；字体缺字时返回 `None`。
pub(crate) fn glyph_fragment(
    ctx: &Context<'_>,
    style: Style,
    character: char,
    class: AtomClass,
) -> Option<Fragment> {
    let glyph = ctx.font.glyph(character)?;
    Some(glyph_id_fragment(
        ctx,
        glyph,
        ctx.font_size(style),
        style.color,
        class,
        style.is_script(),
    ))
}

/// 由字形 ID 建立片段：宽度含斜体校正（扩展形状除外），上下伸以墨迹边界为准。
pub(crate) fn glyph_id_fragment(
    ctx: &Context<'_>,
    glyph: u16,
    size: f64,
    color: Option<MathColor>,
    class: AtomClass,
    script: bool,
) -> Fragment {
    let font = ctx.font;
    let extended_shape = font.is_extended_shape(glyph);
    let italics = ctx.units(font.italics_correction(glyph).unwrap_or_default(), size);
    let advance = ctx.units(font.advance(glyph), size);
    let width = if extended_shape {
        advance
    } else {
        advance + italics
    };
    let (ascent, descent) = font
        .bounds(glyph)
        .map(|bounds| {
            (
                ctx.units(bounds.y_max, size),
                -ctx.units(bounds.y_min, size),
            )
        })
        .unwrap_or((0.0, 0.0));
    // 重音附着点缺省为宽度与斜体校正之和的一半；下重音不使用上附着点。
    let top_attach = font
        .top_accent_attachment(glyph)
        .map(|value| ctx.units(value, size))
        .unwrap_or((advance + italics) / 2.0);
    let bottom_attach = (advance - italics) / 2.0;

    let mut frame = Frame::new(width, ascent + descent, ascent);
    frame.push(Point::new(0.0, ascent), Item::Glyph { glyph, size, color });
    let mut fragment = Fragment::new(frame, class);
    fragment.italics_correction = italics;
    fragment.accent_attach = (top_attach, bottom_attach);
    fragment.text_like = !extended_shape;
    fragment.script = script;
    fragment.glyph = Some(GlyphSource { glyph, size });
    fragment.stretchy_x = font.is_stretchable(glyph, Axis::X);
    fragment
}

/// 片段沿某方向用于伸展比较的尺寸；横向去掉 `glyph_id_fragment` 加上的斜体校正。
fn stretch_advance(ctx: &Context<'_>, fragment: &Fragment, axis: Axis) -> f64 {
    match axis {
        Axis::X => {
            let extended = fragment
                .glyph
                .is_some_and(|source| ctx.font.is_extended_shape(source.glyph));
            if extended {
                fragment.width()
            } else {
                fragment.width() - fragment.italics_correction
            }
        }
        Axis::Y => fragment.height(),
    }
}

/// 把单字形片段伸展到目标尺寸；允许短缺 `short_fall`。非字形片段原样返回。
pub(crate) fn stretch(
    ctx: &Context<'_>,
    fragment: Fragment,
    axis: Axis,
    target: f64,
    short_fall: f64,
) -> Fragment {
    let Some(source) = fragment.glyph else {
        return fragment;
    };
    let advance = stretch_advance(ctx, &fragment, axis);
    let short_target = target - short_fall;
    if short_target <= advance {
        return fragment;
    }
    let Some(construction) = ctx.font.construction(source.glyph, axis) else {
        return fragment;
    };
    let color = first_color(&fragment.frame);

    // 先找足够大的预制变体。
    let mut best = source.glyph;
    let mut best_advance = advance;
    for (variant, measurement) in &construction.variants {
        best = *variant;
        best_advance = ctx.units(*measurement, source.size);
        if short_target <= best_advance {
            break;
        }
    }

    let rebuilt = |glyph: u16| {
        let mut next = glyph_id_fragment(
            ctx,
            glyph,
            source.size,
            color,
            fragment.class,
            fragment.script,
        );
        next.limits = fragment.limits;
        next.large = fragment.large;
        next
    };

    match construction.assembly {
        Some(assembly) if short_target > best_advance => {
            let mut assembled = assemble(
                ctx,
                &assembly,
                source.size,
                color,
                target,
                axis,
                fragment.class,
            );
            assembled.limits = fragment.limits;
            assembled.large = fragment.large;
            assembled.script = fragment.script;
            assembled
        }
        _ => rebuilt(best),
    }
}

fn first_color(frame: &Frame) -> Option<MathColor> {
    frame.items.iter().find_map(|(_, item)| match item {
        Item::Glyph { color, .. } => *color,
        _ => None,
    })
}

/// 展开组装件：确定延伸段重复次数与连接段重叠比例，然后逐段定位。
fn assemble(
    ctx: &Context<'_>,
    assembly: &Assembly,
    size: f64,
    color: Option<MathColor>,
    target: f64,
    axis: Axis,
    class: AtomClass,
) -> Fragment {
    let font = ctx.font;
    let min_overlap = ctx.units(font.min_connector_overlap(), size);
    let units = |value: f64| ctx.units(value, size);

    // ratio：0 表示最大重叠，1 表示最小重叠。
    let mut full;
    let mut ratio;
    let mut repeat = 0;
    loop {
        full = 0.0;
        ratio = 0.0;
        let selected = parts(assembly, repeat);
        let mut growable = 0.0;
        for (index, part) in selected.iter().enumerate() {
            let mut advance = units(part.full_advance);
            if let Some(next) = selected.get(index + 1) {
                let max_overlap = units(part.end_connector.min(next.start_connector));
                advance -= max_overlap;
                growable += (max_overlap - min_overlap).max(0.0);
            }
            full += advance;
        }
        if full < target {
            let delta = target - full;
            ratio = if growable > 0.0 {
                (delta / growable).min(1.0)
            } else {
                0.0
            };
            full += ratio * growable;
        }
        if target <= full || repeat >= MAX_REPEATS {
            break;
        }
        repeat += 1;
    }

    let selected = parts(assembly, repeat);
    let mut placed = Vec::with_capacity(selected.len());
    let mut offset = 0.0;
    for (index, part) in selected.iter().enumerate() {
        let mut advance = units(part.full_advance);
        if let Some(next) = selected.get(index + 1) {
            let max_overlap = units(part.end_connector.min(next.start_connector));
            advance -= max_overlap;
            advance += ratio * (max_overlap - min_overlap);
        }
        placed.push((part.glyph, offset));
        offset += advance;
    }

    let extents = |glyph: u16| {
        font.bounds(glyph)
            .map(|bounds| (units(bounds.y_max), -units(bounds.y_min)))
            .unwrap_or((0.0, 0.0))
    };

    let mut frame = match axis {
        Axis::X => {
            let (ascent, descent) = placed
                .iter()
                .map(|(glyph, _)| extents(*glyph))
                .fold((0.0_f64, 0.0_f64), |(a, d), (ga, gd)| {
                    (a.max(ga), d.max(gd))
                });
            let mut frame = Frame::new(full, ascent + descent, ascent);
            for (glyph, x) in &placed {
                frame.push(
                    Point::new(*x, ascent),
                    Item::Glyph {
                        glyph: *glyph,
                        size,
                        color,
                    },
                );
            }
            frame
        }
        Axis::Y => {
            let width = placed
                .iter()
                .map(|(glyph, _)| units(font.advance(*glyph)))
                .fold(0.0_f64, f64::max);
            // 竖直组装件的各段按字形原点对齐；墨迹低于基线的字形按下伸上移。
            let mut frame = Frame::new(width, full, full);
            for (glyph, y) in &placed {
                let descent = extents(*glyph).1;
                frame.push(
                    Point::new(0.0, full - y - descent),
                    Item::Glyph {
                        glyph: *glyph,
                        size,
                        color,
                    },
                );
            }
            frame
        }
    };
    frame.items.shrink_to_fit();
    let mut fragment = Fragment::new(frame, class);
    fragment.italics_correction = units(assembly.italics_correction);
    if axis == Axis::X {
        fragment.accent_attach = (full / 2.0, full / 2.0);
    }
    fragment
}

/// 按重复次数展开组装段：延伸段重复 `repeat` 次，其余各出现一次。
fn parts(assembly: &Assembly, repeat: usize) -> Vec<AssemblyPart> {
    assembly
        .parts
        .iter()
        .flat_map(|part| {
            let count = if part.extender { repeat } else { 1 };
            std::iter::repeat_n(*part, count)
        })
        .collect()
}

/// 竖直方向以数学轴为中心重新设定基线。
pub(crate) fn center_on_axis(ctx: &Context<'_>, fragment: &mut Fragment, style: Style) {
    let axis = ctx.styled(MathConstant::AxisHeight, style);
    let height = fragment.height();
    let baseline = (height + axis * 2.0) / 2.0;
    fragment.frame.baseline = baseline;
    fragment.base_ascent = fragment.ascent();
    fragment.base_descent = fragment.descent();
}

/// 片段某角在给定高度处的 MATH 字距（pt）；非单字形片段为零。
pub(crate) fn kern_at_height(
    ctx: &Context<'_>,
    fragment: &Fragment,
    corner: Corner,
    height: f64,
) -> f64 {
    let Some(source) = fragment.glyph else {
        return 0.0;
    };
    let units_per_pt = ctx.font.units_per_em() / source.size;
    ctx.font
        .kern_at_height(source.glyph, corner, height * units_per_pt)
        .map(|value| ctx.units(value, source.size))
        .unwrap_or(0.0)
}
