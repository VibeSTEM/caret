//! 公式布局：把文档数学树排成以 pt 为单位、基线为 y=0（向下为正）的绘制项列表。
//!
//! 排版算法移植自 Typst（见各子模块文件头），字体度量全部来自 OpenType MATH 表。
//! [`MathLayoutEngine`] 驻留字体并按（内容哈希、字号、显示/行内、字体标识）缓存结果。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod accent;
mod alphabet;
mod context;
mod convert;
mod fenced;
mod fraction;
mod frame;
mod glyph;
mod radical;
mod run;
mod scripts;
mod table;
mod text;

use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use caret_doc::InlineEnvironmentTemplates;
use caret_doc::{Align, DocumentUnits, MathContent, MathFlow, MathPathStep, NodeId};

pub use crate::fonts::{MathFont, MathFontError};
pub use context::{MathDiagnostic, MathSize};
pub use frame::MathColor;
pub use text::{
    MathTextContainer, MathTextRequest, MathTextRun, MathTextRunBox, MathTextRunStyle,
    MathTextShape, MathTextShapeError, MathTextShaper,
};

/// 可见数学空槽的宽度（em），由命中与 caret 几何共用。
pub const MATH_PLACEHOLDER_WIDTH_EM: f64 = 0.6;
/// 可见数学空槽的高度（em），由命中与 caret 几何共用。
pub const MATH_PLACEHOLDER_HEIGHT_EM: f64 = 0.7;

use crate::backend::{BackendNodeKind, Node, NodePosition, flow_nodes_for_layout};
use context::{Context, Style};
use convert::Converter;
use frame::Item;

/// 公式的排版样式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MathStyle {
    /// 行内公式（TeX text style）。
    Inline,
    /// 显示公式（TeX display style）。
    Display,
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// 显示公式宿主提供给顶层数学表格的正文行网格。
pub struct MathHostRowGrid {
    /// 正文基础行框的上行高。
    pub ascent: f64,
    /// 正文基础行框的下行高。
    pub descent: f64,
    /// 正文基础行框额外 leading。
    pub leading: f64,
    /// 宿主采用的正文行距语义。
    pub spacing: caret_grid_layout::RowSpacing,
}

/// 数学排版输入错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathLayoutError {
    InvalidSize,
    InvalidTemplate,
    TextShapingFailed,
}

impl Display for MathLayoutError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSize => formatter.write_str("math.layout.invalid_size"),
            Self::InvalidTemplate => formatter.write_str("math.layout.invalid_template"),
            Self::TextShapingFailed => formatter.write_str("math.layout.text_shaping_failed"),
        }
    }
}

impl std::error::Error for MathLayoutError {}

/// 布局输出的绘制项；坐标以公式左端基线为原点，y 向下。
#[derive(Debug, Clone, PartialEq)]
pub enum MathItem {
    /// 字形；`(x, y)` 为字形原点（基线左端），`size` 为字号（pt）。
    Glyph {
        glyph: u16,
        x: f64,
        y: f64,
        size: f64,
        color: Option<MathColor>,
    },
    /// 实心矩形（分数线、根号顶线、上下横线、表格线），`(x, y)` 为左上角。
    Rule {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        color: Option<MathColor>,
    },
    /// 线段（删除线），从 `(x, y)` 到 `(x + dx, y + dy)`。
    Line {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
        thickness: f64,
        color: Option<MathColor>,
    },
    /// 矩形背景或边框（`\colorbox`、`\boxed`、占位框），`(x, y)` 为左上角。
    Rectangle {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        fill: Option<MathColor>,
        stroke: Option<MathColor>,
        stroke_width: f64,
    },
    /// 正文 Parley 文本岛；实际 glyph/run 使用 engine 持有的 token sidecar 绘制。
    TextIsland {
        token: u64,
        x: f64,
        baseline_y: f64,
        width: f64,
        color: Option<MathColor>,
    },
}

