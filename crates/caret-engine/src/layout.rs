//! 组合文档、字体塑形、段落行布局与分页，生成供视图消费的只读快照。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use crate::{
    Affinity, MathEditTarget, MathPositionAffinity, MathPositionHost, MathSequencePath,
    MathSequenceStep, MathTextFlowPath, MathTextPosition, MathTreePosition, Pos, Selection,
    Session, StoryId,
};
use caret_doc::{
    Block, Document, DocumentUnits, FlowContent, MathContent, MathFlow, MathPathStep, NodeId,
    Paragraph, ParagraphContent,
};
use caret_layout::{
    FontCatalog, FontCatalogError, FontLineMetrics, HitAffinity, InlineObjectLayout, LineMetrics,
    LineSpacing, PageLayout, PaginationItem, ParagraphLayoutOptions, ParagraphLayouter,
    ParagraphPaginationInput, ParleyParagraphLayout, PositionedGlyphLayoutItem,
    ResolvedDocumentTextSettings, ResolvedParagraphStyle, ShapingStyle, TextIslandLayout,
    VisualAffinity, calculate_content_line_metrics_with_grid, calculate_line_metrics_with_grid,
    paginate_paragraphs, paragraph_has_empty_line, resolve_paragraph_style,
};
use std::{collections::HashMap, ops::Range, sync::Arc};

mod math_navigation;

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
    math_caret_height_pt: f32,
    math_caret_baseline_pt: f32,
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
            math_caret_height_pt: input.math_caret_height_pt,
            math_caret_baseline_pt: input.math_caret_baseline_pt,
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
            math_caret_height_pt: self.math_caret_height_pt,
            math_caret_baseline_pt: self.math_caret_baseline_pt,
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
    math_caret_height_pt: f32,
    math_caret_baseline_pt: f32,
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

fn find_inline_math_by_id<'a>(
    nodes: &'a [ParagraphContent],
    id: &NodeId,
) -> Option<&'a caret_doc::InlineMath> {
    nodes.iter().find_map(|node| match node {
        FlowContent::Math(math) if &math.id == id => Some(math),
        FlowContent::Environment(environment) => find_inline_math_by_id(&environment.children, id),
        FlowContent::Text(_)
        | FlowContent::Math(_)
        | FlowContent::SoftBreak(_)
        | FlowContent::Opaque(_) => None,
    })
}

fn math_flow_at_layout_path<'a>(
    root: &'a MathFlow,
    steps: &[MathPathStep],
) -> Option<&'a MathFlow> {
    match math_layout_target_at_path(root, steps)? {
        MathLayoutTarget::Flow(flow) => Some(flow),
        MathLayoutTarget::Content(_) => None,
    }
}

fn math_text_flow_at_layout_path<'a>(
    root: &'a MathFlow,
    steps: &[MathPathStep],
) -> Option<&'a caret_doc::TextFlow> {
    match math_layout_target_at_path(root, steps)? {
        MathLayoutTarget::Content(MathContent::Text(text)) => Some(text),
        MathLayoutTarget::Flow(_) | MathLayoutTarget::Content(_) => None,
    }
}

fn math_sequence_edge_child_indices(
    offset: usize,
    length: usize,
    affinity: MathPositionAffinity,
) -> [Option<usize>; 2] {
    match affinity {
        MathPositionAffinity::Upstream => {
            [offset.checked_sub(1), (offset < length).then_some(offset)]
        }
        MathPositionAffinity::Downstream => {
            [(offset < length).then_some(offset), offset.checked_sub(1)]
        }
    }
}

fn math_environment_at_layout_path<'a>(
    root: &'a MathFlow,
    steps: &[MathPathStep],
) -> Option<&'a caret_doc::InlineEnvironment<MathContent>> {
    match math_layout_target_at_path(root, steps)? {
        MathLayoutTarget::Content(MathContent::Environment(environment)) => Some(environment),
        MathLayoutTarget::Flow(_) | MathLayoutTarget::Content(_) => None,
    }
}

enum MathLayoutTarget<'a> {
    Flow(&'a MathFlow),
    Content(&'a MathContent),
}

fn math_layout_target_at_path<'a>(
    root: &'a MathFlow,
    steps: &[MathPathStep],
) -> Option<MathLayoutTarget<'a>> {
    let mut target = MathLayoutTarget::Flow(root);
    for step in steps {
        target = match (target, step) {
            (MathLayoutTarget::Flow(flow), MathPathStep::Content(index)) => {
                MathLayoutTarget::Content(flow.content.get(*index)?)
            }
            (MathLayoutTarget::Content(MathContent::Math(node)), MathPathStep::Operand(index)) => {
                MathLayoutTarget::Flow(node.children.get(*index)?)
            }
            (
                MathLayoutTarget::Content(MathContent::Environment(environment)),
                MathPathStep::EnvironmentChild(index),
            ) => MathLayoutTarget::Content(environment.children.get(*index)?),
            _ => return None,
        };
    }
    Some(target)
}

fn math_sequence_path_for_flow(steps: &[MathPathStep]) -> Option<MathSequencePath> {
    let mut sequence = Vec::with_capacity(steps.len() / 2);
    let mut content_index = None;
    for step in steps {
        match *step {
            MathPathStep::Content(index) => {
                if content_index.replace(index).is_some() {
                    return None;
                }
            }
            MathPathStep::Operand(operand_index) => {
                sequence.push(MathSequenceStep::Operand {
                    content_index: content_index.take()?,
                    operand_index,
                });
            }
            MathPathStep::EnvironmentChild(index) => {
                sequence.push(MathSequenceStep::EnvironmentChildren {
                    content_index: content_index.take()?,
                });
                content_index = Some(index);
            }
        }
    }
    content_index
        .is_none()
        .then(|| MathSequencePath::new(sequence))
}

fn math_sequence_path_for_environment_children(steps: &[MathPathStep]) -> Option<MathSequencePath> {
    let mut sequence = Vec::with_capacity(steps.len() / 2 + 1);
    let mut content_index = None;
    for step in steps {
        match *step {
            MathPathStep::Content(index) => {
                if content_index.replace(index).is_some() {
                    return None;
                }
            }
            MathPathStep::Operand(operand_index) => {
                sequence.push(MathSequenceStep::Operand {
                    content_index: content_index.take()?,
                    operand_index,
                });
            }
            MathPathStep::EnvironmentChild(index) => {
                sequence.push(MathSequenceStep::EnvironmentChildren {
                    content_index: content_index.take()?,
                });
                content_index = Some(index);
            }
        }
    }
    sequence.push(MathSequenceStep::EnvironmentChildren {
        content_index: content_index.take()?,
    });
    content_index
        .is_none()
        .then(|| MathSequencePath::new(sequence))
}

/// 单代文档布局结果。页面和段落位置共用同一分页输出。
#[derive(Debug, Clone)]
pub struct DocumentLayoutSnapshot {
    pub session_id: Option<NodeId>,
    pub document_generation: u64,
    pub font_generation: u64,
    /// 文档显式请求的数学字体不可用时，保留请求并回退到内置 STIX Two Math。
    pub math_font_diagnostic: Option<MathFontDiagnostic>,
    /// 正文与公式文本共享塑形后诊断通道，保留宿主节点 ID 和类型。
    pub text_diagnostics: Vec<TextLayoutDiagnostic>,
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
    math_block_sources: HashMap<NodeId, MathBlockSource>,
    pub(crate) math_hosts: HashMap<NodeId, MathTextLayoutHost>,
    pub(crate) math_projections: Vec<MathPageProjection>,
    math_projection_lookup: HashMap<(usize, NodeId), usize>,
    math_projections_by_id: HashMap<NodeId, Vec<usize>>,
    paragraph_lookup: HashMap<NodeId, usize>,
}

/// 数学字体请求无法使用时的布局诊断；不改写文档设置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathFontDiagnostic {
    /// 稳定诊断键，供界面本地化。
    pub code: &'static str,
    /// `.cdoc` 中保留的原始请求族名。
    pub requested_family: String,
}

/// 单个文本塑形诊断及其文档宿主身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextLayoutDiagnostic {
    /// 产生诊断的段落或公式节点。
    pub node_id: NodeId,
    /// `paragraph` 或 `math`。
    pub node_kind: &'static str,
    /// 稳定诊断键，供界面本地化。
    pub code: &'static str,
    /// 用于定位缺失内容的诊断细节。
    pub detail: String,
}

