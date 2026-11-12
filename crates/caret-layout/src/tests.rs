//! 覆盖 Parley 高层布局、Caret Unicode tailoring、字体偏好及跨 Run 行为。

use std::collections::BTreeMap;

use caret_doc::{
    DocumentSettings, InlineEnvironment, InlineEnvironmentKind, NodeId, Paragraph,
    ParagraphContent, Run, TextFlow,
};
use parley_engine::{Analysis, AnalysisOptions, Analyzer, BaseDirection};

use crate::shaping::{StyledSpan, boundaries_for_spans};
use crate::{
    FontCatalog, FontLineMetrics, GlyphRasterOptions, GlyphRasterizer, InlineObjectLayout,
    LineSpacing, ParagraphLayoutOptions, ParagraphLayouter, ResolvedDocumentTextSettings,
    ShapingStyle, TextIslandError, TextIslandSpan, UNICODE_DATA_VERSION, UnicodeBoundaries,
    VisualAffinity, has_empty_line, paragraph_has_empty_line,
};

fn paragraph(text: &str, language: Option<&str>) -> Paragraph {
    Paragraph {
        id: NodeId::generate(),
        language: language.map(str::to_owned),
        layout: None,
        inlines: vec![text_flow(Run {
            text: text.to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }
}

fn font_catalog() -> FontCatalog {
    FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"))
}

#[test]
fn font_catalog_generations_are_unique_across_instances() {
    let first = font_catalog();
    let second = font_catalog();
    assert_ne!(first.generation(), second.generation());
}

#[test]
fn font_catalog_includes_the_embedded_code_font() {
    let catalog = font_catalog();
    assert!(
        catalog.families().iter().any(|family| family
            .name
            .eq_ignore_ascii_case(caret_font_roles::DEFAULT_CODE_FONT_FAMILY)),
        "embedded code font is missing from the selectable catalog"
    );
}

#[test]
fn font_catalog_includes_the_embedded_math_font() {
    let catalog = FontCatalog::system()
        .unwrap_or_else(|error| panic!("system font catalog unavailable: {error}"));
    assert!(catalog.families().iter().any(|family| {
        family
            .name
            .eq_ignore_ascii_case(caret_font_roles::DEFAULT_MATH_FONT_FAMILY)
    }));
}

#[test]
fn inline_object_uses_parley_advance_and_expands_the_shared_line_box() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let base_style = ShapingStyle::new(1200);
    let text = paragraph("a\u{fffc}b", Some("en"));
    let mut layouter = ParagraphLayouter::new();
    let (_, zero_width_object) = layouter
        .layout_with_inline_objects(
            &mut catalog,
            &text,
            &settings,
            &base_style,
            layout_options(200.0),
            &[InlineObjectLayout {
                id: 41,
                source_bytes: 1..4,
                advance_pt: 0.0,
                ascent_pt: 0.0,
                descent_pt: 0.0,
            }],
        )
        .unwrap_or_else(|error| panic!("zero-width inline object should lay out: {error}"));
    let (_, with_object) = layouter
        .layout_with_inline_objects(
            &mut catalog,
            &text,
            &settings,
            &base_style,
            layout_options(200.0),
            &[InlineObjectLayout {
                id: 42,
                source_bytes: 1..4,
                advance_pt: 30.0,
                ascent_pt: 20.0,
                descent_pt: 5.0,
            }],
        )
        .unwrap_or_else(|error| panic!("inline object paragraph should lay out: {error}"));

    let line = match with_object.layout.lines().next() {
        Some(line) => line,
        None => panic!("paragraph layout should have a line"),
    };
    assert!(line.items().any(|item| matches!(item, parley::PositionedLayoutItem::InlineBox(box_item) if box_item.id == 42)));
    let object_advance =
        with_object.paragraph.lines[0].advance_pt - zero_width_object.paragraph.lines[0].advance_pt;
    assert!(
        (object_advance - 30.0).abs() < 0.5,
        "the inline object contributes its 30 pt advance, got {object_advance} pt"
    );
    assert!(
        with_object.paragraph.lines[0].metrics.height_pt
            > zero_width_object.paragraph.lines[0].metrics.height_pt
    );

    let object = line
        .items()
        .find_map(|item| match item {
            parley::PositionedLayoutItem::InlineBox(box_item) => Some(box_item),
            parley::PositionedLayoutItem::GlyphRun(_) => None,
        })
        .unwrap_or_else(|| panic!("inline object box should be positioned"));
    let composition_selection = with_object
        .selection_text_geometry("a\u{fffc}b", 0..5)
        .unwrap_or_else(|| panic!("composition selection should include the inline object"));
    assert!(composition_selection.iter().any(|(bounds, _)| {
        bounds.x0 <= f64::from(object.x) && bounds.x1 >= f64::from(object.x + object.width)
    }));
    assert!(with_object.selection_text_geometry("a", 0..5).is_none());
    assert!(with_object.selection_text_geometry("é", 1..2).is_none());
    let left_hit = with_object
        .cursor_from_point_on_line(object.x + object.width * 0.25, 0)
        .unwrap_or_else(|| panic!("left half of inline object should be hit-testable"));
    let right_hit = with_object
        .cursor_from_point_on_line(object.x + object.width * 0.75, 0)
        .unwrap_or_else(|| panic!("right half of inline object should be hit-testable"));
    assert_eq!(left_hit.cursor.byte_index(), 1);
    assert_eq!(left_hit.logical_affinity, crate::HitAffinity::Downstream);
    assert_eq!(right_hit.cursor.byte_index(), 4);
    assert_eq!(right_hit.logical_affinity, crate::HitAffinity::Upstream);
    let left_cursor = with_object.cursor_geometry(left_hit.cursor, 0.0);
    let right_cursor = with_object.cursor_geometry(right_hit.cursor, 0.0);
    assert!((left_cursor.x0 - f64::from(object.x)).abs() < 0.01);
    assert!((right_cursor.x0 - f64::from(object.x + object.width)).abs() < 0.01);

    let selection = with_object
        .selection_geometry(1..4)
        .unwrap_or_else(|| panic!("formula source range should be selectable"));
    let formula_selection = selection
        .iter()
        .find(|(bounds, _)| {
            (bounds.x0 - f64::from(object.x)).abs() < 0.01
                && (bounds.x1 - f64::from(object.x + object.width)).abs() < 0.01
        })
        .unwrap_or_else(|| panic!("formula selection should cover its inline box"));
    assert!(formula_selection.0.y1 > formula_selection.0.y0);
    assert_eq!(formula_selection.1, 0);
    assert!((formula_selection.0.y0 - f64::from(with_object.line_box_tops_pt[0])).abs() < 0.01);
    assert!(
        (formula_selection.0.y1
            - formula_selection.0.y0
            - f64::from(with_object.paragraph.lines[0].metrics.height_pt))
        .abs()
            < 0.01
    );

    let whole_selection = with_object
        .selection_geometry(0..5)
        .unwrap_or_else(|| panic!("text and formula selection should be available"));
    let covering_rectangles = whole_selection
        .iter()
        .filter(|(bounds, _)| {
            bounds.x0 <= f64::from(object.x) && bounds.x1 >= f64::from(object.x + object.width)
        })
        .count();
    assert_eq!(
        covering_rectangles, 1,
        "formula selection must not be duplicated"
    );

    let partial_formula_selection = with_object
        .selection_geometry(2..3)
        .unwrap_or_else(|| panic!("partial UTF-8 formula range should be selectable"));
    let formula_rectangles = partial_formula_selection
        .iter()
        .filter(|(bounds, _)| {
            (bounds.x0 - f64::from(object.x)).abs() < 0.01
                && (bounds.x1 - f64::from(object.x + object.width)).abs() < 0.01
        })
        .count();
    assert_eq!(
        formula_rectangles, 1,
        "partial U+FFFC range selects its formula box"
    );
}

/// 构造带行内对象的段落布局，供对象边界选区几何测试使用。
fn paragraph_layout_with_inline_objects(
    text_value: &str,
    language: Option<&str>,
    objects: &[InlineObjectLayout],
) -> crate::ParleyParagraphLayout {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let base_style = ShapingStyle::new(1200);
    let text = paragraph(text_value, language);
    let mut layouter = ParagraphLayouter::new();
    layouter
        .layout_with_inline_objects(
            &mut catalog,
            &text,
            &settings,
            &base_style,
            layout_options(200.0),
            objects,
        )
        .unwrap_or_else(|error| panic!("paragraph with inline objects should layout: {error}"))
        .1
}

/// 返回行内对象在行内的版面盒区间 [x0, x1]。
fn inline_object_box(layout: &crate::ParleyParagraphLayout, id: u64) -> (f64, f64) {
    for line in layout.layout.lines() {
        for item in line.items() {
            if let parley::PositionedLayoutItem::InlineBox(box_item) = item
                && box_item.id == id
            {
                let x0 = f64::from(box_item.x);
                return (x0, x0 + f64::from(box_item.width));
            }
        }
    }
    panic!("inline object {id} should be positioned");
}

fn inline_object_layout() -> InlineObjectLayout {
    InlineObjectLayout {
        id: 42,
        source_bytes: 1..4,
        advance_pt: 30.0,
        ascent_pt: 20.0,
        descent_pt: 5.0,
    }
}

fn second_inline_object_layout() -> InlineObjectLayout {
    InlineObjectLayout {
        id: 43,
        source_bytes: 4..7,
        advance_pt: 30.0,
        ascent_pt: 20.0,
        descent_pt: 5.0,
    }
}

#[test]
fn selection_starting_after_inline_object_excludes_its_box() {
    let layout =
        paragraph_layout_with_inline_objects("a\u{fffc}b", Some("en"), &[inline_object_layout()]);
    let (object_x0, object_x1) = inline_object_box(&layout, 42);

    // 选区起点紧接对象之后：矩形从对象右缘开始，而不是回溯到对象左缘。
    let after = layout
        .selection_text_geometry("a\u{fffc}b", 4..5)
        .unwrap_or_else(|| panic!("text after the inline object should be selectable"));
    assert_eq!(after.len(), 1);
    assert!(
        (after[0].0.x0 - object_x1).abs() < 0.01,
        "selection after the object must start at its right edge: {} vs {object_x1}",
        after[0].0.x0
    );

    // 对象自身被完整选中时保留整框。
    let covering = layout
        .selection_text_geometry("a\u{fffc}b", 1..4)
        .unwrap_or_else(|| panic!("inline object source range should be selectable"));
    assert!(covering.iter().any(|(bounds, _)| {
        (bounds.x0 - object_x0).abs() < 0.01 && (bounds.x1 - object_x1).abs() < 0.01
    }));

    // 选区在对象之前时不会扩展进对象框。
    let before = layout
        .selection_text_geometry("a\u{fffc}b", 0..1)
        .unwrap_or_else(|| panic!("text before the inline object should be selectable"));
    assert!(
        before
            .iter()
            .all(|(bounds, _)| bounds.x1 <= object_x0 + 0.01)
    );
}

#[test]
fn selection_after_inline_object_in_mixed_bidi_run_excludes_its_box() {
    let text = "abc\u{fffc}אב";
    let layout = paragraph_layout_with_inline_objects(
        text,
        Some("he"),
        &[InlineObjectLayout {
            id: 42,
            source_bytes: 3..6,
            advance_pt: 30.0,
            ascent_pt: 20.0,
            descent_pt: 5.0,
        }],
    );
    let (object_x0, object_x1) = inline_object_box(&layout, 42);
    // 对象之后的 RTL 文本在视觉上位于对象右侧；选区起点紧随对象字节之后。
    let after = layout
        .selection_text_geometry(text, 6..10)
        .unwrap_or_else(|| panic!("RTL text after the inline object should be selectable"));
    assert_eq!(after.len(), 1);
    let (bounds, _) = after[0];
    assert!(
        bounds.x1 <= object_x0 + 0.01 || bounds.x0 >= object_x1 - 0.01,
        "bidirectional selection must not cover the unselected inline object: {bounds:?}"
    );
    assert!(
        (bounds.x0 - object_x1).abs() < 0.01,
        "selection after the object must start at the object right edge: {} vs {object_x1}",
        bounds.x0
    );
}

#[test]
fn selection_skips_adjacent_unselected_inline_objects() {
    let layout = paragraph_layout_with_inline_objects(
        "a\u{fffc}\u{fffc}b",
        Some("en"),
        &[inline_object_layout(), second_inline_object_layout()],
    );
    let (first_x0, first_x1) = inline_object_box(&layout, 42);
    let (second_x0, second_x1) = inline_object_box(&layout, 43);

    // 选区起点在第二个对象之后：矩形只覆盖其后文本，不回溯到任一对象框。
    let after = layout
        .selection_text_geometry("a\u{fffc}\u{fffc}b", 7..8)
        .unwrap_or_else(|| panic!("text after adjacent inline objects should be selectable"));
    assert_eq!(after.len(), 1);
    assert!((after[0].0.x0 - second_x1).abs() < 0.01);
    assert!(after[0].0.x1 <= 200.0);

    // 选区覆盖第二个对象但不覆盖第一个：保留第二个整框，第一个框保持未选中。
    let covering = layout
        .selection_text_geometry("a\u{fffc}\u{fffc}b", 4..8)
        .unwrap_or_else(|| panic!("second inline object text should be selectable"));
    assert!(
        covering
            .iter()
            .all(|(bounds, _)| { bounds.x1 <= first_x0 + 0.01 || bounds.x0 >= first_x1 - 0.01 })
    );
    assert!(
        covering
            .iter()
            .any(|(bounds, _)| { bounds.x0 <= second_x0 + 0.01 && bounds.x1 >= second_x1 - 0.01 })
    );
}

fn selection_union(rectangles: &[(parley::BoundingBox, usize)]) -> (f64, f64) {
    let mut union = (f64::INFINITY, f64::NEG_INFINITY);
    for (bounds, _) in rectangles {
        union.0 = union.0.min(bounds.x0);
        union.1 = union.1.max(bounds.x1);
    }
    union
}

fn assert_selection_rectangles_are_disjoint(
    rectangles: &[(parley::BoundingBox, usize)],
    label: &str,
) {
    for (index, (left, left_line)) in rectangles.iter().enumerate() {
        assert!(
            left.x1 > left.x0,
            "{label}: rect {index} must have positive width"
        );
        for (right, right_line) in rectangles.iter().skip(index + 1) {
            if left_line != right_line {
                continue;
            }
            let overlap = left.x1.min(right.x1) - left.x0.max(right.x0);
            assert!(
                overlap <= 0.01,
                "{label}: selection rectangles must not overlap: {left:?} vs {right:?}"
            );
        }
    }
}

/// 用零推进对象的同文布局作为纯文本基准，按对象结束位置补偿对象宽度。
fn expected_ltr_selection_span(
    zero_layout: &crate::ParleyParagraphLayout,
    text: &str,
    range: std::ops::Range<usize>,
    objects: &[(std::ops::Range<usize>, f64)],
) -> Option<(f64, f64)> {
    let zero_rectangles = zero_layout
        .selection_text_geometry(text, range.clone())
        .unwrap_or_else(|| panic!("zero-advance layout should select {range:?}"));
    if zero_rectangles.is_empty() {
        // 选区只覆盖零宽对象本身：纯文本基准没有矩形，由调用方用对象盒断言。
        return None;
    }
    let (zero_x0, zero_x1) = selection_union(&zero_rectangles);
    let start_shift: f64 = objects
        .iter()
        .filter(|(object, _)| range.start >= object.end)
        .map(|(_, width)| width)
        .sum();
    let end_shift: f64 = objects
        .iter()
        .filter(|(object, _)| range.end >= object.end)
        .map(|(_, width)| width)
        .sum();
    Some((zero_x0 + start_shift, zero_x1 + end_shift))
}

fn assert_object_interval_selected_once(
    rectangles: &[(parley::BoundingBox, usize)],
    interval: (f64, f64),
    line_index: usize,
    selected: bool,
    label: &str,
) {
    let (object_x0, object_x1) = interval;
    let mut overlap_total = 0.0;
    let mut containing_rectangles = 0;
    for (bounds, rectangle_line) in rectangles {
        if *rectangle_line != line_index {
            continue;
        }
        let overlap = bounds.x1.min(object_x1) - bounds.x0.max(object_x0);
        if overlap <= 0.01 {
            continue;
        }
        overlap_total += overlap;
        let contains = bounds.x0 <= object_x0 + 0.01 && bounds.x1 >= object_x1 - 0.01;
        assert!(
            contains,
            "{label}: a rectangle may only touch the object interval completely: {bounds:?}"
        );
        containing_rectangles += 1;
    }
    let expected = if selected { object_x1 - object_x0 } else { 0.0 };
    assert!(
        (overlap_total - expected).abs() < 0.01,
        "{label}: object interval overlap {overlap_total} must equal {expected}"
    );
    assert!(
        containing_rectangles <= 1,
        "{label}: the object interval must appear at most once"
    );
}

#[test]
fn inline_object_selection_rectangles_are_stable_on_both_object_sides() {
    let text = "ab\u{fffc}cd";
    let object = InlineObjectLayout {
        id: 42,
        source_bytes: 2..5,
        advance_pt: 30.0,
        ascent_pt: 20.0,
        descent_pt: 5.0,
    };
    let layout =
        paragraph_layout_with_inline_objects(text, Some("en"), std::slice::from_ref(&object));
    let zero_object = InlineObjectLayout {
        advance_pt: 0.0,
        ..object.clone()
    };
    let zero_layout =
        paragraph_layout_with_inline_objects(text, Some("en"), std::slice::from_ref(&zero_object));
    let (object_x0, object_x1) = inline_object_box(&layout, 42);
    let object_width = object_x1 - object_x0;
    let objects = [(object.source_bytes.clone(), object_width)];

    for range in [0..2usize, 0..5, 2..5, 2..6, 0..7, 5..6] {
        let label = format!("range {range:?}");
        let rectangles = layout
            .selection_text_geometry(text, range.clone())
            .unwrap_or_else(|| panic!("{label}: selection geometry missing"));
        assert_selection_rectangles_are_disjoint(&rectangles, &label);
        let (union_x0, union_x1) = selection_union(&rectangles);
        if let Some(expected) =
            expected_ltr_selection_span(&zero_layout, text, range.clone(), &objects)
        {
            assert!(
                (union_x0 - expected.0).abs() < 0.01 && (union_x1 - expected.1).abs() < 0.01,
                "{label}: selection union ({union_x0}, {union_x1}) must stay within {expected:?}"
            );
        } else {
            let selected =
                range.start <= object.source_bytes.start && object.source_bytes.end <= range.end;
            assert!(
                selected
                    && (union_x0 - object_x0).abs() < 0.01
                    && (union_x1 - object_x1).abs() < 0.01,
                "{label}: object-only selection must keep the object box span"
            );
        }
        let selected =
            range.start <= object.source_bytes.start && object.source_bytes.end <= range.end;
        assert_object_interval_selected_once(
            &rectangles,
            (object_x0, object_x1),
            0,
            selected,
            &label,
        );
    }
}

#[test]
fn adjacent_inline_objects_do_not_duplicate_selection_rectangles() {
    let text = "a\u{fffc}\u{fffc}b";
    let first = InlineObjectLayout {
        id: 42,
        source_bytes: 1..4,
        advance_pt: 30.0,
        ascent_pt: 20.0,
        descent_pt: 5.0,
    };
    let second = InlineObjectLayout {
        id: 43,
        source_bytes: 4..7,
        advance_pt: 30.0,
        ascent_pt: 20.0,
        descent_pt: 5.0,
    };
    let layout =
        paragraph_layout_with_inline_objects(text, Some("en"), &[first.clone(), second.clone()]);
    let zero_layout = paragraph_layout_with_inline_objects(
        text,
        Some("en"),
        &[
            InlineObjectLayout {
                advance_pt: 0.0,
                ..first.clone()
            },
            InlineObjectLayout {
                advance_pt: 0.0,
                ..second.clone()
            },
        ],
    );
    let first_box = inline_object_box(&layout, 42);
    let second_box = inline_object_box(&layout, 43);
    let objects = [
        (first.source_bytes.clone(), first_box.1 - first_box.0),
        (second.source_bytes.clone(), second_box.1 - second_box.0),
    ];

    for range in [0..8usize, 0..1, 0..4, 1..4, 4..7, 1..7, 4..8, 7..8] {
        let label = format!("range {range:?}");
        let rectangles = layout
            .selection_text_geometry(text, range.clone())
            .unwrap_or_else(|| panic!("{label}: selection geometry missing"));
        assert_selection_rectangles_are_disjoint(&rectangles, &label);
        let (union_x0, union_x1) = selection_union(&rectangles);
        if let Some(expected) =
            expected_ltr_selection_span(&zero_layout, text, range.clone(), &objects)
        {
            assert!(
                (union_x0 - expected.0).abs() < 0.01 && (union_x1 - expected.1).abs() < 0.01,
                "{label}: selection union ({union_x0}, {union_x1}) must stay within {expected:?}"
            );
        } else {
            let selected_boxes = [
                (first_box, first.source_bytes.clone()),
                (second_box, second.source_bytes.clone()),
            ]
            .into_iter()
            .filter(|(_, object)| range.start <= object.start && object.end <= range.end)
            .map(|(bounds, _)| bounds)
            .collect::<Vec<_>>();
            assert!(!selected_boxes.is_empty());
            let expected_x0 = selected_boxes
                .iter()
                .map(|bounds| bounds.0)
                .fold(f64::INFINITY, f64::min);
            let expected_x1 = selected_boxes
                .iter()
                .map(|bounds| bounds.1)
                .fold(f64::NEG_INFINITY, f64::max);
            assert!(
                (union_x0 - expected_x0).abs() < 0.01 && (union_x1 - expected_x1).abs() < 0.01,
                "{label}: object-only selection must keep the selected object boxes"
            );
        }
        assert_object_interval_selected_once(
            &rectangles,
            first_box,
            0,
            range.start <= first.source_bytes.start && first.source_bytes.end <= range.end,
            &label,
        );
        assert_object_interval_selected_once(
            &rectangles,
            second_box,
            0,
            range.start <= second.source_bytes.start && second.source_bytes.end <= range.end,
            &label,
        );
    }
}

#[test]
fn mixed_bidi_inline_object_selection_rectangles_do_not_overlap() {
    let text = "abc\u{fffc}אב";
    let object = InlineObjectLayout {
        id: 42,
        source_bytes: 3..6,
        advance_pt: 30.0,
        ascent_pt: 20.0,
        descent_pt: 5.0,
    };
    let layout =
        paragraph_layout_with_inline_objects(text, Some("he"), std::slice::from_ref(&object));
    let (object_x0, object_x1) = inline_object_box(&layout, 42);
    let object_line = 0;
    let line_advance = layout.paragraph.lines[0].advance_pt;
    for range in [0..3usize, 3..6, 6..10, 0..10] {
        let label = format!("range {range:?}");
        let rectangles = layout
            .selection_text_geometry(text, range.clone())
            .unwrap_or_else(|| panic!("{label}: bidirectional selection geometry missing"));
        assert_selection_rectangles_are_disjoint(&rectangles, &label);
        for (bounds, _) in &rectangles {
            assert!(bounds.x0 >= -0.01 && bounds.x1 <= f64::from(line_advance) + 0.01);
        }
        assert_object_interval_selected_once(
            &rectangles,
            (object_x0, object_x1),
            object_line,
            range.start <= object.source_bytes.start && object.source_bytes.end <= range.end,
            &label,
        );
    }
}

#[test]
fn zero_width_inline_object_adds_no_selection_rectangle() {
    let text = "a\u{fffc}b";
    let layout = paragraph_layout_with_inline_objects(
        text,
        Some("en"),
        &[InlineObjectLayout {
            id: 42,
            source_bytes: 1..4,
            advance_pt: 0.0,
            ascent_pt: 0.0,
            descent_pt: 0.0,
        }],
    );
    for range in [0..1usize, 0..5, 1..4, 4..5] {
        let label = format!("zero-width range {range:?}");
        let rectangles = layout
            .selection_text_geometry(text, range)
            .unwrap_or_else(|| panic!("{label}: selection geometry missing"));
        assert_selection_rectangles_are_disjoint(&rectangles, &label);
        assert!(
            rectangles.iter().all(|(bounds, _)| bounds.x1 > bounds.x0),
            "{label}: zero-width rerects must not be published"
        );
    }
}

/// 构造自动换行的多行段落，并要求“对象位于非末行、末行比对象盒右缘更短”。
/// 这是全局视觉 span 会错误裁剪对象行右侧文本的几何前置条件；不同平台字体宽度
/// 不同，因此按词数扫描出满足条件的稳定夹具。
fn wrapped_multiline_inline_object_fixture(
    language: Option<&str>,
    prefix_word: &str,
    suffix_word: &str,
) -> Option<(crate::ParleyParagraphLayout, String, (f64, f64))> {
    for prefix_words in [16usize, 18, 20, 22, 24, 26, 28, 30, 32] {
        for suffix_words in [1usize, 2, 3] {
            let prefix = prefix_word.repeat(prefix_words);
            let suffix = suffix_word.repeat(suffix_words);
            let text = format!("{prefix}\u{fffc}{suffix}");
            let start = prefix.len();
            let object = InlineObjectLayout {
                id: 42,
                source_bytes: start..start + '\u{fffc}'.len_utf8(),
                advance_pt: 30.0,
                ascent_pt: 20.0,
                descent_pt: 5.0,
            };
            let layout = paragraph_layout_with_inline_objects(
                &text,
                language,
                std::slice::from_ref(&object),
            );
            let (box_x0, box_x1) = inline_object_box(&layout, 42);
            let lines: Vec<_> = layout.layout.lines().collect();
            let Some(object_line) = lines.iter().position(|line| {
                line.items().any(|item| {
                    matches!(item, parley::PositionedLayoutItem::InlineBox(inline_box) if inline_box.id == 42)
                })
            }) else {
                continue;
            };
            let last_advance = f64::from(lines[lines.len() - 1].metrics().advance);
            if lines.len() >= 3 && object_line + 1 < lines.len() && last_advance < box_x1 {
                return Some((layout, text, (box_x0, box_x1)));
            }
        }
    }
    None
}

fn assert_full_multiline_selection_covers_every_line(
    layout: &crate::ParleyParagraphLayout,
    text: &str,
    object_span: (f64, f64),
    label: &str,
) {
    let rectangles = layout
        .selection_text_geometry(text, 0..text.len())
        .unwrap_or_else(|| panic!("{label}: full multiline selection geometry missing"));
    assert_selection_rectangles_are_disjoint(&rectangles, label);
    let line_count = layout.layout.lines().len();
    let mut spans_per_line = vec![Vec::new(); line_count];
    for (bounds, line_index) in &rectangles {
        spans_per_line
            .get_mut(*line_index)
            .unwrap_or_else(|| panic!("{label}: selection rect on unknown line {line_index}"))
            .push((bounds.x0, bounds.x1));
    }
    for (index, line) in layout.layout.lines().enumerate() {
        let metrics = line.metrics();
        let expected_x0 = f64::from(metrics.offset) + f64::from(metrics.inline_min_coord);
        let expected_x1 = expected_x0 + f64::from(metrics.advance);
        let spans = &spans_per_line[index];
        assert!(
            !spans.is_empty(),
            "{label}: full selection must highlight line {index}"
        );
        let union_x0 = spans
            .iter()
            .map(|(x0, _)| *x0)
            .fold(f64::INFINITY, f64::min);
        let union_x1 = spans
            .iter()
            .map(|(_, x1)| *x1)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (union_x0 - expected_x0).abs() < 0.05 && (union_x1 - expected_x1).abs() < 0.05,
            "{label}: line {index} highlight ({union_x0}, {union_x1}) must cover the selected \
             full line ({expected_x0}, {expected_x1})"
        );
    }
    let object_line = layout
        .layout
        .lines()
        .position(|line| {
            line.items().any(|item| {
                matches!(item, parley::PositionedLayoutItem::InlineBox(inline_box) if inline_box.id == 42)
            })
        })
        .unwrap_or_else(|| panic!("{label}: inline object must stay on a line"));
    assert_object_interval_selected_once(&rectangles, object_span, object_line, true, label);
}

#[test]
fn wrapped_multiline_selection_keeps_text_after_the_object_on_its_line() {
    let Some((layout, text, object_span)) =
        wrapped_multiline_inline_object_fixture(Some("en"), "word ", "word ")
    else {
        panic!(
            "fixture must wrap to >=3 lines with the object on a non-last line and a short last line"
        );
    };
    assert_full_multiline_selection_covers_every_line(
        &layout,
        &text,
        object_span,
        "wrapped multiline full selection",
    );

    // 明确 bug 前置条件与修复结果：对象行行尾仍有已选文本，且对象盒只出现一次。
    let lines: Vec<_> = layout.layout.lines().collect();
    let last_advance = f64::from(lines[lines.len() - 1].metrics().advance);
    assert!(
        last_advance < object_span.1,
        "fixture precondition: the short last line must end before the object's right edge"
    );
    let object_line = lines
        .iter()
        .position(|line| {
            line.items().any(|item| {
                matches!(item, parley::PositionedLayoutItem::InlineBox(inline_box) if inline_box.id == 42)
            })
        })
        .unwrap_or_else(|| panic!("object line must exist"));
    let object_line_metrics = lines[object_line].metrics();
    assert!(
        f64::from(object_line_metrics.advance) > object_span.1 + 1.0,
        "the object line must have selected text to the right of the object box"
    );
}

#[test]
fn wrapped_rtl_multiline_selection_keeps_text_after_the_object_on_its_line() {
    let Some((layout, text, object_span)) =
        wrapped_multiline_inline_object_fixture(Some("he"), "מילה ", "מילה ")
    else {
        panic!(
            "RTL fixture must wrap to >=3 lines with the object on a non-last line and a short last line"
        );
    };
    assert_full_multiline_selection_covers_every_line(
        &layout,
        &text,
        object_span,
        "wrapped RTL multiline full selection",
    );
}

#[test]
fn wrapped_bidi_multiline_selection_keeps_text_after_the_object_on_its_line() {
    // LTR 基方向、前后混合 RTL 词：多行 bidi 行框上的对象两侧文本仍需完整高亮。
    let Some((layout, text, object_span)) =
        wrapped_multiline_inline_object_fixture(None, "word ", "מילה ")
    else {
        panic!(
            "bidi fixture must wrap to >=3 lines with the object on a non-last line and a short last line"
        );
    };
    assert_full_multiline_selection_covers_every_line(
        &layout,
        &text,
        object_span,
        "wrapped bidi multiline full selection",
    );
}

#[test]
fn inline_object_wider_than_line_occupies_an_overflowing_line() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let base_style = ShapingStyle::new(1200);
    let text = paragraph("a\u{fffc}b", Some("en"));
    let mut layouter = ParagraphLayouter::new();
    for width_pt in [8.0, 10.0, 20.0] {
        let (_, layout) = layouter
            .layout_with_inline_objects(
                &mut catalog,
                &text,
                &settings,
                &base_style,
                layout_options(width_pt),
                &[InlineObjectLayout {
                    id: 42,
                    source_bytes: 1..4,
                    advance_pt: 30.0,
                    ascent_pt: 20.0,
                    descent_pt: 5.0,
                }],
            )
            .unwrap_or_else(|error| {
                panic!(
                    "an overwide inline object should remain layoutable at {width_pt} pt: {error}"
                )
            });

        assert_eq!(
            layout
                .paragraph
                .lines
                .iter()
                .map(|line| line.source_bytes.clone())
                .collect::<Vec<_>>(),
            [0..1, 1..4, 4..5]
        );
        assert!(layout.paragraph.lines[1].overflows_width);
    }
}

