//! 编解码语义节点并保留未知字段。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;
use serde_json::Map;
use std::collections::BTreeSet;

pub(crate) fn block_from_value(value: Value) -> Result<Block, ModelError> {
    let Some(object) = value.as_object() else {
        return Err(ModelError::InvalidDocument(
            "block node must be an object".to_owned(),
        ));
    };
    match object.get("type").and_then(Value::as_str) {
        Some("equation") => {
            let mut ids = BTreeSet::new();
            if !valid_node_id(object) || !insert_known_id(object, &mut ids) {
                return Err(invalid_math_encoding("equation id"));
            }
            match supported_math_tree_with_ids(object.get("mathTree"), &mut ids) {
                MathTreeSupport::Known => serde_json::from_value(without_fields(value, &["type"])?)
                    .map(Block::Equation)
                    .map_err(|error| ModelError::InvalidDocument(error.to_string())),
                MathTreeSupport::FutureOpaque => Ok(Block::Opaque(OpaqueNode { raw: value })),
                MathTreeSupport::Invalid => Err(invalid_math_encoding("equation mathTree")),
            }
        }
        Some("align") => match supported_align(object) {
            MathTreeSupport::Known => serde_json::from_value(without_fields(value, &["type"])?)
                .map(Block::Align)
                .map_err(|error| ModelError::InvalidDocument(error.to_string())),
            MathTreeSupport::FutureOpaque => Ok(Block::Opaque(OpaqueNode { raw: value })),
            MathTreeSupport::Invalid => Err(invalid_math_encoding("align mathTree")),
        },
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
        Some("table") => parse_table(value),
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
        Some("inlineMath") => {
            let mut ids = BTreeSet::new();
            if !valid_node_id(object) || !insert_known_id(object, &mut ids) {
                return Err(invalid_math_encoding("inlineMath id"));
            }
            match supported_math_tree_with_ids(object.get("mathTree"), &mut ids) {
                MathTreeSupport::Known => serde_json::from_value(without_fields(value, &["type"])?)
                    .map(ParagraphContent::Math)
                    .map_err(|error| ModelError::InvalidDocument(error.to_string())),
                MathTreeSupport::FutureOpaque => {
                    Ok(ParagraphContent::Opaque(OpaqueNode { raw: value }))
                }
                MathTreeSupport::Invalid => Err(invalid_math_encoding("inlineMath mathTree")),
            }
        }
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

/// 数学载体解码结果。
///
/// `FutureOpaque` 只用于当前 schema 内的未知节点类型/结构变体，载体出现这类节点时
/// 保留为不透明数据；`schemaVersion` 不匹配（含更高版本）一律返回 `Invalid`，
/// 不做向前兼容，也不降级为 Opaque。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MathTreeSupport {
    Known,
    FutureOpaque,
    Invalid,
}

impl MathTreeSupport {
    fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::Invalid, _) | (_, Self::Invalid) => Self::Invalid,
            (Self::FutureOpaque, _) | (_, Self::FutureOpaque) => Self::FutureOpaque,
            _ => Self::Known,
        }
    }
}

fn invalid_math_encoding(where_: &str) -> ModelError {
    ModelError::InvalidDocument(format!("invalid known math encoding: {where_}"))
}

fn valid_node_id(object: &Map<String, Value>) -> bool {
    object
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| NodeId::parse(id.to_owned()).ok())
        .is_some()
}

/// 判定当前数学树的编码。
///
/// 只接受当前 `schemaVersion`，更高或更低的版本都按格式错误拒绝，不做向前兼容；
/// 当前 schema 内的已知节点严格校验，未知节点类型仍保留为不透明数据。
pub(crate) fn supported_math_tree(value: Option<&Value>) -> MathTreeSupport {
    supported_math_tree_with_ids(value, &mut BTreeSet::new())
}

