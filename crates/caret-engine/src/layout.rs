//! 组合文档、字体塑形、段落行布局与分页，生成供视图消费的只读快照。

use crate::{Affinity, Pos, Selection, Session, StoryId};
use caret_doc::{Block, Document, NodeId, Paragraph};
use caret_layout::{
    FontCatalog, FontCatalogError, PageLayout, PaginationItem, ParagraphLayoutOptions,
    ParagraphLayouter, ParagraphPaginationInput, ParleyParagraphLayout,
    ResolvedDocumentTextSettings, ShapingStyle, VisualAffinity, paginate_paragraphs,
    paragraph_has_empty_line, resolve_paragraph_style,
};
use std::collections::HashMap;

const UTF16_CHECKPOINT_STRIDE_BYTES: usize = 64;

/// 文档布局失败时返回稳定的错误类别；不产生部分快照。
#[derive(Debug)]
pub enum DocumentLayoutError {
    InvalidDocument(String),
    FontPreferences(String),
    FontCatalog(String),
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
            Self::FontCatalog(error) => write!(formatter, "layout.font_catalog: {error}"),
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

impl From<FontCatalogError> for DocumentLayoutError {
    fn from(error: FontCatalogError) -> Self {
        Self::FontCatalog(error.to_string())
    }
}

/// 已排版段落的源 ID、原文與行框；正文文本只在快照里保留一份。
#[derive(Debug, Clone)]
pub struct LaidOutParagraph {
    pub id: NodeId,
    pub text: String,
    pub layout: ParleyParagraphLayout,
    utf16_checkpoints: Vec<(usize, usize)>,
    line_locations: Vec<Option<CaretLineLocation>>,
}

#[derive(Debug, Clone, Copy)]
struct CaretLineLocation {
    page_index: usize,
    y0_pt: f64,
}

impl LaidOutParagraph {
    fn new(id: NodeId, text: String, layout: ParleyParagraphLayout) -> Self {
        let mut utf16_checkpoints = vec![(0, 0)];
        let mut utf16_offset = 0;
        let mut last_checkpoint = 0;
        for (byte_offset, character) in text.char_indices() {
            if byte_offset.saturating_sub(last_checkpoint) >= UTF16_CHECKPOINT_STRIDE_BYTES {
                utf16_checkpoints.push((byte_offset, utf16_offset));
                last_checkpoint = byte_offset;
            }
            utf16_offset += character.len_utf16();
        }
        let line_locations = vec![None; layout.paragraph.lines.len()];
        Self {
            id,
            text,
            layout,
            utf16_checkpoints,
            line_locations,
        }
    }

    /// 将 Rust UTF-8 字节位置映射到 Qt UTF-16 代码单元位置。
    pub fn utf16_offset(&self, byte_offset: usize) -> Option<usize> {
        self.text.get(..byte_offset)?;
        let checkpoint_index = self
            .utf16_checkpoints
            .partition_point(|(byte, _)| *byte <= byte_offset)
            .saturating_sub(1);
        let (checkpoint_byte, checkpoint_utf16) = self
            .utf16_checkpoints
            .get(checkpoint_index)
            .copied()
            .unwrap_or((0, 0));
        let checkpoint_prefix = self.text.get(checkpoint_byte..byte_offset)?;
        Some(checkpoint_utf16 + checkpoint_prefix.encode_utf16().count())
    }
}

/// 一页的文档坐标和段落行片段。
#[derive(Debug, Clone, PartialEq)]
pub struct PageLayoutSnapshot {
    pub width_pt: f64,
    pub height_pt: f64,
    pub margin_left_pt: f64,
    pub margin_top_pt: f64,
    pub margin_right_pt: f64,
    pub margin_bottom_pt: f64,
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

/// 页面局部坐标中的插入点几何；宽度由界面主题决定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageCaretGeometry {
    pub page_index: usize,
    pub x_pt: f64,
    pub y0_pt: f64,
    pub y1_pt: f64,
}

/// 页面命中结果；文档环境亲和性与视觉光标侧分别记录。
#[derive(Debug, Clone, PartialEq)]
pub struct PageHit {
    pub position: Pos,
    pub visual_affinity: VisualAffinity,
    pub caret: PageCaretGeometry,
}

impl DocumentLayoutSnapshot {
    pub fn paragraph_by_id(&self, id: &NodeId) -> Option<&LaidOutParagraph> {
        self.paragraphs.get(*self.paragraph_lookup.get(id)?)
    }

