//! 定义文档树、节点 DTO、字段校验与语义节点序列化。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

mod block;
mod codec;
mod features;
mod inline;
mod inline_environment;
mod math;
mod paragraph;
mod run;
mod validation;

pub use validation::{MAX_DOCUMENT_NESTING_DEPTH, is_valid_language_tag};

pub use block::{Block, BlockEnvironment, BlockEnvironmentKind, ExplicitBreak};
pub use inline::{
    FlowContent, MathContent, ParagraphContent, SoftBreak, TextFlow, TextFlowSourceMapEntry,
};
pub use inline_environment::{InlineEnvironment, InlineEnvironmentKind};
pub use math::{
    Align, Equation, InlineMath, MATH_TREE_SCHEMA_VERSION, MAX_MATH_SOURCE_BYTES,
    MAX_MATH_TREE_DEPTH, MAX_MATH_TREE_NODES, MAX_MATH_TREE_PAYLOAD_BYTES, MathAlignRow, MathFlow,
    MathNode, MathNodeKind, MathSourceRange, MathTree,
};
pub use paragraph::{Paragraph, ParagraphLayoutOverrides};
pub use run::Run;

/// 文档字体设置的统一默认标记；具体字体由字体角色解析。
pub const DEFAULT_FONT_FAMILY: &str = "<default>";

// 保留节点转换函数在 crate 内原有的访问路径。
#[allow(unused_imports)]
pub(crate) use codec::{
    block_from_value, block_to_value, paragraph_content_from_value, paragraph_content_to_value,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// 文档中稳定且唯一的节点标识。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// 验证并构造文件中读取的 ID。
    pub fn parse(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
            return Err(ModelError::InvalidNodeId(value));
        }
        Ok(Self(value))
    }

    /// 为新建节点生成进程内唯一 ID。
    pub fn generate() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        Self(format!(
            "n-{}-{timestamp:x}-{sequence:x}",
            std::process::id()
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for NodeId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    InvalidNodeId(String),
    InvalidDocument(String),
    DuplicateNodeId(String),
    InvalidLanguageTag(String),
    ForbiddenDirectFormat { node: String, property: String },
    InvalidEnvironmentScope(String),
    NestingLimit,
}

impl Display for ModelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidNodeId(id) => write!(formatter, "document.invalid_node_id: {id:?}"),
            Self::InvalidDocument(reason) => write!(formatter, "document.invalid: {reason}"),
            Self::DuplicateNodeId(id) => write!(formatter, "document.duplicate_node_id: {id}"),
            Self::InvalidLanguageTag(tag) => {
                write!(formatter, "document.invalid_language_tag: {tag}")
            }
            Self::ForbiddenDirectFormat { node, property } => {
                write!(
                    formatter,
                    "document.forbidden_direct_format: {node}.{property}"
                )
            }
            Self::InvalidEnvironmentScope(kind) => {
                write!(formatter, "document.invalid_environment_scope: {kind}")
            }
            Self::NestingLimit => write!(formatter, "document.nesting_limit_exceeded"),
        }
    }
}

impl std::error::Error for ModelError {}

