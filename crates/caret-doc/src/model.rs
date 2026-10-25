//! 定义文档树、节点 DTO、字段校验与语义节点序列化。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fmt::{Display, Formatter};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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

    pub fn validate(&self) -> Result<(), ModelError> {
        if self.page_sections.len() != 1 {
            return Err(ModelError::InvalidDocument(format!(
                "M0 requires exactly one PageSection, got {}",
                self.page_sections.len()
            )));
        }
        let mut ids = HashSet::new();
        if let Some(language) = &self.global_settings.default_language {
            validate_language_tag(language)?;
        }
        for section in &self.page_sections {
            insert_id(&mut ids, &section.id)?;
            section.geometry().map_err(ModelError::from)?;
        }
        let mut count = 0usize;
        for block in &self.body {
            validate_block(block, 0, &mut ids, &mut count)?;
        }
        Ok(())
    }

    pub fn required_features(&self) -> Vec<String> {
        let mut features = std::collections::BTreeSet::new();
        if self.global_settings.default_language.is_some()
            || self.body.iter().any(block_has_language)
        {
            features.insert("caret.text.language-tags".to_owned());
        }
        if self.global_settings.theme.is_some() {
            features.insert("caret.theme.snapshot".to_owned());
        }
        if self
            .global_settings
            .theme
            .as_ref()
            .is_some_and(|theme| !theme.environment_templates.is_empty())
        {
            features.insert("caret.env.templates".to_owned());
        }
        for block in &self.body {
            collect_block_features(block, &mut features);
        }
        features.into_iter().collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct DocumentSettings {
    pub default_language: Option<String>,
    pub default_font_family: Option<String>,
    pub default_styles: BTreeMap<String, Value>,
    pub format_overrides: BTreeMap<String, Value>,
    pub theme: Option<ThemeSnapshot>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
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
            inlines: Vec::new(),
            mark_style: BTreeMap::new(),
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
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

fn insert_id<'a>(ids: &mut HashSet<&'a str>, id: &'a NodeId) -> Result<(), ModelError> {
    if id.0.is_empty() || id.0.len() > 128 || id.0.chars().any(char::is_control) {
        return Err(ModelError::InvalidNodeId(id.0.clone()));
    }
    if !ids.insert(id.0.as_str()) {
        return Err(ModelError::DuplicateNodeId(id.0.clone()));
    }
    Ok(())
}

fn validate_block<'a>(
    block: &'a Block,
    depth: usize,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
) -> Result<(), ModelError> {
    validate_depth(depth, node_count)?;
    match block {
        Block::Paragraph(paragraph) => validate_paragraph(paragraph, depth, ids, node_count),
        Block::Environment(environment) => {
            insert_id(ids, &environment.id)?;
            for child in &environment.children {
                validate_block(child, depth + 1, ids, node_count)?;
            }
            if let Some(title) = &environment.title {
                validate_paragraph(title, depth + 1, ids, node_count)?;
            }
            Ok(())
        }
        Block::ExplicitBreak(_) | Block::Opaque(_) => Ok(()),
    }
}

fn validate_paragraph<'a>(
    paragraph: &'a Paragraph,
    depth: usize,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
) -> Result<(), ModelError> {
    validate_depth(depth, node_count)?;
    insert_id(ids, &paragraph.id)?;
    if let Some(language) = &paragraph.language {
        validate_language_tag(language)?;
    }
    for property in ["fontFamily", "bold", "italic"] {
        if paragraph.props.contains_key(property) {
            return Err(ModelError::ForbiddenDirectFormat {
                node: "Paragraph".to_owned(),
                property: property.to_owned(),
            });
        }
    }
    for inline in &paragraph.inlines {
        validate_inline(inline, depth + 1, ids, node_count)?;
    }
    Ok(())
}

