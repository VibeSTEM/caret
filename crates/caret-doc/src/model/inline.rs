//! 定义行内内容节点、格式环境和软换行节点。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{InlineEnvironment, InlineMath, MathNode, MathSourceRange, OpaqueNode, Run};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 同一文本流片段中的有序 Run；数学文本与正文文本使用同一载体。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFlow {
    #[serde(default)]
    pub runs: Vec<Run>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<MathSourceRange>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 正文与数学共用的强类型内容流节点。
#[derive(Debug, Clone, PartialEq)]
pub enum FlowContent<T> {
    Text(TextFlow),
    Math(T),
    Environment(InlineEnvironment<FlowContent<T>>),
    SoftBreak(SoftBreak),
    Opaque(OpaqueNode),
}

/// 段落正文的内容项。
pub type ParagraphContent = FlowContent<InlineMath>;

/// 数学树中与文本流、数学节点及格式环境交替排列的内容项。
pub type MathContent = FlowContent<MathNode>;

/// 段落内显式换行；不同于新建 Paragraph 的 Enter 操作。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SoftBreak {
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