/// 某个数学节点在公式中的范围，供命中测试、光标与选区使用。
#[derive(Debug, Clone, PartialEq)]
pub struct MathNodeBox {
    /// 持久结构节点 ID；TextFlow/Run 没有自身 ID 时为 `None`。
    pub id: Option<NodeId>,
    /// 路径作用域：MathTree 使用根 MathFlow ID；Align 本体、行和单元格边界分别用自身 ID。
    pub scope_id: NodeId,
    /// 相对 MathFlow scope 的结构路径；Align 本体/行盒路径为空，cell 文本以 cell root 为 scope。
    pub path: Vec<MathPathStep>,
    /// TextFlow 内的 Run 下标；普通数学节点为 `None`。
    pub text_run_index: Option<usize>,
    /// 该盒是否代表 TextFlow（空 TextFlow 没有 Run 下标）。
    pub is_text_flow: bool,
    /// MathFlow 聚合盒。
    pub is_flow: bool,
    /// 透明 Row 这类容器盒会按子节点扩展范围。
    pub is_container: bool,
    pub x: f64,
    /// 顶边（相对基线，向下为正，通常为负）。
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// 节点自身基线相对公式基线的偏移。
    pub baseline: f64,
    /// 该节点所在数学样式的有效字号（pt），供树原生光标继续继承。
    pub font_size_pt: Option<f64>,
}

/// 排版结果。颜色为 `None` 的项使用正文颜色。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MathLayout {
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
    pub items: Vec<MathItem>,
    /// 按前序遍历排列；外层节点在内层之前。
    pub nodes: Vec<MathNodeBox>,
    /// Exactly 行距中各行对应的绘制项范围及裁切纵坐标（相对公式基线）。
    pub row_clips: Vec<MathRowClip>,
    /// 顶层宿主表格行的稳定身份和基线（相对公式基线），与是否使用固定行裁切无关。
    pub row_baselines: Vec<MathRowBaseline>,
    pub diagnostics: Vec<MathDiagnostic>,
    /// 文本岛请求的非通用字体族不可用，已使用回退字体。
    pub requested_text_family_missing: bool,
}

#[derive(Debug, Clone, PartialEq)]
/// 固定行距下数学表格的一行墨迹裁切区。
pub struct MathRowClip {
    /// 由数学表格提供的稳定行身份；无身份的临时表格行仅用于绘制裁切。
    pub row_id: Option<NodeId>,
    /// `MathLayout.items` 中属于该行的半开区间。
    pub item_range: std::ops::Range<usize>,
    /// 相对公式基线的裁切上边缘。
    pub y0: f64,
    /// 相对公式基线的裁切下边缘。
    pub y1: f64,
}

/// 顶层宿主表格行的垂直定位。
#[derive(Debug, Clone, PartialEq)]
pub struct MathRowBaseline {
    /// 由宿主表格提供的稳定行身份。
    pub row_id: NodeId,
    /// 相对公式基线的行基线位置。
    pub baseline: f64,
}

/// 以给定数学字体与字号（pt）排版一棵数学树。
///
/// 此便捷 API 不接入正文字体选择和 shaping：其中的 `TextFlow` / `\\text{...}` 内容会走
/// 数学字体的降级路径，不能获得正文字体族、脚本/语言偏好或 glyph fallback。需要正文文本
/// 外观与回退语义时，必须调用 [`layout_math_with_text_shaper`]。
pub fn layout_math(
    font: &MathFont,
    root: &MathFlow,
    size: f64,
    style: MathStyle,
) -> Result<MathLayout, MathLayoutError> {
    validate_size(size)?;
    layout_nodes(
        font,
        flow_nodes_for_layout(root),
        size,
        style,
        None,
        None,
        None,
    )
}

/// 无正文 shaper 的模板布局入口；TextFlow 仍走数学字体降级路径。
pub fn layout_math_with_templates(
    font: &MathFont,
    root: &MathFlow,
    size: f64,
    style: MathStyle,
    templates: &InlineEnvironmentTemplates,
) -> Result<MathLayout, MathLayoutError> {
    validate_size(size)?;
    validate_flow_templates(root, templates)?;
    layout_nodes(
        font,
        crate::backend::flow_nodes_for_layout_with_templates(root, Some(templates)),
        size,
        style,
        None,
        None,
        None,
    )
}