fn validate_inline<'a>(
    inline: &'a InlineNode,
    depth: usize,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
) -> Result<(), ModelError> {
    validate_depth(depth, node_count)?;
    match inline {
        InlineNode::Run(run) => {
            if let Some(language) = &run.language {
                validate_language_tag(language)?;
            }
            for property in ["fontFamily", "bold", "italic"] {
                if run.props.contains_key(property) {
                    return Err(ModelError::ForbiddenDirectFormat {
                        node: "Run".to_owned(),
                        property: property.to_owned(),
                    });
                }
            }
            Ok(())
        }
        InlineNode::Environment(environment) => {
            insert_id(ids, &environment.id)?;
            if matches!(
                environment.kind,
                InlineEnvironmentKind::Bold | InlineEnvironmentKind::Italic
            ) && environment
                .props
                .get("enabled")
                .is_some_and(|value| !value.is_boolean())
            {
                return Err(ModelError::InvalidDocument(
                    "Bold/Italic enabled must be boolean".to_owned(),
                ));
            }
            if environment.kind == InlineEnvironmentKind::Font
                && environment
                    .props
                    .get("fontFamily")
                    .is_some_and(|value| !value.is_string())
            {
                return Err(ModelError::InvalidDocument(
                    "Font fontFamily must be a string".to_owned(),
                ));
            }
            for child in &environment.children {
                validate_inline(child, depth + 1, ids, node_count)?;
            }
            Ok(())
        }
        InlineNode::SoftBreak(_) | InlineNode::Opaque(_) => Ok(()),
    }
}

fn validate_language_tag(tag: &str) -> Result<(), ModelError> {
    if is_well_formed_language_tag(tag) {
        Ok(())
    } else {
        Err(ModelError::InvalidLanguageTag(tag.to_owned()))
    }
}

fn is_well_formed_language_tag(tag: &str) -> bool {
    const GRANDFATHERED: &[&str] = &[
        "art-lojban",
        "cel-gaulish",
        "en-gb-oed",
        "i-ami",
        "i-bnn",
        "i-default",
        "i-enochian",
        "i-hak",
        "i-klingon",
        "i-lux",
        "i-mingo",
        "i-navajo",
        "i-pwn",
        "i-tao",
        "i-tay",
        "i-tsu",
        "no-bok",
        "no-nyn",
        "sgn-be-fr",
        "sgn-be-nl",
        "sgn-ch-de",
        "zh-guoyu",
        "zh-hakka",
        "zh-min",
        "zh-min-nan",
        "zh-xiang",
    ];
    if GRANDFATHERED
        .iter()
        .any(|known| tag.eq_ignore_ascii_case(known))
    {
        return true;
    }
    if !tag.is_ascii() {
        return false;
    }
    let subtags: Vec<&str> = tag.split('-').collect();
    if subtags.is_empty()
        || subtags.iter().any(|subtag| {
            subtag.is_empty()
                || subtag.len() > 8
                || !subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
    {
        return false;
    }
    if subtags[0].eq_ignore_ascii_case("x") {
        return subtags.len() > 1;
    }
    let language = subtags[0];
    if !(2..=8).contains(&language.len())
        || !language.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return false;
    }

    let mut index = 1;
    if language.len() <= 3 {
        let mut extlang_count = 0;
        while index < subtags.len()
            && extlang_count < 3
            && subtags[index].len() == 3
            && subtags[index]
                .bytes()
                .all(|byte| byte.is_ascii_alphabetic())
        {
            index += 1;
            extlang_count += 1;
        }
    }
    if index < subtags.len()
        && subtags[index].len() == 4
        && subtags[index]
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic())
    {
        index += 1;
    }
    if index < subtags.len()
        && ((subtags[index].len() == 2
            && subtags[index]
                .bytes()
                .all(|byte| byte.is_ascii_alphabetic()))
            || (subtags[index].len() == 3
                && subtags[index].bytes().all(|byte| byte.is_ascii_digit())))
    {
        index += 1;
    }
    let mut variants = std::collections::BTreeSet::new();
    while index < subtags.len()
        && ((5..=8).contains(&subtags[index].len())
            || (subtags[index].len() == 4 && subtags[index].as_bytes()[0].is_ascii_digit()))
    {
        if !variants.insert(subtags[index].to_ascii_lowercase()) {
            return false;
        }
        index += 1;
    }

    let mut extensions = std::collections::BTreeSet::new();
    while index < subtags.len() && subtags[index].len() == 1 {
        let singleton = subtags[index].to_ascii_lowercase();
        if singleton == "x" || !extensions.insert(singleton) {
            break;
        }
        index += 1;
        let extension_start = index;
        while index < subtags.len() && (2..=8).contains(&subtags[index].len()) {
            index += 1;
        }
        if index == extension_start {
            return false;
        }
    }
    if index < subtags.len() && subtags[index].eq_ignore_ascii_case("x") {
        index += 1;
        if index == subtags.len() {
            return false;
        }
        while index < subtags.len() && (1..=8).contains(&subtags[index].len()) {
            index += 1;
        }
    }
    index == subtags.len()
}