    pub fn paragraph_index_by_id(&self, id: &NodeId) -> Option<usize> {
        self.paragraph_lookup.get(id).copied()
    }

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
        let mut closest_line: Option<(f64, usize, usize, f64)> = None;
        for fragment in &page.layout.fragments {
            let Some(&paragraph_index) = self.paragraph_lookup.get(&fragment.paragraph_id) else {
                continue;
            };
            let Some(paragraph) = self.paragraphs.get(paragraph_index) else {
                continue;
            };
            let mut line_top = f64::from(fragment.y_offset_pt);
            for line_index in fragment.line_range.clone() {
                let Some(line) = paragraph.layout.paragraph.lines.get(line_index) else {
                    continue;
                };
                let line_bottom = line_top + f64::from(line.metrics.height_pt);
                let vertical_distance = if content_y < line_top {
                    line_top - content_y
                } else if content_y > line_bottom {
                    content_y - line_bottom
                } else {
                    0.0
                };
                if closest_line
                    .as_ref()
                    .is_none_or(|(distance, _, _, _)| vertical_distance < *distance)
                {
                    closest_line = Some((vertical_distance, paragraph_index, line_index, line_top));
                }
                line_top = line_bottom;
            }
        }
        let (_, paragraph_index, line_index, line_top) = closest_line?;
        let paragraph = self.paragraphs.get(paragraph_index)?;
        let cursor = paragraph
            .layout
            .cursor_from_point_on_line((x_pt - page.margin_left_pt) as f32, line_index)?;
        let cursor_bounds = paragraph.layout.cursor_geometry(cursor.cursor, 0.0);
        let line = paragraph.layout.paragraph.lines.get(line_index)?;
        let caret_y0 = page.margin_top_pt + line_top;
        Some(PageHit {
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
            caret: PageCaretGeometry {
                page_index,
                x_pt: cursor_bounds.x0 + page.margin_left_pt,
                y0_pt: caret_y0,
                y1_pt: caret_y0 + f64::from(line.metrics.height_pt),
            },
        })
    }

    /// 根据已提交的 Rust 光标位置返回插入点几何，覆盖分页片段和自定义行框。
    pub fn caret_geometry(
        &self,
        position: &Pos,
        visual_affinity: VisualAffinity,
    ) -> Option<PageCaretGeometry> {
        if position.story != StoryId::Body {
            return None;
        }
        let paragraph = self.paragraph_by_id(&position.paragraph_id)?;
        let cursor = paragraph
            .layout
            .cursor_at(position.offset, visual_affinity)?;
        let (bounds, line_index) = paragraph.layout.cursor_geometry_with_line(cursor, 0.0)?;
        let line = paragraph.layout.paragraph.lines.get(line_index)?;
        let location = paragraph.line_locations.get(line_index)?.as_ref()?;
        Some(PageCaretGeometry {
            page_index: location.page_index,
            x_pt: self.pages.get(location.page_index)?.margin_left_pt + bounds.x0,
            y0_pt: location.y0_pt,
            y1_pt: location.y0_pt + f64::from(line.metrics.height_pt),
        })
    }

    /// 返回裁切到可见分页片段内的选区矩形。
    pub fn selection_geometry(&self, selection: &Selection) -> Option<Vec<PageSelectionRect>> {
        self.range_geometry(selection, false)
    }

    /// 返回按字体度量定位的预编辑下划线。
    pub fn composition_geometry(&self, selection: &Selection) -> Option<Vec<PageSelectionRect>> {
        self.range_geometry(selection, true)
    }

