//! `MathFlow` 到布局与 LaTeX 后端共享的临时节点视图。
//!
//! 该视图只在单次后端调用期间存在，不参与文档存储、编辑或兼容解码。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::collections::BTreeMap;

use caret_doc::{FlowContent, InlineEnvironmentKind, MathContent, MathFlow, NodeId, Run, TextFlow};

use BackendNodeKind as MathNodeKind;

/// 后端私有节点类别。文本解析标记只存在于这个临时表示中，不进入文档 schema。
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
            B::TextContent | B::TextSpace | B::TextSymbol | B::TextStyle => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub(crate) id: NodeId,
    pub(crate) kind: BackendNodeKind,
    pub(crate) text: Option<String>,
    pub(crate) attributes: BTreeMap<String, String>,
    pub(crate) children: Vec<Node>,
}

impl Node {
    fn leaf(id: NodeId, kind: BackendNodeKind, text: Option<String>) -> Self {
        Self {
            id,
            kind,
            text,
            attributes: BTreeMap::new(),
            children: Vec::new(),
        }
    }
}

pub(crate) fn flow_nodes(flow: &MathFlow) -> Vec<Node> {
    contents_nodes(&flow.content)
}

pub(crate) fn contents_nodes(contents: &[MathContent]) -> Vec<Node> {
    contents.iter().flat_map(content_nodes).collect()
}

fn content_nodes(content: &MathContent) -> Vec<Node> {
    match content {
        FlowContent::Text(text) => text_nodes(text),
        FlowContent::Math(node) => vec![math_node(node)],
        FlowContent::Environment(environment) => {
            let children = contents_nodes(&environment.children);
            let style = match environment.kind {
                InlineEnvironmentKind::Bold => Some("bf"),
                InlineEnvironmentKind::Italic => Some("it"),
                InlineEnvironmentKind::Font => match environment
                    .props
                    .get("fontFamily")
                    .and_then(serde_json::Value::as_str)
                {
                    Some("sans-serif") => Some("sf"),
                    Some("monospace") => Some("tt"),
                    _ => Some("rm"),
                },
            };
            vec![Node {
                id: environment.id.clone(),
                kind: MathNodeKind::TextStyle,
                text: None,
                attributes: BTreeMap::from([(
                    "style".to_owned(),
                    style.unwrap_or("rm").to_owned(),
                )]),
                children,
            }]
        }
        FlowContent::SoftBreak(_) => vec![Node::leaf(
            NodeId::generate(),
            MathNodeKind::TextSpace,
            Some("space".into()),
        )],
        FlowContent::Opaque(_) => vec![Node::leaf(
            NodeId::generate(),
            MathNodeKind::TextContent,
            Some("�".into()),
        )],
    }
}

fn text_nodes(flow: &TextFlow) -> Vec<Node> {
    let mut output = Vec::new();
    for run in &flow.runs {
        append_run(&mut output, run, NodeId::generate());
    }
    output
}

fn append_run(output: &mut Vec<Node>, run: &Run, id: NodeId) {
    let text = Node::leaf(
        id.clone(),
        MathNodeKind::TextContent,
        Some(run.text.clone()),
    );
    let mut style = None;
    if run.props.get("bold").and_then(serde_json::Value::as_bool) == Some(true) {
        style = Some("bf");
    }
    if run.props.get("italic").and_then(serde_json::Value::as_bool) == Some(true) {
        style = Some("it");
    }
    output.push(if let Some(style) = style {
        Node {
            id,
            kind: MathNodeKind::TextStyle,
            text: None,
            attributes: BTreeMap::from([("style".into(), style.into())]),
            children: vec![text],
        }
    } else {
        text
    });
}

fn math_node(node: &caret_doc::MathNode) -> Node {
    Node {
        id: node.id.clone(),
        kind: node.kind.into(),
        text: node.text.clone(),
        attributes: node.attributes.clone(),
        children: contents_nodes_from_flows(&node.children),
    }
}

fn contents_nodes_from_flows(flows: &[MathFlow]) -> Vec<Node> {
    flows
        .iter()
        .flat_map(|flow| match flow.content.as_slice() {
            [MathContent::Math(node)]
                if matches!(
                    node.kind,
                    caret_doc::MathNodeKind::Hline
                        | caret_doc::MathNodeKind::Intertext
                        | caret_doc::MathNodeKind::Placeholder
                ) =>
            {
                vec![math_node(node)]
            }
            _ => vec![Node {
                id: flow.id.clone(),
                kind: MathNodeKind::Row,
                text: None,
                attributes: BTreeMap::new(),
                children: contents_nodes(&flow.content),
            }],
        })
        .collect()
}
