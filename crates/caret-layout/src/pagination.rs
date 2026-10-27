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
mod tests;
