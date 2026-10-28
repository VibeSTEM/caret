//! 编解码语义节点并保留未知字段。

use super::*;
use serde_json::Map;

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