#[test]
fn inline_object_caret_affinity_selects_the_object_or_wrapped_neighbor() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let base_style = ShapingStyle::new(1200);
    let text = paragraph("a\u{fffc}b", Some("en"));
    let mut layouter = ParagraphLayouter::new();
    let (_, layout) = layouter
        .layout_with_inline_objects(
            &mut catalog,
            &text,
            &settings,
            &base_style,
            layout_options(20.0),
            &[InlineObjectLayout {
                id: 42,
                source_bytes: 1..4,
                advance_pt: 30.0,
                ascent_pt: 20.0,
                descent_pt: 5.0,
            }],
        )
        .unwrap_or_else(|error| panic!("layout failed: {error}"));
    let upstream_before = layout
        .cursor_at(1, crate::VisualAffinity::Upstream)
        .and_then(|cursor| layout.cursor_geometry_with_line(cursor, 0.0))
        .unwrap_or_else(|| panic!("upstream formula boundary should be placeable"));
    let downstream_before = layout
        .cursor_at(1, crate::VisualAffinity::Downstream)
        .and_then(|cursor| layout.cursor_geometry_with_line(cursor, 0.0))
        .unwrap_or_else(|| panic!("downstream formula boundary should be placeable"));
    let upstream_after = layout
        .cursor_at(4, crate::VisualAffinity::Upstream)
        .and_then(|cursor| layout.cursor_geometry_with_line(cursor, 0.0))
        .unwrap_or_else(|| panic!("upstream formula end should be placeable"));
    let downstream_after = layout
        .cursor_at(4, crate::VisualAffinity::Downstream)
        .and_then(|cursor| layout.cursor_geometry_with_line(cursor, 0.0))
        .unwrap_or_else(|| panic!("downstream formula end should be placeable"));

    assert_eq!(upstream_before.1, 0);
    assert!(upstream_before.0.x0 > 0.0);
    assert_eq!(downstream_before.1, 1);
    assert_eq!(downstream_before.0.x0, 0.0);
    assert_eq!(upstream_after.1, 1);
    assert_eq!(upstream_after.0.x0, 30.0);
    assert_eq!(downstream_after.1, 2);
    assert_eq!(downstream_after.0.x0, 0.0);
}

