//! 根据调用方提供的已测量行列尺寸计算共享网格几何。
//
// SPDX-License-Identifier: Apache-2.0
//
// 本文件移植自 Typst：
//   仓库：https://github.com/typst/typst
//   原路径：crates/typst-layout/src/math/table.rs
//   提交：9dfd3a08500b7896045f907433cf7b4b02434fad（v0.15.1）
//   版权：Copyright 2023 Typst Project Developers，Apache License 2.0
//   修改：抽取为不依赖数学片段、字体度量或文档模型的纯几何 API；支持调用方指定列宽、行最小基线度量和整行内容。

#[derive(Debug, Clone, Copy, PartialEq)]
/// 调用方测量得到的单元格尺寸，所有度量使用同一套布局单位。
///
/// 水平位置向右、垂直位置向下；基线向上到 ink 顶部的距离记为 `ascent`，
/// 从基线向下到 ink 底部的距离记为 `descent`。三者都是 signed 度量，
/// 负 advance 或超出基线方向的 ink 边界均可表示。
pub struct CellMetrics {
    /// 有符号水平 advance；负值可表示负水平空间。
    pub width: f64,
    /// 从基线向上到 ink 顶边的有符号距离；与布局的垂直坐标方向相反。
    pub ascent: f64,
    /// 从基线向下到 ink 底边的有符号距离。
    pub descent: f64,
}

#[derive(Debug, Clone, PartialEq)]
/// 网格中的一行：按列放置的单元格，或占用整行宽度的内容。
pub enum GridRow {
    /// 按列放置测量单元格；行宽由各列内容和 `GridSpec` 的列宽决定。
    Cells(Vec<CellMetrics>),
    /// 不参与列宽测量、从网格左边缘放置的整行内容。
    Full(CellMetrics),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 单元格在其列轨道中的水平对齐方式。
pub enum HorizontalAlignment {
    /// 单元格左边与轨道左边对齐。
    Left,
    /// 单元格在轨道内居中；也是缺少对应对齐项时的默认值。
    Center,
    /// 单元格右边与轨道右边对齐。
    Right,
}

#[derive(Debug, Clone, PartialEq)]
/// 计算网格布局的输入规则。长度和坐标都使用与 `CellMetrics` 相同的布局单位。
pub struct GridSpec {
    /// 预先决定的列轨道宽度；`Some(width)` 是精确宽度，`None` 或缺少的项按 max-content 测量。
    /// 精确宽度不得为负；内容超过轨道时轨道宽度保持不变，并由结果的溢出标志报告。
    pub column_widths: Vec<Option<f64>>,
    /// 每列的水平对齐方式；缺少对应项时使用居中对齐。
    pub column_alignments: Vec<HorizontalAlignment>,
    /// 竖线所在列边界索引；索引 `k` 表示第 `k` 列之前，`0` 与列数分别是左右边缘。
    /// 线位于预留列间距的中心，索引必须不大于网格列数；整行内容扩宽网格时，右边缘线跟随最终宽度。
    pub vertical_rules: Vec<usize>,
    /// 横线所在行边界索引；索引 `k` 表示第 `k` 行之前，`0` 与行数分别是上下边缘。
    /// 线位于预留行间距的中心，索引必须不大于输入行数；空网格的 `0` 是唯一边界。
    pub horizontal_rules: Vec<usize>,
    /// 相邻列或带竖线的列边界间距，单位为布局单位；必须有限且非负。
    pub column_gap: f64,
    /// 相邻行或带横线的行边界间距，单位为布局单位；必须有限且非负。
    pub row_gap: f64,
    /// 每行最小基线上方距离，单位为布局单位；作为 signed 行度量的下限，必须有限且非负。
    pub minimum_row_ascent: f64,
    /// 每行最小基线下方距离，单位为布局单位；作为 signed 行度量的下限，必须有限且非负。
    pub minimum_row_descent: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// 单元格在网格坐标中的放置点；原点为网格左上角，`x` 向右、`baseline` 向下。
pub struct CellPosition {
    /// 网格左边缘起算的水平坐标，单位为布局单位。
    pub x: f64,
    /// 网格上边缘起算、向下为正的基线坐标，单位为布局单位。
    pub baseline: f64,
}

#[derive(Debug, Clone, PartialEq)]
/// 一行的几何结果；坐标原点为网格左上角，垂直坐标向下为正。
pub struct RowLayout {
    /// 行框上边缘的 y 坐标，单位为布局单位。
    pub top: f64,
    /// 行基线的 y 坐标，单位为布局单位。
    pub baseline: f64,
    /// 该行采用的基线上方距离，单位为布局单位；输入 signed 度量与最小值取较大者。
    pub ascent: f64,
    /// 该行采用的基线下方距离，单位为布局单位；输入 signed 度量与最小值取较大者。
    pub descent: f64,
    /// 与输入 `GridRow::Cells` 中的单元格逐项对应的位置；整行内容没有此项。
    pub cells: Vec<CellPosition>,
    /// `GridRow::Full` 内容的位置；非整行内容时为 `None`。整行内容从 x=0 放置。
    pub full: Option<CellPosition>,
}

#[derive(Debug, Clone, PartialEq)]
/// 网格的最终几何结果。所有坐标以左上角为原点，x 向右、y 向下，单位为布局单位。
pub struct GridLayout {
    /// 网格总宽度，单位为布局单位。
    pub width: f64,
    /// 网格总高度，单位为布局单位。
    pub height: f64,
    /// 各列轨道的 `(左边缘 x, 宽度)`，单位为布局单位。
    pub columns: Vec<(f64, f64)>,
    /// 各行的布局结果，与输入行逐项对应。
    pub rows: Vec<RowLayout>,
    /// 各竖线的 x 坐标，单位为布局单位。
    pub vertical_rule_positions: Vec<f64>,
    /// 各横线的 y 坐标，单位为布局单位。
    pub horizontal_rule_positions: Vec<f64>,
    /// 单元格宽于其列轨道，或整行内容宽于含已决列宽的网格时为 `true`。
    /// 整行内容在没有任何 `Some` 已决列宽时可扩展网格宽度，不因此报告溢出。
    pub content_overflow: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 布局输入或计算结果中的可恢复错误；调用方可修正输入后重试。
pub enum GridLayoutError {
    /// 输入度量不是有限值，或间距、行最小度量、精确列宽为负。
    InvalidMetric,
    /// 有限输入的算术运算产生了非有限的输出几何。
    NonFiniteResult,
    /// 网格线索引超过对应的列数或行数。
    RuleOutOfBounds,
}

impl std::fmt::Display for GridLayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMetric => {
                f.write_str("grid metrics must be finite; gaps and row minima must be non-negative")
            }
            Self::NonFiniteResult => f.write_str("grid layout result is not finite"),
            Self::RuleOutOfBounds => f.write_str("grid rule index is out of bounds"),
        }
    }
}