/// 排版公式并通过调用方提供的正文 shaping 回调处理 TextFlow 文本岛。
///
/// 含局部 Parley token 的结果不进入 `MathLayoutEngine` 内容缓存；调用方必须与结果同代持有回调 sidecar。
pub fn layout_math_with_text_shaper(
    font: &MathFont,
    root: &MathFlow,
    size: f64,
    style: MathStyle,
    text_shaper: &mut dyn MathTextShaper,
) -> Result<MathLayout, MathLayoutError> {
    validate_size(size)?;
    layout_nodes(
        font,
        flow_nodes_for_layout(root),
        size,
        style,
        Some(text_shaper),
        None,
        None,
    )
}

/// 通过正文 shaper 和已解析主题模板排版数学文本岛。
pub fn layout_math_with_text_shaper_and_templates(
    font: &MathFont,
    root: &MathFlow,
    size: f64,
    style: MathStyle,
    templates: &InlineEnvironmentTemplates,
    text_shaper: &mut dyn MathTextShaper,
) -> Result<MathLayout, MathLayoutError> {
    validate_size(size)?;
    validate_flow_templates(root, templates)?;
    layout_nodes(
        font,
        crate::backend::flow_nodes_for_layout_with_templates(root, Some(templates)),
        size,
        style,
        Some(text_shaper),
        None,
        None,
    )
}

/// 以对齐公式的稳定行/单元格身份进行显示公式排版。
///
/// 此便捷 API 不接入正文字体选择和 shaping：单元格中的 `TextFlow` / `\\text{...}` 内容会
/// 走数学字体的降级路径，不能获得正文字体族、脚本/语言偏好或 glyph fallback。需要正文
/// 文本外观与回退语义时，必须调用 [`layout_align_with_text_shaper`]。
///
/// `Align` 本身与数学树不是同一种文档结构。此适配只在布局调用期间构造后端矩阵，
/// 保留 Align、行和单元格根流的 ID，不改写或规范化持久化模型。
pub fn layout_align(
    font: &MathFont,
    align: &Align,
    size: f64,
) -> Result<MathLayout, MathLayoutError> {
    layout_align_internal(font, align, size, None, None, None)
}

/// 无正文 shaper 的 Align 模板布局入口；TextFlow 仍走数学字体降级路径。
pub fn layout_align_with_templates(
    font: &MathFont,
    align: &Align,
    size: f64,
    templates: &InlineEnvironmentTemplates,
) -> Result<MathLayout, MathLayoutError> {
    layout_align_internal(font, align, size, Some(templates), None, None)
}

/// 使用调用方正文 shaping 处理 Align 单元格中的 TextFlow。
pub fn layout_align_with_text_shaper(
    font: &MathFont,
    align: &Align,
    size: f64,
    text_shaper: &mut dyn MathTextShaper,
) -> Result<MathLayout, MathLayoutError> {
    layout_align_internal(font, align, size, None, Some(text_shaper), None)
}

/// 使用正文 shaper 和已解析主题模板处理 Align 单元格中的 TextFlow。
pub fn layout_align_with_text_shaper_and_templates(
    font: &MathFont,
    align: &Align,
    size: f64,
    templates: &InlineEnvironmentTemplates,
    text_shaper: &mut dyn MathTextShaper,
) -> Result<MathLayout, MathLayoutError> {
    layout_align_internal(font, align, size, Some(templates), Some(text_shaper), None)
}

/// 使用正文宿主行网格排版 Align；嵌套数学矩阵仍使用其原有数学行策略。
pub fn layout_align_with_host_row_grid<'a>(
    font: &'a MathFont,
    align: &Align,
    size: f64,
    templates: &InlineEnvironmentTemplates,
    text_shaper: Option<&'a mut dyn MathTextShaper>,
    row_grid: MathHostRowGrid,
) -> Result<MathLayout, MathLayoutError> {
    layout_align_internal(
        font,
        align,
        size,
        Some(templates),
        text_shaper,
        Some(row_grid),
    )
}

