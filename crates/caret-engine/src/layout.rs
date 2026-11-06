//! 组合文档、字体塑形、段落行布局与分页，生成供视图消费的只读快照。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use crate::{
    Affinity, MathEditTarget, MathPositionAffinity, MathPositionHost, MathTextFlowPath,
    MathTextPosition, Pos, Selection, Session, StoryId,
};
use caret_doc::{
    Block, Document, DocumentUnits, FlowContent, MathContent, MathFlow, MathPathStep, NodeId,
    Paragraph, ParagraphContent,
};
use caret_layout::{
    FontCatalog, FontCatalogError, HitAffinity, InlineObjectLayout, PageLayout, PaginationItem,
    ParagraphLayoutOptions, ParagraphLayouter, ParagraphPaginationInput, ParleyParagraphLayout,
    PositionedGlyphLayoutItem, ResolvedDocumentTextSettings, ResolvedParagraphStyle, ShapingStyle,
    TextIslandLayout, TextIslandLayouter, VisualAffinity, paginate_paragraphs,
    paragraph_has_empty_line, resolve_paragraph_style,
};
use std::{collections::HashMap, ops::Range, sync::Arc};

const UTF16_CHECKPOINT_STRIDE_BYTES: usize = 64;
const LINE_GRID_LATIN_SAMPLE: char = 'M';
// 零推进行内文本岛使用独立的透明推进，保留可与相邻正文区分的点击槽位。
const EMPTY_INLINE_MATH_CARET_ADVANCE_PT: f32 = 1.5;
// 零推进数学文本保持 caret 几何不变，只扩展命中区的最小页面高度。
const ZERO_ADVANCE_MATH_HIT_HEIGHT_PT: f64 = 2.25;

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
    Math(String),
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
            Self::Math(error) => write!(formatter, "layout.math: {error}"),
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
    source_paragraph: Arc<Paragraph>,
    resolved_style: ResolvedParagraphStyle,
    pub text: Arc<str>,
    pub layout: Arc<ParleyParagraphLayout>,
    /// 行内 MathBox 布局与 Parley inline-box ID 按顺序一一对应。
    pub inline_math_layouts: Arc<[Arc<caret_math::layout::MathLayout>]>,
    /// 行内 MathBox 与文档 InlineMath 稳定 ID 按顺序一一对应。
    pub(crate) inline_math_ids: Arc<[NodeId]>,
    /// 行内公式对象在宿主段落逻辑文本中的原子源范围。
    pub inline_math_source_ranges: Arc<[Range<usize>]>,
    /// 行内公式 token 到同一布局代际 Parley 文本岛 sidecar 的映射。
    pub(crate) inline_math_text_layouts: Arc<[HashMap<u64, MathTextSidecar>]>,
    utf16_checkpoints: Arc<[(usize, usize)]>,
    line_locations: Vec<Option<CaretLineLocation>>,
}

#[derive(Debug, Clone, Copy)]
struct CaretLineLocation {
    page_index: usize,
    y0_pt: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct MathTextSidecar {
    pub(crate) layout: Arc<TextIslandLayout>,
    pub(crate) color: Option<caret_math::layout::MathColor>,
    pub(crate) target: MathEditTarget,
    pub(crate) host: MathPositionHost,
    source_spans: Vec<MathTextSourceSpan>,
    empty_caret_metrics: Option<(f32, f32)>,
    fallback_font_size_centipoints: u16,
}

#[derive(Debug, Clone)]
struct MathTextSourceSpan {
    range: std::ops::Range<usize>,
    flow_offset: usize,
    path: MathTextFlowPath,
}

#[derive(Clone, Copy)]
enum MathTextMoveRequest {
    Visual(bool),
    Vertical(isize, Option<f32>),
    LineEdge(bool),
}

impl MathTextSidecar {
    fn font_size_centipoints_at(&self, offset: usize, visual_affinity: VisualAffinity) -> u16 {
        let mut upstream = None;
        let mut downstream = None;
        for line in self.layout.layout.lines() {
            for item in line.items() {
                let caret_layout::PositionedGlyphLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let range = glyph_run.run().text_range();
                let Some(size) = font_size_centipoints(glyph_run.run().font_size()) else {
                    continue;
                };
                if range.start < offset && offset < range.end {
                    return size;
                }
                if range.end == offset {
                    upstream = Some(size);
                }
                if range.start == offset {
                    downstream = Some(size);
                }
            }
        }
        match visual_affinity {
            VisualAffinity::Upstream => upstream.or(downstream),
            VisualAffinity::Downstream => downstream.or(upstream),
        }
        .unwrap_or(self.fallback_font_size_centipoints)
    }

    fn offset_at_position(&self, position: &MathTextPosition) -> Option<usize> {
        let downstream = position.affinity == MathPositionAffinity::Downstream;
        let matches_position = |span: &&MathTextSourceSpan| {
            if span.path != position.path || position.offset < span.flow_offset {
                return false;
            }
            position.offset
                <= span
                    .flow_offset
                    .saturating_add(span.range.end - span.range.start)
        };
        let span = if downstream {
            self.source_spans
                .iter()
                .find(matches_position)
                .filter(|span| {
                    position.offset
                        < span
                            .flow_offset
                            .saturating_add(span.range.end.saturating_sub(span.range.start))
                })
                .or_else(|| {
                    self.source_spans.iter().find(|span| {
                        span.path == position.path
                            && span.range.start == span.range.end
                            && span.flow_offset == position.offset
                    })
                })
                .or_else(|| self.source_spans.iter().rev().find(matches_position))
        } else {
            self.source_spans
                .iter()
                .rev()
                .find(matches_position)
                .filter(|span| position.offset > span.flow_offset)
                .or_else(|| {
                    self.source_spans.iter().rev().find(|span| {
                        span.path == position.path
                            && span.range.start == span.range.end
                            && span.flow_offset == position.offset
                    })
                })
                .or_else(|| self.source_spans.iter().find(matches_position))
        }?;
        span.range
            .start
            .checked_add(position.offset.checked_sub(span.flow_offset)?)
    }

    fn position_at_offset(
        &self,
        offset: usize,
        affinity: MathPositionAffinity,
    ) -> Option<MathTextPosition> {
        let downstream = affinity == MathPositionAffinity::Downstream;
        let span = if downstream {
            self.source_spans
                .iter()
                .find(|span| span.range.start <= offset && offset < span.range.end)
                .or_else(|| {
                    self.source_spans.iter().find(|span| {
                        span.range.start == span.range.end && span.range.start == offset
                    })
                })
                .or_else(|| {
                    self.source_spans
                        .last()
                        .filter(|span| span.range.end == offset)
                })
        } else {
            self.source_spans
                .iter()
                .rev()
                .find(|span| span.range.start < offset && offset <= span.range.end)
                .or_else(|| {
                    self.source_spans.iter().rev().find(|span| {
                        span.range.start == span.range.end && span.range.start == offset
                    })
                })
                .or_else(|| {
                    self.source_spans
                        .first()
                        .filter(|span| span.range.start == offset)
                })
        }?;
        let local_offset = span
            .flow_offset
            .checked_add(offset.checked_sub(span.range.start)?)?;
        Some(MathTextPosition::new(
            span.path.clone(),
            local_offset,
            affinity,
        ))
    }
}

impl LaidOutParagraph {
    fn new(input: LaidOutParagraphInput) -> Self {
        let mut utf16_checkpoints = vec![(0, 0)];
        let mut utf16_offset = 0;
        let mut last_checkpoint = 0;
        for (byte_offset, character) in input.text.char_indices() {
            if byte_offset.saturating_sub(last_checkpoint) >= UTF16_CHECKPOINT_STRIDE_BYTES {
                utf16_checkpoints.push((byte_offset, utf16_offset));
                last_checkpoint = byte_offset;
            }
            utf16_offset += character.len_utf16();
        }
        let line_locations = vec![None; input.layout.paragraph.lines.len()];
        Self {
            id: input.source_paragraph.id.clone(),
            story: input.story,
            environment_ids: Arc::from(input.environment_ids),
            source_paragraph: Arc::new(input.source_paragraph),
            resolved_style: input.resolved_style,
            text: Arc::from(input.text),
            layout: Arc::new(input.layout),
            inline_math_layouts: Arc::from(input.inline_math_layouts),
            inline_math_ids: Arc::from(input.inline_math_ids),
            inline_math_source_ranges: Arc::from(input.inline_math_source_ranges),
            inline_math_text_layouts: Arc::from(input.inline_math_text_layouts),
            utf16_checkpoints: Arc::from(utf16_checkpoints),
            line_locations,
        }
    }

