//! 组合文档、字体塑形、段落行布局与分页，生成供视图消费的只读快照。

use crate::Session;
use caret_doc::{Block, Document, NodeId, Paragraph};
use caret_layout::{
    FontCatalog, PageLayout, PaginationItem, ParagraphLayoutOptions, ParagraphLayouter,
    ParagraphPaginationInput, ParleyParagraphLayout, ResolvedDocumentTextSettings, ShapingStyle,
    paginate_paragraphs, paragraph_has_empty_line, resolve_paragraph_style,
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
    use caret_doc::{Block, Document, InlineNode, Run};
    use caret_layout::{FontCatalog, ShapingStyle};
    use std::collections::BTreeMap;

    use super::DocumentLayoutService;

    #[test]
    fn document_layout_snapshot_uses_parley_lines_and_pagination() {
        let mut document = Document::blank();
        let text = "A mixed run sequence: e\u{301}, family 👩‍👩‍👧‍👦, and text that wraps. ".repeat(16);
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
    }
}
