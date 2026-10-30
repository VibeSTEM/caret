//! 定义与解析后端无关的持久化数学树。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::NodeId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 当前 Caret 数学树 schema 版本。
pub const MATH_TREE_SCHEMA_VERSION: u16 = 1;
/// `.cdoc` 中单个公式源码的最大 UTF-8 字节数。
pub const MAX_MATH_SOURCE_BYTES: usize = 1024 * 1024;
/// 数学树允许的最大根到叶深度。
pub const MAX_MATH_TREE_DEPTH: usize = 32;

/// Caret 自有、版本化且不依赖解析器 AST 的数学表达式。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MathTree {
    pub schema_version: u16,
    pub source: String,
    pub root: MathNode,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 有序数学语义节点。`text` 保存节点自身的原子内容，`children` 保存操作数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MathNode {
    pub id: NodeId,
    pub kind: MathNodeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<MathSourceRange>,
    #[serde(default)]
    pub children: Vec<MathNode>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 可编辑数学树的节点类别；有序子节点保留 TeX 原子和符号次序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MathNodeKind {
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
    Accent,
    Space,
    Style,
    Group,
    Placeholder,
}

/// 相对 `MathTree::source` 的半开 UTF-8 字节范围。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MathSourceRange {
    pub start: u32,
    pub end: u32,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 行内数学对象，作为段落内联节点保存。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineMath {
    pub id: NodeId,
    pub math_tree: MathTree,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 单行显示公式，作为文档块保存。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Equation {
    pub id: NodeId,
    pub math_tree: MathTree,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 多行对齐公式。每一行由有序的、可独立编辑的公式单元格组成。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Align {
    pub id: NodeId,
    /// 完整输入源，包含被解析成行/单元格分隔的原始分隔符。
    pub source: String,
    pub rows: Vec<MathAlignRow>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 对齐公式的一行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MathAlignRow {
    pub id: NodeId,
    pub cells: Vec<MathTree>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
