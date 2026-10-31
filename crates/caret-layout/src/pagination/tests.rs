//! 验证分页、段落约束和页面推进规则。

use super::{PaginationError, PaginationItem, ParagraphPaginationInput, paginate_paragraphs};
use crate::{LineMetrics, ParagraphLayout, ParagraphLine};
use caret_doc::NodeId;
use std::ops::Range;

fn paragraph(heights: &[f32]) -> ParagraphLayout {
    ParagraphLayout {
        lines: heights
            .iter()
            .enumerate()
            .map(|(index, height)| ParagraphLine {
                source_bytes: index..index + 1,
                consumed_bytes: index..index + 1,
                advance_pt: 1.0,
                overflows_width: false,
                metrics: LineMetrics {
                    height_pt: *height,
                    baseline_pt: *height,
                    content_metrics_overflow_line_box: false,
                    object_ink_overflows_line_box: false,
                    clip_ink_to_line_height: false,
                    caret_top_pt: 0.0,
                    caret_height_pt: *height,
                },
            })
            .collect(),
        total_height_pt: heights.iter().sum(),
    }
}

fn input<'a>(
    id: &'a NodeId,
    layout: &'a ParagraphLayout,
    before: f32,
    after: f32,
) -> PaginationItem<'a> {
    PaginationItem::Paragraph(ParagraphPaginationInput {
        paragraph_id: id,
        layout,
        space_before_pt: before,
        space_after_pt: after,
        widow_orphan_control: false,
        keep_with_next: false,
        keep_lines_together: false,
        page_break_before: false,
    })
}

#[test]
fn next_paragraph_starts_new_page_when_spacing_and_line_do_not_fit()
-> Result<(), Box<dyn std::error::Error>> {
    let first_id = NodeId::generate();
    let second_id = NodeId::generate();
    let first = paragraph(&[30.0]);
    let second = paragraph(&[30.0]);
    let pages = paginate_paragraphs(
        &[
            input(&first_id, &first, 0.0, 8.0),
            input(&second_id, &second, 5.0, 0.0),
        ],
        50.0,
    )?;

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].fragments[0].y_offset_pt, 0.0);
    assert_eq!(pages[1].fragments[0].y_offset_pt, 0.0);
    Ok(())
}

#[test]
fn adjacent_spacing_uses_larger_before_or_after_value() -> Result<(), Box<dyn std::error::Error>> {
    let first_id = NodeId::generate();
    let second_id = NodeId::generate();
    let first = paragraph(&[30.0]);
    let second = paragraph(&[30.0]);
    let pages = paginate_paragraphs(
        &[
            input(&first_id, &first, 0.0, 8.0),
            input(&second_id, &second, 5.0, 0.0),
        ],
        100.0,
    )?;

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].fragments[1].y_offset_pt, 38.0);
    Ok(())
}

#[test]
fn soft_wrapped_lines_do_not_repeat_paragraph_spacing() -> Result<(), Box<dyn std::error::Error>> {
    let first_id = NodeId::generate();
    let second_id = NodeId::generate();
    let first = paragraph(&[10.0, 10.0, 10.0]);
    let second = paragraph(&[10.0]);
    let pages = paginate_paragraphs(
        &[
            input(&first_id, &first, 0.0, 5.0),
            input(&second_id, &second, 7.0, 0.0),
        ],
        100.0,
    )?;

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].fragments[0].line_range, 0..3);
    assert_eq!(pages[0].fragments[1].y_offset_pt, 37.0);
    Ok(())
}

#[test]
fn resolved_default_style_supplies_document_spacing_to_pagination()
-> Result<(), Box<dyn std::error::Error>> {
    let first_id = NodeId::generate();
    let second_id = NodeId::generate();
    let first = paragraph(&[12.0]);
    let second = paragraph(&[12.0]);
    let style = crate::resolve_paragraph_style(&caret_doc::DocumentSettings::default(), None)?;
    let pages = paginate_paragraphs(
        &[
            PaginationItem::Paragraph(ParagraphPaginationInput::with_style(
                &first_id, &first, style,
            )),
            PaginationItem::Paragraph(ParagraphPaginationInput::with_style(
                &second_id, &second, style,
            )),
        ],
        100.0,
    )?;

    assert_eq!(pages[0].fragments[1].y_offset_pt, 20.0);
    Ok(())
}

