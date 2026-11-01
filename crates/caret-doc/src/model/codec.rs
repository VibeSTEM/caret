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
        Some("equation") if supported_tree(object.get("mathTree")) => {
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
        Some("inlineMath") if supported_tree(object.get("mathTree")) => {
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
    let source_range = object
        .remove("sourceRange")
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| ModelError::InvalidDocument(error.to_string()))?;
    Ok(TextFlow {
        runs,
        source_range,
        extra: object.into_iter().collect(),
    })
}

fn supported_tree(value: Option<&Value>) -> bool {
    let Some(tree) = value.and_then(Value::as_object) else {
        return false;
    };
    tree.get("schemaVersion").and_then(Value::as_u64)
        == Some(super::MATH_TREE_SCHEMA_VERSION as u64)
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
            .is_some_and(|cells| cells.iter().all(|cell| supported_tree(Some(cell))))
    })
}

fn supported_math_node(value: &Value, parent: Option<super::MathNodeKind>) -> bool {
    let Some(node) = value.as_object() else {
        return false;
    };
    let Some(kind_name) = node.get("kind").and_then(Value::as_str) else {
        return false;
    };
    let Ok(kind) =
        serde_json::from_value::<super::MathNodeKind>(Value::String(kind_name.to_owned()))
    else {
        return false;
    };
    let mut attributes = BTreeMap::new();
    if let Some(values) = node.get("attributes") {
        let Some(values) = values.as_object() else {
            return false;
        };
        for (key, value) in values {
            let Some(value) = value.as_str() else {
                return false;
            };
            attributes.insert(key.clone(), value.to_owned());
        }
    }
    let child_flows = match node.get("children") {
        None => Vec::new(),
        Some(Value::Array(children)) => {
            let Some(children) = children
                .iter()
                .map(math_flow_primary_kind)
                .collect::<Option<Vec<_>>>()
            else {
                return false;
            };
            children
        }
        Some(_) => return false,
    };
    super::math::math_node_attributes_supported(
        kind,
        node.get("text").and_then(Value::as_str),
        &attributes,
    ) && super::math::math_node_structure_supported(
        parent,
        kind,
        node.get("text").and_then(Value::as_str),
        &child_flows,
    ) && node.get("children").is_none_or(|children| {
        children.as_array().is_some_and(|children| {
            children
                .iter()
                .all(|child| supported_math_flow(child, Some(kind)))
        })
    })
}

fn math_flow_primary_kind(flow: &Value) -> Option<super::MathNodeKind> {
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
        source_range: object
            .remove("sourceRange")
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| ModelError::InvalidDocument(error.to_string()))?,
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
                source_range: None,
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
                source_range: value.source_range.as_ref(),
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
                source_range: value.source_range.as_ref(),
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
    source_range: Option<&'a super::MathSourceRange>,
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
    if let Some(source_range) = environment.source_range {
        object.insert(
            "sourceRange".to_owned(),
            serde_json::to_value(source_range)
                .map_err(|error| ModelError::InvalidDocument(error.to_string()))?,
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