    fn range_geometry(
        &self,
        selection: &Selection,
        composition: bool,
    ) -> Option<Vec<PageSelectionRect>> {
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
        if selection.anchor == selection.focus {
            return Some(Vec::new());
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
                let line_rectangles = if composition {
                    paragraph
                        .layout
                        .composition_text_geometry(&paragraph.text, range_start..range_end)?
                } else {
                    paragraph
                        .layout
                        .selection_text_geometry(&paragraph.text, range_start..range_end)?
                };
                let mut fragment_line_tops = Vec::with_capacity(fragment.line_range.len());
                let mut fragment_line_top_pt = f64::from(fragment.y_offset_pt);
                for line_index in fragment.line_range.clone() {
                    let line = paragraph.layout.paragraph.lines.get(line_index)?;
                    fragment_line_tops.push(fragment_line_top_pt);
                    fragment_line_top_pt += f64::from(line.metrics.height_pt);
                }
                for (bounds, line_index) in line_rectangles {
                    if !fragment.line_range.contains(&line_index) {
                        continue;
                    }
                    let local_line_index = line_index.checked_sub(fragment.line_range.start)?;
                    let line_top =
                        page.margin_top_pt + *fragment_line_tops.get(local_line_index)?;
                    let line = paragraph.layout.paragraph.lines.get(line_index)?;
                    let (y0, y1) = if composition {
                        (
                            (line_top + bounds.y0).max(0.0),
                            (line_top + bounds.y1).min(page.height_pt),
                        )
                    } else {
                        (line_top, line_top + f64::from(line.metrics.height_pt))
                    };
                    if y1 <= y0 {
                        continue;
                    }
                    rectangles.push(PageSelectionRect {
                        page_index,
                        x0_pt: bounds.x0 + page.margin_left_pt,
                        y0_pt: y0,
                        x1_pt: bounds.x1 + page.margin_left_pt,
                        y1_pt: y1,
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
    baseline_anchor_cache: Option<BaselineAnchorCache>,
}

#[derive(Debug, Clone, PartialEq)]
struct BaselineAnchorCache {
    font_generation: u64,
    settings: ResolvedDocumentTextSettings,
    style: ShapingStyle,
    metrics: caret_layout::FontLineMetrics,
}

impl DocumentLayoutService {
    pub fn new() -> Self {
        Self::default()
    }

    fn resolve_baseline_anchor_metrics(
        &mut self,
        catalog: &mut FontCatalog,
        settings: &ResolvedDocumentTextSettings,
        style: &ShapingStyle,
    ) -> Result<caret_layout::FontLineMetrics, DocumentLayoutError> {
        let font_generation = catalog.generation();
        if let Some(cache) = &self.baseline_anchor_cache
            && cache.font_generation == font_generation
            && cache.settings == *settings
            && cache.style == *style
        {
            return Ok(cache.metrics);
        }
        let metrics = self
            .layouter
            .measure_font_metrics(catalog, settings, style, settings.default_language())
            .map_err(|error| DocumentLayoutError::Shaping(error.to_string()))?;
        self.baseline_anchor_cache = Some(BaselineAnchorCache {
            font_generation,
            settings: settings.clone(),
            style: style.clone(),
            metrics,
        });
        Ok(metrics)
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

    /// 重排一个发生暂态变化的段落，复用同一文档其他段落的已塑形结果。
    pub fn layout_session_reusing(
        &mut self,
        session: &Session,
        previous: &DocumentLayoutSnapshot,
        changed_paragraph_id: &NodeId,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        let mut snapshot = self.layout_with_reuse(
            session.document(),
            session.document_generation(),
            catalog,
            base_style,
            Some((previous, changed_paragraph_id)),
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
        self.layout_with_reuse(document, document_generation, catalog, base_style, None)
    }

    fn layout_with_reuse(
        &mut self,
        document: &Document,
        document_generation: u64,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
        reuse: Option<(&DocumentLayoutSnapshot, &NodeId)>,
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
        let baseline_anchor_metrics =
            self.resolve_baseline_anchor_metrics(catalog, &text_settings, base_style)?;

        let mut ordered_inputs = Vec::new();
        collect_blocks(&document.body, &mut ordered_inputs)?;

        let mut empty_mark_metrics_by_language = HashMap::new();
        empty_mark_metrics_by_language.insert(
            text_settings.default_language().map(str::to_owned),
            baseline_anchor_metrics,
        );
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
            if let Some((previous, changed_paragraph_id)) = reuse
                && &paragraph.id != changed_paragraph_id
                && let Some(previous_paragraph) = previous.paragraph_by_id(&paragraph.id)
            {
                paragraphs.push(previous_paragraph.clone());
                resolved_styles.push(style);
                continue;
            }
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
                    .measure_font_metrics(
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
                        baseline_anchor_metrics,
                    },
                )
                .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
            resolved_styles.push(style);
            paragraphs.push(LaidOutParagraph::new(paragraph.id.clone(), text, layout));
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
        let pages: Vec<_> = page_layouts
            .into_iter()
            .map(|layout| PageLayoutSnapshot {
                width_pt: page_width_pt,
                height_pt: page_height_pt,
                margin_left_pt: geometry.margins.left,
                margin_top_pt: geometry.margins.top,
                margin_right_pt: geometry.margins.right,
                margin_bottom_pt: geometry.margins.bottom,
                layout,
            })
            .collect();
        let paragraph_lookup: HashMap<_, _> = paragraphs
            .iter()
            .enumerate()
            .map(|(index, paragraph)| (paragraph.id.clone(), index))
            .collect();
        for (page_index, page) in pages.iter().enumerate() {
            for fragment in &page.layout.fragments {
                let Some(&paragraph_index) = paragraph_lookup.get(&fragment.paragraph_id) else {
                    continue;
                };
                let Some(paragraph) = paragraphs.get_mut(paragraph_index) else {
                    continue;
                };
                let mut y0_pt = page.margin_top_pt + f64::from(fragment.y_offset_pt);
                for line_index in fragment.line_range.clone() {
                    let Some(line) = paragraph.layout.paragraph.lines.get(line_index) else {
                        continue;
                    };
                    if let Some(location) = paragraph.line_locations.get_mut(line_index) {
                        *location = Some(CaretLineLocation { page_index, y0_pt });
                    }
                    y0_pt += f64::from(line.metrics.height_pt);
                }
            }
        }
        Ok(DocumentLayoutSnapshot {
            session_id: None,
            document_generation,
            font_generation: catalog.generation(),
            paragraph_lookup,
            paragraphs,
            pages,
        })
    }
}

/// 持有系统字体目录与复用布局器的会话级布局运行时。
pub struct DocumentLayoutRuntime {
    catalog: FontCatalog,
    service: DocumentLayoutService,
}

impl DocumentLayoutRuntime {
    /// 扫描系统字体一次，并复用目录与布局器处理后续文档修订。
    pub fn new_system() -> Result<Self, DocumentLayoutError> {
        Ok(Self {
            catalog: FontCatalog::system()?,
            service: DocumentLayoutService::new(),
        })
    }

    /// 使用当前文档和字体代际生成布局快照。
    pub fn layout_session(
        &mut self,
        session: &Session,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        let base_style = ShapingStyle::new(
            session
                .document()
                .global_settings
                .default_font_size_centipoints,
        );
        self.service
            .layout_session(session, &mut self.catalog, &base_style)
    }

    /// 复用未变段落的塑形结果，仅重新布局指定段落并计算新的分页。
    pub fn layout_session_reusing(
        &mut self,
        session: &Session,
        previous: &DocumentLayoutSnapshot,
        changed_paragraph_id: &NodeId,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        let base_style = ShapingStyle::new(
            session
                .document()
                .global_settings
                .default_font_size_centipoints,
        );
        self.service.layout_session_reusing(
            session,
            previous,
            changed_paragraph_id,
            &mut self.catalog,
            &base_style,
        )
    }

    /// 当前字体目录代际，用于丢弃过期异步结果。
    pub const fn font_generation(&self) -> u64 {
        self.catalog.generation()
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
    use caret_doc::{
        Block, BlockEnvironment, BlockEnvironmentKind, Document, ExplicitBreak, InlineNode, NodeId,
        OpaqueNode, Paragraph, Run, SoftBreak,
    };
    use caret_layout::{
        FontCatalog, FontCatalogError, ResolvedDocumentTextSettings, ShapingStyle, VisualAffinity,
    };
    use std::collections::BTreeMap;

    use super::{DocumentLayoutError, DocumentLayoutService};

    fn paragraph(text: &str) -> Paragraph {
        let mut paragraph = Paragraph::empty(NodeId::generate());
        paragraph.inlines = vec![InlineNode::Run(Run {
            text: text.to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })];
        paragraph
    }

    #[test]
    fn blank_page_click_uses_nearest_line_for_caret_placement() {
        let document = Document::blank();
        let mut service = DocumentLayoutService::new();
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
        let snapshot = service
            .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
            .unwrap_or_else(|error| panic!("document layout failed: {error}"));
        let page = &snapshot.pages[0];

        let hit = snapshot
            .hit_test(0, page.width_pt / 2.0, page.height_pt / 2.0)
            .unwrap_or_else(|| panic!("blank page click did not find a caret stop"));

        assert_eq!(hit.position.offset, 0);
        assert_eq!(hit.position.paragraph_id, snapshot.paragraphs[0].id);
        assert_eq!(hit.caret.page_index, 0);
        assert_eq!(hit.caret.y0_pt, page.margin_top_pt);
    }

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
            .layout(&document, 7, &mut catalog, &ShapingStyle::new(1200))
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
        assert_eq!(hit.caret.page_index, 0);
        assert!(
            (hit.caret.x_pt - (snapshot.pages[0].margin_left_pt + cursor_rect.x0)).abs() < 0.01
        );
        assert!((hit.caret.y0_pt - snapshot.pages[0].margin_top_pt).abs() < 0.01);
        assert!(
            (hit.caret.y1_pt - hit.caret.y0_pt - f64::from(first_line.metrics.height_pt)).abs()
                < 0.01
        );
        let hit_view = snapshot.view_for_hit(&hit);
        assert_eq!(hit_view.caret.page_index, 0);
        assert!((hit_view.caret.x_pt - hit.caret.x_pt).abs() < f64::EPSILON);

        let first_fragment = &snapshot.pages[0].layout.fragments[0];
        let fragment_line_index = first_fragment.line_range.start
            + (first_fragment.line_range.len().saturating_sub(1) / 2);
        let fragment_line = &snapshot.paragraphs[0].layout.paragraph.lines[fragment_line_index];
        let line_offset = snapshot.paragraphs[0].layout.paragraph.lines
            [first_fragment.line_range.start..fragment_line_index]
            .iter()
            .map(|line| f64::from(line.metrics.height_pt))
            .sum::<f64>();
        let fragment_line_cursor = snapshot.paragraphs[0]
            .layout
            .cursor_at(fragment_line.source_bytes.start, VisualAffinity::Downstream)
            .unwrap_or_else(|| panic!("fragment line start is not a cursor stop"));
        let fragment_line_cursor_rect = snapshot.paragraphs[0]
            .layout
            .cursor_geometry(fragment_line_cursor, 0.0);
        let fragment_line_hit = snapshot
            .hit_test(
                0,
                snapshot.pages[0].margin_left_pt + fragment_line_cursor_rect.x0,
                snapshot.pages[0].margin_top_pt
                    + f64::from(first_fragment.y_offset_pt)
                    + line_offset
                    + f64::from(fragment_line.metrics.height_pt) / 2.0,
            )
            .unwrap_or_else(|| panic!("fragment line hit did not resolve"));
        assert_eq!(fragment_line_hit.caret.page_index, 0);
        assert!(
            (fragment_line_hit.caret.y0_pt
                - (snapshot.pages[0].margin_top_pt
                    + f64::from(first_fragment.y_offset_pt)
                    + line_offset))
                .abs()
                < 0.01
        );

        let last_page_index = snapshot.pages.len() - 1;
        let last_page = &snapshot.pages[last_page_index];
        let last_fragment = &last_page.layout.fragments[0];
        let last_line_index = last_fragment.line_range.start;
        let last_line = &snapshot.paragraphs[0].layout.paragraph.lines[last_line_index];
        let last_line_cursor = snapshot.paragraphs[0]
            .layout
            .cursor_at(last_line.source_bytes.start, VisualAffinity::Downstream)
            .unwrap_or_else(|| panic!("last page line start is not a cursor stop"));
        let last_line_cursor_rect = snapshot.paragraphs[0]
            .layout
            .cursor_geometry(last_line_cursor, 0.0);
        let last_page_hit = snapshot
            .hit_test(
                last_page_index,
                last_page.margin_left_pt + last_line_cursor_rect.x0,
                last_page.margin_top_pt
                    + f64::from(last_fragment.y_offset_pt + last_line.metrics.height_pt / 2.0),
            )
            .unwrap_or_else(|| panic!("last page hit did not resolve"));
        assert_eq!(last_page_hit.caret.page_index, last_page_index);
        assert!(last_page_hit.caret.y0_pt >= last_page.margin_top_pt);
        assert!(last_page_hit.caret.y1_pt <= last_page.height_pt - last_page.margin_bottom_pt);

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
    fn long_paragraph_selection_uses_page_local_line_positions() {
        let mut document = Document::blank();
        let text = "a\n".repeat(20_000);
        let Block::Paragraph(paragraph) = &mut document.body[0] else {
            panic!("blank document should start with a paragraph");
        };
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
            .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
            .unwrap_or_else(|error| panic!("document layout failed: {error}"));
        let paragraph = &snapshot.paragraphs[0];
        let line_index = paragraph.layout.paragraph.lines.len() - 20;
        let line = &paragraph.layout.paragraph.lines[line_index];
        let page_index = snapshot
            .pages
            .iter()
            .position(|page| {
                page.layout.fragments.iter().any(|fragment| {
                    fragment.paragraph_id == paragraph.id
                        && fragment.line_range.contains(&line_index)
                })
            })
            .unwrap_or_else(|| panic!("late paragraph line has no page"));
        let position = Pos {
            story: StoryId::Body,
            paragraph_id: paragraph.id.clone(),
            offset: line.source_bytes.start,
            affinity: Affinity::Downstream,
        };
        let caret = snapshot
            .caret_geometry(&position, VisualAffinity::Downstream)
            .unwrap_or_else(|| panic!("late paragraph caret geometry failed"));
        assert_eq!(caret.page_index, page_index);
        let selection = Selection {
            anchor: position.clone(),
            focus: Pos {
                offset: line.consumed_bytes.end,
                affinity: Affinity::Upstream,
                ..position
            },
        };
        let selected = snapshot
            .selection_geometry(&selection)
            .unwrap_or_else(|| panic!("late paragraph selection geometry failed"));
        let rect = selected
            .iter()
            .find(|rect| rect.page_index == page_index)
            .unwrap_or_else(|| panic!("late paragraph selection has no page-local rect"));

        assert!((rect.y0_pt - caret.y0_pt).abs() < 0.000_001);
        assert!((rect.y1_pt - caret.y1_pt).abs() < 0.000_001);
    }

    #[test]
    fn stale_document_or_font_layout_snapshot_is_rejected() {
        let session = Session::new(Document::blank())
            .unwrap_or_else(|error| panic!("session creation failed: {error}"));
        let mut service = DocumentLayoutService::new();
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
        let snapshot = service
            .layout_session(&session, &mut catalog, &ShapingStyle::new(1200))
            .unwrap_or_else(|error| panic!("document layout failed: {error}"));

        assert!(snapshot.is_current_for(&session, catalog.generation()));
        assert!(!snapshot.is_current_for(&session, catalog.generation().saturating_add(1)));

        let mut edited = session;
        edited
            .run(Command::InsertText("changed".to_owned()))
            .unwrap_or_else(|error| panic!("document edit failed: {error}"));
        assert!(!snapshot.is_current_for(&edited, catalog.generation()));
    }

    #[test]
    fn layout_errors_reject_the_entire_document_and_leave_the_service_reusable() {
        let mut invalid_model = Document::blank();
        invalid_model.page_sections.clear();
        let mut unknown_break = Document::blank();
        unknown_break.body.push(Block::ExplicitBreak(ExplicitBreak {
            kind: "column".to_owned(),
            extra: BTreeMap::new(),
        }));
        let mut opaque_block = Document::blank();
        opaque_block.body.push(Block::Opaque(OpaqueNode {
            raw: serde_json::json!({"type": "future_block", "content": "preserved"}),
        }));
        let mut environment = Document::blank();
        environment.body.push(Block::Environment(BlockEnvironment {
            id: NodeId::generate(),
            kind: BlockEnvironmentKind::Showcase,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children: vec![Block::Paragraph(paragraph("nested body"))],
            extra: BTreeMap::new(),
        }));
        let mut unknown_empty_mark = Document::blank();
        let Block::Paragraph(empty) = &mut unknown_empty_mark.body[0] else {
            unreachable!();
        };
        empty
            .mark_style
            .insert("futureMetric".to_owned(), serde_json::json!(true));
        let mut service = DocumentLayoutService::new();
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));

        for (document, expected_prefix) in [
            (invalid_model, "layout.invalid_document:"),
            (unknown_break, "layout.unknown_break_kind: column"),
            (opaque_block, "layout.unsupported_content"),
            (environment, "layout.unsupported_environment_semantics"),
            (
                unknown_empty_mark,
                "layout.unsupported_paragraph_mark_style",
            ),
        ] {
            let Err(error) = service.layout(&document, 7, &mut catalog, &ShapingStyle::new(1200))
            else {
                panic!("unsupported input must not publish a partial snapshot");
            };
            assert!(error.to_string().starts_with(expected_prefix));
            let valid = service
                .layout(
                    &Document::blank(),
                    8,
                    &mut catalog,
                    &ShapingStyle::new(1200),
                )
                .unwrap_or_else(|error| panic!("service failed after rejected input: {error}"));
            assert_eq!(valid.document_generation, 8);
            assert_eq!(valid.paragraphs.len(), 1);
            assert_eq!(valid.pages.len(), 1);
        }
    }

    #[test]
    fn unavailable_font_catalog_keeps_its_layout_error_category() {
        let error = DocumentLayoutError::from(FontCatalogError::Empty);
        assert!(
            matches!(&error, DocumentLayoutError::FontCatalog(detail) if detail == "layout.font_catalog_empty")
        );
        assert_eq!(
            error.to_string(),
            "layout.font_catalog: layout.font_catalog_empty"
        );
    }

    #[test]
    fn failed_font_measurement_preserves_the_last_valid_baseline_cache() {
        let mut service = DocumentLayoutService::new();
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
        let settings = ResolvedDocumentTextSettings::from_document_settings(
            &Document::blank().global_settings,
        )
        .unwrap_or_else(|error| panic!("text settings rejected: {error}"));
        let style = ShapingStyle::new(1200);
        let valid = service
            .resolve_baseline_anchor_metrics(&mut catalog, &settings, &style)
            .unwrap_or_else(|error| panic!("font measurement failed: {error}"));
        let saved_cache = service.baseline_anchor_cache.clone();

        let Err(error) =
            service.resolve_baseline_anchor_metrics(&mut catalog, &settings, &ShapingStyle::new(0))
        else {
            panic!("zero-sized font must not produce baseline metrics");
        };
        assert!(matches!(error, DocumentLayoutError::Shaping(_)));
        assert_eq!(
            error.to_string(),
            "layout.shaping: layout.invalid_font_size"
        );
        assert_eq!(service.baseline_anchor_cache, saved_cache);
        assert_eq!(
            service
                .resolve_baseline_anchor_metrics(&mut catalog, &settings, &style)
                .unwrap_or_else(|error| panic!("valid cache was lost: {error}")),
            valid
        );
    }

    #[test]
    fn baseline_cache_tracks_style_language_and_refreshed_font_generation() {
        let mut document = Document::blank();
        let session = Session::new(document.clone())
            .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
        let mut service = DocumentLayoutService::new();
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
        let style = ShapingStyle::new(1200);
        let first = service
            .layout_session(&session, &mut catalog, &style)
            .unwrap_or_else(|error| panic!("first layout failed: {error}"));
        let first_cache = service
            .baseline_anchor_cache
            .clone()
            .unwrap_or_else(|| panic!("successful layout needs a baseline cache"));
        let same = service
            .layout_session(&session, &mut catalog, &style)
            .unwrap_or_else(|error| panic!("repeated layout failed: {error}"));
        assert_eq!(service.baseline_anchor_cache.as_ref(), Some(&first_cache));
        assert_eq!(same.pages, first.pages);

        document.global_settings.default_language = Some("ru".to_owned());
        let changed_settings =
            ResolvedDocumentTextSettings::from_document_settings(&document.global_settings)
                .unwrap_or_else(|error| panic!("language settings rejected: {error}"));
        let larger_style = ShapingStyle {
            font_size_centipoints: 2400,
            bold: true,
            italic: true,
            language: Some("el".to_owned()),
            requested_family: Some("Caret Missing Baseline Font".to_owned()),
        };
        let larger_metrics = service
            .resolve_baseline_anchor_metrics(&mut catalog, &changed_settings, &larger_style)
            .unwrap_or_else(|error| panic!("changed baseline failed: {error}"));
        let changed_cache = service
            .baseline_anchor_cache
            .as_ref()
            .unwrap_or_else(|| panic!("changed style needs a new cache entry"));
        assert_eq!(changed_cache.settings, changed_settings);
        assert_eq!(changed_cache.style, larger_style);
        assert!(larger_metrics.ascent_pt > first_cache.metrics.ascent_pt);
        assert_eq!(changed_cache.font_generation, catalog.generation());

        let refreshed_generation = catalog
            .refresh()
            .unwrap_or_else(|error| panic!("font refresh failed: {error}"));
        assert!(!first.is_current_for(&session, refreshed_generation));
        let refreshed = service
            .layout_session(&session, &mut catalog, &style)
            .unwrap_or_else(|error| panic!("layout after refresh failed: {error}"));
        assert!(refreshed.is_current_for(&session, refreshed_generation));
        assert_eq!(
            service
                .baseline_anchor_cache
                .as_ref()
                .map(|cache| cache.font_generation),
            Some(refreshed_generation)
        );
    }

    #[test]
    fn page_hit_testing_rejects_nonfinite_and_outside_coordinates() {
        let mut document = Document::blank();
        document.body = vec![Block::Paragraph(paragraph("a"))];
        let mut service = DocumentLayoutService::new();
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
        let snapshot = service
            .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
            .unwrap_or_else(|error| panic!("document layout failed: {error}"));
        let page = &snapshot.pages[0];
        for (x, y) in [
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
            (-1.0, 0.0),
            (0.0, -1.0),
            (page.width_pt + 1.0, 0.0),
            (0.0, page.height_pt + 1.0),
        ] {
            assert!(snapshot.hit_test(0, x, y).is_none());
        }
        assert!(snapshot.hit_test(1, 0.0, 0.0).is_none());
        for (x, y, offset) in [
            (0.0, 0.0, 0),
            (0.0, page.height_pt, 0),
            (page.width_pt, 0.0, 1),
            (page.width_pt, page.height_pt, 1),
        ] {
            let hit = snapshot
                .hit_test(0, x, y)
                .unwrap_or_else(|| panic!("page edge should select nearest text"));
            assert_eq!(hit.position.offset, offset);
            assert_eq!(hit.caret.y0_pt, page.margin_top_pt);
        }
    }

    #[test]
    fn reversed_cross_paragraph_selection_and_invalid_utf8_positions_are_handled() {
        let mut document = Document::blank();
        document.body = ["outside-before", "e\u{301}", "中", "outside-after"]
            .into_iter()
            .map(|text| Block::Paragraph(paragraph(text)))
            .collect();
        let mut service = DocumentLayoutService::new();
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
        let snapshot = service
            .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
            .unwrap_or_else(|error| panic!("document layout failed: {error}"));
        let start = Pos {
            story: StoryId::Body,
            paragraph_id: snapshot.paragraphs[1].id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        let end = Pos {
            paragraph_id: snapshot.paragraphs[2].id.clone(),
            offset: "中".len(),
            affinity: Affinity::Upstream,
            ..start.clone()
        };
        let forward = snapshot
            .selection_geometry(&Selection {
                anchor: start.clone(),
                focus: end.clone(),
            })
            .unwrap_or_else(|| panic!("cross-paragraph selection rejected"));
        let reversed = snapshot
            .selection_geometry(&Selection {
                anchor: end.clone(),
                focus: start.clone(),
            })
            .unwrap_or_else(|| panic!("reversed selection rejected"));
        assert_eq!(forward, reversed);
        assert_eq!(forward.len(), 2);
        assert!(forward[0].y0_pt < forward[1].y0_pt);
        assert!(forward.iter().all(|rect| rect.x1_pt > rect.x0_pt));

        for invalid in [
            Pos {
                offset: 2,
                ..start.clone()
            },
            Pos {
                offset: 4,
                ..start.clone()
            },
            Pos {
                paragraph_id: NodeId::generate(),
                ..start.clone()
            },
            Pos {
                story: StoryId::EnvironmentTitle(NodeId::generate()),
                ..start.clone()
            },
        ] {
            assert!(
                snapshot
                    .caret_geometry(&invalid, VisualAffinity::Downstream)
                    .is_none()
            );
            assert!(
                snapshot
                    .selection_geometry(&Selection {
                        anchor: invalid,
                        focus: end.clone(),
                    })
                    .is_none()
            );
        }
        assert!(
            snapshot
                .caret_geometry(
                    &Pos {
                        offset: 1,
                        ..start.clone()
                    },
                    VisualAffinity::Downstream
                )
                .is_none()
        );
        assert_eq!(
            snapshot.selection_geometry(&Selection {
                anchor: start.clone(),
                focus: start
            }),
            Some(Vec::new())
        );
    }

    #[test]
    fn soft_break_is_selected_logically_without_a_trailing_highlight_block() {
        let mut document = Document::blank();
        let Block::Paragraph(paragraph) = &mut document.body[0] else {
            unreachable!();
        };
        paragraph.inlines = vec![
            InlineNode::Run(Run {
                text: "你好".to_owned(),
                language: Some("zh".to_owned()),
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }),
            InlineNode::SoftBreak(SoftBreak::default()),
            InlineNode::Run(Run {
                text: "你好".to_owned(),
                language: Some("zh".to_owned()),
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }),
        ];
        let paragraph_id = paragraph.id.clone();
        let mut service = DocumentLayoutService::new();
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
        let snapshot = service
            .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
            .unwrap_or_else(|error| panic!("document layout failed: {error}"));
        let start = Pos {
            story: StoryId::Body,
            paragraph_id,
            offset: 0,
            affinity: Affinity::Downstream,
        };
        let text_end = Pos {
            offset: "你好".len(),
            affinity: Affinity::Upstream,
            ..start.clone()
        };
        let break_end = Pos {
            offset: "你好\n".len(),
            ..text_end.clone()
        };
        let text_rectangles = snapshot
            .selection_geometry(&Selection {
                anchor: start.clone(),
                focus: text_end,
            })
            .unwrap_or_else(|| panic!("text selection geometry failed"));
        let break_rectangles = snapshot
            .selection_geometry(&Selection {
                anchor: start,
                focus: break_end,
            })
            .unwrap_or_else(|| panic!("soft-break selection geometry failed"));
        assert_eq!(break_rectangles, text_rectangles);
    }

    #[test]
    fn explicit_breaks_keep_empty_pages_without_inventing_text_hit_targets() {
        let first = paragraph("a");
        let last = paragraph("b");
        let first_id = first.id.clone();
        let last_id = last.id.clone();
        let page_break = || Block::ExplicitBreak(ExplicitBreak::default());
        let mut document = Document::blank();
        document.body = vec![
            page_break(),
            Block::Paragraph(first),
            page_break(),
            page_break(),
            Block::Paragraph(last),
            page_break(),
        ];
        let mut service = DocumentLayoutService::new();
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
        let snapshot = service
            .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
            .unwrap_or_else(|error| panic!("document layout failed: {error}"));
        assert_eq!(snapshot.pages.len(), 5);
        for page in [0, 2, 4] {
            assert!(snapshot.pages[page].layout.fragments.is_empty());
            assert!(snapshot.hit_test(page, 100.0, 100.0).is_none());
        }
        for (id, expected_page) in [(first_id, 1), (last_id, 3)] {
            let caret = snapshot
                .caret_geometry(
                    &Pos {
                        story: StoryId::Body,
                        paragraph_id: id.clone(),
                        offset: 0,
                        affinity: Affinity::Downstream,
                    },
                    VisualAffinity::Downstream,
                )
                .unwrap_or_else(|| panic!("text after page break has no caret"));
            assert_eq!(caret.page_index, expected_page);
            let hit = snapshot
                .hit_test(expected_page, caret.x_pt, caret.y0_pt)
                .unwrap_or_else(|| panic!("text page has no hit target"));
            assert_eq!(hit.position.paragraph_id, id);
            assert_eq!(hit.position.offset, 0);
        }
    }
}