#[test]
fn explicit_page_break_events_preserve_leading_consecutive_and_trailing_empty_pages()
-> Result<(), Box<dyn std::error::Error>> {
    let first_id = NodeId::generate();
    let second_id = NodeId::generate();
    let first = paragraph(&[20.0]);
    let second = paragraph(&[20.0]);
    let pages = paginate_paragraphs(
        &[
            PaginationItem::ExplicitPageBreak,
            PaginationItem::ExplicitPageBreak,
            input(&first_id, &first, 0.0, 0.0),
            PaginationItem::ExplicitPageBreak,
            input(&second_id, &second, 0.0, 0.0),
            PaginationItem::ExplicitPageBreak,
        ],
        100.0,
    )?;

    assert_eq!(pages.len(), 5);
    assert!(pages[0].fragments.is_empty());
    assert!(pages[1].fragments.is_empty());
    assert_eq!(pages[2].fragments[0].paragraph_id, first_id);
    assert_eq!(pages[3].fragments[0].paragraph_id, second_id);
    assert!(pages[4].fragments.is_empty());
    Ok(())
}

#[test]
fn a_tall_line_is_placed_once_without_empty_pages() -> Result<(), Box<dyn std::error::Error>> {
    let id = NodeId::generate();
    let layout = paragraph(&[120.0]);
    let pages = paginate_paragraphs(&[input(&id, &layout, 0.0, 0.0)], 50.0)?;

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].used_height_pt, 120.0);
    assert_eq!(pages[0].fragments[0].line_range, Range { start: 0, end: 1 });
    Ok(())
}

#[test]
fn paragraph_fragments_preserve_stable_id_and_line_ranges() -> Result<(), Box<dyn std::error::Error>>
{
    let id = NodeId::generate();
    let layout = paragraph(&[30.0, 30.0, 30.0]);
    let pages = paginate_paragraphs(&[input(&id, &layout, 0.0, 0.0)], 50.0)?;

    assert_eq!(pages.len(), 3);
    for (index, page) in pages.iter().enumerate() {
        assert_eq!(page.fragments[0].paragraph_id, id);
        assert_eq!(page.fragments[0].line_range, index..index + 1);
    }
    Ok(())
}

#[test]
fn widow_orphan_control_keeps_two_lines_on_each_page() -> Result<(), Box<dyn std::error::Error>> {
    let id = NodeId::generate();
    let layout = paragraph(&[20.0, 20.0, 20.0, 20.0]);
    let mut item = input(&id, &layout, 0.0, 0.0);
    if let PaginationItem::Paragraph(paragraph) = &mut item {
        paragraph.widow_orphan_control = true;
    }
    let pages = paginate_paragraphs(&[item], 50.0)?;

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].fragments[0].line_range, 0..2);
    assert_eq!(pages[1].fragments[0].line_range, 2..4);
    Ok(())
}

#[test]
fn widow_control_always_advances_when_a_page_has_only_one_line_of_capacity()
-> Result<(), Box<dyn std::error::Error>> {
    let id = NodeId::generate();
    let layout = paragraph(&[20.0; 5]);
    let mut item = input(&id, &layout, 0.0, 0.0);
    if let PaginationItem::Paragraph(paragraph) = &mut item {
        paragraph.widow_orphan_control = true;
    }
    let pages = paginate_paragraphs(&[item], 20.0)?;

    assert_eq!(pages.len(), 5);
    assert_eq!(pages[0].fragments[0].line_range, 0..1);
    assert_eq!(pages[1].fragments[0].line_range, 1..2);
    assert_eq!(pages[2].fragments[0].line_range, 2..3);
    assert_eq!(pages[3].fragments[0].line_range, 3..4);
    assert_eq!(pages[4].fragments[0].line_range, 4..5);
    Ok(())
}

#[test]
fn impossible_widow_constraints_do_not_overflow_page_fitting_lines()
-> Result<(), Box<dyn std::error::Error>> {
    let id = NodeId::generate();
    let three_lines = paragraph(&[30.0; 3]);
    let mut item = input(&id, &three_lines, 0.0, 0.0);
    if let PaginationItem::Paragraph(paragraph) = &mut item {
        paragraph.widow_orphan_control = true;
    }
    let pages = paginate_paragraphs(&[item], 60.0)?;

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].used_height_pt, 60.0);
    assert_eq!(pages[0].fragments[0].line_range, 0..2);
    assert_eq!(pages[1].used_height_pt, 30.0);
    assert_eq!(pages[1].fragments[0].line_range, 2..3);
    Ok(())
}

