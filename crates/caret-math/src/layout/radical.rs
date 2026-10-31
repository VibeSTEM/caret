//! 根式排版：根号按被开方式高度伸展，带可选根指数。
//
// SPDX-License-Identifier: Apache-2.0
//
// 本文件移植自 Typst：
//   仓库：https://github.com/typst/typst
//   原路径：crates/typst-layout/src/math/radical.rs
//   提交：84cca406aafe7f830ed3f37f1cc6f0c8b6b65a03（v0.15.1）
//   版权：Copyright 2023 Typst Project Developers，Apache License 2.0
//   修改：改为接收已排好的片段与 Caret 样式；顶线以实心矩形输出；去掉描边样式分支。

use read_fonts::tables::math::MathConstant as C;

use super::context::{Context, MathSize, Style};
use super::font::Axis;
use super::frame::{Fragment, Frame, Item, Point};
use super::glyph::{glyph_fragment, stretch};
use crate::symbols::AtomClass;

/// 排版根式；TeXbook 第 443 页规则 11 与第 360 页 `\root` 定义。
pub(crate) fn layout_radical(
    ctx: &Context<'_>,
    style: Style,
    radicand: Fragment,
    index: Option<Fragment>,
) -> Fragment {
    let radicand = radicand.frame;
    let thickness = ctx.styled(C::RadicalRuleThickness, style);
    let gap = ctx.styled(
        if style.size == MathSize::Display {
            C::RadicalDisplayStyleVerticalGap
        } else {
            C::RadicalVerticalGap
        },
        style,
    );
    let target = radicand.height + thickness + gap;

    let Some(sqrt) = glyph_fragment(ctx, style, '√', AtomClass::Ord) else {
        return Fragment::new(radicand, AtomClass::Ord);
    };
    let sqrt = stretch(ctx, sqrt, Axis::Y, target, 0.0).frame;

    let extra_ascender = ctx.styled(C::RadicalExtraAscender, style);
    let kern_before = ctx.styled(C::RadicalKernBeforeDegree, style);
    let kern_after = ctx.styled(C::RadicalKernAfterDegree, style);
    let raise_factor = ctx.font.constant(C::RadicalDegreeBottomRaisePercent) / 100.0;
    let line_width = radicand.width;
    let index = index.map(|index| index.frame);

    // 保留原间隙，再把剩余空间平均分到上下。
    let gap = gap.max((sqrt.height - thickness - radicand.height + gap) / 2.0);
    let sqrt_ascent = radicand.ascent() + gap + thickness;
    let descent = sqrt.height - sqrt_ascent;
    let inner_ascent = sqrt_ascent + extra_ascender;

    let mut sqrt_offset = 0.0;
    let mut shift_up = 0.0;
    let mut ascent = inner_ascent;
    if let Some(index) = &index {
        sqrt_offset = kern_before + index.width + kern_after;
        // 额外加上根指数下伸，避免下伸部分与根号相撞。
        shift_up = raise_factor * (inner_ascent - descent) + index.descent();
        ascent = ascent.max(shift_up + index.ascent());
    }

    let sqrt_x = sqrt_offset.max(0.0);
    let radicand_x = sqrt_x + sqrt.width;
    let radicand_y = ascent - radicand.ascent();
    let width = radicand_x + line_width;
    let height = ascent + descent;

    let sqrt_pos = Point::new(sqrt_x, radicand_y - gap - thickness);
    let line_top = radicand_y - gap - thickness;
    let radicand_pos = Point::new(radicand_x, radicand_y);

    let mut frame = Frame::new(width, height, ascent);
    if let Some(index) = index {
        let index_x = -sqrt_offset.min(0.0) + kern_before;
        let index_y = ascent - index.ascent() - shift_up;
        frame.push_frame(Point::new(index_x, index_y), index);
    }
    frame.push_frame(sqrt_pos, sqrt);
    frame.push(
        Point::new(radicand_x, line_top),
        Item::Rule {
            width: line_width,
            height: thickness,
            color: style.color,
        },
    );
    frame.push_frame(radicand_pos, radicand);
    Fragment::new(frame, AtomClass::Ord)
}
