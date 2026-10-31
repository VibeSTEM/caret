//! 矩阵、`cases`、`array` 与对齐环境的表格排版。
//
// SPDX-License-Identifier: Apache-2.0
//
// 本文件移植自 Typst：
//   仓库：https://github.com/typst/typst
//   原路径：crates/typst-layout/src/math/table.rs
//   提交：84cca406aafe7f830ed3f37f1cc6f0c8b6b65a03（v0.15.1）
//   版权：Copyright 2023 Typst Project Developers，Apache License 2.0
//   修改：单元格改为单个片段（列对齐由 Caret 按列指定，不做单元格内子列）；
//   新增整行（`\intertext`）；横竖线以实心矩形输出；行高下限沿用 Typst 的“分母样式圆括号”。

use read_fonts::tables::math::MathConstant as C;

use super::context::{Context, Style};
use super::frame::{Fragment, Frame, Item, Point};
use super::glyph::glyph_fragment;
use crate::symbols::AtomClass;

/// 表格线粗细（em）。
const STROKE_EM: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    Left,
    Center,
    Right,
}

pub(crate) enum TableRow {
    Cells(Vec<Fragment>),
    /// 跨越全部列、左对齐的整行。
    Full(Fragment),
}

pub(crate) struct TableSpec {
    pub aligns: Vec<Align>,
    /// 竖线位置：k 表示第 k 列之前（0 为最左，列数为最右）。
    pub vlines: Vec<usize>,
    /// 横线位置：k 表示第 k 行之前。
    pub hlines: Vec<usize>,
    pub column_gap: f64,
    pub row_gap: f64,
}

pub(crate) fn layout_table(
    ctx: &Context<'_>,
    style: Style,
    rows: Vec<TableRow>,
    spec: &TableSpec,
) -> Fragment {
    let nrows = rows.len();
    let ncols = rows
        .iter()
        .map(|row| match row {
            TableRow::Cells(cells) => cells.len(),
            TableRow::Full(_) => 0,
        })
        .max()
        .unwrap_or(0)
        .max(spec.aligns.len());
    if nrows == 0 {
        return Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord);
    }

    let (paren_ascent, paren_descent) =
        glyph_fragment(ctx, style.denominator(), '(', AtomClass::Open)
            .map(|g| (g.ascent(), g.descent()))
            .unwrap_or((0.0, 0.0));

    let mut widths = vec![0.0_f64; ncols];
    let mut heights = vec![(paren_ascent, paren_descent); nrows];
    for (r, row) in rows.iter().enumerate() {
        let cells: &[Fragment] = match row {
            TableRow::Cells(cells) => cells,
            TableRow::Full(full) => std::slice::from_ref(full),
        };
        for (c, cell) in cells.iter().enumerate() {
            heights[r].0 = heights[r].0.max(cell.ascent());
            heights[r].1 = heights[r].1.max(cell.descent());
            if let TableRow::Cells(_) = row {
                widths[c] = widths[c].max(cell.width());
            }
        }
    }

    let gap_x = spec.column_gap;
    let gap_y = spec.row_gap;
    let stroke = STROKE_EM * ctx.font_size(style);

    // 列起点：竖线占据一个列间隙。
    let mut column_x = Vec::with_capacity(ncols);
    let mut x = 0.0;
    let mut vline_x = Vec::new();
    for c in 0..=ncols {
        if spec.vlines.contains(&c) {
            vline_x.push(x + gap_x / 2.0);
            x += gap_x;
        } else if c > 0 && c < ncols {
            x += gap_x;
        }
        if let Some(width) = widths.get(c) {
            column_x.push(x);
            x += width;
        }
    }
    let mut total_width = x;
    for row in &rows {
        if let TableRow::Full(full) = row {
            total_width = total_width.max(full.width());
        }
    }

    let top_pad = if spec.hlines.contains(&0) { gap_y } else { 0.0 };
    let mut row_y = Vec::with_capacity(nrows);
    let mut y = top_pad;
    let mut hline_y = Vec::new();
    for (r, &(ascent, descent)) in heights.iter().enumerate() {
        if r > 0 {
            if spec.hlines.contains(&r) {
                hline_y.push(y + gap_y / 2.0);
            }
            y += gap_y;
        } else if spec.hlines.contains(&0) {
            hline_y.push(gap_y / 2.0);
        }
        row_y.push(y);
        y += ascent + descent;
    }
    if spec.hlines.contains(&nrows) {
        hline_y.push(y + gap_y / 2.0);
        y += gap_y;
    }
    let total_height = y;

    let axis = ctx.styled(C::AxisHeight, style);
    let mut frame = Frame::new(total_width, total_height, total_height / 2.0 + axis);
    for (r, row) in rows.into_iter().enumerate() {
        let baseline_y = row_y[r] + heights[r].0;
        match row {
            TableRow::Cells(cells) => {
                for (c, cell) in cells.into_iter().enumerate() {
                    let slack = widths[c] - cell.width();
                    let dx = match spec.aligns.get(c).copied().unwrap_or(Align::Center) {
                        Align::Left => 0.0,
                        Align::Center => slack / 2.0,
                        Align::Right => slack,
                    };
                    let pos = Point::new(column_x[c] + dx, baseline_y - cell.ascent());
                    frame.push_frame(pos, cell.frame);
                }
            }
            TableRow::Full(full) => {
                let pos = Point::new(0.0, baseline_y - full.ascent());
                frame.push_frame(pos, full.frame);
            }
        }
    }
    for vx in vline_x {
        frame.push(
            Point::new(vx - stroke / 2.0, 0.0),
            Item::Rule {
                width: stroke,
                height: total_height,
                color: style.color,
            },
        );
    }
    for hy in hline_y {
        frame.push(
            Point::new(0.0, hy - stroke / 2.0),
            Item::Rule {
                width: total_width,
                height: stroke,
                color: style.color,
            },
        );
    }
    Fragment::new(frame, AtomClass::Ord)
}