fn validate_depth(depth: usize, node_count: &mut usize) -> Result<(), ModelError> {
    if depth > 64 {
        return Err(ModelError::NestingLimit);
    }
    *node_count = node_count.saturating_add(1);
    if *node_count > 2_000_000 {
        return Err(ModelError::InvalidDocument(
            "node limit exceeded".to_owned(),
        ));
    }
    Ok(())
}

fn block_has_language(block: &Block) -> bool {
    match block {
        Block::Paragraph(paragraph) => {
            paragraph.language.is_some()
                || paragraph.inlines.iter().any(|inline| match inline {
                    InlineNode::Run(run) => run.language.is_some(),
                    InlineNode::Environment(environment) => {
                        environment.children.iter().any(inline_has_language)
                    }
                    InlineNode::SoftBreak(_) | InlineNode::Opaque(_) => false,
                })
        }
        Block::Environment(environment) => {
            environment.title.as_ref().is_some_and(|title| {
                title.language.is_some() || title.inlines.iter().any(inline_has_language)
            }) || environment.children.iter().any(block_has_language)
        }
        Block::ExplicitBreak(_) | Block::Opaque(_) => false,
    }
}

fn inline_has_language(inline: &InlineNode) -> bool {
    match inline {
        InlineNode::Run(run) => run.language.is_some(),
        InlineNode::Environment(environment) => {
            environment.children.iter().any(inline_has_language)
        }
        InlineNode::SoftBreak(_) | InlineNode::Opaque(_) => false,
    }
}

fn collect_block_features(block: &Block, features: &mut std::collections::BTreeSet<String>) {
    match block {
        Block::Paragraph(paragraph) => {
            if paragraph.language.is_some() {
                features.insert("caret.text.language-tags".to_owned());
            }
            for inline in &paragraph.inlines {
                collect_inline_features(inline, features);
            }
        }
        Block::Environment(environment) => {
            features.insert(
                match environment.kind {
                    BlockEnvironmentKind::Section => "caret.env.section",
                    BlockEnvironmentKind::Showcase => "caret.env.showcase",
                }
                .to_owned(),
            );
            if environment.definition_id.is_some() {
                features.insert("caret.env.templates".to_owned());
            }
            if let Some(title) = &environment.title {
                if title.language.is_some() {
                    features.insert("caret.text.language-tags".to_owned());
                }
                for inline in &title.inlines {
                    collect_inline_features(inline, features);
                }
            }
            for child in &environment.children {
                collect_block_features(child, features);
            }
        }
        Block::ExplicitBreak(_) | Block::Opaque(_) => {}
    }
}

