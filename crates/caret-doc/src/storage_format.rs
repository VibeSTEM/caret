//! 验证 `.cdoc` 外层格式、必需能力并提供安全预览/读写类型。

use crate::model::{Block, Document, Media, ModelError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

pub const CURRENT_FORMAT_VERSION: u32 = 1;
pub const CURRENT_THEME_FORMAT_VERSION: u32 = 1;
const MAX_MEDIA_BYTES: usize = 32 * 1024 * 1024;
const MAX_MEDIA_BASE64_BYTES: usize = MAX_MEDIA_BYTES.div_ceil(3) * 4;

const KNOWN_FEATURES: &[&str] = &[
    "caret.env.bold",
    "caret.env.explicit-format-state",
    "caret.env.font",
    "caret.env.italic",
    "caret.env.section",
    "caret.env.showcase",
    "caret.env.templates",
    "caret.font.family-preferences",
    "caret.paragraph.layout-settings",
    "caret.text.default-font-size",
    "caret.text.language-tags",
    "caret.theme.snapshot",
];

/// 可供编辑并保存的当前 `.cdoc` 文档。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CdocFile {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub required_features: Vec<String>,
    #[serde(default)]
    pub media: Vec<Media>,
    pub document: Document,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl CdocFile {
    pub fn new(document: Document) -> Result<Self, DocumentError> {
        let mut file = Self {
            format: "caret".to_owned(),
            version: CURRENT_FORMAT_VERSION,
            required_features: Vec::new(),
            media: Vec::new(),
            document,
            extra: BTreeMap::new(),
        };
        file.refresh_required_features();
        file.validate()?;
        Ok(file)
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    pub fn required_features(&self) -> &[String] {
        &self.required_features
    }

    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.format != "caret" {
            return Err(DocumentError::InvalidFormat(self.format.clone()));
        }
        if self.version != CURRENT_FORMAT_VERSION {
            return Err(DocumentError::UnsupportedDocumentVersion(self.version));
        }
        self.document.validate()?;
        validate_media(&self.media)?;
        validate_feature_list(&self.required_features)?;
        if self
            .required_features
            .iter()
            .any(|feature| !KNOWN_FEATURES.contains(&feature.as_str()))
            || contains_opaque_block(&self.document.body)
        {
            return Err(DocumentError::ReadOnlyPreview);
        }
        if self
            .document
            .global_settings
            .theme
            .as_ref()
            .is_some_and(|theme| theme.format_version != CURRENT_THEME_FORMAT_VERSION)
        {
            return Err(DocumentError::UnsupportedThemeVersion(
                self.document
                    .global_settings
                    .theme
                    .as_ref()
                    .map_or(0, |theme| theme.format_version),
            ));
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, DocumentError> {
        self.validate()?;
        let mut candidate = self.clone();
        candidate.refresh_required_features();
        serde_json::to_vec_pretty(&CdocFileRaw::from(&candidate))
            .map_err(|error| DocumentError::Serialization(error.to_string()))
    }

    fn refresh_required_features(&mut self) {
        self.required_features = self.document.required_features();
        self.required_features.sort();
        self.required_features.dedup();
    }
}

impl Serialize for CdocFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.validate().map_err(serde::ser::Error::custom)?;
        CdocFileRaw::from(self).serialize(serializer)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CdocFileRaw<'a> {
    format: &'a str,
    version: u32,
    required_features: &'a [String],
    media: &'a [Media],
    document: &'a Document,
    #[serde(flatten)]
    extra: &'a BTreeMap<String, Value>,
}

impl<'a> From<&'a CdocFile> for CdocFileRaw<'a> {
    fn from(file: &'a CdocFile) -> Self {
        Self {
            format: &file.format,
            version: file.version,
            required_features: &file.required_features,
            media: &file.media,
            document: &file.document,
            extra: &file.extra,
        }
    }
}

/// 读取 `.cdoc` 的结果；安全预览不能通过该 API 保存。
#[derive(Debug, Clone, PartialEq)]
pub enum OpenedDocument {
    Editable(CdocFile),
    SafePreview(ReadOnlyPreview),
}

impl OpenedDocument {
    pub fn document(&self) -> &Document {
        match self {
            Self::Editable(file) => file.document(),
            Self::SafePreview(preview) => preview.document(),
        }
    }

    pub fn is_read_only(&self) -> bool {
        matches!(self, Self::SafePreview(_))
    }
}

/// 只读安全预览保留完整 DTO 与不透明节点；通用 JSON 视图按需生成。
#[derive(Debug, Clone, PartialEq)]
pub struct ReadOnlyPreview {
    file: CdocFile,
    pub reasons: Vec<PreviewReason>,
}

impl ReadOnlyPreview {
    pub fn document(&self) -> &Document {
        self.file.document()
    }

    /// 仅在调用方需要 JSON 视图时创建，避免打开预览时复制整份文档。
    pub fn to_json_value(&self) -> Result<Value, DocumentError> {
        serde_json::to_value(CdocFileRaw::from(&self.file))
            .map_err(|error| DocumentError::Serialization(error.to_string()))
    }

    pub fn can_save(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewReason {
    UnsupportedRequiredFeature(String),
    UnsupportedNodeKind,
    UnsupportedThemeVersion(u32),
}

/// `.cdoc` 读取、验证或编码失败原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentError {
    InvalidJson(String),
    InvalidFormat(String),
    InvalidDocument(String),
    UnsupportedDocumentVersion(u32),
    UnsupportedThemeVersion(u32),
    InvalidRequiredFeatures(String),
    InvalidMedia(String),
    ReadOnlyPreview,
    Serialization(String),
}

impl Display for DocumentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson(reason) => write!(formatter, "cdoc.invalid_json: {reason}"),
            Self::InvalidFormat(format) => write!(formatter, "cdoc.invalid_format: {format}"),
            Self::InvalidDocument(reason) => write!(formatter, "cdoc.invalid_document: {reason}"),
            Self::UnsupportedDocumentVersion(version) => {
                write!(formatter, "cdoc.unsupported_version: {version}")
            }
            Self::UnsupportedThemeVersion(version) => {
                write!(formatter, "cdoc.unsupported_theme_version: {version}")
            }
            Self::InvalidRequiredFeatures(reason) => {
                write!(formatter, "cdoc.invalid_required_features: {reason}")
            }
            Self::InvalidMedia(reason) => write!(formatter, "cdoc.invalid_media: {reason}"),
            Self::ReadOnlyPreview => write!(formatter, "cdoc.safe_preview_read_only"),
            Self::Serialization(reason) => write!(formatter, "cdoc.serialization: {reason}"),
        }
    }
}

