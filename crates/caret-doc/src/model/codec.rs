//! 编解码语义节点并保留未知字段。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;
use serde_json::Map;

pub(crate) fn block_from_value(value: Value) -> Result<Block, ModelError> {
    let Some(object) = value.as_object() else {
        return Err(ModelError::InvalidDocument(
            "block node must be an object".to_owned(),
        ));
    };
    match object.get("type").and_then(Value::as_str) {
        Some("equation") if supported_math_tree(object.get("mathTree")) => {
            serde_json::from_value(without_fields(value, &["type"])?)
                .map(Block::Equation)
                .map_err(|error| ModelError::InvalidDocument(error.to_string()))
        }
        Some("equation") => Ok(Block::Opaque(OpaqueNode { raw: value })),
        Some("align") if supported_align(object) => {
            serde_json::from_value(without_fields(value, &["type"])?)
                .map(Block::Align)
                .map_err(|error| ModelError::InvalidDocument(error.to_string()))
        }
        Some("align") => Ok(Block::Opaque(OpaqueNode { raw: value })),
        Some("paragraph") => serde_json::from_value(without_fields(value, &["type"])?)
            .map(Block::Paragraph)
            .map_err(|error| ModelError::InvalidDocument(error.to_string())),
        Some("environment") if object.get("scope").and_then(Value::as_str) == Some("block") => {
            let kind = object.get("kind").and_then(Value::as_str);
            match kind {
                Some("section") | Some("custom") => parse_block_environment(value),
                _ => Ok(Block::Opaque(OpaqueNode { raw: value })),
            }
        }
        Some("explicitBreak") => serde_json::from_value(without_fields(value, &["type"])?)
            .map(Block::ExplicitBreak)
            .map_err(|error| ModelError::InvalidDocument(error.to_string())),
        _ => Ok(Block::Opaque(OpaqueNode { raw: value })),
    }
}

pub(crate) fn paragraph_content_from_value(value: Value) -> Result<ParagraphContent, ModelError> {
    let Some(object) = value.as_object() else {
        return Err(ModelError::InvalidDocument(
            "paragraph content must be an object".to_owned(),
        ));
    };
    match object.get("type").and_then(Value::as_str) {
        Some("inlineMath") if supported_math_tree(object.get("mathTree")) => {
            serde_json::from_value(without_fields(value, &["type"])?)
                .map(ParagraphContent::Math)
                .map_err(|error| ModelError::InvalidDocument(error.to_string()))
        }
        Some("inlineMath") => Ok(ParagraphContent::Opaque(OpaqueNode { raw: value })),
        Some("text") if object.get("runs").is_some() && !object.contains_key("text") => {
            parse_text_flow(value).map(ParagraphContent::Text)
        }
        Some("text") => Ok(ParagraphContent::Opaque(OpaqueNode { raw: value })),
        Some("softBreak") => serde_json::from_value(without_fields(value, &["type"])?)
            .map(ParagraphContent::SoftBreak)
            .map_err(|error| ModelError::InvalidDocument(error.to_string())),
        Some("environment") if object.get("scope").and_then(Value::as_str) == Some("inline") => {
            let kind = object.get("kind").and_then(Value::as_str);
            if kind
                .and_then(InlineEnvironmentKind::from_storage_name)
                .is_some()
            {
                parse_inline_environment(value, paragraph_content_from_value)
                    .map(ParagraphContent::Environment)
            } else {
                Ok(ParagraphContent::Opaque(OpaqueNode { raw: value }))
            }
        }
        _ => Ok(ParagraphContent::Opaque(OpaqueNode { raw: value })),
    }
}