/// 文档内保存的语义树。修订代际、选择和布局不属于此 DTO。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    #[serde(default)]
    pub global_settings: DocumentSettings,
    #[serde(default)]
    pub body: Vec<Block>,
    pub page_sections: Vec<PageSection>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Document {
    pub fn blank() -> Self {
        let paragraph = Paragraph::empty(NodeId::generate());
        Self {
            global_settings: DocumentSettings::default(),
            body: vec![Block::Paragraph(paragraph)],
            page_sections: vec![PageSection::default_with_id(NodeId::generate())],
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DocumentSettings {
    pub default_language: Option<String>,
    /// 默认拉丁字体族，也是未配置脚本的文档级最终回退。
    pub default_font_family: Option<String>,
    /// CJK 文字体系共用默认值；脚本和语言覆盖可继续收紧范围。
    pub default_cjk_font_family: Option<String>,
    /// 公式上下文使用的字体族；`<default>` 解析为内置数学字体。
    pub default_math_font_family: Option<String>,
    /// 命令输入与代码块使用的字体族；`<default>` 解析为内置等宽字体。
    pub default_code_font_family: String,
    /// 全局正文默认字号，单位为百分之一 pt。
    #[serde(default = "default_font_size_centipoints")]
    pub default_font_size_centipoints: u16,
    /// Unicode Script 短标签（常见值与 ISO 15924 短码一致）到字体族的可选覆盖。
    pub script_font_families: BTreeMap<String, String>,
    /// 实际脚本范围内按 BCP 47 语言选择的字体族覆盖。
    pub language_script_font_families: BTreeMap<String, BTreeMap<String, String>>,
    /// 新文档与全局段落格式默认值；旧文件缺省时读取内置排版主题。
    pub paragraph_layout: ParagraphLayoutStyle,
    pub default_styles: BTreeMap<String, Value>,
    pub format_overrides: BTreeMap<String, Value>,
    pub theme: Option<ThemeSnapshot>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 文档正文默认字体的可配置目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentFontTarget {
    Script(String),
    ScriptLanguage { script: String, language: String },
}

impl DocumentFontTarget {
    /// 验证并规范化字体目标，使大小写不同的等价键映射到同一身份。
    pub fn normalized(&self) -> Result<Self, ModelError> {
        fn normalize_script(script: &str) -> Result<String, ModelError> {
            if script.len() != 4 || !script.bytes().all(|byte| byte.is_ascii_alphabetic()) {
                return Err(ModelError::InvalidDocument(format!(
                    "invalid Unicode Script key: {script}"
                )));
            }
            let mut normalized = script.to_ascii_lowercase();
            normalized[..1].make_ascii_uppercase();
            Ok(normalized)
        }

        match self {
            Self::Script(script) => Ok(Self::Script(normalize_script(script)?)),
            Self::ScriptLanguage { script, language } => {
                let script = normalize_script(script)?;
                if !validation::is_valid_language_tag(language) {
                    return Err(ModelError::InvalidLanguageTag(language.clone()));
                }
                Ok(Self::ScriptLanguage {
                    script,
                    language: language.to_ascii_lowercase(),
                })
            }
        }
    }
}

impl Default for DocumentSettings {
    fn default() -> Self {
        Self {
            default_language: None,
            default_font_family: None,
            default_cjk_font_family: None,
            default_math_font_family: Some(DEFAULT_FONT_FAMILY.to_owned()),
            default_code_font_family: DEFAULT_FONT_FAMILY.to_owned(),
            default_font_size_centipoints: default_font_size_centipoints(),
            script_font_families: BTreeMap::new(),
            language_script_font_families: BTreeMap::new(),
            paragraph_layout: ParagraphLayoutStyle::default(),
            default_styles: BTreeMap::new(),
            format_overrides: BTreeMap::new(),
            theme: None,
            extra: BTreeMap::new(),
        }
    }
}

impl DocumentSettings {
    pub fn font_family(&self, target: &DocumentFontTarget) -> Option<&str> {
        match target.normalized().ok()? {
            DocumentFontTarget::Script(script) if script == "Latn" => self
                .script_font_families
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(&script))
                .map(|(_, family)| family.as_str())
                .or(self.default_font_family.as_deref()),
            DocumentFontTarget::Script(script) => self
                .script_font_families
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(&script))
                .map(|(_, family)| family.as_str()),
            DocumentFontTarget::ScriptLanguage { script, language } => self
                .language_script_font_families
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(&script))
                .and_then(|(_, languages)| {
                    languages
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case(&language))
                        .map(|(_, family)| family.as_str())
                }),
        }
    }

    pub fn set_font_family(
        &mut self,
        target: &DocumentFontTarget,
        family: Option<String>,
    ) -> Result<(), ModelError> {
        let target = target.normalized()?;
        match &target {
            DocumentFontTarget::Script(script) if script == "Latn" => {
                self.script_font_families
                    .retain(|key, _| !key.eq_ignore_ascii_case(script));
                self.default_font_family = family;
            }
            DocumentFontTarget::Script(script) => {
                self.script_font_families
                    .retain(|key, _| !key.eq_ignore_ascii_case(script));
                if let Some(family) = family {
                    self.script_font_families.insert(script.clone(), family);
                }
            }
            DocumentFontTarget::ScriptLanguage { script, language } => {
                let mut languages = self
                    .language_script_font_families
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(script))
                    .map(|(key, _)| key.clone())
                    .and_then(|key| self.language_script_font_families.remove(&key))
                    .unwrap_or_default();
                languages.retain(|key, _| !key.eq_ignore_ascii_case(language));
                if let Some(family) = family {
                    languages.insert(language.clone(), family);
                }
                if !languages.is_empty() {
                    self.language_script_font_families
                        .insert(script.clone(), languages);
                }
            }
        }
        Ok(())
    }
}

