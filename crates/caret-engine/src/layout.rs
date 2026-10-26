//! 组合文档、字体塑形、段落行布局与分页，生成供视图消费的只读快照。

use caret_doc::{Block, Document, NodeId, Paragraph};
use caret_layout::{
    FontCatalog, FontLineMetrics, PageLayout, PaginationItem, ParagraphLayout,
    ParagraphPaginationInput, ResolvedDocumentTextSettings, ShapingStyle, TextShaper,
    has_empty_line, layout_shaped_paragraph, paginate_paragraphs, paragraph_text,
    resolve_paragraph_style,
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
#[derive(Debug, Clone, PartialEq)]
pub struct LaidOutParagraph {
    pub id: NodeId,
    pub text: String,
    pub shaped: caret_layout::ShapedParagraph,
    pub layout: ParagraphLayout,
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
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentLayoutSnapshot {
    pub document_generation: u64,
    pub font_generation: u64,
    pub paragraphs: Vec<LaidOutParagraph>,
    pub pages: Vec<PageLayoutSnapshot>,
}

/// 可复用塑形器；调用方跨文档代际复用字体目录，避免按段落扫描系统字体。
#[derive(Debug, Default)]
pub struct DocumentLayoutService {
    shaper: TextShaper,
}

impl DocumentLayoutService {
    pub fn new() -> Self {
        Self::default()
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
            let text = paragraph_text(paragraph);
            let shaped = self
                .shaper
                .shape_paragraph_with_resolved_settings(
                    catalog,
                    paragraph,
                    &text_settings,
                    base_style,
                )
                .map_err(|error| DocumentLayoutError::Shaping(error.to_string()))?
                .clone();
            let style =
                resolve_paragraph_style(&document.global_settings, paragraph.layout.as_ref())
                    .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
            let needs_mark_metrics = has_empty_line(&text);
            if needs_mark_metrics && !paragraph.mark_style.is_empty() {
                return Err(DocumentLayoutError::UnsupportedParagraphMarkStyle);
            }
            let language_key = paragraph.language.clone();
            let empty_mark_metrics =
                if let Some(metrics) = empty_mark_metrics_by_language.get(&language_key) {
                    *metrics
                } else {
                    let metrics = self.empty_mark_metrics(
                        catalog,
                        base_style,
                        &text_settings,
                        paragraph.language.as_deref(),
                    )?;
                    empty_mark_metrics_by_language.insert(language_key, metrics);
                    metrics
                };
            let layout = layout_shaped_paragraph(
                &text,
                &shaped,
                content_width_pt as f32,
                style.line_spacing,
                empty_mark_metrics,
                &[],
            )
            .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
            resolved_styles.push(style);
            paragraphs.push(LaidOutParagraph {
                id: paragraph.id.clone(),
                text,
                shaped,
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
                            &paragraph.layout,
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
            document_generation,
            font_generation: catalog.generation(),
            paragraphs,
            pages,
        })
    }

    fn empty_mark_metrics(
        &mut self,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
        text_settings: &ResolvedDocumentTextSettings,
        language_override: Option<&str>,
    ) -> Result<FontLineMetrics, DocumentLayoutError> {
        let shaped = self
            .shaper
            .shape_with_resolved_settings(
                catalog,
                " ",
                text_settings,
                language_override,
                base_style,
            )
            .map_err(|error| DocumentLayoutError::Shaping(error.to_string()))?;
        shaped
            .text
            .runs()
            .first()
            .map(|run| FontLineMetrics::from(run.font_metrics))
            .ok_or_else(|| DocumentLayoutError::Shaping("missing empty mark metrics".into()))
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
