//! 重音、上下横线与删除线（`\cancel` 系列）的排版。
//
// SPDX-License-Identifier: Apache-2.0
//
// 本文件移植自 Typst：
//   仓库：https://github.com/typst/typst
//   原路径：crates/typst-layout/src/math/accent.rs、line.rs、cancel.rs
//   提交：9dfd3a08500b7896045f907433cf7b4b02434fad（v0.15.1）
//   版权：Copyright 2023 Typst Project Developers，Apache License 2.0
//   修改：合并三个文件；去掉 flac 字形替换与精确帧宽选项；横线改为实心矩形；
//   删除线固定沿包围盒对角线并输出为 Caret 的线段项；新增 `\cancelto` 的目标放置。

use read_fonts::tables::math::MathConstant as C;

use super::context::{Context, Style};
use super::frame::{Fragment, Frame, Item, Point};
use super::glyph::stretch;
use crate::fonts::Axis;

/// 删除线粗细（pt）。
const CANCEL_STROKE: f64 = 0.5;
/// 删除线比对角线多出的长度（pt）。
const CANCEL_EXTRA: f64 = 3.0;

/// 重音：`accent` 为已排好的重音字形片段，按基底宽度横向伸展。
pub(crate) fn layout_accent(
    ctx: &Context<'_>,
    style: Style,
    base: Fragment,
    accent: Fragment,
    top: bool,
) -> Fragment {
    let size = ctx.font_size(style);
    let base_attach = base.accent_attach;
    let accent = stretch(ctx, accent, Axis::X, base.width(), 0.5 * size);
    let accent_attach = if top {
        accent.accent_attach.0
    } else {
        accent.accent_attach.1
    };
    let accent = accent.frame;
    let width = base.width();
    let accent_x = if top { base_attach.0 } else { base_attach.1 } - accent_attach;

    let (gap, accent_pos, base_pos) = if top {
        let accent_base_height = ctx.styled(C::AccentBaseHeight, style);
        let gap = -accent.descent() - base.ascent().min(accent_base_height);
        (
            gap,
            Point::new(accent_x, 0.0),
            Point::new(0.0, accent.height + gap),
        )
    } else {
        let gap = -accent.ascent();
        (
            gap,
            Point::new(accent_x, base.height() + gap),
            Point::new(0.0, 0.0),
        )
    };
    let height = accent.height + gap + base.height();
    let baseline = base_pos.y + base.ascent();

    let class = base.class;
    let text_like = base.text_like;
    let italics = base.italics_correction;
    let (base_ascent, base_descent) = (base.base_ascent, base.base_descent);
    let glyph = base.glyph;
    let mut frame = Frame::new(width, height, baseline);
    frame.push_frame(base_pos, base.frame);
    frame.push_frame(accent_pos, accent);
    let mut fragment = Fragment::new(frame, class);
    fragment.base_ascent = base_ascent;
    fragment.base_descent = base_descent;
    fragment.italics_correction = italics;
    fragment.text_like = text_like;
    fragment.accent_attach = base_attach;
    // 保留基底字形以便脚标读取其角落字距。
    fragment.glyph = glyph;
    fragment
}

/// `\overline` / `\underline`。
pub(crate) fn layout_line(
    ctx: &Context<'_>,
    style: Style,
    content: Fragment,
    above: bool,
) -> Fragment {
    let (extra_height, line_y, content_y, baseline, thickness, line_adjust);
    if above {
        let sep = ctx.styled(C::OverbarExtraAscender, style);
        thickness = ctx.styled(C::OverbarRuleThickness, style);
        let gap = ctx.styled(C::OverbarVerticalGap, style);
        extra_height = sep + thickness + gap;
        line_y = sep;
        content_y = extra_height;
        baseline = content.ascent() + extra_height;
        line_adjust = 0.0;
    } else {
        let sep = ctx.styled(C::UnderbarExtraDescender, style);
        thickness = ctx.styled(C::UnderbarRuleThickness, style);
        let gap = ctx.styled(C::UnderbarVerticalGap, style);
        extra_height = sep + thickness + gap;
        line_y = content.height() + gap;
        content_y = 0.0;
        baseline = content.ascent();
        line_adjust = -content.italics_correction;
    }
    let width = content.width();
    let height = content.height() + extra_height;
    let text_like = content.text_like;
    let italics = content.italics_correction;
    let class = content.class;
    let mut frame = Frame::new(width, height, baseline);
    frame.push_frame(Point::new(0.0, content_y), content.frame);
    frame.push(
        Point::new(0.0, line_y),
        Item::Rule {
            width: (width + line_adjust).max(0.0),
            height: thickness,
            color: style.color,
        },
    );
    let mut fragment = Fragment::new(frame, class);
    fragment.italics_correction = italics;
    fragment.text_like = text_like;
    fragment
}

/// 删除线方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelKind {
    /// `\cancel`：左下到右上。
    Forward,
    /// `\bcancel`：左上到右下。
    Back,
    /// `\xcancel`：两条交叉。
    Cross,
}

/// 沿包围盒对角线画删除线，线长为对角线长加 3pt，以中心对称。
pub(crate) fn layout_cancel(style: Style, body: Fragment, kind: CancelKind) -> Fragment {
    let mut fragment = body;
    let (width, height) = (fragment.width(), fragment.height());
    let diagonal = (width * width + height * height).sqrt();
    if diagonal <= f64::EPSILON {
        return fragment;
    }
    let half = (diagonal + CANCEL_EXTRA) / 2.0;
    let (ux, uy) = (width / diagonal, height / diagonal);
    let (cx, cy) = (width / 2.0, height / 2.0);
    let mut line = |sign: f64| {
        // sign 为 1 时从左下到右上（y 向下，所以 dy 为负）。
        let start = Point::new(cx - half * ux, cy + sign * half * uy);
        fragment.frame.push(
            start,
            Item::Line {
                dx: 2.0 * half * ux,
                dy: -sign * 2.0 * half * uy,
                thickness: CANCEL_STROKE,
                color: style.color,
            },
        );
    };
    match kind {
        CancelKind::Forward => line(1.0),
        CancelKind::Back => line(-1.0),
        CancelKind::Cross => {
            line(1.0);
            line(-1.0);
        }
    }
    fragment
}

/// `\cancelto{target}{body}`：带删除线，目标值放在线的右上端之外。
pub(crate) fn layout_cancel_to(style: Style, body: Fragment, target: Fragment) -> Fragment {
    let crossed = layout_cancel(style, body, CancelKind::Forward);
    let (width, height) = (crossed.width(), crossed.height());
    let diagonal = (width * width + height * height).sqrt().max(f64::EPSILON);
    let half = (diagonal + CANCEL_EXTRA) / 2.0;
    let end_x = width / 2.0 + half * width / diagonal;
    let end_y = height / 2.0 - half * height / diagonal;
    let target_x = end_x;
    let target_y = end_y - target.height();
    let top = target_y.min(0.0);
    let total_width = width.max(target_x + target.width());
    let total_height = height - top;
    let class = crossed.class;
    let baseline = crossed.ascent() - top;
    let mut frame = Frame::new(total_width, total_height, baseline);
    frame.push_frame(Point::new(0.0, -top), crossed.frame);
    frame.push_frame(Point::new(target_x, target_y - top), target.frame);
    Fragment::new(frame, class)
}
