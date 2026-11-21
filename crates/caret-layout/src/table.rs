//! 表格网格几何：按已测量的单元格内容计算等分列轨道、行高与单元格内容盒。
//
// 本模块只消费调用方测量好的单元格高度/首基线，不负责文本断行、公式排版或分页；
// 列宽策略、内容重排与分页片段由布局调用方（046）决定。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_grid_layout::{
    CellMetrics, GridRow, GridSpec, HorizontalAlignment, RowSpacing, layout_grid,
};

#[cfg(test)]
mod tests;

/// 表格网格的算法常量与版心宽度。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableLayoutOptions {
    /// 单元格内容区可用总宽度（不含边框与内边距，含列间隙），单位为 pt；必须有限且为正。
    pub content_width_pt: f64,
    /// 列数；必须大于零。
    pub column_count: usize,
    /// 单元格内容区四周内边距，单位为 pt；必须有限且非负。
    pub cell_padding_pt: f64,
    /// 表格外边线宽度，计入左右内缩；必须有限且非负。
    pub border_width_pt: f64,
    /// 相邻列之间的间隙，单位为 pt；必须有限且非负。
    pub column_gap_pt: f64,
    /// 每列的最小宽度，单位为 pt；必须有限且非负。
    pub minimum_column_width_pt: f64,
    /// 每行至少保留的基线上方高度，用于空行/空单元格占位；必须有限且非负。
    pub minimum_row_ascent_pt: f64,
    /// 每行至少保留的基线下方高度；必须有限且非负。
    pub minimum_row_descent_pt: f64,
}

impl TableLayoutOptions {
    fn validate(&self) -> Result<(), TableLayoutError> {
        let finite_non_negative = |value: f64| value.is_finite() && value >= 0.0;
        if !self.content_width_pt.is_finite() || self.content_width_pt <= 0.0 {
            return Err(TableLayoutError::InvalidOptions);
        }
        if self.column_count == 0 {
            return Err(TableLayoutError::InvalidOptions);
        }
        if !finite_non_negative(self.cell_padding_pt)
            || !finite_non_negative(self.border_width_pt)
            || !finite_non_negative(self.column_gap_pt)
            || !finite_non_negative(self.minimum_column_width_pt)
            || !finite_non_negative(self.minimum_row_ascent_pt)
            || !finite_non_negative(self.minimum_row_descent_pt)
        {
            return Err(TableLayoutError::InvalidOptions);
        }
        Ok(())
    }
}

/// 一个已测量单元格的内容盒度量。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableCellMeasurement {
    /// 单元格内容总高度（不含内边距），单位为 pt；必须有限且非负。
    pub content_height_pt: f64,
    /// 首个内容基线与内容区顶边的距离，单位为 pt；必须有限且非负。
    pub first_baseline_pt: f64,
}

impl TableCellMeasurement {
    fn validate(&self) -> Result<(), TableLayoutError> {
        if !self.content_height_pt.is_finite()
            || self.content_height_pt < 0.0
            || !self.first_baseline_pt.is_finite()
            || self.first_baseline_pt < 0.0
        {
            return Err(TableLayoutError::InvalidMeasurement);
        }
        Ok(())
    }
}

/// 一行中按列顺序排列的已测量单元格。
#[derive(Debug, Clone, PartialEq)]
pub struct TableRowMeasurement {
    pub cells: Vec<TableCellMeasurement>,
}

/// 单个单元格在表格内的几何位置。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableCellGeometry {
    /// 列下标，从零开始。
    pub column_index: usize,
    /// 单元格内容区左上角相对表格原点的位置，单位为 pt。
    pub content_x_pt: f64,
    pub content_y_pt: f64,
    /// 内容区尺寸，单位为 pt。
    pub content_width_pt: f64,
    pub content_height_pt: f64,
    /// 首基线相对表格原点的 y 坐标，单位为 pt。
    pub first_baseline_y_pt: f64,
}

