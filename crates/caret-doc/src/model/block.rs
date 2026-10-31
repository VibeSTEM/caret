//! 定义文档级块序列、块环境与显式分页节点。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{Align, Equation, NodeId, OpaqueNode, Paragraph};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 文档或块环境正文中的块级节点。
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph(Paragraph),
    Equation(Equation),
    Align(Align),
    Environment(BlockEnvironment),
    ExplicitBreak(ExplicitBreak),
    Opaque(OpaqueNode),
}

/// 内置块环境类型；Section 有章节语义，Custom 是可重命名的通用容器。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockEnvironmentKind {
    Section,
    Custom,
}

/// 可嵌套的块环境；标题独立于其有序正文子项。
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEnvironment {
    pub id: NodeId,
    pub kind: BlockEnvironmentKind,
    pub definition_id: Option<String>,
    pub props: BTreeMap<String, Value>,
    pub title: Option<Paragraph>,
    pub children: Vec<Block>,
    pub extra: BTreeMap<String, Value>,
}

/// 段落流中的显式分页节点。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExplicitBreak {
    #[serde(default)]
    pub kind: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