fn collect_inline_features(inline: &InlineNode, features: &mut std::collections::BTreeSet<String>) {
    match inline {
        InlineNode::Run(run) => {
            if run.language.is_some() {
                features.insert("caret.text.language-tags".to_owned());
            }
        }
        InlineNode::Environment(environment) => {
            features.insert(
                match environment.kind {
                    InlineEnvironmentKind::Bold => "caret.env.bold",
                    InlineEnvironmentKind::Italic => "caret.env.italic",
                    InlineEnvironmentKind::Font => "caret.env.font",
                }
                .to_owned(),
            );
            if environment.definition_id.is_some() {
                features.insert("caret.env.templates".to_owned());
            }
            if matches!(
                environment.kind,
                InlineEnvironmentKind::Bold | InlineEnvironmentKind::Italic
            ) && environment.props.contains_key("enabled")
            {
                features.insert("caret.env.explicit-format-state".to_owned());
            }
            for child in &environment.children {
                collect_inline_features(child, features);
            }
        }
        InlineNode::SoftBreak(_) | InlineNode::Opaque(_) => {}
    }
}

pub(crate) fn block_from_value(value: Value) -> Result<Block, ModelError> {
    let Some(object) = value.as_object() else {
        return Err(ModelError::InvalidDocument(
            "block node must be an object".to_owned(),
        ));
    };
    match object.get("type").and_then(Value::as_str) {
        Some("paragraph") => serde_json::from_value(without_fields(value, &["type"])?)
            .map(Block::Paragraph)
            .map_err(|error| ModelError::InvalidDocument(error.to_string())),
        Some("environment") if object.get("scope").and_then(Value::as_str) == Some("block") => {
            let kind = object.get("kind").and_then(Value::as_str);
            match kind {
                Some("section") | Some("showcase") => parse_block_environment(value),
                _ => Ok(Block::Opaque(OpaqueNode { raw: value })),
            }
        }
        Some("explicitBreak") => serde_json::from_value(without_fields(value, &["type"])?)
            .map(Block::ExplicitBreak)
            .map_err(|error| ModelError::InvalidDocument(error.to_string())),
        _ => Ok(Block::Opaque(OpaqueNode { raw: value })),
    }
}

pub(crate) fn inline_from_value(value: Value) -> Result<InlineNode, ModelError> {
    let Some(object) = value.as_object() else {
        return Err(ModelError::InvalidDocument(
            "inline node must be an object".to_owned(),
        ));
    };
    match object.get("type").and_then(Value::as_str) {
        Some("run") => serde_json::from_value(without_fields(value, &["type"])?)
            .map(InlineNode::Run)
            .map_err(|error| ModelError::InvalidDocument(error.to_string())),
        Some("softBreak") => serde_json::from_value(without_fields(value, &["type"])?)
            .map(InlineNode::SoftBreak)
            .map_err(|error| ModelError::InvalidDocument(error.to_string())),
        Some("environment") if object.get("scope").and_then(Value::as_str) == Some("inline") => {
            let kind = object.get("kind").and_then(Value::as_str);
            match kind {
                Some("bold") | Some("italic") | Some("font") => parse_inline_environment(value),
                _ => Ok(InlineNode::Opaque(OpaqueNode { raw: value })),
            }
        }
        _ => Ok(InlineNode::Opaque(OpaqueNode { raw: value })),
    }
}

fn parse_block_environment(value: Value) -> Result<Block, ModelError> {
    let mut object = value.as_object().cloned().ok_or_else(|| {
        ModelError::InvalidDocument("environment node must be an object".to_owned())
    })?;
    let kind = object
        .remove("kind")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| ModelError::InvalidDocument("environment kind is missing".to_owned()))?;
    object.remove("type");
    object.remove("scope");
    let kind = match kind.as_str() {
        "section" => BlockEnvironmentKind::Section,
        "showcase" => BlockEnvironmentKind::Showcase,
        _ => return Err(ModelError::InvalidEnvironmentScope(kind)),
    };
    let children = take_nodes(&mut object, "children", block_from_value)?;
    let title = object
        .remove("title")
        .filter(|value| !value.is_null())
        .map(serde_json::from_value::<Paragraph>)
        .transpose()
        .map_err(|error| ModelError::InvalidDocument(error.to_string()))?;
    let node = BlockEnvironment {
        id: parse_id_field(&mut object)?,
        kind,
        definition_id: take_optional_string(&mut object, "definitionId")?,
        props: take_map(&mut object, "props")?,
        title,
        children,
        extra: object.into_iter().collect(),
    };
    Ok(Block::Environment(node))
}

