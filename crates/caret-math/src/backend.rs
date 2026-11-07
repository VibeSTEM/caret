//! `MathFlow` 到布局与 LaTeX 后端共享的临时节点视图。
//!
//! 该视图只在单次后端调用期间存在，不参与文档存储、编辑或兼容解码。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::collections::BTreeMap;

use caret_doc::{
    FlowContent, InlineEnvironmentKind, MathContent, MathFlow, MathPathStep, NodeId, Run, TextFlow,
};

use BackendNodeKind as MathNodeKind;

/// 后端私有节点类别。文本解析与 MathFlow 边界标记只存在于临时表示中，不进入文档 schema。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackendNodeKind {
    Row,
    Symbol,
    Text,
    Operator,
    Fraction,
    Radical,
    Superscript,
    Subscript,
    SubSup,
    Delimited,
    SizedDelimiter,
    Matrix,
    Substack,
    Intertext,
    Hline,
    Sum,
    Integral,
    Product,
    Limit,
    OverUnder,
    CancelTo,
    Accent,
    Space,
    TextContent,
    TextSpace,
    TextSymbol,
    TextStyle,
    SoftBreak,
    Opaque,
    MathAlphabet,
    Color,
    TextColor,
    ColorBox,
    FColorBox,
    Strut,
    Phantom,
    Style,
    Group,
    Placeholder,
}

impl From<caret_doc::MathNodeKind> for BackendNodeKind {
    fn from(kind: caret_doc::MathNodeKind) -> Self {
        use BackendNodeKind as B;
        use caret_doc::MathNodeKind as M;
        match kind {
            M::Row => B::Row,
            M::Symbol => B::Symbol,
            M::Text => B::Text,
            M::Operator => B::Operator,
            M::Fraction => B::Fraction,
            M::Radical => B::Radical,
            M::Superscript => B::Superscript,
            M::Subscript => B::Subscript,
            M::SubSup => B::SubSup,
            M::Delimited => B::Delimited,
            M::SizedDelimiter => B::SizedDelimiter,
            M::Matrix => B::Matrix,
            M::Substack => B::Substack,
            M::Intertext => B::Intertext,
            M::Hline => B::Hline,
            M::Sum => B::Sum,
            M::Integral => B::Integral,
            M::Product => B::Product,
            M::Limit => B::Limit,
            M::OverUnder => B::OverUnder,
            M::CancelTo => B::CancelTo,
            M::Accent => B::Accent,
            M::Space => B::Space,
            M::Color => B::Color,
            M::TextColor => B::TextColor,
            M::ColorBox => B::ColorBox,
            M::FColorBox => B::FColorBox,
            M::Strut => B::Strut,
            M::Phantom => B::Phantom,
            M::Style => B::Style,
            M::Group => B::Group,
            M::Placeholder => B::Placeholder,
        }
    }
}