#[test]
fn inline_object_boundaries_fence_adjacent_combining_graphemes() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let base_style = ShapingStyle::new(1200);
    let text = paragraph("a\u{fffc}\u{0301}b", Some("en"));
    let mut layouter = ParagraphLayouter::new();
    let (_, layout) = layouter
        .layout_with_inline_objects(
            &mut catalog,
            &text,
            &settings,
            &base_style,
            layout_options(200.0),
            &[InlineObjectLayout {
                id: 42,
                source_bytes: 1..4,
                advance_pt: 12.0,
                ascent_pt: 10.0,
                descent_pt: 2.0,
            }],
        )
        .unwrap_or_else(|error| panic!("inline object should lay out: {error}"));

    assert_eq!(layout.grapheme_boundaries, [0, 1, 4, 6, 7]);
    assert!(
        layout
            .cursor_at(4, crate::VisualAffinity::Downstream)
            .is_some()
    );
    assert!(layout.selection_geometry(4..6).is_some());
}

#[test]
fn inline_objects_in_rtl_runs_map_visual_sides_to_logical_edges() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let base_style = ShapingStyle::new(1200);
    let text = paragraph("א\u{fffc}\u{fffc}ב", Some("he"));
    let mut layouter = ParagraphLayouter::new();
    let (_, layout) = layouter
        .layout_with_inline_objects(
            &mut catalog,
            &text,
            &settings,
            &base_style,
            layout_options(200.0),
            &[
                InlineObjectLayout {
                    id: 42,
                    source_bytes: 2..5,
                    advance_pt: 30.0,
                    ascent_pt: 20.0,
                    descent_pt: 5.0,
                },
                InlineObjectLayout {
                    id: 43,
                    source_bytes: 5..8,
                    advance_pt: 30.0,
                    ascent_pt: 20.0,
                    descent_pt: 5.0,
                },
            ],
        )
        .unwrap_or_else(|error| panic!("RTL inline objects should lay out: {error}"));
    let line = layout
        .layout
        .lines()
        .next()
        .unwrap_or_else(|| panic!("RTL paragraph should have a line"));
    let boxes = line
        .items()
        .filter_map(|item| match item {
            parley::PositionedLayoutItem::InlineBox(box_item) => {
                Some((box_item.id, box_item.x, box_item.width))
            }
            parley::PositionedLayoutItem::GlyphRun(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(boxes.len(), 2);

    for (id, source_start, source_end) in [(42, 2, 5), (43, 5, 8)] {
        let (_, x, width) = boxes
            .iter()
            .find(|(box_id, _, _)| *box_id == id)
            .copied()
            .unwrap_or_else(|| panic!("inline box {id} should be positioned"));
        let visual_left = layout
            .cursor_from_point_on_line(x + width * 0.25, 0)
            .unwrap_or_else(|| panic!("left side of RTL formula {id} should be hit-testable"));
        let visual_right = layout
            .cursor_from_point_on_line(x + width * 0.75, 0)
            .unwrap_or_else(|| panic!("right side of RTL formula {id} should be hit-testable"));
        assert_eq!(visual_left.cursor.byte_index(), source_end);
        assert_eq!(
            visual_left.cursor.affinity(),
            crate::VisualAffinity::Upstream
        );
        assert_eq!(visual_left.logical_affinity, crate::HitAffinity::Upstream);
        assert_eq!(visual_right.cursor.byte_index(), source_start);
        assert_eq!(
            visual_right.cursor.affinity(),
            crate::VisualAffinity::Downstream
        );
        assert_eq!(
            visual_right.logical_affinity,
            crate::HitAffinity::Downstream
        );
        assert!((layout.cursor_geometry(visual_left.cursor, 0.0).x0 - f64::from(x)).abs() < 0.01);
        assert!(
            (layout.cursor_geometry(visual_right.cursor, 0.0).x0 - f64::from(x + width)).abs()
                < 0.01
        );
    }
}

#[test]
fn duplicate_inline_object_ids_and_ranges_are_rejected_atomically() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let base_style = ShapingStyle::new(1200);
    let text = paragraph("a\u{fffc}\u{fffc}b", Some("en"));
    let object = |id, source_bytes| InlineObjectLayout {
        id,
        source_bytes,
        advance_pt: 20.0,
        ascent_pt: 12.0,
        descent_pt: 4.0,
    };
    let mut layouter = ParagraphLayouter::new();
    for invalid_objects in [
        vec![object(42, 1..4), object(42, 4..7)],
        vec![object(42, 1..4), object(43, 1..4)],
    ] {
        assert!(matches!(
            layouter.layout_with_inline_objects(
                &mut catalog,
                &text,
                &settings,
                &base_style,
                layout_options(200.0),
                &invalid_objects,
            ),
            Err(crate::ParagraphLayoutError::InconsistentShaping)
        ));
    }
    assert!(
        layouter
            .layout_with_inline_objects(
                &mut catalog,
                &text,
                &settings,
                &base_style,
                layout_options(200.0),
                &[object(42, 1..4), object(43, 4..7)],
            )
            .is_ok()
    );
}

fn text_flow(run: Run) -> ParagraphContent {
    ParagraphContent::Text(TextFlow {
        runs: vec![run],
        extra: Default::default(),
    })
}

#[test]
fn invalid_layout_requests_leave_the_reusable_layouter_available() {
    let mut catalog = font_catalog();
    let mut layouter = ParagraphLayouter::new();
    let text = paragraph("still usable", Some("en"));
    let settings = document_settings();
    for width in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let Err(error) = layouter.layout(
            &mut catalog,
            &text,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(width),
        ) else {
            panic!("invalid width must be rejected");
        };
        assert_eq!(error.to_string(), "layout.invalid_width");
    }
    let Err(error) = layouter.layout(
        &mut catalog,
        &text,
        &settings,
        &ShapingStyle::new(0),
        layout_options(120.0),
    ) else {
        panic!("zero font size must be rejected");
    };
    assert_eq!(error.to_string(), "layout.invalid_font_size");
    let Err(error) = layouter.layout(
        &mut catalog,
        &text,
        &settings,
        &ShapingStyle::new(1200),
        ParagraphLayoutOptions {
            spacing: LineSpacing::Multiple { percent: 0 },
            ..layout_options(120.0)
        },
    ) else {
        panic!("invalid spacing must be rejected");
    };
    assert_eq!(error.to_string(), "layout.invalid_line_spacing");
    let (source, result) = layouter
        .layout(
            &mut catalog,
            &text,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(120.0),
        )
        .unwrap_or_else(|error| panic!("reusable layout failed after bad input: {error}"));
    assert_eq!(source, "still usable");
    assert!(!result.paragraph.lines.is_empty());
}

#[test]
fn first_line_indent_offsets_only_the_first_visual_line() {
    let mut catalog = font_catalog();
    let mut layouter = ParagraphLayouter::new();
    let source = paragraph("one two three four five six seven eight", Some("en"));
    let style = ShapingStyle::new(1200);
    let settings = document_settings();
    let (_, layout) = layouter
        .layout(
            &mut catalog,
            &source,
            &settings,
            &style,
            ParagraphLayoutOptions {
                width_pt: 90.0,
                first_line_indent_chars: Some(2),
                ..layout_options(90.0)
            },
        )
        .unwrap_or_else(|error| panic!("indented paragraph failed to layout: {error}"));

    let glyph_x = layout
        .layout
        .lines()
        .map(|line| {
            line.items()
                .filter_map(|item| match item {
                    parley::PositionedLayoutItem::GlyphRun(run) => {
                        run.positioned_glyphs().next().map(|glyph| glyph.x)
                    }
                    parley::PositionedLayoutItem::InlineBox(_) => None,
                })
                .next()
                .unwrap_or_else(|| panic!("visual line has no glyph"))
        })
        .collect::<Vec<_>>();

    assert!(glyph_x.len() > 1, "fixture must wrap to multiple lines");
    assert!(
        glyph_x[0] >= 23.9,
        "first line should begin after a 2 em indent: {glyph_x:?}"
    );
    assert!(
        glyph_x[1] < 1.0,
        "continuation lines should keep the normal margin"
    );
    let first_cursor = layout
        .cursor_at(0, crate::VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("paragraph start is not a cursor boundary"));
    let cursor_x = layout.cursor_geometry(first_cursor, 0.0).x0;
    assert!(
        cursor_x >= 23.9,
        "the first-line caret should follow the indent: {cursor_x}"
    );
}

#[test]
fn empty_first_line_indent_places_the_caret_after_the_indent() {
    let mut catalog = font_catalog();
    let mut layouter = ParagraphLayouter::new();
    let source = paragraph("", Some("en"));
    let style = ShapingStyle::new(1200);
    let (_, layout) = layouter
        .layout(
            &mut catalog,
            &source,
            &document_settings(),
            &style,
            ParagraphLayoutOptions {
                first_line_indent_chars: Some(2),
                ..layout_options(240.0)
            },
        )
        .unwrap_or_else(|error| panic!("empty indented paragraph failed to layout: {error}"));
    let cursor = layout
        .cursor_at(0, crate::VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("empty paragraph has no cursor boundary"));
    let cursor_x = layout.cursor_geometry(cursor, 0.0).x0;
    assert!(
        cursor_x >= 23.9,
        "empty first-line caret should follow the 2 em indent: {cursor_x}"
    );
}

#[test]
fn ordinary_and_em_spaces_use_font_relative_advances() {
    let mut catalog = font_catalog();
    let mut layouter = ParagraphLayouter::new();
    let source = paragraph("a a\u{2003}a", Some("en"));
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &source,
            &document_settings(),
            &ShapingStyle::new(1200),
            layout_options(200.0),
        )
        .unwrap_or_else(|error| panic!("space paragraph failed to layout: {error}"));

    let mut advances = Vec::new();
    for line in result.layout.lines() {
        for run in line.runs() {
            for cluster in run.clusters() {
                let range = cluster.text_range();
                let cluster_text = &"a a\u{2003}a"[range];
                if cluster_text == " " || cluster_text == "\u{2003}" {
                    advances.push((cluster_text, cluster.advance()));
                }
            }
        }
    }

    assert_eq!(
        advances.len(),
        2,
        "both source spaces remain separate clusters"
    );
    assert!(
        (advances[0].1 - 6.0).abs() < 0.1,
        "U+0020 should occupy half of the 12 pt em: {advances:?}"
    );
    assert!(
        (advances[1].1 - 12.0).abs() < 0.1,
        "U+2003 should occupy one 12 pt em: {advances:?}"
    );
}

