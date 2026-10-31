//! 上下标与上下限的排版：位移、MATH 角落字距、斜体校正与宽度计算。
//
// SPDX-License-Identifier: Apache-2.0
//
// 本文件移植自 Typst：
//   仓库：https://github.com/typst/typst
//   原路径：crates/typst-layout/src/math/scripts.rs
//   提交：84cca406aafe7f830ed3f37f1cc6f0c8b6b65a03（v0.15.1）
//   版权：Copyright 2023 Typst Project Developers，Apache License 2.0
//   修改：只保留右上、右下与上、下四个位置（Caret 文档树没有前置上下标）；
//   常量改由 Caret 上下文读取；结果直接返回片段。

use read_fonts::tables::math::MathConstant as C;

use super::context::{Context, Style};
use super::font::Corner;
use super::frame::{Fragment, Frame, Point};
use super::glyph::kern_at_height;

/// 四个附着位置：右上、右下、上限、下限。
#[derive(Default)]
pub(crate) struct Attachments {
    pub sup: Option<Fragment>,
    pub sub: Option<Fragment>,
    pub top: Option<Fragment>,
    pub bottom: Option<Fragment>,
}

/// 排版带附着的基底。`stretchy` 表示基底是横向伸展的字形（如 `\overbrace`）。
pub(crate) fn layout_attachments(
    ctx: &Context<'_>,
    style: Style,
    base: Fragment,
    stretchy: bool,
    attachments: Attachments,
) -> Fragment {
    let Attachments {
        sup,
        sub,
        top,
        bottom,
    } = attachments;
    let class = base.class;
    let measure = |fragment: &Option<Fragment>, f: fn(&Fragment) -> f64| {
        fragment.as_ref().map(f).unwrap_or(0.0)
    };

    let (sup_shift, sub_shift) = if sup.is_none() && sub.is_none() {
        (0.0, 0.0)
    } else {
        compute_script_shifts(ctx, style, &base, &sup, &sub)
    };
    let (top_shift, bottom_shift) =
        compute_limit_shifts(ctx, style, &base, stretchy, &top, &bottom);

    let ascent = base
        .ascent()
        .max(sup_shift + measure(&sup, Fragment::ascent))
        .max(top_shift + measure(&top, Fragment::ascent));
    let descent = base
        .descent()
        .max(sub_shift + measure(&sub, Fragment::descent))
        .max(bottom_shift + measure(&bottom, Fragment::descent));
    let height = ascent + descent;

    // 上限向右、下限向左各偏移基底斜体校正的一半。
    let delta = base.italics_correction / 2.0;
    let (top_pre, top_post) = top
        .as_ref()
        .map(|t| {
            let half = (t.width() - base.width()) / 2.0;
            (half - delta, half + delta)
        })
        .unwrap_or_default();
    let (bottom_pre, bottom_post) = bottom
        .as_ref()
        .map(|b| {
            let half = (b.width() - base.width()) / 2.0;
            (half + delta, half - delta)
        })
        .unwrap_or_default();

    let space_after_script = ctx.styled(C::SpaceAfterScript, style);
    let (sup_post, sup_kern) = sup
        .as_ref()
        .map(|s| {
            let kern = math_kern(ctx, &base, s, sup_shift, Corner::TopRight);
            (space_after_script + s.width() + kern, kern)
        })
        .unwrap_or_default();
    // 基底的包围盒已含斜体校正，下标需左移该量（OpenType MATH 字距算法）。
    let (sub_post, sub_kern) = sub
        .as_ref()
        .map(|s| {
            let kern =
                math_kern(ctx, &base, s, sub_shift, Corner::BottomRight) - base.italics_correction;
            (space_after_script + s.width() + kern, kern)
        })
        .unwrap_or_default();

    let pre_width = top_pre.max(bottom_pre).max(0.0);
    let base_width = base.width();
    let post_width = top_post
        .max(bottom_post)
        .max(sup_post)
        .max(sub_post)
        .max(0.0);
    let width = pre_width + base_width + post_width;

    let mut frame = Frame::new(width, height, ascent);
    let base_y = ascent - base.ascent();
    let italics = base.italics_correction;
    let base_large = base.large;
    frame.push_frame(Point::new(pre_width, base_y), base.frame);
    if let Some(s) = sup {
        let y = ascent - sup_shift - s.ascent();
        frame.push_frame(Point::new(pre_width + base_width + sup_kern, y), s.frame);
    }
    if let Some(s) = sub {
        let y = ascent + sub_shift - s.ascent();
        frame.push_frame(Point::new(pre_width + base_width + sub_kern, y), s.frame);
    }
    if let Some(t) = top {
        let y = ascent - top_shift - t.ascent();
        frame.push_frame(Point::new(pre_width - top_pre, y), t.frame);
    }
    if let Some(b) = bottom {
        let y = ascent + bottom_shift - b.ascent();
        frame.push_frame(Point::new(pre_width - bottom_pre, y), b.frame);
    }
    let mut fragment = Fragment::new(frame, class);
    fragment.large = base_large;
    // 只有上下限时保留基底斜体校正，便于后续附着。
    if super::is_zero(sup_shift) && super::is_zero(sub_shift) {
        fragment.italics_correction = italics;
    }
    fragment
}

