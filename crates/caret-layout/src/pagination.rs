//! 将段落行按页面高度切分为稳定节点 ID 与行范围引用。

use std::ops::Range;

use caret_doc::NodeId;

use crate::ParagraphLayout;

/// 参与分页的段落及其段间距策略。
#[derive(Debug, Clone, Copy)]
pub struct ParagraphPaginationInput<'a> {
    pub paragraph_id: &'a NodeId,
    pub layout: &'a ParagraphLayout,
    pub space_before_pt: f32,
    pub space_after_pt: f32,
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

    for item in items {
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

        let mut line_index = 0;
        let mut pending_space_pt =
            if current_page(&pages).is_some_and(|page| !page.fragments.is_empty()) {
                previous_space_after_pt.max(paragraph.space_before_pt)
            } else {
                0.0
            };

        while line_index < paragraph.layout.lines.len() {
            let line_height_pt = paragraph.layout.lines[line_index].metrics.height_pt;
            if !line_height_pt.is_finite() || line_height_pt <= 0.0 {
                return Err(PaginationError::InvalidLineHeight);
            }

            let Some(page) = current_page(&pages) else {
                return Err(PaginationError::InvalidLineHeight);
            };
            if !page.fragments.is_empty()
                && page.used_height_pt + pending_space_pt + line_height_pt > page_height_pt
            {
                pages.push(PageLayout::default());
                pending_space_pt = 0.0;
                continue;
            }

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

        previous_space_after_pt = paragraph.space_after_pt;
    }

    Ok(pages)
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