#[test]
fn code_font_keeps_native_monospace_space_advance() {
    let mut catalog = font_catalog();
    let mut layouter = ParagraphLayouter::new();
    let source_text = "i W";
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph(source_text, Some("en")),
            &document_settings(),
            &ShapingStyle::code(1200),
            layout_options(200.0),
        )
        .unwrap_or_else(|error| panic!("code paragraph failed to layout: {error}"));

    let advances: Vec<_> = result
        .layout
        .lines()
        .flat_map(|line| line.runs())
        .flat_map(|run| run.clusters())
        .map(|cluster| (&source_text[cluster.text_range()], cluster.advance()))
        .collect();
    assert_eq!(advances.len(), 3);
    let cell_width = advances[0].1;
    assert!(cell_width > 0.0);
    assert!(
        advances
            .iter()
            .all(|(_, advance)| (*advance - cell_width).abs() < 0.01),
        "code glyphs and spaces should occupy one native monospace cell: {advances:?}"
    );
}

#[test]
fn ordinary_space_calibration_does_not_change_nbsp_width_on_a_reused_layouter() {
    let mut catalog = font_catalog();
    let mut layouter = ParagraphLayouter::new();
    let settings = document_settings();
    let style = ShapingStyle::new(1200);
    let mut measure = |layouter: &mut ParagraphLayouter, text: &str, target: &str| {
        let (_, result) = layouter
            .layout(
                &mut catalog,
                &paragraph(text, Some("en")),
                &settings,
                &style,
                layout_options(240.0),
            )
            .unwrap_or_else(|error| panic!("space test text failed to layout: {error}"));
        result
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .flat_map(|run| run.clusters())
            .filter(|cluster| &text[cluster.text_range()] == target)
            .map(|cluster| cluster.advance())
            .next()
    };

    let before = measure(&mut layouter, "a\u{00a0}b", "\u{00a0}")
        .unwrap_or_else(|| panic!("NBSP cluster was not shaped"));
    let ordinary = measure(&mut layouter, "a b", " ")
        .unwrap_or_else(|| panic!("ordinary space cluster was not shaped"));
    let after = measure(&mut layouter, "a\u{00a0}b", "\u{00a0}")
        .unwrap_or_else(|| panic!("NBSP cluster was not shaped after calibration"));

    assert!((ordinary - 6.0).abs() < 0.1);
    assert!(
        (before - after).abs() < 0.01,
        "NBSP width must not depend on earlier layouts: before={before}, after={after}"
    );
}

#[test]
fn space_calibration_preserves_shaping_across_equal_style_runs() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let mut split = paragraph("", Some("en"));
    split.inlines = vec![
        text_flow(Run {
            text: "a off".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
        text_flow(Run {
            text: "ice".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
    ];
    let (_, flat) = layouter
        .layout(
            &mut catalog,
            &paragraph("a office", Some("en")),
            &settings,
            &ShapingStyle::new(1200),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("flat text failed to layout: {error}"));
    let (_, split) = layouter
        .layout(
            &mut catalog,
            &split,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("equal-style runs failed to layout: {error}"));

    assert_eq!(
        glyph_signature(&flat.layout),
        glyph_signature(&split.layout),
        "splitting equal-style text at a space must preserve glyph shaping and advances"
    );
}

#[test]
fn ordinary_space_width_includes_combining_marks_in_its_grapheme() {
    let mut catalog = font_catalog();
    let mut layouter = ParagraphLayouter::new();
    let source_text = "a \u{301}a";
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph(source_text, Some("en")),
            &document_settings(),
            &ShapingStyle::new(1200),
            layout_options(200.0),
        )
        .unwrap_or_else(|error| panic!("combining-space paragraph failed to layout: {error}"));

    let cluster = result
        .layout
        .lines()
        .flat_map(|line| line.runs())
        .flat_map(|run| run.clusters())
        .find(|cluster| {
            let range = cluster.text_range();
            source_text[range].starts_with(' ')
        })
        .unwrap_or_else(|| panic!("space grapheme cluster is missing"));
    assert!(
        (cluster.advance() - 6.0).abs() < 0.1,
        "space and its combining mark must occupy half an em: {} pt",
        cluster.advance()
    );
}

fn document_settings() -> ResolvedDocumentTextSettings {
    ResolvedDocumentTextSettings::from_document_settings(&DocumentSettings::default())
        .unwrap_or_else(|error| panic!("document settings failed: {error}"))
}

fn empty_metrics() -> FontLineMetrics {
    FontLineMetrics {
        ascent_pt: 9.6,
        descent_pt: 2.4,
        leading_pt: 0.0,
    }
}

fn layout_options(width_pt: f32) -> ParagraphLayoutOptions {
    ParagraphLayoutOptions {
        width_pt,
        spacing: LineSpacing::Natural,
        line_grid_metrics: empty_metrics(),
        line_grid_font_size_centipoints: 1200,
        first_line_indent_chars: None,
    }
}

fn glyph_signature(layout: &parley::Layout<[u8; 4]>) -> Vec<(u32, f32, f32, f32)> {
    layout
        .lines()
        .flat_map(|line| {
            line.items().filter_map(|item| match item {
                parley::PositionedLayoutItem::GlyphRun(run) => Some(run),
                parley::PositionedLayoutItem::InlineBox(_) => None,
            })
        })
        .flat_map(|run| run.positioned_glyphs().collect::<Vec<_>>())
        .map(|glyph| (glyph.id, glyph.x, glyph.y, glyph.advance))
        .collect()
}

#[test]
fn unicode_boundaries_keep_original_graphemes_and_cover_utf8_text() {
    let text = "e\u{301} 👩‍👩‍👧‍👦\n中文";
    let boundaries = UnicodeBoundaries::new(text);
    assert_eq!(UNICODE_DATA_VERSION, "17.0.0");
    assert!(boundaries.grapheme.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(boundaries.grapheme.first(), Some(&0));
    assert_eq!(boundaries.grapheme.last(), Some(&text.len()));
    assert!(boundaries.grapheme.contains(&"e\u{301}".len()));
    assert!(boundaries.line_break.contains(&0));
    assert!(boundaries.line_break.contains(&text.len()));
}

#[test]
fn locale_tailoring_keeps_english_chinese_and_dictionary_break_rules() {
    let english = "A short scientific paper.";
    let english_boundaries = UnicodeBoundaries::new_with_language(english, Some("en-US"));
    assert!(english_boundaries.line_break.contains(&"A ".len()));
    assert!(english_boundaries.line_break.contains(&english.len()));

    let thai = "ภาษาไทยภาษาไทย";
    let thai_boundaries = UnicodeBoundaries::new_with_language(thai, Some("th-TH"));
    assert!(thai_boundaries.line_break.len() > 2);
    assert_eq!(thai_boundaries.line_break.first(), Some(&0));
    assert_eq!(thai_boundaries.line_break.last(), Some(&thai.len()));
}

#[test]
fn language_tailoring_ignores_same_language_run_boundaries() {
    let text = "ภาษาไทยภาษาไทย";
    let style = ShapingStyle {
        language: Some("th-TH".to_owned()),
        ..ShapingStyle::new(1200)
    };
    let flat = [StyledSpan {
        text,
        style: style.clone(),
        underline: false,
    }];
    let split_at = 12;
    let split = [
        StyledSpan {
            text: &text[..split_at],
            style: style.clone(),
            underline: false,
        },
        StyledSpan {
            text: &text[split_at..],
            style,
            underline: false,
        },
    ];

    let flat_boundaries = boundaries_for_spans(text, &flat);
    let split_boundaries = boundaries_for_spans(text, &split);

    assert_eq!(split_boundaries, flat_boundaries);

    let chinese_text = "你好，世界。再见！";
    let chinese_style = ShapingStyle {
        language: Some("zh-Hans".to_owned()),
        ..ShapingStyle::new(1200)
    };
    let flat_chinese = [StyledSpan {
        text: chinese_text,
        style: chinese_style.clone(),
        underline: false,
    }];
    let split_at = "你好".len();
    let split_chinese = [
        StyledSpan {
            text: &chinese_text[..split_at],
            style: chinese_style.clone(),
            underline: false,
        },
        StyledSpan {
            text: &chinese_text[split_at..],
            style: chinese_style,
            underline: false,
        },
    ];
    assert_eq!(
        boundaries_for_spans(chinese_text, &split_chinese),
        boundaries_for_spans(chinese_text, &flat_chinese)
    );
}

#[test]
fn unicode_17_grapheme_and_line_break_conformance_corpora_pass() {
    run_unicode_break_corpus(
        include_str!("../../../tests/fixtures/unicode-17/GraphemeBreakTest.txt"),
        true,
    );
    run_unicode_break_corpus(
        include_str!("../../../tests/fixtures/unicode-17/LineBreakTest.txt"),
        false,
    );
}

#[test]
fn parley_layout_matches_icu_line_ranges_for_english_chinese_and_bidi() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    for (text, language) in [
        ("alpha beta gamma delta epsilon", Some("en")),
        ("你好，世界。再见！", Some("zh-Hans")),
        ("abc אבג 中文 xyz", Some("en")),
    ] {
        let paragraph = paragraph(text, language);
        let (_, layout) = layouter
            .layout(
                &mut catalog,
                &paragraph,
                &settings,
                &ShapingStyle::new(1200),
                layout_options(48.0),
            )
            .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
        assert!(layout.paragraph.lines.len() > 1);
        assert_eq!(
            layout.grapheme_boundaries.first(),
            Some(&0),
            "grapheme start for {text:?}"
        );
        assert_eq!(
            layout.grapheme_boundaries.last(),
            Some(&text.len()),
            "grapheme end for {text:?}"
        );
        assert!(layout.paragraph.lines.iter().all(|line| {
            text.is_char_boundary(line.source_bytes.start)
                && text.is_char_boundary(line.source_bytes.end)
                && line.metrics.height_pt.is_finite()
                && line.metrics.height_pt > 0.0
        }));
        assert!(
            layout
                .paragraph
                .lines
                .windows(2)
                .all(|lines| { lines[0].consumed_bytes.end == lines[1].source_bytes.start })
        );
    }
}

#[test]
fn swash_rasterizes_the_font_face_and_glyph_selected_by_parley() {
    let mut catalog = font_catalog();
    let mut layouter = ParagraphLayouter::new();
    let (_, layout) = layouter
        .layout(
            &mut catalog,
            &paragraph("Raster", Some("en")),
            &document_settings(),
            &ShapingStyle::new(1200),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    let (run, glyph) = layout
        .layout
        .lines()
        .flat_map(|line| line.items())
        .find_map(|item| match item {
            parley::PositionedLayoutItem::GlyphRun(run) => {
                let glyph = run.clone().positioned_glyphs().next();
                glyph.map(|glyph| (run, glyph))
            }
            parley::PositionedLayoutItem::InlineBox(_) => None,
        })
        .unwrap_or_else(|| panic!("Parley layout should contain a glyph"));
    let normalized_coords: Vec<_> = run
        .run()
        .normalized_coords()
        .iter()
        .map(|coordinate| coordinate.to_bits())
        .collect();
    let mut rasterizer = GlyphRasterizer::default();
    let bitmap = rasterizer
        .rasterize(
            run.run().font(),
            glyph.id,
            GlyphRasterOptions {
                pixels_per_em: run.run().font_size() * 96.0 / 72.0,
                pixel_phase: [0.0, 0.0],
                normalized_coords: &normalized_coords,
                synthesis: run.run().synthesis(),
                foreground: [28, 28, 28, 255],
            },
        )
        .unwrap_or_else(|| panic!("Swash should rasterize Parley's glyph"));
    let cached_bitmap = rasterizer
        .rasterize(
            run.run().font(),
            glyph.id,
            GlyphRasterOptions {
                pixels_per_em: run.run().font_size() * 96.0 / 72.0,
                pixel_phase: [0.0, 0.0],
                normalized_coords: &normalized_coords,
                synthesis: run.run().synthesis(),
                foreground: [28, 28, 28, 255],
            },
        )
        .unwrap_or_else(|| panic!("cached glyph should remain available"));
    assert!(std::sync::Arc::ptr_eq(&bitmap, &cached_bitmap));
    assert!(bitmap.width > 0);
    assert!(bitmap.height > 0);
    assert!(
        bitmap
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0)
    );
    let pixel_count = usize::try_from(bitmap.width)
        .unwrap_or_else(|error| panic!("glyph width is invalid: {error}"))
        .checked_mul(
            usize::try_from(bitmap.height)
                .unwrap_or_else(|error| panic!("glyph height is invalid: {error}")),
        )
        .unwrap_or_else(|| panic!("glyph size overflows"));
    assert_eq!(bitmap.pixels.len(), pixel_count * 4);
}

#[test]
fn run_language_and_paragraph_environment_styles_reach_parley() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let mut paragraph = paragraph("Bold Thai text ภาษาไทย", Some("en"));
    let bold = InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::new(),
        children: vec![text_flow(Run {
            text: "Bold Thai text ภาษาไทย".to_owned(),
            language: Some("th-TH".to_owned()),
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    };
    paragraph.inlines = vec![ParagraphContent::Environment(bold)];
    let (text, result) = layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(120.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    assert_eq!(text, "Bold Thai text ภาษาไทย");
    assert!(result.paragraph.lines.len() > 1);
    assert!(
        result
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .any(|run| run.font_attrs().weight == fontique::FontWeight::BOLD)
    );
}

#[test]
fn missing_font_falls_back_for_mixed_scripts_and_emoji() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let text = "Caret 文稿 العربية Русский Ελληνικά 😀 👩‍👩‍👧‍👦";
    let style = ShapingStyle {
        requested_family: Some("Caret Missing Test Family 8D5A".to_owned()),
        ..ShapingStyle::new(1200)
    };
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph(text, Some("en")),
            &settings,
            &style,
            layout_options(360.0),
        )
        .unwrap_or_else(|error| panic!("fallback shaping failed: {error}"));

    assert!(result.requested_family_missing);
    let glyphs = glyph_signature(&result.layout);
    assert!(!glyphs.is_empty());
    assert!(glyphs.iter().all(|glyph| glyph.0 != 0));
    assert!(
        result
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .any(|run| run.is_rtl())
    );
    assert!(result.paragraph.lines.iter().all(|line| {
        line.advance_pt.is_finite()
            && line.metrics.height_pt.is_finite()
            && line.metrics.height_pt > 0.0
    }));
    assert!(
        result
            .grapheme_boundaries
            .binary_search(&text.len())
            .is_ok()
    );

    for emoji_text in ["😀", "👩‍👩‍👧‍👦"] {
        let (_, emoji_layout) = layouter
            .layout(
                &mut catalog,
                &paragraph(emoji_text, Some("en")),
                &settings,
                &style,
                layout_options(120.0),
            )
            .unwrap_or_else(|error| panic!("emoji layout failed for {emoji_text:?}: {error}"));
        assert_eq!(emoji_layout.grapheme_boundaries, vec![0, emoji_text.len()]);
        let emoji_glyphs = glyph_signature(&emoji_layout.layout);
        assert!(!emoji_glyphs.is_empty());
        assert!(emoji_glyphs.iter().all(|glyph| glyph.0 != 0));
    }
}

#[test]
fn half_point_font_sizes_and_cyrillic_greek_keep_finite_layout_metrics() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let text = "Русский текст · Ελληνικά";
    let half_point = ShapingStyle {
        font_size_centipoints: 50,
        ..ShapingStyle::new(1200)
    };
    let (_, half_point_layout) = layouter
        .layout(
            &mut catalog,
            &paragraph(text, Some("ru")),
            &settings,
            &half_point,
            layout_options(180.0),
        )
        .unwrap_or_else(|error| panic!("0.5 pt layout failed: {error}"));
    assert!(
        half_point_layout
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .all(|run| run.font_size() == 0.5)
    );
    assert!(!glyph_signature(&half_point_layout.layout).is_empty());
    assert!(
        half_point_layout
            .paragraph
            .lines
            .iter()
            .all(|line| { line.metrics.height_pt.is_finite() && line.metrics.height_pt > 0.0 })
    );

    let fractional_point = ShapingStyle {
        font_size_centipoints: 1225,
        ..ShapingStyle::new(1200)
    };
    let (_, fractional_layout) = layouter
        .layout(
            &mut catalog,
            &paragraph("12.25 pt", Some("en")),
            &settings,
            &fractional_point,
            layout_options(180.0),
        )
        .unwrap_or_else(|error| panic!("12.25 pt layout failed: {error}"));
    assert!(
        fractional_layout
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .all(|run| run.font_size() == 12.25)
    );
}

#[test]
fn same_style_run_boundaries_preserve_positioned_glyphs() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let text = "office العربية e\u{301}";
    let mut split = paragraph("", None);
    split.inlines = vec![
        text_flow(Run {
            text: "office العربية e".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
        text_flow(Run {
            text: "\u{301}".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
    ];
    let (_, flat_layout) = layouter
        .layout(
            &mut catalog,
            &paragraph(text, None),
            &settings,
            &ShapingStyle::new(1200),
            layout_options(480.0),
        )
        .unwrap_or_else(|error| panic!("flat paragraph layout failed: {error}"));
    let (_, split_layout) = layouter
        .layout(
            &mut catalog,
            &split,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(480.0),
        )
        .unwrap_or_else(|error| panic!("split-run paragraph layout failed: {error}"));

    assert_eq!(
        glyph_signature(&flat_layout.layout),
        glyph_signature(&split_layout.layout)
    );
    assert_eq!(flat_layout.paragraph.lines, split_layout.paragraph.lines);
}

#[test]
fn font_environment_and_nested_bold_reach_parley_style_ranges() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let family = catalog.families()[0].name.clone();
    let paragraph = Paragraph {
        id: NodeId::generate(),
        language: Some("en".to_owned()),
        layout: None,
        inlines: vec![
            text_flow(Run {
                text: "plain ".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }),
            ParagraphContent::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Font,
                definition_id: None,
                props: BTreeMap::from([(
                    "fontFamily".to_owned(),
                    serde_json::Value::String(family),
                )]),
                children: vec![ParagraphContent::Environment(InlineEnvironment {
                    id: NodeId::generate(),
                    kind: InlineEnvironmentKind::Bold,
                    definition_id: None,
                    props: BTreeMap::new(),
                    children: vec![text_flow(Run {
                        text: "bold".to_owned(),
                        language: None,
                        props: BTreeMap::new(),
                        extra: BTreeMap::new(),
                    })],
                    extra: BTreeMap::new(),
                })],
                extra: BTreeMap::new(),
            }),
        ],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let mut layouter = ParagraphLayouter::new();
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("nested style layout failed: {error}"));
    assert!(!result.requested_family_missing);
    assert!(
        result
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .any(|run| {
                run.text_range().start >= "plain ".len()
                    && run.font_attrs().weight == fontique::FontWeight::BOLD
            })
    );
}

#[test]
fn font_environment_default_marker_does_not_request_a_literal_font() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let paragraph = Paragraph {
        id: NodeId::generate(),
        language: Some("en".to_owned()),
        layout: None,
        inlines: vec![ParagraphContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Font,
            definition_id: None,
            props: BTreeMap::from([(
                "fontFamily".to_owned(),
                serde_json::Value::String(caret_doc::DEFAULT_FONT_FAMILY.to_owned()),
            )]),
            children: vec![text_flow(Run {
                text: "ordinary text".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        })],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let mut layouter = ParagraphLayouter::new();
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("default-font environment layout failed: {error}"));

    assert!(!result.requested_family_missing);
    assert!(result.layout.lines().next().is_some());
}

#[test]
fn text_island_matches_paragraph_shaping_and_keeps_trailing_space_advance() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let style = ShapingStyle::new(1200);
    let source = "office ";
    let mut paragraph_layouter = ParagraphLayouter::new();
    let (_, paragraph_result) = paragraph_layouter
        .layout(
            &mut catalog,
            &paragraph(source, Some("en")),
            &settings,
            &style,
            layout_options(1000.0),
        )
        .unwrap_or_else(|error| panic!("paragraph shaping failed: {error}"));
    let island = paragraph_layouter
        .shape_text_island(
            &mut catalog,
            &settings,
            &[
                TextIslandSpan {
                    text: "of",
                    style: style.clone(),
                    underline: false,
                },
                TextIslandSpan {
                    text: "fice ",
                    style: style.clone(),
                    underline: false,
                },
            ],
        )
        .unwrap_or_else(|error| panic!("text island shaping failed: {error}"));

    assert_eq!(island.text, source);
    assert_eq!(
        glyph_signature(&island.layout),
        glyph_signature(&paragraph_result.layout)
    );
    assert_eq!(island.layout.lines().count(), 1);
    assert!(
        (island.metrics.advance_pt - paragraph_result.paragraph.lines[0].advance_pt).abs() < 0.01
    );
    assert!(island.metrics.advance_pt > 0.0);
    assert!(island.metrics.ascent_pt.is_finite() && island.metrics.ascent_pt > 0.0);
    assert!(island.metrics.descent_pt.is_finite() && island.metrics.descent_pt >= 0.0);
    let without_space = paragraph_layouter
        .shape_text_island(
            &mut catalog,
            &settings,
            &[TextIslandSpan {
                text: "office",
                style: style.clone(),
                underline: false,
            }],
        )
        .unwrap_or_else(|error| panic!("text without trailing whitespace failed: {error}"));
    assert!(island.metrics.advance_pt - without_space.metrics.advance_pt > 5.9);
}