/// 判定整棵原始数学树，并把其中已知节点的 ID 收集到 `ids`。
///
/// `ids` 由 equation/inlineMath 载体检查传入（先放入载体 ID），用于发现整块转为
/// Opaque 前载体与树之间的重复 ID；Align 的块/行/单元格 ID 由 `supported_align`
/// 单独收集。
/// 版本判定与 [`supported_math_tree`] 一致：任何不匹配的 `schemaVersion` 都是
/// `Invalid`，未知节点类型才返回 `FutureOpaque`。
fn supported_math_tree_with_ids(
    value: Option<&Value>,
    ids: &mut BTreeSet<String>,
) -> MathTreeSupport {
    let Some(tree) = value.and_then(Value::as_object) else {
        return MathTreeSupport::Invalid;
    };
    let Some(version) = tree.get("schemaVersion").and_then(Value::as_u64) else {
        return MathTreeSupport::Invalid;
    };
    if version != super::MATH_TREE_SCHEMA_VERSION as u64 {
        return MathTreeSupport::Invalid;
    }
    let Some(root) = tree.get("root") else {
        return MathTreeSupport::Invalid;
    };
    let support = supported_math_flow(root, None);
    if support == MathTreeSupport::Invalid {
        return support;
    }
    if support == MathTreeSupport::FutureOpaque && !known_math_ids_are_unique(root, ids) {
        return MathTreeSupport::Invalid;
    }
    support
}

fn known_math_ids_are_unique(flow: &Value, ids: &mut BTreeSet<String>) -> bool {
    let Some(flow) = flow.as_object() else {
        return false;
    };
    if !insert_known_id(flow, ids) {
        return false;
    }
    let Some(content) = flow.get("content").and_then(Value::as_array) else {
        return false;
    };
    content
        .iter()
        .all(|item| known_math_ids_are_unique_in_content(item, ids))
}

fn known_math_ids_are_unique_in_content(value: &Value, ids: &mut BTreeSet<String>) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.get("type").and_then(Value::as_str) == Some("math") {
        let Some(kind) = object.get("kind").and_then(Value::as_str) else {
            return false;
        };
        if serde_json::from_value::<super::MathNodeKind>(Value::String(kind.to_owned())).is_err() {
            return true;
        }
        return insert_known_id(object, ids)
            && object.get("children").is_none_or(|children| {
                children.as_array().is_some_and(|children| {
                    children
                        .iter()
                        .all(|child| known_math_ids_are_unique(child, ids))
                })
            });
    }
    if object.get("type").and_then(Value::as_str) == Some("environment")
        && object.get("scope").and_then(Value::as_str) == Some("inline")
    {
        let Some(kind) = object.get("kind").and_then(Value::as_str) else {
            return false;
        };
        return InlineEnvironmentKind::from_storage_name(kind).is_none()
            || (insert_known_id(object, ids)
                && object
                    .get("children")
                    .and_then(Value::as_array)
                    .is_some_and(|children| {
                        children
                            .iter()
                            .all(|child| known_math_ids_are_unique_in_content(child, ids))
                    }));
    }
    true
}

fn insert_known_id(object: &Map<String, Value>, ids: &mut BTreeSet<String>) -> bool {
    object
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|id| ids.insert(id.to_owned()))
}

fn supported_math_flow(value: &Value, parent: Option<super::MathNodeKind>) -> MathTreeSupport {
    let Some(flow) = value.as_object() else {
        return MathTreeSupport::Invalid;
    };
    if !valid_node_id(flow) {
        return MathTreeSupport::Invalid;
    }
    let Some(content) = flow.get("content").and_then(Value::as_array) else {
        return MathTreeSupport::Invalid;
    };
    content.iter().fold(MathTreeSupport::Known, |result, item| {
        result.combine(supported_math_content_with_parent(item, parent))
    })
}

fn supported_math_content(value: &Value) -> MathTreeSupport {
    supported_math_content_with_parent(value, None)
}

