//! 组合文档、字体塑形、段落行布局与分页，生成供视图消费的只读快照。

use crate::{Affinity, Pos, Selection, Session, StoryId};
use caret_doc::{Block, Document, NodeId, Paragraph};
use caret_layout::{
    FontCatalog, PageLayout, PaginationItem, ParagraphLayoutOptions, ParagraphLayouter,
    ParagraphPaginationInput, ParleyParagraphLayout, ResolvedDocumentTextSettings, ShapingStyle,
    VisualAffinity, paginate_paragraphs, paragraph_has_empty_line, resolve_paragraph_style,
};
use std::collections::HashMap;

/// 文档布局失败时返回稳定的错误类别；不产生部分快照。
#[derive(Debug)]
pub enum DocumentLayoutError {
    InvalidDocument(String),
    FontPreferences(String),
    Shaping(String),
    Paragraph(String),
    Pagination(String),
    UnknownBreakKind(String),
    UnsupportedContent,
    UnsupportedEnvironmentSemantics,
    UnsupportedParagraphMarkStyle,
}

impl std::fmt::Display for DocumentLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDocument(error) => write!(formatter, "layout.invalid_document: {error}"),
            Self::FontPreferences(error) => write!(formatter, "layout.font_preferences: {error}"),
            Self::Shaping(error) => write!(formatter, "layout.shaping: {error}"),
            Self::Paragraph(error) => write!(formatter, "layout.paragraph: {error}"),
            Self::Pagination(error) => write!(formatter, "layout.pagination: {error}"),
            Self::UnknownBreakKind(kind) => {
                write!(formatter, "layout.unknown_break_kind: {kind}")
            }
            Self::UnsupportedContent => formatter.write_str("layout.unsupported_content"),
            Self::UnsupportedEnvironmentSemantics => {
                formatter.write_str("layout.unsupported_environment_semantics")
            }
            Self::UnsupportedParagraphMarkStyle => {
                formatter.write_str("layout.unsupported_paragraph_mark_style")
            }
        }
    }
}

impl std::error::Error for DocumentLayoutError {}

/// 已排版段落的源 ID、原文與行框；正文文本只在快照里保留一份。
#[derive(Debug, Clone)]
pub struct LaidOutParagraph {
    pub id: NodeId,
    pub text: String,
    pub layout: ParleyParagraphLayout,
}

/// 一页的文档坐标和段落行片段。
#[derive(Debug, Clone, PartialEq)]
pub struct PageLayoutSnapshot {
    pub width_pt: f64,
    pub height_pt: f64,
    pub margin_left_pt: f64,
    pub margin_top_pt: f64,
    pub layout: PageLayout,
}

/// 单代文档布局结果。页面和段落位置共用同一分页输出。
#[derive(Debug, Clone)]
pub struct DocumentLayoutSnapshot {
    pub session_id: Option<NodeId>,
    pub document_generation: u64,
    pub font_generation: u64,
    pub paragraphs: Vec<LaidOutParagraph>,
    pub pages: Vec<PageLayoutSnapshot>,
    paragraph_lookup: HashMap<NodeId, usize>,
}

/// 页面局部坐标中的选区矩形；一份跨页选择可产生多页矩形。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSelectionRect {
    pub page_index: usize,
    pub x0_pt: f64,
    pub y0_pt: f64,
    pub x1_pt: f64,
    pub y1_pt: f64,
}

/// 页面命中结果；文档环境亲和性与视觉光标侧分别记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageHit {
    pub position: Pos,
    pub visual_affinity: VisualAffinity,
}

impl DocumentLayoutSnapshot {
    /// 判断此布局结果是否仍对应当前文档与字体输入。
    ///
    /// 异步调用方应在安装布局前调用，避免已替换的会话、已编辑的文档或已刷新的字体目录发布过期几何。
    pub fn is_current_for(&self, session: &Session, font_generation: u64) -> bool {
        self.session_id.as_ref() == Some(session.session_id())
            && self.document_generation == session.document_generation()
            && self.font_generation == font_generation
    }