#[test]
fn derived_text_island_font_size_scales_calibrated_spaces() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let style = ShapingStyle::new(1200);
    let mut layouter = ParagraphLayouter::new();

    for text in ["a a", "a\u{2003}a"] {
        let base = layouter
            .shape_text_island_at_font_size_pt(
                &mut catalog,
                &settings,
                &[TextIslandSpan {
                    text,
                    style: style.clone(),
                    underline: false,
                }],
                12.0,
            )
            .unwrap_or_else(|error| panic!("base-size text island failed: {error}"));
        for derived_size_pt in [6.0, 6.135] {
            let derived = layouter
                .shape_text_island_at_font_size_pt(
                    &mut catalog,
                    &settings,
                    &[TextIslandSpan {
                        text,
                        style: style.clone(),
                        underline: false,
                    }],
                    derived_size_pt,
                )
                .unwrap_or_else(|error| panic!("derived-size text island failed: {error}"));
            let expected_advance = base.metrics.advance_pt * derived_size_pt / 12.0;
            assert!(
                (derived.metrics.advance_pt - expected_advance).abs() < 0.6,
                "space calibration must use {derived_size_pt} pt for {text:?}: base={}, derived={}, expected={expected_advance}",
                base.metrics.advance_pt,
                derived.metrics.advance_pt,
            );
        }
    }
}

#[test]
fn text_island_shapes_graphemes_across_spans_and_handles_rtl() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let latin = ShapingStyle {
        language: Some("en".to_owned()),
        ..ShapingStyle::new(1200)
    };
    let combining = layouter
        .shape_text_island(
            &mut catalog,
            &settings,
            &[
                TextIslandSpan {
                    text: "e",
                    style: latin.clone(),
                    underline: false,
                },
                TextIslandSpan {
                    text: "\u{301}",
                    style: latin.clone(),
                    underline: true,
                },
            ],
        )
        .unwrap_or_else(|error| panic!("cross-span grapheme shaping failed: {error}"));
    let glyphs = glyph_signature(&combining.layout);
    assert_eq!(combining.text, "e\u{301}");
    assert_eq!(
        glyphs.len(),
        1,
        "base and combining mark should shape as one cluster"
    );

    let ligature_reference = layouter
        .shape_text_island(
            &mut catalog,
            &settings,
            &[TextIslandSpan {
                text: "office",
                style: latin.clone(),
                underline: false,
            }],
        )
        .unwrap_or_else(|error| panic!("reference ligature shaping failed: {error}"));
    let ligature_across_spans = layouter
        .shape_text_island(
            &mut catalog,
            &settings,
            &[
                TextIslandSpan {
                    text: "of",
                    style: latin.clone(),
                    underline: false,
                },
                TextIslandSpan {
                    text: "fice",
                    style: latin.clone(),
                    underline: true,
                },
            ],
        )
        .unwrap_or_else(|error| panic!("cross-span ligature shaping failed: {error}"));
    assert_eq!(
        glyph_signature(&ligature_across_spans.layout),
        glyph_signature(&ligature_reference.layout),
        "underline span boundaries should not change ligature shaping"
    );

    let rtl_style = ShapingStyle {
        language: Some("he".to_owned()),
        ..ShapingStyle::new(1200)
    };
    let rtl = layouter
        .shape_text_island(
            &mut catalog,
            &settings,
            &[
                TextIslandSpan {
                    text: "אב",
                    style: rtl_style.clone(),
                    underline: false,
                },
                TextIslandSpan {
                    text: "ג",
                    style: rtl_style,
                    underline: true,
                },
            ],
        )
        .unwrap_or_else(|error| panic!("cross-span RTL shaping failed: {error}"));
    assert_eq!(rtl.text, "אבג");
    assert_eq!(rtl.layout.lines().count(), 1);
    assert!(
        rtl.layout
            .lines()
            .flat_map(|line| line.runs())
            .any(|run| run.is_rtl())
    );
}

#[test]
fn text_island_hit_and_selection_geometry_stay_on_grapheme_boundaries() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    for text in ["a\u{301}b", "العربية", "ab"] {
        let island = layouter
            .shape_text_island(
                &mut catalog,
                &settings,
                &[TextIslandSpan {
                    text,
                    style: ShapingStyle::new(1200),
                    underline: false,
                }],
            )
            .unwrap_or_else(|error| panic!("grapheme interaction island failed: {error}"));
        let line = island
            .layout
            .lines()
            .next()
            .unwrap_or_else(|| panic!("nonempty island should have a line"));
        let line_metrics = line.metrics();
        let y = (line_metrics.block_min_coord + line_metrics.block_max_coord) / 2.0;
        for x in [0.0, line_metrics.advance] {
            let hit = island
                .cursor_from_point(x, y)
                .unwrap_or_else(|| panic!("line edge should hit {text:?}"));
            assert!(
                island
                    .grapheme_boundaries
                    .contains(&hit.cursor.byte_index()),
                "hit split a grapheme in {text:?}"
            );
            assert!(island.cursor_geometry(hit.cursor, 0.0).is_some());
        }
        if text == "ab" {
            for affinity in [VisualAffinity::Upstream, VisualAffinity::Downstream] {
                let caret = island
                    .cursor_geometry_at(1, affinity, 0.0)
                    .unwrap_or_else(|| panic!("interior caret should be available"));
                let hit = island
                    .cursor_from_point(caret.x0 as f32, ((caret.y0 + caret.y1) / 2.0) as f32)
                    .unwrap_or_else(|| panic!("exact caret coordinate should hit"));
                assert_eq!(hit.cursor.byte_index(), 1);
                assert_eq!(hit.cursor.affinity(), VisualAffinity::Downstream);
                assert_eq!(hit.logical_affinity, crate::HitAffinity::Downstream);
                assert_eq!(
                    hit.logical_affinity,
                    island
                        .logical_affinity_at(1, affinity)
                        .unwrap_or_else(|| panic!("logical affinity should resolve"))
                );
            }
        }
        assert!(
            island
                .selection_geometry(0..text.len())
                .is_some_and(|rectangles| {
                    !rectangles.is_empty()
                        && rectangles.iter().all(|(bounds, _)| bounds.x1 > bounds.x0)
                })
        );
        assert_eq!(island.selection_geometry(0..0), Some(Vec::new()));
        assert!(island.selection_geometry(0..text.len() + 1).is_none());
        if text == "a\u{301}b" {
            assert!(
                island.selection_geometry(1..1).is_none(),
                "a collapsed selection inside a grapheme must not select the grapheme"
            );
        }
    }
}

#[test]
fn text_island_shapes_mandatory_breaks_and_recovers_after_empty_or_failed_input() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let style = ShapingStyle::new(1200);
    let mut layouter = ParagraphLayouter::new();
    let empty = layouter
        .shape_text_island(&mut catalog, &settings, &[])
        .unwrap_or_else(|error| panic!("empty text island should succeed: {error}"));
    assert_eq!(empty.metrics.advance_pt, 0.0);
    assert_eq!(empty.metrics.ascent_pt, 0.0);
    assert_eq!(
        empty.logical_affinity_at(0, VisualAffinity::Downstream),
        Some(crate::HitAffinity::Downstream)
    );
    assert_eq!(empty.metrics.descent_pt, 0.0);
    assert_eq!(empty.layout.lines().count(), 0);
    assert_eq!(empty.grapheme_boundaries, [0]);
    assert!(!empty.requested_family_missing);

    for text in [
        "a\nb",
        "a\u{000b}b",
        "a\u{000c}b",
        "a\u{0085}b",
        "a\u{2028}b",
        "a\u{2029}b",
        "a\r\nb",
    ] {
        let shaped = layouter
            .shape_text_island(
                &mut catalog,
                &settings,
                &[TextIslandSpan {
                    text,
                    style: style.clone(),
                    underline: false,
                }],
            )
            .unwrap_or_else(|error| panic!("mandatory text break should shape: {error}"));
        assert_eq!(shaped.layout.lines().count(), 2, "{text:?}");
        assert_eq!(shaped.text, text);
        assert_eq!(shaped.layout.text_len(), text.len());
        assert!(shaped.metrics.ascent_pt > 0.0, "{text:?}");
        assert!(shaped.metrics.descent_pt > 0.0, "{text:?}");
        assert_eq!(shaped.grapheme_boundaries.first(), Some(&0));
        assert_eq!(shaped.grapheme_boundaries.last(), Some(&text.len()));
    }
    assert_eq!(
        layouter
            .shape_text_island(
                &mut catalog,
                &settings,
                &[TextIslandSpan {
                    text: "invalid size",
                    style: ShapingStyle::new(0),
                    underline: false,
                }],
            )
            .err(),
        Some(TextIslandError::InvalidFontSize),
    );
    let shaped = layouter
        .shape_text_island(
            &mut catalog,
            &settings,
            &[TextIslandSpan {
                text: "recovered",
                style,
                underline: false,
            }],
        )
        .unwrap_or_else(|error| panic!("layouter should recover after a rejected input: {error}"));
    assert_eq!(shaped.layout.lines().count(), 1);
    assert!(shaped.metrics.advance_pt.is_finite());
}