fn parse_inline_environment(value: Value) -> Result<InlineNode, ModelError> {
    let mut object = value.as_object().cloned().ok_or_else(|| {
        ModelError::InvalidDocument("environment node must be an object".to_owned())
    })?;
    let kind = object
        .remove("kind")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| ModelError::InvalidDocument("environment kind is missing".to_owned()))?;
    object.remove("type");
    object.remove("scope");
    let kind = match kind.as_str() {
        "bold" => InlineEnvironmentKind::Bold,
        "italic" => InlineEnvironmentKind::Italic,
        "font" => InlineEnvironmentKind::Font,
        _ => return Err(ModelError::InvalidEnvironmentScope(kind)),
    };
    let children = take_nodes(&mut object, "children", inline_from_value)?;
    let node = InlineEnvironment {
        id: parse_id_field(&mut object)?,
        kind,
        definition_id: take_optional_string(&mut object, "definitionId")?,
        props: take_map(&mut object, "props")?,
        children,
        extra: object.into_iter().collect(),
    };
    Ok(InlineNode::Environment(node))
}

fn without_fields(mut value: Value, fields: &[&str]) -> Result<Value, ModelError> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| ModelError::InvalidDocument("node must be an object".to_owned()))?;
    for field in fields {
        object.remove(*field);
    }
    Ok(value)
}

fn take_nodes<T>(
    object: &mut Map<String, Value>,
    key: &str,
    mut parse: impl FnMut(Value) -> Result<T, ModelError>,
) -> Result<Vec<T>, ModelError> {
    let Some(value) = object.remove(key) else {
        return Ok(Vec::new());
    };
    let Some(values) = value.as_array() else {
        return Err(ModelError::InvalidDocument(format!(
            "{key} must be an array"
        )));
    };
    values.iter().cloned().map(&mut parse).collect()
}

fn parse_id_field(object: &mut Map<String, Value>) -> Result<NodeId, ModelError> {
    let Some(value) = object
        .remove("id")
        .and_then(|value| value.as_str().map(str::to_owned))
    else {
        return Err(ModelError::InvalidDocument(
            "node id is missing or not a string".to_owned(),
        ));
    };
    NodeId::parse(value)
}

fn take_optional_string(
    object: &mut Map<String, Value>,
    key: &str,
) -> Result<Option<String>, ModelError> {
    match object.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(ModelError::InvalidDocument(format!(
            "{key} must be a string"
        ))),
    }
}

fn take_map(
    object: &mut Map<String, Value>,
    key: &str,
) -> Result<BTreeMap<String, Value>, ModelError> {
    match object.remove(key) {
        None => Ok(BTreeMap::new()),
        Some(Value::Object(value)) => Ok(value.into_iter().collect()),
        Some(_) => Err(ModelError::InvalidDocument(format!(
            "{key} must be an object"
        ))),
    }
}

pub(crate) fn block_to_value(block: &Block) -> Result<Value, ModelError> {
    match block {
        Block::Paragraph(value) => with_type("paragraph", value),
        Block::Environment(value) => environment_to_value(
            EnvironmentValue {
                scope: "block",
                kind: value.kind,
                id: &value.id,
                definition_id: &value.definition_id,
                props: &value.props,
                title: value.title.as_ref(),
                children: &value.children,
                extra: &value.extra,
            },
            block_to_value,
        ),
        Block::ExplicitBreak(value) => with_type("explicitBreak", value),
        Block::Opaque(value) => Ok(value.raw.clone()),
    }
}