/// `.cdoc` 持久化的行距类型。长度单位为百分之一 pt。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ParagraphLineSpacing {
    Natural {
        #[serde(default, flatten)]
        extra: BTreeMap<String, Value>,
    },
    Multiple {
        percent: u16,
        #[serde(default, flatten)]
        extra: BTreeMap<String, Value>,
    },
    AtLeast {
        min_centipoints: u32,
        #[serde(default, flatten)]
        extra: BTreeMap<String, Value>,
    },
    Exactly {
        centipoints: u32,
        #[serde(default, flatten)]
        extra: BTreeMap<String, Value>,
    },
}

/// 可全局配置的段落版式值，直接属性与命名样式可在此基础上覆盖。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParagraphLayoutStyle {
    pub line_spacing: ParagraphLineSpacing,
    pub space_before_centipoints: u32,
    pub space_after_centipoints: u32,
    pub widow_orphan_control: bool,
    pub keep_with_next: bool,
    pub keep_lines_together: bool,
    pub page_break_before: bool,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

static DEFAULT_PARAGRAPH_LAYOUT: OnceLock<ParagraphLayoutStyle> = OnceLock::new();
static DEFAULT_FONT_SIZE_CENTIPOINTS: OnceLock<u16> = OnceLock::new();

/// 返回默认排版主题中的正文默认字号，单位为百分之一 pt。
pub fn default_font_size_centipoints() -> u16 {
    *DEFAULT_FONT_SIZE_CENTIPOINTS.get_or_init(|| {
        let profile: BundledLayoutTheme =
            serde_json::from_str(include_str!("../resources/layout-themes/default.json"))
                .unwrap_or_else(|error| panic!("bundled default layout theme is invalid: {error}"));
        assert_eq!(profile.profile_id, "default");
        assert_eq!(profile.format_version, 1);
        assert!(profile.text_style.font_size_centipoints > 0);
        profile.text_style.font_size_centipoints
    })
}

impl Default for ParagraphLayoutStyle {
    fn default() -> Self {
        DEFAULT_PARAGRAPH_LAYOUT
            .get_or_init(|| {
                let profile: BundledLayoutTheme =
                    serde_json::from_str(include_str!("../resources/layout-themes/default.json"))
                        .unwrap_or_else(|error| {
                            panic!("bundled default layout theme is invalid: {error}")
                        });
                assert_eq!(profile.profile_id, "default");
                assert_eq!(profile.format_version, 1);
                profile.paragraph_layout
            })
            .clone()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundledLayoutTheme {
    profile_id: String,
    format_version: u32,
    text_style: BundledDefaultTextStyle,
    paragraph_layout: ParagraphLayoutStyle,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundledDefaultTextStyle {
    font_size_centipoints: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSnapshot {
    pub format_version: u32,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub named_styles: BTreeMap<String, Value>,
    #[serde(default)]
    pub environment_templates: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSection {
    pub id: NodeId,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl PageSection {
    pub fn default_with_id(id: NodeId) -> Self {
        Self {
            id,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpaqueNode {
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    pub id: String,
    pub media_type: String,
    pub data: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests;