/// 一行的几何范围与各单元格位置。
#[derive(Debug, Clone, PartialEq)]
pub struct TableRowGeometry {
    /// 行下标，从零开始。
    pub row_index: usize,
    /// 行上边缘相对表格原点的 y 坐标，单位为 pt。
    pub top_pt: f64,
    pub height_pt: f64,
    pub cells: Vec<TableCellGeometry>,
}

/// 表格网格的最终几何结果；原点为表格左上角，x 向右、y 向下。
#[derive(Debug, Clone, PartialEq)]
pub struct TableGridLayout {
    pub width_pt: f64,
    pub height_pt: f64,
    /// 各列轨道的 `(左边缘 x, 宽度)`，单位为 pt。
    pub columns: Vec<(f64, f64)>,
    /// 各横线的 y 坐标，单位为 pt；首尾为表格上下边线。
    pub horizontal_rule_positions: Vec<f64>,
    /// 各竖线的 x 坐标，单位为 pt；首尾为表格左右边线。
    pub vertical_rule_positions: Vec<f64>,
    pub rows: Vec<TableRowGeometry>,
}

/// 表格网格输入或几何无效时拒绝产生部分结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableLayoutError {
    InvalidOptions,
    InvalidMeasurement,
    NonFiniteResult,
}

impl std::fmt::Display for TableLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidOptions => "layout.invalid_table_options",
            Self::InvalidMeasurement => "layout.invalid_table_measurement",
            Self::NonFiniteResult => "layout.invalid_table_geometry",
        })
    }
}

impl std::error::Error for TableLayoutError {}

impl From<caret_grid_layout::GridLayoutError> for TableLayoutError {
    fn from(_: caret_grid_layout::GridLayoutError) -> Self {
        Self::InvalidMeasurement
    }
}