fn supported_math_content_with_parent(
    value: &Value,
    parent: Option<super::MathNodeKind>,
) -> MathTreeSupport {
    let Some(content) = value.as_object() else {
        return MathTreeSupport::Invalid;
    };
    match content.get("type").and_then(Value::as_str) {
        Some("math") => supported_math_node(value, parent),
        Some("text") => supported_math_text(value),
        Some("softBreak") => MathTreeSupport::Known,
        Some("environment") => match content.get("scope").and_then(Value::as_str) {
            Some("inline") => supported_inline_environment(value),
            Some("block") => MathTreeSupport::Invalid,
            Some(_) => MathTreeSupport::FutureOpaque,
            None => MathTreeSupport::Invalid,
        },
        Some(_) => MathTreeSupport::FutureOpaque,
        None => MathTreeSupport::Invalid,
    }
}

fn supported_math_text(value: &Value) -> MathTreeSupport {
    let Some(object) = value.as_object() else {
        return MathTreeSupport::Invalid;
    };
    if object.contains_key("text") || !matches!(object.get("runs"), Some(Value::Array(_))) {
        return MathTreeSupport::Invalid;
    }
    let Ok(value) = without_fields(value.clone(), &["type"]) else {
        return MathTreeSupport::Invalid;
    };
    let Ok(text) = serde_json::from_value::<TextFlow>(value) else {
        return MathTreeSupport::Invalid;
    };
    if text
        .runs
        .iter()
        .any(|run| super::validation::validate_run(run).is_err())
    {
        MathTreeSupport::Invalid
    } else {
        MathTreeSupport::Known
    }
}

fn supported_inline_environment(value: &Value) -> MathTreeSupport {
    let Some(object) = value.as_object() else {
        return MathTreeSupport::Invalid;
    };
    let Some(kind_name) = object.get("kind").and_then(Value::as_str) else {
        return MathTreeSupport::Invalid;
    };
    let Some(kind) = InlineEnvironmentKind::from_storage_name(kind_name) else {
        return MathTreeSupport::FutureOpaque;
    };
    if !valid_node_id(object) {
        return MathTreeSupport::Invalid;
    }
    if object
        .get("definitionId")
        .is_some_and(|value| !value.is_null() && !value.is_string())
    {
        return MathTreeSupport::Invalid;
    }
    let props = match object.get("props") {
        None => BTreeMap::new(),
        Some(Value::Object(props)) => props.clone().into_iter().collect(),
        Some(_) => return MathTreeSupport::Invalid,
    };
    if super::validation::validate_environment_props(kind, &props).is_err() {
        return MathTreeSupport::Invalid;
    }
    let Some(children) = object.get("children").and_then(Value::as_array) else {
        return MathTreeSupport::Invalid;
    };
    children
        .iter()
        .fold(MathTreeSupport::Known, |state, child| {
            state.combine(supported_math_content(child))
        })
}

fn supported_align(object: &Map<String, Value>) -> MathTreeSupport {
    if !valid_node_id(object) {
        return MathTreeSupport::Invalid;
    }
    let Some(rows) = object.get("rows").and_then(Value::as_array) else {
        return MathTreeSupport::Invalid;
    };
    let mut ids = BTreeSet::new();
    if !insert_known_id(object, &mut ids) {
        return MathTreeSupport::Invalid;
    }
    rows.iter().fold(MathTreeSupport::Known, |result, row| {
        let Some(row) = row.as_object() else {
            return result.combine(MathTreeSupport::Invalid);
        };
        if !valid_node_id(row) || !insert_known_id(row, &mut ids) {
            return result.combine(MathTreeSupport::Invalid);
        }
        let Some(cells) = row.get("cells").and_then(Value::as_array) else {
            return result.combine(MathTreeSupport::Invalid);
        };
        cells.iter().fold(result, |state, cell| {
            let tree_support = supported_math_tree(Some(cell));
            if tree_support == MathTreeSupport::Invalid {
                return state.combine(tree_support);
            }
            // 只有当前 schema 的树才继续检查已知 ID 唯一性：更高版本已在
            // `supported_math_tree` 中返回 Invalid，不再进入这里的已知节点检查。
            // 未知节点导致整格转为 Opaque 时，本检查仍可发现已知兄弟间的重复 ID。
            let ids_are_unique = cell
                .get("root")
                .is_none_or(|root| known_math_ids_are_unique(root, &mut ids));
            state.combine(if ids_are_unique {
                tree_support
            } else {
                MathTreeSupport::Invalid
            })
        })
    })
}

