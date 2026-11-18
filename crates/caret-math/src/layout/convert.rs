//! 文档数学树到排版片段的转换：样式继承、原子类别、各节点类别的排版调度。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{
    DEFAULT_MATH_MATRIX_STYLE, MathAccentPlacement, MathAccentRendering, MathDelimiterSize,
    MathMatrixAlignment, MathMatrixCellSize, MathMatrixColumnGap, math_accent_by_value,
    math_matrix_style,
};
use read_fonts::tables::math::MathConstant as C;

use super::accent::{CancelKind, layout_accent, layout_cancel, layout_cancel_to, layout_line};
use super::alphabet::styled_char;
use super::context::{Context, MathSize, Style, Variant};
use super::fenced::{balanced_height, delimiter};
use super::fraction::layout_fraction;
use super::frame::{Fragment, Frame, Item, MathColor, Point};
use super::glyph::{center_on_axis, glyph_fragment, stretch};
use super::radical::layout_radical;
use super::run::layout_row;
use super::scripts::{Attachments, layout_attachments};
use super::table::{Align, TableRow, TableSpec, layout_table};
use super::text::{
    MathTextContainer, MathTextRequest, MathTextRun, MathTextRunBox, MathTextRunStyle,
    MathTextShape,
};
use crate::backend::BackendNodeKind as K;
use crate::backend::{Node as MathNode, NodePosition};
use crate::fonts::Axis;
use crate::symbols::{AtomClass, delimiter_char, literal_display_char, named_symbol};

/// 占位框颜色。
const PLACEHOLDER_COLOR: MathColor = MathColor {
    red: 0x9e,
    green: 0x9e,
    blue: 0x9e,
    alpha: 0xff,
};
/// `\boxed`、`\colorbox` 的内边距（pt）。
const BOX_PADDING: f64 = 3.0;
/// `\boxed`、`\fcolorbox` 的边框粗细（pt）。
const BOX_STROKE: f64 = 0.4;

pub(crate) struct Converter<'a> {
    pub ctx: Context<'a>,
    pub text_shaper: Option<&'a mut dyn super::MathTextShaper>,
    pub text_shaping_error: Option<super::MathTextShapeError>,
    pub requested_text_family_missing: bool,
    pub(crate) text_tokens: std::collections::HashSet<u64>,
    pub math_text_style: MathTextRunStyle,
}

/// 一次文本岛的累积状态与上下文。
///
/// 文本 run、container 区间和字节偏移在同一个请求内连续累积；遇到数学节点或流式
/// `Row` 时中途落盘。内联样式作用域会替换继承样式与 container 栈，收集完子树后
/// 恢复，因此递归不再需要额外的上下文参数。
struct TextSequenceCollector {
    /// 本次收集固定的排版样式。
    style: Style,
    /// 当前作用域继承到文本 run 的行内样式。
    inherited: MathTextRunStyle,
    /// 当前作用域的 container 栈，外层在前。
    active_containers: Vec<MathTextContainer>,
    /// 正在累积的排版请求。
    request: MathTextRequest,
    /// 当前请求内已累积的 UTF-8 字节偏移。
    byte_offset: usize,
    /// 文本整形错误；置位后停止收集，由调用方统一上报。
    stream_error: Option<super::MathTextShapeError>,
}

impl TextSequenceCollector {
    /// 按初始样式建立收集器；请求随收集器一起累积。
    fn new(
        converter: &Converter<'_>,
        style: Style,
        inherited: MathTextRunStyle,
        active_containers: &[MathTextContainer],
    ) -> Self {
        Self {
            style,
            inherited,
            active_containers: active_containers.to_vec(),
            request: Self::empty_request(converter, style),
            byte_offset: 0,
            stream_error: None,
        }
    }

    /// 按当前样式与颜色构造空请求。
    fn empty_request(converter: &Converter<'_>, style: Style) -> MathTextRequest {
        MathTextRequest {
            font_size_pt: converter.ctx.font_size(style),
            color: style.color,
            runs: Vec::new(),
            containers: Vec::new(),
        }
    }

    /// 落盘当前请求并重置字节偏移。
    fn flush(&mut self, converter: &mut Converter<'_>, out: &mut Vec<Fragment>) {
        let empty = Self::empty_request(converter, self.style);
        let request = std::mem::replace(&mut self.request, empty);
        converter.flush_text_request(request, self.style, out);
        self.byte_offset = 0;
    }
}

/// 查找已登记的同一 container（按作用域与路径判定），返回其下标。
fn find_container(
    containers: &[MathTextContainer],
    container: &MathTextContainer,
) -> Option<usize> {
    containers.iter().position(|existing| {
        existing.scope_id == container.scope_id && existing.path == container.path
    })
}

/// 以统一字段追加 container 区间；区间端点由调用方按语义给出。
fn push_container(
    containers: &mut Vec<MathTextContainer>,
    container: &MathTextContainer,
    start: usize,
    end: usize,
) {
    containers.push(MathTextContainer {
        start_byte: start,
        end_byte: end,
        ..container.clone()
    });
}

/// 登记新进入的 container：未登记者以 `offset` 为起点建零长区间，已登记者保持不变。
fn open_container_ranges(
    containers: &mut Vec<MathTextContainer>,
    opened: &[MathTextContainer],
    offset: usize,
) {
    for container in opened {
        if find_container(containers, container).is_none() {
            push_container(containers, container, offset, offset);
        }
    }
}

/// 把文本片段的结束偏移写入所有生效 container 的区间；未登记者以片段起点补登记。
fn extend_container_ranges(
    containers: &mut Vec<MathTextContainer>,
    active: &[MathTextContainer],
    start: usize,
    end: usize,
) {
    for container in active {
        // 下标来自同一列表的查找，必然有效；直接索引避免额外分支。
        if let Some(index) = find_container(containers, container) {
            containers[index].end_byte = end;
        } else {
            push_container(containers, container, start, end);
        }
    }
}

