//! 定义行内内容节点、格式环境和软换行节点。

use super::{NodeId, OpaqueNode, Run};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 支持的内置行内环境类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InlineEnvironmentKind {
    Bold,
    Italic,
    Font,
}

/// 对子节点范围应用的格式环境；环境内的直接 Run 属性仍可局部覆盖。
#[derive(Debug, Clone, PartialEq)]
pub struct InlineEnvironment {
    pub id: NodeId,
    pub kind: InlineEnvironmentKind,
    pub definition_id: Option<String>,
    pub props: BTreeMap<String, Value>,
    pub children: Vec<InlineNode>,
    pub extra: BTreeMap<String, Value>,
}

/// Paragraph 内按顺序排列的文本、格式环境和显式软换行。
#[derive(Debug, Clone, PartialEq)]
pub enum InlineNode {
    Run(Run),
    Environment(InlineEnvironment),
    SoftBreak(SoftBreak),
    Opaque(OpaqueNode),
}

/// 段落内显式换行；不同于新建 Paragraph 的 Enter 操作。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SoftBreak {
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