/// raw 扫描历史边界策略。
///
/// 类型解码严格校验已知节点；raw 扫描保留较早的输入容忍政策，以免改变
/// capability/retired-field 扫描的遍历范围。
#[derive(Clone, Copy)]
struct MathNodeSupportPolicy {
    flows: MathFlowKindPolicy,
}

/// 操作数流 primary kind 的既有推断方式。
#[derive(Clone, Copy)]
enum MathFlowKindPolicy {
    /// 类型解码器：按 typed 校验规则取首个已知数学项，未知项不改变槽位推断。
    Decoder,
    /// raw 退役字段扫描：类别无法确定的单元素操作数流按 `Row` 继续检查。
    RawScan,
    /// raw 能力扫描：按流中首个数学内容宽松推断，无法确定时按 `Row`。
    RawCapability,
}

impl MathNodeSupportPolicy {
    const DECODER: Self = Self {
        flows: MathFlowKindPolicy::Decoder,
    };
    const RAW_SCAN: Self = Self {
        flows: MathFlowKindPolicy::RawScan,
    };
    const RAW_CAPABILITY: Self = Self {
        flows: MathFlowKindPolicy::RawCapability,
    };
}

impl super::MathNodeKind {
    /// 判定单个原始数学节点是否为类型解码器可解释的形态。
    ///
    /// 返回值是该节点的类别；`None` 表示当前 schema 的节点字段或结构无效。未知 kind
    /// 在此调用前由 raw JSON 边界识别为未知节点类型。与 raw 扫描共用属性/结构实现，
    /// 但保留类型解码器的既有输入边界（见 `MathNodeSupportPolicy`）。
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
        let text = match object.get("text") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text.as_str()),
            Some(_) => return None,
        };
        (math_node_attributes_supported(kind, text, &attributes)
            && math_node_structure_supported(parent, kind, text, &child_flows))
        .then_some(kind)
    }
}