impl std::error::Error for DocumentError {}

impl From<ModelError> for DocumentError {
    fn from(error: ModelError) -> Self {
        Self::InvalidDocument(error.to_string())
    }
}

pub fn open(bytes: &[u8]) -> Result<OpenedDocument, DocumentError> {
    let raw: Value = serde_json::from_slice(bytes)
        .map_err(|error| DocumentError::InvalidJson(error.to_string()))?;
    let object = raw
        .as_object()
        .ok_or_else(|| DocumentError::InvalidDocument("envelope must be an object".to_owned()))?;
    let format = object
        .get("format")
        .and_then(Value::as_str)
        .ok_or_else(|| DocumentError::InvalidFormat("missing or not a string".to_owned()))?;
    if format != "caret" {
        return Err(DocumentError::InvalidFormat(format.to_owned()));
    }
    let version = object
        .get("version")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            DocumentError::InvalidDocument("version must be a positive integer".to_owned())
        })?;
    if version > CURRENT_FORMAT_VERSION {
        return Err(DocumentError::UnsupportedDocumentVersion(version));
    }
    if version < CURRENT_FORMAT_VERSION {
        return Err(DocumentError::UnsupportedDocumentVersion(version));
    }
    let features = parse_features(object.get("requiredFeatures"))?;
    let mut file: CdocFile = serde_json::from_value(raw)
        .map_err(|error| DocumentError::InvalidDocument(error.to_string()))?;
    if file.format != "caret" {
        return Err(DocumentError::InvalidFormat(file.format));
    }
    file.document.validate()?;
    validate_media(&file.media)?;
    let mut reasons: Vec<PreviewReason> = features
        .iter()
        .filter(|feature| !KNOWN_FEATURES.contains(&feature.as_str()))
        .map(|feature| PreviewReason::UnsupportedRequiredFeature(feature.clone()))
        .collect();
    if contains_opaque_block(&file.document.body) {
        reasons.push(PreviewReason::UnsupportedNodeKind);
    }
    if let Some(theme) = file
        .document
        .global_settings
        .theme
        .as_ref()
        .filter(|theme| theme.format_version != CURRENT_THEME_FORMAT_VERSION)
    {
        reasons.push(PreviewReason::UnsupportedThemeVersion(theme.format_version));
    }
    if reasons.is_empty() {
        file.refresh_required_features();
        file.validate()?;
        Ok(OpenedDocument::Editable(file))
    } else {
        Ok(OpenedDocument::SafePreview(ReadOnlyPreview {
            file,
            reasons,
        }))
    }
}