pub(crate) fn inline_to_value(inline: &InlineNode) -> Result<Value, ModelError> {
    match inline {
        InlineNode::Run(value) => with_type("run", value),
        InlineNode::Environment(value) => environment_to_value(
            EnvironmentValue {
                scope: "inline",
                kind: value.kind,
                id: &value.id,
                definition_id: &value.definition_id,
                props: &value.props,
                title: None,
                children: &value.children,
                extra: &value.extra,
            },
            inline_to_value,
        ),
        InlineNode::SoftBreak(value) => with_type("softBreak", value),
        InlineNode::Opaque(value) => Ok(value.raw.clone()),
    }
}

fn with_type<T: Serialize>(node_type: &str, value: &T) -> Result<Value, ModelError> {
    let mut object = serde_json::to_value(value)
        .map_err(|error| ModelError::InvalidDocument(error.to_string()))?
        .as_object()
        .cloned()
        .ok_or_else(|| {
            ModelError::InvalidDocument("node did not serialize as an object".to_owned())
        })?;
    object.insert("type".to_owned(), Value::String(node_type.to_owned()));
    Ok(Value::Object(object))
}

trait EnvironmentKindName {
    fn name(self) -> &'static str;
}

impl EnvironmentKindName for InlineEnvironmentKind {
    fn name(self) -> &'static str {
        match self {
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Font => "font",
        }
    }
}

impl EnvironmentKindName for BlockEnvironmentKind {
    fn name(self) -> &'static str {
        match self {
            Self::Section => "section",
            Self::Showcase => "showcase",
        }
    }
}

struct EnvironmentValue<'a, K, T> {
    scope: &'a str,
    kind: K,
    id: &'a NodeId,
    definition_id: &'a Option<String>,
    props: &'a BTreeMap<String, Value>,
    title: Option<&'a Paragraph>,
    children: &'a [T],
    extra: &'a BTreeMap<String, Value>,
}

fn environment_to_value<K: EnvironmentKindName, T>(
    environment: EnvironmentValue<'_, K, T>,
    mut serialize_child: impl FnMut(&T) -> Result<Value, ModelError>,
) -> Result<Value, ModelError> {
    let mut object: Map<String, Value> = environment.extra.clone().into_iter().collect();
    object.insert("type".to_owned(), Value::String("environment".to_owned()));
    object.insert(
        "scope".to_owned(),
        Value::String(environment.scope.to_owned()),
    );
    object.insert("id".to_owned(), Value::String(environment.id.0.clone()));
    object.insert(
        "kind".to_owned(),
        Value::String(environment.kind.name().to_owned()),
    );
    if let Some(definition_id) = environment.definition_id {
        object.insert(
            "definitionId".to_owned(),
            Value::String(definition_id.clone()),
        );
    }
    object.insert(
        "props".to_owned(),
        Value::Object(environment.props.clone().into_iter().collect()),
    );
    if let Some(title) = environment.title {
        object.insert(
            "title".to_owned(),
            serde_json::to_value(title)
                .map_err(|error| ModelError::InvalidDocument(error.to_string()))?,
        );
    }
    object.insert(
        "children".to_owned(),
        Value::Array(
            environment
                .children
                .iter()
                .map(&mut serialize_child)
                .collect::<Result<_, _>>()?,
        ),
    );
    Ok(Value::Object(object))
}

impl Serialize for Block {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        block_to_value(self)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Block {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        block_from_value(value).map_err(serde::de::Error::custom)
    }
}

impl Serialize for InlineNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        inline_to_value(self)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for InlineNode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        inline_from_value(value).map_err(serde::de::Error::custom)
    }
}
