//! 定义行内内容节点、格式环境和软换行节点。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{InlineEnvironment, OpaqueNode, Run};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

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