fn parse_features(raw: Option<&Value>) -> Result<Vec<String>, DocumentError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let Some(values) = raw.as_array() else {
        return Err(DocumentError::InvalidRequiredFeatures(
            "requiredFeatures must be an array".to_owned(),
        ));
    };
    let mut features = Vec::<String>::with_capacity(values.len());
    for value in values {
        let Some(feature) = value.as_str() else {
            return Err(DocumentError::InvalidRequiredFeatures(
                "feature identifiers must be strings".to_owned(),
            ));
        };
        if feature.is_empty() {
            return Err(DocumentError::InvalidRequiredFeatures(format!(
                "empty feature identifier {feature:?}"
            )));
        }
        if features
            .last()
            .is_some_and(|previous| previous.as_str() >= feature)
        {
            return Err(DocumentError::InvalidRequiredFeatures(
                "feature identifiers must be sorted and unique".to_owned(),
            ));
        }
        features.push(feature.to_owned());
    }
    Ok(features)
}

fn validate_feature_list(features: &[String]) -> Result<(), DocumentError> {
    if features.iter().any(String::is_empty)
        || features.windows(2).any(|window| window[0] >= window[1])
    {
        return Err(DocumentError::InvalidRequiredFeatures(
            "feature identifiers must be non-empty, sorted, and unique".to_owned(),
        ));
    }
    Ok(())
}

fn validate_media(media: &[Media]) -> Result<(), DocumentError> {
    let mut ids = std::collections::BTreeSet::new();
    for item in media {
        if item.id.is_empty() || !ids.insert(&item.id) {
            return Err(DocumentError::InvalidMedia(format!(
                "empty or duplicate media id {:?}",
                item.id
            )));
        }
        if item.media_type.is_empty() {
            return Err(DocumentError::InvalidMedia("mediaType is empty".to_owned()));
        }
        validate_base64(&item.data)?;
    }
    Ok(())
}

fn validate_base64(data: &str) -> Result<(), DocumentError> {
    if data.len() > MAX_MEDIA_BASE64_BYTES {
        return Err(DocumentError::InvalidMedia(
            "base64 payload is malformed or exceeds 32 MiB".to_owned(),
        ));
    }

    let mut decoder = base64::read::DecoderReader::new(
        data.as_bytes(),
        &base64::engine::general_purpose::STANDARD,
    );
    let decoded_bytes = std::io::copy(&mut decoder, &mut std::io::sink())
        .map_err(|_| DocumentError::InvalidMedia("base64 payload is malformed".to_owned()))?;
    if decoded_bytes > MAX_MEDIA_BYTES as u64 {
        return Err(DocumentError::InvalidMedia(
            "decoded media payload exceeds 32 MiB".to_owned(),
        ));
    }
    Ok(())
}

fn contains_opaque_block(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Opaque(_) => true,
        Block::Paragraph(paragraph) => paragraph.inlines.iter().any(contains_opaque_inline),
        Block::Environment(environment) => {
            environment
                .title
                .as_ref()
                .is_some_and(|title| title.inlines.iter().any(contains_opaque_inline))
                || contains_opaque_block(&environment.children)
        }
        Block::ExplicitBreak(_) => false,
    })
}

fn contains_opaque_inline(inline: &crate::model::InlineNode) -> bool {
    use crate::model::InlineNode;
    match inline {
        InlineNode::Opaque(_) => true,
        InlineNode::Environment(environment) => {
            environment.children.iter().any(contains_opaque_inline)
        }
        InlineNode::Run(_) | InlineNode::SoftBreak(_) => false,
    }
}
