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
    /// 表格外框可用的最大宽度，单位为 pt；必须有限且为正。
    /// 内容列宽会扣除左右外边框、每格左右内边距与显式列间隙后再等分。
    pub available_width_pt: f64,
    /// 列数；必须大于零。
    pub column_count: usize,
    /// 单元格内容区四周内边距，单位为 pt；必须有限且非负。
    pub cell_padding_pt: f64,
    /// 表格外边线宽度，计入左右内缩；必须有限且非负。
    pub border_width_pt: f64,
    /// 相邻单元格内边距之间的额外间隙，单位为 pt；必须有限且非负。
    pub column_gap_pt: f64,
    /// 每列的最小宽度，单位为 pt；必须有限且非负。
    pub minimum_column_width_pt: f64,
    /// 每行至少保留的基线上方高度，用于空行/空单元格占位；必须有限且非负。
    pub minimum_row_ascent_pt: f64,
    /// 每行至少保留的基线下方高度；必须有限且非负。
    pub minimum_row_descent_pt: f64,
}

impl TableLayoutOptions {
    /// 文档表格 v1 的默认内边距/边线/最小列宽；行最小度量由正文行框提供。
    pub const DEFAULT_CELL_PADDING_PT: f64 = 4.0;
    pub const DEFAULT_BORDER_WIDTH_PT: f64 = 0.5;
    pub const DEFAULT_COLUMN_GAP_PT: f64 = 0.0;
    pub const DEFAULT_MINIMUM_COLUMN_WIDTH_PT: f64 = 24.0;

    /// 使用文档表格默认视觉参数构造等分列表格选项。
    pub fn text_defaults(
        available_width_pt: f64,
        column_count: usize,
        minimum_row_ascent_pt: f64,
        minimum_row_descent_pt: f64,
    ) -> Self {
        Self {
            available_width_pt,
            column_count,
            cell_padding_pt: Self::DEFAULT_CELL_PADDING_PT,
            border_width_pt: Self::DEFAULT_BORDER_WIDTH_PT,
            column_gap_pt: Self::DEFAULT_COLUMN_GAP_PT,
            minimum_column_width_pt: Self::DEFAULT_MINIMUM_COLUMN_WIDTH_PT,
            minimum_row_ascent_pt,
            minimum_row_descent_pt,
        }
    }

    fn validate(&self) -> Result<(), TableLayoutError> {
        let finite_non_negative = |value: f64| value.is_finite() && value >= 0.0;
        if !self.available_width_pt.is_finite() || self.available_width_pt <= 0.0 {
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

/// 单元格内一个已测量块（段落、公式盒或嵌套表盒）的垂直度量。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasuredCellBlock {
    /// 块自身高度，单位为 pt；必须有限且非负。
    pub height_pt: f64,
    /// 块顶到首基线的距离，单位为 pt；无基线的盒为 0。
    pub first_baseline_pt: f64,
    /// 与上一块的段前距；首块忽略，单位为 pt。
    pub space_before_pt: f64,
    /// 与下一块的段后距，单位为 pt。
    pub space_after_pt: f64,
}

impl MeasuredCellBlock {
    fn validate(&self) -> Result<(), TableLayoutError> {
        let values = [
            self.height_pt,
            self.first_baseline_pt,
            self.space_before_pt,
            self.space_after_pt,
        ];
        if values
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(TableLayoutError::InvalidMeasurement);
        }
        Ok(())
    }
}

/// 一个已测量单元格的内容盒度量。
#[derive(Debug, Clone, PartialEq)]
pub struct TableCellMeasurement {
    pub blocks: Vec<MeasuredCellBlock>,
}

impl TableCellMeasurement {
    fn validate(&self) -> Result<(), TableLayoutError> {
        if self.blocks.is_empty() {
            return Err(TableLayoutError::InvalidMeasurement);
        }
        for block in &self.blocks {
            block.validate()?;
        }
        Ok(())
    }

    /// 块顶（含块间段距）与高度；首块段前距忽略。
    fn block_tops(&self) -> Vec<f64> {
        let mut tops = Vec::with_capacity(self.blocks.len());
        let mut y = 0.0_f64;
        let mut previous_space_after = 0.0_f64;
        for (index, block) in self.blocks.iter().enumerate() {
            if index > 0 {
                y += previous_space_after.max(block.space_before_pt);
            }
            tops.push(y);
            y += block.height_pt;
            previous_space_after = block.space_after_pt;
        }
        tops
    }

    fn content_height_pt(&self) -> f64 {
        self.blocks
            .last()
            .zip(self.block_tops().last())
            .map_or(0.0, |(block, top)| top + block.height_pt)
    }

    fn first_baseline_pt(&self) -> f64 {
        self.blocks
            .first()
            .map_or(0.0, |block| block.first_baseline_pt)
    }
}

/// 一行中按列顺序排列的已测量单元格。
#[derive(Debug, Clone, PartialEq)]
pub struct TableRowMeasurement {
    pub cells: Vec<TableCellMeasurement>,
}

/// 单元格内一个块的放置几何；坐标相对单元格内容区顶边。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableCellBlockGeometry {
    pub top_pt: f64,
    /// 块首基线相对单元格内容区顶边的 y 坐标；无基线时为 `top_pt`。
    pub first_baseline_y_pt: f64,
    pub height_pt: f64,
}

