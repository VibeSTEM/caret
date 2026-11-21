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
use caret_structure::BlockContainerPath;
use std::{collections::HashMap, ops::Range, sync::Arc};

mod block_navigation;
mod geometry;
mod hit;
mod items;
mod math_navigation;
mod math_text;
mod produce;
mod query;
mod table_layout;
pub use caret_layout::FragmentHost;
pub(crate) use produce::math_flow_is_empty_host;
mod shared;
use shared::*;

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
    pub container: BlockContainerPath,
    source_paragraph: Arc<Paragraph>,
    resolved_style: ResolvedParagraphStyle,
    layout_width_pt: f32,
    math_caret_height_pt: f32,
    math_caret_baseline_pt: f32,
    pub text: Arc<str>,
    pub layout: Arc<ParleyParagraphLayout>,
    /// 行内 MathBox 布局与 Parley inline-box ID 按顺序一一对应。
    pub inline_math_layouts: Arc<[Arc<caret_math::layout::MathLayout>]>,
    /// 行内 MathBox 与文档 InlineMath 稳定 ID 按顺序一一对应。
    inline_math_ids: Arc<[NodeId]>,
    /// 行内公式对象在宿主段落逻辑文本中的原子源范围。
    pub inline_math_source_ranges: Arc<[Range<usize>]>,
    /// 行内公式 token 到同一布局代际 Parley 文本岛 sidecar 的映射。
    inline_math_text_layouts: Arc<[HashMap<u64, MathTextSidecar>]>,
    utf16_checkpoints: Arc<[(usize, usize)]>,
    pub(crate) line_locations: Vec<Option<CaretLineLocation>>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CaretLineLocation {
    pub(crate) page_index: usize,
    /// 行左边缘的页面局部 x 坐标（已含页边距与单元格位置），单位为 pt。
    pub(crate) x0_pt: f64,
    /// 行顶边的页面局部 y 坐标，单位为 pt。
    pub(crate) y0_pt: f64,
    /// 页面局部的矩形裁剪框；正文行为 None。
    pub(crate) clip: Option<PageClip>,
}

/// 页面局部矩形裁剪框；caret/选区/命中/绘制共用同一判断。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PageClip {
    pub(crate) x0_pt: f64,
    pub(crate) y0_pt: f64,
    pub(crate) x1_pt: f64,
    pub(crate) y1_pt: f64,
}

impl PageClip {
    pub(crate) fn from_array(values: [f64; 4]) -> Self {
        Self {
            x0_pt: values[0],
            y0_pt: values[1],
            x1_pt: values[2],
            y1_pt: values[3],
        }
    }

    /// 点是否落在裁剪框内（包含边界）。
    pub(crate) fn contains(&self, x_pt: f64, y_pt: f64) -> bool {
        x_pt >= self.x0_pt && x_pt <= self.x1_pt && y_pt >= self.y0_pt && y_pt <= self.y1_pt
    }

    /// 将插入点几何裁剪进可见区域；完全不可见时返回 false。
    pub(crate) fn clamp_caret(&self, caret: &mut PageCaretGeometry) -> bool {
        caret.x_pt = caret.x_pt.clamp(self.x0_pt, self.x1_pt);
        caret.y0_pt = caret.y0_pt.clamp(self.y0_pt, self.y1_pt);
        caret.y1_pt = caret.y1_pt.clamp(self.y0_pt, self.y1_pt);
        caret.y1_pt > caret.y0_pt
    }

    /// 将选区矩形裁剪进可见区域；裁剪后为空时返回 false。
    pub(crate) fn clamp_rect(&self, rect: &mut PageSelectionRect) -> bool {
        rect.x0_pt = rect.x0_pt.clamp(self.x0_pt, self.x1_pt);
        rect.x1_pt = rect.x1_pt.clamp(self.x0_pt, self.x1_pt);
        rect.y0_pt = rect.y0_pt.clamp(self.y0_pt, self.y1_pt);
        rect.y1_pt = rect.y1_pt.clamp(self.y0_pt, self.y1_pt);
        rect.x1_pt > rect.x0_pt && rect.y1_pt > rect.y0_pt
    }
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
    /// 文本岛所在的视觉结构槽深度；由公式排版在塑形请求中提供。
    structural_depth: u32,
}

#[derive(Debug, Clone)]
struct MathTextSourceSpan {
    range: std::ops::Range<usize>,
    flow_offset: usize,
    path: MathTextFlowPath,
}

struct LaidOutParagraphInput {
    source_paragraph: Paragraph,
    story: StoryId,
    container: BlockContainerPath,
    resolved_style: ResolvedParagraphStyle,
    layout_width_pt: f32,
    math_caret_height_pt: f32,
    math_caret_baseline_pt: f32,
    text: String,
    layout: ParleyParagraphLayout,
    inline_math_layouts: Vec<Arc<caret_math::layout::MathLayout>>,
    inline_math_ids: Vec<NodeId>,
    inline_math_source_ranges: Vec<Range<usize>>,
    inline_math_text_layouts: Vec<HashMap<u64, MathTextSidecar>>,
}

/// 单元格内显示公式在页面上的放置；与正文公式投影共用同一坐标约定。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableMathPlacement {
    pub(crate) math_id: NodeId,
    /// 公式盒左边缘的页面局部 x 坐标。
    pub(crate) object_x_pt: f64,
    /// 公式基线的页面局部 y 坐标。
    pub(crate) baseline_y_pt: f64,
    pub(crate) caret_line_top_pt: f64,
    pub(crate) caret_baseline_y_pt: f64,
    pub(crate) caret_height_pt: f64,
    pub(crate) clip_y_pt: Option<[f64; 2]>,
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
    /// 本页单元格内显示公式的放置；不回写 PageLayout 的分页片段。
    pub(crate) table_math_placements: Vec<TableMathPlacement>,
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

/// 一次分页产生的页面、环境范围与段落索引。
#[derive(Debug)]
pub(crate) struct PageGeometries {
    pub(crate) pages: Vec<PageLayoutSnapshot>,
    pub(crate) paragraph_lookup: HashMap<NodeId, usize>,
    pub(crate) environment_ranges: Vec<PageEnvironmentRange>,
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
    /// 已测量的表格网格几何；页面片段只保存放置引用，几何由快照单点持有。
    pub(crate) table_layouts: HashMap<NodeId, Arc<caret_layout::TableGridLayout>>,
    /// 块级公式 token 与正文 Parley sidecar 成对保存，避免布局缓存脱离字体数据。
    math_block_text_layouts: HashMap<NodeId, HashMap<u64, MathTextSidecar>>,
    math_block_sources: HashMap<NodeId, MathBlockSource>,
    math_hosts: HashMap<NodeId, MathTextLayoutHost>,
    math_projection_index: MathProjectionIndex,
    paragraph_lookup: HashMap<NodeId, usize>,
}

/// 按页面和数学节点索引已生成的公式页面投影。
#[derive(Debug, Clone, Default)]
pub(crate) struct MathProjectionIndex {
    projections: Vec<MathPageProjection>,
    by_page_and_id: HashMap<(usize, NodeId), usize>,
    by_id: HashMap<NodeId, Vec<usize>>,
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

/// 公式树序列在页面上的插入点，供公式内部和跨宿主拖选共用。
#[derive(Debug, Clone, PartialEq)]
pub struct PageMathTreeDragStop {
    pub target: MathEditTarget,
    pub position: MathTreePosition,
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

pub use produce::{DocumentLayoutRuntime, DocumentLayoutService};

#[cfg(test)]
mod tests;