/// 同一代公式布局到页面坐标的投影；裁切与正文行使用相同规则。
#[derive(Debug, Clone)]
pub(crate) struct MathPageProjection {
    pub(crate) math_id: NodeId,
    pub(crate) page_index: usize,
    pub(crate) object_x_pt: f64,
    pub(crate) baseline_y_pt: f64,
    pub(crate) caret_line_top_pt: f64,
    pub(crate) caret_baseline_y_pt: f64,
    pub(crate) caret_height_pt: f64,
    pub(crate) clip_y_pt: Option<[f64; 2]>,
    pub(crate) row_clips: Vec<MathPageRowClip>,
    pub(crate) row_baselines: Vec<MathPageRowBaseline>,
    inline_index: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct MathPageRowClip {
    pub(crate) row_id: NodeId,
    pub(crate) y0_pt: f64,
    pub(crate) y1_pt: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct MathPageRowBaseline {
    pub(crate) baseline_y_pt: f64,
}

impl MathPageProjection {
    /// 根据数学节点自身基线生成正文高度的 caret，保持脚本相对正文基线的偏移。
    fn caret_at_math_baseline(&self, x_pt: f64, math_baseline_y_pt: f64) -> PageCaretGeometry {
        let baseline_shift = math_baseline_y_pt - self.caret_baseline_y_pt;
        PageCaretGeometry {
            page_index: self.page_index,
            x_pt,
            y0_pt: self.caret_line_top_pt + baseline_shift,
            y1_pt: self.caret_line_top_pt + baseline_shift + self.caret_height_pt,
        }
    }

    /// 完全不可见的几何不发布；部分可见的几何取行裁切交集。
    fn clip_range(&self, top: f64, bottom: f64) -> Option<(f64, f64)> {
        let (top, bottom) = match self.clip_y_pt {
            Some([clip_top, clip_bottom]) => (top.max(clip_top), bottom.min(clip_bottom)),
            None => (top, bottom),
        };
        (top < bottom).then_some((top, bottom))
    }

    fn contains_y(&self, y: f64) -> bool {
        self.clip_y_pt
            .is_none_or(|[top, bottom]| y >= top && y <= bottom)
    }

    fn row_clip(&self, row_id: Option<&NodeId>) -> Option<[f64; 2]> {
        let row_id = row_id?;
        self.row_clips
            .iter()
            .find(|clip| &clip.row_id == row_id)
            .map(|clip| [clip.y0_pt, clip.y1_pt])
    }

    fn contains_y_for_row(&self, y: f64, row_id: Option<&NodeId>) -> bool {
        self.row_clip(row_id)
            .is_none_or(|[top, bottom]| y >= top && y <= bottom)
            && self.contains_y(y)
    }

    fn clip_range_for_row(
        &self,
        top: f64,
        bottom: f64,
        row_id: Option<&NodeId>,
    ) -> Option<(f64, f64)> {
        let (top, bottom) = match (self.row_clip(row_id), self.clip_y_pt) {
            (Some([row_top, row_bottom]), Some([clip_top, clip_bottom])) => (
                top.max(row_top).max(clip_top),
                bottom.min(row_bottom).min(clip_bottom),
            ),
            (Some([row_top, row_bottom]), None) => (top.max(row_top), bottom.min(row_bottom)),
            (None, _) => return self.clip_range(top, bottom),
        };
        (top < bottom).then_some((top, bottom))
    }

    fn clip_caret(&self, mut caret: PageCaretGeometry) -> Option<PageCaretGeometry> {
        let Some([clip_top, clip_bottom]) = self.clip_y_pt else {
            return Some(caret);
        };
        if let Some((top, bottom)) = self.clip_range(caret.y0_pt, caret.y1_pt) {
            (caret.y0_pt, caret.y1_pt) = (top, bottom);
            return Some(caret);
        }

        // Exactly 行距可完全裁掉数学内容；逻辑光标仍需锚定到最近可见边缘。
        let height = (caret.y1_pt - caret.y0_pt).min(clip_bottom - clip_top);
        if (caret.y0_pt + caret.y1_pt) / 2.0 < clip_top {
            caret.y0_pt = clip_top;
            caret.y1_pt = clip_top + height;
        } else {
            caret.y0_pt = clip_bottom - height;
            caret.y1_pt = clip_bottom;
        }
        Some(caret)
    }

    fn clip_caret_for_row(
        &self,
        mut caret: PageCaretGeometry,
        row_id: Option<&NodeId>,
    ) -> Option<PageCaretGeometry> {
        let Some([row_top, row_bottom]) = self.row_clip(row_id) else {
            return self.clip_caret(caret);
        };
        let Some((top, bottom)) = self.clip_range_for_row(caret.y0_pt, caret.y1_pt, row_id) else {
            let height = (caret.y1_pt - caret.y0_pt).min(row_bottom - row_top);
            if (caret.y0_pt + caret.y1_pt) / 2.0 < row_top {
                caret.y0_pt = row_top;
                caret.y1_pt = row_top + height;
            } else {
                caret.y0_pt = row_bottom - height;
                caret.y1_pt = row_bottom;
            }
            return Some(caret);
        };
        caret.y0_pt = top;
        caret.y1_pt = bottom;
        Some(caret)
    }

    pub(crate) fn drawing_clip(&self) -> Option<[f64; 2]> {
        self.clip_y_pt
    }
}

fn math_row_id_for_host(host: &MathPositionHost) -> Option<&NodeId> {
    match host {
        MathPositionHost::AlignCell { row_id, .. } => Some(row_id),
        MathPositionHost::InlineMath { .. } | MathPositionHost::Equation { .. } => None,
    }
}

impl DocumentLayoutSnapshot {
    /// 返回显示公式块首尾的正文高度光标几何，Align 使用首行/末行边缘。
    fn math_block_boundary_caret(
        &self,
        block_id: &NodeId,
        direction: crate::editing::Direction,
    ) -> Option<PageCaretGeometry> {
        let mut projections = self
            .math_projections_by_id
            .get(block_id)?
            .iter()
            .filter_map(|index| self.math_projections.get(*index))
            .filter(|projection| projection.inline_index.is_none())
            .collect::<Vec<_>>();
        projections.sort_by_key(|projection| projection.page_index);
        let projection = match direction {
            crate::editing::Direction::Backward => projections.first()?,
            crate::editing::Direction::Forward => projections.last()?,
        };
        let page = self.pages.get(projection.page_index)?;
        let x_pt = match direction {
            crate::editing::Direction::Backward => page.margin_left_pt,
            crate::editing::Direction::Forward => page.width_pt - page.margin_right_pt,
        };
        let row_baseline = match direction {
            crate::editing::Direction::Backward => projection.row_baselines.first(),
            crate::editing::Direction::Forward => projection.row_baselines.last(),
        };
        Some(row_baseline.map_or_else(
            || PageCaretGeometry {
                page_index: projection.page_index,
                x_pt,
                y0_pt: projection.caret_line_top_pt,
                y1_pt: projection.caret_line_top_pt + projection.caret_height_pt,
            },
            |row| projection.caret_at_math_baseline(x_pt, row.baseline_y_pt),
        ))
    }

    /// 返回块外侧停靠点的页面 x 坐标；环境容器和显示公式消费同一边界查询。
    pub fn block_boundary_x(
        &self,
        block_id: &NodeId,
        page_index: usize,
        direction: crate::editing::Direction,
    ) -> Option<f64> {
        let environment_edge = self
            .environment_ranges
            .iter()
            .filter(|range| &range.environment_id == block_id && range.page_index == page_index)
            .map(|range| match direction {
                crate::editing::Direction::Backward => range.x0_pt,
                crate::editing::Direction::Forward => range.x1_pt,
            })
            .next();
        if environment_edge.is_some() {
            return environment_edge;
        }
        let has_projection = self
            .math_projections_by_id
            .get(block_id)?
            .iter()
            .filter_map(|index| self.math_projections.get(*index))
            .any(|projection| {
                projection.page_index == page_index && projection.inline_index.is_none()
            });
        if !has_projection {
            return None;
        }
        let page = self.pages.get(page_index)?;
        match direction {
            crate::editing::Direction::Backward => Some(page.margin_left_pt),
            crate::editing::Direction::Forward => Some(page.width_pt - page.margin_right_pt),
        }
    }

    /// 将块类型的边界几何适配合并到共享外侧停靠光标投影。
    pub fn block_boundary_caret(
        &self,
        block_id: &NodeId,
        direction: crate::editing::Direction,
        mut fallback: PageCaretGeometry,
    ) -> PageCaretGeometry {
        if let Some(caret) = self.math_block_boundary_caret(block_id, direction) {
            return caret;
        }
        fallback.x_pt = self
            .block_boundary_x(block_id, fallback.page_index, direction)
            .unwrap_or(fallback.x_pt);
        fallback
    }

    /// 按页索引取连续片段，避免命中时扫描其他页面的公式。
    fn math_projections_on_page(&self, page_index: usize) -> &[MathPageProjection] {
        let start = self
            .math_projections
            .partition_point(|projection| projection.page_index < page_index);
        let end = self
            .math_projections
            .partition_point(|projection| projection.page_index <= page_index);
        &self.math_projections[start..end]
    }

    fn math_projections_for_target(
        &self,
        target: &MathEditTarget,
    ) -> impl Iterator<Item = &MathPageProjection> {
        let math_id = match target {
            MathEditTarget::Inline { math_id, .. } => math_id,
            MathEditTarget::Block { block_id, .. } => block_id,
        };
        self.math_projections_by_id
            .get(math_id)
            .into_iter()
            .flatten()
            .filter_map(|index| self.math_projections.get(*index))
    }
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
    pub token: u64,
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

/// 单个数学文本岛在页面上的字素停靠点，供通用画布拖选协议消费。
#[derive(Debug, Clone, PartialEq)]
pub struct PageMathTextDragStop {
    pub token: u64,
    pub order_utf8: usize,
    pub target: MathEditTarget,
    pub position: MathTextPosition,
    pub visual_affinity: VisualAffinity,
    pub caret: PageCaretGeometry,
}

/// 数学文本竖直导航结果；跨文本岛时携带可由 Session 校验的页面命中。
#[derive(Debug, Clone, PartialEq)]
pub enum MathTextVerticalTarget {
    SameTextIsland {
        position: MathTextPosition,
        visual_affinity: VisualAffinity,
        preferred_x_pt: f32,
    },
    DifferentTextIsland(PageMathTextHit),
    DifferentMathStructure(PageMathStructureHit),
}

/// 数学树焦点按页面几何移动后的文本岛或结构位置。
#[derive(Debug, Clone, PartialEq)]
pub enum MathTreeVerticalTarget {
    Structure(PageMathStructureHit),
    Text(PageMathTextHit),
}

/// 页面内命中的必需数学结构槽；路径、宿主和插入点来自同一布局代际。
#[derive(Debug, Clone, PartialEq)]
pub struct PageMathStructureHit {
    pub session_id: Option<NodeId>,
    pub document_generation: u64,
    pub font_generation: u64,
    pub target: MathEditTarget,
    pub position: MathTreePosition,
    pub caret: PageCaretGeometry,
}

impl DocumentLayoutSnapshot {
    pub(crate) fn math_projection(
        &self,
        page_index: usize,
        math_id: &NodeId,
    ) -> Option<&MathPageProjection> {
        self.math_host(math_id)?;
        let index = self
            .math_projection_lookup
            .get(&(page_index, math_id.clone()))?;
        self.math_projections.get(*index)
    }

    pub(crate) fn math_host(&self, math_id: &NodeId) -> Option<&MathTextLayoutHost> {
        self.math_hosts.get(math_id)
    }

    fn math_text_row_id<'a>(
        &'a self,
        target: &MathEditTarget,
        position: &MathTextPosition,
    ) -> Option<&'a NodeId> {
        let MathEditTarget::Block {
            block_id,
            align: true,
        } = target
        else {
            return None;
        };
        let (row_index, _) = position.path.align_cell()?;
        let MathBlockSource::Align(align) = self.math_block_sources.get(block_id)? else {
            return None;
        };
        Some(&align.rows.get(row_index)?.id)
    }
    fn math_projection_items(
        &self,
        projection: &MathPageProjection,
    ) -> Option<(
        &caret_math::layout::MathLayout,
        &HashMap<u64, MathTextSidecar>,
    )> {
        match &self.math_host(&projection.math_id)?.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let paragraph = self.paragraph_by_id(paragraph_id)?;
                let index = projection.inline_index?;
                if paragraph.inline_math_ids.get(index)? != math_id {
                    return None;
                }
                Some((
                    paragraph.inline_math_layouts.get(index)?.as_ref(),
                    paragraph.inline_math_text_layouts.get(index)?,
                ))
            }
            MathEditTarget::Block { block_id, .. } => Some((
                self.math_blocks.get(block_id)?.as_ref(),
                self.math_block_text_layouts.get(block_id)?,
            )),
        }
    }

    fn math_scope_root(
        &self,
        projection: &MathPageProjection,
        scope_id: &NodeId,
    ) -> Option<&MathFlow> {
        let host = self.math_host(&projection.math_id)?;
        host.for_scope(scope_id)?;
        let root = match &host.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                &find_inline_math_by_id(
                    &self.paragraph_by_id(paragraph_id)?.source_paragraph.inlines,
                    math_id,
                )?
                .math_tree
                .root
            }
            MathEditTarget::Block {
                block_id,
                align: false,
            } => match self.math_block_sources.get(block_id)? {
                MathBlockSource::Equation(equation) => &equation.math_tree.root,
                MathBlockSource::Align(_) => return None,
            },
            MathEditTarget::Block {
                block_id,
                align: true,
            } => match self.math_block_sources.get(block_id)? {
                MathBlockSource::Align(align) => align
                    .rows
                    .iter()
                    .flat_map(|row| &row.cells)
                    .find(|cell| &cell.root.id == scope_id)
                    .map(|cell| &cell.root)?,
                MathBlockSource::Equation(_) => return None,
            },
        };
        (root.id == *scope_id).then_some(root)
    }

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
        let mut zero_width_hit = None;
        for projection in self.math_projections_on_page(page_index) {
            if !projection.contains_y(y_pt) {
                continue;
            }
            let Some((layout, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            let geometry = MathTextItemGeometry {
                object_x_pt: projection.object_x_pt,
                object_baseline_y_pt: projection.baseline_y_pt,
                projection,
                items: &layout.items,
                sidecars,
            };
            if let Some(mut hit) = hit_math_text_items(page_index, x_pt, y_pt, &geometry) {
                if !projection.contains_y_for_row(y_pt, math_row_id_for_host(&hit.host)) {
                    continue;
                }
                let Some((normalized_caret, _)) = math_text_caret_in_items(
                    &geometry,
                    &hit.target,
                    &hit.position,
                    hit.visual_affinity,
                ) else {
                    continue;
                };
                let Some(caret) = projection
                    .clip_caret_for_row(normalized_caret, math_row_id_for_host(&hit.host))
                else {
                    continue;
                };
                hit.caret = caret;
                hit.session_id = self.session_id.clone();
                hit.document_generation = self.document_generation;
                hit.font_generation = self.font_generation;
                if hit.is_zero_width {
                    zero_width_hit = Some(hit);
                } else {
                    return Some(hit);
                }
            }
        }
        zero_width_hit
    }

    /// 为通用拖选协议返回本页所有可见数学文本岛的字素停靠点。
    pub fn math_text_drag_stops(&self, page_index: usize) -> Vec<PageMathTextDragStop> {
        let mut stops = Vec::new();
        for projection in self.math_projections_on_page(page_index) {
            let Some((layout, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            let geometry = MathTextItemGeometry {
                object_x_pt: projection.object_x_pt,
                object_baseline_y_pt: projection.baseline_y_pt,
                projection,
                items: &layout.items,
                sidecars,
            };
            for item in &layout.items {
                let caret_math::layout::MathItem::TextIsland { token, .. } = item else {
                    continue;
                };
                let Some(sidecar) = sidecars.get(token) else {
                    continue;
                };
                for &order_utf8 in &sidecar.layout.grapheme_boundaries {
                    for visual_affinity in [VisualAffinity::Upstream, VisualAffinity::Downstream] {
                        let logical_affinity = sidecar
                            .layout
                            .logical_affinity_at(order_utf8, visual_affinity)
                            .map(|affinity| match affinity {
                                HitAffinity::Upstream => MathPositionAffinity::Upstream,
                                HitAffinity::Downstream => MathPositionAffinity::Downstream,
                            })
                            .unwrap_or(MathPositionAffinity::Downstream);
                        let Some(position) =
                            sidecar.position_at_offset(order_utf8, logical_affinity)
                        else {
                            continue;
                        };
                        let Some((mut caret, _)) = math_text_caret_in_items(
                            &geometry,
                            &sidecar.target,
                            &position,
                            visual_affinity,
                        ) else {
                            continue;
                        };
                        let Some((clip_top, clip_bottom)) = projection.clip_range_for_row(
                            caret.y0_pt,
                            caret.y1_pt,
                            math_row_id_for_host(&sidecar.host),
                        ) else {
                            continue;
                        };
                        caret.y0_pt = clip_top;
                        caret.y1_pt = clip_bottom;
                        stops.push(PageMathTextDragStop {
                            token: *token,
                            order_utf8,
                            target: sidecar.target.clone(),
                            position,
                            visual_affinity,
                            caret,
                        });
                    }
                }
            }
        }
        stops
    }

    /// 从当前页同一快照重新解析拖选端点，不接受已过期或不匹配的 UI 位置。
    pub fn math_text_drag_hit(
        &self,
        page_index: usize,
        token: u64,
        target: &MathEditTarget,
        position: &MathTextPosition,
        order_utf8: usize,
        visual_affinity: VisualAffinity,
    ) -> Option<PageMathTextHit> {
        for projection in self.math_projections_on_page(page_index) {
            let Some((layout, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            let Some(sidecar) = sidecars.get(&token) else {
                continue;
            };
            if &sidecar.target != target || sidecar.offset_at_position(position) != Some(order_utf8)
            {
                continue;
            }
            let geometry = MathTextItemGeometry {
                object_x_pt: projection.object_x_pt,
                object_baseline_y_pt: projection.baseline_y_pt,
                projection,
                items: &layout.items,
                sidecars,
            };
            let (caret, _) =
                math_text_caret_in_items(&geometry, target, position, visual_affinity)?;
            let (y0_pt, y1_pt) = projection.clip_range_for_row(
                caret.y0_pt,
                caret.y1_pt,
                math_row_id_for_host(&sidecar.host),
            )?;
            return Some(PageMathTextHit {
                token,
                session_id: self.session_id.clone(),
                document_generation: self.document_generation,
                font_generation: self.font_generation,
                target: target.clone(),
                host: sidecar.host.clone(),
                position: position.clone(),
                visual_affinity,
                is_zero_width: sidecar.layout.metrics.advance_pt <= 0.0,
                caret: PageCaretGeometry {
                    y0_pt,
                    y1_pt,
                    ..caret
                },
            });
        }
        None
    }

    /// 命中同代布局中的数学树序列，并返回其插入位置。
    pub fn hit_test_math_structure(
        &self,
        page_index: usize,
        x_pt: f64,
        y_pt: f64,
    ) -> Option<PageMathStructureHit> {
        if !x_pt.is_finite() || !y_pt.is_finite() {
            return None;
        }
        let page = self.pages.get(page_index)?;
        if x_pt < 0.0 || y_pt < 0.0 || x_pt > page.width_pt || y_pt > page.height_pt {
            return None;
        }
        let mut best: Option<(f64, u8, f64, usize, PageMathStructureHit)> = None;
        for projection in self.math_projections_on_page(page_index) {
            if !projection.contains_y(y_pt) {
                continue;
            }
            let Some((layout, _)) = self.math_projection_items(projection) else {
                continue;
            };
            let Some(host) = self.math_host(&projection.math_id) else {
                continue;
            };
            for node in &layout.nodes {
                if node.is_text_flow || (!node.is_flow && !node.is_container) {
                    continue;
                }
                // 零尺寸可选 Placeholder 仅保留路径身份，不代表可见插入槽。
                if node.width <= 0.0 || node.height <= 0.0 {
                    continue;
                }
                let Some(scope_host) = host.for_scope(&node.scope_id) else {
                    continue;
                };
                if !projection.contains_y_for_row(y_pt, math_row_id_for_host(&scope_host.host)) {
                    continue;
                }
                let Some(root) = self.math_scope_root(projection, &node.scope_id) else {
                    continue;
                };
                let (path, contents, environment_children) = if node.is_flow {
                    let Some(flow) = math_flow_at_layout_path(root, &node.path) else {
                        continue;
                    };
                    let Some(path) = math_sequence_path_for_flow(&node.path) else {
                        continue;
                    };
                    (path, flow.content.as_slice(), false)
                } else {
                    let Some(environment) = math_environment_at_layout_path(root, &node.path)
                    else {
                        continue;
                    };
                    let Some(path) = math_sequence_path_for_environment_children(&node.path) else {
                        continue;
                    };
                    (path, environment.children.as_slice(), true)
                };
                // 空行内宿主不占正文推进，命中区收窄为以公式 caret 居中的插入槽；
                // 槽与相邻正文重叠的部分由正文优先判定吸收。
                let empty_inline_host = projection.inline_index.is_some()
                    && node.path.is_empty()
                    && math_flow_is_empty_host(root);
                let (x0_pt, x1_pt, width_pt) = if empty_inline_host {
                    let slot_width = node
                        .font_size_pt
                        .map_or(node.width, |size| (size * 0.5).max(4.0));
                    let center = projection.object_x_pt + node.x;
                    (
                        center - slot_width / 2.0,
                        center + slot_width / 2.0,
                        slot_width,
                    )
                } else {
                    let x0_pt = projection.object_x_pt + node.x;
                    (x0_pt, x0_pt + node.width, node.width)
                };
                let y0_pt = projection.baseline_y_pt + node.y;
                let height_pt = node.height;
                let y1_pt = y0_pt + height_pt;
                let caret_band = projection.caret_at_math_baseline(
                    x0_pt + width_pt / 2.0,
                    projection.baseline_y_pt + node.baseline,
                );
                let hit_y0_pt = y0_pt.min(caret_band.y0_pt);
                let hit_y1_pt = y1_pt.max(caret_band.y1_pt);
                if !x0_pt.is_finite()
                    || !y0_pt.is_finite()
                    || !x1_pt.is_finite()
                    || !y1_pt.is_finite()
                    || x_pt < x0_pt
                    || x_pt > x1_pt
                {
                    continue;
                }
                let strict_hit = y_pt >= y0_pt && y_pt <= y1_pt;
                if !strict_hit && (y_pt < hit_y0_pt || y_pt > hit_y1_pt) {
                    continue;
                }
                if empty_inline_host && self.point_inside_body_text(page_index, x_pt, y_pt) {
                    continue;
                }
                let child_step = |index| {
                    if environment_children {
                        MathPathStep::EnvironmentChild(index)
                    } else {
                        MathPathStep::Content(index)
                    }
                };
                let raw_offset = if contents.is_empty() {
                    0
                } else {
                    contents
                        .iter()
                        .enumerate()
                        .filter_map(|(index, _)| {
                            let mut child_path = node.path.clone();
                            child_path.push(child_step(index));
                            layout
                                .nodes
                                .iter()
                                .find(|child| {
                                    child.scope_id == node.scope_id && child.path == child_path
                                })
                                .filter(|child| {
                                    child.x.is_finite()
                                        && child.width.is_finite()
                                        && child.width > 0.0
                                })
                                .map(|child| (index, child))
                        })
                        .filter(|(_, child)| {
                            x_pt >= projection.object_x_pt + child.x + child.width / 2.0
                        })
                        .map(|(index, _)| index + 1)
                        .max()
                        .unwrap_or(0)
                };
                let mut offset = raw_offset;
                while !environment_children
                    && offset > 0
                    && matches!(
                        contents.get(offset - 1),
                        Some(MathContent::Math(node))
                            if node.kind == caret_doc::MathNodeKind::Placeholder
                    )
                {
                    offset -= 1;
                }
                let position = MathTreePosition {
                    host: scope_host.host,
                    path,
                    offset,
                    affinity: if offset == 0 {
                        MathPositionAffinity::Downstream
                    } else {
                        MathPositionAffinity::Upstream
                    },
                };
                let Some(caret) = self
                    .math_tree_caret(&host.target, &position)
                    .map(|(caret, _)| caret)
                else {
                    continue;
                };
                let hit = PageMathStructureHit {
                    session_id: self.session_id.clone(),
                    document_generation: self.document_generation,
                    font_generation: self.font_generation,
                    target: host.target.clone(),
                    position,
                    caret,
                };
                let priority = u8::from(!strict_hit);
                let distance = (y_pt - (y0_pt + y1_pt) / 2.0).abs();
                let area = width_pt * height_pt;
                let depth = node.path.len();
                if best.as_ref().is_none_or(
                    |(best_distance, best_priority, best_area, best_depth, _)| {
                        priority < *best_priority
                            || (priority == *best_priority
                                && if priority == 0 {
                                    area < *best_area
                                        || (area == *best_area
                                            && (depth > *best_depth
                                                || (depth == *best_depth
                                                    && distance < *best_distance)))
                                } else {
                                    distance < *best_distance
                                        || (distance == *best_distance
                                            && (area < *best_area
                                                || (area == *best_area && depth > *best_depth)))
                                })
                    },
                ) {
                    best = Some((distance, priority, area, depth, hit));
                }
            }
        }
        best.map(|(_, _, _, _, hit)| hit)
    }

    /// 判断页面点是否落在正宽正文/文本岛字素内部。
    ///
    /// 与 app 命中测试使用同一内缩规则：正文保留正宽字素内部区域，字素间的重合边界
    /// 留给零宽公式 caret。空 InlineMath 的插入槽与相邻文字重叠时据此让正文优先，
    /// 避免吞掉邻字点击。
    fn point_inside_body_text(&self, page_index: usize, x_pt: f64, y_pt: f64) -> bool {
        let Some(hit) = self.hit_test(page_index, x_pt, y_pt) else {
            return false;
        };
        let Some(paragraph) = self.paragraph_by_id(&hit.position.paragraph_id) else {
            return false;
        };
        let boundaries = &paragraph.layout.grapheme_boundaries;
        let Ok(offset_index) = boundaries.binary_search(&hit.position.offset) else {
            return false;
        };
        let mut candidates = Vec::with_capacity(2);
        if let Some(start_index) = offset_index.checked_sub(1) {
            candidates.push((boundaries[start_index], boundaries[offset_index]));
        }
        if let Some(end) = boundaries.get(offset_index + 1) {
            candidates.push((boundaries[offset_index], *end));
        }
        candidates.into_iter().any(|(start, end)| {
            if start >= end
                || paragraph
                    .inline_math_source_ranges
                    .iter()
                    .any(|range| range.start == start && range.end == end)
            {
                return false;
            }
            let selection = Selection {
                anchor: Pos {
                    offset: start,
                    affinity: Affinity::Downstream,
                    ..hit.position.clone()
                },
                focus: Pos {
                    offset: end,
                    affinity: Affinity::Upstream,
                    ..hit.position.clone()
                },
            };
            self.selection_geometry(&selection)
                .is_some_and(|rectangles| {
                    rectangles.into_iter().any(|rectangle| {
                        let inset =
                            1.5_f64.min((rectangle.x1_pt - rectangle.x0_pt).max(0.0) * 0.25);
                        rectangle.page_index == page_index
                            && x_pt > rectangle.x0_pt + inset
                            && x_pt < rectangle.x1_pt - inset
                            && y_pt >= rectangle.y0_pt
                            && y_pt <= rectangle.y1_pt
                    })
                })
        })
    }

    /// 根据同代公式结构插入点返回所属 MathFlow 的页面 caret 几何。
    pub fn math_tree_caret_geometry(
        &self,
        target: &MathEditTarget,
        position: &MathTreePosition,
    ) -> Option<PageCaretGeometry> {
        self.math_tree_caret(target, position)
            .map(|(caret, _)| caret)
    }

    /// 把公式布局节点解析为所在结构序列：路径、序列长度和子项是否为环境子节点。
    ///
    /// MathFlow 聚合成节点（非 TextFlow）对应其内容序列，透明容器或 InlineEnvironment
    /// 对应环境子节点序列；其他布局盒不提供序列位置。
    fn math_layout_sequence(
        &self,
        projection: &MathPageProjection,
        node: &caret_math::layout::MathNodeBox,
    ) -> Option<(MathSequencePath, usize, bool)> {
        if node.is_flow && !node.is_text_flow {
            let root = self.math_scope_root(projection, &node.scope_id)?;
            let flow = math_flow_at_layout_path(root, &node.path)?;
            Some((
                math_sequence_path_for_flow(&node.path)?,
                flow.content.len(),
                false,
            ))
        } else if node.is_container {
            let root = self.math_scope_root(projection, &node.scope_id)?;
            let environment = math_environment_at_layout_path(root, &node.path)?;
            Some((
                math_sequence_path_for_environment_children(&node.path)?,
                environment.children.len(),
                true,
            ))
        } else {
            None
        }
    }

    /// 根据树原生数学插入位置返回同一数学盒中的光标几何和有效字号。
    pub fn math_tree_caret(
        &self,
        target: &MathEditTarget,
        position: &MathTreePosition,
    ) -> Option<(PageCaretGeometry, u16)> {
        for projection in self.math_projections_for_target(target) {
            let Some((layout, _)) = self.math_projection_items(projection) else {
                continue;
            };
            let Some(host) = self.math_host(&projection.math_id) else {
                continue;
            };
            if &host.target != target {
                continue;
            }
            for node in &layout.nodes {
                let Some((sequence_path, sequence_len, environment_children)) =
                    self.math_layout_sequence(projection, node)
                else {
                    continue;
                };
                if !node.x.is_finite()
                    || !node.y.is_finite()
                    || !node.baseline.is_finite()
                    || !node.width.is_finite()
                    || !node.height.is_finite()
                    || node.height < 0.0
                {
                    continue;
                }
                let Some(scope_host) = host.for_scope(&node.scope_id) else {
                    continue;
                };
                if scope_host.host != position.host || sequence_path != position.path {
                    continue;
                }
                if position.offset > sequence_len {
                    continue;
                }
                let caret_font_size = node
                    .font_size_pt
                    .filter(|size| size.is_finite() && *size > 0.0)
                    .unwrap_or(f64::from(self.default_font_size_centipoints) / 100.0);
                let edge_child_indices = math_sequence_edge_child_indices(
                    position.offset,
                    sequence_len,
                    position.affinity,
                );
                let mut edge_geometry = None;
                for child_index in edge_child_indices.into_iter().flatten() {
                    let mut child_path = node.path.clone();
                    child_path.push(if environment_children {
                        MathPathStep::EnvironmentChild(child_index)
                    } else {
                        MathPathStep::Content(child_index)
                    });
                    if let Some(child) = layout
                        .nodes
                        .iter()
                        .find(|child| child.scope_id == node.scope_id && child.path == child_path)
                    {
                        if child.is_container {
                            let nested_position = self
                                .math_scope_root(projection, &node.scope_id)
                                .and_then(|root| {
                                    math_environment_at_layout_path(root, &child.path).and_then(
                                        |environment| {
                                            math_sequence_path_for_environment_children(&child.path)
                                                .map(|path| {
                                                    let at_start = child_index == position.offset;
                                                    MathTreePosition {
                                                        host: position.host.clone(),
                                                        path,
                                                        offset: if at_start {
                                                            0
                                                        } else {
                                                            environment.children.len()
                                                        },
                                                        affinity: if at_start {
                                                            MathPositionAffinity::Downstream
                                                        } else {
                                                            MathPositionAffinity::Upstream
                                                        },
                                                    }
                                                })
                                        },
                                    )
                                });
                            if let Some(caret) = nested_position
                                .as_ref()
                                .and_then(|nested| self.math_tree_caret(target, nested))
                            {
                                return Some(caret);
                            }
                        }
                        if !child.x.is_finite()
                            || !child.y.is_finite()
                            || !child.height.is_finite()
                            || child.height <= 0.0
                        {
                            continue;
                        }
                        if child.is_text_flow {
                            let text_path = MathTextFlowPath::from_layout_path(
                                child.path.clone(),
                                scope_host.align_cell,
                            );
                            let text_length = self
                                .math_scope_root(projection, &node.scope_id)
                                .and_then(|root| math_text_flow_at_layout_path(root, &child.path))
                                .map(|text| {
                                    text.runs.iter().map(|run| run.text.len()).sum::<usize>()
                                });
                            if let Some(text_length) = text_length {
                                let (offset, affinity) = if child_index == position.offset {
                                    (0, MathPositionAffinity::Downstream)
                                } else {
                                    (text_length, MathPositionAffinity::Upstream)
                                };
                                let text_position =
                                    MathTextPosition::new(text_path, offset, affinity);
                                let visual_affinity = match affinity {
                                    MathPositionAffinity::Upstream => VisualAffinity::Upstream,
                                    MathPositionAffinity::Downstream => VisualAffinity::Downstream,
                                };
                                if let Some(caret) = self
                                    .math_text_caret_with_clipping(
                                        target,
                                        &text_position,
                                        visual_affinity,
                                        true,
                                    )
                                    .map(|(caret, _)| caret)
                                {
                                    let font_size_centipoints = child
                                        .font_size_pt
                                        .and_then(|size| visible_font_size_centipoints(size as f32))
                                        .unwrap_or(self.default_font_size_centipoints);
                                    return Some((caret, font_size_centipoints));
                                }
                            }
                        }
                        if edge_geometry.is_none() {
                            edge_geometry = Some((
                                if child_index == position.offset {
                                    child.x
                                } else {
                                    child.x + child.width
                                },
                                child.baseline,
                                child.font_size_pt,
                            ));
                        }
                        break;
                    }
                }
                let (local_x, caret_baseline, caret_font_size) =
                    edge_geometry.unwrap_or_else(|| {
                        let local_x = if position.offset == 0 {
                            node.x
                        } else if position.offset == sequence_len {
                            node.x + node.width
                        } else {
                            let mut child_path = node.path.clone();
                            child_path.push(if environment_children {
                                MathPathStep::EnvironmentChild(position.offset)
                            } else {
                                MathPathStep::Content(position.offset)
                            });
                            layout
                                .nodes
                                .iter()
                                .find(|child| {
                                    child.scope_id == node.scope_id && child.path == child_path
                                })
                                .map_or(node.x + node.width, |child| child.x)
                        };
                        (local_x, node.baseline, Some(caret_font_size))
                    });
                let raw_caret = projection.caret_at_math_baseline(
                    projection.object_x_pt + local_x,
                    projection.baseline_y_pt + caret_baseline,
                );
                let Some(caret) =
                    projection.clip_caret_for_row(raw_caret, math_row_id_for_host(&position.host))
                else {
                    continue;
                };
                let font_size_centipoints = caret_font_size
                    .and_then(|size| visible_font_size_centipoints(size as f32))
                    .unwrap_or(self.default_font_size_centipoints);
                return Some((caret, font_size_centipoints));
            }
        }
        None
    }

    /// 返回公式树内部同一序列中锚点到焦点的页面选区矩形。
    ///
    /// 只接受同一宿主、同一序列路径的端点：扩选不进入嵌套结构，焦点离开锚点所在
    /// 序列时由调用方转换为整个公式宿主的文档范围，因此跨路径端点不属于本查询。
    /// 每个被选结构项独立投影：嵌套 Math/Environment 作为原子取整盒；纵向范围使用
    /// 同位置 caret 的正文行框，Align 按所属行裁切。空范围返回空矩形而不是过期投影。
    pub fn math_tree_selection_geometry(
        &self,
        target: &MathEditTarget,
        anchor: &MathTreePosition,
        focus: &MathTreePosition,
    ) -> Option<Vec<PageSelectionRect>> {
        if anchor.host != focus.host || anchor.path != focus.path {
            return None;
        }
        let (start, end) = if anchor.offset <= focus.offset {
            (anchor.offset, focus.offset)
        } else {
            (focus.offset, anchor.offset)
        };
        if start == end {
            return Some(Vec::new());
        }
        for projection in self.math_projections_for_target(target) {
            let Some((layout, _)) = self.math_projection_items(projection) else {
                continue;
            };
            let Some(host) = self.math_host(&projection.math_id) else {
                continue;
            };
            if &host.target != target {
                continue;
            }
            for node in &layout.nodes {
                let Some((sequence_path, sequence_len, environment_children)) =
                    self.math_layout_sequence(projection, node)
                else {
                    continue;
                };
                let Some(scope_host) = host.for_scope(&node.scope_id) else {
                    continue;
                };
                if scope_host.host != anchor.host || sequence_path != anchor.path {
                    continue;
                }
                if end > sequence_len {
                    continue;
                }
                let row_id = math_row_id_for_host(&anchor.host);
                let mut rectangles = Vec::with_capacity(end - start);
                for index in start..end {
                    let mut child_path = node.path.clone();
                    child_path.push(if environment_children {
                        MathPathStep::EnvironmentChild(index)
                    } else {
                        MathPathStep::Content(index)
                    });
                    let Some(child) = layout
                        .nodes
                        .iter()
                        .find(|child| child.scope_id == node.scope_id && child.path == child_path)
                    else {
                        continue;
                    };
                    if !child.x.is_finite()
                        || !child.width.is_finite()
                        || !child.baseline.is_finite()
                        || child.width <= 0.0
                    {
                        continue;
                    }
                    let x0_pt = projection.object_x_pt + child.x;
                    let x1_pt = x0_pt + child.width;
                    let caret_band = projection
                        .caret_at_math_baseline(x0_pt, projection.baseline_y_pt + child.baseline);
                    let Some((y0_pt, y1_pt)) =
                        projection.clip_range_for_row(caret_band.y0_pt, caret_band.y1_pt, row_id)
                    else {
                        continue;
                    };
                    rectangles.push(PageSelectionRect {
                        page_index: projection.page_index,
                        x0_pt,
                        y0_pt,
                        x1_pt,
                        y1_pt,
                    });
                }
                return Some(rectangles);
            }
        }
        None
    }

    /// 根据已提交 TextFlow 位置返回同代文本岛 Parley 几何中的数学光标。
    pub fn math_text_caret(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
    ) -> Option<(PageCaretGeometry, u16)> {
        self.math_text_caret_with_clipping(target, position, visual_affinity, true)
    }

    /// 返回未按宿主行框裁切的数学 caret，用于保留多行文本的视觉顺序。
    pub fn math_text_caret_geometry_for_visual_order(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
    ) -> Option<PageCaretGeometry> {
        self.math_text_caret_with_clipping(target, position, visual_affinity, false)
            .map(|(caret, _)| caret)
    }

    fn math_text_caret_with_clipping(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        clip: bool,
    ) -> Option<(PageCaretGeometry, u16)> {
        for projection in self.math_projections_for_target(target) {
            let Some((layout, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            let geometry = MathTextItemGeometry {
                object_x_pt: projection.object_x_pt,
                object_baseline_y_pt: projection.baseline_y_pt,
                projection,
                items: &layout.items,
                sidecars,
            };
            if let Some((caret, size)) =
                math_text_caret_in_items(&geometry, target, position, visual_affinity)
            {
                return if clip {
                    projection
                        .clip_caret_for_row(caret, self.math_text_row_id(target, position))
                        .map(|caret| (caret, size))
                } else {
                    Some((caret, size))
                };
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

    pub(crate) fn math_text_host(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
    ) -> Option<MathPositionHost> {
        for projection in self.math_projections_for_target(target) {
            let Some((_, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            for sidecar in sidecars.values() {
                if &sidecar.target == target && sidecar.offset_at_position(position).is_some() {
                    return Some(sidecar.host.clone());
                }
            }
        }
        None
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
        for projection in self.math_projections_for_target(target) {
            let Some((layout, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            let geometry = MathTextItemGeometry {
                object_x_pt: projection.object_x_pt,
                object_baseline_y_pt: projection.baseline_y_pt,
                projection,
                items: &layout.items,
                sidecars,
            };
            if let Some(rectangles) =
                math_text_selection_in_items(&geometry, target, anchor, focus, composition)
            {
                return Some(
                    rectangles
                        .into_iter()
                        .filter_map(|mut rect| {
                            (rect.y0_pt, rect.y1_pt) = projection.clip_range_for_row(
                                rect.y0_pt,
                                rect.y1_pt,
                                self.math_text_row_id(target, anchor),
                            )?;
                            Some(rect)
                        })
                        .collect(),
                );
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

    fn adjacent_math_text_hit(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        line_delta: isize,
        preferred_x: Option<f32>,
    ) -> Option<PageMathTextHit> {
        if line_delta == 0 {
            return None;
        }
        let current = self.math_text_caret_geometry(target, position, visual_affinity)?;
        let current_host = self.math_text_host(target, position)?;
        let (current_path, current_offset) =
            MathTreePosition::sequence_boundary_from_text_path(position.path.steps()).ok()?;
        let preferred_x = preferred_x.unwrap_or(current.x_pt as f32);
        if !preferred_x.is_finite() {
            return None;
        }
        let current_y = (current.y0_pt + current.y1_pt) / 2.0;
        let mut best: Option<((usize, f64, f64), PageMathTextHit)> = None;
        for projection in self.math_projections_for_target(target) {
            let Some((layout, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            for item in &layout.items {
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
                if &sidecar.target != target {
                    continue;
                }
                if sidecar.layout.text.is_empty() {
                    let Some(next_position) =
                        sidecar.position_at_offset(0, MathPositionAffinity::Downstream)
                    else {
                        continue;
                    };
                    if !math_navigation::math_text_candidate_is_vertical_neighbor(
                        &current_host,
                        current_path.steps(),
                        current_offset,
                        &sidecar.host,
                        &next_position.path,
                    ) {
                        continue;
                    }
                    let Some((caret, _)) = math_text_caret_in_items(
                        &MathTextItemGeometry {
                            object_x_pt: projection.object_x_pt,
                            object_baseline_y_pt: projection.baseline_y_pt,
                            projection,
                            items: &layout.items,
                            sidecars,
                        },
                        target,
                        &next_position,
                        visual_affinity,
                    ) else {
                        continue;
                    };
                    let Some((clip_top, clip_bottom)) = projection.clip_range_for_row(
                        caret.y0_pt,
                        caret.y1_pt,
                        math_row_id_for_host(&sidecar.host),
                    ) else {
                        continue;
                    };
                    let center_y = (clip_top + clip_bottom) / 2.0;
                    let is_ahead = if line_delta > 0 {
                        (projection.page_index, center_y) > (current.page_index, current_y)
                    } else {
                        (projection.page_index, center_y) < (current.page_index, current_y)
                    };
                    let key = (
                        projection.page_index.abs_diff(current.page_index),
                        if projection.page_index == current.page_index {
                            (center_y - current_y).abs()
                        } else {
                            0.0
                        },
                        (caret.x_pt - f64::from(preferred_x)).abs(),
                    );
                    if !is_ahead
                        || sidecar
                            .position_at_offset(0, MathPositionAffinity::Downstream)
                            .as_ref()
                            == Some(position)
                        || best.as_ref().is_some_and(|(best_key, _)| key >= *best_key)
                    {
                        continue;
                    }
                    best = Some((
                        key,
                        PageMathTextHit {
                            token: *token,
                            session_id: self.session_id.clone(),
                            document_generation: self.document_generation,
                            font_generation: self.font_generation,
                            target: target.clone(),
                            host: sidecar.host.clone(),
                            position: next_position,
                            visual_affinity,
                            is_zero_width: true,
                            caret: PageCaretGeometry {
                                page_index: projection.page_index,
                                x_pt: caret.x_pt,
                                y0_pt: clip_top,
                                y1_pt: clip_bottom,
                            },
                        },
                    ));
                    continue;
                }
                let Some(first_line) = sidecar.layout.layout.lines().next() else {
                    continue;
                };
                let island_x = projection.object_x_pt + *x;
                let island_baseline = projection.baseline_y_pt + *baseline_y;
                for line in sidecar.layout.layout.lines() {
                    let metrics = line.metrics();
                    let local_y = (metrics.block_min_coord + metrics.block_max_coord) / 2.0;
                    let local_x = (f64::from(preferred_x) - island_x) as f32;
                    let Some(hit) = sidecar.layout.cursor_from_point(local_x, local_y) else {
                        continue;
                    };
                    let logical_affinity = match hit.logical_affinity {
                        HitAffinity::Upstream => MathPositionAffinity::Upstream,
                        HitAffinity::Downstream => MathPositionAffinity::Downstream,
                    };
                    let Some(next_position) =
                        sidecar.position_at_offset(hit.cursor.byte_index(), logical_affinity)
                    else {
                        continue;
                    };
                    if !math_navigation::math_text_candidate_is_vertical_neighbor(
                        &current_host,
                        current_path.steps(),
                        current_offset,
                        &sidecar.host,
                        &next_position.path,
                    ) {
                        continue;
                    }
                    let Some((bounds, line_index)) =
                        sidecar.layout.cursor_geometry_with_line(hit.cursor, 0.0)
                    else {
                        continue;
                    };
                    let Some(selected_line) = sidecar.layout.layout.get(line_index) else {
                        continue;
                    };
                    let selected_baseline = island_baseline
                        + f64::from(
                            selected_line.metrics().baseline - first_line.metrics().baseline,
                        );
                    let caret =
                        projection.caret_at_math_baseline(island_x + bounds.x0, selected_baseline);
                    let Some((clip_top, clip_bottom)) = projection.clip_range_for_row(
                        caret.y0_pt,
                        caret.y1_pt,
                        math_row_id_for_host(&sidecar.host),
                    ) else {
                        continue;
                    };
                    let center_y = (clip_top + clip_bottom) / 2.0;
                    let is_forward = line_delta > 0;
                    let is_ahead = if is_forward {
                        (projection.page_index, center_y) > (current.page_index, current_y)
                    } else {
                        (projection.page_index, center_y) < (current.page_index, current_y)
                    };
                    if !is_ahead {
                        continue;
                    }
                    let key = (
                        projection.page_index.abs_diff(current.page_index),
                        if projection.page_index == current.page_index {
                            (center_y - current_y).abs()
                        } else {
                            0.0
                        },
                        (caret.x_pt - f64::from(preferred_x)).abs(),
                    );
                    if sidecar
                        .position_at_offset(hit.cursor.byte_index(), logical_affinity)
                        .as_ref()
                        == Some(position)
                        || best.as_ref().is_some_and(|(best_key, _)| key >= *best_key)
                    {
                        continue;
                    }
                    let candidate = PageMathTextHit {
                        token: *token,
                        session_id: self.session_id.clone(),
                        document_generation: self.document_generation,
                        font_generation: self.font_generation,
                        target: target.clone(),
                        host: sidecar.host.clone(),
                        position: next_position,
                        visual_affinity: hit.cursor.affinity(),
                        is_zero_width: sidecar.layout.metrics.advance_pt <= 0.0,
                        caret: PageCaretGeometry {
                            page_index: projection.page_index,
                            x_pt: caret.x_pt,
                            y0_pt: clip_top,
                            y1_pt: clip_bottom,
                        },
                    };
                    best = Some((key, candidate));
                }
            }
        }
        best.map(|(_, hit)| hit)
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
            let vertical_origin_x = if matches!(movement, MathTextMoveRequest::Vertical(_, _)) {
                let local_x = sidecar.layout.cursor_geometry(cursor, 0.0)?.x0;
                let page_x = self
                    .math_text_caret_geometry(target, position, visual_affinity)?
                    .x_pt;
                Some((page_x - local_x) as f32)
            } else {
                None
            };
            let Some((next_cursor, preferred_x)) = (match movement {
                MathTextMoveRequest::Visual(forward) => sidecar
                    .layout
                    .move_visual(cursor, forward)
                    .map(|cursor| (cursor, None)),
                MathTextMoveRequest::Vertical(line_delta, preferred_x) => sidecar
                    .layout
                    .move_vertical(
                        cursor,
                        line_delta,
                        preferred_x.map(|x| x - vertical_origin_x.unwrap_or(0.0)),
                    )
                    .map(|(cursor, preferred_x)| {
                        (cursor, Some(preferred_x + vertical_origin_x.unwrap_or(0.0)))
                    }),
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
    fn structure_selection_geometry(
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

    /// 返回选中块级公式在同一代布局中的整栏矩形。
    ///
    /// 每个矩形横跨页面版心宽度，纵向采用该公式片段的 MathBox 范围并裁切到页面，
    /// 供选区层与活动公式高亮共用同一几何来源；不进入公式内部 MathBox。
    /// 任一公式 ID 在当前快照中没有任何分页片段时返回 `None`，让调用方拒绝过期投影。
    ///
    /// 退化片段（零高度或裁切后纵向区间为空）在当前布局契约下不可达：Equation
    /// 无 soft break 时宿主行框由正文行网格撑开正高度（`calculate_math_host_metrics`），
    /// 含 soft break 的 Equation 与 Align 由 `calculate_math_align_host_metrics`
    /// 拒绝非正高度，分页再拒绝负高度；即使公式行高于整页，页边距校验也保证裁切后
    /// 仍为正高度。因此下面的跳过分支只是防御性检查，不应据此断言快照过期。
    pub fn math_block_geometry(&self, block_ids: &[NodeId]) -> Option<Vec<PageSelectionRect>> {
        if block_ids.is_empty() {
            return Some(Vec::new());
        }
        let mut found = vec![false; block_ids.len()];
        let mut rectangles = Vec::new();
        for (page_index, page) in self.pages.iter().enumerate() {
            let x0_pt = page.margin_left_pt;
            let x1_pt = page.width_pt - page.margin_right_pt;
            if !x0_pt.is_finite() || !x1_pt.is_finite() || x1_pt <= x0_pt {
                return None;
            }
            for fragment in &page.layout.math_fragments {
                let Some(index) = block_ids
                    .iter()
                    .position(|block_id| block_id == &fragment.math_id)
                else {
                    continue;
                };
                let y0_pt = (page.margin_top_pt + f64::from(fragment.y_offset_pt))
                    .clamp(0.0, page.height_pt);
                let y1_pt = (page.margin_top_pt
                    + f64::from(fragment.y_offset_pt)
                    + f64::from(fragment.height_pt))
                .clamp(0.0, page.height_pt);
                if !y0_pt.is_finite() || !y1_pt.is_finite() || y1_pt <= y0_pt {
                    continue;
                }
                // 只有真正产出矩形后才标记该 ID 已定位，退化片段不能让整体误判为成功。
                // 当前布局契约保证片段高度为正且裁切区间非空，见函数文档。
                found[index] = true;
                rectangles.push(PageSelectionRect {
                    page_index,
                    x0_pt,
                    y0_pt,
                    x1_pt,
                    y1_pt,
                });
            }
        }
        found.iter().all(|found| *found).then_some(rectangles)
    }

    /// 返回结构选区在同一代布局中的全部可见矩形：环境外正文、被选块环境与公式块。
    ///
    /// 文本端点必须与本快照同代；选中的公式 ID 任一缺少分页片段时整体
    /// 返回 `None`。空块环境没有排版内容，因此没有环境矩形，这不视为过期投影。
    pub fn structure_selection_rectangles(
        &self,
        anchor: &Pos,
        focus: &Pos,
        selected_environment_ids: &[NodeId],
        selected_math_block_ids: &[NodeId],
    ) -> Option<Vec<PageSelectionRect>> {
        let mut rectangles =
            self.structure_selection_geometry(anchor, focus, selected_environment_ids)?;
        rectangles.extend(
            self.environment_ranges
                .iter()
                .filter(|range| selected_environment_ids.contains(&range.environment_id))
                .map(|range| PageSelectionRect {
                    page_index: range.page_index,
                    x0_pt: range.x0_pt,
                    y0_pt: range.y0_pt,
                    x1_pt: range.x1_pt,
                    y1_pt: range.y1_pt,
                }),
        );
        rectangles.extend(self.math_block_geometry(selected_math_block_ids)?);
        Some(rectangles)
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
                let x0_pt = bounds.x0 + page.margin_left_pt;
                let x1_pt = bounds.x1 + page.margin_left_pt;
                // Qt 快照校验拒绝零宽几何；退化行内对象不应让整份选区投影失败。
                if x1_pt <= x0_pt || y1 <= y0 {
                    continue;
                }
                rectangles.push(PageSelectionRect {
                    page_index: location.page_index,
                    x0_pt,
                    y0_pt: y0,
                    x1_pt,
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
        self.layout_session_reusing_paragraphs(
            session,
            previous,
            Some(changed_paragraph_id),
            catalog,
            base_style,
        )
    }

    /// 复用所有正文段落塑形结果，并重算含块级公式的页面几何。
    pub fn layout_session_reusing_all_paragraphs(
        &mut self,
        session: &Session,
        previous: &DocumentLayoutSnapshot,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        self.layout_session_reusing_paragraphs(session, previous, None, catalog, base_style)
    }

    fn layout_session_reusing_paragraphs(
        &mut self,
        session: &Session,
        previous: &DocumentLayoutSnapshot,
        changed_paragraph_id: Option<&NodeId>,
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
        reuse: Option<(&DocumentLayoutSnapshot, Option<&NodeId>)>,
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
        let math_font_diagnostic = document_contains_math(&document.body)
            .then(|| text_settings.font_preferences().math_family())
            .flatten()
            .filter(|family| {
                !family.eq_ignore_ascii_case(caret_font_roles::DEFAULT_MATH_FONT_FAMILY)
            })
            .map(|requested_family| MathFontDiagnostic {
                code: if catalog
                    .families()
                    .iter()
                    .any(|family| family.name.eq_ignore_ascii_case(requested_family))
                {
                    "math.font.unsupported_family"
                } else {
                    "math.font.family_missing"
                },
                requested_family: requested_family.to_owned(),
            });
        if document_contains_math(&document.body) && self.math_layout_engine.is_none() {
            self.math_layout_engine = Some(
                caret_math::layout::MathLayoutEngine::new()
                    .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
            );
        }
        let default_paragraph_style = resolve_paragraph_style(&document.global_settings, None)
            .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
        let default_line_spacing = default_paragraph_style.line_spacing;
        let line_grid_metrics = self.resolve_line_grid_metrics(
            catalog,
            &text_settings,
            base_style,
            text_settings.default_language(),
        )?;
        let default_caret_metrics =
            calculate_line_metrics_with_grid(line_grid_metrics, &[], &[], default_line_spacing)
                .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
        let mut ordered_inputs = Vec::new();
        {
            let mut math_context = MathLayoutContext {
                engine: self.math_layout_engine.as_ref(),
                catalog,
                settings: &text_settings,
                base_style,
                text_layouter: &mut self.layouter,
                font_size_centipoints: base_style.font_size_centipoints,
                content_width_pt: content_width_pt as f32,
                line_grid_metrics,
                line_spacing: default_line_spacing,
                space_before_pt: default_paragraph_style.space_before_pt,
                space_after_pt: default_paragraph_style.space_after_pt,
                caret_metrics: default_caret_metrics,
            };
            collect_blocks(
                &document.body,
                &mut ordered_inputs,
                &mut Vec::new(),
                &mut math_context,
                reuse.map(|(previous, _)| previous),
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
                && changed_paragraph_id.is_none_or(|changed_id| &paragraph.id != changed_id)
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
            let paragraph_caret_metrics =
                calculate_line_metrics_with_grid(line_grid_metrics, &[], &[], style.line_spacing)
                    .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
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
                    text_layouter: &mut self.layouter,
                    font_size_centipoints: base_style.font_size_centipoints,
                    content_width_pt: content_width_pt as f32,
                    line_grid_metrics,
                    line_spacing: style.line_spacing,
                    space_before_pt: style.space_before_pt,
                    space_after_pt: style.space_after_pt,
                    caret_metrics: paragraph_caret_metrics,
                };
                let previous_paragraph =
                    reuse.and_then(|(previous, _)| previous.paragraph_by_id(&paragraph.id));
                collect_inline_math_layouts(paragraph, &mut inline_context, previous_paragraph)?
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
                math_caret_height_pt: paragraph_caret_metrics.height_pt,
                math_caret_baseline_pt: paragraph_caret_metrics.baseline_pt,
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
        let math_block_sources = ordered_inputs
            .iter()
            .filter_map(|item| match item {
                LayoutInput::MathBlock {
                    math_id, source, ..
                } => Some((math_id.clone(), source.as_ref().clone())),
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
                    baseline_pt,
                    caret_baseline_pt,
                    caret_height_pt,
                    clip_ink_to_line_height,
                    space_before_pt,
                    space_after_pt,
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
                            baseline_pt,
                            caret_baseline_pt,
                            caret_height_pt,
                            clip_ink_to_line_height,
                            space_before_pt,
                            space_after_pt,
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
        let mut math_hosts = HashMap::new();
        collect_math_hosts(&document.body, &mut math_hosts);
        let math_projections =
            collect_math_projections(&pages, &paragraphs, &paragraph_lookup, &math_blocks);
        let math_projection_lookup = math_projections
            .iter()
            .enumerate()
            .map(|(index, projection)| ((projection.page_index, projection.math_id.clone()), index))
            .collect();
        let text_diagnostics = collect_text_layout_diagnostics(&paragraphs, &math_blocks);
        let mut math_projections_by_id: HashMap<NodeId, Vec<usize>> = HashMap::new();
        for (index, projection) in math_projections.iter().enumerate() {
            math_projections_by_id
                .entry(projection.math_id.clone())
                .or_default()
                .push(index);
        }
        Ok(DocumentLayoutSnapshot {
            session_id: None,
            document_generation,
            font_generation: catalog.generation(),
            math_font_diagnostic,
            text_diagnostics,
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
            math_block_sources,
            math_hosts,
            math_projections,
            math_projection_lookup,
            math_projections_by_id,
        })
    }
}

fn collect_math_hosts(blocks: &[Block], hosts: &mut HashMap<NodeId, MathTextLayoutHost>) {
    fn inline(
        nodes: &[ParagraphContent],
        paragraph_id: &NodeId,
        hosts: &mut HashMap<NodeId, MathTextLayoutHost>,
    ) {
        for node in nodes {
            match node {
                FlowContent::Math(math) => {
                    hosts.insert(
                        math.id.clone(),
                        MathTextLayoutHost::inline(
                            paragraph_id.clone(),
                            math.id.clone(),
                            math.math_tree.root.id.clone(),
                        ),
                    );
                }
                FlowContent::Environment(environment) => {
                    inline(&environment.children, paragraph_id, hosts)
                }
                _ => {}
            }
        }
    }
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => inline(&paragraph.inlines, &paragraph.id, hosts),
            Block::Environment(environment) => {
                if let Some(title) = &environment.title {
                    inline(&title.inlines, &title.id, hosts);
                }
                collect_math_hosts(&environment.children, hosts);
            }
            Block::Equation(equation) => {
                hosts.insert(
                    equation.id.clone(),
                    MathTextLayoutHost::equation(
                        equation.id.clone(),
                        equation.math_tree.root.id.clone(),
                    ),
                );
            }
            Block::Align(align) => {
                hosts.insert(
                    align.id.clone(),
                    MathTextLayoutHost::align(align.id.clone(), align),
                );
            }
            _ => {}
        }
    }
}

fn collect_math_projections(
    pages: &[PageLayoutSnapshot],
    paragraphs: &[LaidOutParagraph],
    paragraph_lookup: &HashMap<NodeId, usize>,
    math_blocks: &HashMap<NodeId, Arc<caret_math::layout::MathLayout>>,
) -> Vec<MathPageProjection> {
    let mut projections = Vec::new();
    for (page_index, page) in pages.iter().enumerate() {
        for fragment in &page.layout.math_fragments {
            if math_blocks.contains_key(&fragment.math_id) {
                projections.push(MathPageProjection {
                    math_id: fragment.math_id.clone(),
                    page_index,
                    object_x_pt: page.margin_left_pt + f64::from(fragment.x_offset_pt),
                    baseline_y_pt: page.margin_top_pt
                        + f64::from(fragment.y_offset_pt)
                        + f64::from(fragment.baseline_pt),
                    caret_line_top_pt: page.margin_top_pt + f64::from(fragment.y_offset_pt),
                    caret_baseline_y_pt: page.margin_top_pt
                        + f64::from(fragment.y_offset_pt)
                        + f64::from(fragment.caret_baseline_pt),
                    caret_height_pt: f64::from(fragment.caret_height_pt),
                    clip_y_pt: fragment.clip_ink_to_line_height.then_some([
                        page.margin_top_pt + f64::from(fragment.y_offset_pt),
                        page.margin_top_pt
                            + f64::from(fragment.y_offset_pt)
                            + f64::from(fragment.height_pt),
                    ]),
                    row_clips: math_blocks
                        .get(&fragment.math_id)
                        .into_iter()
                        .flat_map(|layout| &layout.row_clips)
                        .filter_map(|clip| {
                            Some(MathPageRowClip {
                                row_id: clip.row_id.clone()?,
                                y0_pt: page.margin_top_pt
                                    + f64::from(fragment.y_offset_pt)
                                    + f64::from(fragment.baseline_pt)
                                    + clip.y0,
                                y1_pt: page.margin_top_pt
                                    + f64::from(fragment.y_offset_pt)
                                    + f64::from(fragment.baseline_pt)
                                    + clip.y1,
                            })
                        })
                        .collect(),
                    row_baselines: math_blocks
                        .get(&fragment.math_id)
                        .into_iter()
                        .flat_map(|layout| &layout.row_baselines)
                        .map(|row| MathPageRowBaseline {
                            baseline_y_pt: page.margin_top_pt
                                + f64::from(fragment.y_offset_pt)
                                + f64::from(fragment.baseline_pt)
                                + row.baseline,
                        })
                        .collect(),
                    inline_index: None,
                });
            }
        }
        for fragment in &page.layout.fragments {
            let Some(paragraph) = paragraph_lookup
                .get(&fragment.paragraph_id)
                .and_then(|index| paragraphs.get(*index))
            else {
                continue;
            };
            let mut line_top = page.margin_top_pt + f64::from(fragment.y_offset_pt);
            for line_index in fragment.line_range.clone() {
                let Some(line_box) = paragraph.layout.paragraph.lines.get(line_index) else {
                    continue;
                };
                let height = f64::from(line_box.metrics.height_pt);
                if let (Some(line), Some(raw_top), Some(adjustment)) = (
                    paragraph.layout.layout.get(line_index),
                    paragraph.layout.parley_line_tops_pt.get(line_index),
                    paragraph.layout.baseline_adjustments_pt.get(line_index),
                ) {
                    for item in line.items() {
                        let PositionedGlyphLayoutItem::InlineBox(inline_box) = item else {
                            continue;
                        };
                        let Ok(index) = usize::try_from(inline_box.id) else {
                            continue;
                        };
                        let Some(math_id) = paragraph.inline_math_ids.get(index) else {
                            continue;
                        };
                        projections.push(MathPageProjection {
                            math_id: math_id.clone(),
                            page_index,
                            object_x_pt: page.margin_left_pt + f64::from(inline_box.x),
                            baseline_y_pt: line_top
                                + f64::from(inline_box.y)
                                + f64::from(inline_box.baseline.unwrap_or(inline_box.height))
                                - f64::from(*raw_top)
                                + f64::from(*adjustment),
                            caret_line_top_pt: line_top,
                            caret_baseline_y_pt: line_top
                                + f64::from(paragraph.math_caret_baseline_pt),
                            caret_height_pt: f64::from(paragraph.math_caret_height_pt),
                            clip_y_pt: line_box
                                .metrics
                                .clip_ink_to_line_height
                                .then_some([line_top, line_top + height]),
                            row_clips: Vec::new(),
                            row_baselines: Vec::new(),
                            inline_index: Some(index),
                        });
                    }
                }
                line_top += height;
            }
        }
    }
    projections
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
    object_x_pt: f64,
    object_baseline_y_pt: f64,
    projection: &'a MathPageProjection,
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
        let island_baseline_y_pt = geometry.object_baseline_y_pt + baseline_y;
        let (caret_x_pt, text_baseline_y_pt) = if sidecar.layout.text.is_empty() {
            sidecar.empty_caret_metrics?;
            (0.0, island_baseline_y_pt)
        } else {
            let (bounds, line_index) = sidecar.layout.cursor_geometry_with_line(
                sidecar.layout.cursor_at(offset, visual_affinity)?,
                0.0,
            )?;
            let first_baseline = sidecar.layout.layout.lines().next()?.metrics().baseline;
            let line_baseline = sidecar.layout.layout.get(line_index)?.metrics().baseline;
            (
                bounds.x0,
                island_baseline_y_pt + f64::from(line_baseline - first_baseline),
            )
        };
        let island_x_pt = geometry.object_x_pt + *x + caret_x_pt;
        let caret = geometry
            .projection
            .caret_at_math_baseline(island_x_pt, text_baseline_y_pt);
        return Some((
            caret,
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
                    page_index: geometry.projection.page_index,
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
    geometry: &MathTextItemGeometry<'_>,
) -> Option<PageMathTextHit> {
    let mut zero_width_hit = None;
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
        if !geometry
            .projection
            .contains_y_for_row(page_y_pt, math_row_id_for_host(&sidecar.host))
        {
            continue;
        }
        let island_x_pt = geometry.object_x_pt + x;
        let island_baseline_y_pt = geometry.object_baseline_y_pt + baseline_y;
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
                token: *token,
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
            token: *token,
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
                y0_pt: island_baseline_y_pt + caret_bounds.y0 - f64::from(line.metrics().baseline),
                y1_pt: island_baseline_y_pt + caret_bounds.y1 - f64::from(line.metrics().baseline),
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

    /// 复用全部正文段落塑形结果，并按当前公式重新计算分页和页面投影。
    pub fn layout_session_reusing_all_paragraphs(
        &mut self,
        session: &Session,
        previous: &DocumentLayoutSnapshot,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        let base_style = ShapingStyle::new(
            session
                .document()
                .global_settings
                .default_font_size_centipoints,
        );
        self.service.layout_session_reusing_all_paragraphs(
            session,
            previous,
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

/// 判断公式根流是否为空宿主：内容为空或仅一个根占位符。
///
/// 与 caret-math `is_empty_host_flow` 的判据保持一致；空宿主在行内流中不占推进，
/// 只保留 MathNodeBox 作为公式 caret/命中锚点。
pub(crate) fn math_flow_is_empty_host(flow: &MathFlow) -> bool {
    flow.content.is_empty()
        || matches!(
            flow.content.as_slice(),
            [MathContent::Math(node)] if node.kind == caret_doc::MathNodeKind::Placeholder
        )
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
pub(crate) struct MathTextScopeHost {
    pub(crate) host: MathPositionHost,
    pub(crate) align_cell: Option<(usize, usize)>,
}

#[derive(Debug, Clone)]
pub(crate) struct MathTextLayoutHost {
    pub(crate) target: MathEditTarget,
    pub(crate) scopes: HashMap<NodeId, MathTextScopeHost>,
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
    text_layouter: &'a mut ParagraphLayouter,
    host: MathTextLayoutHost,
    next_token: u64,
    sidecars: HashMap<u64, MathTextSidecar>,
}

impl<'a> EngineMathTextShaper<'a> {
    fn new(
        catalog: &'a mut FontCatalog,
        settings: &'a ResolvedDocumentTextSettings,
        base_style: &'a ShapingStyle,
        text_layouter: &'a mut ParagraphLayouter,
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
            .map(|run| math_text_span(run, self.base_style, self.settings))
            .collect::<Vec<_>>();
        let layout = self
            .text_layouter
            .shape_text_island_at_font_size_pt(self.catalog, self.settings, &spans, font_size_pt)
            .map_err(|error| match error {
                caret_layout::TextIslandError::InvalidFontSize => {
                    caret_math::layout::MathTextShapeError::InvalidMetrics
                }
                caret_layout::TextIslandError::InconsistentShaping => {
                    caret_math::layout::MathTextShapeError::Failed
                }
            })?;
        let metrics = layout.metrics;
        let requested_family_missing = layout.requested_family_missing;
        let missing_requested_families = layout.missing_requested_families.clone();
        let has_missing_glyph = layout.has_missing_glyph;
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
                .shape_text_island_at_font_size_pt(
                    self.catalog,
                    self.settings,
                    &metrics_span,
                    font_size_pt,
                )
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
            missing_requested_families,
            has_missing_glyph,
            runs,
            containers,
        })
    }
}

fn math_text_span<'a>(
    run: &'a caret_math::layout::MathTextRun,
    base_style: &ShapingStyle,
    settings: &ResolvedDocumentTextSettings,
) -> caret_layout::TextIslandSpan<'a> {
    let mut style = run
        .run
        .as_ref()
        .map_or_else(|| base_style.clone(), |run| base_style.with_run(run));
    // 数学 Font 环境已在 request.style 中解析；Run.props 不保存局部字体。
    style.requested_family = run
        .style
        .font_family
        .clone()
        .or_else(|| base_style.requested_family.clone());
    style.font_role = caret_layout::FontRole::Body;
    style.bold = run.style.bold.unwrap_or(style.bold);
    style.italic = run.style.italic.unwrap_or(style.italic);
    style.language = style
        .language
        .or_else(|| settings.default_language().map(str::to_owned));
    caret_layout::TextIslandSpan {
        text: &run.text,
        style,
        underline: run
            .run
            .as_ref()
            .is_some_and(caret_layout::run_has_underline),
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
    if shaped.layout.lines().next().is_none() {
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
    }
    let mut spans = Vec::with_capacity(request.runs.len());
    let mut byte_offset = 0_usize;
    for run in &request.runs {
        let end = byte_offset.saturating_add(run.text.len());
        spans.push(byte_offset..end);
        byte_offset = end;
    }
    let first_baseline = shaped
        .layout
        .lines()
        .next()
        .map_or(0.0, |line| line.metrics().baseline);
    let island_top = first_baseline - metrics.ascent_pt;
    let mut clusters = Vec::<(std::ops::Range<usize>, f32, f32, f32, f32, f32, bool)>::new();
    for line in shaped.layout.lines() {
        let mut x = line.metrics().inline_min_coord + line.metrics().offset;
        let y0 = line.metrics().content_block_min_coord - island_top;
        let y1 = line.metrics().content_block_max_coord - island_top;
        let baseline = line.metrics().baseline - island_top;
        for run in line.runs() {
            for cluster in run.visual_clusters() {
                let advance = cluster.advance();
                let range = cluster.text_range();
                if !x.is_finite() || !advance.is_finite() || advance < 0.0 {
                    continue;
                }
                clusters.push((range, x, x + advance, y0, y1, baseline, cluster.is_rtl()));
                x += advance;
            }
        }
    }
    clusters.sort_unstable_by_key(|(range, ..)| (range.start, range.end));
    let ends_with_hard_break = shaped.text.chars().next_back().is_some_and(|character| {
        matches!(
            character,
            '\n' | '\r' | '\u{000b}' | '\u{000c}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
        )
    });
    let bounds_for_range = |span: &std::ops::Range<usize>| {
        let first = clusters.partition_point(|(range, ..)| range.end <= span.start);
        if span.is_empty() {
            if let Some(bounds) =
                shaped.cursor_geometry_at(span.start, VisualAffinity::Downstream, 0.0)
            {
                let midpoint = ((bounds.y0 + bounds.y1) * 0.5) as f32;
                let baseline = shaped
                    .layout
                    .lines()
                    .find_map(|line| {
                        let line_metrics = line.metrics();
                        (line_metrics.block_min_coord <= midpoint
                            && midpoint <= line_metrics.block_max_coord)
                            .then_some(line_metrics.baseline - island_top)
                    })
                    .unwrap_or(metrics.ascent_pt);
                return Some((
                    bounds.x0 as f32,
                    bounds.y0 as f32 - island_top,
                    bounds.x1 as f32,
                    bounds.y1 as f32 - island_top,
                    baseline,
                ));
            }
            let cluster = clusters
                .get(first.wrapping_sub(1))
                .filter(|(range, ..)| range.end == span.start)
                .map(|(_, x0, x1, y0, y1, baseline, is_rtl)| {
                    let x = if *is_rtl { *x0 } else { *x1 };
                    (x, *y0, x, *y1, *baseline)
                })
                .or_else(|| {
                    clusters
                        .get(first)
                        .filter(|(range, ..)| range.start == span.start)
                        .map(|(_, x0, x1, y0, y1, baseline, is_rtl)| {
                            let x = if *is_rtl { *x1 } else { *x0 };
                            (x, *y0, x, *y1, *baseline)
                        })
                });
            return cluster;
        }
        let mut x0 = f32::INFINITY;
        let mut x1 = f32::NEG_INFINITY;
        let mut y0 = f32::INFINITY;
        let mut y1 = f32::NEG_INFINITY;
        let mut first_baseline = None;
        for (range, cluster_x0, cluster_x1, cluster_y0, cluster_y1, baseline, _) in
            clusters.iter().skip(first)
        {
            if range.start >= span.end {
                break;
            }
            if span.start < range.end {
                x0 = x0.min(*cluster_x0);
                x1 = x1.max(*cluster_x1);
                y0 = y0.min(*cluster_y0);
                y1 = y1.max(*cluster_y1);
                first_baseline.get_or_insert(*baseline);
            }
        }
        // 最末换行后的空行没有 cluster，但末尾 caret 仍属于结束于此的 Run 和环境。
        if ends_with_hard_break
            && span.end == shaped.text.len()
            && let Some(bounds) =
                shaped.cursor_geometry_at(span.end, VisualAffinity::Downstream, 0.0)
        {
            let caret_x = bounds.x0 as f32;
            let caret_y0 = bounds.y0 as f32 - island_top;
            let caret_y1 = bounds.y1 as f32 - island_top;
            if caret_x.is_finite() && caret_y0.is_finite() && caret_y1.is_finite() {
                x0 = x0.min(caret_x);
                x1 = x1.max(caret_x);
                y0 = y0.min(caret_y0);
                y1 = y1.max(caret_y1);
                if first_baseline.is_none() {
                    let midpoint = ((bounds.y0 + bounds.y1) * 0.5) as f32;
                    first_baseline = shaped.layout.lines().find_map(|line| {
                        let metrics = line.metrics();
                        (metrics.block_min_coord <= midpoint && midpoint <= metrics.block_max_coord)
                            .then_some(metrics.baseline - island_top)
                    });
                }
            }
        }
        if !x0.is_finite() {
            Some((0.0, 0.0, 0.0, 0.0, 0.0))
        } else {
            Some((x0, y0, x1, y1, first_baseline.unwrap_or(y0)))
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
        let (x0, y0, x1, y1, baseline) =
            bounds_for_range(span).unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0));
        caret_math::layout::MathTextRunBox {
            id,
            scope_id,
            path,
            run_index,
            is_text_flow,
            is_flow,
            is_container,
            x: f64::from(x0),
            y: f64::from(y0),
            width: f64::from((x1 - x0).max(0.0)),
            height: f64::from((y1 - y0).max(0.0)),
            baseline: f64::from(baseline - y0),
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

fn collect_text_layout_diagnostics(
    paragraphs: &[LaidOutParagraph],
    math_blocks: &HashMap<NodeId, Arc<caret_math::layout::MathLayout>>,
) -> Vec<TextLayoutDiagnostic> {
    let mut diagnostics = Vec::new();
    for paragraph in paragraphs {
        for family in &paragraph.layout.missing_requested_families {
            diagnostics.push(TextLayoutDiagnostic {
                node_id: paragraph.id.clone(),
                node_kind: "paragraph",
                code: "text.font.family_missing",
                detail: family.clone(),
            });
        }
        if paragraph.layout.has_missing_glyph {
            diagnostics.push(TextLayoutDiagnostic {
                node_id: paragraph.id.clone(),
                node_kind: "paragraph",
                code: "text.glyph.missing",
                detail: "paragraph contains an unavailable glyph".to_owned(),
            });
        }
        for (math_id, layout) in paragraph
            .inline_math_ids
            .iter()
            .zip(paragraph.inline_math_layouts.iter())
        {
            diagnostics.extend(
                layout
                    .diagnostics
                    .iter()
                    .map(|diagnostic| TextLayoutDiagnostic {
                        node_id: math_id.clone(),
                        node_kind: "math",
                        code: diagnostic.code,
                        detail: diagnostic.detail.clone(),
                    }),
            );
        }
    }
    diagnostics.extend(math_blocks.iter().flat_map(|(math_id, layout)| {
        layout
            .diagnostics
            .iter()
            .map(|diagnostic| TextLayoutDiagnostic {
                node_id: math_id.clone(),
                node_kind: "math",
                code: diagnostic.code,
                detail: diagnostic.detail.clone(),
            })
    }));
    diagnostics.sort_by(|left, right| {
        left.node_id
            .to_string()
            .cmp(&right.node_id.to_string())
            .then_with(|| left.node_kind.cmp(right.node_kind))
            .then_with(|| left.code.cmp(right.code))
            .then_with(|| left.detail.cmp(&right.detail))
    });
    diagnostics
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
    previous: Option<&'c LaidOutParagraph>,
    previous_inline_math: HashMap<NodeId, (usize, &'c caret_doc::InlineMath)>,
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
                    let reused = self.previous.and_then(|previous| {
                        let (index, source) = self.previous_inline_math.get(&math.id)?;
                        if *source != math {
                            return None;
                        }
                        Some((
                            Arc::clone(previous.inline_math_layouts.get(*index)?),
                            previous.inline_math_text_layouts.get(*index)?.clone(),
                        ))
                    });
                    let (layout, sidecars) = if let Some(reused) = reused {
                        reused
                    } else if math_flow_contains_text(&math.math_tree.root) {
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
                            .layout_with_text_shaper_and_templates(
                                &math.math_tree.root,
                                self.font_size_pt,
                                caret_math::layout::MathStyle::Inline,
                                self.context.settings.inline_environment_templates(),
                                &mut shaper,
                            )
                            .map_err(|error| DocumentLayoutError::Math(error.to_string()))?;
                        (layout, shaper.into_sidecars())
                    } else if !self
                        .context
                        .settings
                        .inline_environment_templates()
                        .is_empty()
                    {
                        (
                            Arc::new(
                                caret_math::layout::layout_math_with_templates(
                                    engine.font(),
                                    &math.math_tree.root,
                                    self.font_size_pt,
                                    caret_math::layout::MathStyle::Inline,
                                    self.context.settings.inline_environment_templates(),
                                )
                                .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                            ),
                            HashMap::new(),
                        )
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
                    // 空宿主只保留节点盒供 caret/命中使用，不占正文推进；
                    // 零推进文本岛仍用透明最小推进保留点击槽位。
                    let advance_pt = as_f32(layout.inline_advance)
                        .ok_or_else(|| {
                            DocumentLayoutError::Math("invalid inline math advance".to_owned())
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

fn index_previous_inline_math(
    previous: &LaidOutParagraph,
) -> HashMap<NodeId, (usize, &caret_doc::InlineMath)> {
    fn collect<'a>(
        nodes: &'a [ParagraphContent],
        sources: &mut HashMap<NodeId, &'a caret_doc::InlineMath>,
    ) {
        for node in nodes {
            match node {
                FlowContent::Math(math) => {
                    sources.insert(math.id.clone(), math);
                }
                FlowContent::Environment(environment) => collect(&environment.children, sources),
                FlowContent::Text(_) | FlowContent::SoftBreak(_) | FlowContent::Opaque(_) => {}
            }
        }
    }

    let mut sources = HashMap::new();
    collect(&previous.source_paragraph.inlines, &mut sources);
    previous
        .inline_math_ids
        .iter()
        .enumerate()
        .filter_map(|(index, id)| {
            sources
                .remove(id)
                .map(|source| (id.clone(), (index, source)))
        })
        .collect()
}

fn collect_inline_math_layouts(
    paragraph: &Paragraph,
    context: &mut MathLayoutContext<'_>,
    previous: Option<&LaidOutParagraph>,
) -> Result<InlineMathLayouts, DocumentLayoutError> {
    let font_size_pt =
        DocumentUnits::centipoints_to_points(u32::from(context.font_size_centipoints));
    let math_base_style = resolved_math_base_style(
        context.base_style,
        context.settings,
        paragraph.language.as_deref(),
    );
    let previous =
        previous.filter(|previous| previous.source_paragraph.language == paragraph.language);
    let previous_inline_math = previous.map(index_previous_inline_math).unwrap_or_default();
    let mut builder = InlineMathLayoutBuilder {
        context,
        base_style: &math_base_style,
        paragraph_id: paragraph.id.clone(),
        previous,
        previous_inline_math,
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

fn resolved_math_base_style(
    base_style: &ShapingStyle,
    settings: &ResolvedDocumentTextSettings,
    paragraph_language: Option<&str>,
) -> ShapingStyle {
    let mut style = base_style.clone();
    style.language = paragraph_language
        .map(str::to_owned)
        .or_else(|| settings.default_language().map(str::to_owned))
        .or_else(|| base_style.language.clone());
    style
}

struct MathLayoutContext<'a> {
    engine: Option<&'a caret_math::layout::MathLayoutEngine>,
    catalog: &'a mut FontCatalog,
    settings: &'a ResolvedDocumentTextSettings,
    base_style: &'a ShapingStyle,
    text_layouter: &'a mut ParagraphLayouter,
    font_size_centipoints: u16,
    content_width_pt: f32,
    line_grid_metrics: FontLineMetrics,
    line_spacing: LineSpacing,
    space_before_pt: f32,
    space_after_pt: f32,
    caret_metrics: LineMetrics,
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
        source: Box<MathBlockSource>,
        layout: Arc<caret_math::layout::MathLayout>,
        text_layouts: HashMap<u64, MathTextSidecar>,
        environment_ids: Vec<NodeId>,
        x_offset_pt: f32,
        width_pt: f32,
        height_pt: f32,
        baseline_pt: f32,
        caret_baseline_pt: f32,
        caret_height_pt: f32,
        clip_ink_to_line_height: bool,
        space_before_pt: f32,
        space_after_pt: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum MathBlockSource {
    Equation(caret_doc::Equation),
    Align(caret_doc::Align),
}

fn reuse_math_block(
    previous: Option<&DocumentLayoutSnapshot>,
    math_id: &NodeId,
    source: &MathBlockSource,
) -> Option<(
    Arc<caret_math::layout::MathLayout>,
    HashMap<u64, MathTextSidecar>,
)> {
    let previous = previous?;
    if previous.math_block_sources.get(math_id) != Some(source) {
        return None;
    }
    Some((
        Arc::clone(previous.math_blocks.get(math_id)?),
        previous.math_block_text_layouts.get(math_id)?.clone(),
    ))
}

fn collect_blocks<'a>(
    blocks: &'a [Block],
    ordered: &mut Vec<LayoutInput<'a>>,
    environment_ids: &mut Vec<NodeId>,
    context: &mut MathLayoutContext<'_>,
    previous: Option<&DocumentLayoutSnapshot>,
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
                collect_blocks(
                    &environment.children,
                    ordered,
                    environment_ids,
                    context,
                    previous,
                )?;
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
                let source = MathBlockSource::Equation(equation.clone());
                let reused = reuse_math_block(previous, &equation.id, &source);
                let (layout, text_layouts) = if let Some(reused) = reused {
                    reused
                } else if math_flow_contains_text(&equation.math_tree.root) {
                    let math_base_style =
                        resolved_math_base_style(context.base_style, context.settings, None);
                    let mut shaper = EngineMathTextShaper::new(
                        context.catalog,
                        context.settings,
                        &math_base_style,
                        context.text_layouter,
                        MathTextLayoutHost::equation(
                            equation.id.clone(),
                            equation.math_tree.root.id.clone(),
                        ),
                    );
                    let layout = engine
                        .layout_with_text_shaper_and_templates(
                            &equation.math_tree.root,
                            DocumentUnits::centipoints_to_points(u32::from(
                                context.font_size_centipoints,
                            )),
                            caret_math::layout::MathStyle::Display,
                            context.settings.inline_environment_templates(),
                            &mut shaper,
                        )
                        .map_err(|error| DocumentLayoutError::Math(error.to_string()))?;
                    (layout, shaper.into_sidecars())
                } else if !context.settings.inline_environment_templates().is_empty() {
                    (
                        Arc::new(
                            caret_math::layout::layout_math_with_templates(
                                engine.font(),
                                &equation.math_tree.root,
                                DocumentUnits::centipoints_to_points(u32::from(
                                    context.font_size_centipoints,
                                )),
                                caret_math::layout::MathStyle::Display,
                                context.settings.inline_environment_templates(),
                            )
                            .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                        ),
                        HashMap::new(),
                    )
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
                    source,
                    layout,
                    text_layouts,
                    environment_ids.clone(),
                    context,
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
                let source = MathBlockSource::Align(align.clone());
                let (layout, text_layouts) = if contains_text {
                    let math_base_style =
                        resolved_math_base_style(context.base_style, context.settings, None);
                    let mut shaper = EngineMathTextShaper::new(
                        context.catalog,
                        context.settings,
                        &math_base_style,
                        context.text_layouter,
                        MathTextLayoutHost::align(align.id.clone(), align),
                    );
                    (
                        Arc::new(
                            caret_math::layout::layout_align_with_host_row_grid(
                                engine.font(),
                                align,
                                DocumentUnits::centipoints_to_points(u32::from(
                                    context.font_size_centipoints,
                                )),
                                context.settings.inline_environment_templates(),
                                Some(&mut shaper),
                                math_host_row_grid(
                                    context.line_grid_metrics,
                                    context.caret_metrics,
                                    context.line_spacing,
                                )?,
                            )
                            .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                        ),
                        shaper.into_sidecars(),
                    )
                } else if !context.settings.inline_environment_templates().is_empty() {
                    (
                        Arc::new(
                            caret_math::layout::layout_align_with_host_row_grid(
                                engine.font(),
                                align,
                                DocumentUnits::centipoints_to_points(u32::from(
                                    context.font_size_centipoints,
                                )),
                                context.settings.inline_environment_templates(),
                                None,
                                math_host_row_grid(
                                    context.line_grid_metrics,
                                    context.caret_metrics,
                                    context.line_spacing,
                                )?,
                            )
                            .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                        ),
                        HashMap::new(),
                    )
                } else {
                    (
                        Arc::new(
                            caret_math::layout::layout_align_with_host_row_grid(
                                engine.font(),
                                align,
                                DocumentUnits::centipoints_to_points(u32::from(
                                    context.font_size_centipoints,
                                )),
                                context.settings.inline_environment_templates(),
                                None,
                                math_host_row_grid(
                                    context.line_grid_metrics,
                                    context.caret_metrics,
                                    context.line_spacing,
                                )?,
                            )
                            .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                        ),
                        HashMap::new(),
                    )
                };
                push_math_block(
                    ordered,
                    source,
                    layout,
                    text_layouts,
                    environment_ids.clone(),
                    context,
                )?;
            }
            Block::Opaque(_) => return Err(DocumentLayoutError::UnsupportedContent),
        }
    }
    Ok(())
}

fn push_math_block<'a>(
    ordered: &mut Vec<LayoutInput<'a>>,
    source: MathBlockSource,
    layout: Arc<caret_math::layout::MathLayout>,
    text_layouts: HashMap<u64, MathTextSidecar>,
    environment_ids: Vec<NodeId>,
    context: &MathLayoutContext<'_>,
) -> Result<(), DocumentLayoutError> {
    let math_id = match &source {
        MathBlockSource::Equation(equation) => equation.id.clone(),
        MathBlockSource::Align(align) => align.id.clone(),
    };
    let width_pt = layout.width as f32;
    let host_metrics = match &source {
        MathBlockSource::Equation(equation)
            if !math_flow_contains_soft_break(&equation.math_tree.root) =>
        {
            calculate_math_host_metrics(&layout, context.line_grid_metrics, context.line_spacing)?
        }
        MathBlockSource::Equation(_) => calculate_math_align_host_metrics(&layout)?,
        MathBlockSource::Align(_) => calculate_math_align_host_metrics(&layout)?,
    };
    let height_pt = host_metrics.height_pt;
    if !context.content_width_pt.is_finite()
        || context.content_width_pt <= 0.0
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
        source: Box::new(source),
        layout,
        text_layouts,
        environment_ids,
        x_offset_pt: (context.content_width_pt - width_pt) / 2.0,
        width_pt,
        height_pt,
        baseline_pt: host_metrics.baseline_pt,
        caret_baseline_pt: context.caret_metrics.baseline_pt,
        caret_height_pt: context.caret_metrics.height_pt,
        clip_ink_to_line_height: host_metrics.clip_ink_to_line_height,
        space_before_pt: context.space_before_pt,
        space_after_pt: context.space_after_pt,
    });
    Ok(())
}

fn calculate_math_host_metrics(
    layout: &caret_math::layout::MathLayout,
    line_grid_metrics: FontLineMetrics,
    line_spacing: LineSpacing,
) -> Result<LineMetrics, DocumentLayoutError> {
    let width_pt = layout.width as f32;
    let ascent_pt = layout.ascent as f32;
    let descent_pt = layout.descent as f32;
    if !width_pt.is_finite() || !ascent_pt.is_finite() || !descent_pt.is_finite() || width_pt < 0.0
    {
        return Err(DocumentLayoutError::Math(
            "invalid display math line metrics".to_owned(),
        ));
    }
    calculate_content_line_metrics_with_grid(line_grid_metrics, ascent_pt, descent_pt, line_spacing)
        .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))
}

fn calculate_math_align_host_metrics(
    layout: &caret_math::layout::MathLayout,
) -> Result<LineMetrics, DocumentLayoutError> {
    let height_pt = (layout.ascent + layout.descent) as f32;
    let baseline_pt = layout.ascent as f32;
    if !height_pt.is_finite() || height_pt <= 0.0 || !baseline_pt.is_finite() {
        return Err(DocumentLayoutError::Math(
            "invalid Align grid metrics".to_owned(),
        ));
    }
    Ok(LineMetrics {
        height_pt,
        baseline_pt,
        content_metrics_overflow_line_box: false,
        object_ink_overflows_line_box: false,
        // Exactly 行距的逐行裁切已附在 MathLayout 的行项范围上。
        clip_ink_to_line_height: false,
    })
}

fn math_host_row_grid(
    line_grid: FontLineMetrics,
    body_metrics: LineMetrics,
    spacing: LineSpacing,
) -> Result<caret_math::layout::MathHostRowGrid, DocumentLayoutError> {
    Ok(caret_math::layout::MathHostRowGrid {
        ascent: f64::from(line_grid.ascent_pt),
        descent: f64::from(line_grid.descent_pt),
        leading: f64::from(line_grid.leading_pt),
        spacing: caret_layout::row_spacing_for_content_line_spacing(
            spacing,
            body_metrics.height_pt,
            body_metrics.baseline_pt,
        )
        .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?,
    })
}

fn math_flow_contains_soft_break(flow: &MathFlow) -> bool {
    flow.content.iter().any(|content| match content {
        MathContent::SoftBreak(_) => true,
        MathContent::Math(node) => node.children.iter().any(math_flow_contains_soft_break),
        MathContent::Environment(environment) => environment
            .children
            .iter()
            .any(math_content_contains_soft_break),
        MathContent::Text(_) | MathContent::Opaque(_) => false,
    })
}

fn math_content_contains_soft_break(content: &MathContent) -> bool {
    match content {
        MathContent::SoftBreak(_) => true,
        MathContent::Math(node) => node.children.iter().any(math_flow_contains_soft_break),
        MathContent::Environment(environment) => environment
            .children
            .iter()
            .any(math_content_contains_soft_break),
        MathContent::Text(_) | MathContent::Opaque(_) => false,
    }
}

#[cfg(test)]
mod tests;