impl Converter<'_> {
    fn styled_position(
        &self,
        position: &Option<NodePosition>,
        style: Style,
    ) -> Option<NodePosition> {
        let mut position = position.clone()?;
        position.font_size_pt = Some(self.ctx.font_size(style));
        Some(position)
    }

    /// 排版一个节点，并记录其范围供命中测试。
    pub(crate) fn node(&mut self, node: &MathNode, style: Style) -> Fragment {
        if matches!(
            node.kind,
            K::TextContent | K::TextSpace | K::TextSymbol | K::TextStyle | K::SoftBreak | K::Opaque
        ) {
            return self.layout(node, style);
        }
        let mut fragment = self.layout(node, style);
        // 可选占位符不参与排版，但其 MathFlow 身份仍供结构导航使用。
        for &index in node.kind.optional_placeholder_operands().iter().rev() {
            let Some(child) = node.children.get(index) else {
                continue;
            };
            if !is_placeholder_flow(child) {
                continue;
            }
            let baseline = fragment.frame.baseline;
            fragment.frame.insert(
                0,
                Point::new(0.0, baseline),
                Item::Node {
                    id: child.id.clone(),
                    position: self.styled_position(&child.position, style),
                    width: 0.0,
                    height: 0.0,
                    baseline: 0.0,
                },
            );
        }
        let frame = &fragment.frame;
        let record = Item::Node {
            id: node.id.clone(),
            position: self.styled_position(&node.position, style),
            width: frame.width,
            height: frame.height,
            baseline: frame.baseline,
        };
        fragment
            .frame
            .items
            .insert(0, (Point::new(0.0, 0.0), record));
        fragment
    }

    /// 把子节点序列排成一行；嵌套 Row 与 `\color` 展开到同一行参与间距计算。
    pub(crate) fn row(&mut self, children: &[MathNode], style: Style) -> Fragment {
        let mut fragments = Vec::with_capacity(children.len());
        if self.text_shaper.is_some() {
            let inherited = self.math_text_style.clone();
            self.collect_text_sequence(children, style, inherited, &[], &mut fragments);
        } else {
            self.collect(children, style, &mut fragments);
        }
        if fragments.is_empty() {
            return Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord);
        }
        layout_row(&self.ctx, style, fragments)
    }

    fn collect_text_sequence(
        &mut self,
        children: &[MathNode],
        style: Style,
        inherited: MathTextRunStyle,
        active_containers: &[MathTextContainer],
        out: &mut Vec<Fragment>,
    ) {
        let mut collector = TextSequenceCollector::new(self, style, inherited, active_containers);
        self.collect_text_sequence_into(children, &mut collector, out);
        collector.flush(self, out);
        if let Some(error) = collector.stream_error {
            self.text_shaping_error = Some(error);
        }
    }

    /// 收集文本岛内的子节点；非文本节点在当前请求边界落盘后直接排版。
    fn collect_text_sequence_into(
        &mut self,
        children: &[MathNode],
        collector: &mut TextSequenceCollector,
        out: &mut Vec<Fragment>,
    ) {
        for child in children {
            if collector.stream_error.is_some() {
                return;
            }
            match child.kind {
                K::Text if Self::attribute(child, "style") != "text" => {
                    collector.flush(self, out);
                    let math_style = Style {
                        text_mode: false,
                        ..collector.style
                    };
                    out.push(self.node(child, math_style));
                }
                K::TextContent | K::TextSymbol | K::TextSpace | K::SoftBreak | K::Opaque => {
                    let isolated = child.kind == K::Opaque;
                    if isolated {
                        collector.flush(self, out);
                    }
                    let Some(position) = child.position.as_ref() else {
                        collector.stream_error = Some(super::MathTextShapeError::UnsupportedInput);
                        return;
                    };
                    let text = if child.kind == K::SoftBreak {
                        "\n".to_owned()
                    } else if child.kind == K::TextSpace {
                        " ".to_owned()
                    } else if child.kind == K::Opaque {
                        "�".to_owned()
                    } else {
                        child
                            .run
                            .as_ref()
                            .map(|run| run.text.clone())
                            .or_else(|| child.text.clone())
                            .unwrap_or_default()
                    };
                    let Some(end) = collector.byte_offset.checked_add(text.len()) else {
                        collector.stream_error = Some(super::MathTextShapeError::UnsupportedInput);
                        return;
                    };
                    extend_container_ranges(
                        &mut collector.request.containers,
                        &collector.active_containers,
                        collector.byte_offset,
                        end,
                    );
                    collector.request.runs.push(MathTextRun {
                        run: child.run.clone(),
                        text,
                        scope_id: position.scope_id.clone(),
                        path: position.path.clone(),
                        run_index: position.text_run_index,
                        is_text_flow: position.is_text_flow,
                        style: collector.inherited.clone(),
                    });
                    collector.byte_offset = end;
                    if isolated {
                        collector.flush(self, out);
                    }
                }
                K::Text | K::TextStyle => {
                    let Some(position) = child.position.as_ref() else {
                        collector.stream_error = Some(super::MathTextShapeError::UnsupportedInput);
                        return;
                    };
                    let container = MathTextContainer {
                        id: child.id.clone(),
                        scope_id: position.scope_id.clone(),
                        path: position.path.clone(),
                        start_byte: collector.byte_offset,
                        end_byte: collector.byte_offset,
                        is_flow: position.is_flow,
                        is_container: position.is_container,
                    };
                    let mut nested_containers = collector.active_containers.clone();
                    nested_containers.push(container);
                    open_container_ranges(
                        &mut collector.request.containers,
                        &nested_containers,
                        collector.byte_offset,
                    );
                    let mut nested_style = collector.inherited.clone();
                    if let Some(environment_style) = child.text_style.as_ref() {
                        apply_inline_environment_style(&mut nested_style, environment_style);
                    }
                    let outer_style = std::mem::replace(&mut collector.inherited, nested_style);
                    let outer_containers =
                        std::mem::replace(&mut collector.active_containers, nested_containers);
                    self.collect_text_sequence_into(&child.children, collector, out);
                    collector.inherited = outer_style;
                    collector.active_containers = outer_containers;
                }
                K::Row
                    if child
                        .position
                        .as_ref()
                        .is_some_and(|position| position.is_flow) =>
                {
                    collector.flush(self, out);
                    if child.children.is_empty() {
                        out.push(self.node(child, collector.style));
                        continue;
                    }
                    let mut frame = Frame::new(0.0, 0.0, 0.0);
                    frame.push(
                        Point::default(),
                        Item::Node {
                            id: child.id.clone(),
                            position: self.styled_position(&child.position, collector.style),
                            width: 0.0,
                            height: 0.0,
                            baseline: 0.0,
                        },
                    );
                    let mut marker = Fragment::new(frame, AtomClass::Ord);
                    marker.position_marker = true;
                    out.push(marker);
                    let Some(_position) = child.position.as_ref() else {
                        collector.stream_error = Some(super::MathTextShapeError::UnsupportedInput);
                        return;
                    };
                    self.collect_text_sequence_into(&child.children, collector, out);
                    collector.flush(self, out);
                }
                K::Hline | K::Intertext => {
                    collector.flush(self, out);
                }
                _ => {
                    collector.flush(self, out);
                    let math_style = if collector.style.text_mode {
                        Style {
                            text_mode: false,
                            variant: Variant::Normal,
                            ..collector.style
                        }
                    } else {
                        collector.style
                    };
                    let previous_text_style =
                        std::mem::replace(&mut self.math_text_style, collector.inherited.clone());
                    let fragment = self.node(child, math_style);
                    self.math_text_style = previous_text_style;
                    out.push(fragment);
                }
            }
        }
    }

    fn flush_text_request(
        &mut self,
        request: MathTextRequest,
        _style: Style,
        out: &mut Vec<Fragment>,
    ) {
        if request.runs.is_empty() && request.containers.is_empty() {
            return;
        }
        out.push(self.shape_text_request(request));
    }

    fn collect(&mut self, children: &[MathNode], style: Style, out: &mut Vec<Fragment>) {
        for child in children {
            match child.kind {
                K::Row
                    if child.children.is_empty()
                        && child
                            .position
                            .as_ref()
                            .is_some_and(|position| position.is_flow) =>
                {
                    out.push(self.node(child, style));
                }
                K::Row => {
                    let mut frame = Frame::new(0.0, 0.0, 0.0);
                    frame.push(
                        Point::default(),
                        Item::Node {
                            id: child.id.clone(),
                            position: self.styled_position(&child.position, style),
                            width: 0.0,
                            height: 0.0,
                            baseline: 0.0,
                        },
                    );
                    let mut marker = Fragment::new(frame, AtomClass::Ord);
                    marker.position_marker = true;
                    out.push(marker);
                    self.collect(&child.children, style, out);
                }
                K::Hline | K::Intertext => {}
                _ => out.push(self.node(child, style)),
            }
        }
    }

    fn text_segments(
        &mut self,
        children: &[MathNode],
        style: Style,
        inherited: MathTextRunStyle,
    ) -> Vec<Fragment> {
        if self.text_shaper.is_some() {
            let mut fragments = Vec::new();
            self.collect_text_sequence(children, style, inherited, &[], &mut fragments);
            return fragments;
        }
        let mut fragments = Vec::new();
        let mut index = 0;
        while index < children.len() {
            let child = &children[index];
            match child.kind {
                K::TextStyle => {
                    let mut text_style = inherited.clone();
                    apply_text_style(
                        &mut text_style,
                        Self::attribute(child, "style"),
                        child.attributes.get("fontFamily").map(String::as_str),
                    );
                    let nested_style = Style {
                        variant: Variant::Roman,
                        text_mode: true,
                        ..style
                    };
                    let nested = self.text_segments(&child.children, nested_style, text_style);
                    let mut fragment = self.joined(nested, AtomClass::Ord, nested_style);
                    let (width, height, baseline) = (
                        fragment.frame.width,
                        fragment.frame.height,
                        fragment.frame.baseline,
                    );
                    fragment.frame.insert(
                        0,
                        Point::default(),
                        Item::Node {
                            id: child.id.clone(),
                            position: self.styled_position(&child.position, style),
                            width,
                            height,
                            baseline,
                        },
                    );
                    fragments.push(fragment);
                }
                _ => {
                    let math_style = Style {
                        text_mode: false,
                        variant: Variant::Normal,
                        ..style
                    };
                    fragments.push(self.node(child, math_style));
                }
            }
            index += 1;
        }
        fragments
    }

    fn shape_text_request(&mut self, request: MathTextRequest) -> Fragment {
        let shaped = match self.text_shaper.as_deref_mut() {
            Some(shaper) => shaper.shape(request.clone()),
            None => return Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord),
        };
        let shaped = match shaped {
            Ok(shaped) => shaped,
            Err(error) => {
                self.text_shaping_error = Some(error);
                return Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord);
            }
        };
        if !self.validate_text_shape(&shaped, &request) {
            self.text_shaping_error = Some(super::MathTextShapeError::InvalidMetrics);
            return Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord);
        }
        self.requested_text_family_missing |= shaped.requested_family_missing;
        for family in &shaped.missing_requested_families {
            self.ctx
                .diagnostic("math.text.font.family_missing", family.clone());
        }
        if shaped.has_missing_glyph {
            self.ctx.diagnostic(
                "math.glyph.missing",
                "formula text island contains an unavailable glyph",
            );
        }
        let mut frame = Frame::new(shaped.width, shaped.ascent + shaped.descent, shaped.ascent);
        if !request.runs.is_empty() {
            frame.push(
                Point::default(),
                Item::TextIsland {
                    token: shaped.token,
                    width: shaped.width,
                    baseline: shaped.ascent,
                    color: request.color,
                },
            );
        }
        let mut text_node_order = Vec::with_capacity(shaped.runs.len() + shaped.containers.len());
        let mut byte_offset: usize = 0;
        for (index, run) in request.runs.iter().enumerate() {
            let end = byte_offset.saturating_add(run.text.len());
            text_node_order.push((
                byte_offset,
                std::cmp::Reverse(end),
                run.path.len(),
                1_u8,
                index,
            ));
            byte_offset = end;
        }
        for (index, container) in request.containers.iter().enumerate() {
            text_node_order.push((
                container.start_byte,
                std::cmp::Reverse(container.end_byte),
                container.path.len(),
                0_u8,
                index,
            ));
        }
        text_node_order.sort_by_key(|order| (order.0, order.1, order.2, order.3));
        for (_, _, _, kind, index) in text_node_order {
            let (point, node) = if kind == 0 {
                let container = shaped.containers[index].clone();
                (
                    Point::new(container.x, container.y),
                    Item::Node {
                        id: container.id,
                        position: Some(NodePosition {
                            scope_id: container.scope_id,
                            path: container.path,
                            text_run_index: None,
                            is_text_flow: false,
                            is_flow: container.is_flow,
                            is_container: container.is_container,
                            font_size_pt: Some(request.font_size_pt),
                        }),
                        width: container.width,
                        height: container.height,
                        baseline: container.baseline,
                    },
                )
            } else {
                let run = shaped.runs[index].clone();
                (
                    Point::new(run.x, run.y),
                    Item::Node {
                        id: None,
                        position: Some(NodePosition {
                            scope_id: run.scope_id,
                            path: run.path,
                            text_run_index: run.run_index,
                            is_text_flow: run.is_text_flow,
                            is_flow: run.is_flow,
                            is_container: false,
                            font_size_pt: Some(request.font_size_pt),
                        }),
                        width: run.width,
                        height: run.height,
                        baseline: run.baseline,
                    },
                )
            };
            frame.push(point, node);
        }
        let mut fragment = Fragment::new(frame, AtomClass::Ord);
        if request.runs.is_empty() {
            fragment.position_marker = true;
        } else {
            fragment.text_like = true;
        }
        fragment
    }

    /// 校验整形结果与请求是否一致；任何违规都统一映射为 `InvalidMetrics`。
    ///
    /// token 登记必须最先执行：即使后续字段校验失败，该 token 也已占用。
    fn validate_text_shape(&mut self, shaped: &MathTextShape, request: &MathTextRequest) -> bool {
        self.text_tokens.insert(shaped.token)
            && Self::valid_text_metrics(shaped)
            && Self::validate_runs(shaped, request)
            && Self::validate_containers(shaped, request)
    }

    /// 文本岛整体几何：宽度与上/下行高必须有限且非负。
    fn valid_text_metrics(shaped: &MathTextShape) -> bool {
        shaped.width.is_finite()
            && shaped.width >= 0.0
            && shaped.ascent.is_finite()
            && shaped.ascent >= 0.0
            && shaped.descent.is_finite()
            && shaped.descent >= 0.0
    }

    /// run 数量、逐项身份与几何必须与请求一致。
    fn validate_runs(shaped: &MathTextShape, request: &MathTextRequest) -> bool {
        shaped.runs.len() == request.runs.len()
            && shaped.runs.iter().zip(&request.runs).all(|(bounds, run)| {
                bounds.id.is_none()
                    && !bounds.is_flow
                    && !bounds.is_container
                    && bounds.scope_id == run.scope_id
                    && bounds.path == run.path
                    && bounds.run_index == run.run_index
                    && bounds.is_text_flow == run.is_text_flow
                    && Self::valid_bounds_geometry(bounds)
            })
    }

    /// container 数量、逐项身份与几何必须与请求一致。
    fn validate_containers(shaped: &MathTextShape, request: &MathTextRequest) -> bool {
        shaped.containers.len() == request.containers.len()
            && shaped
                .containers
                .iter()
                .zip(&request.containers)
                .all(|(bounds, container)| {
                    bounds.id.as_ref() == container.id.as_ref()
                        && bounds.scope_id == container.scope_id
                        && bounds.path == container.path
                        && bounds.run_index.is_none()
                        && bounds.is_flow == container.is_flow
                        && bounds.is_container == container.is_container
                        && Self::valid_bounds_geometry(bounds)
                })
    }

    /// 边界几何：坐标、宽高与基线必须有限，宽高非负。
    fn valid_bounds_geometry(bounds: &MathTextRunBox) -> bool {
        bounds.x.is_finite()
            && bounds.y.is_finite()
            && bounds.width.is_finite()
            && bounds.width >= 0.0
            && bounds.height.is_finite()
            && bounds.height >= 0.0
            && bounds.baseline.is_finite()
    }

    fn child(&mut self, node: &MathNode, index: usize, style: Style) -> Fragment {
        match node.children.get(index) {
            Some(child) => self.node(child, style),
            None => self.placeholder(style),
        }
    }

    /// 可缺省的子项：不存在或为占位节点时返回 `None`。
    fn optional(&mut self, node: &MathNode, index: usize, style: Style) -> Option<Fragment> {
        node.children
            .get(index)
            .filter(|child| child.kind != K::Placeholder && !is_placeholder_flow(child))
            .map(|child| self.node(child, style))
    }

    fn attribute<'n>(node: &'n MathNode, key: &str) -> &'n str {
        node.attributes.get(key).map_or("", String::as_str)
    }

    fn layout(&mut self, node: &MathNode, style: Style) -> Fragment {
        match node.kind {
            K::Row if is_empty_host_flow(node) => {
                // 宿主根占位保留内部节点盒与几何，但不把交互区域画成公式必填槽。
                let mut fragment = if node.children.is_empty() {
                    self.placeholder(style)
                } else {
                    self.row(&node.children, style)
                };
                fragment.frame.hide();
                fragment
            }
            // 空流仍是可编辑槽位；仅在布局中产生占位几何，不修改 MathFlow。
            K::Row
                if node.children.is_empty()
                    && node
                        .position
                        .as_ref()
                        .is_some_and(|position| position.is_flow) =>
            {
                self.placeholder(style)
            }
            K::Row => self.row(&node.children, style),
            K::Group => {
                let mut group = self.row(&node.children, style);
                group.class = AtomClass::Ord;
                group
            }
            K::Style => {
                let size = match Self::attribute(node, "style") {
                    "display" => Some(MathSize::Display),
                    "text" => Some(MathSize::Text),
                    "script" => Some(MathSize::Script),
                    "scriptscript" => Some(MathSize::ScriptScript),
                    _ => None,
                };
                let style = size.map_or(style, |size| style.with_size(size));
                self.row(&node.children, style)
            }
            K::Placeholder => self.placeholder(style),
            K::Symbol => self.symbol(node, style),
            K::Operator => self.operator(node, style),
            K::Fraction => self.fraction(node, style),
            K::Radical => {
                let index = self.optional(node, 0, style.with_size(MathSize::ScriptScript));
                let body = self.child(node, 1, style.cramped());
                layout_radical(&self.ctx, style, body, index)
            }
            K::Superscript | K::Subscript | K::SubSup => self.scripts(node, style),
            K::Sum | K::Product | K::Integral | K::Limit => self.big_operator(node, style),
            K::Delimited => self.delimited(node, style),
            K::SizedDelimiter => self.sized_delimiter(node, style),
            K::Matrix => self.matrix(node, style),
            K::Substack => {
                let rows = node
                    .children
                    .iter()
                    .map(|line| TableRow::Cells {
                        id: None,
                        position: line.position.clone(),
                        cells: vec![self.node(line, style)],
                    })
                    .collect();
                let spec = TableSpec {
                    aligns: vec![Align::Center],
                    vlines: Vec::new(),
                    hlines: Vec::new(),
                    column_gap: 0.0,
                    row_gap: 0.0,
                    row_spacing: None,
                    base_row_ascent: 0.0,
                    base_row_descent: 0.0,
                    base_row_leading: 0.0,
                };
                layout_table(&mut self.ctx, style, rows, &spec)
            }
            K::OverUnder => {
                let mut base = self.child(node, 0, style);
                let top = self.optional(node, 1, style.superscript());
                let bottom = self.optional(node, 2, style.subscript());
                let class = base.class;
                base.limits = true;
                let mut fragment = layout_attachments(
                    &self.ctx,
                    style,
                    base,
                    false,
                    Attachments {
                        top,
                        bottom,
                        ..Attachments::default()
                    },
                );
                fragment.class = class;
                fragment
            }
            K::CancelTo => {
                let target = self.child(node, 0, style.superscript());
                let body = self.child(node, 1, style);
                layout_cancel_to(style, body, target)
            }
            K::Accent => self.accent(node, style),
            K::Space => self.space(node, style),
            K::Text | K::MathAlphabet => self.text(node, style),
            K::TextContent
            | K::TextSpace
            | K::TextSymbol
            | K::TextStyle
            | K::SoftBreak
            | K::Opaque => {
                let text_style = Style {
                    text_mode: true,
                    variant: Variant::Roman,
                    ..style
                };
                self.text_item(node, text_style)
            }
            K::Color | K::TextColor => {
                let color = MathColor::parse_hex(Self::attribute(node, "color"));
                let style = Style {
                    color: color.or(style.color),
                    ..style
                };
                self.row(&node.children, style)
            }
            K::ColorBox | K::FColorBox => self.color_box(node, style),
            K::Strut => {
                let em = self.ctx.font_size(style);
                let value = |prefix: &str| {
                    let read = |suffix: &str, default: f64| {
                        node.attributes
                            .get(&format!("{prefix}{suffix}"))
                            .and_then(|value| value.parse::<f64>().ok())
                            .unwrap_or(default)
                    };
                    let denominator = read("Denominator", 1.0);
                    if super::is_zero(denominator) {
                        0.0
                    } else {
                        read("Numerator", 0.0) / denominator * em
                    }
                };
                let (height, depth) = (value("height"), value("depth"));
                Fragment::new(Frame::new(0.0, height + depth, height), AtomClass::Ord)
            }
            K::Phantom => {
                let mut body = self.child(node, 0, style);
                body.frame.hide();
                match Self::attribute(node, "phantomKind") {
                    "hphantom" => {
                        body.frame.height = 0.0;
                        body.frame.baseline = 0.0;
                    }
                    "vphantom" => body.frame.width = 0.0,
                    _ => {}
                }
                body.glyph = None;
                body.class = AtomClass::Ord;
                body
            }
            K::Hline | K::Intertext => Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord),
        }
    }

    // ---- 基本字形 ----

    /// 字符片段；缺字时记录诊断并画缺字框。
    fn character(&mut self, character: char, class: AtomClass, style: Style) -> Fragment {
        let shown = if style.text_mode && style.variant == Variant::Roman {
            character
        } else {
            styled_char(character, style.variant)
        };
        let found = glyph_fragment(&self.ctx, style, shown, class)
            .or_else(|| glyph_fragment(&self.ctx, style, character, class));
        match found {
            Some(fragment) => fragment,
            None => {
                self.ctx
                    .diagnostic("math.glyph.missing", format!("U+{:04X}", character as u32));
                self.tofu(class, style)
            }
        }
    }

    fn tofu(&self, class: AtomClass, style: Style) -> Fragment {
        let em = self.ctx.font_size(style);
        let (width, height) = (0.5 * em, 0.7 * em);
        let mut frame = Frame::new(width, height, height);
        frame.push(
            Point::new(0.0, 0.0),
            Item::Rect {
                width,
                height,
                fill: None,
                stroke: Some((style.color, 0.05 * em)),
            },
        );
        Fragment::new(frame, class)
    }

    fn placeholder(&self, style: Style) -> Fragment {
        let em = self.ctx.font_size(style);
        let (width, height) = (
            super::MATH_PLACEHOLDER_WIDTH_EM * em,
            super::MATH_PLACEHOLDER_HEIGHT_EM * em,
        );
        let mut frame = Frame::new(width, height, height);
        frame.push(
            Point::new(0.0, 0.0),
            Item::Rect {
                width,
                height,
                fill: None,
                stroke: Some((Some(PLACEHOLDER_COLOR), 0.05 * em)),
            },
        );
        Fragment::new(frame, AtomClass::Ord)
    }

    fn symbol(&mut self, node: &MathNode, style: Style) -> Fragment {
        let text = node.text.as_deref().unwrap_or_default();
        if let Some(class) = node.attributes.get("atomClass") {
            let class = AtomClass::parse(class).unwrap_or(AtomClass::Ord);
            let fragments: Vec<Fragment> = text
                .chars()
                .map(|character| self.character(literal_display_char(character), class, style))
                .collect();
            return self.joined(fragments, class, style);
        }
        match named_symbol(text) {
            Some(symbol) => self.character(symbol.character, symbol.class, style),
            None => {
                self.ctx.diagnostic("math.symbol.unknown", text);
                self.tofu(AtomClass::Ord, style)
            }
        }
    }

    /// 多个字符合成一个原子（无原子间距）。
    fn joined(&self, mut fragments: Vec<Fragment>, class: AtomClass, style: Style) -> Fragment {
        if fragments.len() == 1
            && let Some(mut single) = fragments.pop()
        {
            single.class = class;
            return single;
        }
        for fragment in &mut fragments {
            fragment.class = AtomClass::Ord;
        }
        let mut joined = layout_row(&self.ctx, style, fragments);
        joined.class = class;
        joined
    }

    fn upright_word(&mut self, text: &str, class: AtomClass, style: Style) -> Fragment {
        let upright = Style {
            variant: Variant::Roman,
            ..style
        };
        let fragments = text
            .chars()
            .map(|character| self.character(character, AtomClass::Ord, upright))
            .collect();
        self.joined(fragments, class, style)
    }

    fn operator(&mut self, node: &MathNode, style: Style) -> Fragment {
        let text = node.text.as_deref().unwrap_or_default();
        let mut fragment = self.upright_word(text, AtomClass::Op, style);
        fragment.limits = Self::attribute(node, "limits") == "true";
        fragment.large = true;
        fragment.text_like = true;
        fragment
    }

    // ---- 结构 ----

    fn fraction(&mut self, node: &MathNode, style: Style) -> Fragment {
        let kind = Self::attribute(node, "style");
        let inner = match kind {
            "dfrac" | "cfrac" => style.with_size(MathSize::Display),
            "tfrac" => style.with_size(MathSize::Text),
            _ => style,
        };
        let num = self.child(node, 0, inner.numerator());
        let denom = self.child(node, 1, inner.denominator());
        let padding = 0.1 * self.ctx.font_size(inner);
        let mut fragment = layout_fraction(&self.ctx, inner, num, denom, true, padding);
        fragment.class = AtomClass::Inner;
        fragment
    }

    fn scripts(&mut self, node: &MathNode, style: Style) -> Fragment {
        let base = self.child(node, 0, style);
        let (sub, sup) = match node.kind {
            K::Superscript => (None, node.children.get(1)),
            K::Subscript => (node.children.get(1), None),
            _ => (node.children.get(1), node.children.get(2)),
        };
        let sup = sup.map(|child| self.node(child, style.superscript()));
        let sub = sub.map(|child| self.node(child, style.subscript()));
        self.attach(style, base, sup, sub)
    }

    /// 按基底决定放成上下限还是右上右下脚标。
    fn attach(
        &mut self,
        style: Style,
        base: Fragment,
        sup: Option<Fragment>,
        sub: Option<Fragment>,
    ) -> Fragment {
        let display = style.size == MathSize::Display;
        let limits = if base.large {
            base.limits && display
        } else {
            base.limits
        };
        let stretchy = base.stretchy_x;
        let class = base.class;
        let mut fragment = if limits {
            layout_attachments(
                &self.ctx,
                style,
                base,
                stretchy,
                Attachments {
                    top: sup,
                    bottom: sub,
                    ..Attachments::default()
                },
            )
        } else {
            layout_attachments(
                &self.ctx,
                style,
                base,
                false,
                Attachments {
                    sup,
                    sub,
                    ..Attachments::default()
                },
            )
        };
        fragment.class = class;
        fragment
    }

    fn big_operator(&mut self, node: &MathNode, style: Style) -> Fragment {
        let display = style.size == MathSize::Display;
        let (base, limits) = match node.kind {
            K::Limit => {
                let mut base = self.upright_word("lim", AtomClass::Op, style);
                base.large = true;
                (base, true)
            }
            _ => {
                let character = match node.kind {
                    K::Sum => '∑',
                    K::Product => '∏',
                    _ => match Self::attribute(node, "integralKind") {
                        "iint" => '∬',
                        "iiint" => '∭',
                        "oint" => '∮',
                        "oiint" => '∯',
                        _ => '∫',
                    },
                };
                let mut glyph = self.character(character, AtomClass::Op, style);
                if display {
                    let min_height = self.ctx.styled(C::DisplayOperatorMinHeight, style);
                    glyph = stretch(&self.ctx, glyph, Axis::Y, min_height, 0.0);
                }
                center_on_axis(&self.ctx, &mut glyph, style);
                glyph.text_like = false;
                glyph.large = true;
                (glyph, node.kind != K::Integral)
            }
        };
        let mut base = base;
        base.limits = limits;
        let sub = self.optional(node, 0, style.subscript());
        let sup = if node.kind == K::Limit {
            None
        } else {
            self.optional(node, 1, style.superscript())
        };
        if sub.is_none() && sup.is_none() {
            return base;
        }
        self.attach(style, base, sup, sub)
    }

    fn delimited(&mut self, node: &MathNode, style: Style) -> Fragment {
        let body = self.child(node, 0, style);
        let target = balanced_height(&self.ctx, style, std::slice::from_ref(&body));
        let mut parts = Vec::with_capacity(3);
        let left = delimiter_char(Self::attribute(node, "left"));
        let right = delimiter_char(Self::attribute(node, "right"));
        if let Some(open) =
            left.and_then(|c| delimiter(&self.ctx, style, c, AtomClass::Open, target))
        {
            parts.push(open);
        }
        parts.push(body);
        if let Some(close) =
            right.and_then(|c| delimiter(&self.ctx, style, c, AtomClass::Close, target))
        {
            parts.push(close);
        }
        let mut fragment = layout_row(&self.ctx, style, parts);
        fragment.class = AtomClass::Inner;
        fragment.glyph = None;
        fragment
    }

    fn sized_delimiter(&mut self, node: &MathNode, style: Style) -> Fragment {
        let class = match Self::attribute(node, "atomClass") {
            "open" => AtomClass::Open,
            "close" => AtomClass::Close,
            "rel" => AtomClass::Rel,
            _ => AtomClass::Ord,
        };
        let scale =
            MathDelimiterSize::parse(Self::attribute(node, "size")).map_or(1.2, |size| size.scale);
        let target = scale * self.ctx.font_size(style);
        let Some(character) = delimiter_char(node.text.as_deref().unwrap_or("empty")) else {
            return Fragment::new(Frame::new(0.0, 0.0, 0.0), class);
        };
        delimiter(&self.ctx, style, character, class, target)
            .unwrap_or_else(|| self.tofu(class, style))
    }

    fn matrix(&mut self, node: &MathNode, style: Style) -> Fragment {
        let kind = match Self::attribute(node, "style") {
            "" => DEFAULT_MATH_MATRIX_STYLE.name,
            other => other,
        };
        let em = self.ctx.font_size(style);
        let style_spec = math_matrix_style(kind).unwrap_or(&DEFAULT_MATH_MATRIX_STYLE);
        let left = style_spec.left_delimiter;
        let right = style_spec.right_delimiter;
        let columns = node
            .children
            .iter()
            .filter(|child| child.kind == K::Row)
            .map(|row| row.children.len())
            .max()
            .unwrap_or(0);
        let column_gap = match style_spec.column_gap {
            MathMatrixColumnGap::HalfEm => 0.5 * em,
            MathMatrixColumnGap::Em => em,
            MathMatrixColumnGap::Zero => 0.0,
        };
        let mut spec = TableSpec {
            aligns: vec![Align::Center; columns],
            vlines: Vec::new(),
            hlines: Vec::new(),
            column_gap,
            row_gap: 0.2 * em,
            row_spacing: None,
            base_row_ascent: 0.0,
            base_row_descent: 0.0,
            base_row_leading: 0.0,
        };
        if node.id.as_ref() == self.ctx.host_table_id.as_ref()
            && let Some(row_spacing) = self.ctx.host_row_spacing
        {
            spec.row_spacing = Some(row_spacing);
            spec.base_row_ascent = self.ctx.host_row_ascent;
            spec.base_row_descent = self.ctx.host_row_descent;
            spec.base_row_leading = self.ctx.host_row_leading;
        }
        // `gathered` 不是持久格式支持的样式，但公开布局入口仍可直接收到它；保留既有字号。
        let cell_style = if kind == "gathered" {
            style
        } else {
            match style_spec.cell_size {
                MathMatrixCellSize::Inherit => style,
                MathMatrixCellSize::Compact => style.with_size(style.size.min(MathSize::Text)),
            }
        };
        match style_spec.alignment {
            MathMatrixAlignment::Left => {
                spec.aligns = vec![Align::Left; columns];
            }
            MathMatrixAlignment::Alternating => {
                spec.aligns = (0..columns)
                    .map(|index| {
                        if index % 2 == 0 {
                            Align::Right
                        } else {
                            Align::Left
                        }
                    })
                    .collect();
            }
            MathMatrixAlignment::Columns => {
                let (aligns, vlines) = parse_columns(Self::attribute(node, "columns"));
                spec.aligns = aligns;
                spec.aligns
                    .resize(columns.max(spec.aligns.len()), Align::Center);
                spec.vlines = vlines;
            }
            MathMatrixAlignment::Center => {}
        }
        let mut rows = Vec::new();
        for child in &node.children {
            match child.kind {
                K::Row => {
                    let cells = child
                        .children
                        .iter()
                        .map(|cell| self.node(cell, cell_style))
                        .collect();
                    rows.push(TableRow::Cells {
                        id: child.id.clone(),
                        position: child.position.clone(),
                        cells,
                    });
                }
                K::Hline => spec.hlines.push(rows.len()),
                K::Intertext => rows.push(TableRow::Full(self.node(child, cell_style))),
                _ => rows.push(TableRow::Cells {
                    id: None,
                    position: child.position.clone(),
                    cells: vec![self.node(child, cell_style)],
                }),
            }
        }
        let table = layout_table(&mut self.ctx, cell_style, rows, &spec);
        if left.is_none() && right.is_none() {
            return table;
        }
        let target = table.height();
        let mut parts = Vec::with_capacity(3);
        if let Some(open) =
            left.and_then(|c| delimiter(&self.ctx, style, c, AtomClass::Open, target))
        {
            parts.push(open);
        }
        parts.push(table);
        if let Some(close) =
            right.and_then(|c| delimiter(&self.ctx, style, c, AtomClass::Close, target))
        {
            parts.push(close);
        }
        let mut fragment = layout_row(&self.ctx, style, parts);
        fragment.class = AtomClass::Inner;
        fragment
    }

    fn accent(&mut self, node: &MathNode, style: Style) -> Fragment {
        let accent = Self::attribute(node, "accent");
        let Some(accent_spec) = math_accent_by_value(accent) else {
            self.ctx.diagnostic("math.accent.unknown", accent);
            return self.child(node, 0, style);
        };
        match accent_spec.rendering {
            MathAccentRendering::Overline => {
                let body = self.child(node, 0, style.cramped());
                layout_line(&self.ctx, style, body, true)
            }
            MathAccentRendering::Underline => {
                let body = self.child(node, 0, style);
                layout_line(&self.ctx, style, body, false)
            }
            MathAccentRendering::CancelForward
            | MathAccentRendering::CancelBackward
            | MathAccentRendering::CancelCross => {
                let body = self.child(node, 0, style);
                let kind = match accent_spec.rendering {
                    MathAccentRendering::CancelBackward => CancelKind::Back,
                    MathAccentRendering::CancelCross => CancelKind::Cross,
                    _ => CancelKind::Forward,
                };
                layout_cancel(style, body, kind)
            }
            MathAccentRendering::Boxed => {
                let body = self.child(node, 0, style);
                boxed(body, None, Some(style.color), style)
            }
            MathAccentRendering::Negate => {
                let body = self.child(node, 0, style);
                self.negate(body, style)
            }
            MathAccentRendering::Mark {
                character,
                placement,
                brace,
            } => {
                let top = placement == MathAccentPlacement::Over;
                let body_style = if top { style.cramped() } else { style };
                let body = self.child(node, 0, body_style);
                let Some(mark) = glyph_fragment(&self.ctx, style, character, AtomClass::Ord) else {
                    self.ctx
                        .diagnostic("math.glyph.missing", format!("U+{:04X}", character as u32));
                    return body;
                };
                if brace {
                    return self.brace(body, mark, top, style);
                }
                layout_accent(&self.ctx, style, body, mark, top)
            }
        }
    }

    /// 横向花括号：伸展到内容宽度，与内容隔开 0.25em；结果之后的脚标按上下限放置。
    fn brace(&mut self, body: Fragment, mark: Fragment, top: bool, style: Style) -> Fragment {
        let gap = 0.25 * self.ctx.font_size(style);
        let mark = stretch(&self.ctx, mark, Axis::X, body.width(), 0.0);
        let width = body.width().max(mark.width());
        let class = body.class;
        let (body_x, mark_x) = ((width - body.width()) / 2.0, (width - mark.width()) / 2.0);
        let (body_height, mark_height) = (body.height(), mark.height());
        let height = body_height + gap + mark_height;
        let mut frame;
        if top {
            frame = Frame::new(width, height, mark.height() + gap + body.ascent());
            frame.push_frame(Point::new(mark_x, 0.0), mark.frame);
            frame.push_frame(Point::new(body_x, mark_height + gap), body.frame);
        } else {
            frame = Frame::new(width, height, body.ascent());
            frame.push_frame(Point::new(body_x, 0.0), body.frame);
            frame.push_frame(Point::new(mark_x, body_height + gap), mark.frame);
        }
        let mut fragment = Fragment::new(frame, class);
        fragment.limits = true;
        fragment.stretchy_x = true;
        fragment
    }

    /// `\not`：在内容上居中叠加长斜线。
    fn negate(&mut self, body: Fragment, style: Style) -> Fragment {
        let Some(slash) = glyph_fragment(&self.ctx, style, '⧸', AtomClass::Ord)
            .or_else(|| glyph_fragment(&self.ctx, style, '/', AtomClass::Ord))
        else {
            return body;
        };
        let class = body.class;
        let width = body.width().max(slash.width());
        let ascent = body.ascent().max(slash.ascent());
        let descent = body.descent().max(slash.descent());
        let mut frame = Frame::new(width, ascent + descent, ascent);
        let body_pos = Point::new((width - body.width()) / 2.0, ascent - body.ascent());
        let slash_pos = Point::new((width - slash.width()) / 2.0, ascent - slash.ascent());
        frame.push_frame(body_pos, body.frame);
        frame.push_frame(slash_pos, slash.frame);
        Fragment::new(frame, class)
    }

    fn space(&mut self, node: &MathNode, style: Style) -> Fragment {
        let em = self.ctx.font_size(style);
        let width = match Self::attribute(node, "spaceKind") {
            "thin" => 3.0 / 18.0,
            "medium" => 4.0 / 18.0,
            "thick" => 5.0 / 18.0,
            "negative_thin" => -3.0 / 18.0,
            "quad" => 1.0,
            "qquad" => 2.0,
            "hspace" => {
                let read = |key: &str, default: f64| {
                    node.attributes
                        .get(key)
                        .and_then(|value| value.parse::<f64>().ok())
                        .unwrap_or(default)
                };
                let denominator = read("valueDenominator", 1.0);
                if super::is_zero(denominator) {
                    0.0
                } else {
                    read("valueNumerator", 0.0) / denominator
                }
            }
            _ => 0.25,
        };
        Fragment::spacing(width * em)
    }

    // ---- 文本与字母表 ----

    fn text(&mut self, node: &MathNode, style: Style) -> Fragment {
        let kind = Self::attribute(node, "style");
        let has_text_items = node.children.iter().any(has_text_items);
        let text_mode = kind == "text" || has_text_items;
        let variant = match kind {
            "rm" | "text" => Variant::Roman,
            "bf" => Variant::Bold,
            "it" => Variant::Italic,
            "sf" => Variant::SansSerif,
            "tt" => Variant::Monospace,
            "bb" => Variant::DoubleStruck,
            "cal" | "scr" => Variant::Script,
            "frak" => Variant::Fraktur,
            "boldsymbol" | "pmb" => Variant::BoldItalic,
            _ => style.variant,
        };
        let inner = Style {
            variant,
            text_mode,
            ..style
        };
        if !text_mode {
            let mut fragment = self.row(&node.children, inner);
            if node.children.len() > 1 {
                fragment.class = AtomClass::Ord;
            }
            return fragment;
        }
        if self.text_shaper.is_some() {
            let mut text_style = MathTextRunStyle::default();
            apply_text_style(
                &mut text_style,
                kind,
                node.attributes.get("fontFamily").map(String::as_str),
            );
            let fragments = self.text_segments(&node.children, inner, text_style);
            let mut fragment = self.joined(fragments, AtomClass::Ord, inner);
            fragment.text_like = true;
            return fragment;
        }
        let fragments: Vec<Fragment> = node
            .children
            .iter()
            .map(|child| self.text_item(child, inner))
            .collect();
        let mut fragment = self.joined(fragments, AtomClass::Ord, inner);
        fragment.text_like = true;
        fragment
    }

    fn text_variant(base: Variant, emphasis: &str) -> Variant {
        let (bold, italic, sans_serif, monospace) = Self::emphasis_state(base);
        let (bold, italic) = match emphasis {
            "bf" => (true, italic),
            "it" => (bold, true),
            "md" => (false, italic),
            "up" => (bold, false),
            _ => return base,
        };
        Self::compose_emphasis(bold, italic, sans_serif, monospace)
    }

    fn text_family_variant(base: Variant, family: Variant) -> Variant {
        let (bold, italic, _, _) = Self::emphasis_state(base);
        match (family, bold, italic) {
            (Variant::SansSerif, true, true) => Variant::SansSerifBoldItalic,
            (Variant::SansSerif, true, false) => Variant::SansSerifBold,
            (Variant::SansSerif, false, true) => Variant::SansSerifItalic,
            (Variant::SansSerif, false, false) => Variant::SansSerif,
            // 数学等宽字母表没有粗斜体变体；此降级路径优先保留粗斜语义。
            (Variant::Monospace, true, true) => Variant::MonospaceBoldItalic,
            (Variant::Monospace, true, false) => Variant::MonospaceBold,
            (Variant::Monospace, false, true) => Variant::MonospaceItalic,
            (Variant::Monospace, false, false) => Variant::Monospace,
            _ => base,
        }
    }

    fn emphasis_state(variant: Variant) -> (bool, bool, bool, bool) {
        match variant {
            Variant::Bold => (true, false, false, false),
            Variant::Italic => (false, true, false, false),
            Variant::BoldItalic => (true, true, false, false),
            Variant::SansSerif => (false, false, true, false),
            Variant::SansSerifBold => (true, false, true, false),
            Variant::SansSerifItalic => (false, true, true, false),
            Variant::SansSerifBoldItalic => (true, true, true, false),
            Variant::Monospace => (false, false, false, true),
            Variant::MonospaceBold => (true, false, false, true),
            Variant::MonospaceItalic => (false, true, false, true),
            Variant::MonospaceBoldItalic => (true, true, false, true),
            _ => (false, false, false, false),
        }
    }

    fn compose_emphasis(bold: bool, italic: bool, sans_serif: bool, monospace: bool) -> Variant {
        if monospace {
            return match (bold, italic) {
                (true, true) => Variant::MonospaceBoldItalic,
                (true, false) => Variant::MonospaceBold,
                (false, true) => Variant::MonospaceItalic,
                (false, false) => Variant::Monospace,
            };
        }
        match (sans_serif, bold, italic) {
            (true, true, true) => Variant::SansSerifBoldItalic,
            (true, true, false) => Variant::SansSerifBold,
            (true, false, true) => Variant::SansSerifItalic,
            (true, false, false) => Variant::SansSerif,
            (false, true, true) => Variant::BoldItalic,
            (false, true, false) => Variant::Bold,
            (false, false, true) => Variant::Italic,
            (false, false, false) => Variant::Roman,
        }
    }

    /// 文本模式中的一项；数学子项恢复数学模式。
    fn text_item(&mut self, node: &MathNode, style: Style) -> Fragment {
        let em = self.ctx.font_size(style);
        let mut fragment = match node.kind {
            K::TextContent | K::TextSymbol | K::Opaque => {
                let text = if node.kind == K::Opaque {
                    "�"
                } else {
                    node.run
                        .as_ref()
                        .map(|run| run.text.as_str())
                        .or(node.text.as_deref())
                        .unwrap_or_default()
                };
                let fragments = text
                    .chars()
                    .map(|character| {
                        if character == ' ' {
                            Fragment::new(Frame::new(0.25 * em, 0.0, 0.0), AtomClass::Ord)
                        } else {
                            self.character(character, AtomClass::Ord, style)
                        }
                    })
                    .collect();
                self.joined(fragments, AtomClass::Ord, style)
            }
            K::TextSpace => Fragment::new(Frame::new(0.25 * em, 0.0, 0.0), AtomClass::Ord),
            K::SoftBreak => {
                // 数学字形回退路径无法排多行；要求调用方提供正文文本塑形器。
                self.text_shaping_error = Some(super::MathTextShapeError::UnsupportedInput);
                Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord)
            }
            K::TextStyle => {
                let variant = match Self::attribute(node, "style") {
                    "bf" | "it" | "md" | "up" => {
                        Self::text_variant(style.variant, Self::attribute(node, "style"))
                    }
                    "bi" => Variant::BoldItalic,
                    "sf" => Self::text_family_variant(style.variant, Variant::SansSerif),
                    "tt" => Self::text_family_variant(style.variant, Variant::Monospace),
                    "font" => style.variant,
                    _ => Variant::Roman,
                };
                let inner = Style { variant, ..style };
                let fragments = node
                    .children
                    .iter()
                    .map(|child| self.text_item(child, inner))
                    .collect();
                self.joined(fragments, AtomClass::Ord, inner)
            }
            _ => {
                let math = Style {
                    text_mode: false,
                    variant: Variant::Normal,
                    ..style
                };
                return self.node(node, math);
            }
        };
        let record = Item::Node {
            id: node.id.clone(),
            position: self.styled_position(&node.position, style),
            width: fragment.frame.width,
            height: fragment.frame.height,
            baseline: fragment.frame.baseline,
        };
        fragment
            .frame
            .items
            .insert(0, (Point::new(0.0, 0.0), record));
        fragment
    }

    fn color_box(&mut self, node: &MathNode, style: Style) -> Fragment {
        let (fill, border) = if node.kind == K::ColorBox {
            (MathColor::parse_hex(Self::attribute(node, "color")), None)
        } else {
            (
                MathColor::parse_hex(Self::attribute(node, "fillColor")),
                Some(MathColor::parse_hex(Self::attribute(node, "borderColor"))),
            )
        };
        let body = match node.children.first() {
            Some(child) if child.kind == K::Text => {
                let text_style = Style {
                    text_mode: true,
                    variant: Variant::Roman,
                    ..style
                };
                if self.text_shaper.is_some() {
                    let fragments = self.text_segments(
                        &child.children,
                        text_style,
                        MathTextRunStyle::default(),
                    );
                    self.joined(fragments, AtomClass::Ord, text_style)
                } else {
                    let fragments = child
                        .children
                        .iter()
                        .map(|item| self.text_item(item, text_style))
                        .collect();
                    self.joined(fragments, AtomClass::Ord, text_style)
                }
            }
            Some(child) => self.node(child, style),
            None => Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord),
        };
        boxed(body, fill, border, style)
    }
}

