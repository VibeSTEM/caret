//! 定义行内格式环境的节点类型与稳定文档标识。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{NodeId, ThemeSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 支持的内置行内环境类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

/// 解析后的行内格式模板；同一组数据供正文、数学布局和编辑状态复用。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlineEnvironmentTemplates {
    defaults: BTreeMap<InlineEnvironmentKind, InlineTextStyle>,
    variants: BTreeMap<(InlineEnvironmentKind, String), InlineTextStyle>,
}

/// 文档主题中的行内模板无效或被实例引用但不存在。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineEnvironmentTemplateError(pub String);

impl std::fmt::Display for InlineEnvironmentTemplateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for InlineEnvironmentTemplateError {}

impl InlineEnvironmentTemplates {
    /// 从主题快照解析受支持的 Bold、Italic、Font 默认模板和命名变体。
    pub fn from_theme(
        theme: Option<&ThemeSnapshot>,
    ) -> Result<Self, InlineEnvironmentTemplateError> {
        let Some(theme) = theme else {
            return Ok(Self::default());
        };
        let mut result = Self::default();
        for (raw_kind, template_value) in &theme.default_inline_environment_templates {
            let kind = InlineEnvironmentKind::from_storage_name(raw_kind).ok_or_else(|| {
                InlineEnvironmentTemplateError(format!(
                    "unknown default inline environment kind: {raw_kind}"
                ))
            })?;
            let template_id = template_value.as_str().ok_or_else(|| {
                InlineEnvironmentTemplateError(format!(
                    "default inline environment template reference must be a string: {raw_kind}"
                ))
            })?;
            let value = theme
                .environment_templates
                .get(template_id)
                .ok_or_else(|| {
                    InlineEnvironmentTemplateError(format!(
                        "missing environment template: {template_id}"
                    ))
                })?;
            let template = value.as_object().ok_or_else(|| {
                InlineEnvironmentTemplateError(format!(
                    "environment template must be an object: {template_id}"
                ))
            })?;
            if template
                .keys()
                .any(|key| !matches!(key.as_str(), "baseKind" | "props" | "variants"))
            {
                return Err(InlineEnvironmentTemplateError(format!(
                    "unsupported environment template field: {template_id}"
                )));
            }
            let base_kind = template
                .get("baseKind")
                .and_then(Value::as_str)
                .and_then(InlineEnvironmentKind::from_storage_name)
                .ok_or_else(|| {
                    InlineEnvironmentTemplateError(format!(
                        "environment template has invalid baseKind: {template_id}"
                    ))
                })?;
            if base_kind != kind {
                return Err(InlineEnvironmentTemplateError(format!(
                    "environment template {template_id} has baseKind {}, expected {}",
                    base_kind.storage_name(),
                    kind.storage_name()
                )));
            }
            let props = parse_template_props(kind, template.get("props"), template_id)?;
            result.defaults.insert(kind, props);
            if let Some(variants) = template.get("variants") {
                let variants = variants.as_object().ok_or_else(|| {
                    InlineEnvironmentTemplateError(format!(
                        "environment template variants must be an object: {template_id}"
                    ))
                })?;
                for (variant_id, variant) in variants {
                    if variant_id.trim().is_empty() {
                        return Err(InlineEnvironmentTemplateError(format!(
                            "environment template has an empty variant ID: {template_id}"
                        )));
                    }
                    let style = parse_template_props(kind, Some(variant), variant_id)?;
                    if result
                        .variants
                        .insert((kind, variant_id.clone()), style)
                        .is_some()
                    {
                        return Err(InlineEnvironmentTemplateError(format!(
                            "duplicate inline environment variant ID: {variant_id}"
                        )));
                    }
                }
            }
        }
        Ok(result)
    }

    /// 当前模板是否包含任何可作用于文档环境的默认样式或变体。
    pub fn is_empty(&self) -> bool {
        self.defaults.is_empty() && self.variants.is_empty()
    }

    pub fn resolve<Child>(
        &self,
        environment: &InlineEnvironment<Child>,
    ) -> Result<InlineTextStyle, InlineEnvironmentTemplateError> {
        let mut style = self
            .defaults
            .get(&environment.kind)
            .cloned()
            .unwrap_or_default();
        if let Some(variant_id) = environment.definition_id.as_deref() {
            let variant = self
                .variants
                .get(&(environment.kind, variant_id.to_owned()))
                .ok_or_else(|| {
                    InlineEnvironmentTemplateError(format!(
                        "missing {} environment variant: {variant_id}",
                        environment.kind.storage_name()
                    ))
                })?;
            merge_text_style(&mut style, variant);
        }
        let instance = environment.instance_text_style();
        merge_text_style(&mut style, &instance);
        if environment.kind.supports_enabled() {
            let value = match environment.kind {
                InlineEnvironmentKind::Bold => &mut style.bold,
                InlineEnvironmentKind::Italic => &mut style.italic,
                InlineEnvironmentKind::Font => unreachable!(),
            };
            if value.is_none() {
                *value = Some(true);
            }
        }
        Ok(style)
    }
}

