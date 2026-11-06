//! 矩阵、`cases`、`array` 与对齐环境的表格排版。
//
// SPDX-License-Identifier: Apache-2.0
//
// 本文件移植自 Typst：
//   仓库：https://github.com/typst/typst
//   原路径：crates/typst-layout/src/math/table.rs
//   提交：9dfd3a08500b7896045f907433cf7b4b02434fad（v0.15.1）
//   版权：Copyright 2023 Typst Project Developers，Apache License 2.0
//   修改：网格列宽、行高、对齐和网格线坐标已抽取至 caret-grid-layout；本适配器保留数学片段、数学轴线、字号线宽和行节点盒；
//   单元格为单个片段（按列指定对齐）；支持整行（`\intertext`）；为矩阵行输出稳定节点 ID 命中盒。

use caret_grid_layout::{CellMetrics, GridRow, GridSpec, HorizontalAlignment, layout_grid};
use read_fonts::tables::math::MathConstant as C;

use super::context::{Context, Style};
use caret_doc::NodeId;

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
    Cells {
        id: Option<NodeId>,
        cells: Vec<Fragment>,
    },
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
    ctx: &mut Context<'_>,
    style: Style,
    rows: Vec<TableRow>,
    spec: &TableSpec,
) -> Fragment {
    if rows.is_empty() {
        return Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord);
    }

    let (paren_ascent, paren_descent) =
        glyph_fragment(ctx, style.denominator(), '(', AtomClass::Open)
            .map(|g| (g.ascent(), g.descent()))
            .unwrap_or((0.0, 0.0));

    let stroke = STROKE_EM * ctx.font_size(style);
    let grid_rows: Vec<_> = rows
        .iter()
        .map(|row| match row {
            TableRow::Cells { cells, .. } => GridRow::Cells(cells.iter().map(metrics).collect()),
            TableRow::Full(full) => GridRow::Full(metrics(full)),
        })
        .collect();
    let grid_spec = GridSpec {
        // 显式保留源算法配置的空列。
        column_widths: vec![None; spec.aligns.len()],
        column_alignments: spec
            .aligns
            .iter()
            .map(|align| match align {
                Align::Left => HorizontalAlignment::Left,
                Align::Center => HorizontalAlignment::Center,
                Align::Right => HorizontalAlignment::Right,
            })
            .collect(),
        vertical_rules: spec.vlines.clone(),
        horizontal_rules: spec.hlines.clone(),
        column_gap: spec.column_gap,
        row_gap: spec.row_gap,
        minimum_row_ascent: paren_ascent,
        minimum_row_descent: paren_descent,
    };
    let grid = match layout_grid(&grid_rows, &grid_spec) {
        Ok(grid) => grid,
        Err(error) => {
            ctx.diagnostic("math.grid.layout", error.to_string());
            return Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord);
        }
    };

    let axis = ctx.styled(C::AxisHeight, style);
    let mut frame = Frame::new(grid.width, grid.height, grid.height / 2.0 + axis);
    for (r, row) in rows.into_iter().enumerate() {
        let row_layout = &grid.rows[r];
        match row {
            TableRow::Cells { id, cells } => {
                if let Some(id) = id {
                    frame.push(
                        Point::new(0.0, row_layout.top),
                        Item::Node {
                            id,
                            width: grid.width,
                            height: row_layout.ascent + row_layout.descent,
                            baseline: row_layout.ascent,
                        },
                    );
                }
                for (cell, position) in cells.into_iter().zip(&row_layout.cells) {
                    let pos = Point::new(position.x, position.baseline - cell.ascent());
                    frame.push_frame(pos, cell.frame);
                }
            }
            TableRow::Full(full) => {
                let position = row_layout.full.unwrap_or(caret_grid_layout::CellPosition {
                    x: 0.0,
                    baseline: row_layout.baseline,
                });
                let pos = Point::new(position.x, position.baseline - full.ascent());
                frame.push_frame(pos, full.frame);
            }
        }
    }
    for vx in grid.vertical_rule_positions {
        frame.push(
            Point::new(vx - stroke / 2.0, 0.0),
            Item::Rule {
                width: stroke,
                height: grid.height,
                color: style.color,
            },
        );
    }
    for hy in grid.horizontal_rule_positions {
        frame.push(
            Point::new(0.0, hy - stroke / 2.0),
            Item::Rule {
                width: grid.width,
                height: stroke,
                color: style.color,
            },
        );
    }
    Fragment::new(frame, AtomClass::Ord)
}

fn metrics(fragment: &Fragment) -> CellMetrics {
    CellMetrics {
        width: fragment.width(),
        ascent: fragment.ascent(),
        descent: fragment.descent(),
    }
}