#[test]
fn widow_control_does_not_loop_when_one_line_remains_after_a_split()
-> Result<(), Box<dyn std::error::Error>> {
    let id = NodeId::generate();
    let layout = paragraph(&[20.0; 5]);
    let mut item = input(&id, &layout, 0.0, 0.0);
    if let PaginationItem::Paragraph(paragraph) = &mut item {
        paragraph.widow_orphan_control = true;
    }
    let pages = paginate_paragraphs(&[item], 80.0)?;

    let mut ranges = pages
        .iter()
        .flat_map(|page| {
            page.fragments
                .iter()
                .map(|fragment| fragment.line_range.clone())
        })
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);
    assert_eq!(ranges, vec![0..3, 3..5]);
    Ok(())
}

#[test]
fn keep_lines_together_moves_a_fitting_paragraph_to_the_next_page()
-> Result<(), Box<dyn std::error::Error>> {
    let first_id = NodeId::generate();
    let second_id = NodeId::generate();
    let first = paragraph(&[20.0]);
    let second = paragraph(&[10.0, 10.0]);
    let mut second_item = input(&second_id, &second, 0.0, 0.0);
    if let PaginationItem::Paragraph(paragraph) = &mut second_item {
        paragraph.keep_lines_together = true;
    }
    let pages = paginate_paragraphs(&[input(&first_id, &first, 0.0, 0.0), second_item], 35.0)?;

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[1].fragments[0].paragraph_id, second_id);
    assert_eq!(pages[1].fragments[0].line_range, 0..2);
    Ok(())
}

#[test]
fn keep_with_next_moves_a_short_heading_with_the_next_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let first_id = NodeId::generate();
    let heading_id = NodeId::generate();
    let body_id = NodeId::generate();
    let first = paragraph(&[20.0]);
    let heading = paragraph(&[20.0]);
    let body = paragraph(&[20.0, 20.0]);
    let mut heading_item = input(&heading_id, &heading, 0.0, 0.0);
    if let PaginationItem::Paragraph(paragraph) = &mut heading_item {
        paragraph.keep_with_next = true;
    }
    let mut body_item = input(&body_id, &body, 0.0, 0.0);
    if let PaginationItem::Paragraph(paragraph) = &mut body_item {
        paragraph.widow_orphan_control = true;
    }
    let pages = paginate_paragraphs(
        &[input(&first_id, &first, 0.0, 0.0), heading_item, body_item],
        70.0,
    )?;

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[1].fragments[0].paragraph_id, heading_id);
    assert_eq!(pages[1].fragments[1].paragraph_id, body_id);
    assert_eq!(pages[1].fragments[1].line_range, 0..2);
    Ok(())
}

#[test]
fn chained_keep_with_next_paragraphs_stay_together() -> Result<(), Box<dyn std::error::Error>> {
    let previous_id = NodeId::generate();
    let first_heading_id = NodeId::generate();
    let second_heading_id = NodeId::generate();
    let body_id = NodeId::generate();
    let previous = paragraph(&[25.0]);
    let first_heading = paragraph(&[20.0]);
    let second_heading = paragraph(&[20.0]);
    let body = paragraph(&[20.0]);
    let mut first_heading_item = input(&first_heading_id, &first_heading, 0.0, 0.0);
    let mut second_heading_item = input(&second_heading_id, &second_heading, 0.0, 0.0);
    if let PaginationItem::Paragraph(paragraph) = &mut first_heading_item {
        paragraph.keep_with_next = true;
    }
    if let PaginationItem::Paragraph(paragraph) = &mut second_heading_item {
        paragraph.keep_with_next = true;
    }
    let pages = paginate_paragraphs(
        &[
            input(&previous_id, &previous, 0.0, 0.0),
            first_heading_item,
            second_heading_item,
            input(&body_id, &body, 0.0, 0.0),
        ],
        70.0,
    )?;

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[1].fragments.len(), 3);
    assert_eq!(pages[1].fragments[0].paragraph_id, first_heading_id);
    assert_eq!(pages[1].fragments[1].paragraph_id, second_heading_id);
    assert_eq!(pages[1].fragments[2].paragraph_id, body_id);
    Ok(())
}