    /// 将页面局部的 pt 坐标映射到最近的有效逻辑文本位置。
    pub fn hit_test(&self, page_index: usize, x_pt: f64, y_pt: f64) -> Option<PageHit> {
        if !x_pt.is_finite() || !y_pt.is_finite() {
            return None;
        }
        let page = self.pages.get(page_index)?;
        if x_pt < 0.0 || y_pt < 0.0 || x_pt > page.width_pt || y_pt > page.height_pt {
            return None;
        }
        let content_y = y_pt - page.margin_top_pt;
        if content_y < 0.0 {
            return None;
        }

        let fragment_index = page
            .layout
            .fragments
            .partition_point(|fragment| f64::from(fragment.y_offset_pt) <= content_y)
            .checked_sub(1)?;
        let fragment = page.layout.fragments.get(fragment_index)?;
        let paragraph_index = *self.paragraph_lookup.get(&fragment.paragraph_id)?;
        let paragraph = self.paragraphs.get(paragraph_index)?;
        let mut fragment_line_top = f64::from(fragment.y_offset_pt);
        for line_index in fragment.line_range.clone() {
            let line = paragraph.layout.paragraph.lines.get(line_index)?;
            let line_bottom = fragment_line_top + f64::from(line.metrics.height_pt);
            if content_y < line_bottom
                || (line_index + 1 == fragment.line_range.end && content_y <= line_bottom)
            {
                let cursor = paragraph
                    .layout
                    .cursor_from_point_on_line((x_pt - page.margin_left_pt) as f32, line_index)?;
                return Some(PageHit {
                    position: Pos {
                        story: StoryId::Body,
                        paragraph_id: paragraph.id.clone(),
                        offset: cursor.cursor.byte_index(),
                        affinity: match cursor.logical_affinity {
                            caret_layout::HitAffinity::Upstream => Affinity::Upstream,
                            caret_layout::HitAffinity::Downstream => Affinity::Downstream,
                        },
                    },
                    visual_affinity: cursor.cursor.affinity(),
                });
            }
            fragment_line_top = line_bottom;
        }
        None
    }

