//! 定义行内格式环境的节点类型与稳定文档标识。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::NodeId;
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

impl InlineEnvironmentKind {
    /// 返回 `.cdoc` 中使用的稳定 kind 名称。
    pub const fn storage_name(self) -> &'static str {
        match self {
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Font => "font",
        }
    }

    /// 从 `.cdoc` kind 名称解析内置行内环境。
    pub fn from_storage_name(value: &str) -> Option<Self> {
        match value {
            "bold" => Some(Self::Bold),
            "italic" => Some(Self::Italic),
            "font" => Some(Self::Font),
            _ => None,
        }
    }

    /// 返回该环境 kind 声明的必需能力标识。
    pub const fn required_feature(self) -> &'static str {
        match self {
            Self::Bold => "caret.env.bold",
            Self::Italic => "caret.env.italic",
            Self::Font => "caret.env.font",
        }
    }

    /// Bold 与 Italic 支持用 `enabled` 显式关闭继承格式。
    pub const fn supports_enabled(self) -> bool {
        matches!(self, Self::Bold | Self::Italic)
    }

    /// Font 环境以 `fontFamily` 指定局部字体族。
    pub const fn supports_font_family(self) -> bool {
        matches!(self, Self::Font)
    }
}

/// 对子节点范围应用的行内格式环境。
#[derive(Debug, Clone, PartialEq)]
pub struct InlineEnvironment<Child> {
    pub id: NodeId,
    pub kind: InlineEnvironmentKind,
    pub definition_id: Option<String>,
    /// 公式文本中的源码范围；正文 InlineEnvironment 不使用此字段。
    pub source_range: Option<super::MathSourceRange>,
    pub props: BTreeMap<String, Value>,
    pub children: Vec<Child>,
    pub extra: BTreeMap<String, Value>,
}