#[test]
fn oversized_keep_group_starts_on_a_fresh_page_without_repeated_breaks()
-> Result<(), Box<dyn std::error::Error>> {
    let previous_id = NodeId::generate();
    let heading_id = NodeId::generate();
    let body_id = NodeId::generate();
    let previous = paragraph(&[50.0]);
    let heading = paragraph(&[20.0]);
    let body = paragraph(&[40.0, 40.0, 40.0]);
    let mut heading_item = input(&heading_id, &heading, 0.0, 0.0);
    let mut body_item = input(&body_id, &body, 0.0, 0.0);
    if let PaginationItem::Paragraph(paragraph) = &mut heading_item {
        paragraph.keep_with_next = true;
    }
    if let PaginationItem::Paragraph(paragraph) = &mut body_item {
        paragraph.keep_lines_together = true;
    }
    let pages = paginate_paragraphs(
        &[
            input(&previous_id, &previous, 0.0, 0.0),
            heading_item,
            body_item,
        ],
        100.0,
    )?;

    assert_eq!(pages.len(), 3);
    assert_eq!(pages[0].fragments[0].paragraph_id, previous_id);
    assert_eq!(pages[1].fragments[0].paragraph_id, heading_id);
    assert_eq!(pages[1].fragments[1].paragraph_id, body_id);
    assert_eq!(pages[1].fragments[1].line_range, 0..2);
    assert_eq!(pages[2].fragments[0].paragraph_id, body_id);
    assert_eq!(pages[2].fragments[0].line_range, 2..3);
    Ok(())
}

#[test]
fn keep_with_next_without_a_successor_does_not_keep_the_paragraph_together()
-> Result<(), Box<dyn std::error::Error>> {
    let previous_id = NodeId::generate();
    let final_id = NodeId::generate();
    let previous = paragraph(&[30.0]);
    let final_paragraph = paragraph(&[20.0; 4]);
    let mut final_item = input(&final_id, &final_paragraph, 0.0, 0.0);
    if let PaginationItem::Paragraph(paragraph) = &mut final_item {
        paragraph.keep_with_next = true;
    }
    let pages = paginate_paragraphs(
        &[input(&previous_id, &previous, 0.0, 0.0), final_item],
        80.0,
    )?;

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].fragments[1].line_range, 0..2);
    assert_eq!(pages[1].fragments[0].line_range, 2..4);
    Ok(())
}

#[test]
fn page_break_before_starts_a_new_page_after_existing_content()
-> Result<(), Box<dyn std::error::Error>> {
    let first_id = NodeId::generate();
    let second_id = NodeId::generate();
    let first = paragraph(&[10.0]);
    let second = paragraph(&[10.0]);
    let mut second_item = input(&second_id, &second, 0.0, 0.0);
    if let PaginationItem::Paragraph(paragraph) = &mut second_item {
        paragraph.page_break_before = true;
    }
    let pages = paginate_paragraphs(&[input(&first_id, &first, 0.0, 0.0), second_item], 100.0)?;

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].fragments[0].paragraph_id, first_id);
    assert_eq!(pages[1].fragments[0].paragraph_id, second_id);
    Ok(())
}

#[test]
fn rejects_invalid_page_geometry_and_spacing() {
    let id = NodeId::generate();
    let layout = paragraph(&[10.0]);
    assert_eq!(
        paginate_paragraphs(&[input(&id, &layout, 0.0, 0.0)], f32::INFINITY),
        Err(PaginationError::InvalidPageHeight)
    );
    assert_eq!(
        paginate_paragraphs(&[input(&id, &layout, -1.0, 0.0)], 100.0),
        Err(PaginationError::InvalidParagraphSpacing)
    );
}

#[test]
fn malformed_paragraph_lines_do_not_produce_partial_pages() {
    let id = NodeId::generate();
    let valid = paragraph(&[10.0]);
    for invalid in [paragraph(&[]), paragraph(&[0.0]), paragraph(&[f32::NAN])] {
        let Err(error) = paginate_paragraphs(
            &[input(&id, &valid, 0.0, 0.0), input(&id, &invalid, 0.0, 0.0)],
            100.0,
        ) else {
            panic!("invalid paragraph must reject the complete layout");
        };
        assert_eq!(error, PaginationError::InvalidLineHeight);
        assert_eq!(error.to_string(), "layout.invalid_line_height");
    }
}