    fn reused_for_layout(&self, story: StoryId, environment_ids: &[NodeId]) -> Self {
        Self {
            id: self.id.clone(),
            story,
            environment_ids: Arc::from(environment_ids),
            source_paragraph: Arc::clone(&self.source_paragraph),
            resolved_style: self.resolved_style,
            text: Arc::clone(&self.text),
            layout: Arc::clone(&self.layout),
            inline_math_layouts: Arc::clone(&self.inline_math_layouts),
            inline_math_ids: Arc::clone(&self.inline_math_ids),
            inline_math_source_ranges: Arc::clone(&self.inline_math_source_ranges),
            inline_math_text_layouts: Arc::clone(&self.inline_math_text_layouts),
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

struct LaidOutParagraphInput {
    source_paragraph: Paragraph,
    story: StoryId,
    environment_ids: Vec<NodeId>,
    resolved_style: ResolvedParagraphStyle,
    text: String,
    layout: ParleyParagraphLayout,
    inline_math_layouts: Vec<Arc<caret_math::layout::MathLayout>>,
    inline_math_ids: Vec<NodeId>,
    inline_math_source_ranges: Vec<Range<usize>>,
    inline_math_text_layouts: Vec<HashMap<u64, MathTextSidecar>>,
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

/// 块级环境在单页上的内容范围，由其标题、段落和显示公式的实际分页片段合并得到。
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
    content_width_pt: f32,
    resolved_text_settings: ResolvedDocumentTextSettings,
    base_style: ShapingStyle,
    pub paragraphs: Vec<LaidOutParagraph>,
    pub pages: Vec<PageLayoutSnapshot>,
    pub environment_ranges: Vec<PageEnvironmentRange>,
    /// 已排版的块级公式，页面片段通过 ID 引用同一份 MathBox。
    pub math_blocks: HashMap<NodeId, Arc<caret_math::layout::MathLayout>>,
    /// 块级公式 token 与正文 Parley sidecar 成对保存，避免布局缓存脱离字体数据。
    pub(crate) math_block_text_layouts: HashMap<NodeId, HashMap<u64, MathTextSidecar>>,
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

/// 页面内命中的公式正文文本位置；结构路径和宿主身份来自同一布局代际。
#[derive(Debug, Clone, PartialEq)]
pub struct PageMathTextHit {
    pub session_id: Option<NodeId>,
    pub document_generation: u64,
    pub font_generation: u64,
    pub target: MathEditTarget,
    pub host: MathPositionHost,
    pub position: MathTextPosition,
    pub visual_affinity: VisualAffinity,
    pub is_zero_width: bool,
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

    /// 命中公式中的正文文本岛，并从同一快照返回宿主、TextFlow 路径与光标几何。
    pub fn hit_test_math_text(
        &self,
        page_index: usize,
        x_pt: f64,
        y_pt: f64,
    ) -> Option<PageMathTextHit> {
        if !x_pt.is_finite() || !y_pt.is_finite() {
            return None;
        }
        let page = self.pages.get(page_index)?;
        if x_pt < 0.0 || y_pt < 0.0 || x_pt > page.width_pt || y_pt > page.height_pt {
            return None;
        }
        let annotate_hit = |mut hit: PageMathTextHit| {
            hit.session_id = self.session_id.clone();
            hit.document_generation = self.document_generation;
            hit.font_generation = self.font_generation;
            hit
        };
        let mut zero_width_hit = None;

        for fragment in &page.layout.math_fragments {
            let Some(layout) = self.math_blocks.get(&fragment.math_id) else {
                continue;
            };
            let Some(sidecars) = self.math_block_text_layouts.get(&fragment.math_id) else {
                continue;
            };
            if sidecars.is_empty() {
                continue;
            }
            let object_x_pt = page.margin_left_pt + f64::from(fragment.x_offset_pt);
            let object_baseline_y_pt =
                page.margin_top_pt + f64::from(fragment.y_offset_pt) + layout.ascent;
            if let Some(hit) = hit_math_text_items(
                page_index,
                x_pt,
                y_pt,
                object_x_pt,
                object_baseline_y_pt,
                &layout.items,
                sidecars,
            ) {
                let hit = annotate_hit(hit);
                if hit.is_zero_width {
                    zero_width_hit = Some(hit);
                } else {
                    return Some(hit);
                }
            }
        }

        for fragment in &page.layout.fragments {
            let Some(&paragraph_index) = self.paragraph_lookup.get(&fragment.paragraph_id) else {
                continue;
            };
            let Some(paragraph) = self.paragraphs.get(paragraph_index) else {
                continue;
            };
            let mut page_line_top_pt = page.margin_top_pt + f64::from(fragment.y_offset_pt);
            for line_index in fragment.line_range.clone() {
                let line_box = paragraph.layout.paragraph.lines.get(line_index)?;
                let line_height_pt = f64::from(line_box.metrics.height_pt);
                let Some(line) = paragraph.layout.layout.get(line_index) else {
                    page_line_top_pt += line_height_pt;
                    continue;
                };
                let raw_top_pt = f64::from(*paragraph.layout.parley_line_tops_pt.get(line_index)?);
                let baseline_adjustment_pt =
                    f64::from(*paragraph.layout.baseline_adjustments_pt.get(line_index)?);
                for item in line.items() {
                    let PositionedGlyphLayoutItem::InlineBox(inline_box) = item else {
                        continue;
                    };
                    let Ok(inline_index) = usize::try_from(inline_box.id) else {
                        continue;
                    };
                    let Some(math_layout) = paragraph.inline_math_layouts.get(inline_index) else {
                        continue;
                    };
                    let Some(sidecars) = paragraph.inline_math_text_layouts.get(inline_index)
                    else {
                        continue;
                    };
                    let object_x_pt = page.margin_left_pt + f64::from(inline_box.x);
                    let object_baseline_y_pt = page_line_top_pt
                        + f64::from(inline_box.y)
                        + f64::from(inline_box.baseline.unwrap_or(inline_box.height))
                        - raw_top_pt
                        + baseline_adjustment_pt;
                    if let Some(hit) = hit_math_text_items(
                        page_index,
                        x_pt,
                        y_pt,
                        object_x_pt,
                        object_baseline_y_pt,
                        &math_layout.items,
                        sidecars,
                    ) {
                        let hit = annotate_hit(hit);
                        if hit.is_zero_width {
                            zero_width_hit = Some(hit);
                        } else {
                            return Some(hit);
                        }
                    }
                }
                page_line_top_pt += line_height_pt;
            }
        }
        zero_width_hit
    }

    /// 根据已提交 TextFlow 位置返回同代文本岛 Parley 几何中的数学光标。
    pub fn math_text_caret(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
    ) -> Option<(PageCaretGeometry, u16)> {
        for (page_index, page) in self.pages.iter().enumerate() {
            for fragment in &page.layout.math_fragments {
                let Some(layout) = self.math_blocks.get(&fragment.math_id) else {
                    continue;
                };
                let Some(sidecars) = self.math_block_text_layouts.get(&fragment.math_id) else {
                    continue;
                };
                let geometry = MathTextItemGeometry {
                    page_index,
                    object_x_pt: page.margin_left_pt + f64::from(fragment.x_offset_pt),
                    object_baseline_y_pt: page.margin_top_pt
                        + f64::from(fragment.y_offset_pt)
                        + layout.ascent,
                    items: &layout.items,
                    sidecars,
                };
                if let Some(caret) =
                    math_text_caret_in_items(&geometry, target, position, visual_affinity)
                {
                    return Some(caret);
                }
            }

            for fragment in &page.layout.fragments {
                let Some(&paragraph_index) = self.paragraph_lookup.get(&fragment.paragraph_id)
                else {
                    continue;
                };
                let Some(paragraph) = self.paragraphs.get(paragraph_index) else {
                    continue;
                };
                let mut line_top_pt = page.margin_top_pt + f64::from(fragment.y_offset_pt);
                for line_index in fragment.line_range.clone() {
                    let Some(line_box) = paragraph.layout.paragraph.lines.get(line_index) else {
                        continue;
                    };
                    let line_height_pt = f64::from(line_box.metrics.height_pt);
                    let Some(line) = paragraph.layout.layout.get(line_index) else {
                        line_top_pt += line_height_pt;
                        continue;
                    };
                    let Some(raw_top_pt) = paragraph.layout.parley_line_tops_pt.get(line_index)
                    else {
                        line_top_pt += line_height_pt;
                        continue;
                    };
                    let Some(baseline_adjustment_pt) =
                        paragraph.layout.baseline_adjustments_pt.get(line_index)
                    else {
                        line_top_pt += line_height_pt;
                        continue;
                    };
                    for item in line.items() {
                        let PositionedGlyphLayoutItem::InlineBox(inline_box) = item else {
                            continue;
                        };
                        let Ok(inline_index) = usize::try_from(inline_box.id) else {
                            continue;
                        };
                        let Some(math_layout) = paragraph.inline_math_layouts.get(inline_index)
                        else {
                            continue;
                        };
                        let Some(sidecars) = paragraph.inline_math_text_layouts.get(inline_index)
                        else {
                            continue;
                        };
                        let object_baseline_y_pt = line_top_pt
                            + f64::from(inline_box.y)
                            + f64::from(inline_box.baseline.unwrap_or(inline_box.height))
                            - f64::from(*raw_top_pt)
                            + f64::from(*baseline_adjustment_pt);
                        let geometry = MathTextItemGeometry {
                            page_index,
                            object_x_pt: page.margin_left_pt + f64::from(inline_box.x),
                            object_baseline_y_pt,
                            items: &math_layout.items,
                            sidecars,
                        };
                        if let Some(caret) =
                            math_text_caret_in_items(&geometry, target, position, visual_affinity)
                        {
                            return Some(caret);
                        }
                    }
                    line_top_pt += line_height_pt;
                }
            }
        }
        None
    }

    /// 根据已提交 TextFlow 位置返回数学光标页面几何。
    pub fn math_text_caret_geometry(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
    ) -> Option<PageCaretGeometry> {
        self.math_text_caret(target, position, visual_affinity)
            .map(|(caret, _)| caret)
    }

    /// 返回同一数学文本岛中端点之间、由该代 Parley 布局生成的选区矩形。
    pub fn math_text_selection_geometry(
        &self,
        target: &MathEditTarget,
        anchor: &MathTextPosition,
        focus: &MathTextPosition,
    ) -> Option<Vec<PageSelectionRect>> {
        self.math_text_range_geometry(target, anchor, focus, false)
    }

    /// 返回数学文本输入法预编辑范围的字体下划线几何。
    pub fn math_text_composition_geometry(
        &self,
        target: &MathEditTarget,
        anchor: &MathTextPosition,
        focus: &MathTextPosition,
    ) -> Option<Vec<PageSelectionRect>> {
        self.math_text_range_geometry(target, anchor, focus, true)
    }

    fn math_text_range_geometry(
        &self,
        target: &MathEditTarget,
        anchor: &MathTextPosition,
        focus: &MathTextPosition,
        composition: bool,
    ) -> Option<Vec<PageSelectionRect>> {
        for (page_index, page) in self.pages.iter().enumerate() {
            for fragment in &page.layout.math_fragments {
                let Some(layout) = self.math_blocks.get(&fragment.math_id) else {
                    continue;
                };
                let Some(sidecars) = self.math_block_text_layouts.get(&fragment.math_id) else {
                    continue;
                };
                let geometry = MathTextItemGeometry {
                    page_index,
                    object_x_pt: page.margin_left_pt + f64::from(fragment.x_offset_pt),
                    object_baseline_y_pt: page.margin_top_pt
                        + f64::from(fragment.y_offset_pt)
                        + layout.ascent,
                    items: &layout.items,
                    sidecars,
                };
                if let Some(rectangles) =
                    math_text_selection_in_items(&geometry, target, anchor, focus, composition)
                {
                    return Some(rectangles);
                }
            }
            for fragment in &page.layout.fragments {
                let Some(&paragraph_index) = self.paragraph_lookup.get(&fragment.paragraph_id)
                else {
                    continue;
                };
                let Some(paragraph) = self.paragraphs.get(paragraph_index) else {
                    continue;
                };
                let mut line_top_pt = page.margin_top_pt + f64::from(fragment.y_offset_pt);
                for line_index in fragment.line_range.clone() {
                    let Some(line_box) = paragraph.layout.paragraph.lines.get(line_index) else {
                        continue;
                    };
                    let line_height_pt = f64::from(line_box.metrics.height_pt);
                    let Some(line) = paragraph.layout.layout.get(line_index) else {
                        line_top_pt += line_height_pt;
                        continue;
                    };
                    let Some(raw_top_pt) = paragraph.layout.parley_line_tops_pt.get(line_index)
                    else {
                        line_top_pt += line_height_pt;
                        continue;
                    };
                    let Some(baseline_adjustment_pt) =
                        paragraph.layout.baseline_adjustments_pt.get(line_index)
                    else {
                        line_top_pt += line_height_pt;
                        continue;
                    };
                    for item in line.items() {
                        let PositionedGlyphLayoutItem::InlineBox(inline_box) = item else {
                            continue;
                        };
                        let Ok(inline_index) = usize::try_from(inline_box.id) else {
                            continue;
                        };
                        let Some(math_layout) = paragraph.inline_math_layouts.get(inline_index)
                        else {
                            continue;
                        };
                        let Some(sidecars) = paragraph.inline_math_text_layouts.get(inline_index)
                        else {
                            continue;
                        };
                        let object_baseline_y_pt = line_top_pt
                            + f64::from(inline_box.y)
                            + f64::from(inline_box.baseline.unwrap_or(inline_box.height))
                            - f64::from(*raw_top_pt)
                            + f64::from(*baseline_adjustment_pt);
                        let geometry = MathTextItemGeometry {
                            page_index,
                            object_x_pt: page.margin_left_pt + f64::from(inline_box.x),
                            object_baseline_y_pt,
                            items: &math_layout.items,
                            sidecars,
                        };
                        if let Some(rectangles) = math_text_selection_in_items(
                            &geometry,
                            target,
                            anchor,
                            focus,
                            composition,
                        ) {
                            return Some(rectangles);
                        }
                    }
                    line_top_pt += line_height_pt;
                }
            }
        }
        None
    }

    /// 在同一公式文本岛内按 Parley 视觉顺序移动数学光标。
    pub fn move_math_text_visual(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        forward: bool,
    ) -> Option<(MathTextPosition, VisualAffinity)> {
        self.move_math_text_cursor(
            target,
            position,
            visual_affinity,
            MathTextMoveRequest::Visual(forward),
        )
        .map(|(position, affinity, _)| (position, affinity))
    }

    /// 在同一公式文本岛内按保留的目标 x 上下移动数学光标。
    pub fn move_math_text_vertical(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        line_delta: isize,
        preferred_x: Option<f32>,
    ) -> Option<(MathTextPosition, VisualAffinity, f32)> {
        let (position, affinity, preferred_x) = self.move_math_text_cursor(
            target,
            position,
            visual_affinity,
            MathTextMoveRequest::Vertical(line_delta, preferred_x),
        )?;
        Some((position, affinity, preferred_x?))
    }

    /// 将同一公式文本岛内的数学光标移到视觉行首尾。
    pub fn move_math_text_line_edge(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        end: bool,
    ) -> Option<(MathTextPosition, VisualAffinity)> {
        self.move_math_text_cursor(
            target,
            position,
            visual_affinity,
            MathTextMoveRequest::LineEdge(end),
        )
        .map(|(position, affinity, _)| (position, affinity))
    }

    fn move_math_text_cursor(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        movement: MathTextMoveRequest,
    ) -> Option<(MathTextPosition, VisualAffinity, Option<f32>)> {
        for sidecar in self
            .math_block_text_layouts
            .values()
            .flat_map(HashMap::values)
            .chain(
                self.paragraphs
                    .iter()
                    .flat_map(|paragraph| paragraph.inline_math_text_layouts.iter())
                    .flat_map(HashMap::values),
            )
        {
            if &sidecar.target != target {
                continue;
            }
            let Some(offset) = sidecar.offset_at_position(position) else {
                continue;
            };
            let Some(cursor) = sidecar.layout.cursor_at(offset, visual_affinity) else {
                continue;
            };
            let Some((next_cursor, preferred_x)) = (match movement {
                MathTextMoveRequest::Visual(forward) => sidecar
                    .layout
                    .move_visual(cursor, forward)
                    .map(|cursor| (cursor, None)),
                MathTextMoveRequest::Vertical(line_delta, preferred_x) => sidecar
                    .layout
                    .move_vertical(cursor, line_delta, preferred_x)
                    .map(|(cursor, preferred_x)| (cursor, Some(preferred_x))),
                MathTextMoveRequest::LineEdge(end) => sidecar
                    .layout
                    .move_to_visual_line_edge(cursor, end)
                    .map(|cursor| (cursor, None)),
            }) else {
                continue;
            };
            let logical_affinity = match sidecar
                .layout
                .logical_affinity_at(next_cursor.byte_index(), next_cursor.affinity())?
            {
                HitAffinity::Upstream => MathPositionAffinity::Upstream,
                HitAffinity::Downstream => MathPositionAffinity::Downstream,
            };
            let next_position =
                sidecar.position_at_offset(next_cursor.byte_index(), logical_affinity)?;
            return Some((next_position, next_cursor.affinity(), preferred_x));
        }
        None
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
    DocumentUnits::points_to_font_size_centipoints(font_size_pt)
}

fn visible_font_size_centipoints(font_size_pt: f32) -> Option<u16> {
    if !font_size_pt.is_finite() || font_size_pt <= 0.0 {
        return None;
    }
    let centipoints = (f64::from(font_size_pt) * 100.0)
        .round()
        .clamp(1.0, f64::from(u16::MAX));
    #[expect(
        clippy::cast_possible_truncation,
        reason = "显示投影先将有限字号夹取到 u16 百分之一 pt 范围"
    )]
    Some(centipoints as u16)
}

/// 可复用塑形器；调用方跨文档代际复用字体目录，避免按段落扫描系统字体。
#[derive(Default)]
pub struct DocumentLayoutService {
    layouter: ParagraphLayouter,
    text_island_layouter: TextIslandLayouter,
    line_grid_metrics_cache: Option<LineGridMetricsCache>,
    math_layout_engine: Option<caret_math::layout::MathLayoutEngine>,
}

#[derive(Debug, Clone, PartialEq)]
struct LineGridMetricsCache {
    font_generation: u64,
    settings: ResolvedDocumentTextSettings,
    style: ShapingStyle,
    metrics: caret_layout::FontLineMetrics,
}

impl DocumentLayoutService {
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
        let current_settings = ResolvedDocumentTextSettings::from_document_settings(
            &session.document().global_settings,
        )
        .map_err(|error| DocumentLayoutError::FontPreferences(error.to_string()))?;
        let current_content_width_pt = session
            .document()
            .page_sections
            .first()
            .and_then(|section| section.geometry().ok())
            .and_then(|geometry| geometry.content_size_pt().ok())
            .map(|(width, _)| width as f32);
        if previous.session_id.as_ref() != Some(session.session_id())
            || previous.font_generation != catalog.generation()
            || previous.resolved_text_settings != current_settings
            || previous.base_style != *base_style
            || Some(previous.content_width_pt) != current_content_width_pt
        {
            return self.layout_session(session, catalog, base_style);
        }
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
        if document_contains_math(&document.body) && self.math_layout_engine.is_none() {
            self.math_layout_engine = Some(
                caret_math::layout::MathLayoutEngine::new()
                    .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
            );
        }
        let mut ordered_inputs = Vec::new();
        {
            let mut math_context = MathLayoutContext {
                engine: self.math_layout_engine.as_ref(),
                catalog,
                settings: &text_settings,
                base_style,
                text_layouter: &mut self.text_island_layouter,
                font_size_centipoints: base_style.font_size_centipoints,
                content_width_pt: content_width_pt as f32,
            };
            collect_blocks(
                &document.body,
                &mut ordered_inputs,
                &mut Vec::new(),
                &mut math_context,
            )?;
        }

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
                && previous_paragraph.source_paragraph.as_ref() == *paragraph
                && previous_paragraph.resolved_style == style
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
            let inline_layouts = if let Some(engine) = self.math_layout_engine.as_ref() {
                let mut inline_context = MathLayoutContext {
                    engine: Some(engine),
                    catalog,
                    settings: &text_settings,
                    base_style,
                    text_layouter: &mut self.text_island_layouter,
                    font_size_centipoints: base_style.font_size_centipoints,
                    content_width_pt: content_width_pt as f32,
                };
                collect_inline_math_layouts(paragraph, &mut inline_context)?
            } else {
                InlineMathLayouts {
                    objects: Vec::new(),
                    layouts: Vec::new(),
                    ids: Vec::new(),
                    text_layouts: Vec::new(),
                }
            };
            let InlineMathLayouts {
                objects: inline_object_layouts,
                layouts: inline_math_layouts,
                ids: inline_math_ids,
                text_layouts: inline_math_text_layouts,
            } = inline_layouts;
            let (text, layout) = self
                .layouter
                .layout_with_inline_objects(
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
                    &inline_object_layouts,
                )
                .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
            resolved_styles.push(style);
            paragraphs.push(LaidOutParagraph::new(LaidOutParagraphInput {
                source_paragraph: (*paragraph).clone(),
                story: story.clone(),
                environment_ids: environment_ids.clone(),
                resolved_style: style,
                text,
                layout,
                inline_math_layouts,
                inline_math_ids,
                inline_math_source_ranges: inline_object_layouts
                    .iter()
                    .map(|object| object.source_bytes.clone())
                    .collect(),
                inline_math_text_layouts,
            }));
        }

        let mut pagination_items = Vec::with_capacity(ordered_inputs.len());
        let mut paragraph_index = 0;
        let math_blocks = ordered_inputs
            .iter()
            .filter_map(|item| match item {
                LayoutInput::MathBlock {
                    math_id, layout, ..
                } => Some((math_id.clone(), Arc::clone(layout))),
                LayoutInput::Paragraph { .. } | LayoutInput::ExplicitBreak => None,
            })
            .collect::<HashMap<_, _>>();
        let math_block_environment_ids = ordered_inputs
            .iter()
            .filter_map(|item| match item {
                LayoutInput::MathBlock {
                    math_id,
                    environment_ids,
                    ..
                } => Some((math_id.clone(), environment_ids.clone())),
                LayoutInput::Paragraph { .. } | LayoutInput::ExplicitBreak => None,
            })
            .collect::<HashMap<_, _>>();
        let math_block_text_layouts = ordered_inputs
            .iter()
            .filter_map(|item| match item {
                LayoutInput::MathBlock {
                    math_id,
                    text_layouts,
                    ..
                } => Some((math_id.clone(), text_layouts.clone())),
                LayoutInput::Paragraph { .. } | LayoutInput::ExplicitBreak => None,
            })
            .collect::<HashMap<_, _>>();
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
                LayoutInput::MathBlock {
                    math_id,
                    x_offset_pt,
                    width_pt,
                    height_pt,
                    ..
                } => {
                    let math_id_ref = math_blocks
                        .get_key_value(&math_id)
                        .map(|(id, _)| id)
                        .ok_or_else(|| {
                            DocumentLayoutError::InvalidDocument(
                                "missing block math layout".to_owned(),
                            )
                        })?;
                    pagination_items.push(PaginationItem::MathBlock(
                        caret_layout::MathBlockPaginationInput {
                            math_id: math_id_ref,
                            x_offset_pt,
                            width_pt,
                            height_pt,
                        },
                    ));
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
        let environment_ranges = collect_environment_ranges(
            &pages,
            &paragraphs,
            &paragraph_lookup,
            &math_block_environment_ids,
        );
        Ok(DocumentLayoutSnapshot {
            session_id: None,
            document_generation,
            font_generation: catalog.generation(),
            default_font_size_centipoints: base_style.font_size_centipoints,
            content_width_pt: content_width_pt as f32,
            resolved_text_settings: text_settings,
            base_style: base_style.clone(),
            paragraph_lookup,
            paragraphs,
            pages,
            environment_ranges,
            math_blocks,
            math_block_text_layouts,
        })
    }
}

fn collect_environment_ranges(
    pages: &[PageLayoutSnapshot],
    paragraphs: &[LaidOutParagraph],
    paragraph_lookup: &HashMap<NodeId, usize>,
    math_block_environment_ids: &HashMap<NodeId, Vec<NodeId>>,
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
        for fragment in &page.layout.math_fragments {
            let Some(environment_ids) = math_block_environment_ids.get(&fragment.math_id) else {
                continue;
            };
            let y0_pt = page.margin_top_pt + f64::from(fragment.y_offset_pt);
            let y1_pt = y0_pt + f64::from(fragment.height_pt);
            for environment_id in environment_ids {
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

struct MathTextItemGeometry<'a> {
    page_index: usize,
    object_x_pt: f64,
    object_baseline_y_pt: f64,
    items: &'a [caret_math::layout::MathItem],
    sidecars: &'a HashMap<u64, MathTextSidecar>,
}

fn math_text_caret_in_items(
    geometry: &MathTextItemGeometry<'_>,
    target: &MathEditTarget,
    position: &MathTextPosition,
    visual_affinity: VisualAffinity,
) -> Option<(PageCaretGeometry, u16)> {
    for item in geometry.items {
        let caret_math::layout::MathItem::TextIsland {
            token,
            x,
            baseline_y,
            ..
        } = item
        else {
            continue;
        };
        let Some(sidecar) = geometry.sidecars.get(token) else {
            continue;
        };
        if &sidecar.target != target {
            continue;
        }
        let Some(offset) = sidecar.offset_at_position(position) else {
            continue;
        };
        let local_cursor_x = if sidecar.layout.text.is_empty() {
            0.0
        } else {
            let Some(bounds) = sidecar
                .layout
                .cursor_geometry_at(offset, visual_affinity, 0.0)
            else {
                continue;
            };
            bounds.x0
        };
        let (ascent_pt, descent_pt) = if sidecar.layout.text.is_empty() {
            sidecar.empty_caret_metrics?
        } else {
            (
                sidecar.layout.metrics.ascent_pt,
                sidecar.layout.metrics.descent_pt,
            )
        };
        let island_x_pt = geometry.object_x_pt + *x + local_cursor_x;
        let island_baseline_y_pt = geometry.object_baseline_y_pt + baseline_y;
        return Some((
            PageCaretGeometry {
                page_index: geometry.page_index,
                x_pt: island_x_pt,
                y0_pt: island_baseline_y_pt - f64::from(ascent_pt),
                y1_pt: island_baseline_y_pt + f64::from(descent_pt),
            },
            sidecar.font_size_centipoints_at(offset, visual_affinity),
        ));
    }
    None
}

fn math_text_selection_in_items(
    geometry: &MathTextItemGeometry<'_>,
    target: &MathEditTarget,
    anchor: &MathTextPosition,
    focus: &MathTextPosition,
    composition: bool,
) -> Option<Vec<PageSelectionRect>> {
    for item in geometry.items {
        let caret_math::layout::MathItem::TextIsland {
            token,
            x,
            baseline_y,
            ..
        } = item
        else {
            continue;
        };
        let Some(sidecar) = geometry.sidecars.get(token) else {
            continue;
        };
        if &sidecar.target != target {
            continue;
        }
        let Some(anchor_offset) = sidecar.offset_at_position(anchor) else {
            continue;
        };
        let Some(focus_offset) = sidecar.offset_at_position(focus) else {
            continue;
        };
        let range = anchor_offset.min(focus_offset)..anchor_offset.max(focus_offset);
        let rectangles = if composition {
            sidecar.layout.composition_geometry(range)?
        } else {
            sidecar.layout.selection_geometry(range)?
        };
        if rectangles.is_empty() {
            return Some(Vec::new());
        }
        let line_baseline = sidecar.layout.layout.lines().next()?.metrics().baseline;
        let island_x_pt = geometry.object_x_pt + *x;
        let island_baseline_y_pt = geometry.object_baseline_y_pt + baseline_y;
        return Some(
            rectangles
                .into_iter()
                .map(|(bounds, _line_index)| PageSelectionRect {
                    page_index: geometry.page_index,
                    x0_pt: island_x_pt + bounds.x0,
                    y0_pt: island_baseline_y_pt + bounds.y0 - f64::from(line_baseline),
                    x1_pt: island_x_pt + bounds.x1,
                    y1_pt: island_baseline_y_pt + bounds.y1 - f64::from(line_baseline),
                })
                .collect(),
        );
    }
    None
}

fn hit_math_text_items(
    page_index: usize,
    page_x_pt: f64,
    page_y_pt: f64,
    object_x_pt: f64,
    object_baseline_y_pt: f64,
    items: &[caret_math::layout::MathItem],
    sidecars: &HashMap<u64, MathTextSidecar>,
) -> Option<PageMathTextHit> {
    let mut zero_width_hit = None;
    for item in items {
        let caret_math::layout::MathItem::TextIsland {
            token,
            x,
            baseline_y,
            ..
        } = item
        else {
            continue;
        };
        let Some(sidecar) = sidecars.get(token) else {
            continue;
        };
        let island_x_pt = object_x_pt + x;
        let island_baseline_y_pt = object_baseline_y_pt + baseline_y;
        if sidecar.layout.text.is_empty() {
            let Some((ascent_pt, descent_pt)) = sidecar.empty_caret_metrics else {
                continue;
            };
            let island_y0_pt = island_baseline_y_pt - f64::from(ascent_pt);
            let island_y1_pt = island_baseline_y_pt + f64::from(descent_pt);
            let (hit_y0_pt, hit_y1_pt) = expand_zero_advance_hit_y(island_y0_pt, island_y1_pt);
            if (page_x_pt - island_x_pt).abs() > 4.0
                || page_y_pt < hit_y0_pt
                || page_y_pt > hit_y1_pt
            {
                continue;
            }
            let Some(position) = sidecar.position_at_offset(0, MathPositionAffinity::Downstream)
            else {
                continue;
            };
            zero_width_hit = Some(PageMathTextHit {
                session_id: None,
                document_generation: 0,
                font_generation: 0,
                target: sidecar.target.clone(),
                host: sidecar.host.clone(),
                position,
                visual_affinity: VisualAffinity::Downstream,
                is_zero_width: true,
                caret: PageCaretGeometry {
                    page_index,
                    x_pt: island_x_pt,
                    y0_pt: island_y0_pt,
                    y1_pt: island_y1_pt,
                },
            });
            continue;
        }
        let Some(line) = sidecar.layout.layout.lines().next() else {
            continue;
        };
        let metrics = sidecar.layout.metrics;
        if metrics.ascent_pt + metrics.descent_pt <= 0.0 {
            continue;
        }
        let island_y0_pt = island_baseline_y_pt - f64::from(metrics.ascent_pt);
        let island_y1_pt = island_baseline_y_pt + f64::from(metrics.descent_pt);
        let zero_advance = metrics.advance_pt <= 0.0;
        let (hit_y0_pt, hit_y1_pt) = if zero_advance {
            expand_zero_advance_hit_y(island_y0_pt, island_y1_pt)
        } else {
            (island_y0_pt, island_y1_pt)
        };
        let outside_x = if zero_advance {
            (page_x_pt - island_x_pt).abs() > 4.0
        } else {
            page_x_pt < island_x_pt || page_x_pt > island_x_pt + f64::from(metrics.advance_pt)
        };
        if !island_x_pt.is_finite()
            || !island_baseline_y_pt.is_finite()
            || outside_x
            || page_y_pt < hit_y0_pt
            || page_y_pt > hit_y1_pt
        {
            continue;
        }
        let local_x = if zero_advance {
            0.0
        } else {
            (page_x_pt - island_x_pt) as f32
        };
        let local_y =
            (page_y_pt - island_baseline_y_pt + f64::from(line.metrics().baseline)) as f32;
        let Some(hit) = sidecar.layout.cursor_from_point(local_x, local_y) else {
            continue;
        };
        let affinity = match hit.logical_affinity {
            HitAffinity::Upstream => MathPositionAffinity::Upstream,
            HitAffinity::Downstream => MathPositionAffinity::Downstream,
        };
        let Some(position) = sidecar.position_at_offset(hit.cursor.byte_index(), affinity) else {
            continue;
        };
        let Some(caret_bounds) = sidecar.layout.cursor_geometry(hit.cursor, 0.0) else {
            continue;
        };
        let text_hit = PageMathTextHit {
            session_id: None,
            document_generation: 0,
            font_generation: 0,
            target: sidecar.target.clone(),
            host: sidecar.host.clone(),
            position,
            visual_affinity: hit.cursor.affinity(),
            is_zero_width: zero_advance,
            caret: PageCaretGeometry {
                page_index,
                x_pt: island_x_pt + caret_bounds.x0,
                y0_pt: island_y0_pt,
                y1_pt: island_y1_pt,
            },
        };
        if zero_advance {
            zero_width_hit = Some(text_hit);
        } else {
            return Some(text_hit);
        }
    }
    zero_width_hit
}

fn expand_zero_advance_hit_y(y0_pt: f64, y1_pt: f64) -> (f64, f64) {
    let extra = (ZERO_ADVANCE_MATH_HIT_HEIGHT_PT - (y1_pt - y0_pt)).max(0.0) / 2.0;
    (y0_pt - extra, y1_pt + extra)
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

fn contains_inline_math(nodes: &[ParagraphContent]) -> bool {
    nodes.iter().any(|node| match node {
        FlowContent::Math(_) => true,
        FlowContent::Environment(environment) => contains_inline_math(&environment.children),
        FlowContent::Text(_) | FlowContent::SoftBreak(_) | FlowContent::Opaque(_) => false,
    })
}

fn math_flow_contains_text(flow: &MathFlow) -> bool {
    flow.content.iter().any(|content| match content {
        MathContent::Text(_) => true,
        MathContent::Math(node) => node.children.iter().any(math_flow_contains_text),
        MathContent::Environment(environment) => math_inline_contains_text(&environment.children),
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => true,
    })
}

fn math_inline_contains_text(nodes: &[FlowContent<caret_doc::MathNode>]) -> bool {
    nodes.iter().any(|node| match node {
        FlowContent::Text(_) => true,
        FlowContent::Math(math) => math.children.iter().any(math_flow_contains_text),
        FlowContent::Environment(environment) => math_inline_contains_text(&environment.children),
        FlowContent::SoftBreak(_) | FlowContent::Opaque(_) => true,
    })
}

#[derive(Debug, Clone)]
struct MathTextScopeHost {
    host: MathPositionHost,
    align_cell: Option<(usize, usize)>,
}

#[derive(Debug, Clone)]
struct MathTextLayoutHost {
    target: MathEditTarget,
    scopes: HashMap<NodeId, MathTextScopeHost>,
}

impl MathTextLayoutHost {
    fn inline(paragraph_id: NodeId, math_id: NodeId, scope_id: NodeId) -> Self {
        Self {
            target: MathEditTarget::Inline {
                paragraph_id,
                math_id: math_id.clone(),
            },
            scopes: HashMap::from([(
                scope_id,
                MathTextScopeHost {
                    host: MathPositionHost::InlineMath { owner_id: math_id },
                    align_cell: None,
                },
            )]),
        }
    }

    fn equation(block_id: NodeId, scope_id: NodeId) -> Self {
        Self {
            target: MathEditTarget::Block {
                block_id: block_id.clone(),
                align: false,
            },
            scopes: HashMap::from([(
                scope_id,
                MathTextScopeHost {
                    host: MathPositionHost::Equation { owner_id: block_id },
                    align_cell: None,
                },
            )]),
        }
    }

    fn align(block_id: NodeId, align: &caret_doc::Align) -> Self {
        let mut scopes = HashMap::new();
        for (row_index, row) in align.rows.iter().enumerate() {
            for (cell_index, cell) in row.cells.iter().enumerate() {
                scopes.insert(
                    cell.root.id.clone(),
                    MathTextScopeHost {
                        host: MathPositionHost::AlignCell {
                            row_id: row.id.clone(),
                            cell_id: cell.root.id.clone(),
                        },
                        align_cell: Some((row_index, cell_index)),
                    },
                );
            }
        }
        Self {
            target: MathEditTarget::Block {
                block_id,
                align: true,
            },
            scopes,
        }
    }

    fn for_scope(&self, scope_id: &NodeId) -> Option<MathTextScopeHost> {
        self.scopes.get(scope_id).cloned()
    }
}

struct EngineMathTextShaper<'a> {
    catalog: &'a mut FontCatalog,
    settings: &'a ResolvedDocumentTextSettings,
    base_style: &'a ShapingStyle,
    text_layouter: &'a mut TextIslandLayouter,
    host: MathTextLayoutHost,
    next_token: u64,
    sidecars: HashMap<u64, MathTextSidecar>,
}

impl<'a> EngineMathTextShaper<'a> {
    fn new(
        catalog: &'a mut FontCatalog,
        settings: &'a ResolvedDocumentTextSettings,
        base_style: &'a ShapingStyle,
        text_layouter: &'a mut TextIslandLayouter,
        host: MathTextLayoutHost,
    ) -> Self {
        Self {
            catalog,
            settings,
            base_style,
            text_layouter,
            host,
            next_token: 0,
            sidecars: HashMap::new(),
        }
    }

    fn into_sidecars(self) -> HashMap<u64, MathTextSidecar> {
        self.sidecars
    }
}

impl caret_math::layout::MathTextShaper for EngineMathTextShaper<'_> {
    fn shape(
        &mut self,
        request: caret_math::layout::MathTextRequest,
    ) -> Result<caret_math::layout::MathTextShape, caret_math::layout::MathTextShapeError> {
        let font_size_pt = request.font_size_pt as f32;
        if !font_size_pt.is_finite() || font_size_pt <= 0.0 {
            return Err(caret_math::layout::MathTextShapeError::InvalidMetrics);
        }
        let spans = request
            .runs
            .iter()
            .map(|run| {
                let props = run.run.as_ref().map(|run| &run.props);
                let requested_family = run
                    .style
                    .font_family
                    .clone()
                    .or_else(|| {
                        props
                            .and_then(|props| props.get("fontFamily"))
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    })
                    .or_else(|| self.base_style.requested_family.clone());
                let mut style = self.base_style.clone();
                style.requested_family = requested_family;
                style.font_role = caret_layout::FontRole::Body;
                style.bold = run.style.bold.unwrap_or(style.bold);
                style.italic = run.style.italic.unwrap_or(style.italic);
                style.language = run
                    .run
                    .as_ref()
                    .and_then(|run| run.language.clone())
                    .or_else(|| style.language.clone())
                    .or_else(|| self.settings.default_language().map(str::to_owned));
                caret_layout::TextIslandSpan {
                    text: &run.text,
                    style,
                    underline: props
                        .and_then(|props| props.get("underline"))
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                }
            })
            .collect::<Vec<_>>();
        let layout = self
            .text_layouter
            .shape_at_font_size_pt(self.catalog, self.settings, &spans, font_size_pt)
            .map_err(|error| match error {
                caret_layout::TextIslandError::InvalidFontSize => {
                    caret_math::layout::MathTextShapeError::InvalidMetrics
                }
                caret_layout::TextIslandError::MandatoryBreak => {
                    caret_math::layout::MathTextShapeError::UnsupportedInput
                }
                caret_layout::TextIslandError::InconsistentShaping => {
                    caret_math::layout::MathTextShapeError::Failed
                }
            })?;
        let metrics = layout.metrics;
        let requested_family_missing = layout.requested_family_missing;
        let empty_caret_metrics = if layout.text.is_empty() {
            let style = spans
                .first()
                .map(|span| span.style.clone())
                .unwrap_or_else(|| self.base_style.clone());
            let metrics_span = [caret_layout::TextIslandSpan {
                text: "M",
                style,
                underline: false,
            }];
            let metrics_layout = self
                .text_layouter
                .shape_at_font_size_pt(self.catalog, self.settings, &metrics_span, font_size_pt)
                .map_err(|_| caret_math::layout::MathTextShapeError::InvalidMetrics)?;
            Some((
                metrics_layout.metrics.ascent_pt,
                metrics_layout.metrics.descent_pt,
            ))
        } else {
            None
        };
        let fallback_font_size_centipoints = visible_font_size_centipoints(font_size_pt)
            .ok_or(caret_math::layout::MathTextShapeError::InvalidMetrics)?;
        let (runs, containers) = math_text_run_boxes(&request, &layout, metrics);
        let scope_id = request
            .runs
            .iter()
            .find(|run| run.is_text_flow)
            .map(|run| &run.scope_id)
            .or_else(|| {
                request
                    .containers
                    .first()
                    .map(|container| &container.scope_id)
            })
            // SoftBreak/Opaque 可能触发塑形，但不会产生 TextFlow run 或容器。
            .or_else(|| request.runs.first().map(|run| &run.scope_id))
            .ok_or(caret_math::layout::MathTextShapeError::Failed)?;
        let scope_host = self
            .host
            .for_scope(scope_id)
            .ok_or(caret_math::layout::MathTextShapeError::Failed)?;
        let source_spans = math_text_source_spans(&request, scope_host.align_cell)?;
        let token = self.next_token;
        self.next_token = self
            .next_token
            .checked_add(1)
            .ok_or(caret_math::layout::MathTextShapeError::Failed)?;
        if self
            .sidecars
            .insert(
                token,
                MathTextSidecar {
                    layout: Arc::new(layout),
                    color: request.color,
                    target: self.host.target.clone(),
                    host: scope_host.host,
                    source_spans,
                    empty_caret_metrics,
                    fallback_font_size_centipoints,
                },
            )
            .is_some()
        {
            return Err(caret_math::layout::MathTextShapeError::Failed);
        }
        Ok(caret_math::layout::MathTextShape {
            token,
            width: f64::from(metrics.advance_pt),
            ascent: f64::from(metrics.ascent_pt),
            descent: f64::from(metrics.descent_pt),
            requested_family_missing,
            runs,
            containers,
        })
    }
}

fn math_text_run_boxes(
    request: &caret_math::layout::MathTextRequest,
    shaped: &TextIslandLayout,
    metrics: caret_layout::TextIslandMetrics,
) -> (
    Vec<caret_math::layout::MathTextRunBox>,
    Vec<caret_math::layout::MathTextRunBox>,
) {
    let Some(line) = shaped.layout.lines().next() else {
        let runs = request
            .runs
            .iter()
            .map(|run| caret_math::layout::MathTextRunBox {
                id: None,
                scope_id: run.scope_id.clone(),
                path: run.path.clone(),
                run_index: run.run_index,
                is_text_flow: run.is_text_flow,
                is_flow: false,
                is_container: false,
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
                baseline: 0.0,
            })
            .collect();
        let containers = request
            .containers
            .iter()
            .map(|container| caret_math::layout::MathTextRunBox {
                id: container.id.clone(),
                scope_id: container.scope_id.clone(),
                path: container.path.clone(),
                run_index: None,
                is_text_flow: false,
                is_flow: container.is_flow,
                is_container: container.is_container,
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
                baseline: 0.0,
            })
            .collect();
        return (runs, containers);
    };
    let mut spans = Vec::with_capacity(request.runs.len());
    let mut byte_offset = 0_usize;
    for run in &request.runs {
        let end = byte_offset.saturating_add(run.text.len());
        spans.push(byte_offset..end);
        byte_offset = end;
    }
    let mut clusters = Vec::<(std::ops::Range<usize>, f32, f32, bool)>::new();
    let mut x = line.metrics().inline_min_coord + line.metrics().offset;
    for run in line.runs() {
        for cluster in run.visual_clusters() {
            let advance = cluster.advance();
            let range = cluster.text_range();
            if !x.is_finite() || !advance.is_finite() || advance < 0.0 {
                continue;
            }
            clusters.push((range, x, x + advance, cluster.is_rtl()));
            x += advance;
        }
    }
    clusters.sort_unstable_by_key(|(range, _, _, _)| (range.start, range.end));
    let bounds_for_range = |span: &std::ops::Range<usize>| {
        let first = clusters.partition_point(|(range, _, _, _)| range.end <= span.start);
        if span.is_empty() {
            let x = clusters
                .get(first.wrapping_sub(1))
                .filter(|(range, _, _, _)| range.end == span.start)
                .map(|(_, x0, x1, is_rtl)| if *is_rtl { *x0 } else { *x1 })
                .or_else(|| {
                    clusters
                        .get(first)
                        .filter(|(range, _, _, _)| range.start == span.start)
                        .map(|(_, x0, x1, is_rtl)| if *is_rtl { *x1 } else { *x0 })
                })
                .unwrap_or(0.0);
            return Some((x, 0.0));
        }
        let mut x0 = f32::INFINITY;
        let mut x1 = f32::NEG_INFINITY;
        for (range, cluster_x0, cluster_x1, _) in clusters.iter().skip(first) {
            if range.start >= span.end {
                break;
            }
            if span.start < range.end {
                x0 = x0.min(*cluster_x0);
                x1 = x1.max(*cluster_x1);
            }
        }
        if !x0.is_finite() {
            Some((0.0, 0.0))
        } else {
            Some((x0, (x1 - x0).max(0.0)))
        }
    };
    let make_box = |id: Option<caret_doc::NodeId>,
                    scope_id: caret_doc::NodeId,
                    path: Vec<caret_doc::MathPathStep>,
                    run_index: Option<usize>,
                    is_text_flow: bool,
                    is_flow: bool,
                    is_container: bool,
                    span: &std::ops::Range<usize>| {
        let (x, width) = bounds_for_range(span).unwrap_or((0.0, 0.0));
        caret_math::layout::MathTextRunBox {
            id,
            scope_id,
            path,
            run_index,
            is_text_flow,
            is_flow,
            is_container,
            x: f64::from(x),
            y: 0.0,
            width: f64::from(width),
            height: f64::from(metrics.ascent_pt + metrics.descent_pt),
            baseline: f64::from(metrics.ascent_pt),
        }
    };
    let runs = request
        .runs
        .iter()
        .zip(spans)
        .map(|(run, span)| {
            make_box(
                None,
                run.scope_id.clone(),
                run.path.clone(),
                run.run_index,
                run.is_text_flow,
                false,
                false,
                &span,
            )
        })
        .collect();
    let containers = request
        .containers
        .iter()
        .map(|container| {
            make_box(
                container.id.clone(),
                container.scope_id.clone(),
                container.path.clone(),
                None,
                false,
                container.is_flow,
                container.is_container,
                &(container.start_byte..container.end_byte),
            )
        })
        .collect();
    (runs, containers)
}

fn math_text_source_spans(
    request: &caret_math::layout::MathTextRequest,
    align_cell: Option<(usize, usize)>,
) -> Result<Vec<MathTextSourceSpan>, caret_math::layout::MathTextShapeError> {
    let mut spans = Vec::new();
    let mut byte_offset = 0_usize;
    let mut flow_key: Option<(NodeId, Vec<MathPathStep>)> = None;
    let mut flow_offset = 0_usize;
    for run in &request.runs {
        let end = byte_offset
            .checked_add(run.text.len())
            .ok_or(caret_math::layout::MathTextShapeError::Failed)?;
        if run.is_text_flow {
            let key = (run.scope_id.clone(), run.path.clone());
            if flow_key.as_ref() != Some(&key) {
                flow_key = Some(key);
                flow_offset = 0;
            }
            spans.push(MathTextSourceSpan {
                range: byte_offset..end,
                flow_offset,
                path: MathTextFlowPath::from_layout_path(run.path.clone(), align_cell),
            });
            flow_offset = flow_offset
                .checked_add(run.text.len())
                .ok_or(caret_math::layout::MathTextShapeError::Failed)?;
        }
        byte_offset = end;
    }
    Ok(spans)
}

fn document_contains_math(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph(paragraph) => contains_inline_math(&paragraph.inlines),
        Block::Environment(environment) => {
            environment
                .title
                .as_ref()
                .is_some_and(|paragraph| contains_inline_math(&paragraph.inlines))
                || document_contains_math(&environment.children)
        }
        Block::Equation(_) | Block::Align(_) => true,
        Block::ExplicitBreak(_) | Block::Opaque(_) => false,
    })
}

struct InlineMathLayouts {
    objects: Vec<InlineObjectLayout>,
    layouts: Vec<Arc<caret_math::layout::MathLayout>>,
    ids: Vec<NodeId>,
    text_layouts: Vec<HashMap<u64, MathTextSidecar>>,
}

struct InlineMathLayoutBuilder<'c, 'a> {
    context: &'c mut MathLayoutContext<'a>,
    base_style: &'c ShapingStyle,
    paragraph_id: NodeId,
    font_size_pt: f64,
    offset: usize,
    objects: Vec<InlineObjectLayout>,
    layouts: Vec<Arc<caret_math::layout::MathLayout>>,
    ids: Vec<NodeId>,
    text_layouts: Vec<HashMap<u64, MathTextSidecar>>,
}

impl InlineMathLayoutBuilder<'_, '_> {
    fn visit(&mut self, nodes: &[ParagraphContent]) -> Result<(), DocumentLayoutError> {
        for node in nodes {
            match node {
                FlowContent::Text(text) => {
                    for run in &text.runs {
                        self.offset = self.offset.checked_add(run.text.len()).ok_or_else(|| {
                            DocumentLayoutError::Math("inline text offset overflow".to_owned())
                        })?;
                    }
                }
                FlowContent::Math(math) => {
                    let start = self.offset;
                    self.offset =
                        self.offset
                            .checked_add('\u{fffc}'.len_utf8())
                            .ok_or_else(|| {
                                DocumentLayoutError::Math("inline math offset overflow".to_owned())
                            })?;
                    let engine = self
                        .context
                        .engine
                        .ok_or(DocumentLayoutError::UnsupportedContent)?;
                    let (layout, sidecars) = if math_flow_contains_text(&math.math_tree.root) {
                        let mut shaper = EngineMathTextShaper::new(
                            self.context.catalog,
                            self.context.settings,
                            self.base_style,
                            self.context.text_layouter,
                            MathTextLayoutHost::inline(
                                self.paragraph_id.clone(),
                                math.id.clone(),
                                math.math_tree.root.id.clone(),
                            ),
                        );
                        let layout = engine
                            .layout_with_text_shaper(
                                &math.math_tree.root,
                                self.font_size_pt,
                                caret_math::layout::MathStyle::Inline,
                                &mut shaper,
                            )
                            .map_err(|error| DocumentLayoutError::Math(error.to_string()))?;
                        (layout, shaper.into_sidecars())
                    } else {
                        (
                            engine
                                .layout(
                                    &math.math_tree.root,
                                    self.font_size_pt,
                                    caret_math::layout::MathStyle::Inline,
                                )
                                .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                            HashMap::new(),
                        )
                    };
                    let as_f32 = |value: f64| {
                        let value = value as f32;
                        value.is_finite().then_some(value)
                    };
                    let advance_pt = as_f32(layout.width)
                        .ok_or_else(|| {
                            DocumentLayoutError::Math("invalid inline math width".to_owned())
                        })?
                        .max(0.0);
                    let advance_pt = if advance_pt == 0.0
                        && sidecars
                            .values()
                            .any(|sidecar| sidecar.layout.metrics.advance_pt <= 0.0)
                    {
                        EMPTY_INLINE_MATH_CARET_ADVANCE_PT
                    } else {
                        advance_pt
                    };
                    let ascent_pt = as_f32(layout.ascent)
                        .ok_or_else(|| {
                            DocumentLayoutError::Math("invalid inline math ascent".to_owned())
                        })?
                        .max(0.0);
                    let descent_pt = as_f32(layout.descent)
                        .ok_or_else(|| {
                            DocumentLayoutError::Math("invalid inline math descent".to_owned())
                        })?
                        .max(0.0);
                    let id = u64::try_from(self.layouts.len()).map_err(|_| {
                        DocumentLayoutError::Math("too many inline math objects".to_owned())
                    })?;
                    self.objects.push(InlineObjectLayout {
                        id,
                        source_bytes: start..self.offset,
                        advance_pt,
                        ascent_pt,
                        descent_pt,
                    });
                    self.layouts.push(layout);
                    self.ids.push(math.id.clone());
                    self.text_layouts.push(sidecars);
                }
                FlowContent::Environment(environment) => self.visit(&environment.children)?,
                FlowContent::SoftBreak(_) => {
                    self.offset = self.offset.checked_add('\n'.len_utf8()).ok_or_else(|| {
                        DocumentLayoutError::Math("inline break offset overflow".to_owned())
                    })?;
                }
                FlowContent::Opaque(_) => {
                    self.offset =
                        self.offset
                            .checked_add('\u{fffc}'.len_utf8())
                            .ok_or_else(|| {
                                DocumentLayoutError::Math(
                                    "opaque inline offset overflow".to_owned(),
                                )
                            })?;
                }
            }
        }
        Ok(())
    }
}

fn collect_inline_math_layouts(
    paragraph: &Paragraph,
    context: &mut MathLayoutContext<'_>,
) -> Result<InlineMathLayouts, DocumentLayoutError> {
    let font_size_pt =
        DocumentUnits::centipoints_to_points(u32::from(context.font_size_centipoints));
    let mut math_base_style = context.base_style.clone();
    math_base_style.language = paragraph
        .language
        .clone()
        .or_else(|| context.settings.default_language().map(str::to_owned))
        .or_else(|| context.base_style.language.clone());
    let mut builder = InlineMathLayoutBuilder {
        context,
        base_style: &math_base_style,
        paragraph_id: paragraph.id.clone(),
        font_size_pt,
        offset: 0,
        objects: Vec::new(),
        layouts: Vec::new(),
        ids: Vec::new(),
        text_layouts: Vec::new(),
    };
    builder.visit(&paragraph.inlines)?;
    Ok(InlineMathLayouts {
        objects: builder.objects,
        layouts: builder.layouts,
        ids: builder.ids,
        text_layouts: builder.text_layouts,
    })
}

struct MathLayoutContext<'a> {
    engine: Option<&'a caret_math::layout::MathLayoutEngine>,
    catalog: &'a mut FontCatalog,
    settings: &'a ResolvedDocumentTextSettings,
    base_style: &'a ShapingStyle,
    text_layouter: &'a mut TextIslandLayouter,
    font_size_centipoints: u16,
    content_width_pt: f32,
}

enum LayoutInput<'a> {
    Paragraph {
        paragraph: &'a Paragraph,
        story: StoryId,
        environment_ids: Vec<NodeId>,
    },
    ExplicitBreak,
    MathBlock {
        math_id: NodeId,
        layout: Arc<caret_math::layout::MathLayout>,
        text_layouts: HashMap<u64, MathTextSidecar>,
        environment_ids: Vec<NodeId>,
        x_offset_pt: f32,
        width_pt: f32,
        height_pt: f32,
    },
}