fn layout_align_internal<'a>(
    font: &'a MathFont,
    align: &Align,
    size: f64,
    templates: Option<&InlineEnvironmentTemplates>,
    text_shaper: Option<&'a mut dyn MathTextShaper>,
    host_row_grid: Option<MathHostRowGrid>,
) -> Result<MathLayout, MathLayoutError> {
    validate_size(size)?;
    if let Some(templates) = templates {
        for cell in align.rows.iter().flat_map(|row| &row.cells) {
            validate_flow_templates(&cell.root, templates)?;
        }
    }
    let rows = align
        .rows
        .iter()
        .map(|row| Node {
            id: Some(row.id.clone()),
            position: Some(NodePosition {
                scope_id: row.id.clone(),
                path: Vec::new(),
                text_run_index: None,
                is_text_flow: false,
                is_flow: false,
                is_container: false,
                font_size_pt: None,
            }),
            kind: BackendNodeKind::Row,
            text: None,
            run: None,
            attributes: Default::default(),
            text_style: None,
            children: row
                .cells
                .iter()
                .flat_map(|cell| match templates {
                    Some(templates) => crate::backend::flow_nodes_for_layout_with_templates(
                        &cell.root,
                        Some(templates),
                    ),
                    None => flow_nodes_for_layout(&cell.root),
                })
                .collect(),
        })
        .collect();
    let matrix = Node {
        id: Some(align.id.clone()),
        position: Some(NodePosition {
            scope_id: align.id.clone(),
            path: Vec::new(),
            text_run_index: None,
            is_text_flow: false,
            is_flow: false,
            is_container: false,
            font_size_pt: None,
        }),
        kind: BackendNodeKind::Matrix,
        text: None,
        run: None,
        attributes: std::collections::BTreeMap::from([("style".to_owned(), "aligned".to_owned())]),
        text_style: None,
        children: rows,
    };
    layout_nodes(
        font,
        vec![matrix],
        size,
        MathStyle::Display,
        text_shaper,
        host_row_grid.map(|_| align.id.clone()),
        host_row_grid,
    )
}

