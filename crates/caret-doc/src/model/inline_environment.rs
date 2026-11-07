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

/// 由内置行内环境提供的样式增量，供正文与数学 TextFlow 合并到继承样式中。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlineTextStyle {
    /// 粗体请求；`Some` 覆盖继承值，`None` 保持继承值，具体字体面与合成由布局层决定。
    pub bold: Option<bool>,
    /// 斜体请求；`Some` 覆盖继承值，`None` 保持继承值，具体字体面与合成由布局层决定。
    pub italic: Option<bool>,
    /// 请求使用的字体族；`Some` 替换继承值，`None` 保持继承值。
    pub font_family: Option<String>,
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

impl<Child> InlineEnvironment<Child> {
    /// 解析环境实例属性，返回可合并到当前文本样式的增量。
    ///
    /// 本方法不解析主题模板和变体。Bold/Italic 的 `enabled` 缺失时默认为开启；Font 的
    /// `fontFamily` 缺失、不是字符串或为空白时不覆盖继承字体。字体是否可用、真/伪粗斜体
    /// 及 `<default>` 回退由布局层处理，本模型不查询字体目录。
    pub fn text_style(&self) -> InlineTextStyle {
        match self.kind {
            InlineEnvironmentKind::Bold => InlineTextStyle {
                bold: Some(
                    self.props
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                ),
                ..InlineTextStyle::default()
            },
            InlineEnvironmentKind::Italic => InlineTextStyle {
                italic: Some(
                    self.props
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                ),
                ..InlineTextStyle::default()
            },
            InlineEnvironmentKind::Font => InlineTextStyle {
                font_family: self
                    .props
                    .get("fontFamily")
                    .and_then(Value::as_str)
                    .filter(|family| !family.trim().is_empty())
                    .map(str::to_owned),
                ..InlineTextStyle::default()
            },
        }
    }
}

/// 对子节点范围应用的行内格式环境。
#[derive(Debug, Clone, PartialEq)]
pub struct InlineEnvironment<Child> {
    pub id: NodeId,
    pub kind: InlineEnvironmentKind,
    pub definition_id: Option<String>,
    pub props: BTreeMap<String, Value>,
    pub children: Vec<Child>,
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(
        kind: InlineEnvironmentKind,
        props: BTreeMap<String, Value>,
    ) -> InlineEnvironment<()> {
        InlineEnvironment {
            id: NodeId::generate(),
            kind,
            definition_id: None,
            props,
            children: Vec::new(),
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn inline_environment_styles_share_enabled_and_font_family_defaults() {
        assert_eq!(
            environment(InlineEnvironmentKind::Bold, BTreeMap::new()).text_style(),
            InlineTextStyle {
                bold: Some(true),
                ..InlineTextStyle::default()
            }
        );
        assert_eq!(
            environment(
                InlineEnvironmentKind::Italic,
                BTreeMap::from([("enabled".to_owned(), Value::Bool(false))]),
            )
            .text_style(),
            InlineTextStyle {
                italic: Some(false),
                ..InlineTextStyle::default()
            }
        );
        assert_eq!(
            environment(
                InlineEnvironmentKind::Font,
                BTreeMap::from([(
                    "fontFamily".to_owned(),
                    Value::String("sans-serif".to_owned()),
                )]),
            )
            .text_style(),
            InlineTextStyle {
                font_family: Some("sans-serif".to_owned()),
                ..InlineTextStyle::default()
            }
        );
        assert_eq!(
            environment(
                InlineEnvironmentKind::Font,
                BTreeMap::from([("fontFamily".to_owned(), Value::String("  ".to_owned()),)]),
            )
            .text_style(),
            InlineTextStyle::default()
        );
    }
}