fn is_placeholder_flow(node: &MathNode) -> bool {
    node.kind == K::Row
        && node
            .position
            .as_ref()
            .is_some_and(|position| position.is_flow)
        && matches!(node.children.as_slice(), [only] if only.kind == K::Placeholder)
}

pub(crate) fn is_empty_host_flow(node: &MathNode) -> bool {
    node.position.as_ref().is_some_and(|position| {
        position.is_flow
            && position.path.is_empty()
            && (node.children.is_empty()
                || matches!(node.children.as_slice(), [only] if only.kind == K::Placeholder))
    })
}

fn has_text_items(node: &MathNode) -> bool {
    matches!(
        node.kind,
        K::TextContent | K::TextSpace | K::TextSymbol | K::TextStyle | K::SoftBreak | K::Opaque
    ) || (node.kind == K::Row && node.children.iter().any(has_text_items))
}

fn apply_text_style(style: &mut MathTextRunStyle, value: &str, font_family: Option<&str>) {
    match value {
        "bf" => style.bold = Some(true),
        "md" => style.bold = Some(false),
        "it" => style.italic = Some(true),
        "up" => style.italic = Some(false),
        "bi" => {
            style.bold = Some(true);
            style.italic = Some(true);
        }
        "rm" => {
            style.bold = Some(false);
            style.italic = Some(false);
            style.font_family = Some("serif".to_owned());
        }
        "sf" => style.font_family = Some("sans-serif".to_owned()),
        "tt" => style.font_family = Some("monospace".to_owned()),
        _ => {}
    }
    if let Some(font_family) = font_family {
        style.font_family = Some(font_family.to_owned());
    }
}