fn layout_nodes<'a>(
    font: &'a MathFont,
    backend_nodes: Vec<Node>,
    size: f64,
    style: MathStyle,
    text_shaper: Option<&'a mut dyn MathTextShaper>,
    host_table_id: Option<NodeId>,
    host_row_grid: Option<MathHostRowGrid>,
) -> Result<MathLayout, MathLayoutError> {
    let mut converter = Converter {
        ctx: Context::new(font, size),
        text_shaper,
        text_shaping_error: None,
        requested_text_family_missing: false,
        text_tokens: std::collections::HashSet::new(),
        math_text_style: Default::default(),
    };
    converter.ctx.host_table_id = host_table_id;
    if let Some(row_grid) = host_row_grid {
        converter.ctx.host_row_spacing = Some(row_grid.spacing);
        converter.ctx.host_row_ascent = row_grid.ascent;
        converter.ctx.host_row_descent = row_grid.descent;
        converter.ctx.host_row_leading = row_grid.leading;
    }
    let fragment = match backend_nodes.as_slice() {
        [node] => converter.node(node, Style::root(style == MathStyle::Display)),
        nodes => converter.row(nodes, Style::root(style == MathStyle::Display)),
    };
    if converter.text_shaping_error.is_some() {
        return Err(MathLayoutError::TextShapingFailed);
    }
    let frame = fragment.frame;
    let baseline = frame.baseline;
    let mut output_item_prefix = Vec::with_capacity(frame.items.len() + 1);
    output_item_prefix.push(0);
    for (_, item) in &frame.items {
        let previous = *output_item_prefix.last().unwrap_or(&0);
        output_item_prefix.push(previous + usize::from(!matches!(item, Item::Node { .. })));
    }
    let mut layout = MathLayout {
        width: frame.width,
        ascent: frame.ascent(),
        descent: frame.descent(),
        items: Vec::with_capacity(frame.items.len()),
        nodes: Vec::new(),
        row_clips: frame
            .row_clips
            .iter()
            .map(|clip| MathRowClip {
                row_id: clip.row_id.clone(),
                item_range: output_item_prefix[clip.item_range.start]
                    ..output_item_prefix[clip.item_range.end],
                y0: clip.y0 - baseline,
                y1: clip.y1 - baseline,
            })
            .collect(),
        row_baselines: frame
            .row_baselines
            .iter()
            .map(|row| MathRowBaseline {
                row_id: row.row_id.clone(),
                baseline: row.baseline - baseline,
            })
            .collect(),
        diagnostics: converter.ctx.diagnostics,
        requested_text_family_missing: converter.requested_text_family_missing,
    };
    for (point, item) in frame.items {
        let (x, y) = (point.x, point.y - baseline);
        match item {
            Item::Glyph { glyph, size, color } => layout.items.push(MathItem::Glyph {
                glyph,
                x,
                y,
                size,
                color,
            }),
            Item::Rule {
                width,
                height,
                color,
            } => layout.items.push(MathItem::Rule {
                x,
                y,
                width,
                height,
                color,
            }),
            Item::Line {
                dx,
                dy,
                thickness,
                color,
            } => layout.items.push(MathItem::Line {
                x,
                y,
                dx,
                dy,
                thickness,
                color,
            }),
            Item::Rect {
                width,
                height,
                fill,
                stroke,
            } => layout.items.push(MathItem::Rectangle {
                x,
                y,
                width,
                height,
                fill,
                stroke: stroke.and_then(|(color, _)| color),
                stroke_width: stroke.map_or(0.0, |(_, width)| width),
            }),
            Item::TextIsland {
                token,
                width,
                baseline,
                color,
            } => layout.items.push(MathItem::TextIsland {
                token,
                x,
                baseline_y: y + baseline,
                width,
                color,
            }),
            Item::Node {
                id,
                position,
                width,
                height,
                baseline: node_baseline,
            } => {
                if let Some(position) = position {
                    layout.nodes.push(MathNodeBox {
                        id,
                        scope_id: position.scope_id,
                        path: position.path,
                        text_run_index: position.text_run_index,
                        is_text_flow: position.is_text_flow,
                        is_flow: position.is_flow,
                        is_container: position.is_container,
                        x,
                        y,
                        width,
                        height,
                        baseline: y + node_baseline,
                        font_size_pt: position.font_size_pt,
                    });
                }
            }
        }
    }
    coalesce_container_boxes(&mut layout.nodes);
    expand_container_boxes(&mut layout.nodes);
    Ok(layout)
}

type ContainerBoxIdentity = (NodeId, NodeId, Vec<(u8, usize)>);

fn coalesce_container_boxes(nodes: &mut Vec<MathNodeBox>) {
    let mut indexes: HashMap<ContainerBoxIdentity, usize> = HashMap::new();
    let mut coalesced: Vec<MathNodeBox> = Vec::with_capacity(nodes.len());
    for node in nodes.drain(..) {
        let identity = node.id.as_ref().and_then(|id| {
            (node.is_flow || node.is_container).then(|| {
                let path = node
                    .path
                    .iter()
                    .map(|step| match step {
                        MathPathStep::Content(index) => (0, *index),
                        MathPathStep::EnvironmentChild(index) => (1, *index),
                        MathPathStep::Operand(index) => (2, *index),
                    })
                    .collect();
                (id.clone(), node.scope_id.clone(), path)
            })
        });
        let Some(identity) = identity else {
            coalesced.push(node);
            continue;
        };
        if let Some(&index) = indexes.get(&identity) {
            let existing = &mut coalesced[index];
            let existing_was_empty = existing.width == 0.0 && existing.height == 0.0;
            let right = (existing.x + existing.width).max(node.x + node.width);
            let bottom = (existing.y + existing.height).max(node.y + node.height);
            existing.x = existing.x.min(node.x);
            existing.y = existing.y.min(node.y);
            existing.width = right - existing.x;
            existing.height = bottom - existing.y;
            existing.is_flow |= node.is_flow;
            existing.is_container |= node.is_container;
            if existing_was_empty && (node.width != 0.0 || node.height != 0.0) {
                existing.baseline = node.baseline;
            }
        } else {
            indexes.insert(identity, coalesced.len());
            coalesced.push(node);
        }
    }
    *nodes = coalesced;
}