pub(crate) fn math_content_from_value(value: Value) -> Result<MathContent, ModelError> {
    let Some(object) = value.as_object() else {
        return Err(ModelError::InvalidDocument(
            "math content must be an object".to_owned(),
        ));
    };
    match object.get("type").and_then(Value::as_str) {
        Some("math") => serde_json::from_value(without_fields(value, &["type"])?)
            .map(MathContent::Math)
            .map_err(|error| ModelError::InvalidDocument(error.to_string())),
        Some("text") if object.get("runs").is_some() && !object.contains_key("text") => {
            parse_text_flow(value).map(MathContent::Text)
        }
        Some("text") => Ok(MathContent::Opaque(OpaqueNode { raw: value })),
        Some("softBreak") => serde_json::from_value(without_fields(value, &["type"])?)
            .map(MathContent::SoftBreak)
            .map_err(|error| ModelError::InvalidDocument(error.to_string())),
        Some("environment") if object.get("scope").and_then(Value::as_str) == Some("inline") => {
            let kind = object.get("kind").and_then(Value::as_str);
            if kind
                .and_then(InlineEnvironmentKind::from_storage_name)
                .is_some()
            {
                parse_inline_environment(value, math_content_from_value)
                    .map(MathContent::Environment)
            } else {
                Ok(MathContent::Opaque(OpaqueNode { raw: value }))
            }
        }
        _ => Ok(MathContent::Opaque(OpaqueNode { raw: value })),
    }
}

fn parse_text_flow(value: Value) -> Result<TextFlow, ModelError> {
    let mut object = value
        .as_object()
        .cloned()
        .ok_or_else(|| ModelError::InvalidDocument("text flow must be an object".to_owned()))?;
    object.remove("type");
    let runs = take_nodes(&mut object, "runs", |value| {
        serde_json::from_value(value)
            .map_err(|error| ModelError::InvalidDocument(error.to_string()))
    })?;
    Ok(TextFlow {
        runs,
        extra: object.into_iter().collect(),
    })
}

/// 判定原始 JSON 是否声明当前数学树 schema，且根流属于当前解码器可解释的形态。
fn supported_math_tree(value: Option<&Value>) -> bool {
    let Some(tree) = value else {
        return false;
    };
    super::MathTree::storage_schema_is_current(tree)
        && tree
            .get("root")
            .is_some_and(|root| supported_math_flow(root, None))
}

fn supported_math_flow(value: &Value, parent: Option<super::MathNodeKind>) -> bool {
    let Some(flow) = value.as_object() else {
        return false;
    };
    flow.get("content")
        .and_then(Value::as_array)
        .is_some_and(|content| {
            content
                .iter()
                .all(|item| match item.get("type").and_then(Value::as_str) {
                    Some("math") => supported_math_node(item, parent),
                    Some("text") => {
                        item.get("runs")
                            .and_then(Value::as_array)
                            .is_some_and(|runs| runs.iter().all(Value::is_object))
                            && item.get("text").is_none()
                    }
                    Some("softBreak") => true,
                    Some("environment")
                        if item.get("scope").and_then(Value::as_str) == Some("inline") =>
                    {
                        item.get("kind")
                            .and_then(Value::as_str)
                            .and_then(InlineEnvironmentKind::from_storage_name)
                            .is_some()
                            && item
                                .get("children")
                                .and_then(Value::as_array)
                                .is_some_and(|children| children.iter().all(supported_math_content))
                    }
                    _ => false,
                })
        })
}

fn supported_math_content(value: &Value) -> bool {
    let Some(content) = value.as_object() else {
        return false;
    };
    match content.get("type").and_then(Value::as_str) {
        Some("math") => supported_math_node(value, None),
        Some("text") => {
            content
                .get("runs")
                .and_then(Value::as_array)
                .is_some_and(|runs| runs.iter().all(Value::is_object))
                && content.get("text").is_none()
        }
        Some("softBreak") => true,
        Some("environment") if content.get("scope").and_then(Value::as_str) == Some("inline") => {
            let kind = content.get("kind").and_then(Value::as_str);
            kind.and_then(InlineEnvironmentKind::from_storage_name)
                .is_some()
                && content
                    .get("children")
                    .and_then(Value::as_array)
                    .is_some_and(|children| children.iter().all(supported_math_content))
        }
        _ => false,
    }
}

