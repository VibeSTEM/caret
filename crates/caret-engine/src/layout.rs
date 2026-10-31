//! 组合文档、字体塑形、段落行布局与分页，生成供视图消费的只读快照。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use crate::{Affinity, Pos, Selection, Session, StoryId};
use caret_doc::{Block, Document, NodeId, Paragraph};
use caret_layout::{
    FontCatalog, FontCatalogError, PageLayout, PaginationItem, ParagraphLayoutOptions,
    ParagraphLayouter, ParagraphPaginationInput, ParleyParagraphLayout, PositionedGlyphLayoutItem,
    ResolvedDocumentTextSettings, ShapingStyle, VisualAffinity, paginate_paragraphs,
    paragraph_has_empty_line, resolve_paragraph_style,
};
use std::{collections::HashMap, sync::Arc};

const UTF16_CHECKPOINT_STRIDE_BYTES: usize = 64;
const LINE_GRID_LATIN_SAMPLE: char = 'M';

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

/// 已排版段落的源 ID、原文与行框；只读塑形数据跨快照共享，分页位置按快照独立保存。
#[derive(Debug, Clone)]
pub struct LaidOutParagraph {
    pub id: NodeId,
    pub story: StoryId,
    pub environment_ids: Arc<[NodeId]>,
    pub text: Arc<str>,
    pub layout: Arc<ParleyParagraphLayout>,
    utf16_checkpoints: Arc<[(usize, usize)]>,
    line_locations: Vec<Option<CaretLineLocation>>,
}

#[derive(Debug, Clone, Copy)]
struct CaretLineLocation {
    page_index: usize,
    y0_pt: f64,
}