#[test]
fn text_island_keeps_adjacent_source_breaks_and_cross_run_boundaries() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let style = ShapingStyle::new(1200);
    for second in ["\u{000b}", "\u{000c}", "\u{0085}"] {
        let text = format!("a\r{second}b");
        let spans = [
            TextIslandSpan {
                text: "a\r",
                style: style.clone(),
                underline: false,
            },
            TextIslandSpan {
                text: second,
                style: style.clone(),
                underline: false,
            },
            TextIslandSpan {
                text: "b",
                style: style.clone(),
                underline: false,
            },
        ];
        let layout = ParagraphLayouter::new()
            .shape_text_island(&mut catalog, &settings, &spans)
            .unwrap_or_else(|error| panic!("adjacent breaks should shape: {error}"));
        assert_eq!(layout.text, text);
        assert_eq!(layout.layout.lines().count(), 3, "{text:?}");
        assert_eq!(
            layout.grapheme_boundaries,
            crate::UnicodeBoundaries::new(&text).grapheme
        );
        assert!(
            layout
                .cursor_at(2, crate::VisualAffinity::Downstream)
                .is_some()
        );
    }
}

#[test]
fn paragraph_and_text_island_share_mandatory_break_shaping_and_source_offsets() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let style = ShapingStyle::new(1200);
    for mandatory_break in ["\u{000b}", "\u{000c}", "\u{0085}"] {
        let text = format!("a{mandatory_break}b");
        let mut source_paragraph = paragraph(&text, None);
        let first = text_flow(Run {
            text: format!("a{}", &text[1..1 + mandatory_break.len()]),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        });
        let second = text_flow(Run {
            text: "b".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        });
        source_paragraph.inlines = vec![first, second];
        let mut paragraph_layouter = ParagraphLayouter::new();
        let (logical_text, paragraph_layout) = paragraph_layouter
            .layout(
                &mut catalog,
                &source_paragraph,
                &settings,
                &style,
                layout_options(200.0),
            )
            .unwrap_or_else(|error| panic!("paragraph should shape {text:?}: {error}"));
        let island = ParagraphLayouter::new()
            .shape_text_island(
                &mut catalog,
                &settings,
                &[TextIslandSpan {
                    text: &text,
                    style: style.clone(),
                    underline: false,
                }],
            )
            .unwrap_or_else(|error| panic!("text island should shape {text:?}: {error}"));

        assert_eq!(logical_text, text);
        assert_eq!(paragraph_layout.layout.lines().count(), 2, "{text:?}");
        assert_eq!(
            paragraph_layout.layout.lines().count(),
            island.layout.lines().count(),
            "paragraph and math TextFlow must share mandatory break behavior"
        );
        assert_eq!(
            paragraph_layout.grapheme_boundaries,
            UnicodeBoundaries::new(&text).grapheme,
            "paragraph source offsets must remain unchanged"
        );
        assert_eq!(
            paragraph_layout.grapheme_boundaries, island.grapheme_boundaries,
            "paragraph and math TextFlow must expose identical source boundaries"
        );
    }

    let source = "a\u{0085}\u{fffc}\u{0301}";
    let paragraph = paragraph(source, None);
    let (_, layout) = ParagraphLayouter::new()
        .layout_with_inline_objects(
            &mut catalog,
            &paragraph,
            &settings,
            &style,
            layout_options(200.0),
            &[InlineObjectLayout {
                id: 7,
                source_bytes: 3..6,
                advance_pt: 12.0,
                ascent_pt: 9.0,
                descent_pt: 3.0,
            }],
        )
        .unwrap_or_else(|error| panic!("source with inline object should lay out: {error}"));
    assert!(layout.grapheme_boundaries.contains(&6));
    assert!(
        layout.cursor_at(6, VisualAffinity::Downstream).is_some(),
        "mandatory break normalization must preserve the inline object end fence"
    );
}

#[test]
fn paragraph_and_text_island_composition_geometry_excludes_mandatory_breaks() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let style = ShapingStyle::new(1200);
    for mandatory_break in ["\n", "\u{000b}", "\u{000c}", "\u{0085}", "\r\n"] {
        let text = format!("a{mandatory_break}b");
        let paragraph = paragraph(&text, None);
        let (_, paragraph_layout) = ParagraphLayouter::new()
            .layout(
                &mut catalog,
                &paragraph,
                &settings,
                &style,
                layout_options(200.0),
            )
            .unwrap_or_else(|error| panic!("paragraph should shape {text:?}: {error}"));
        let island = ParagraphLayouter::new()
            .shape_text_island(
                &mut catalog,
                &settings,
                &[TextIslandSpan {
                    text: &text,
                    style: style.clone(),
                    underline: false,
                }],
            )
            .unwrap_or_else(|error| panic!("text island should shape {text:?}: {error}"));
        let break_range = 1..1 + mandatory_break.len();
        assert_eq!(
            paragraph_layout.composition_text_geometry(&text, break_range.clone()),
            Some(Vec::new()),
            "paragraph IME geometry should omit {mandatory_break:?}"
        );
        assert_eq!(
            island.composition_geometry(break_range),
            Some(Vec::new()),
            "math TextFlow IME geometry should omit {mandatory_break:?}"
        );
        assert!(
            paragraph_layout
                .composition_text_geometry(&text, 0..text.len())
                .is_some_and(|rectangles| !rectangles.is_empty())
        );
        assert!(
            island
                .composition_geometry(0..text.len())
                .is_some_and(|rectangles| !rectangles.is_empty())
        );
    }
    let text = "a\u{0301}b";
    let paragraph = paragraph(text, None);
    let (_, paragraph_layout) = ParagraphLayouter::new()
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &style,
            layout_options(200.0),
        )
        .unwrap_or_else(|error| panic!("combining text should shape: {error}"));
    let island = ParagraphLayouter::new()
        .shape_text_island(
            &mut catalog,
            &settings,
            &[TextIslandSpan {
                text,
                style,
                underline: false,
            }],
        )
        .unwrap_or_else(|error| panic!("combining text island should shape: {error}"));
    assert!(
        paragraph_layout
            .composition_text_geometry(text, 1..3)
            .is_some_and(|rectangles| !rectangles.is_empty()),
        "a nonempty selection inside a grapheme retains the paragraph expansion behavior"
    );
    assert!(
        island
            .composition_geometry(1..3)
            .is_some_and(|rectangles| !rectangles.is_empty()),
        "a nonempty selection inside a grapheme retains the TextFlow expansion behavior"
    );
}

#[test]
fn multiline_text_island_uses_rtl_line_geometry_and_composition_baseline() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let style = ShapingStyle::new(1200);
    let text = "a\nאבג";
    let layout = ParagraphLayouter::new()
        .shape_text_island(
            &mut catalog,
            &settings,
            &[TextIslandSpan {
                text,
                style,
                underline: false,
            }],
        )
        .unwrap_or_else(|error| panic!("multiline RTL text should shape: {error}"));
    let line = layout
        .layout
        .lines()
        .nth(1)
        .unwrap_or_else(|| panic!("second line missing"));
    let first_logical = line
        .runs()
        .flat_map(|run| run.clusters())
        .find(|cluster| cluster.text_range().start == 2)
        .unwrap_or_else(|| panic!("RTL first cluster missing"));
    let expected_x =
        f64::from(first_logical.visual_offset().unwrap_or_default() + first_logical.advance());
    let caret = layout
        .cursor_geometry_at(2, crate::VisualAffinity::Upstream, 0.0)
        .unwrap_or_else(|| panic!("RTL line start caret missing"));
    assert!((caret.x0 - expected_x).abs() < 0.01);
    assert!(caret.y0 >= f64::from(line.metrics().block_min_coord) - 0.01);
    let rectangles = layout
        .composition_geometry(2..text.len())
        .unwrap_or_else(|| panic!("second-line composition missing"));
    assert!(!rectangles.is_empty());
    assert!(rectangles.iter().all(
        |(bounds, index)| *index == 1 && bounds.y0 > f64::from(line.metrics().block_min_coord)
    ));
    let across_lines = layout
        .composition_geometry(0..text.len())
        .unwrap_or_else(|| panic!("cross-line composition missing"));
    assert!(across_lines.iter().any(|(_, index)| *index == 0));
    assert!(across_lines.iter().any(|(_, index)| *index == 1));
}

#[test]
fn generic_font_families_are_not_reported_as_missing_paragraph_requests() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    for family in ["sans-serif", "monospace", "Monospace", " monospace "] {
        let style = ShapingStyle {
            requested_family: Some(family.to_owned()),
            ..ShapingStyle::new(1200)
        };
        let mut layouter = ParagraphLayouter::new();
        let (_, result) = layouter
            .layout(
                &mut catalog,
                &paragraph("generic family", Some("en")),
                &settings,
                &style,
                layout_options(240.0),
            )
            .unwrap_or_else(|error| panic!("generic font layout failed: {error}"));
        assert!(
            !result.requested_family_missing,
            "{family} is a generic family"
        );
        let mut island_layouter = ParagraphLayouter::new();
        let island = island_layouter
            .shape_text_island(
                &mut catalog,
                &settings,
                &[TextIslandSpan {
                    text: "generic family",
                    style,
                    underline: false,
                }],
            )
            .unwrap_or_else(|error| panic!("generic font island failed: {error}"));
        assert!(
            !island.requested_family_missing,
            "{family} is generic in islands too"
        );
    }
}

#[test]
fn paragraphs_and_math_text_islands_share_missing_glyph_detection() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let style = ShapingStyle::new(1200);
    let text = "A\u{10ffff}e\u{301}";
    let mut paragraph_layouter = ParagraphLayouter::new();
    let (_, paragraph_layout) = paragraph_layouter
        .layout(
            &mut catalog,
            &paragraph(text, Some("en")),
            &settings,
            &style,
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("paragraph shaping failed: {error}"));
    let mut island_layouter = ParagraphLayouter::new();
    let island = island_layouter
        .shape_text_island(
            &mut catalog,
            &settings,
            &[TextIslandSpan {
                text,
                style,
                underline: false,
            }],
        )
        .unwrap_or_else(|error| panic!("text island shaping failed: {error}"));

    assert_eq!(paragraph_layout.has_missing_glyph, island.has_missing_glyph);
}

#[test]
fn text_island_rebuilds_font_fallback_after_catalog_refresh() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let style = ShapingStyle {
        requested_family: Some("Caret Missing Island Family B1F9".to_owned()),
        ..ShapingStyle::new(1200)
    };
    let mut layouter = ParagraphLayouter::new();
    let first = layouter
        .shape_text_island(
            &mut catalog,
            &settings,
            &[TextIslandSpan {
                text: "fallback 文本",
                style: style.clone(),
                underline: false,
            }],
        )
        .unwrap_or_else(|error| panic!("initial island fallback failed: {error}"));
    assert!(first.requested_family_missing);
    assert!(!glyph_signature(&first.layout).is_empty());

    let old_generation = catalog.generation();
    let new_generation = catalog
        .refresh()
        .unwrap_or_else(|error| panic!("font catalog refresh failed: {error}"));
    assert!(new_generation > old_generation);
    let refreshed = layouter
        .shape_text_island(
            &mut catalog,
            &settings,
            &[TextIslandSpan {
                text: "fallback 文本",
                style,
                underline: false,
            }],
        )
        .unwrap_or_else(|error| panic!("island fallback after refresh failed: {error}"));
    assert!(refreshed.requested_family_missing);
    assert_eq!(
        glyph_signature(&refreshed.layout),
        glyph_signature(&first.layout)
    );
}

#[test]
fn paragraph_layouter_rebuilds_parley_context_after_font_refresh() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let paragraph = paragraph("font cache refresh", Some("en"));
    let options = layout_options(240.0);
    layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::new(1200),
            options,
        )
        .unwrap_or_else(|error| panic!("initial Parley layout failed: {error}"));
    let previous_generation = catalog.generation();
    let refreshed_generation = catalog
        .refresh()
        .unwrap_or_else(|error| panic!("font catalog refresh failed: {error}"));
    assert!(refreshed_generation > previous_generation);
    layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::new(1200),
            options,
        )
        .unwrap_or_else(|error| panic!("Parley layout after refresh failed: {error}"));
}

#[test]
fn empty_and_trailing_break_lines_keep_positive_metrics() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let mut empty = paragraph("", Some("en"));
    empty.inlines.clear();
    let (_, empty_layout) = layouter
        .layout(
            &mut catalog,
            &empty,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("empty paragraph layout failed: {error}"));
    assert!(has_empty_line(""));
    assert_eq!(empty_layout.paragraph.lines.len(), 1);
    assert!(empty_layout.paragraph.total_height_pt > 0.0);
    let empty_cursor = empty_layout
        .cursor_at(0, crate::VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("empty paragraph start is not a valid cursor"));
    let empty_cursor_geometry = empty_layout.cursor_geometry(empty_cursor, 1.0);
    assert_eq!(empty_cursor_geometry.x0, 0.0);
    assert_eq!(
        empty_cursor_geometry.y1 - empty_cursor_geometry.y0,
        f64::from(empty_layout.paragraph.lines[0].metrics.height_pt)
    );

    let trailing = paragraph("line\n", Some("en"));
    let (text, result) = layouter
        .layout(
            &mut catalog,
            &trailing,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("trailing break layout failed: {error}"));
    assert_eq!(text, "line\n");
    assert_eq!(result.paragraph.lines.len(), 2);
    assert_eq!(
        result.paragraph.lines[1].source_bytes,
        text.len()..text.len()
    );
}

