//! 定界符围合（`\left … \right`）：按内容平衡高度伸展两侧定界符。
//
// SPDX-License-Identifier: Apache-2.0
//
// 本文件移植自 Typst：
//   仓库：https://github.com/typst/typst
//   原路径：crates/typst-layout/src/math/fenced.rs、fragment/glyph.rs（定界符短缺量）
//   提交：9dfd3a08500b7896045f907433cf7b4b02434fad（v0.15.1）
//   版权：Copyright 2023 Typst Project Developers，Apache License 2.0
//   修改：改为接收已排好的内容片段；不支持 `\middle`；伸展后直接做轴心对齐。

use read_fonts::tables::math::MathConstant as C;

use super::context::{Context, Style};
use super::font::Axis;
use super::frame::Fragment;
use super::glyph::{center_on_axis, glyph_fragment, stretch};
use crate::symbols::AtomClass;

/// 定界符允许的短缺量（em）。
const DELIM_SHORT_FALL: f64 = 0.1;

/// 内容关于数学轴平衡后的高度。
pub(crate) fn balanced_height(ctx: &Context<'_>, style: Style, body: &[Fragment]) -> f64 {
    let axis = ctx.styled(C::AxisHeight, style);
    body.iter()
        .map(|f| 2.0 * (f.ascent() - axis).max(f.descent() + axis))
        .fold(0.0, f64::max)
}

/// 生成伸展到 `target` 高度并以轴为中心的定界符；缺字时返回 `None`。
pub(crate) fn delimiter(
    ctx: &Context<'_>,
    style: Style,
    character: char,
    class: AtomClass,
    target: f64,
) -> Option<Fragment> {
    let glyph = glyph_fragment(ctx, style, character, class)?;
    let short_fall = DELIM_SHORT_FALL * ctx.font_size(style);
    let mut stretched = stretch(ctx, glyph, Axis::Y, target, short_fall);
    center_on_axis(ctx, &mut stretched, style);
    stretched.text_like = false;
    Some(stretched)
}