/// 按等分列宽与自然行高计算表格网格几何。
///
/// 列宽取 `(可用宽度 - 两侧边线内缩 - 列间隙) / 列数` 与最小列宽的较大值；
/// 行高为行内单元格内容高度的最大值加上下内边距，且不低于调用方给出的最小行度量。
/// 单元格内容按顶部对齐；跨单元格的首基线对齐由后续切片决定，不在 v1 几何中表达。
pub fn layout_table_grid(
    rows: &[TableRowMeasurement],
    options: TableLayoutOptions,
) -> Result<TableGridLayout, TableLayoutError> {
    options.validate()?;
    for row in rows {
        if row.cells.len() != options.column_count {
            return Err(TableLayoutError::InvalidMeasurement);
        }
        for cell in &row.cells {
            cell.validate()?;
        }
    }

    let gaps = options.column_gap_pt * (options.column_count.saturating_sub(1) as f64);
    let available = options.content_width_pt - gaps;
    if !available.is_finite() {
        return Err(TableLayoutError::NonFiniteResult);
    }
    let column_width =
        (available / options.column_count as f64).max(options.minimum_column_width_pt);
    if !column_width.is_finite() || column_width <= 0.0 {
        return Err(TableLayoutError::NonFiniteResult);
    }

    let grid_rows = rows
        .iter()
        .map(|row| {
            GridRow::Cells(
                row.cells
                    .iter()
                    .map(|cell| CellMetrics {
                        width: column_width,
                        ascent: options.cell_padding_pt + cell.first_baseline_pt,
                        descent: options.cell_padding_pt + cell.content_height_pt
                            - cell.first_baseline_pt,
                    })
                    .collect(),
            )
        })
        .collect::<Vec<_>>();

    let spec = GridSpec {
        column_widths: vec![Some(column_width); options.column_count],
        column_alignments: vec![HorizontalAlignment::Left; options.column_count],
        vertical_rules: Vec::new(),
        horizontal_rules: Vec::new(),
        column_gap: options.column_gap_pt,
        row_gap: 0.0,
        minimum_row_ascent: options.minimum_row_ascent_pt,
        minimum_row_descent: options.minimum_row_descent_pt,
        row_spacing: Some(RowSpacing::Natural),
        base_row_ascent: options.minimum_row_ascent_pt,
        base_row_descent: options.minimum_row_descent_pt,
        base_row_leading: 0.0,
    };
    let grid = layout_grid(&grid_rows, &spec)?;

    let horizontal_inset = options.border_width_pt + options.cell_padding_pt;
    let vertical_inset = options.border_width_pt;
    let mut laid_out_rows = Vec::with_capacity(rows.len());
    for (row_index, row) in rows.iter().enumerate() {
        let Some(grid_row) = grid.rows.get(row_index) else {
            return Err(TableLayoutError::NonFiniteResult);
        };
        let mut cells = Vec::with_capacity(row.cells.len());
        for (column_index, cell) in row.cells.iter().enumerate() {
            let Some((column_x, _)) = grid.columns.get(column_index).copied() else {
                return Err(TableLayoutError::NonFiniteResult);
            };
            cells.push(TableCellGeometry {
                column_index,
                content_x_pt: horizontal_inset + column_x,
                content_y_pt: vertical_inset + grid_row.top + options.cell_padding_pt,
                content_width_pt: column_width,
                content_height_pt: cell.content_height_pt,
                first_baseline_y_pt: vertical_inset
                    + grid_row.top
                    + options.cell_padding_pt
                    + cell.first_baseline_pt,
            });
        }
        laid_out_rows.push(TableRowGeometry {
            row_index,
            top_pt: vertical_inset + grid_row.top,
            height_pt: grid_row.ascent + grid_row.descent,
            cells,
        });
    }

    let width_pt = 2.0 * options.border_width_pt + 2.0 * options.cell_padding_pt + grid.width;
    let height_pt = 2.0 * vertical_inset + grid.height;
    let columns = grid
        .columns
        .iter()
        .map(|(x, width)| (horizontal_inset + x, *width))
        .collect::<Vec<_>>();
    let mut horizontal_rule_positions = Vec::with_capacity(laid_out_rows.len() + 1);
    horizontal_rule_positions.push(0.0);
    horizontal_rule_positions.extend(laid_out_rows.iter().map(|row| row.top_pt + row.height_pt));
    let mut vertical_rule_positions = Vec::with_capacity(columns.len() + 1);
    vertical_rule_positions.push(0.0);
    vertical_rule_positions.extend(columns.iter().map(|(x, width)| x + width));
    let result = TableGridLayout {
        width_pt,
        height_pt,
        columns,
        horizontal_rule_positions,
        vertical_rule_positions,
        rows: laid_out_rows,
    };
    let finite = |value: f64| value.is_finite();
    if !finite(result.width_pt)
        || !finite(result.height_pt)
        || result
            .columns
            .iter()
            .any(|(x, width)| !finite(*x) || !finite(*width))
        || result
            .horizontal_rule_positions
            .iter()
            .chain(result.vertical_rule_positions.iter())
            .any(|position| !finite(*position))
        || result.rows.iter().any(|row| {
            !finite(row.top_pt)
                || !finite(row.height_pt)
                || row.cells.iter().any(|cell| {
                    !finite(cell.content_x_pt)
                        || !finite(cell.content_y_pt)
                        || !finite(cell.content_width_pt)
                        || !finite(cell.content_height_pt)
                        || !finite(cell.first_baseline_y_pt)
                })
        })
    {
        return Err(TableLayoutError::NonFiniteResult);
    }
    Ok(result)
}

/// 计算 v1 等分列宽；调用方可用于在测量单元格内容前确定每段可用宽度。
///
/// 结果与 [`layout_table_grid`] 内部使用的列宽一致：可用宽度扣除列间隙后等分，
/// 再与最小列宽取较大值。
pub fn table_column_width_pt(options: &TableLayoutOptions) -> Result<f64, TableLayoutError> {
    options.validate()?;
    let gaps = options.column_gap_pt * (options.column_count.saturating_sub(1) as f64);
    let available = options.content_width_pt - gaps;
    if !available.is_finite() {
        return Err(TableLayoutError::NonFiniteResult);
    }
    let width = (available / options.column_count as f64).max(options.minimum_column_width_pt);
    if !width.is_finite() || width <= 0.0 {
        return Err(TableLayoutError::NonFiniteResult);
    }
    Ok(width)
}