    /// 返回裁切到可见分页片段内的选区矩形。
    pub fn selection_geometry(&self, selection: &Selection) -> Option<Vec<PageSelectionRect>> {
        if selection.anchor.story != StoryId::Body || selection.focus.story != StoryId::Body {
            return None;
        }
        let anchor_index = *self.paragraph_lookup.get(&selection.anchor.paragraph_id)?;
        let focus_index = *self.paragraph_lookup.get(&selection.focus.paragraph_id)?;
        let anchor_paragraph = self.paragraphs.get(anchor_index)?;
        let focus_paragraph = self.paragraphs.get(focus_index)?;
        if selection.anchor.offset > anchor_paragraph.text.len()
            || !anchor_paragraph
                .text
                .is_char_boundary(selection.anchor.offset)
            || selection.focus.offset > focus_paragraph.text.len()
            || !focus_paragraph
                .text
                .is_char_boundary(selection.focus.offset)
        {
            return None;
        }
        let (start, start_paragraph, end, end_paragraph) =
            if (anchor_index, selection.anchor.offset) <= (focus_index, selection.focus.offset) {
                (
                    &selection.anchor,
                    anchor_index,
                    &selection.focus,
                    focus_index,
                )
            } else {
                (
                    &selection.focus,
                    focus_index,
                    &selection.anchor,
                    anchor_index,
                )
            };
        let mut rectangles = Vec::new();

        for (page_index, page) in self.pages.iter().enumerate() {
            for fragment in &page.layout.fragments {
                let &paragraph_index = self.paragraph_lookup.get(&fragment.paragraph_id)?;
                if paragraph_index < start_paragraph || paragraph_index > end_paragraph {
                    continue;
                }
                let paragraph = self.paragraphs.get(paragraph_index)?;
                let first_line = paragraph
                    .layout
                    .paragraph
                    .lines
                    .get(fragment.line_range.start)?;
                let last_line = paragraph
                    .layout
                    .paragraph
                    .lines
                    .get(fragment.line_range.end.checked_sub(1)?)?;
                let range_start = if paragraph_index == start_paragraph {
                    start.offset
                } else {
                    0
                }
                .max(first_line.source_bytes.start);
                let range_end = if paragraph_index == end_paragraph {
                    end.offset
                } else {
                    paragraph.text.len()
                }
                .min(last_line.consumed_bytes.end);
                if range_start >= range_end {
                    continue;
                }
                let line_rectangles = paragraph
                    .layout
                    .selection_geometry(range_start..range_end)?;
                for (bounds, line_index) in line_rectangles {
                    if !fragment.line_range.contains(&line_index) {
                        continue;
                    }
                    let fragment_top = f64::from(
                        *paragraph
                            .layout
                            .line_box_tops_pt
                            .get(fragment.line_range.start)?,
                    );
                    let y_offset =
                        f64::from(fragment.y_offset_pt) + page.margin_top_pt - fragment_top;
                    rectangles.push(PageSelectionRect {
                        page_index,
                        x0_pt: bounds.x0 + page.margin_left_pt,
                        y0_pt: bounds.y0 + y_offset,
                        x1_pt: bounds.x1 + page.margin_left_pt,
                        y1_pt: bounds.y1 + y_offset,
                    });
                }
            }
        }
        Some(rectangles)
    }
}

/// 可复用塑形器；调用方跨文档代际复用字体目录，避免按段落扫描系统字体。
#[derive(Default)]
pub struct DocumentLayoutService {
    layouter: ParagraphLayouter,
}

impl DocumentLayoutService {
    pub fn new() -> Self {
        Self::default()
    }

    /// 从 Session 同步读取文档与代际，避免调用方把旧/新文档与错误 generation 配对。
    pub fn layout_session(
        &mut self,
        session: &Session,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        let mut snapshot = self.layout(
            session.document(),
            session.document_generation(),
            catalog,
            base_style,
        )?;
        snapshot.session_id = Some(session.session_id().clone());
        Ok(snapshot)
    }