fn supported_math_node(value: &Value, parent: Option<super::MathNodeKind>) -> MathTreeSupport {
    let Some(node) = value.as_object() else {
        return MathTreeSupport::Invalid;
    };
    let Some(kind_name) = node.get("kind").and_then(Value::as_str) else {
        return MathTreeSupport::Invalid;
    };
    let Ok(kind) =
        serde_json::from_value::<super::MathNodeKind>(Value::String(kind_name.to_owned()))
    else {
        return MathTreeSupport::FutureOpaque;
    };
    if !valid_node_id(node) {
        return MathTreeSupport::Invalid;
    }
    let Some(validated_kind) = super::MathNodeKind::supported_from_storage(node, parent) else {
        return MathTreeSupport::Invalid;
    };
    debug_assert_eq!(validated_kind, kind);
    let Some(children) = node.get("children") else {
        return MathTreeSupport::Known;
    };
    let Some(children) = children.as_array() else {
        return MathTreeSupport::Invalid;
    };
    children
        .iter()
        .fold(MathTreeSupport::Known, |state, child| {
            state.combine(supported_math_flow(child, Some(kind)))
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

/// 类型解码器：按类型验证器的规则取流中首个已知数学项；未知项按 Opaque 忽略。
fn decoder_flow_primary_kind(flow: &Value) -> Option<super::MathNodeKind> {
    let content = flow.as_object()?.get("content")?.as_array()?;
    let kind = content.iter().find_map(|item| {
        if item.get("type").and_then(Value::as_str) != Some("math") {
            return None;
        }
        item.get("kind").and_then(Value::as_str).and_then(|kind| {
            serde_json::from_value::<super::MathNodeKind>(Value::String(kind.to_owned())).ok()
        })
    });
    Some(match kind {
        Some(
            kind @ (super::MathNodeKind::Placeholder
            | super::MathNodeKind::Hline
            | super::MathNodeKind::Intertext),
        ) => kind,
        _ => super::MathNodeKind::Row,
    })
}

/// raw 退役字段扫描：类别无法确定的单元素流按 `Row` 继续检查。
fn raw_scan_flow_primary_kind(flow: &Value) -> Option<super::MathNodeKind> {
    let content = flow.as_object()?.get("content")?.as_array()?;
    if let [item] = content.as_slice()
        && let Some(item) = item.as_object()
        && item.get("type").and_then(Value::as_str) == Some("math")
    {
        let kind_name = item.get("kind")?.as_str()?;
        let kind =
            serde_json::from_value::<super::MathNodeKind>(Value::String(kind_name.to_owned())).ok();
        return Some(match kind {
            Some(
                kind @ (super::MathNodeKind::Placeholder
                | super::MathNodeKind::Hline
                | super::MathNodeKind::Intertext),
            ) => kind,
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

fn parse_table(value: Value) -> Result<Block, ModelError> {
    let mut object = value
        .as_object()
        .cloned()
        .ok_or_else(|| ModelError::InvalidDocument("table node must be an object".to_owned()))?;
    object.remove("type");
    let id = parse_id_field(&mut object)?;
    let rows = take_nodes(&mut object, "rows", parse_table_row)?;
    reject_unknown_table_fields(&object, "table")?;
    Ok(Block::Table(Table { id, rows }))
}

fn parse_table_row(value: Value) -> Result<TableRow, ModelError> {
    let mut object = value.as_object().cloned().ok_or_else(|| {
        ModelError::InvalidDocument("table row node must be an object".to_owned())
    })?;
    let id = parse_id_field(&mut object)?;
    let cells = take_nodes(&mut object, "cells", parse_table_cell)?;
    reject_unknown_table_fields(&object, "table row")?;
    Ok(TableRow { id, cells })
}

fn parse_table_cell(value: Value) -> Result<TableCell, ModelError> {
    let mut object = value.as_object().cloned().ok_or_else(|| {
        ModelError::InvalidDocument("table cell node must be an object".to_owned())
    })?;
    let id = parse_id_field(&mut object)?;
    let blocks = take_nodes(&mut object, "blocks", block_from_value)?;
    reject_unknown_table_fields(&object, "table cell")?;
    Ok(TableCell { id, blocks })
}

fn reject_unknown_table_fields(object: &Map<String, Value>, node: &str) -> Result<(), ModelError> {
    if object.is_empty() {
        return Ok(());
    }
    let fields = object.keys().cloned().collect::<Vec<_>>().join(", ");
    Err(ModelError::InvalidDocument(format!(
        "{node} has unknown fields: {fields}"
    )))
}

/// 表格按行、单元格与单元格块序列递归编码；未知字段在解码时已经拒绝。
fn table_to_value(table: &Table) -> Result<Value, ModelError> {
    let mut rows = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let mut cells = Vec::with_capacity(row.cells.len());
        for cell in &row.cells {
            let blocks = cell
                .blocks
                .iter()
                .map(block_to_value)
                .collect::<Result<Vec<_>, _>>()?;
            let mut cell_object = Map::new();
            cell_object.insert("id".to_owned(), Value::String(cell.id.0.clone()));
            cell_object.insert("blocks".to_owned(), Value::Array(blocks));
            cells.push(Value::Object(cell_object));
        }
        let mut row_object = Map::new();
        row_object.insert("id".to_owned(), Value::String(row.id.0.clone()));
        row_object.insert("cells".to_owned(), Value::Array(cells));
        rows.push(Value::Object(row_object));
    }
    let mut object = Map::new();
    object.insert("type".to_owned(), Value::String("table".to_owned()));
    object.insert("id".to_owned(), Value::String(table.id.0.clone()));
    object.insert("rows".to_owned(), Value::Array(rows));
    Ok(Value::Object(object))
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
        Block::Table(value) => table_to_value(value),
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