#[test]
fn paragraph_empty_line_scan_handles_split_crlf_without_copying_text() {
    let mut paragraph = paragraph("", Some("en"));
    paragraph.inlines = vec![
        text_flow(Run {
            text: "first\r".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Italic,
            definition_id: None,
            props: BTreeMap::new(),
            children: vec![text_flow(Run {
                text: "\nsecond".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        }),
    ];
    assert!(!paragraph_has_empty_line(&paragraph));
    paragraph
        .inlines
        .push(ParagraphContent::SoftBreak(caret_doc::SoftBreak {
            extra: BTreeMap::new(),
        }));
    assert!(paragraph_has_empty_line(&paragraph));
}

#[test]
fn paragraph_and_text_empty_line_scans_share_mandatory_break_semantics() {
    for mandatory_break in [
        '\n', '\r', '\u{000b}', '\u{000c}', '\u{0085}', '\u{2028}', '\u{2029}',
    ] {
        let text = format!("line{mandatory_break}");
        assert_eq!(
            paragraph_has_empty_line(&paragraph(&text, Some("en"))),
            has_empty_line(&text),
            "paragraph and text scans disagree for {mandatory_break:?}"
        );
    }
}

#[test]
fn parley_visual_cursor_moves_through_utf8_and_bidi_grapheme_boundaries() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let text = "abc אבג 中文 e\u{301} 👩‍👩‍👧‍👦";
    let paragraph_model = paragraph(text, Some("en"));
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph_model,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(320.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    let mut cursor = result
        .cursor_at(0, crate::VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("paragraph start is not a visual cursor"));
    let mut visited = Vec::new();
    for _ in 0..text.len() * 4 {
        assert!(
            result
                .grapheme_boundaries
                .binary_search(&cursor.byte_index())
                .is_ok()
        );
        if visited.contains(&(cursor.byte_index(), cursor.affinity())) {
            break;
        }
        visited.push((cursor.byte_index(), cursor.affinity()));
        let Some(next) = result.move_visual(cursor, true) else {
            break;
        };
        if next == cursor {
            break;
        }
        cursor = next;
    }
    assert!(visited.len() > 2);
}

#[test]
fn vertical_visual_movement_preserves_expected_x_and_line_edges_are_grapheme_safe() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let text = "alpha beta gamma delta";
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph(text, Some("en")),
            &settings,
            &ShapingStyle::new(1200),
            layout_options(38.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    assert!(result.paragraph.lines.len() > 1);
    let start = result
        .cursor_at(0, crate::VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("paragraph start is not a visual cursor"));

    let (next, preferred_x) = result
        .move_vertical(start, 1, Some(18.0))
        .unwrap_or_else(|| panic!("vertical movement did not reach the next line"));
    assert_eq!(preferred_x, 18.0);
    let (bounds, line_index) = result
        .cursor_geometry_with_line(next, 0.0)
        .unwrap_or_else(|| panic!("vertical target has no line geometry"));
    assert_eq!(line_index, 1);
    assert!(bounds.x0.is_finite());
    assert!(
        result
            .grapheme_boundaries
            .binary_search(&next.byte_index())
            .is_ok()
    );

    let line_end = result
        .move_to_visual_line_edge(next, true)
        .unwrap_or_else(|| panic!("visual line end is not a grapheme caret"));
    assert!(
        result
            .grapheme_boundaries
            .binary_search(&line_end.byte_index())
            .is_ok()
    );
    assert!(result.move_vertical(start, 0, None).is_none());
    assert!(result.move_vertical(start, 1, Some(f32::NAN)).is_none());
}

#[test]
fn paragraph_and_text_island_vertical_movement_share_visual_affinity() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let style = ShapingStyle::new(1200);
    let text = "ab\nab";
    let (_, paragraph_layout) = ParagraphLayouter::new()
        .layout(
            &mut catalog,
            &paragraph(text, None),
            &settings,
            &style,
            layout_options(200.0),
        )
        .unwrap_or_else(|error| panic!("paragraph should shape: {error}"));
    let island = ParagraphLayouter::new()
        .shape_text_island(
            &mut catalog,
            &settings,
            &[TextIslandSpan {
                text,
                style,
                underline: false,
            }],
        )
        .unwrap_or_else(|error| panic!("text island should shape: {error}"));
    let start = paragraph_layout
        .cursor_at(1, VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("paragraph interior should be a valid cursor"));
    let paragraph_target = paragraph_layout
        .move_vertical(start, 1, None)
        .unwrap_or_else(|| panic!("paragraph vertical movement should reach the next line"))
        .0;
    let island_target = island
        .move_vertical(start, 1, None)
        .unwrap_or_else(|| panic!("text island vertical movement should reach the next line"))
        .0;
    assert_eq!(paragraph_target, island_target);
}

#[test]
fn composition_underline_uses_each_mixed_script_run_and_wraps() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let text = "hello中文hello中文hello中文";
    for width in [240.0, 35.0] {
        let (_, layout) = layouter
            .layout(
                &mut catalog,
                &paragraph(text, Some("zh")),
                &settings,
                &ShapingStyle::new(1200),
                layout_options(width),
            )
            .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
        let rectangles = layout
            .composition_text_geometry(text, 0..text.len())
            .unwrap_or_else(|| panic!("composition geometry failed"));
        assert!(!rectangles.is_empty());
        if width < 100.0 {
            assert!(rectangles.iter().any(|(_, line_index)| *line_index > 0));
        }
        for (bounds, line_index) in rectangles {
            let line = layout
                .layout
                .get(line_index)
                .unwrap_or_else(|| panic!("underline references missing line {line_index}"));
            let matching = line.items().any(|item| match item {
                crate::PositionedGlyphLayoutItem::GlyphRun(run) => {
                    let metrics = run.run().font_metrics();
                    let expected = f64::from(run.baseline() - metrics.underline_offset)
                        - f64::from(layout.parley_line_tops_pt[line_index])
                        + f64::from(layout.baseline_adjustments_pt[line_index]);
                    bounds.x0 >= f64::from(run.offset()) - 0.001
                        && bounds.x1 <= f64::from(run.offset() + run.advance()) + 0.001
                        && (bounds.y0 - expected).abs() < 0.001
                }
                _ => false,
            });
            assert!(matching, "underline does not match a shaped font run");
            assert!(bounds.y1 > bounds.y0);
        }
    }
}

#[test]
fn composition_underline_stays_at_same_height_when_chinese_joins_line() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let preedit = "as da s";
    let texts = [preedit.to_owned(), format!("中文{preedit}")];
    let mut underline_y = Vec::new();
    for text in &texts {
        let (_, layout) = layouter
            .layout(
                &mut catalog,
                &paragraph(text, Some("zh")),
                &settings,
                &ShapingStyle::new(1200),
                layout_options(240.0),
            )
            .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
        let rectangles = layout
            .composition_text_geometry(text, text.len() - preedit.len()..text.len())
            .unwrap_or_else(|| panic!("composition geometry failed"));
        assert_eq!(rectangles.len(), 1);
        underline_y.push(rectangles[0].0.y0);
    }
    // 单位为 pt：同一字体的预编辑字形不应因同行已有中文而改变下划线高度。
    assert!((underline_y[0] - underline_y[1]).abs() < 0.001);
}

#[test]
fn persistent_underline_follows_shaped_runs_across_soft_wrap_without_changing_line_boxes() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let mut model = paragraph("underline中文underline", Some("zh-Hans"));
    let plain_model = model.clone();
    let ParagraphContent::Text(flow) = &mut model.inlines[0] else {
        unreachable!();
    };
    let run = &mut flow.runs[0];
    run.props
        .insert("underline".to_owned(), serde_json::json!(true));
    let (_, layout) = layouter
        .layout(
            &mut catalog,
            &model,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(55.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    assert!(layout.paragraph.lines.len() > 1);
    let mut rectangles = Vec::new();
    for line_index in 0..layout.layout.len() {
        rectangles.extend(layout.underline_geometry_for_line(line_index));
    }
    assert!(rectangles.len() > 1);
    assert!(
        rectangles
            .iter()
            .all(|(bounds, _)| bounds.x1 > bounds.x0 && bounds.y1 > bounds.y0)
    );
    assert!(rectangles.iter().all(|(bounds, _)| {
        bounds.y0 >= 0.0 && bounds.y0 <= f64::from(layout.paragraph.total_height_pt)
    }));

    let (_, plain_layout) = layouter
        .layout(
            &mut catalog,
            &plain_model,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(55.0),
        )
        .unwrap_or_else(|error| panic!("plain Parley layout failed: {error}"));
    assert_eq!(layout.paragraph.lines, plain_layout.paragraph.lines);
    assert_eq!(
        layout.paragraph.total_height_pt,
        plain_layout.paragraph.total_height_pt
    );
}

#[test]
fn caret_and_selection_geometry_never_split_extended_graphemes() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let text = "e\u{301} 👩‍👩‍👧‍👦";
    let paragraph_model = paragraph(text, Some("en"));
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph_model,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));

    assert!(
        result
            .cursor_at(1, crate::VisualAffinity::Downstream)
            .is_none()
    );
    assert!(
        result
            .cursor_at("e\u{301}".len(), crate::VisualAffinity::Downstream)
            .is_some()
    );
    let snapped = result
        .selection_geometry(1..2)
        .unwrap_or_else(|| panic!("valid selection was rejected"));
    let full_cluster = result
        .selection_geometry(0.."e\u{301}".len())
        .unwrap_or_else(|| panic!("valid grapheme selection was rejected"));
    assert!(!snapped.is_empty());
    assert_eq!(snapped, full_cluster);
    assert_eq!(result.selection_geometry(0..0), Some(Vec::new()));
    assert!(result.selection_geometry(1..1).is_none());

    let cjk = paragraph("你好hello", Some("zh"));
    let (_, cjk_layout) = layouter
        .layout(
            &mut catalog,
            &cjk,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(240.0),
        )
        .unwrap_or_else(|error| panic!("CJK Parley layout failed: {error}"));
    let first_cjk = cjk_layout
        .selection_geometry(0.."你".len())
        .unwrap_or_else(|| panic!("first CJK grapheme selection was rejected"));
    let second_cjk = cjk_layout
        .selection_geometry("你".len().."你好".len())
        .unwrap_or_else(|| panic!("second CJK grapheme selection was rejected"));
    assert_ne!(
        first_cjk, second_cjk,
        "adjacent CJK graphemes should have distinct selection geometry"
    );
    let line = result
        .layout
        .lines()
        .next()
        .unwrap_or_else(|| panic!("nonempty paragraph has no layout line"));
    let y = line.metrics().block_min_coord + line.metrics().line_height / 2.0;
    for x in (0..240).step_by(3) {
        let hit = result
            .cursor_from_point(x as f32, y)
            .unwrap_or_else(|| panic!("hit at x={x} did not resolve to a cursor"));
        assert!(
            result
                .grapheme_boundaries
                .binary_search(&hit.cursor.byte_index())
                .is_ok(),
            "hit returned a non-grapheme byte offset {}",
            hit.cursor.byte_index()
        );
    }
}

#[test]
fn selection_background_follows_shared_line_box_not_font_ink_bounds() {
    let mut catalog = font_catalog();
    let mut layouter = ParagraphLayouter::new();
    let settings = document_settings();
    for text in ["Latin text", "中文正文"] {
        let paragraph_model = paragraph(text, Some("en"));
        let (_, layout) = layouter
            .layout(
                &mut catalog,
                &paragraph_model,
                &settings,
                &ShapingStyle::new(1200),
                layout_options(240.0),
            )
            .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
        let line_height = f64::from(layout.paragraph.lines[0].metrics.height_pt);
        let selection = layout
            .selection_geometry(0..text.len())
            .unwrap_or_else(|| panic!("selection geometry failed for {text}"));
        assert_eq!(selection.len(), 1);
        assert!(selection[0].0.y0.abs() < 0.01);
        assert!((selection[0].0.y1 - line_height).abs() < 0.01);
    }
}

#[test]
fn exact_ltr_cluster_boundary_prefers_logical_downstream() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph("ab", Some("en")),
            &settings,
            &ShapingStyle::new(1200),
            layout_options(160.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    let upstream = result
        .cursor_at(1, crate::VisualAffinity::Upstream)
        .unwrap_or_else(|| panic!("upstream boundary cursor missing"));
    let downstream = result
        .cursor_at(1, crate::VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("downstream boundary cursor missing"));
    let upstream_rect = result.cursor_geometry(upstream, 1.0);
    let downstream_rect = result.cursor_geometry(downstream, 1.0);
    assert_eq!(upstream_rect.x0, downstream_rect.x0);
    let hit = result
        .cursor_from_point_on_line(upstream_rect.x0 as f32, 0)
        .unwrap_or_else(|| panic!("exact boundary hit did not resolve"));
    assert_eq!(hit.cursor.byte_index(), 1);
    assert_eq!(hit.cursor.affinity(), crate::VisualAffinity::Downstream);
    assert_eq!(hit.logical_affinity, crate::HitAffinity::Downstream);
}

#[test]
fn hit_testing_preserves_logical_cluster_side_for_ltr_and_rtl_at_small_sizes() {
    for font_size_centipoints in [50, 1200] {
        let mut catalog = font_catalog();
        let settings = document_settings();
        let style = ShapingStyle {
            font_size_centipoints,
            ..ShapingStyle::new(1200)
        };
        let mut layouter = ParagraphLayouter::new();
        for (text, language, expected_left, expected_right) in [
            (
                "abc",
                "en",
                crate::HitAffinity::Upstream,
                crate::HitAffinity::Downstream,
            ),
            (
                "אבג",
                "he",
                crate::HitAffinity::Downstream,
                crate::HitAffinity::Upstream,
            ),
        ] {
            let (_, result) = layouter
                .layout(
                    &mut catalog,
                    &paragraph(text, Some(language)),
                    &settings,
                    &style,
                    layout_options(160.0),
                )
                .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
            let upstream = result
                .cursor_at(2, crate::VisualAffinity::Upstream)
                .unwrap_or_else(|| panic!("upstream boundary cursor missing"));
            let downstream = result
                .cursor_at(2, crate::VisualAffinity::Downstream)
                .unwrap_or_else(|| panic!("downstream boundary cursor missing"));
            let upstream_x = result.cursor_geometry(upstream, 0.0).x0;
            let downstream_x = result.cursor_geometry(downstream, 0.0).x0;
            assert_eq!(upstream_x, downstream_x);
            let epsilon = f32::from(font_size_centipoints) * 0.0001;
            let cases = [
                ((upstream_x - f64::from(epsilon)) as f32, expected_left),
                ((upstream_x + f64::from(epsilon)) as f32, expected_right),
                (upstream_x as f32, crate::HitAffinity::Downstream),
            ];
            for (x, expected_affinity) in cases {
                let hit = result
                    .cursor_from_point_on_line(x, 0)
                    .unwrap_or_else(|| panic!("hit at x={x} did not resolve"));
                assert_eq!(
                    hit.cursor.byte_index(),
                    2,
                    "text={text:?}, size={font_size_centipoints}"
                );
                assert_eq!(
                    hit.logical_affinity, expected_affinity,
                    "text={text:?}, size={font_size_centipoints}, x={x}"
                );
            }
        }
    }
}