    /// 根据单一 PageSection 布局正文。未知块先拒绝整次布局，避免静默漏画内容。
    pub fn layout(
        &mut self,
        document: &Document,
        document_generation: u64,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        document
            .validate()
            .map_err(|error| DocumentLayoutError::InvalidDocument(error.to_string()))?;
        let section = document
            .page_sections
            .first()
            .ok_or_else(|| DocumentLayoutError::InvalidDocument("missing PageSection".into()))?;
        let geometry = section
            .geometry()
            .map_err(|error| DocumentLayoutError::InvalidDocument(error.to_string()))?;
        let (page_width_pt, page_height_pt) = geometry.page_size_pt();
        let (content_width_pt, content_height_pt) = geometry
            .content_size_pt()
            .map_err(|error| DocumentLayoutError::InvalidDocument(error.to_string()))?;
        let text_settings =
            ResolvedDocumentTextSettings::from_document_settings(&document.global_settings)
                .map_err(|error| DocumentLayoutError::FontPreferences(error.to_string()))?;

        let mut ordered_inputs = Vec::new();
        collect_blocks(&document.body, &mut ordered_inputs)?;

        let mut empty_mark_metrics_by_language = HashMap::new();
        let paragraph_count = ordered_inputs
            .iter()
            .filter(|item| matches!(item, LayoutInput::Paragraph(_)))
            .count();
        let mut paragraphs = Vec::with_capacity(paragraph_count);
        let mut resolved_styles = Vec::with_capacity(paragraph_count);
        for item in &ordered_inputs {
            let LayoutInput::Paragraph(paragraph) = item else {
                continue;
            };
            let style =
                resolve_paragraph_style(&document.global_settings, paragraph.layout.as_ref())
                    .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
            let placeholder_metrics = caret_layout::FontLineMetrics {
                ascent_pt: 9.6,
                descent_pt: 2.4,
                leading_pt: 0.0,
            };
            let needs_mark_metrics = paragraph_has_empty_line(paragraph);
            if needs_mark_metrics && !paragraph.mark_style.is_empty() {
                return Err(DocumentLayoutError::UnsupportedParagraphMarkStyle);
            }
            let language_key = paragraph.language.clone();
            let empty_mark_metrics = if !needs_mark_metrics {
                placeholder_metrics
            } else if let Some(metrics) = empty_mark_metrics_by_language.get(&language_key) {
                *metrics
            } else {
                let metrics = self
                    .layouter
                    .measure_empty_mark_metrics(
                        catalog,
                        &text_settings,
                        base_style,
                        paragraph.language.as_deref(),
                    )
                    .map_err(|error| DocumentLayoutError::Shaping(error.to_string()))?;
                empty_mark_metrics_by_language.insert(language_key, metrics);
                metrics
            };
            let (text, layout) = self
                .layouter
                .layout(
                    catalog,
                    paragraph,
                    &text_settings,
                    base_style,
                    ParagraphLayoutOptions {
                        width_pt: content_width_pt as f32,
                        spacing: style.line_spacing,
                        empty_mark_metrics,
                    },
                )
                .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
            resolved_styles.push(style);
            paragraphs.push(LaidOutParagraph {
                id: paragraph.id.clone(),
                text,
                layout,
            });
        }

        let mut pagination_items = Vec::with_capacity(ordered_inputs.len());
        let mut paragraph_index = 0;
        for item in ordered_inputs {
            match item {
                LayoutInput::ExplicitBreak => {
                    pagination_items.push(PaginationItem::ExplicitPageBreak);
                }
                LayoutInput::Paragraph(_) => {
                    let paragraph = paragraphs.get(paragraph_index).ok_or_else(|| {
                        DocumentLayoutError::InvalidDocument("missing laid out paragraph".into())
                    })?;
                    let style = resolved_styles.get(paragraph_index).ok_or_else(|| {
                        DocumentLayoutError::InvalidDocument("missing paragraph style".into())
                    })?;
                    pagination_items.push(PaginationItem::Paragraph(
                        ParagraphPaginationInput::with_style(
                            &paragraph.id,
                            &paragraph.layout.paragraph,
                            *style,
                        ),
                    ));
                    paragraph_index += 1;
                }
            }
        }
        let page_layouts = paginate_paragraphs(&pagination_items, content_height_pt as f32)
            .map_err(|error| DocumentLayoutError::Pagination(error.to_string()))?;
        let pages = page_layouts
            .into_iter()
            .map(|layout| PageLayoutSnapshot {
                width_pt: page_width_pt,
                height_pt: page_height_pt,
                margin_left_pt: geometry.margins.left,
                margin_top_pt: geometry.margins.top,
                layout,
            })
            .collect();
        Ok(DocumentLayoutSnapshot {
            session_id: None,
            document_generation,
            font_generation: catalog.generation(),
            paragraph_lookup: paragraphs
                .iter()
                .enumerate()
                .map(|(index, paragraph)| (paragraph.id.clone(), index))
                .collect(),
            paragraphs,
            pages,
        })
    }
}

enum LayoutInput<'a> {
    Paragraph(&'a Paragraph),
    ExplicitBreak,
}