impl std::error::Error for GridLayoutError {}

/// 根据已测量的行和网格规则计算列轨道、基线、单元格位置及网格线坐标。
///
/// `rows` 中各单元格的宽度、基线上下距离以及 `spec` 中的长度必须使用同一布局单位。
/// 网格原点为左上角，x 向右、y 向下。缺少列对齐项时使用居中；`Some` 列宽为精确轨道，
/// `None` 或缺少的列宽按 max-content 测量。整行内容从 x=0 开始；没有任何精确列宽时，
/// 它可以扩大网格宽度，且右边缘竖线随最终宽度移动；存在精确列宽时超宽内容通过 `content_overflow` 报告。
///
/// 空 `rows` 仍可由 `column_widths` 声明列轨道；横线索引 `0` 是唯一行边界，最多产生一条
/// 横线并预留一次 `row_gap`。无行且无该横线时高度为零。
///
/// # Errors
///
/// 返回 [`GridLayoutError::InvalidMetric`]（度量非法）、[`GridLayoutError::RuleOutOfBounds`]
///（线索引越界）或 [`GridLayoutError::NonFiniteResult`]（运算溢出）。这些错误不修改输入，
/// 调用方可以修正度量或索引后重试。
pub fn layout_grid(rows: &[GridRow], spec: &GridSpec) -> Result<GridLayout, GridLayoutError> {
    let valid = |value: f64| value.is_finite() && value >= 0.0;
    let valid_cell = |cell: &CellMetrics| {
        cell.width.is_finite() && cell.ascent.is_finite() && cell.descent.is_finite()
    };
    if !valid(spec.column_gap)
        || !valid(spec.row_gap)
        || !valid(spec.minimum_row_ascent)
        || !valid(spec.minimum_row_descent)
        || spec
            .column_widths
            .iter()
            .flatten()
            .any(|value| !valid(*value))
        || rows.iter().any(|row| match row {
            GridRow::Cells(cells) => cells.iter().any(|cell| !valid_cell(cell)),
            GridRow::Full(cell) => !valid_cell(cell),
        })
    {
        return Err(GridLayoutError::InvalidMetric);
    }

    let columns_count = rows
        .iter()
        .map(|row| match row {
            GridRow::Cells(cells) => cells.len(),
            GridRow::Full(_) => 0,
        })
        .max()
        .unwrap_or(0)
        .max(spec.column_widths.len());
    if spec.vertical_rules.iter().any(|rule| *rule > columns_count)
        || spec.horizontal_rules.iter().any(|rule| *rule > rows.len())
    {
        return Err(GridLayoutError::RuleOutOfBounds);
    }

    let mut widths = vec![0.0_f64; columns_count];
    let mut heights = vec![(spec.minimum_row_ascent, spec.minimum_row_descent); rows.len()];
    for (row_index, row) in rows.iter().enumerate() {
        match row {
            GridRow::Cells(cells) => {
                for (column, cell) in cells.iter().enumerate() {
                    widths[column] = widths[column].max(cell.width.max(0.0));
                    heights[row_index].0 = heights[row_index].0.max(cell.ascent);
                    heights[row_index].1 = heights[row_index].1.max(cell.descent);
                }
            }
            GridRow::Full(cell) => {
                heights[row_index].0 = heights[row_index].0.max(cell.ascent);
                heights[row_index].1 = heights[row_index].1.max(cell.descent);
            }
        }
    }
    let has_resolved_widths = spec.column_widths.iter().any(Option::is_some);
    for (index, width) in spec.column_widths.iter().enumerate() {
        if let Some(width) = width {
            widths[index] = *width;
        }
    }

    let mut columns = Vec::with_capacity(columns_count);
    let mut vertical_rule_positions = Vec::new();
    let mut x = 0.0;
    for column in 0..=columns_count {
        if spec.vertical_rules.contains(&column) {
            vertical_rule_positions.push(x + spec.column_gap / 2.0);
            x += spec.column_gap;
        } else if column > 0 && column < columns_count {
            x += spec.column_gap;
        }
        if let Some(width) = widths.get(column) {
            columns.push((x, *width));
            x += width;
        }
    }
    let mut total_width = x;
    let mut content_overflow = rows.iter().any(|row| match row {
        GridRow::Cells(cells) => cells
            .iter()
            .enumerate()
            .any(|(column, cell)| cell.width > widths[column]),
        GridRow::Full(_) => false,
    });
    for row in rows {
        if let GridRow::Full(cell) = row
            && cell.width > total_width
        {
            if has_resolved_widths {
                content_overflow = true;
            } else {
                total_width = cell.width;
            }
        }
    }
    if !has_resolved_widths
        && spec.vertical_rules.contains(&columns_count)
        && let Some(right_rule) = vertical_rule_positions.last_mut()
    {
        *right_rule = total_width - spec.column_gap / 2.0;
    }

    let mut y = if spec.horizontal_rules.contains(&0) {
        spec.row_gap
    } else {
        0.0
    };
    let mut horizontal_rule_positions = Vec::new();
    if rows.is_empty() && spec.horizontal_rules.contains(&0) {
        horizontal_rule_positions.push(spec.row_gap / 2.0);
    }
    let mut row_layouts = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            if spec.horizontal_rules.contains(&index) {
                horizontal_rule_positions.push(y + spec.row_gap / 2.0);
            }
            y += spec.row_gap;
        } else if spec.horizontal_rules.contains(&0) {
            horizontal_rule_positions.push(spec.row_gap / 2.0);
        }
        let (ascent, descent) = heights[index];
        let baseline = y + ascent;
        let mut layout = RowLayout {
            top: y,
            baseline,
            ascent,
            descent,
            cells: Vec::new(),
            full: None,
        };
        match row {
            GridRow::Cells(cells) => {
                for (column, cell) in cells.iter().enumerate() {
                    let (column_x, column_width) = columns[column];
                    let slack = column_width - cell.width;
                    let offset = match spec
                        .column_alignments
                        .get(column)
                        .copied()
                        .unwrap_or(HorizontalAlignment::Center)
                    {
                        HorizontalAlignment::Left => 0.0,
                        HorizontalAlignment::Center => slack / 2.0,
                        HorizontalAlignment::Right => slack,
                    };
                    layout.cells.push(CellPosition {
                        x: column_x + offset,
                        baseline,
                    });
                }
            }
            GridRow::Full(_) => layout.full = Some(CellPosition { x: 0.0, baseline }),
        }
        row_layouts.push(layout);
        y += ascent + descent;
    }
    if !rows.is_empty() && spec.horizontal_rules.contains(&rows.len()) {
        horizontal_rule_positions.push(y + spec.row_gap / 2.0);
        y += spec.row_gap;
    }

    let grid = GridLayout {
        width: total_width,
        height: y,
        columns,
        rows: row_layouts,
        vertical_rule_positions,
        horizontal_rule_positions,
        content_overflow,
    };
    let finite = |value: f64| value.is_finite();
    if !finite(grid.width)
        || !finite(grid.height)
        || grid
            .columns
            .iter()
            .any(|(x, width)| !finite(*x) || !finite(*width))
        || grid.rows.iter().any(|row| {
            !finite(row.top)
                || !finite(row.baseline)
                || !finite(row.ascent)
                || !finite(row.descent)
                || row
                    .cells
                    .iter()
                    .any(|cell| !finite(cell.x) || !finite(cell.baseline))
                || row
                    .full
                    .is_some_and(|cell| !finite(cell.x) || !finite(cell.baseline))
        })
        || grid
            .vertical_rule_positions
            .iter()
            .any(|position| !finite(*position))
        || grid
            .horizontal_rule_positions
            .iter()
            .any(|position| !finite(*position))
    {
        return Err(GridLayoutError::NonFiniteResult);
    }
    Ok(grid)
}

#[cfg(test)]
mod tests;