#[test]
fn hit_testing_keeps_visual_sides_at_bidi_boundaries() {
    for font_size_centipoints in [50, 1200] {
        let mut catalog = font_catalog();
        let settings = document_settings();
        let style = ShapingStyle {
            font_size_centipoints,
            ..ShapingStyle::new(1200)
        };
        let mut layouter = ParagraphLayouter::new();
        let (_, result) = layouter
            .layout(
                &mut catalog,
                &paragraph("aאבb", Some("en")),
                &settings,
                &style,
                layout_options(160.0),
            )
            .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
        let upstream = result
            .cursor_at(1, crate::VisualAffinity::Upstream)
            .unwrap_or_else(|| panic!("upstream bidi cursor missing"));
        let downstream = result
            .cursor_at(1, crate::VisualAffinity::Downstream)
            .unwrap_or_else(|| panic!("downstream bidi cursor missing"));
        let upstream_x = result.cursor_geometry(upstream, 0.0).x0;
        let downstream_x = result.cursor_geometry(downstream, 0.0).x0;
        assert_ne!(upstream_x, downstream_x);
        let epsilon = f32::from(font_size_centipoints) * 0.0001;
        for (x, expected_visual) in [
            (
                (upstream_x - f64::from(epsilon)) as f32,
                crate::VisualAffinity::Upstream,
            ),
            (
                (downstream_x - f64::from(epsilon)) as f32,
                crate::VisualAffinity::Downstream,
            ),
        ] {
            let hit = result
                .cursor_from_point_on_line(x, 0)
                .unwrap_or_else(|| panic!("bidi hit at x={x} did not resolve"));
            assert_eq!(hit.cursor.byte_index(), 1);
            assert_eq!(hit.cursor.affinity(), expected_visual);
        }
    }
}

#[test]
fn multiline_bidi_hit_and_cursor_geometry_stay_on_the_selected_line() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &paragraph("abc אבג def", Some("en")),
            &settings,
            &ShapingStyle::new(1200),
            layout_options(24.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    assert!(result.paragraph.lines.len() > 1);

    let hit = result
        .cursor_from_point_on_line(0.0, 1)
        .unwrap_or_else(|| panic!("second-line bidi hit did not resolve"));
    assert_eq!(hit.cursor.byte_index(), 10);
    assert_eq!(hit.logical_affinity, crate::HitAffinity::Upstream);

    let bounds = result.cursor_geometry(hit.cursor, 1.0);
    let second_line_top = f64::from(result.line_box_tops_pt[1]);
    let second_line_bottom =
        second_line_top + f64::from(result.paragraph.lines[1].metrics.height_pt);
    let cursor_mid_y = (bounds.y0 + bounds.y1) / 2.0;
    assert!(
        (second_line_top..=second_line_bottom).contains(&cursor_mid_y),
        "cursor y={cursor_mid_y} is outside second line {second_line_top}..{second_line_bottom}"
    );
}

#[test]
fn hard_line_break_caret_geometry_moves_to_the_following_or_trailing_line() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let cases = [
        ("line\n", 5, 1),
        ("a\nb", 2, 1),
        ("a\r\nb", 3, 1),
        ("\n\n", 2, 2),
        ("אבג\nדהו", 7, 1),
    ];
    for (text, offset, expected_line) in cases {
        let mut layouter = ParagraphLayouter::new();
        let (_, result) = layouter
            .layout(
                &mut catalog,
                &paragraph(text, Some("en")),
                &settings,
                &ShapingStyle::new(1200),
                layout_options(160.0),
            )
            .unwrap_or_else(|error| panic!("Parley layout failed for {text:?}: {error}"));
        assert!(result.paragraph.lines.len() > expected_line);
        let cursor = result
            .cursor_at(offset, crate::VisualAffinity::Upstream)
            .unwrap_or_else(|| panic!("cursor at {offset} missing for {text:?}"));
        let bounds = result.cursor_geometry(cursor, 1.0);
        let parley_line = result
            .layout
            .lines()
            .nth(expected_line)
            .unwrap_or_else(|| panic!("Parley line {expected_line} missing for {text:?}"));
        let parley_line_metrics = parley_line.metrics();
        let expected_x = if text.starts_with('א') {
            parley_line_metrics.offset + parley_line_metrics.advance
        } else {
            parley_line_metrics.offset
        };
        assert_eq!(bounds.x0, f64::from(expected_x), "{text:?}");
        let expected_top = f64::from(result.line_box_tops_pt[expected_line]);
        let expected_bottom =
            expected_top + f64::from(result.paragraph.lines[expected_line].metrics.height_pt);
        let cursor_mid_y = (bounds.y0 + bounds.y1) / 2.0;
        assert!(
            (expected_top..=expected_bottom).contains(&cursor_mid_y),
            "{text:?}: cursor y={cursor_mid_y} is outside expected line {expected_line} ({expected_top}..{expected_bottom})"
        );
    }
}

#[test]
fn hit_testing_snaps_combining_cluster_to_its_logical_edges() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let text = "e\u{301}";
    let mut split = paragraph("", Some("en"));
    split.inlines = vec![
        text_flow(Run {
            text: "e".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
        text_flow(Run {
            text: "\u{301}".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
    ];
    let mut layouter = ParagraphLayouter::new();
    let (_, result) = layouter
        .layout(
            &mut catalog,
            &split,
            &settings,
            &ShapingStyle {
                font_size_centipoints: 50,
                ..ShapingStyle::new(1200)
            },
            layout_options(160.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    assert_eq!(text, "e\u{301}");
    let start = result
        .cursor_at(0, crate::VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("cluster start cursor missing"));
    let end = result
        .cursor_at(text.len(), crate::VisualAffinity::Upstream)
        .unwrap_or_else(|| panic!("cluster end cursor missing"));
    let start_x = result.cursor_geometry(start, 0.0).x0;
    let end_x = result.cursor_geometry(end, 0.0).x0;
    let epsilon = (end_x - start_x).abs() * 0.1;
    let start_hit = result
        .cursor_from_point_on_line((start_x + epsilon) as f32, 0)
        .unwrap_or_else(|| panic!("cluster start hit did not resolve"));
    let end_hit = result
        .cursor_from_point_on_line((end_x - epsilon) as f32, 0)
        .unwrap_or_else(|| panic!("cluster end hit did not resolve"));
    assert_eq!(start_hit.cursor.byte_index(), 0);
    assert_eq!(start_hit.logical_affinity, crate::HitAffinity::Downstream);
    assert_eq!(end_hit.cursor.byte_index(), text.len());
    assert_eq!(end_hit.logical_affinity, crate::HitAffinity::Upstream);
}

#[test]
fn grapheme_boundaries_cross_run_and_environment_boundaries() {
    let mut catalog = font_catalog();
    let settings = document_settings();
    let mut layouter = ParagraphLayouter::new();
    let mut paragraph = paragraph("", Some("en"));
    paragraph.inlines = vec![
        text_flow(Run {
            text: "e".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: BTreeMap::new(),
            children: vec![text_flow(Run {
                text: "\u{301}".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        }),
    ];
    let (text, result) = layouter
        .layout(
            &mut catalog,
            &paragraph,
            &settings,
            &ShapingStyle::new(1200),
            layout_options(160.0),
        )
        .unwrap_or_else(|error| panic!("Parley layout failed: {error}"));
    assert_eq!(text, "e\u{301}");
    assert_eq!(result.grapheme_boundaries, vec![0, text.len()]);
    assert!(
        result
            .cursor_at(1, crate::VisualAffinity::Downstream)
            .is_none()
    );
    assert_eq!(result.paragraph.lines.len(), 1);
}

#[test]
fn unicode_17_bidi_character_corpus_checks_levels_and_visual_order() {
    let mut analyzer = Analyzer::new();
    let mut analysis = Analysis::new();
    for (line_number, line) in
        include_str!("../../../tests/fixtures/unicode-17/BidiCharacterTest.txt")
            .lines()
            .enumerate()
    {
        let Some(test_case) = line.split('#').next() else {
            continue;
        };
        if test_case.trim().is_empty() {
            continue;
        }
        let fields: Vec<_> = test_case.split(';').map(str::trim).collect();
        if fields.len() != 5 {
            panic!("invalid bidi corpus line {}", line_number + 1);
        }
        let mut text = String::new();
        for codepoint in fields[0].split_ascii_whitespace() {
            let scalar = match u32::from_str_radix(codepoint, 16)
                .ok()
                .and_then(char::from_u32)
            {
                Some(scalar) => scalar,
                None => panic!("invalid bidi codepoint on line {}", line_number + 1),
            };
            text.push(scalar);
        }
        let base_direction = match fields[1] {
            "0" => BaseDirection::Ltr,
            "1" => BaseDirection::Rtl,
            "2" => BaseDirection::Auto,
            _ => panic!("invalid bidi direction on line {}", line_number + 1),
        };
        let expected_paragraph_level = match fields[2].parse::<u8>() {
            Ok(level) => level,
            Err(_) => panic!("invalid bidi paragraph level on line {}", line_number + 1),
        };
        analyzer.analyze(
            &text,
            &AnalysisOptions {
                base_direction,
                ..AnalysisOptions::default()
            },
            &mut analysis,
        );
        assert_eq!(
            analysis.paragraph_level().to_u8(),
            expected_paragraph_level,
            "paragraph level mismatch on bidi corpus line {}",
            line_number + 1
        );
        let actual_levels: Vec<_> = if analysis.bidi_levels().is_empty() {
            vec![0; text.chars().count()]
        } else {
            analysis
                .bidi_levels()
                .iter()
                .map(|level| level.to_u8())
                .collect()
        };
        let expected_levels: Vec<_> = fields[3].split_ascii_whitespace().collect();
        assert_eq!(
            actual_levels.len(),
            expected_levels.len(),
            "bidi level count mismatch on corpus line {}",
            line_number + 1
        );
        for (actual, expected) in actual_levels.iter().zip(expected_levels.iter().copied()) {
            if expected != "x" {
                let expected = expected.parse::<u8>().unwrap_or_else(|error| {
                    panic!("invalid bidi level on line {}: {error}", line_number + 1)
                });
                assert_eq!(
                    *actual,
                    expected,
                    "resolved bidi level mismatch on corpus line {}",
                    line_number + 1
                );
            }
        }
        let mut actual_visual_order: Vec<_> = expected_levels
            .iter()
            .enumerate()
            .filter_map(|(index, expected)| (*expected != "x").then_some(index))
            .collect();
        reorder_by_bidi_levels(&mut actual_visual_order, &actual_levels);
        let expected_visual_order: Vec<_> = fields[4]
            .split_ascii_whitespace()
            .filter_map(|index| index.parse::<usize>().ok())
            .collect();
        assert_eq!(
            actual_visual_order,
            expected_visual_order,
            "bidi visual order mismatch on corpus line {}",
            line_number + 1
        );
    }
}

fn reorder_by_bidi_levels(visual_order: &mut [usize], levels: &[u8]) {
    let Some(max_level) = visual_order.iter().map(|index| levels[*index]).max() else {
        return;
    };
    let Some(min_odd_level) = visual_order
        .iter()
        .map(|index| levels[*index])
        .filter(|level| level % 2 == 1)
        .min()
    else {
        return;
    };
    for level in (min_odd_level..=max_level).rev() {
        let mut start = 0;
        while start < visual_order.len() {
            if levels[visual_order[start]] < level {
                start += 1;
                continue;
            }
            let mut end = start + 1;
            while end < visual_order.len() && levels[visual_order[end]] >= level {
                end += 1;
            }
            visual_order[start..end].reverse();
            start = end;
        }
    }
}

fn run_unicode_break_corpus(corpus: &str, grapheme: bool) {
    for (line_number, line) in corpus.lines().enumerate() {
        let Some(test_case) = line.split('#').next() else {
            continue;
        };
        if test_case.trim().is_empty() {
            continue;
        }
        let mut text = String::new();
        let mut expected = Vec::new();
        for token in test_case.split_ascii_whitespace() {
            match token {
                "÷" => expected.push(text.len()),
                "×" => {}
                codepoint => text.push(
                    char::from_u32(u32::from_str_radix(codepoint, 16).unwrap_or_else(|error| {
                        panic!(
                            "invalid scalar {codepoint:?} on line {}: {error}",
                            line_number + 1
                        )
                    }))
                    .unwrap_or_else(|| panic!("invalid scalar on line {}", line_number + 1)),
                ),
            }
        }
        if expected.first().copied() != Some(0) {
            expected.insert(0, 0);
        }
        let boundaries = UnicodeBoundaries::new(&text);
        let actual = if grapheme {
            &boundaries.grapheme
        } else {
            &boundaries.line_break
        };
        assert_eq!(
            actual,
            &expected,
            "Unicode 17 {} mismatch on line {}: {line}",
            if grapheme { "grapheme" } else { "line break" },
            line_number + 1
        );
    }
}

#[test]
fn rtl_partial_selections_keep_only_the_selected_side_of_an_inline_object() {
    // RTL 基方向下对象与两侧字符的视觉顺序由 bidi 决定；分别选中对象+逻辑前一字、
    // 对象+逻辑后一字，验证 keep_left/keep_right 的 rtl 交换只保留真正选中一侧的文本。
    let text = "א\u{fffc}ב";
    let layout = paragraph_layout_with_inline_objects(
        text,
        Some("he"),
        &[InlineObjectLayout {
            id: 42,
            source_bytes: 2..5,
            advance_pt: 30.0,
            ascent_pt: 20.0,
            descent_pt: 5.0,
        }],
    );
    let (object_x0, object_x1) = inline_object_box(&layout, 42);

    let before = layout
        .selection_text_geometry(text, 0..5)
        .unwrap_or_else(|| panic!("object plus preceding RTL text should be selectable"));
    let before_min = before
        .iter()
        .map(|(bounds, _)| bounds.x0)
        .fold(f64::INFINITY, f64::min);
    let before_max = before
        .iter()
        .map(|(bounds, _)| bounds.x1)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        before_min >= object_x0 - 0.01 && before_max > object_x1 + 0.01,
        "selection before the object must keep the selected text on its visual right: {before:?}"
    );

    let after = layout
        .selection_text_geometry(text, 2..7)
        .unwrap_or_else(|| panic!("object plus following RTL text should be selectable"));
    let after_min = after
        .iter()
        .map(|(bounds, _)| bounds.x0)
        .fold(f64::INFINITY, f64::min);
    let after_max = after
        .iter()
        .map(|(bounds, _)| bounds.x1)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        after_min < object_x0 - 0.01 && after_max <= object_x1 + 0.01,
        "selection after the object must keep the selected text on its visual left: {after:?}"
    );

    let object_only = layout
        .selection_text_geometry(text, 2..5)
        .unwrap_or_else(|| panic!("object-only RTL selection should be selectable"));
    assert!(
        object_only
            .iter()
            .all(|(bounds, _)| bounds.x0 >= object_x0 - 0.01 && bounds.x1 <= object_x1 + 0.01),
        "object-only RTL selection must not include neighbouring text: {object_only:?}"
    );
}