impl BackendNodeKind {
    pub(crate) fn optional_placeholder_operands(self) -> &'static [usize] {
        self.persisted()
            .map_or(&[], caret_doc::MathNodeKind::optional_placeholder_operands)
    }

    pub(crate) fn persisted(self) -> Option<caret_doc::MathNodeKind> {
        use BackendNodeKind as B;
        use caret_doc::MathNodeKind as M;
        Some(match self {
            B::Row => M::Row,
            B::Symbol => M::Symbol,
            B::Text => M::Text,
            B::Operator => M::Operator,
            B::Fraction => M::Fraction,
            B::Radical => M::Radical,
            B::Superscript => M::Superscript,
            B::Subscript => M::Subscript,
            B::SubSup => M::SubSup,
            B::Delimited => M::Delimited,
            B::SizedDelimiter => M::SizedDelimiter,
            B::Matrix => M::Matrix,
            B::Substack => M::Substack,
            B::Intertext => M::Intertext,
            B::Hline => M::Hline,
            B::Sum => M::Sum,
            B::Integral => M::Integral,
            B::Product => M::Product,
            B::Limit => M::Limit,
            B::OverUnder => M::OverUnder,
            B::CancelTo => M::CancelTo,
            B::Accent => M::Accent,
            B::Space => M::Space,
            B::Color => M::Color,
            B::TextColor => M::TextColor,
            B::ColorBox => M::ColorBox,
            B::FColorBox => M::FColorBox,
            B::Strut => M::Strut,
            B::Phantom => M::Phantom,
            B::Style => M::Style,
            B::Group => M::Group,
            B::Placeholder => M::Placeholder,
            B::MathAlphabet => M::Text,
            B::TextContent
            | B::TextSpace
            | B::TextSymbol
            | B::TextStyle
            | B::SoftBreak
            | B::Opaque => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NodePosition {
    pub(crate) scope_id: NodeId,
    pub(crate) path: Vec<MathPathStep>,
    pub(crate) text_run_index: Option<usize>,
    pub(crate) is_text_flow: bool,
    pub(crate) is_flow: bool,
    pub(crate) is_container: bool,
    /// 当前数学样式的字号；序列化投影不携带布局字号。
    pub(crate) font_size_pt: Option<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub(crate) id: Option<NodeId>,
    /// LaTeX 序列化不消费布局身份，因此只在布局投影时设置。
    pub(crate) position: Option<NodePosition>,
    pub(crate) kind: BackendNodeKind,
    pub(crate) text: Option<String>,
    /// 布局投影保留完整 Run，以供共享正文 shaping 使用；序列化投影不克隆它。
    pub(crate) run: Option<Run>,
    pub(crate) attributes: BTreeMap<String, String>,
    pub(crate) children: Vec<Node>,
}

impl Node {
    fn leaf(
        id: Option<NodeId>,
        position: Option<NodePosition>,
        kind: BackendNodeKind,
        text: Option<String>,
    ) -> Self {
        Self {
            id,
            position,
            kind,
            text,
            run: None,
            attributes: BTreeMap::new(),
            children: Vec::new(),
        }
    }
}

pub(crate) fn flow_nodes(flow: &MathFlow) -> Vec<Node> {
    contents_nodes_in_scope(&flow.content, None, &[])
}

pub(crate) fn flow_nodes_for_layout(flow: &MathFlow) -> Vec<Node> {
    let position = Some(NodePosition {
        scope_id: flow.id.clone(),
        path: Vec::new(),
        text_run_index: None,
        is_text_flow: false,
        is_flow: true,
        is_container: false,
        font_size_pt: None,
    });
    let children = if flow.content.is_empty() {
        Vec::new()
    } else {
        contents_nodes_in_scope(&flow.content, Some(&flow.id), &[])
    };
    vec![Node {
        id: Some(flow.id.clone()),
        position,
        kind: MathNodeKind::Row,
        text: None,
        run: None,
        attributes: BTreeMap::new(),
        children,
    }]
}

fn contents_nodes_in_scope(
    contents: &[MathContent],
    scope_id: Option<&NodeId>,
    parent_path: &[MathPathStep],
) -> Vec<Node> {
    contents
        .iter()
        .enumerate()
        .flat_map(|(index, content)| {
            let path = if scope_id.is_some() {
                let mut path = parent_path.to_vec();
                path.push(MathPathStep::Content(index));
                path
            } else {
                Vec::new()
            };
            content_nodes(content, scope_id, path)
        })
        .collect()
}

fn content_nodes(
    content: &MathContent,
    scope_id: Option<&NodeId>,
    path: Vec<MathPathStep>,
) -> Vec<Node> {
    match content {
        FlowContent::Text(text) => text_nodes(text, scope_id, path),
        FlowContent::Math(node) => vec![math_node(node, scope_id, path)],
        FlowContent::Environment(environment) => {
            let children = environment
                .children
                .iter()
                .enumerate()
                .flat_map(|(index, child)| {
                    let child_path = if scope_id.is_some() {
                        let mut child_path = path.clone();
                        child_path.push(MathPathStep::EnvironmentChild(index));
                        child_path
                    } else {
                        Vec::new()
                    };
                    content_nodes(child, scope_id, child_path)
                })
                .collect();
            let style = match environment.kind {
                InlineEnvironmentKind::Bold => Some(
                    if environment
                        .props
                        .get("enabled")
                        .and_then(serde_json::Value::as_bool)
                        == Some(false)
                    {
                        "md"
                    } else {
                        "bf"
                    },
                ),
                InlineEnvironmentKind::Italic => Some(
                    if environment
                        .props
                        .get("enabled")
                        .and_then(serde_json::Value::as_bool)
                        == Some(false)
                    {
                        "up"
                    } else {
                        "it"
                    },
                ),
                InlineEnvironmentKind::Font => match environment
                    .props
                    .get("fontFamily")
                    .and_then(serde_json::Value::as_str)
                {
                    Some("sans-serif") => Some("sf"),
                    Some("monospace") => Some("tt"),
                    _ if scope_id.is_some() => Some("font"),
                    _ => Some("rm"),
                },
            };
            let mut attributes =
                BTreeMap::from([("style".to_owned(), style.unwrap_or("rm").to_owned())]);
            if environment.kind == InlineEnvironmentKind::Font
                && let Some(family) = environment
                    .props
                    .get("fontFamily")
                    .and_then(serde_json::Value::as_str)
            {
                attributes.insert("fontFamily".to_owned(), family.to_owned());
            }
            vec![Node {
                id: Some(environment.id.clone()),
                position: scope_id.map(|scope_id| NodePosition {
                    scope_id: scope_id.clone(),
                    path,
                    text_run_index: None,
                    is_text_flow: false,
                    is_flow: false,
                    is_container: true,
                    font_size_pt: None,
                }),
                kind: MathNodeKind::TextStyle,
                text: None,
                run: None,
                attributes,
                children,
            }]
        }
        FlowContent::SoftBreak(_) => vec![Node::leaf(
            None,
            position(scope_id, path, None, false, false),
            MathNodeKind::SoftBreak,
            None,
        )],
        FlowContent::Opaque(_) => vec![Node::leaf(
            None,
            position(scope_id, path, None, false, false),
            MathNodeKind::Opaque,
            None,
        )],
    }
}

fn text_nodes(flow: &TextFlow, scope_id: Option<&NodeId>, path: Vec<MathPathStep>) -> Vec<Node> {
    let mut output = Vec::new();
    if flow.runs.is_empty() {
        let Some(scope_id) = scope_id else {
            return output;
        };
        output.push(Node::leaf(
            None,
            position(Some(scope_id), path, None, true, false),
            MathNodeKind::TextContent,
            Some(String::new()),
        ));
        return output;
    }
    for (run_index, run) in flow.runs.iter().enumerate() {
        append_run(&mut output, run, scope_id, path.clone(), run_index);
    }
    output
}

fn append_run(
    output: &mut Vec<Node>,
    run: &Run,
    scope_id: Option<&NodeId>,
    path: Vec<MathPathStep>,
    run_index: usize,
) {
    let mut node = Node::leaf(
        None,
        position(scope_id, path, Some(run_index), true, false),
        MathNodeKind::TextContent,
        Some(run.text.clone()),
    );
    if scope_id.is_some() {
        node.run = Some(run.clone());
    }
    output.push(node);
}

fn position(
    scope_id: Option<&NodeId>,
    path: Vec<MathPathStep>,
    text_run_index: Option<usize>,
    is_text_flow: bool,
    is_flow: bool,
) -> Option<NodePosition> {
    scope_id.map(|scope_id| NodePosition {
        scope_id: scope_id.clone(),
        path,
        text_run_index,
        is_text_flow,
        is_flow,
        is_container: false,
        font_size_pt: None,
    })
}

fn math_node(
    node: &caret_doc::MathNode,
    scope_id: Option<&NodeId>,
    path: Vec<MathPathStep>,
) -> Node {
    Node {
        id: Some(node.id.clone()),
        position: position(scope_id, path.clone(), None, false, false).map(|mut position| {
            position.is_container = matches!(
                node.kind,
                caret_doc::MathNodeKind::Row | caret_doc::MathNodeKind::Text
            );
            position
        }),
        kind: node.kind.into(),
        text: node.text.clone(),
        run: None,
        attributes: node.attributes.clone(),
        children: contents_nodes_from_flows(&node.children, scope_id, &path),
    }
}

fn contents_nodes_from_flows(
    flows: &[MathFlow],
    scope_id: Option<&NodeId>,
    parent_path: &[MathPathStep],
) -> Vec<Node> {
    flows
        .iter()
        .enumerate()
        .flat_map(|(index, flow)| {
            let operand_path = if scope_id.is_some() {
                let mut path = parent_path.to_vec();
                path.push(MathPathStep::Operand(index));
                path
            } else {
                Vec::new()
            };
            match flow.content.as_slice() {
                [MathContent::Math(node)]
                    if matches!(
                        node.kind,
                        caret_doc::MathNodeKind::Hline | caret_doc::MathNodeKind::Intertext
                    ) || (scope_id.is_none()
                        && node.kind == caret_doc::MathNodeKind::Placeholder) =>
                {
                    let path = if scope_id.is_some() {
                        let mut path = operand_path;
                        path.push(MathPathStep::Content(0));
                        path
                    } else {
                        Vec::new()
                    };
                    vec![math_node(node, scope_id, path)]
                }
                _ => vec![Node {
                    id: Some(flow.id.clone()),
                    position: position(scope_id, operand_path.clone(), None, false, true),
                    kind: MathNodeKind::Row,
                    text: None,
                    run: None,
                    attributes: BTreeMap::new(),
                    children: contents_nodes_in_scope(&flow.content, scope_id, &operand_path),
                }],
            }
        })
        .collect()
}
