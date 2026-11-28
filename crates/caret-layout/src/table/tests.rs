// 表格网格几何测试。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{
    MeasuredCellBlock, TableCellMeasurement, TableLayoutError, TableLayoutOptions,
    TableRowMeasurement, layout_table_grid,
};

fn options() -> TableLayoutOptions {
    TableLayoutOptions {
        available_width_pt: 200.0,
        column_count: 2,
        cell_padding_pt: 4.0,
        border_width_pt: 1.0,
        column_gap_pt: 0.0,
        minimum_column_width_pt: 10.0,
        minimum_row_ascent_pt: 8.0,
        minimum_row_descent_pt: 2.0,
    }
}

fn cell(content_height_pt: f64, first_baseline_pt: f64) -> TableCellMeasurement {
    TableCellMeasurement {
        blocks: vec![MeasuredCellBlock {
            height_pt: content_height_pt,
            first_baseline_pt,
            space_before_pt: 0.0,
            space_after_pt: 0.0,
        }],
    }
}

fn row(left: TableCellMeasurement, right: TableCellMeasurement) -> TableRowMeasurement {
    TableRowMeasurement {
        cells: vec![left, right],
    }
}

fn close(left: f64, right: f64) {
    assert!((left - right).abs() < 1e-9, "expected {right}, got {left}");
}

#[test]
fn equal_columns_and_padded_cell_boxes_are_derived_from_content_width() {
    let layout = layout_table_grid(
        &[
            row(cell(20.0, 14.0), cell(30.0, 14.0)),
            row(cell(10.0, 14.0), cell(10.0, 14.0)),
        ],
        options(),
    )
    .unwrap_or_else(|error| panic!("table grid must layout: {error}"));

    close(layout.width_pt, 200.0);
    assert!(layout.width_pt <= options().available_width_pt);
    close(layout.columns[0].0, 5.0);
    close(layout.columns[1].0, 104.0);
    close(layout.columns[0].1, 91.0);

    let first = &layout.rows[0].cells[0];
    close(first.content_x_pt, 5.0);
    close(first.content_y_pt, 5.0);
    close(first.content_width_pt, 91.0);
    close(first.content_height_pt, 20.0);
    close(first.first_baseline_y_pt, 19.0);
    close(layout.rows[0].top_pt, 1.0);
    // 行高取行内最高单元格内容 + 上下内边距：max(20, 30) + 8。
    close(layout.rows[0].height_pt, 38.0);
    close(layout.rows[1].top_pt, 39.0);
    close(layout.rows[1].height_pt, 20.0);
    close(layout.height_pt, 1.0 + 38.0 + 20.0 + 1.0);
}

#[test]
fn every_column_has_content_insets_and_rules_bisect_the_padding_gap() {
    let mut options = options();
    options.column_count = 3;
    let layout = layout_table_grid(
        &[TableRowMeasurement {
            cells: vec![cell(10.0, 8.0), cell(10.0, 8.0), cell(10.0, 8.0)],
        }],
        options,
    )
    .unwrap_or_else(|error| panic!("table grid must layout: {error}"));
    let row = &layout.rows[0];
    for (index, cell) in row.cells.iter().enumerate() {
        let left_rule = layout.vertical_rule_positions[index];
        let right_rule = layout.vertical_rule_positions[index + 1];
        let expected_left_inset = options.cell_padding_pt
            + if index == 0 {
                options.border_width_pt
            } else {
                0.0
            };
        let expected_right_inset = options.cell_padding_pt
            + if index + 1 == options.column_count {
                options.border_width_pt
            } else {
                0.0
            };
        close(cell.content_x_pt - left_rule, expected_left_inset);
        close(
            right_rule - (cell.content_x_pt + cell.content_width_pt),
            expected_right_inset,
        );
    }
    assert_eq!(
        layout.vertical_rule_positions.len(),
        options.column_count + 1
    );
    for (index, (x, width)) in layout.columns.iter().enumerate() {
        close(*x, row.cells[index].content_x_pt);
        close(*width, row.cells[index].content_width_pt);
    }
}

#[test]
fn minimum_column_width_widens_the_table_and_reports_geometry() {
    let mut options = options();
    options.available_width_pt = 30.0;
    options.column_count = 3;
    options.minimum_column_width_pt = 20.0;
    options.column_gap_pt = 2.0;
    let layout = layout_table_grid(
        &[TableRowMeasurement {
            cells: vec![cell(10.0, 8.0), cell(10.0, 8.0), cell(10.0, 8.0)],
        }],
        options,
    )
    .unwrap_or_else(|error| panic!("table grid must layout: {error}"));
    close(layout.columns[0].1, 20.0);
    close(layout.width_pt, 2.0 * 1.0 + 2.0 * 4.0 * 3.0 + 60.0 + 4.0);
}

#[test]
fn empty_cell_lines_keep_minimum_row_metrics() {
    let mut options = options();
    options.minimum_row_ascent_pt = 10.0;
    options.minimum_row_descent_pt = 4.0;
    let layout = layout_table_grid(&[row(cell(0.0, 0.0), cell(0.0, 0.0))], options)
        .unwrap_or_else(|error| panic!("table grid must layout: {error}"));
    scale_row(&layout, 14.0);
}

fn scale_row(layout: &super::TableGridLayout, expected: f64) {
    close(layout.rows[0].height_pt, expected);
}

#[test]
fn invalid_options_and_measurements_are_rejected() {
    let mut bad = options();
    bad.column_count = 0;
    assert_eq!(
        layout_table_grid(&[], bad).err(),
        Some(TableLayoutError::InvalidOptions)
    );

    let bad_measurement = TableRowMeasurement {
        cells: vec![cell(f64::NAN, 0.0), cell(0.0, 0.0)],
    };
    assert_eq!(
        layout_table_grid(&[bad_measurement], options()).err(),
        Some(TableLayoutError::InvalidMeasurement)
    );

    let wrong_columns = TableRowMeasurement {
        cells: vec![cell(1.0, 0.0)],
    };
    assert_eq!(
        layout_table_grid(&[wrong_columns], options()).err(),
        Some(TableLayoutError::InvalidMeasurement)
    );

    let negative_baseline = row(cell(10.0, -1.0), cell(10.0, 0.0));
    assert_eq!(
        layout_table_grid(&[negative_baseline], options()).err(),
        Some(TableLayoutError::InvalidMeasurement)
    );
}

#[test]
fn row_heights_follow_maximum_cell_content_height() {
    let layout = layout_table_grid(&[row(cell(5.0, 5.0), cell(25.0, 15.0))], options())
        .unwrap_or_else(|error| panic!("table grid must layout: {error}"));
    close(layout.rows[0].height_pt, 25.0 + 8.0);
    close(
        layout.rows[0].cells[1].first_baseline_y_pt,
        1.0 + 4.0 + 15.0,
    );
}