impl LaidOutParagraph {
    fn new(
        id: NodeId,
        story: StoryId,
        environment_ids: Vec<NodeId>,
        text: String,
        layout: ParleyParagraphLayout,
    ) -> Self {
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
            story,
            environment_ids: Arc::from(environment_ids),
            text: Arc::from(text),
            layout: Arc::new(layout),
            utf16_checkpoints: Arc::from(utf16_checkpoints),
            line_locations,
        }
    }

    fn reused_for_layout(&self, story: StoryId, environment_ids: &[NodeId]) -> Self {
        Self {
            id: self.id.clone(),
            story,
            environment_ids: Arc::from(environment_ids),
            text: Arc::clone(&self.text),
            layout: Arc::clone(&self.layout),
            utf16_checkpoints: Arc::clone(&self.utf16_checkpoints),
            line_locations: vec![None; self.layout.paragraph.lines.len()],
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

/// 块级环境在单页上的内容范围，由其标题和子块的实际分页片段合并得到。
#[derive(Debug, Clone, PartialEq)]
pub struct PageEnvironmentRange {
    pub environment_id: NodeId,
    pub page_index: usize,
    pub x0_pt: f64,
    pub y0_pt: f64,
    pub x1_pt: f64,
    pub y1_pt: f64,
}

/// 单代文档布局结果。页面和段落位置共用同一分页输出。
#[derive(Debug, Clone)]
pub struct DocumentLayoutSnapshot {
    pub session_id: Option<NodeId>,
    pub document_generation: u64,
    pub font_generation: u64,
    default_font_size_centipoints: u16,
    pub paragraphs: Vec<LaidOutParagraph>,
    pub pages: Vec<PageLayoutSnapshot>,
    pub environment_ranges: Vec<PageEnvironmentRange>,
    paragraph_lookup: HashMap<NodeId, usize>,
}

#[derive(Debug, Clone, Copy)]
struct ParagraphTextRange {
    start_index: usize,
    start_offset: usize,
    end_index: usize,
    end_offset: usize,
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
    /// 返回光标位置当前塑形 Run 的字号；Run 边界按视觉亲和性选择，空段回退到文档默认字号。
    pub fn font_size_centipoints_at(
        &self,
        position: &Pos,
        visual_affinity: VisualAffinity,
    ) -> Option<u16> {
        let paragraph = self.paragraph_by_id(&position.paragraph_id)?;
        if paragraph.story != position.story {
            return None;
        }
        let cursor = paragraph
            .layout
            .cursor_at(position.offset, visual_affinity)?;
        let (_, line_index) = paragraph.layout.cursor_geometry_with_line(cursor, 0.0)?;
        let line = paragraph.layout.layout.get(line_index)?;
        let mut upstream_size = None;
        let mut downstream_size = None;
        for item in line.items() {
            let PositionedGlyphLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let range = run.text_range();
            let font_size = run.font_size();
            if range.start < position.offset && position.offset < range.end {
                return font_size_centipoints(font_size);
            }
            if range.end == position.offset {
                upstream_size = font_size_centipoints(font_size);
            }
            if range.start == position.offset {
                downstream_size = font_size_centipoints(font_size);
            }
        }
        let size = match visual_affinity {
            VisualAffinity::Upstream => upstream_size.or(downstream_size),
            VisualAffinity::Downstream => downstream_size.or(upstream_size),
        };
        Some(size.unwrap_or(self.default_font_size_centipoints))
    }

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
                story: paragraph.story.clone(),
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
        let paragraph = self.paragraph_by_id(&position.paragraph_id)?;
        if paragraph.story != position.story {
            return None;
        }
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

    /// 返回跨 Story 结构选区中环境外文字的几何；选中的块环境由环境矩形单独绘制。
    pub fn structure_selection_geometry(
        &self,
        anchor: &Pos,
        focus: &Pos,
        selected_environment_ids: &[NodeId],
    ) -> Option<Vec<PageSelectionRect>> {
        let anchor_index = *self.paragraph_lookup.get(&anchor.paragraph_id)?;
        let focus_index = *self.paragraph_lookup.get(&focus.paragraph_id)?;
        let anchor_paragraph = self.paragraphs.get(anchor_index)?;
        let focus_paragraph = self.paragraphs.get(focus_index)?;
        if anchor_paragraph.story != anchor.story
            || focus_paragraph.story != focus.story
            || !Self::valid_text_offset(anchor_paragraph, anchor.offset)
            || !Self::valid_text_offset(focus_paragraph, focus.offset)
        {
            return None;
        }
        let (start_index, start_offset, end_index, end_offset) =
            if (anchor_index, anchor.offset) <= (focus_index, focus.offset) {
                (anchor_index, anchor.offset, focus_index, focus.offset)
            } else {
                (focus_index, focus.offset, anchor_index, anchor.offset)
            };
        self.text_range_geometry(
            ParagraphTextRange {
                start_index,
                start_offset,
                end_index,
                end_offset,
            },
            false,
            None,
            selected_environment_ids,
        )
    }

    fn range_geometry(
        &self,
        selection: &Selection,
        composition: bool,
    ) -> Option<Vec<PageSelectionRect>> {
        if selection.anchor.story != selection.focus.story {
            return None;
        }
        let anchor_index = *self.paragraph_lookup.get(&selection.anchor.paragraph_id)?;
        let focus_index = *self.paragraph_lookup.get(&selection.focus.paragraph_id)?;
        let anchor_paragraph = self.paragraphs.get(anchor_index)?;
        let focus_paragraph = self.paragraphs.get(focus_index)?;
        if anchor_paragraph.story != selection.anchor.story
            || focus_paragraph.story != selection.focus.story
            || selection.anchor.story != selection.focus.story
        {
            return None;
        }
        if !Self::valid_text_offset(anchor_paragraph, selection.anchor.offset)
            || !Self::valid_text_offset(focus_paragraph, selection.focus.offset)
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
        self.text_range_geometry(
            ParagraphTextRange {
                start_index: start_paragraph,
                start_offset: start.offset,
                end_index: end_paragraph,
                end_offset: end.offset,
            },
            composition,
            Some(&start.story),
            &[],
        )
    }

    fn valid_text_offset(paragraph: &LaidOutParagraph, offset: usize) -> bool {
        offset <= paragraph.text.len() && paragraph.text.is_char_boundary(offset)
    }

    fn text_range_geometry(
        &self,
        range: ParagraphTextRange,
        composition: bool,
        story: Option<&StoryId>,
        excluded_environment_ids: &[NodeId],
    ) -> Option<Vec<PageSelectionRect>> {
        let mut rectangles = Vec::new();
        for paragraph_index in range.start_index..=range.end_index {
            let paragraph = self.paragraphs.get(paragraph_index)?;
            if story.is_some_and(|story| &paragraph.story != story)
                || paragraph
                    .environment_ids
                    .iter()
                    .any(|id| excluded_environment_ids.contains(id))
            {
                continue;
            }
            let range_start = if paragraph_index == range.start_index {
                range.start_offset
            } else {
                0
            };
            let range_end = if paragraph_index == range.end_index {
                range.end_offset
            } else {
                paragraph.text.len()
            };
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
            for (bounds, line_index) in line_rectangles {
                let line = paragraph.layout.paragraph.lines.get(line_index)?;
                let location = paragraph.line_locations.get(line_index)?.as_ref()?;
                let page = self.pages.get(location.page_index)?;
                let (y0, y1) = if composition {
                    (
                        (location.y0_pt + bounds.y0).max(0.0),
                        (location.y0_pt + bounds.y1).min(page.height_pt),
                    )
                } else {
                    (
                        location.y0_pt,
                        location.y0_pt + f64::from(line.metrics.height_pt),
                    )
                };
                if y1 <= y0 {
                    continue;
                }
                rectangles.push(PageSelectionRect {
                    page_index: location.page_index,
                    x0_pt: bounds.x0 + page.margin_left_pt,
                    y0_pt: y0,
                    x1_pt: bounds.x1 + page.margin_left_pt,
                    y1_pt: y1,
                });
            }
        }
        Some(rectangles)
    }
}

fn font_size_centipoints(font_size_pt: f32) -> Option<u16> {
    let centipoints = f64::from(font_size_pt) * 100.0;
    if !centipoints.is_finite() || centipoints < 1.0 || centipoints > f64::from(u16::MAX) {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "字号已校验并四舍五入到 u16 范围"
    )]
    u16::try_from(centipoints.round() as u32).ok()
}

/// 可复用塑形器；调用方跨文档代际复用字体目录，避免按段落扫描系统字体。
#[derive(Default)]
pub struct DocumentLayoutService {
    layouter: ParagraphLayouter,
    line_grid_metrics_cache: Option<LineGridMetricsCache>,
}

#[derive(Debug, Clone, PartialEq)]
struct LineGridMetricsCache {
    font_generation: u64,
    settings: ResolvedDocumentTextSettings,
    style: ShapingStyle,
    metrics: caret_layout::FontLineMetrics,
}

impl DocumentLayoutService {
    /// 常驻公式后端缓存的条目数；未排过公式时为 None。
    #[cfg(test)]
    pub(crate) fn math_cache_len(&self) -> Option<usize> {
        self.math.as_ref().map(MathLayoutEngine::cached_len)
    }

    pub fn new() -> Self {
        Self::default()
    }

    fn resolve_line_grid_metrics(
        &mut self,
        catalog: &mut FontCatalog,
        settings: &ResolvedDocumentTextSettings,
        style: &ShapingStyle,
        language: Option<&str>,
    ) -> Result<caret_layout::FontLineMetrics, DocumentLayoutError> {
        let font_generation = catalog.generation();
        if let Some(cache) = &self.line_grid_metrics_cache
            && cache.font_generation == font_generation
            && cache.settings == *settings
            && cache.style == *style
        {
            return Ok(cache.metrics);
        }
        let metrics = self
            .layouter
            .measure_font_metrics(
                catalog,
                settings,
                style,
                language,
                Some(LINE_GRID_LATIN_SAMPLE),
            )
            .map_err(|error| DocumentLayoutError::Shaping(error.to_string()))?;
        self.line_grid_metrics_cache = Some(LineGridMetricsCache {
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
        let mut snapshot = self.layout_with_reuse(
            session.document(),
            session.document_generation(),
            catalog,
            base_style,
            None,
            false,
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
            false,
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
        self.layout_with_reuse(
            document,
            document_generation,
            catalog,
            base_style,
            None,
            true,
        )
    }

    fn layout_with_reuse(
        &mut self,
        document: &Document,
        document_generation: u64,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
        reuse: Option<(&DocumentLayoutSnapshot, &NodeId)>,
        validate_document: bool,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        // Session 创建时已校验文档，后续命令维持结构不变量，避免每次排版重复遍历整树。
        if validate_document {
            document
                .validate()
                .map_err(|error| DocumentLayoutError::InvalidDocument(error.to_string()))?;
        }
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
        collect_blocks(&document.body, &mut ordered_inputs, &mut Vec::new())?;

        let paragraph_count = ordered_inputs
            .iter()
            .filter(|item| matches!(item, LayoutInput::Paragraph { .. }))
            .count();
        let mut paragraphs = Vec::with_capacity(paragraph_count);
        let mut resolved_styles = Vec::with_capacity(paragraph_count);
        for item in &ordered_inputs {
            let LayoutInput::Paragraph {
                paragraph,
                story,
                environment_ids,
            } = item
            else {
                continue;
            };
            let style =
                resolve_paragraph_style(&document.global_settings, paragraph.layout.as_ref())
                    .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
            if let Some((previous, changed_paragraph_id)) = reuse
                && &paragraph.id != changed_paragraph_id
                && let Some(previous_paragraph) = previous.paragraph_by_id(&paragraph.id)
                && previous_paragraph.story == *story
                && previous_paragraph.environment_ids.as_ref() == environment_ids
            {
                paragraphs
                    .push(previous_paragraph.reused_for_layout(story.clone(), environment_ids));
                resolved_styles.push(style);
                continue;
            }
            let line_grid_metrics = self.resolve_line_grid_metrics(
                catalog,
                &text_settings,
                base_style,
                text_settings.default_language(),
            )?;
            let needs_mark_metrics = paragraph_has_empty_line(paragraph);
            if needs_mark_metrics && !paragraph.mark_style.is_empty() {
                return Err(DocumentLayoutError::UnsupportedParagraphMarkStyle);
            }
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
                        line_grid_metrics,
                        line_grid_font_size_centipoints: base_style.font_size_centipoints,
                        first_line_indent_chars: style.first_line_indent_chars,
                    },
                )
                .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
            resolved_styles.push(style);
            paragraphs.push(LaidOutParagraph::new(
                paragraph.id.clone(),
                story.clone(),
                environment_ids.clone(),
                text,
                layout,
            ));
        }

        let mut pagination_items = Vec::with_capacity(ordered_inputs.len());
        let mut paragraph_index = 0;
        for item in ordered_inputs {
            match item {
                LayoutInput::ExplicitBreak => {
                    pagination_items.push(PaginationItem::ExplicitPageBreak);
                }
                LayoutInput::Paragraph { .. } => {
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
        let environment_ranges = collect_environment_ranges(&pages, &paragraphs, &paragraph_lookup);
        Ok(DocumentLayoutSnapshot {
            session_id: None,
            document_generation,
            font_generation: catalog.generation(),
            default_font_size_centipoints: base_style.font_size_centipoints,
            paragraph_lookup,
            paragraphs,
            pages,
            environment_ranges,
        })
    }
}

fn collect_environment_ranges(
    pages: &[PageLayoutSnapshot],
    paragraphs: &[LaidOutParagraph],
    paragraph_lookup: &HashMap<NodeId, usize>,
) -> Vec<PageEnvironmentRange> {
    let mut ranges = HashMap::<(NodeId, usize), (f64, f64)>::new();
    for (page_index, page) in pages.iter().enumerate() {
        for fragment in &page.layout.fragments {
            let Some(&paragraph_index) = paragraph_lookup.get(&fragment.paragraph_id) else {
                continue;
            };
            let Some(paragraph) = paragraphs.get(paragraph_index) else {
                continue;
            };
            let Some(first_line) = paragraph
                .line_locations
                .get(fragment.line_range.start)
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let Some(last_line) = fragment
                .line_range
                .end
                .checked_sub(1)
                .and_then(|index| paragraph.line_locations.get(index))
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let Some(line) = paragraph
                .layout
                .paragraph
                .lines
                .get(fragment.line_range.end.saturating_sub(1))
            else {
                continue;
            };
            let y0_pt = first_line.y0_pt;
            let y1_pt = last_line.y0_pt + f64::from(line.metrics.height_pt);
            for environment_id in paragraph.environment_ids.iter() {
                ranges
                    .entry((environment_id.clone(), page_index))
                    .and_modify(|range| {
                        range.0 = range.0.min(y0_pt);
                        range.1 = range.1.max(y1_pt);
                    })
                    .or_insert((y0_pt, y1_pt));
            }
        }
    }
    let mut result = ranges
        .into_iter()
        .filter_map(|((environment_id, page_index), (y0_pt, y1_pt))| {
            let page = pages.get(page_index)?;
            Some(PageEnvironmentRange {
                environment_id,
                page_index,
                x0_pt: page.margin_left_pt,
                y0_pt,
                x1_pt: page.width_pt - page.margin_right_pt,
                y1_pt,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|range| (range.page_index, range.environment_id.clone()));
    result
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

    /// 复制已排序的本机字体族名称，供命令选择器创建短生命周期候选快照。
    pub fn font_family_names(&self) -> Vec<String> {
        self.catalog
            .families()
            .iter()
            .map(|family| family.name.clone())
            .collect()
    }

    /// 当前 ICU Unicode 数据定义的脚本组，供文档默认字体选择器展示。
    pub fn font_script_choices(&self) -> Vec<(String, String)> {
        caret_layout::font_selection::unicode_script_choices()
    }
}

enum LayoutInput<'a> {
    Paragraph {
        paragraph: &'a Paragraph,
        story: StoryId,
        environment_ids: Vec<NodeId>,
    },
    ExplicitBreak,
}

fn collect_blocks<'a>(
    blocks: &'a [Block],
    ordered: &mut Vec<LayoutInput<'a>>,
    environment_ids: &mut Vec<NodeId>,
) -> Result<(), DocumentLayoutError> {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => ordered.push(LayoutInput::Paragraph {
                paragraph,
                story: StoryId::Body,
                environment_ids: environment_ids.clone(),
            }),
            Block::Environment(environment) => {
                environment_ids.push(environment.id.clone());
                if let Some(title) = &environment.title {
                    ordered.push(LayoutInput::Paragraph {
                        paragraph: title,
                        story: StoryId::EnvironmentTitle(environment.id.clone()),
                        environment_ids: environment_ids.clone(),
                    });
                }
                collect_blocks(&environment.children, ordered, environment_ids)?;
                environment_ids.pop();
            }
            Block::ExplicitBreak(page_break) => {
                if !page_break.kind.is_empty() && page_break.kind != "page" {
                    return Err(DocumentLayoutError::UnknownBreakKind(
                        page_break.kind.clone(),
                    ));
                }
                ordered.push(LayoutInput::ExplicitBreak);
            }
            Block::Opaque(_) | Block::Equation(_) | Block::Align(_) => {
                return Err(DocumentLayoutError::UnsupportedContent);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
