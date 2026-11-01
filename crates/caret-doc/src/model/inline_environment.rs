//! 定义行内格式环境的节点类型与稳定文档标识。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{InlineNode, NodeId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 支持的内置行内环境类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InlineEnvironmentKind {
    Bold,
    Italic,
    Underline,
    Font,
}

impl InlineEnvironmentKind {
    /// 返回 `.cdoc` 中使用的稳定 kind 名称。
    pub const fn storage_name(self) -> &'static str {
        match self {
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Underline => "underline",
            Self::Font => "font",
        }
    }

    /// 从 `.cdoc` kind 名称解析内置行内环境。
    pub fn from_storage_name(value: &str) -> Option<Self> {
        match value {
            "bold" => Some(Self::Bold),
            "italic" => Some(Self::Italic),
            "underline" => Some(Self::Underline),
            "font" => Some(Self::Font),
            _ => None,
        }
    }

    /// 返回该环境 kind 声明的必需能力标识。
    pub const fn required_feature(self) -> &'static str {
        match self {
            Self::Bold => "caret.env.bold",
            Self::Italic => "caret.env.italic",
            Self::Underline => "caret.env.underline",
            Self::Font => "caret.env.font",
        }
    }

    /// Bold、Italic 与 Underline 支持用 `enabled` 显式关闭继承格式。
    pub const fn supports_enabled(self) -> bool {
        matches!(self, Self::Bold | Self::Italic | Self::Underline)
    }

    /// Font 环境以 `fontFamily` 指定局部字体族。
    pub const fn supports_font_family(self) -> bool {
        matches!(self, Self::Font)
    }
}

/// 对子节点范围应用的行内格式环境。
#[derive(Debug, Clone, PartialEq)]
pub struct InlineEnvironment {
    pub id: NodeId,
    pub kind: InlineEnvironmentKind,
    pub definition_id: Option<String>,
    pub props: BTreeMap<String, Value>,
    pub children: Vec<InlineNode>,
    pub extra: BTreeMap<String, Value>,
}

/// 把 Run 上的布尔 `underline` 属性读入为 Underline 行内环境。
///
/// `true` 的相邻 Run 合并为一个环境；`false` 直接去掉（不继承即无下划线）。
/// 非布尔取值属于扩展格式，原样保留，由特性检查决定只读预览。
pub(crate) fn migrate_run_underline(nodes: Vec<InlineNode>) -> Vec<InlineNode> {
    let mut output = Vec::with_capacity(nodes.len());
    let mut group: Vec<InlineNode> = Vec::new();
    let flush = |group: &mut Vec<InlineNode>, output: &mut Vec<InlineNode>| {
        if !group.is_empty() {
            output.push(InlineNode::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Underline,
                definition_id: None,
                props: BTreeMap::new(),
                children: std::mem::take(group),
                extra: BTreeMap::new(),
            }));
        }
    };
    for node in nodes {
        match node {
            InlineNode::Run(mut run) => match run.props.get("underline") {
                Some(Value::Bool(true)) => {
                    run.props.remove("underline");
                    group.push(InlineNode::Run(run));
                }
                Some(Value::Bool(false)) => {
                    run.props.remove("underline");
                    flush(&mut group, &mut output);
                    output.push(InlineNode::Run(run));
                }
                _ => {
                    flush(&mut group, &mut output);
                    output.push(InlineNode::Run(run));
                }
            },
            InlineNode::Environment(mut environment) => {
                flush(&mut group, &mut output);
                environment.children = migrate_run_underline(environment.children);
                output.push(InlineNode::Environment(environment));
            }
            other => {
                flush(&mut group, &mut output);
                output.push(other);
            }
        }
    }
    flush(&mut group, &mut output);
    output
}