fn merge_text_style(target: &mut InlineTextStyle, source: &InlineTextStyle) {
    if source.bold.is_some() {
        target.bold = source.bold;
    }
    if source.italic.is_some() {
        target.italic = source.italic;
    }
    if source.font_family.is_some() {
        target.font_family.clone_from(&source.font_family);
    }
}

fn parse_template_props(
    kind: InlineEnvironmentKind,
    value: Option<&Value>,
    template_id: &str,
) -> Result<InlineTextStyle, InlineEnvironmentTemplateError> {
    let Some(value) = value else {
        return Ok(InlineTextStyle::default());
    };
    let props = value.as_object().ok_or_else(|| {
        InlineEnvironmentTemplateError(format!(
            "environment template props must be an object: {template_id}"
        ))
    })?;
    let mut style = InlineTextStyle::default();
    for (name, value) in props {
        match (kind, name.as_str()) {
            (InlineEnvironmentKind::Bold, "enabled") => {
                style.bold = Some(value.as_bool().ok_or_else(|| {
                    InlineEnvironmentTemplateError(format!(
                        "Bold template enabled must be boolean: {template_id}"
                    ))
                })?);
            }
            (InlineEnvironmentKind::Italic, "enabled") => {
                style.italic = Some(value.as_bool().ok_or_else(|| {
                    InlineEnvironmentTemplateError(format!(
                        "Italic template enabled must be boolean: {template_id}"
                    ))
                })?);
            }
            (InlineEnvironmentKind::Font, "fontFamily") => {
                let family = value
                    .as_str()
                    .filter(|family| !family.trim().is_empty())
                    .ok_or_else(|| {
                        InlineEnvironmentTemplateError(format!(
                            "Font template fontFamily must be a non-empty string: {template_id}"
                        ))
                    })?;
                style.font_family = Some(family.to_owned());
            }
            _ => {
                return Err(InlineEnvironmentTemplateError(format!(
                    "unsupported {} template property {name}: {template_id}",
                    kind.storage_name()
                )));
            }
        }
    }
    Ok(style)
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
        let mut style = self.instance_text_style();
        if self.kind.supports_enabled() {
            match self.kind {
                InlineEnvironmentKind::Bold if style.bold.is_none() => style.bold = Some(true),
                InlineEnvironmentKind::Italic if style.italic.is_none() => {
                    style.italic = Some(true);
                }
                InlineEnvironmentKind::Bold
                | InlineEnvironmentKind::Italic
                | InlineEnvironmentKind::Font => {}
            }
        }
        style
    }

    /// 解析主题模板、变体和实例属性的有效样式增量。
    pub fn resolved_text_style(
        &self,
        templates: &InlineEnvironmentTemplates,
    ) -> Result<InlineTextStyle, InlineEnvironmentTemplateError> {
        templates.resolve(self)
    }

    fn instance_text_style(&self) -> InlineTextStyle {
        match self.kind {
            InlineEnvironmentKind::Bold => InlineTextStyle {
                bold: self.props.get("enabled").and_then(Value::as_bool),
                ..InlineTextStyle::default()
            },
            InlineEnvironmentKind::Italic => InlineTextStyle {
                italic: self.props.get("enabled").and_then(Value::as_bool),
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
    use crate::{FlowContent, InlineMath, MathNode};

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

    #[test]
    fn inline_style_resolution_merges_template_variant_and_instance() {
        let theme = ThemeSnapshot {
            format_version: 1,
            source_id: None,
            named_styles: BTreeMap::new(),
            environment_templates: BTreeMap::from([
                (
                    "body-bold".to_owned(),
                    serde_json::json!({
                        "baseKind": "bold",
                        "props": {"enabled": false},
                        "variants": {
                            "body-bold/emphasis": {"enabled": true},
                            "body-bold/quiet": {"enabled": false}
                        }
                    }),
                ),
                (
                    "body-font".to_owned(),
                    serde_json::json!({
                        "baseKind": "font",
                        "props": {"fontFamily": "Theme Sans"},
                        "variants": {"body-font/alternate": {"fontFamily": "Theme Serif"}}
                    }),
                ),
            ]),
            default_inline_environment_templates: BTreeMap::from([
                ("bold".to_owned(), serde_json::json!("body-bold")),
                ("font".to_owned(), serde_json::json!("body-font")),
            ]),
            extra: BTreeMap::new(),
        };
        let templates = InlineEnvironmentTemplates::from_theme(Some(&theme))
            .unwrap_or_else(|error| panic!("valid inline templates should parse: {error}"));

        let bold = environment(InlineEnvironmentKind::Bold, BTreeMap::new());
        assert_eq!(
            bold.resolved_text_style(&templates),
            Ok(InlineTextStyle {
                bold: Some(false),
                ..InlineTextStyle::default()
            })
        );

        let variant = InlineEnvironment {
            definition_id: Some("body-bold/emphasis".to_owned()),
            ..environment(InlineEnvironmentKind::Bold, BTreeMap::new())
        };
        assert_eq!(
            variant.resolved_text_style(&templates),
            Ok(InlineTextStyle {
                bold: Some(true),
                ..InlineTextStyle::default()
            })
        );

        let instance_override = InlineEnvironment {
            definition_id: Some("body-bold/emphasis".to_owned()),
            props: BTreeMap::from([("enabled".to_owned(), Value::Bool(false))]),
            ..environment(InlineEnvironmentKind::Bold, BTreeMap::new())
        };
        assert_eq!(
            instance_override.resolved_text_style(&templates),
            Ok(InlineTextStyle {
                bold: Some(false),
                ..InlineTextStyle::default()
            })
        );

        let font_variant = InlineEnvironment {
            definition_id: Some("body-font/alternate".to_owned()),
            ..environment(InlineEnvironmentKind::Font, BTreeMap::new())
        };
        assert_eq!(
            font_variant.resolved_text_style(&templates),
            Ok(InlineTextStyle {
                font_family: Some("Theme Serif".to_owned()),
                ..InlineTextStyle::default()
            })
        );
    }

    #[test]
    fn inline_templates_resolve_body_and_math_hosts_with_independent_generic_children() {
        assert!(InlineEnvironmentTemplates::default().is_empty());
        let theme = ThemeSnapshot {
            format_version: 1,
            source_id: None,
            named_styles: BTreeMap::new(),
            environment_templates: BTreeMap::from([(
                "body-bold".to_owned(),
                serde_json::json!({
                    "baseKind": "bold",
                    "props": {"enabled": false},
                    "variants": {"body-bold/strong": {"enabled": true}}
                }),
            )]),
            default_inline_environment_templates: BTreeMap::from([(
                "bold".to_owned(),
                serde_json::json!("body-bold"),
            )]),
            extra: BTreeMap::new(),
        };
        let templates = InlineEnvironmentTemplates::from_theme(Some(&theme))
            .unwrap_or_else(|error| panic!("valid inline templates should parse: {error}"));
        assert!(!templates.is_empty());

        let body_environment: InlineEnvironment<FlowContent<MathNode>> = InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: Some("body-bold/strong".to_owned()),
            props: BTreeMap::from([("enabled".to_owned(), Value::Bool(false))]),
            children: Vec::new(),
            extra: BTreeMap::new(),
        };
        let math_environment: InlineEnvironment<FlowContent<InlineMath>> = InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: Some("body-bold/strong".to_owned()),
            props: BTreeMap::new(),
            children: Vec::new(),
            extra: BTreeMap::new(),
        };

        assert_eq!(
            body_environment.resolved_text_style(&templates),
            Ok(InlineTextStyle {
                bold: Some(false),
                ..InlineTextStyle::default()
            })
        );
        assert_eq!(
            math_environment.resolved_text_style(&templates),
            Ok(InlineTextStyle {
                bold: Some(true),
                ..InlineTextStyle::default()
            })
        );
    }

    #[test]
    fn inline_style_resolution_rejects_missing_or_mismatched_templates() {
        let theme = ThemeSnapshot {
            format_version: 1,
            source_id: None,
            named_styles: BTreeMap::new(),
            environment_templates: BTreeMap::from([(
                "body-italic".to_owned(),
                serde_json::json!({"baseKind": "italic", "props": {"enabled": false}}),
            )]),
            default_inline_environment_templates: BTreeMap::from([(
                "bold".to_owned(),
                serde_json::json!("body-italic"),
            )]),
            extra: BTreeMap::new(),
        };
        assert!(InlineEnvironmentTemplates::from_theme(Some(&theme)).is_err());

        let object_reference = ThemeSnapshot {
            format_version: 1,
            source_id: None,
            named_styles: BTreeMap::new(),
            environment_templates: BTreeMap::from([(
                "body-bold".to_owned(),
                serde_json::json!({"baseKind": "bold", "props": {"enabled": true}}),
            )]),
            default_inline_environment_templates: BTreeMap::from([(
                "bold".to_owned(),
                serde_json::json!({"template": "body-bold"}),
            )]),
            extra: BTreeMap::new(),
        };
        assert!(InlineEnvironmentTemplates::from_theme(Some(&object_reference)).is_err());

        let templates = InlineEnvironmentTemplates::default();
        let environment = InlineEnvironment {
            definition_id: Some("missing/variant".to_owned()),
            ..environment(InlineEnvironmentKind::Bold, BTreeMap::new())
        };
        assert!(environment.resolved_text_style(&templates).is_err());
    }

    #[test]
    fn inline_style_resolution_rejects_unsupported_template_and_variant_fields() {
        for template in [
            serde_json::json!({
                "baseKind": "bold",
                "props": {"enabled": false},
                "futureRendering": true
            }),
            serde_json::json!({
                "baseKind": "bold",
                "props": {"enabled": false},
                "variants": {"strong": {"props": {"enabled": true}, "enabled": false}}
            }),
        ] {
            let theme = ThemeSnapshot {
                format_version: 1,
                source_id: None,
                named_styles: BTreeMap::new(),
                environment_templates: BTreeMap::from([("bold".to_owned(), template)]),
                default_inline_environment_templates: BTreeMap::from([(
                    "bold".to_owned(),
                    serde_json::json!("bold"),
                )]),
                extra: BTreeMap::new(),
            };
            assert!(InlineEnvironmentTemplates::from_theme(Some(&theme)).is_err());
        }
    }

    #[test]
    fn inline_template_parser_rejects_malformed_kinds_values_and_variants() {
        fn theme(kind: &str, template: Value, mapping: Value) -> ThemeSnapshot {
            ThemeSnapshot {
                format_version: 1,
                source_id: None,
                named_styles: BTreeMap::new(),
                environment_templates: BTreeMap::from([("template".to_owned(), template)]),
                default_inline_environment_templates: BTreeMap::from([(kind.to_owned(), mapping)]),
                extra: BTreeMap::new(),
            }
        }

        assert!(
            InlineEnvironmentTemplates::from_theme(None)
                .is_ok_and(|templates| templates.is_empty())
        );
        let cases = [
            theme("unknown", Value::Null, Value::String("template".to_owned())),
            theme(
                "bold",
                serde_json::json!({"baseKind": "bold"}),
                serde_json::json!({"template": "template"}),
            ),
            theme("bold", Value::Null, Value::String("missing".to_owned())),
            theme(
                "bold",
                Value::String("template".to_owned()),
                Value::String("template".to_owned()),
            ),
            theme(
                "bold",
                serde_json::json!({"baseKind": "future"}),
                Value::String("template".to_owned()),
            ),
            theme(
                "bold",
                serde_json::json!({"baseKind": "italic"}),
                Value::String("template".to_owned()),
            ),
            theme(
                "bold",
                serde_json::json!({"baseKind": "bold", "props": "enabled"}),
                Value::String("template".to_owned()),
            ),
            theme(
                "bold",
                serde_json::json!({"baseKind": "bold", "props": {"enabled": "yes"}}),
                Value::String("template".to_owned()),
            ),
            theme(
                "italic",
                serde_json::json!({"baseKind": "italic", "props": {"enabled": 1}}),
                Value::String("template".to_owned()),
            ),
            theme(
                "font",
                serde_json::json!({"baseKind": "font", "props": {"fontFamily": "  "}}),
                Value::String("template".to_owned()),
            ),
            theme(
                "font",
                serde_json::json!({"baseKind": "font", "props": {"enabled": true}}),
                Value::String("template".to_owned()),
            ),
            theme(
                "bold",
                serde_json::json!({"baseKind": "bold", "variants": []}),
                Value::String("template".to_owned()),
            ),
            theme(
                "bold",
                serde_json::json!({"baseKind": "bold", "variants": {" ": {}}}),
                Value::String("template".to_owned()),
            ),
        ];
        for theme in cases {
            assert!(InlineEnvironmentTemplates::from_theme(Some(&theme)).is_err());
        }
    }
}