/// 将文档模型的共享环境样式增量应用到数学 TextFlow 的宿主样式。
fn apply_inline_environment_style(
    style: &mut MathTextRunStyle,
    environment_style: &caret_doc::InlineTextStyle,
) {
    if let Some(bold) = environment_style.bold {
        style.bold = Some(bold);
    }
    if let Some(italic) = environment_style.italic {
        style.italic = Some(italic);
    }
    if let Some(font_family) = &environment_style.font_family {
        style.font_family = Some(font_family.clone());
    }
}

/// 加内边距并画背景/边框。`border` 为 `Some` 时描边，其内层为描边颜色。
fn boxed(
    body: Fragment,
    fill: Option<MathColor>,
    border: Option<Option<MathColor>>,
    _style: Style,
) -> Fragment {
    let pad = BOX_PADDING;
    let width = body.width() + 2.0 * pad;
    let height = body.height() + 2.0 * pad;
    let mut frame = Frame::new(width, height, body.ascent() + pad);
    frame.push(
        Point::new(0.0, 0.0),
        Item::Rect {
            width,
            height,
            fill,
            stroke: border.map(|color| (color, BOX_STROKE)),
        },
    );
    frame.push_frame(Point::new(pad, pad), body.frame);
    Fragment::new(frame, AtomClass::Ord)
}