fn collect_blocks<'a>(
    blocks: &'a [Block],
    ordered: &mut Vec<LayoutInput<'a>>,
) -> Result<(), DocumentLayoutError> {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => ordered.push(LayoutInput::Paragraph(paragraph)),
            Block::Environment(_) => {
                return Err(DocumentLayoutError::UnsupportedEnvironmentSemantics);
            }
            Block::ExplicitBreak(page_break) => {
                if !page_break.kind.is_empty() && page_break.kind != "page" {
                    return Err(DocumentLayoutError::UnknownBreakKind(
                        page_break.kind.clone(),
                    ));
                }
                ordered.push(LayoutInput::ExplicitBreak);
            }
            Block::Opaque(_) => return Err(DocumentLayoutError::UnsupportedContent),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{Affinity, Command, Pos, Selection, Session, StoryId};
    use caret_doc::{Block, Document, InlineNode, Run};
    use caret_layout::{FontCatalog, ShapingStyle, VisualAffinity};
    use std::collections::BTreeMap;

    use super::DocumentLayoutService;

    #[test]
    fn document_layout_snapshot_uses_parley_lines_and_pagination() {
        let mut document = Document::blank();
        let text = "A mixed run sequence: e\u{301}, family 👩‍👩‍👧‍👦, and text that wraps. ".repeat(64);
        let Block::Paragraph(paragraph) = &mut document.body[0] else {
            panic!("blank document should start with a paragraph");
        };
        paragraph.language = Some("en".to_owned());
        paragraph.inlines = vec![InlineNode::Run(Run {
            text: text.clone(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })];

        let mut service = DocumentLayoutService::new();
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
        let snapshot = service
            .layout(&document, 7, &mut catalog, &ShapingStyle::default())
            .unwrap_or_else(|error| panic!("document layout failed: {error}"));

        assert_eq!(snapshot.document_generation, 7);
        assert_eq!(snapshot.font_generation, catalog.generation());
        assert_eq!(snapshot.paragraphs.len(), 1);
        assert_eq!(snapshot.paragraphs[0].text, text);
        assert!(snapshot.paragraphs[0].layout.paragraph.lines.len() > 1);
        assert!(!snapshot.pages.is_empty());
        assert_eq!(
            snapshot.pages[0].layout.fragments[0].paragraph_id,
            snapshot.paragraphs[0].id
        );
        assert!(snapshot.pages.len() > 1);

        let first_line = &snapshot.paragraphs[0].layout.paragraph.lines[0];
        let first_cursor = snapshot.paragraphs[0]
            .layout
            .cursor_at(first_line.source_bytes.start, VisualAffinity::Downstream)
            .unwrap_or_else(|| panic!("first line start is not a cursor stop"));
        let cursor_rect = snapshot.paragraphs[0]
            .layout
            .cursor_geometry(first_cursor, 451.0);
        let hit = snapshot
            .hit_test(
                0,
                snapshot.pages[0].margin_left_pt + cursor_rect.x0 + 0.1,
                snapshot.pages[0].margin_top_pt + cursor_rect.y0 + 1.0,
            )
            .unwrap_or_else(|| panic!("first-page hit did not resolve"));
        assert_eq!(hit.position.paragraph_id, snapshot.paragraphs[0].id);
        assert_eq!(hit.position.offset, first_line.source_bytes.start);
        assert_eq!(hit.position.affinity, Affinity::Downstream);

        let boundary_cursor = snapshot.paragraphs[0]
            .layout
            .cursor_at(1, VisualAffinity::Downstream)
            .unwrap_or_else(|| panic!("LTR boundary is not a cursor stop"));
        let boundary_rect = snapshot.paragraphs[0]
            .layout
            .cursor_geometry(boundary_cursor, 0.0);
        let boundary_y =
            snapshot.pages[0].margin_top_pt + f64::from(first_line.metrics.height_pt) / 2.0;
        let left_hit = snapshot
            .hit_test(
                0,
                snapshot.pages[0].margin_left_pt + boundary_rect.x0 - 0.01,
                boundary_y,
            )
            .unwrap_or_else(|| panic!("left boundary hit did not resolve"));
        let right_hit = snapshot
            .hit_test(
                0,
                snapshot.pages[0].margin_left_pt + boundary_rect.x0 + 0.01,
                boundary_y,
            )
            .unwrap_or_else(|| panic!("right boundary hit did not resolve"));
        assert_eq!(left_hit.position.offset, 1);
        assert_eq!(left_hit.position.affinity, Affinity::Upstream);
        assert_eq!(right_hit.position.offset, 1);
        assert_eq!(right_hit.position.affinity, Affinity::Downstream);

        let last_fragment = &snapshot.pages[0].layout.fragments[0];
        let last_page_line_index = last_fragment.line_range.end - 1;
        let last_page_line = &snapshot.paragraphs[0].layout.paragraph.lines[last_page_line_index];
        let fragment_preceding_height = snapshot.paragraphs[0].layout.paragraph.lines
            [last_fragment.line_range.start..last_page_line_index]
            .iter()
            .map(|line| line.metrics.height_pt)
            .sum::<f32>();
        let last_line_bottom = snapshot.pages[0].margin_top_pt
            + f64::from(last_fragment.y_offset_pt + fragment_preceding_height)
            + f64::from(last_page_line.metrics.height_pt);
        let bottom_hit = snapshot
            .hit_test(
                0,
                snapshot.pages[0].margin_left_pt + f64::from(last_page_line.advance_pt / 2.0),
                last_line_bottom - 1.0,
            )
            .unwrap_or_else(|| panic!("page-end line hit did not resolve"));
        assert_eq!(bottom_hit.position.paragraph_id, snapshot.paragraphs[0].id);
        assert!(
            (last_page_line.source_bytes.start..=last_page_line.consumed_bytes.end)
                .contains(&bottom_hit.position.offset)
        );

        let selection = Selection {
            anchor: Pos {
                story: StoryId::Body,
                paragraph_id: snapshot.paragraphs[0].id.clone(),
                offset: 0,
                affinity: Affinity::Downstream,
            },
            focus: Pos {
                story: StoryId::Body,
                paragraph_id: snapshot.paragraphs[0].id.clone(),
                offset: text.len(),
                affinity: Affinity::Upstream,
            },
        };
        let selected = snapshot
            .selection_geometry(&selection)
            .unwrap_or_else(|| panic!("cross-page selection geometry failed"));
        assert!(selected.iter().any(|rect| rect.page_index == 0));
        assert!(
            selected
                .iter()
                .any(|rect| rect.page_index + 1 == snapshot.pages.len())
        );
        assert!(selected.iter().all(|rect| rect.x1_pt > rect.x0_pt));
        let first_page_rects = selected
            .iter()
            .filter(|rect| rect.page_index == 0)
            .collect::<Vec<_>>();
        let min_first_page_y = first_page_rects
            .iter()
            .map(|rect| rect.y0_pt)
            .fold(f64::INFINITY, f64::min);
        let max_first_page_y = first_page_rects
            .iter()
            .map(|rect| rect.y0_pt)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(max_first_page_y > min_first_page_y);
    }

    #[test]
    fn stale_document_or_font_layout_snapshot_is_rejected() {
        let session = Session::new(Document::blank())
            .unwrap_or_else(|error| panic!("session creation failed: {error}"));
        let mut service = DocumentLayoutService::new();
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
        let snapshot = service
            .layout_session(&session, &mut catalog, &ShapingStyle::default())
            .unwrap_or_else(|error| panic!("document layout failed: {error}"));

        assert!(snapshot.is_current_for(&session, catalog.generation()));
        assert!(!snapshot.is_current_for(&session, catalog.generation().saturating_add(1)));

        let mut edited = session;
        edited
            .run(Command::InsertText("changed".to_owned()))
            .unwrap_or_else(|error| panic!("document edit failed: {error}"));
        assert!(!snapshot.is_current_for(&edited, catalog.generation()));
    }
}
