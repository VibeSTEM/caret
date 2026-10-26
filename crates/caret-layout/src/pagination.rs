//! 将段落行按页面高度切分为稳定节点 ID 与行范围引用。

use std::ops::Range;

use caret_doc::NodeId;

use crate::{ParagraphLayout, ResolvedParagraphStyle};

/// 参与分页的段落及其段间距策略。
#[derive(Debug, Clone, Copy)]
pub struct ParagraphPaginationInput<'a> {
    pub paragraph_id: &'a NodeId,
    pub layout: &'a ParagraphLayout,
    pub space_before_pt: f32,
    pub space_after_pt: f32,
    pub widow_orphan_control: bool,
    pub keep_with_next: bool,
    pub keep_lines_together: bool,
    pub page_break_before: bool,
}

impl<'a> ParagraphPaginationInput<'a> {
    /// 从 `.cdoc` 的全局样式与段落覆盖解析结果构建分页输入。
    pub fn with_style(
        paragraph_id: &'a NodeId,
        layout: &'a ParagraphLayout,
        style: ResolvedParagraphStyle,
    ) -> Self {
        Self {
            paragraph_id,
            layout,
            space_before_pt: style.space_before_pt,
            space_after_pt: style.space_after_pt,
            widow_orphan_control: style.widow_orphan_control,
            keep_with_next: style.keep_with_next,
            keep_lines_together: style.keep_lines_together,
            page_break_before: style.page_break_before,
        }
    }
}

/// 按文档顺序送入分页器的段落或显式分页符。
#[derive(Debug, Clone, Copy)]
pub enum PaginationItem<'a> {
    Paragraph(ParagraphPaginationInput<'a>),
    ExplicitPageBreak,
}

/// 一段连续行在页面上的引用；正文仍留在文档模型中。
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphFragment {
    pub paragraph_id: NodeId,
    pub line_range: Range<usize>,
    pub y_offset_pt: f32,
}

/// 一个页面上的段落行片段及其推进高度。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageLayout {
    pub fragments: Vec<ParagraphFragment>,
    pub used_height_pt: f32,
}

/// 分页输入无效时拒绝产生部分结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaginationError {
    InvalidPageHeight,
    InvalidParagraphSpacing,
    InvalidLineHeight,
}

impl std::fmt::Display for PaginationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidPageHeight => "layout.invalid_page_height",
            Self::InvalidParagraphSpacing => "layout.invalid_paragraph_spacing",
            Self::InvalidLineHeight => "layout.invalid_line_height",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for PaginationError {}