fn collect_blocks<'a>(
    blocks: &'a [Block],
    ordered: &mut Vec<LayoutInput<'a>>,
    environment_ids: &mut Vec<NodeId>,
    context: &mut MathLayoutContext<'_>,
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
                collect_blocks(&environment.children, ordered, environment_ids, context)?;
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
            Block::Equation(equation) => {
                let engine = context
                    .engine
                    .ok_or(DocumentLayoutError::UnsupportedContent)?;
                let (layout, text_layouts) = if math_flow_contains_text(&equation.math_tree.root) {
                    let mut shaper = EngineMathTextShaper::new(
                        context.catalog,
                        context.settings,
                        context.base_style,
                        context.text_layouter,
                        MathTextLayoutHost::equation(
                            equation.id.clone(),
                            equation.math_tree.root.id.clone(),
                        ),
                    );
                    let layout = engine
                        .layout_with_text_shaper(
                            &equation.math_tree.root,
                            DocumentUnits::centipoints_to_points(u32::from(
                                context.font_size_centipoints,
                            )),
                            caret_math::layout::MathStyle::Display,
                            &mut shaper,
                        )
                        .map_err(|error| DocumentLayoutError::Math(error.to_string()))?;
                    (layout, shaper.into_sidecars())
                } else {
                    (
                        engine
                            .layout(
                                &equation.math_tree.root,
                                DocumentUnits::centipoints_to_points(u32::from(
                                    context.font_size_centipoints,
                                )),
                                caret_math::layout::MathStyle::Display,
                            )
                            .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                        HashMap::new(),
                    )
                };
                push_math_block(
                    ordered,
                    equation.id.clone(),
                    layout,
                    text_layouts,
                    environment_ids.clone(),
                    context.content_width_pt,
                )?;
            }
            Block::Align(align) => {
                let engine = context
                    .engine
                    .ok_or(DocumentLayoutError::UnsupportedContent)?;
                let contains_text = align
                    .rows
                    .iter()
                    .flat_map(|row| &row.cells)
                    .any(|cell| math_flow_contains_text(&cell.root));
                let (layout, text_layouts) = if contains_text {
                    let mut shaper = EngineMathTextShaper::new(
                        context.catalog,
                        context.settings,
                        context.base_style,
                        context.text_layouter,
                        MathTextLayoutHost::align(align.id.clone(), align),
                    );
                    (
                        Arc::new(
                            caret_math::layout::layout_align_with_text_shaper(
                                engine.font(),
                                align,
                                DocumentUnits::centipoints_to_points(u32::from(
                                    context.font_size_centipoints,
                                )),
                                &mut shaper,
                            )
                            .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                        ),
                        shaper.into_sidecars(),
                    )
                } else {
                    (
                        Arc::new(
                            caret_math::layout::layout_align(
                                engine.font(),
                                align,
                                DocumentUnits::centipoints_to_points(u32::from(
                                    context.font_size_centipoints,
                                )),
                            )
                            .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                        ),
                        HashMap::new(),
                    )
                };
                push_math_block(
                    ordered,
                    align.id.clone(),
                    layout,
                    text_layouts,
                    environment_ids.clone(),
                    context.content_width_pt,
                )?;
            }
            Block::Opaque(_) => return Err(DocumentLayoutError::UnsupportedContent),
        }
    }
    Ok(())
}

fn push_math_block<'a>(
    ordered: &mut Vec<LayoutInput<'a>>,
    math_id: NodeId,
    layout: Arc<caret_math::layout::MathLayout>,
    text_layouts: HashMap<u64, MathTextSidecar>,
    environment_ids: Vec<NodeId>,
    content_width_pt: f32,
) -> Result<(), DocumentLayoutError> {
    let width_pt = layout.width as f32;
    let height_pt = (layout.ascent + layout.descent) as f32;
    if !content_width_pt.is_finite()
        || content_width_pt <= 0.0
        || !width_pt.is_finite()
        || width_pt < 0.0
        || !height_pt.is_finite()
        || height_pt < 0.0
    {
        return Err(DocumentLayoutError::Math(
            "invalid display math geometry".to_owned(),
        ));
    }
    ordered.push(LayoutInput::MathBlock {
        math_id,
        layout,
        text_layouts,
        environment_ids,
        x_offset_pt: (content_width_pt - width_pt) / 2.0,
        width_pt,
        height_pt,
    });
    Ok(())
}

#[cfg(test)]
mod tests;