fn supported_align(object: &Map<String, Value>) -> bool {
    let Some(rows) = object.get("rows").and_then(Value::as_array) else {
        return false;
    };
    rows.iter().all(|row| {
        row.as_object()
            .and_then(|row| row.get("cells"))
            .and_then(Value::as_array)
            .is_some_and(|cells| cells.iter().all(|cell| supported_math_tree(Some(cell))))
    })
}

impl super::MathTree {
    /// 判定原始 JSON 的数学树 schema 版本是否与当前解码器一致。
    pub(crate) fn storage_schema_is_current(tree: &Value) -> bool {
        tree.get("schemaVersion").and_then(Value::as_u64)
            == Some(super::MATH_TREE_SCHEMA_VERSION as u64)
    }
}

/// 类型解码器与 raw 扫描对畸形数学 JSON 的既有容忍边界。
///
/// 两条路径历史上对同一份 JSON 可能给出不同结论：类型解码器把非字符串 `text`
/// 视为缺失，但要求操作数流能给出确定的类别；raw 扫描把非字符串 `text` 视为
/// 不可解释，却把类别无法确定的单元素操作数流按 `Row` 继续检查。共享实现必须
/// 保留这两套输入边界，由调用方显式选择，不能向任何一侧对齐。
#[derive(Clone, Copy)]
struct MathNodeSupportPolicy {
    text: MathTextPolicy,
    flows: MathFlowKindPolicy,
}

/// `text` 字段的既有容忍边界。
#[derive(Clone, Copy)]
enum MathTextPolicy {
    /// 类型解码器：非字符串 `text` 按缺失处理。
    Lenient,
    /// raw 扫描：非字符串 `text` 视为不可解释。
    Strict,
}

/// 操作数流 primary kind 的既有推断方式。
#[derive(Clone, Copy)]
enum MathFlowKindPolicy {
    /// 类型解码器：任一操作数流无法确定 primary kind 即视为不可解释。
    Decoder,
    /// raw 退役字段扫描：类别无法确定的单元素操作数流按 `Row` 继续检查。
    RawScan,
    /// raw 能力扫描：按流中首个数学内容宽松推断，无法确定时按 `Row`。
    RawCapability,
}

impl MathNodeSupportPolicy {
    const DECODER: Self = Self {
        text: MathTextPolicy::Lenient,
        flows: MathFlowKindPolicy::Decoder,
    };
    const RAW_SCAN: Self = Self {
        text: MathTextPolicy::Strict,
        flows: MathFlowKindPolicy::RawScan,
    };
    const RAW_CAPABILITY: Self = Self {
        text: MathTextPolicy::Strict,
        flows: MathFlowKindPolicy::RawCapability,
    };
}

impl super::MathNodeKind {
    /// 判定单个原始数学节点是否为类型解码器可解释的形态。
    ///
    /// 返回值是该节点的类别；`None` 表示只能按不透明数据处理。与 raw 扫描共用同一
    /// 属性/结构实现，但保留类型解码器的既有输入边界（见 `MathNodeSupportPolicy`）。
    pub(crate) fn supported_from_storage(
        object: &Map<String, Value>,
        parent: Option<Self>,
    ) -> Option<Self> {
        Self::supported_with_policy(object, parent, MathNodeSupportPolicy::DECODER)
    }

    /// 判定单个原始数学节点是否为 raw 退役字段扫描可解释的形态。
    ///
    /// 保留旧 raw 检查的既有边界：非字符串 `text` 视为不可解释；类别无法确定的
    /// 单元素操作数流按 `Row` 处理，以便继续检查节点自身的退役字段。
    pub(crate) fn supported_for_raw_scan(
        object: &Map<String, Value>,
        parent: Option<Self>,
    ) -> Option<Self> {
        Self::supported_with_policy(object, parent, MathNodeSupportPolicy::RAW_SCAN)
    }

