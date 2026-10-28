//! 定义文档树、节点 DTO、字段校验与语义节点序列化。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

mod codec;
mod features;
mod validation;

// 保留节点转换函数在 crate 内原有的访问路径。
#[allow(unused_imports)]
pub(crate) use codec::{block_from_value, block_to_value, inline_from_value, inline_to_value};

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
    /// 公式上下文使用的默认数学字体族。
    pub default_math_font_family: Option<String>,
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

impl Default for DocumentSettings {
    fn default() -> Self {
        Self {
            default_language: None,
            default_font_family: None,
            default_cjk_font_family: None,
            default_math_font_family: None,
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
pub enum Block {
    Paragraph(Paragraph),
    Environment(BlockEnvironment),
    ExplicitBreak(ExplicitBreak),
    Opaque(OpaqueNode),
}

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

/// 段落级可选覆盖；未设置的属性继承文档全局段落样式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ParagraphLayoutOverrides {
    pub line_spacing: Option<ParagraphLineSpacing>,
    pub space_before_centipoints: Option<u32>,
    pub space_after_centipoints: Option<u32>,
    pub widow_orphan_control: Option<bool>,
    pub keep_with_next: Option<bool>,
    pub keep_lines_together: Option<bool>,
    pub page_break_before: Option<bool>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub text: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InlineEnvironmentKind {
    Bold,
    Italic,
    Font,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InlineEnvironment {
    pub id: NodeId,
    pub kind: InlineEnvironmentKind,
    pub definition_id: Option<String>,
    pub props: BTreeMap<String, Value>,
    pub children: Vec<InlineNode>,
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockEnvironmentKind {
    Section,
    Showcase,
}

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

#[derive(Debug, Clone, PartialEq)]
pub enum InlineNode {
    Run(Run),
    Environment(InlineEnvironment),
    SoftBreak(SoftBreak),
    Opaque(OpaqueNode),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExplicitBreak {
    #[serde(default)]
    pub kind: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SoftBreak {
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
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