/// 以行推进高度分页；跨页段落只产生节点 ID 与行范围，不复制正文。
///
/// 相邻段落间距取前段段后与后段段前的较大值。新页首段忽略段前距；高于整页的行
/// 只放置一次，允许越过版心，不制造连续空页。
pub fn paginate_paragraphs(
    items: &[PaginationItem<'_>],
    page_height_pt: f32,
) -> Result<Vec<PageLayout>, PaginationError> {
    if !page_height_pt.is_finite() || page_height_pt <= 0.0 {
        return Err(PaginationError::InvalidPageHeight);
    }

    let mut pages = vec![PageLayout::default()];
    let mut previous_space_after_pt = 0.0_f32;
    let keep_group_heights = build_keep_group_heights(items);

    for (item_index, item) in items.iter().enumerate() {
        let PaginationItem::Paragraph(paragraph) = item else {
            pages.push(PageLayout::default());
            previous_space_after_pt = 0.0;
            continue;
        };
        if !paragraph.space_before_pt.is_finite()
            || !paragraph.space_after_pt.is_finite()
            || paragraph.space_before_pt < 0.0
            || paragraph.space_after_pt < 0.0
        {
            return Err(PaginationError::InvalidParagraphSpacing);
        }
        if paragraph.layout.lines.is_empty() {
            return Err(PaginationError::InvalidLineHeight);
        }
        if paragraph
            .layout
            .lines
            .iter()
            .any(|line| !line.metrics.height_pt.is_finite() || line.metrics.height_pt <= 0.0)
        {
            return Err(PaginationError::InvalidLineHeight);
        }

        if paragraph.page_break_before
            && current_page(&pages).is_some_and(|page| !page.fragments.is_empty())
        {
            pages.push(PageLayout::default());
            previous_space_after_pt = 0.0;
        }

        let mut line_index = 0;
        let mut pending_space_pt =
            if current_page(&pages).is_some_and(|page| !page.fragments.is_empty()) {
                previous_space_after_pt.max(paragraph.space_before_pt)
            } else {
                0.0
            };

        if is_keep_group_leader(items, item_index, paragraph)
            && let Some(Some(group_height)) = keep_group_heights.get(item_index)
            && let Some(page) = current_page(&pages)
            && !page.fragments.is_empty()
            && page.used_height_pt + pending_space_pt + group_height > page_height_pt
        {
            pages.push(PageLayout::default());
            pending_space_pt = 0.0;
        }

        if paragraph.keep_lines_together {
            let paragraph_height = paragraph
                .layout
                .lines
                .iter()
                .map(|line| line.metrics.height_pt)
                .sum::<f32>();
            if paragraph_height <= page_height_pt
                && let Some(page) = current_page(&pages)
                && !page.fragments.is_empty()
                && page.used_height_pt + pending_space_pt + paragraph_height > page_height_pt
            {
                pages.push(PageLayout::default());
                pending_space_pt = 0.0;
            }
        }

        while line_index < paragraph.layout.lines.len() {
            let Some(remaining_lines) = paragraph.layout.lines.get(line_index..) else {
                return Err(PaginationError::InvalidLineHeight);
            };
            let Some(page) = current_page(&pages) else {
                return Err(PaginationError::InvalidLineHeight);
            };
            let mut fitting_lines = 0usize;
            let mut fitting_height_pt = pending_space_pt;
            for line in remaining_lines {
                if page.used_height_pt + fitting_height_pt + line.metrics.height_pt > page_height_pt
                {
                    if fitting_lines == 0 && page.fragments.is_empty() {
                        fitting_lines = 1;
                    }
                    break;
                }
                fitting_height_pt += line.metrics.height_pt;
                fitting_lines += 1;
            }
            if fitting_lines == 0 {
                pages.push(PageLayout::default());
                pending_space_pt = 0.0;
                continue;
            }

            let remaining_count = remaining_lines.len();
            let page_has_paragraph = page
                .fragments
                .last()
                .is_some_and(|fragment| fragment.paragraph_id == *paragraph.paragraph_id);
            let mut lines_to_place = fitting_lines;
            if paragraph.widow_orphan_control && fitting_lines < remaining_count {
                if fitting_lines == 1 && !page_has_paragraph && !page.fragments.is_empty() {
                    pages.push(PageLayout::default());
                    pending_space_pt = 0.0;
                    continue;
                }
                if remaining_count - fitting_lines == 1 {
                    let previous_fragment_lines = page
                        .fragments
                        .last()
                        .filter(|fragment| fragment.paragraph_id == *paragraph.paragraph_id)
                        .map_or(0, |fragment| fragment.line_range.len());
                    let candidate = fitting_lines.saturating_sub(1);
                    if candidate > 0 && previous_fragment_lines + candidate >= 2 {
                        lines_to_place = candidate;
                    } else if !page.fragments.is_empty()
                        && (!page_has_paragraph || previous_fragment_lines >= 2)
                    {
                        pages.push(PageLayout::default());
                        pending_space_pt = 0.0;
                        continue;
                    } else {
                        // 无法同时满足两端各两行时，放宽该约束并按页面容量分页。
                        lines_to_place = fitting_lines;
                    }
                }
            }

            for _ in 0..lines_to_place {
                let Some(line) = paragraph.layout.lines.get(line_index) else {
                    return Err(PaginationError::InvalidLineHeight);
                };
                let line_height_pt = line.metrics.height_pt;
                let Some(page) = pages.last_mut() else {
                    return Err(PaginationError::InvalidLineHeight);
                };
                let y_offset_pt = page.used_height_pt + pending_space_pt;
                if let Some(last) = page.fragments.last_mut()
                    && last.paragraph_id == *paragraph.paragraph_id
                    && last.line_range.end == line_index
                {
                    last.line_range.end += 1;
                } else {
                    page.fragments.push(ParagraphFragment {
                        paragraph_id: paragraph.paragraph_id.clone(),
                        line_range: line_index..line_index + 1,
                        y_offset_pt,
                    });
                }
                page.used_height_pt = y_offset_pt + line_height_pt;
                line_index += 1;
                pending_space_pt = 0.0;
            }
        }

        previous_space_after_pt = paragraph.space_after_pt;
    }

    Ok(pages)
}

fn is_keep_group_leader(
    items: &[PaginationItem<'_>],
    item_index: usize,
    paragraph: &ParagraphPaginationInput<'_>,
) -> bool {
    if paragraph.page_break_before || item_index == 0 {
        return true;
    }
    !matches!(
        items.get(item_index - 1),
        Some(PaginationItem::Paragraph(previous)) if previous.keep_with_next
    )
}

fn build_keep_group_heights(items: &[PaginationItem<'_>]) -> Vec<Option<f32>> {
    let mut heights = vec![None; items.len()];
    for index in (0..items.len()).rev() {
        let PaginationItem::Paragraph(paragraph) = items[index] else {
            continue;
        };
        if !paragraph.keep_with_next {
            continue;
        }
        let Some(PaginationItem::Paragraph(next)) = items.get(index + 1) else {
            continue;
        };
        if next.page_break_before || next.layout.lines.is_empty() {
            continue;
        }

        let mut height = paragraph
            .layout
            .lines
            .iter()
            .map(|line| line.metrics.height_pt)
            .sum::<f32>();
        if let Some(PaginationItem::Paragraph(next)) = items.get(index + 1)
            && !next.page_break_before
            && let Some(next_first_line) = next.layout.lines.first()
        {
            height += paragraph.space_after_pt.max(next.space_before_pt);
            if next.keep_with_next {
                height += heights
                    .get(index + 1)
                    .copied()
                    .flatten()
                    .unwrap_or_else(|| {
                        next.layout
                            .lines
                            .iter()
                            .map(|line| line.metrics.height_pt)
                            .sum()
                    });
            } else if next.keep_lines_together {
                height += next
                    .layout
                    .lines
                    .iter()
                    .map(|line| line.metrics.height_pt)
                    .sum::<f32>();
            } else if next.widow_orphan_control && next.layout.lines.len() > 1 {
                height +=
                    next_first_line.metrics.height_pt + next.layout.lines[1].metrics.height_pt;
            } else {
                height += next_first_line.metrics.height_pt;
            }
        }
        heights[index] = Some(height);
    }
    heights
}

fn current_page(pages: &[PageLayout]) -> Option<&PageLayout> {
    pages.last()
}

#[cfg(test)]
mod tests {
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
    fn next_paragraph_starts_new_page_when_spacing_and_line_do_not_fit() {
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
        )
        .unwrap_or_else(|error| panic!("valid paragraphs rejected: {error}"));

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].fragments[0].y_offset_pt, 0.0);
        assert_eq!(pages[1].fragments[0].y_offset_pt, 0.0);
    }

    #[test]
    fn adjacent_spacing_uses_larger_before_or_after_value() {
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
        )
        .unwrap_or_else(|error| panic!("valid paragraphs rejected: {error}"));

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].fragments[1].y_offset_pt, 38.0);
    }

    #[test]
    fn resolved_default_style_supplies_document_spacing_to_pagination() {
        let first_id = NodeId::generate();
        let second_id = NodeId::generate();
        let first = paragraph(&[12.0]);
        let second = paragraph(&[12.0]);
        let style = crate::resolve_paragraph_style(&caret_doc::DocumentSettings::default(), None)
            .unwrap_or_else(|error| panic!("valid default paragraph style rejected: {error}"));
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
        )
        .unwrap_or_else(|error| panic!("valid paragraphs rejected: {error}"));

        assert_eq!(pages[0].fragments[1].y_offset_pt, 22.0);
    }

    #[test]
    fn explicit_page_break_events_preserve_leading_consecutive_and_trailing_empty_pages() {
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
        )
        .unwrap_or_else(|error| panic!("valid paragraphs rejected: {error}"));

        assert_eq!(pages.len(), 5);
        assert!(pages[0].fragments.is_empty());
        assert!(pages[1].fragments.is_empty());
        assert_eq!(pages[2].fragments[0].paragraph_id, first_id);
        assert_eq!(pages[3].fragments[0].paragraph_id, second_id);
        assert!(pages[4].fragments.is_empty());
    }

    #[test]
    fn a_tall_line_is_placed_once_without_empty_pages() {
        let id = NodeId::generate();
        let layout = paragraph(&[120.0]);
        let pages = paginate_paragraphs(&[input(&id, &layout, 0.0, 0.0)], 50.0)
            .unwrap_or_else(|error| panic!("valid paragraph rejected: {error}"));

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].used_height_pt, 120.0);
        assert_eq!(pages[0].fragments[0].line_range, Range { start: 0, end: 1 });
    }

    #[test]
    fn paragraph_fragments_preserve_stable_id_and_line_ranges() {
        let id = NodeId::generate();
        let layout = paragraph(&[30.0, 30.0, 30.0]);
        let pages = paginate_paragraphs(&[input(&id, &layout, 0.0, 0.0)], 50.0)
            .unwrap_or_else(|error| panic!("valid paragraph rejected: {error}"));

        assert_eq!(pages.len(), 3);
        for (index, page) in pages.iter().enumerate() {
            assert_eq!(page.fragments[0].paragraph_id, id);
            assert_eq!(page.fragments[0].line_range, index..index + 1);
        }
    }

    #[test]
    fn widow_orphan_control_keeps_two_lines_on_each_page() {
        let id = NodeId::generate();
        let layout = paragraph(&[20.0, 20.0, 20.0, 20.0]);
        let mut item = input(&id, &layout, 0.0, 0.0);
        if let PaginationItem::Paragraph(paragraph) = &mut item {
            paragraph.widow_orphan_control = true;
        }
        let pages = paginate_paragraphs(&[item], 50.0)
            .unwrap_or_else(|error| panic!("valid paragraph rejected: {error}"));

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].fragments[0].line_range, 0..2);
        assert_eq!(pages[1].fragments[0].line_range, 2..4);
    }

    #[test]
    fn widow_control_always_advances_when_a_page_has_only_one_line_of_capacity() {
        let id = NodeId::generate();
        let layout = paragraph(&[20.0; 5]);
        let mut item = input(&id, &layout, 0.0, 0.0);
        if let PaginationItem::Paragraph(paragraph) = &mut item {
            paragraph.widow_orphan_control = true;
        }
        let pages = paginate_paragraphs(&[item], 20.0)
            .unwrap_or_else(|error| panic!("valid paragraph rejected: {error}"));

        assert_eq!(pages.len(), 5);
        assert_eq!(pages[0].fragments[0].line_range, 0..1);
        assert_eq!(pages[1].fragments[0].line_range, 1..2);
        assert_eq!(pages[2].fragments[0].line_range, 2..3);
        assert_eq!(pages[3].fragments[0].line_range, 3..4);
        assert_eq!(pages[4].fragments[0].line_range, 4..5);
    }

    #[test]
    fn impossible_widow_constraints_do_not_overflow_page_fitting_lines() {
        let id = NodeId::generate();
        let three_lines = paragraph(&[30.0; 3]);
        let mut item = input(&id, &three_lines, 0.0, 0.0);
        if let PaginationItem::Paragraph(paragraph) = &mut item {
            paragraph.widow_orphan_control = true;
        }
        let pages = paginate_paragraphs(&[item], 60.0)
            .unwrap_or_else(|error| panic!("valid paragraph rejected: {error}"));

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].used_height_pt, 60.0);
        assert_eq!(pages[0].fragments[0].line_range, 0..2);
        assert_eq!(pages[1].used_height_pt, 30.0);
        assert_eq!(pages[1].fragments[0].line_range, 2..3);
    }

    #[test]
    fn widow_control_does_not_loop_when_one_line_remains_after_a_split() {
        let id = NodeId::generate();
        let layout = paragraph(&[20.0; 5]);
        let mut item = input(&id, &layout, 0.0, 0.0);
        if let PaginationItem::Paragraph(paragraph) = &mut item {
            paragraph.widow_orphan_control = true;
        }
        let pages = paginate_paragraphs(&[item], 80.0)
            .unwrap_or_else(|error| panic!("valid paragraph rejected: {error}"));

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
    }

    #[test]
    fn keep_lines_together_moves_a_fitting_paragraph_to_the_next_page() {
        let first_id = NodeId::generate();
        let second_id = NodeId::generate();
        let first = paragraph(&[20.0]);
        let second = paragraph(&[10.0, 10.0]);
        let mut second_item = input(&second_id, &second, 0.0, 0.0);
        if let PaginationItem::Paragraph(paragraph) = &mut second_item {
            paragraph.keep_lines_together = true;
        }
        let pages = paginate_paragraphs(&[input(&first_id, &first, 0.0, 0.0), second_item], 35.0)
            .unwrap_or_else(|error| panic!("valid paragraphs rejected: {error}"));

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].fragments[0].paragraph_id, second_id);
        assert_eq!(pages[1].fragments[0].line_range, 0..2);
    }

    #[test]
    fn keep_with_next_moves_a_short_heading_with_the_next_paragraph() {
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
        )
        .unwrap_or_else(|error| panic!("valid paragraphs rejected: {error}"));

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].fragments[0].paragraph_id, heading_id);
        assert_eq!(pages[1].fragments[1].paragraph_id, body_id);
        assert_eq!(pages[1].fragments[1].line_range, 0..2);
    }

    #[test]
    fn chained_keep_with_next_paragraphs_stay_together() {
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
        )
        .unwrap_or_else(|error| panic!("valid paragraphs rejected: {error}"));

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].fragments.len(), 3);
        assert_eq!(pages[1].fragments[0].paragraph_id, first_heading_id);
        assert_eq!(pages[1].fragments[1].paragraph_id, second_heading_id);
        assert_eq!(pages[1].fragments[2].paragraph_id, body_id);
    }

    #[test]
    fn oversized_keep_group_starts_on_a_fresh_page_without_repeated_breaks() {
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
        )
        .unwrap_or_else(|error| panic!("valid paragraphs rejected: {error}"));

        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0].fragments[0].paragraph_id, previous_id);
        assert_eq!(pages[1].fragments[0].paragraph_id, heading_id);
        assert_eq!(pages[1].fragments[1].paragraph_id, body_id);
        assert_eq!(pages[1].fragments[1].line_range, 0..2);
        assert_eq!(pages[2].fragments[0].paragraph_id, body_id);
        assert_eq!(pages[2].fragments[0].line_range, 2..3);
    }

    #[test]
    fn keep_with_next_without_a_successor_does_not_keep_the_paragraph_together() {
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
        )
        .unwrap_or_else(|error| panic!("valid paragraphs rejected: {error}"));

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].fragments[1].line_range, 0..2);
        assert_eq!(pages[1].fragments[0].line_range, 2..4);
    }

    #[test]
    fn page_break_before_starts_a_new_page_after_existing_content() {
        let first_id = NodeId::generate();
        let second_id = NodeId::generate();
        let first = paragraph(&[10.0]);
        let second = paragraph(&[10.0]);
        let mut second_item = input(&second_id, &second, 0.0, 0.0);
        if let PaginationItem::Paragraph(paragraph) = &mut second_item {
            paragraph.page_break_before = true;
        }
        let pages = paginate_paragraphs(&[input(&first_id, &first, 0.0, 0.0), second_item], 100.0)
            .unwrap_or_else(|error| panic!("valid paragraphs rejected: {error}"));

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].fragments[0].paragraph_id, first_id);
        assert_eq!(pages[1].fragments[0].paragraph_id, second_id);
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
}