    /// 判定单个原始数学节点是否为 raw 能力扫描可解释的形态。
    ///
    /// 保留旧能力扫描的既有边界：非字符串 `text` 视为不可解释；操作数类别按流中
    /// 首个数学内容宽松推断，推断失败时按 `Row` 处理。
    pub(crate) fn supported_for_capability_scan(
        object: &Map<String, Value>,
        parent: Option<Self>,
    ) -> Option<Self> {
        Self::supported_with_policy(object, parent, MathNodeSupportPolicy::RAW_CAPABILITY)
    }

    /// 属性/结构判定的共享实现；输入容忍边界完全由策略决定，三条调用路径各自
    /// 传入与自己旧实现一致的政策。
    fn supported_with_policy(
        object: &Map<String, Value>,
        parent: Option<Self>,
        policy: MathNodeSupportPolicy,
    ) -> Option<Self> {
        let kind_name = object.get("kind").and_then(Value::as_str)?;
        let kind: Self = serde_json::from_value(Value::String(kind_name.to_owned())).ok()?;
        let mut attributes = BTreeMap::new();
        if let Some(values) = object.get("attributes") {
            let values = values.as_object()?;
            for (key, value) in values {
                attributes.insert(key.clone(), value.as_str()?.to_owned());
            }
        }
        let child_flows = match object.get("children") {
            None => Vec::new(),
            Some(Value::Array(children)) => children
                .iter()
                .map(|flow| math_flow_primary_kind(flow, policy.flows))
                .collect::<Option<Vec<_>>>()?,
            Some(_) => return None,
        };
        let text = match policy.text {
            MathTextPolicy::Lenient => object.get("text").and_then(Value::as_str),
            MathTextPolicy::Strict => match object.get("text") {
                None | Some(Value::Null) => None,
                Some(Value::String(text)) => Some(text.as_str()),
                Some(_) => return None,
            },
        };
        (math_node_attributes_supported(kind, text, &attributes)
            && math_node_structure_supported(parent, kind, text, &child_flows))
        .then_some(kind)
    }
}

fn supported_math_node(value: &Value, parent: Option<super::MathNodeKind>) -> bool {
    let Some(node) = value.as_object() else {
        return false;
    };
    let Some(kind) = super::MathNodeKind::supported_from_storage(node, parent) else {
        return false;
    };
    node.get("children").is_none_or(|children| {
        children.as_array().is_some_and(|children| {
            children
                .iter()
                .all(|child| supported_math_flow(child, Some(kind)))
        })
    })
}

/// 按调用方的既有边界推断操作数流的 primary kind。
fn math_flow_primary_kind(flow: &Value, policy: MathFlowKindPolicy) -> Option<super::MathNodeKind> {
    match policy {
        MathFlowKindPolicy::Decoder => decoder_flow_primary_kind(flow),
        MathFlowKindPolicy::RawScan => raw_scan_flow_primary_kind(flow),
        MathFlowKindPolicy::RawCapability => Some(raw_capability_flow_primary_kind(flow)),
    }
}

/// 类型解码器：单元素流必须能给出确定的类别，否则视为不可解释。
fn decoder_flow_primary_kind(flow: &Value) -> Option<super::MathNodeKind> {
    let content = flow.as_object()?.get("content")?.as_array()?;
    if content.len() == 1 {
        let item = content[0].as_object()?;
        if item.get("type")?.as_str()? == "math" {
            let kind = item
                .get("kind")?
                .as_str()
                .and_then(|kind| serde_json::from_value(Value::String(kind.to_owned())).ok())?;
            return Some(match kind {
                super::MathNodeKind::Placeholder
                | super::MathNodeKind::Hline
                | super::MathNodeKind::Intertext => kind,
                _ => super::MathNodeKind::Row,
            });
        }
    }
    Some(super::MathNodeKind::Row)
}

