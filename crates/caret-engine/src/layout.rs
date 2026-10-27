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
mod tests;