fn compute_limit_shifts(
    ctx: &Context<'_>,
    style: Style,
    base: &Fragment,
    stretchy: bool,
    top: &Option<Fragment>,
    bottom: &Option<Fragment>,
) -> (f64, f64) {
    let top_shift = top
        .as_ref()
        .map(|t| {
            if stretchy {
                let shift_up = ctx.styled(C::StretchStackTopShiftUp, style);
                let gap_min = ctx.styled(C::StretchStackGapAboveMin, style);
                let gap = (shift_up - base.ascent() - t.descent()).max(gap_min);
                base.ascent() + gap + t.descent()
            } else {
                let gap_min = ctx.styled(C::UpperLimitGapMin, style);
                let rise_min = ctx.styled(C::UpperLimitBaselineRiseMin, style);
                base.ascent() + rise_min.max(gap_min + t.descent())
            }
        })
        .unwrap_or(0.0);
    let bottom_shift = bottom
        .as_ref()
        .map(|b| {
            if stretchy {
                let shift_down = ctx.styled(C::StretchStackBottomShiftDown, style);
                let gap_min = ctx.styled(C::StretchStackGapBelowMin, style);
                let gap = (shift_down - base.descent() - b.ascent()).max(gap_min);
                base.descent() + gap + b.ascent()
            } else {
                let gap_min = ctx.styled(C::LowerLimitGapMin, style);
                let drop_min = ctx.styled(C::LowerLimitBaselineDropMin, style);
                base.descent() + drop_min.max(gap_min + b.ascent())
            }
        })
        .unwrap_or(0.0);
    (top_shift, bottom_shift)
}

fn compute_script_shifts(
    ctx: &Context<'_>,
    style: Style,
    base: &Fragment,
    sup: &Option<Fragment>,
    sub: &Option<Fragment>,
) -> (f64, f64) {
    let sup_shift_up = ctx.styled(
        if style.cramped {
            C::SuperscriptShiftUpCramped
        } else {
            C::SuperscriptShiftUp
        },
        style,
    );
    let sup_bottom_min = ctx.styled(C::SuperscriptBottomMin, style);
    let sup_bottom_max_with_sub = ctx.styled(C::SuperscriptBottomMaxWithSubscript, style);
    let sup_drop_max = ctx.styled(C::SuperscriptBaselineDropMax, style);
    let gap_min = ctx.styled(C::SubSuperscriptGapMin, style);
    let sub_shift_down = ctx.styled(C::SubscriptShiftDown, style);
    let sub_top_max = ctx.styled(C::SubscriptTopMax, style);
    let sub_drop_min = ctx.styled(C::SubscriptBaselineDropMin, style);

    let mut shift_up: f64 = 0.0;
    let mut shift_down: f64 = 0.0;
    let text_like = base.text_like;

    if let Some(sup) = sup {
        shift_up = shift_up
            .max(sup_shift_up)
            .max(if text_like {
                0.0
            } else {
                base.base_ascent - sup_drop_max
            })
            .max(sup_bottom_min + sup.descent());
    }
    if let Some(sub) = sub {
        shift_down = shift_down
            .max(sub_shift_down)
            .max(if text_like {
                0.0
            } else {
                base.base_descent + sub_drop_min
            })
            .max(sub.ascent() - sub_top_max);
    }
    if let (Some(sup), Some(sub)) = (sup, sub) {
        let sup_bottom = shift_up - sup.descent();
        let sub_top = sub.ascent() - shift_down;
        let gap = sup_bottom - sub_top;
        if gap < gap_min {
            let increase = gap_min - gap;
            let sup_only = (sup_bottom_max_with_sub - sup_bottom).clamp(0.0, increase);
            let rest = (increase - sup_only) / 2.0;
            shift_up += sup_only + rest;
            shift_down += rest;
        }
    }
    (shift_up, shift_down)
}

/// 脚标相对基底的字距：两个校正高度处“基底角 + 脚标对角”字距之和取较大者。
fn math_kern(
    ctx: &Context<'_>,
    base: &Fragment,
    script: &Fragment,
    shift: f64,
    corner: Corner,
) -> f64 {
    let (top, bottom) = match corner {
        Corner::TopLeft | Corner::TopRight => (base.ascent() - shift, shift - script.descent()),
        Corner::BottomLeft | Corner::BottomRight => {
            (script.ascent() - shift, shift - base.descent())
        }
    };
    let summed = |height: f64| {
        kern_at_height(ctx, base, corner, height)
            + kern_at_height(ctx, script, corner.inv(), height)
    };
    summed(top).max(summed(bottom))
}