fn expand_container_boxes(nodes: &mut [MathNodeBox]) {
    fn contains(ancestor: &MathNodeBox, child: &MathNodeBox) -> bool {
        ancestor.scope_id == child.scope_id
            && child.path.starts_with(&ancestor.path)
            && (child.path.len() > ancestor.path.len() || child.is_text_flow)
    }

    fn include(bounds: &mut Option<(f64, f64, f64, f64)>, node: &MathNodeBox) {
        let node_bounds = (node.x, node.y, node.x + node.width, node.y + node.height);
        *bounds = Some(match *bounds {
            Some((x0, y0, x1, y1)) => (
                x0.min(node_bounds.0),
                y0.min(node_bounds.1),
                x1.max(node_bounds.2),
                y1.max(node_bounds.3),
            ),
            None => node_bounds,
        });
    }

    // 盒子按前序排列，使每个节点只遍历当前活动流/容器祖先（深度上限为 32）。
    let mut ancestors = Vec::new();
    let mut bounds = vec![None; nodes.len()];
    for index in 0..nodes.len() {
        while ancestors
            .last()
            .is_some_and(|&ancestor| !contains(&nodes[ancestor], &nodes[index]))
        {
            ancestors.pop();
        }
        for &ancestor in &ancestors {
            include(&mut bounds[ancestor], &nodes[index]);
        }
        if nodes[index].is_flow || nodes[index].is_container {
            include(&mut bounds[index], &nodes[index]);
            ancestors.push(index);
        }
    }

    for (node, bounds) in nodes.iter_mut().zip(bounds) {
        let Some((x0, y0, x1, y1)) = bounds else {
            continue;
        };
        node.x = x0;
        node.y = y0;
        node.width = x1 - x0;
        node.height = y1 - y0;
    }
}

fn validate_size(size: f64) -> Result<(), MathLayoutError> {
    if !size.is_finite() || size <= 0.0 || size > DocumentUnits::max_font_size_points() {
        return Err(MathLayoutError::InvalidSize);
    }
    Ok(())
}

fn validate_flow_templates(
    flow: &MathFlow,
    templates: &InlineEnvironmentTemplates,
) -> Result<(), MathLayoutError> {
    for content in &flow.content {
        match content {
            MathContent::Environment(environment) => {
                environment
                    .resolved_text_style(templates)
                    .map_err(|_| MathLayoutError::InvalidTemplate)?;
                for child in &environment.children {
                    validate_content_templates(child, templates)?;
                }
            }
            MathContent::Math(node) => {
                for operand in &node.children {
                    validate_flow_templates(operand, templates)?;
                }
            }
            MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
        }
    }
    Ok(())
}

fn validate_content_templates(
    content: &MathContent,
    templates: &InlineEnvironmentTemplates,
) -> Result<(), MathLayoutError> {
    match content {
        MathContent::Environment(environment) => {
            environment
                .resolved_text_style(templates)
                .map_err(|_| MathLayoutError::InvalidTemplate)?;
            for child in &environment.children {
                validate_content_templates(child, templates)?;
            }
        }
        MathContent::Math(node) => {
            for operand in &node.children {
                validate_flow_templates(operand, templates)?;
            }
        }
        MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
    }
    Ok(())
}

/// 缓存键：内容哈希（含节点 ID）、字号位模式、样式、字体标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    content: u64,
    size_bits: u64,
    style: MathStyle,
    font: u64,
}