/// 解析 `array` 列格式：`l`/`c`/`r` 为列，`|` 为竖线。
fn parse_columns(spec: &str) -> (Vec<Align>, Vec<usize>) {
    let mut aligns = Vec::new();
    let mut vlines = Vec::new();
    for character in spec.chars() {
        match character {
            'l' => aligns.push(Align::Left),
            'c' => aligns.push(Align::Center),
            'r' => aligns.push(Align::Right),
            '|' => vlines.push(aligns.len()),
            _ => {}
        }
    }
    (aligns, vlines)
}

#[cfg(test)]
mod tests {
    use super::{Converter, Variant};

    #[test]
    fn nested_text_emphasis_preserves_font_family_and_boldness() {
        assert_eq!(
            Converter::text_family_variant(Variant::Bold, Variant::SansSerif),
            Variant::SansSerifBold
        );
        assert_eq!(
            Converter::text_variant(Variant::SansSerifBold, "it"),
            Variant::SansSerifBoldItalic
        );
        assert_eq!(
            Converter::text_family_variant(Variant::SansSerifBold, Variant::SansSerif),
            Variant::SansSerifBold
        );
        assert_eq!(
            Converter::text_variant(Variant::SansSerifBoldItalic, "md"),
            Variant::SansSerifItalic
        );
        assert_eq!(
            Converter::text_variant(Variant::Monospace, "md"),
            Variant::Monospace
        );
        assert_eq!(
            Converter::text_variant(Variant::Monospace, "bf"),
            Variant::MonospaceBold
        );
        assert_eq!(
            Converter::text_variant(Converter::text_variant(Variant::Monospace, "bf"), "md"),
            Variant::Monospace
        );
    }
}
