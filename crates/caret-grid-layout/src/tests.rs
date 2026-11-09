//! 验证共享网格布局 API 的几何结果、输入边界与失败行为。

use super::*;

fn cell(width: f64, ascent: f64, descent: f64) -> CellMetrics {
    CellMetrics {
        width,
        ascent,
        descent,
    }
}

fn spec() -> GridSpec {
    GridSpec {
        column_widths: vec![],
        column_alignments: vec![HorizontalAlignment::Left, HorizontalAlignment::Right],
        vertical_rules: vec![],
        horizontal_rules: vec![],
        column_gap: 2.0,
        row_gap: 4.0,
        minimum_row_ascent: 0.0,
        minimum_row_descent: 0.0,
        row_spacing: None,
        base_row_ascent: 0.0,
        base_row_descent: 0.0,
        base_row_leading: 0.0,
    }
}

#[test]
fn aligns_measured_cells_and_places_rules() {
    let rows = [GridRow::Cells(vec![
        cell(2.0, 5.0, 1.0),
        cell(3.0, 4.0, 2.0),
    ])];
    let mut spec = spec();
    spec.vertical_rules = vec![1];
    spec.horizontal_rules = vec![0, 1];
    let grid = match layout_grid(&rows, &spec) {
        Ok(grid) => grid,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(grid.columns, vec![(0.0, 2.0), (4.0, 3.0)]);
    assert_eq!(
        grid.rows[0].cells,
        vec![
            CellPosition {
                x: 0.0,
                baseline: 9.0
            },
            CellPosition {
                x: 4.0,
                baseline: 9.0
            }
        ]
    );
    assert_eq!(grid.vertical_rule_positions, vec![3.0]);
    assert_eq!(grid.horizontal_rule_positions, vec![2.0, 13.0]);
    assert_eq!(grid.height, 15.0);
}

#[test]
fn accepts_resolved_widths_and_full_rows() {
    let rows = [
        GridRow::Cells(vec![cell(3.0, 2.0, 1.0)]),
        GridRow::Full(cell(12.0, 4.0, 2.0)),
    ];
    let mut spec = spec();
    spec.column_alignments = vec![HorizontalAlignment::Left];
    spec.column_widths = vec![Some(8.0)];
    let grid = match layout_grid(&rows, &spec) {
        Ok(grid) => grid,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(grid.columns, vec![(0.0, 8.0)]);
    assert_eq!(grid.width, 8.0);
    assert!(grid.content_overflow);
    assert_eq!(grid.rows[0].cells[0].x, 0.0);
    assert_eq!(
        grid.rows[1].full,
        Some(CellPosition {
            x: 0.0,
            baseline: 11.0
        })
    );
}

#[test]
fn full_row_expansion_moves_only_the_right_edge_vertical_rule() {
    let rows = [
        GridRow::Cells(vec![cell(3.0, 2.0, 1.0)]),
        GridRow::Full(cell(12.0, 4.0, 2.0)),
    ];
    let mut spec = spec();
    spec.vertical_rules = vec![0, 1];

    let grid = match layout_grid(&rows, &spec) {
        Ok(grid) => grid,
        Err(error) => panic!("{error}"),
    };

    assert_eq!(grid.width, 12.0);
    assert_eq!(grid.vertical_rule_positions, vec![1.0, 11.0]);
}

#[test]
fn applies_resolved_column_width_and_right_alignment() {
    let rows = [GridRow::Cells(vec![cell(3.0, 2.0, 1.0)])];
    let mut spec = spec();
    spec.column_widths = vec![Some(8.0)];
    spec.column_alignments = vec![HorizontalAlignment::Right];
    let grid = match layout_grid(&rows, &spec) {
        Ok(grid) => grid,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(grid.columns, vec![(0.0, 8.0)]);
    assert_eq!(grid.rows[0].cells[0].x, 5.0);
}

#[test]
fn keeps_resolved_track_width_and_reports_cell_and_full_row_overflow() {
    let rows = [
        GridRow::Cells(vec![cell(60.0, 2.0, 1.0)]),
        GridRow::Full(cell(70.0, 2.0, 1.0)),
    ];
    let mut spec = spec();
    spec.column_widths = vec![Some(40.0)];
    let grid = match layout_grid(&rows, &spec) {
        Ok(grid) => grid,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(grid.width, 40.0);
    assert!(grid.content_overflow);
}

#[test]
fn accepts_negative_cell_advance() {
    let rows = [GridRow::Cells(vec![
        cell(-2.0, 1.0, 0.0),
        cell(3.0, 1.0, 0.0),
    ])];
    let grid = match layout_grid(&rows, &spec()) {
        Ok(grid) => grid,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(grid.columns, vec![(0.0, 0.0), (2.0, 3.0)]);
    assert_eq!(grid.rows[0].cells.len(), 2);
}

#[test]
fn accepts_signed_baseline_metrics() {
    let rows = [GridRow::Cells(vec![
        cell(1.0, 2.0, -1.0),
        cell(2.0, -1.0, 1.0),
    ])];
    let grid = match layout_grid(&rows, &spec()) {
        Ok(grid) => grid,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(grid.rows[0].ascent, 2.0);
    assert_eq!(grid.rows[0].descent, 1.0);
    assert!(grid.rows[0].cells[1].baseline.is_finite());
}

#[test]
fn rejects_arithmetic_overflow_in_output_geometry() {
    let rows = [GridRow::Cells(vec![
        cell(f64::MAX, 0.0, 0.0),
        cell(f64::MAX, 0.0, 0.0),
    ])];
    assert_eq!(
        layout_grid(&rows, &spec()),
        Err(GridLayoutError::NonFiniteResult)
    );
    let rows = [GridRow::Cells(vec![cell(1.0, f64::MAX, f64::MAX)])];
    assert_eq!(
        layout_grid(&rows, &spec()),
        Err(GridLayoutError::NonFiniteResult)
    );
}

#[test]
fn exactly_row_frames_keep_fixed_advances_and_report_content_overflow() {
    let frame = calculate_row_frame(
        8.0,
        3.0,
        1.0,
        18.0,
        4.0,
        RowSpacing::Exactly { height: 12.0 },
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(frame.height, 12.0);
    assert_eq!(frame.baseline, 9.0);
    assert!(frame.content_overflow);
    assert!(frame.clip_content);

    let spec = GridSpec {
        row_spacing: Some(RowSpacing::Exactly { height: 12.0 }),
        base_row_ascent: 8.0,
        base_row_descent: 3.0,
        base_row_leading: 1.0,
        ..spec()
    };
    let grid = layout_grid(
        &[
            GridRow::Cells(vec![CellMetrics {
                width: 5.0,
                ascent: 18.0,
                descent: 4.0,
            }]),
            GridRow::Cells(vec![CellMetrics {
                width: 5.0,
                ascent: 8.0,
                descent: 3.0,
            }]),
        ],
        &spec,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(grid.rows[0].baseline, 9.0);
    assert!(grid.rows[0].content_overflow);
    assert!(grid.rows[0].clip_content);
    assert_eq!(grid.rows[1].top, 16.0);
    assert_eq!(grid.height, 28.0);
}

#[test]
fn row_frame_accepts_signed_math_extents_and_rejects_overflow() {
    let frame = calculate_row_frame(8.0, 3.0, 0.0, -2.0, 1.0, RowSpacing::Natural)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(frame.height, 11.0);

    assert_eq!(
        calculate_row_frame(f64::MAX, f64::MAX, f64::MAX, 0.0, 0.0, RowSpacing::Natural,),
        Err(GridLayoutError::NonFiniteResult)
    );
}

#[test]
fn row_frame_centers_content_ink_without_pushing_leading_below_the_row() {
    let frame = calculate_row_frame_with_content_leading(
        RowMetrics {
            ascent: 8.0,
            descent: 2.0,
            leading: 1.0,
        },
        RowMetrics {
            ascent: 16.0,
            descent: 4.0,
            leading: 2.0,
        },
        false,
        RowSpacing::Natural,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(frame.height, 22.0);
    assert_eq!(frame.baseline, 17.0);
}

#[test]
fn expand_to_fit_accepts_body_baseline_beyond_short_exact_line_height() {
    let frame = calculate_row_frame(
        4.0,
        1.0,
        0.0,
        8.0,
        2.0,
        RowSpacing::ExpandToFit {
            base_height: 5.0,
            base_baseline: 8.0,
        },
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(frame.baseline, 8.0);
    assert!(frame.height >= frame.baseline + 2.0);
}

#[test]
fn rejects_invalid_metrics_and_rules() {
    let mut spec = spec();
    assert_eq!(
        layout_grid(&[GridRow::Cells(vec![cell(f64::NAN, 0.0, 0.0)])], &spec),
        Err(GridLayoutError::InvalidMetric)
    );
    spec.vertical_rules = vec![3];
    assert_eq!(
        layout_grid(&[GridRow::Cells(vec![cell(1.0, 1.0, 1.0)])], &spec),
        Err(GridLayoutError::RuleOutOfBounds)
    );
}

#[test]
fn lays_out_empty_rows_with_a_single_shared_horizontal_boundary() {
    let mut spec = spec();
    spec.column_widths = vec![Some(5.0), None, Some(3.0)];
    let grid = match layout_grid(&[], &spec) {
        Ok(grid) => grid,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(grid.width, 12.0);
    assert_eq!(grid.height, 0.0);
    assert_eq!(grid.columns, vec![(0.0, 5.0), (7.0, 0.0), (9.0, 3.0)]);
    assert!(grid.horizontal_rule_positions.is_empty());

    spec.horizontal_rules = vec![0];
    let grid = match layout_grid(&[], &spec) {
        Ok(grid) => grid,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(grid.horizontal_rule_positions, vec![2.0]);
    assert_eq!(grid.height, 4.0);

    spec.horizontal_rules = vec![1];
    assert_eq!(
        layout_grid(&[], &spec),
        Err(GridLayoutError::RuleOutOfBounds)
    );
}