/// 缓存条目上限；超过时整体清空，避免无界增长。
const CACHE_LIMIT: usize = 4096;

/// 驻留字体与按公式缓存的布局引擎。
#[derive(Debug)]
pub struct MathLayoutEngine {
    font: &'static MathFont,
    cache: Mutex<HashMap<CacheKey, Arc<MathLayout>>>,
}

impl MathLayoutEngine {
    /// 使用内置 STIX Two Math 字体。
    pub fn new() -> Result<Self, MathFontError> {
        Ok(Self::with_font(MathFont::builtin()?))
    }

    pub fn with_font(font: &'static MathFont) -> Self {
        Self {
            font,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn font(&self) -> &'static MathFont {
        self.font
    }

    /// 排版并缓存；相同内容、字号、样式与字体的重复请求直接返回缓存结果。
    ///
    /// 此便捷 API 没有正文 shaper；含 `TextFlow` 的内容使用数学字体降级路径，不具备正文
    /// 字体选择、脚本/语言偏好或 glyph fallback。需要这些语义时调用
    /// [`Self::layout_with_text_shaper`]。
    pub fn layout(
        &self,
        root: &MathFlow,
        size: f64,
        style: MathStyle,
    ) -> Result<Arc<MathLayout>, MathLayoutError> {
        validate_size(size)?;
        let key = CacheKey {
            content: content_hash(root),
            size_bits: size.to_bits(),
            style,
            font: self.font.identity(),
        };
        if let Some(hit) = self.lock().get(&key) {
            return Ok(Arc::clone(hit));
        }
        let layout = Arc::new(layout_math(self.font, root, size, style)?);
        let mut cache = self.lock();
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, Arc::clone(&layout));
        Ok(layout)
    }

    /// 通过正文 shaping 回调排版；为避免复用悬空的局部 sidecar，此路径不进入内容缓存。
    pub fn layout_with_text_shaper(
        &self,
        root: &MathFlow,
        size: f64,
        style: MathStyle,
        text_shaper: &mut dyn MathTextShaper,
    ) -> Result<Arc<MathLayout>, MathLayoutError> {
        Ok(Arc::new(layout_math_with_text_shaper(
            self.font,
            root,
            size,
            style,
            text_shaper,
        )?))
    }

    /// 通过正文 shaping 和主题模板排版；局部 sidecar 不进入内容缓存。
    pub fn layout_with_text_shaper_and_templates(
        &self,
        root: &MathFlow,
        size: f64,
        style: MathStyle,
        templates: &InlineEnvironmentTemplates,
        text_shaper: &mut dyn MathTextShaper,
    ) -> Result<Arc<MathLayout>, MathLayoutError> {
        Ok(Arc::new(layout_math_with_text_shaper_and_templates(
            self.font,
            root,
            size,
            style,
            templates,
            text_shaper,
        )?))
    }

    /// 当前缓存条目数。
    pub fn cached_len(&self) -> usize {
        self.lock().len()
    }

    pub fn clear_cache(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<CacheKey, Arc<MathLayout>>> {
        // 缓存内容在锁中毒后仍然一致（只有插入与清空），直接取回。
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// 节点内容哈希：目前对整条 MathFlow 做 JSON 编码后哈希，包含扩展字段和源范围。
/// 该实现避免缓存键漏掉布局相关状态，但会分配临时字节缓冲；性能影响待基准验证。
fn content_hash(root: &MathFlow) -> u64 {
    let mut hasher = DefaultHasher::new();
    match serde_json::to_vec(root) {
        Ok(content) => content.hash(&mut hasher),
        Err(_) => format!("{root:?}").hash(&mut hasher),
    }
    hasher.finish()
}

#[cfg(test)]
mod tests;

/// 浮点判零容差（单位与被比较量相同）。
pub(crate) const FLOAT_EPSILON: f64 = 1e-9;

/// 判断浮点值是否在容差内为零。
pub(crate) fn is_zero(value: f64) -> bool {
    value.abs() < FLOAT_EPSILON
}
