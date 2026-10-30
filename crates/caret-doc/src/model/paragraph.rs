//! 定义段落、Run 与直接段落格式覆盖的数据结构。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{InlineNode, NodeId, ParagraphLineSpacing};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 段落文本及其 Run/行内环境序列。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paragraph {
    pub id: NodeId,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub layout: Option<ParagraphLayoutOverrides>,
    #[serde(default)]
    pub inlines: Vec<InlineNode>,
    #[serde(default)]
    pub mark_style: BTreeMap<String, Value>,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Paragraph {
    pub fn empty(id: NodeId) -> Self {
        Self {
            id,
            language: None,
            layout: None,
            inlines: Vec::new(),
            mark_style: BTreeMap::new(),
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

/// 段落级可选直接格式覆盖；未设置的属性继承文档全局段落样式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ParagraphLayoutOverrides {
    pub line_spacing: Option<ParagraphLineSpacing>,
    pub first_line_indent_chars: Option<u16>,
    pub space_before_centipoints: Option<u32>,
    pub space_after_centipoints: Option<u32>,
    pub widow_orphan_control: Option<bool>,
    pub keep_with_next: Option<bool>,
    pub keep_lines_together: Option<bool>,
    pub page_break_before: Option<bool>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