/// raw 退役字段扫描：类别无法确定的单元素流按 `Row` 继续检查。
fn raw_scan_flow_primary_kind(flow: &Value) -> Option<super::MathNodeKind> {
    let content = flow.as_object()?.get("content")?.as_array()?;
    if let [item] = content.as_slice()
        && let Some(item) = item.as_object()
        && item.get("type").and_then(Value::as_str) == Some("math")
    {
        let kind = item
            .get("kind")?
            .as_str()
            .and_then(|kind| serde_json::from_value(Value::String(kind.to_owned())).ok())?;
        return Some(match kind {
            super::MathNodeKind::Placeholder
            | super::MathNodeKind::Hline
            | super::MathNodeKind::Intertext => kind,
            _ => super::MathNodeKind::Row,
        });
    }
    Some(super::MathNodeKind::Row)
}

/// raw 能力扫描：按流中首个数学内容宽松推断，推断失败时按 `Row`。
fn raw_capability_flow_primary_kind(flow: &Value) -> super::MathNodeKind {
    let first_math = flow
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| {
            content
                .iter()
                .find(|item| item.get("type").and_then(Value::as_str) == Some("math"))
        });
    match first_math
        .and_then(|item| item.get("kind"))
        .and_then(Value::as_str)
        .and_then(|name| serde_json::from_value(Value::String(name.to_owned())).ok())
    {
        Some(
            special @ (super::MathNodeKind::Placeholder
            | super::MathNodeKind::Hline
            | super::MathNodeKind::Intertext),
        ) => special,
        _ => super::MathNodeKind::Row,
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
        "custom" => BlockEnvironmentKind::Custom,
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

fn parse_inline_environment<T>(
    value: Value,
    parse_child: impl FnMut(Value) -> Result<T, ModelError>,
) -> Result<InlineEnvironment<T>, ModelError> {
    let mut object = value.as_object().cloned().ok_or_else(|| {
        ModelError::InvalidDocument("environment node must be an object".to_owned())
    })?;
    let kind = object
        .remove("kind")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| ModelError::InvalidDocument("environment kind is missing".to_owned()))?;
    object.remove("type");
    object.remove("scope");
    let kind = InlineEnvironmentKind::from_storage_name(&kind)
        .ok_or_else(|| ModelError::InvalidEnvironmentScope(kind.clone()))?;
    let children = take_nodes(&mut object, "children", parse_child)?;
    let node = InlineEnvironment {
        id: parse_id_field(&mut object)?,
        kind,
        definition_id: take_optional_string(&mut object, "definitionId")?,
        props: take_map(&mut object, "props")?,
        children,
        extra: object.into_iter().collect(),
    };
    Ok(node)
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
        Block::Equation(value) => with_type("equation", value),
        Block::Align(value) => with_type("align", value),
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

pub(crate) fn paragraph_content_to_value(content: &ParagraphContent) -> Result<Value, ModelError> {
    match content {
        ParagraphContent::Text(value) => with_type("text", value),
        ParagraphContent::Math(value) => with_type("inlineMath", value),
        ParagraphContent::Environment(value) => environment_to_value(
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
            paragraph_content_to_value,
        ),
        ParagraphContent::SoftBreak(value) => with_type("softBreak", value),
        ParagraphContent::Opaque(value) => Ok(value.raw.clone()),
    }
}

pub(crate) fn math_content_to_value(content: &MathContent) -> Result<Value, ModelError> {
    match content {
        MathContent::Text(value) => with_type("text", value),
        MathContent::Math(value) => with_type("math", value),
        MathContent::Environment(value) => environment_to_value(
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
            math_content_to_value,
        ),
        MathContent::SoftBreak(value) => with_type("softBreak", value),
        MathContent::Opaque(value) => Ok(value.raw.clone()),
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
        self.storage_name()
    }
}

impl EnvironmentKindName for BlockEnvironmentKind {
    fn name(self) -> &'static str {
        match self {
            Self::Section => "section",
            Self::Custom => "custom",
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

impl Serialize for ParagraphContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        paragraph_content_to_value(self)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ParagraphContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        paragraph_content_from_value(value).map_err(serde::de::Error::custom)
    }
}

impl Serialize for MathContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        math_content_to_value(self)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MathContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        math_content_from_value(value).map_err(serde::de::Error::custom)
    }
}