/// 单个单元格在表格内的几何位置。
#[derive(Debug, Clone, PartialEq)]
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
    /// 单元格内各块相对内容区顶边的放置信息。
    pub blocks: Vec<TableCellBlockGeometry>,
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
    /// 各列内容区的 `(左边缘 x, 宽度)`，单位为 pt。
    pub columns: Vec<(f64, f64)>,
    /// 各横线的 y 坐标，单位为 pt；首尾为表格上下边线。
    pub horizontal_rule_positions: Vec<f64>,
    /// 各竖线的 x 坐标，单位为 pt；首尾为表格左右边线。
    pub vertical_rule_positions: Vec<f64>,
    pub rows: Vec<TableRowGeometry>,
}

impl TableGridLayout {
    /// 返回行范围的上下网格线坐标，单位为 pt。范围为空或超出网格时返回 `None`。
    pub fn row_rule_bounds_pt(&self, rows: std::ops::Range<usize>) -> Option<[f64; 2]> {
        if rows.is_empty() || rows.end > self.rows.len() {
            return None;
        }
        Some([
            *self.horizontal_rule_positions.get(rows.start)?,
            *self.horizontal_rule_positions.get(rows.end)?,
        ])
    }
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
/// 列宽取 `(可用宽度 - 两侧边线 - 每格左右内边距 - 显式列间隙) / 列数`
/// 与最小列宽的较大值。
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

    let column_width = table_column_width_pt(&options)?;

    let grid_rows = rows
        .iter()
        .map(|row| {
            GridRow::Cells(
                row.cells
                    .iter()
                    .map(|cell| CellMetrics {
                        width: column_width,
                        ascent: options.cell_padding_pt + cell.first_baseline_pt(),
                        descent: options.cell_padding_pt + cell.content_height_pt()
                            - cell.first_baseline_pt(),
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
        // 列轨道间距包括相邻两格的内边距和显式列间隙。
        column_gap: 2.0 * options.cell_padding_pt + options.column_gap_pt,
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
            let tops = cell.block_tops();
            let blocks = cell
                .blocks
                .iter()
                .zip(tops)
                .map(|(block, top_pt)| TableCellBlockGeometry {
                    top_pt,
                    first_baseline_y_pt: top_pt + block.first_baseline_pt,
                    height_pt: block.height_pt,
                })
                .collect();
            cells.push(TableCellGeometry {
                column_index,
                content_x_pt: horizontal_inset + column_x,
                content_y_pt: vertical_inset + grid_row.top + options.cell_padding_pt,
                content_width_pt: column_width,
                content_height_pt: cell.content_height_pt(),
                first_baseline_y_pt: vertical_inset
                    + grid_row.top
                    + options.cell_padding_pt
                    + cell.first_baseline_pt(),
                blocks,
            });
        }
        laid_out_rows.push(TableRowGeometry {
            row_index,
            top_pt: vertical_inset + grid_row.top,
            height_pt: grid_row.ascent + grid_row.descent,
            cells,
        });
    }

    let width_pt = 2.0 * horizontal_inset + grid.width;
    let height_pt = 2.0 * vertical_inset + grid.height;
    let columns = grid
        .columns
        .iter()
        .map(|(x, width)| (horizontal_inset + x, *width))
        .collect::<Vec<_>>();
    let mut horizontal_rule_positions = Vec::with_capacity(laid_out_rows.len() + 1);
    horizontal_rule_positions.push(0.0);
    horizontal_rule_positions.extend(
        laid_out_rows
            .iter()
            .take(laid_out_rows.len().saturating_sub(1))
            .map(|row| row.top_pt + row.height_pt),
    );
    horizontal_rule_positions.push(height_pt);
    let mut vertical_rule_positions = Vec::with_capacity(columns.len() + 1);
    vertical_rule_positions.push(0.0);
    vertical_rule_positions.extend(
        columns
            .iter()
            .take(columns.len().saturating_sub(1))
            .map(|(x, width)| x + width + options.cell_padding_pt + options.column_gap_pt / 2.0),
    );
    vertical_rule_positions.push(width_pt);
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
/// 结果与 [`layout_table_grid`] 内部使用的列宽一致：可用宽度扣除边框、每格内边距
/// 和显式列间隙后等分，再与最小列宽取较大值。
pub fn table_column_width_pt(options: &TableLayoutOptions) -> Result<f64, TableLayoutError> {
    options.validate()?;
    let gaps = options.column_gap_pt * (options.column_count.saturating_sub(1) as f64);
    let cell_padding = 2.0 * options.cell_padding_pt * options.column_count as f64;
    let available =
        options.available_width_pt - 2.0 * options.border_width_pt - cell_padding - gaps;
    if !available.is_finite() {
        return Err(TableLayoutError::NonFiniteResult);
    }
    let width = (available / options.column_count as f64).max(options.minimum_column_width_pt);
    if !width.is_finite() || width <= 0.0 {
        return Err(TableLayoutError::NonFiniteResult);
    }
    Ok(width)
}
