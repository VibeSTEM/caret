//! 分式与无横线堆叠（`\binom` 类）的排版。
//
// SPDX-License-Identifier: Apache-2.0
//
// 本文件移植自 Typst：
//   仓库：https://github.com/typst/typst
//   原路径：crates/typst-layout/src/math/fraction.rs
//   提交：84cca406aafe7f830ed3f37f1cc6f0c8b6b65a03（v0.15.1）
//   版权：Copyright 2023 Typst Project Developers，Apache License 2.0
//   修改：改为接收已排好的分子分母片段与 Caret 样式；横线以实心矩形输出；去掉斜分式与描边样式。

use read_fonts::tables::math::MathConstant as C;

use super::context::{Context, MathSize, Style};
use super::frame::{Fragment, Frame, Item, Point};
use crate::symbols::AtomClass;

/// 排版分式。`line` 为假时按堆叠（无横线）处理；`padding` 是横线两端各自的延伸量。
pub(crate) fn layout_fraction(
    ctx: &Context<'_>,
    style: Style,
    num: Fragment,
    denom: Fragment,
    line: bool,
    padding: f64,
) -> Fragment {
    let display = style.size == MathSize::Display;
    let pick = |display_constant, text_constant| {
        ctx.styled(
            if display {
                display_constant
            } else {
                text_constant
            },
            style,
        )
    };
    let num = num.frame;
    let denom = denom.frame;

    let frame = if line {
        let axis = ctx.styled(C::AxisHeight, style);
        let thickness = ctx.styled(C::FractionRuleThickness, style);
        let shift_up = pick(
            C::FractionNumeratorDisplayStyleShiftUp,
            C::FractionNumeratorShiftUp,
        );
        let shift_down = pick(
            C::FractionDenominatorDisplayStyleShiftDown,
            C::FractionDenominatorShiftDown,
        );
        let num_min = pick(C::FractionNumDisplayStyleGapMin, C::FractionNumeratorGapMin);
        let denom_min = pick(
            C::FractionDenomDisplayStyleGapMin,
            C::FractionDenominatorGapMin,
        );

        let num_gap = (shift_up - (axis + thickness / 2.0) - num.descent()).max(num_min);
        let denom_gap = (shift_down + (axis - thickness / 2.0) - denom.ascent()).max(denom_min);

        let line_width = num.width.max(denom.width);
        let width = line_width + 2.0 * padding;
        let height = num.height + num_gap + thickness + denom_gap + denom.height;
        let num_pos = Point::new((width - num.width) / 2.0, 0.0);
        let line_y = num.height + num_gap + thickness / 2.0;
        let denom_pos = Point::new((width - denom.width) / 2.0, height - denom.height);
        let baseline = line_y + axis;

        let mut frame = Frame::new(width, height, baseline);
        frame.push_frame(num_pos, num);
        frame.push_frame(denom_pos, denom);
        frame.push(
            Point::new((width - line_width) / 2.0, line_y - thickness / 2.0),
            Item::Rule {
                width: line_width,
                height: thickness,
                color: style.color,
            },
        );
        frame
    } else {
        let shift_up = pick(C::StackTopDisplayStyleShiftUp, C::StackTopShiftUp);
        let shift_down = pick(C::StackBottomDisplayStyleShiftDown, C::StackBottomShiftDown);
        let gap_min = pick(C::StackDisplayStyleGapMin, C::StackGapMin);

        let gap = (shift_up - num.descent()) + (shift_down - denom.ascent());
        let width = num.width.max(denom.width) + 2.0 * padding;
        let height = num.height + gap.max(gap_min) + denom.height;
        let num_pos = Point::new((width - num.width) / 2.0, 0.0);
        let denom_pos = Point::new((width - denom.width) / 2.0, height - denom.height);
        let baseline = num.ascent() + shift_up + (gap_min - gap).max(0.0) / 2.0;

        let mut frame = Frame::new(width, height, baseline);
        frame.push_frame(num_pos, num);
        frame.push_frame(denom_pos, denom);
        frame
    };
    Fragment::new(frame, AtomClass::Inner)
}
