//! 编解码系统剪贴板使用的 Caret 专用文档片段。
//!
//! 片段直接承载裁切后的规范 `Block` 序列，只额外保存传输所需的形状与首尾边界标记；
//! 这里不复制选区状态，也不建立第二份文档交换 AST。解码严格拒绝非当前 schema、
//! 未知字段、不透明节点和重复身份，保证粘贴前的片段可以被目标文档安全接纳。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use crate::{
    Align, Block, Document, InlineEnvironment, MathAlignRow, MathContent, MathFlow, MathTree,
    ModelError, NodeId, Paragraph, ParagraphContent, ParagraphLineSpacing, TextFlow,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 片段信封的格式标识。
const CLIPBOARD_FRAGMENT_FORMAT: &str = "caret.fragment";
/// 当前片段信封的 schema 版本。
const CLIPBOARD_FRAGMENT_VERSION: u32 = 1;

/// 系统剪贴板片段的内容形状。
#[derive(Debug, Clone, PartialEq)]
pub enum ClipboardFragmentShape {
    /// 单个裁切段落，只能作为行内内容插入段落。
    Inline,
    /// 按文档顺序排列的块序列，按共享宿主边界插入。
    Blocks,
}

/// 可经系统剪贴板传输的 Caret 文档片段。
///
/// `blocks` 是裁切后的规范块序列；首尾被选区截断的段落分别由
/// `start_clipped` 与 `end_clipped` 标记，插入时与目标段落的前缀/后缀合并。
#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardFragment {
    pub shape: ClipboardFragmentShape,
    pub start_clipped: bool,
    pub end_clipped: bool,
    pub blocks: Vec<Block>,
}

/// 反序列化信封；未知信封字段一律拒绝。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClipboardFragmentEnvelope {
    format: String,
    version: u32,
    shape: ClipboardFragmentShapeName,
    start_clipped: bool,
    end_clipped: bool,
    blocks: Vec<Block>,
}

/// 序列化信封；块序列借用片段自身，避免复制整段选区。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardFragmentEnvelopeRef<'a> {
    format: &'static str,
    version: u32,
    shape: ClipboardFragmentShapeName,
    start_clipped: bool,
    end_clipped: bool,
    blocks: &'a [Block],
}

/// wire format 中的片段形状名；当前只支持 `inline` 与 `blocks`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ClipboardFragmentShapeName {
    Inline,
    Blocks,
}

impl From<&ClipboardFragmentShape> for ClipboardFragmentShapeName {
    fn from(shape: &ClipboardFragmentShape) -> Self {
        match shape {
            ClipboardFragmentShape::Inline => Self::Inline,
            ClipboardFragmentShape::Blocks => Self::Blocks,
        }
    }
}

impl From<ClipboardFragmentShapeName> for ClipboardFragmentShape {
    fn from(shape: ClipboardFragmentShapeName) -> Self {
        match shape {
            ClipboardFragmentShapeName::Inline => Self::Inline,
            ClipboardFragmentShapeName::Blocks => Self::Blocks,
        }
    }
}

/// 把片段编码为 Caret 专用剪贴板 JSON 信封。
///
/// 编码前执行与解码相同的严格检查：形状与空内容规则、禁止不透明节点与未知字段、
/// 模型校验和身份唯一性。无法完整表达为当前 schema 的片段返回错误，不做静默裁剪。
pub fn encode_clipboard_fragment_json(fragment: &ClipboardFragment) -> Result<String, ModelError> {
    validate_clipboard_fragment(fragment)?;
    serde_json::to_string(&ClipboardFragmentEnvelopeRef {
        format: CLIPBOARD_FRAGMENT_FORMAT,
        version: CLIPBOARD_FRAGMENT_VERSION,
        shape: ClipboardFragmentShapeName::from(&fragment.shape),
        start_clipped: fragment.start_clipped,
        end_clipped: fragment.end_clipped,
        blocks: &fragment.blocks,
    })
    .map_err(|error| invalid_fragment(format!("failed to encode clipboard fragment: {error}")))
}

/// 从 Caret 专用剪贴板 JSON 信封严格解码片段。
///
/// 拒绝不匹配的 format/version、未知信封字段、未知节点字段、不透明节点或内容、
/// 非当前数学树 schema、空形状内容、重复身份和无法通过文档模型校验的节点。
/// 任何错误都返回 [`ModelError`]；调用方不得在错误后改用 text/plain 粘贴。
pub fn decode_clipboard_fragment_json(json: &str) -> Result<ClipboardFragment, ModelError> {
    let envelope: ClipboardFragmentEnvelope = serde_json::from_str(json)
        .map_err(|error| invalid_fragment(format!("malformed fragment JSON: {error}")))?;
    if envelope.format != CLIPBOARD_FRAGMENT_FORMAT {
        return Err(invalid_fragment(format!(
            "unsupported fragment format: {:?}",
            envelope.format
        )));
    }
    if envelope.version != CLIPBOARD_FRAGMENT_VERSION {
        return Err(invalid_fragment(format!(
            "unsupported fragment version: {}",
            envelope.version
        )));
    }
    let fragment = ClipboardFragment {
        shape: envelope.shape.into(),
        start_clipped: envelope.start_clipped,
        end_clipped: envelope.end_clipped,
        blocks: envelope.blocks,
    };
    validate_clipboard_fragment(&fragment)?;
    Ok(fragment)
}

/// 递归重写片段中的全部节点身份。
///
/// 覆盖块、块环境（含标题）、段落、行内环境、InlineMath、Equation/Align 及其
/// 数学树的 MathFlow/MathNode/MathAlignRow；Run、TextFlow 与 SoftBreak 在当前
/// 模型中不携带身份。不透明节点无法安全改写其原始 JSON，合法片段不会包含这类
/// 节点，因此保持原样。
pub fn regenerate_clipboard_ids(blocks: &mut [Block]) {
    for block in blocks {
        regenerate_block_ids(block);
    }
}

/// 判断给定身份是否出现在片段块序列的任意节点上。
///
/// 供调用方诊断与测试使用。不透明节点没有类型化身份，合法片段也不会包含
/// 这类节点，因此本函数不扫描其原始 JSON。
pub fn clipboard_fragment_contains_id(blocks: &[Block], id: &NodeId) -> bool {
    blocks.iter().any(|block| block_contains_id(block, id))
}

fn invalid_fragment(reason: impl std::fmt::Display) -> ModelError {
    ModelError::InvalidDocument(format!("invalid clipboard fragment: {reason}"))
}

fn unsupported_fragment_node(reason: impl std::fmt::Display) -> ModelError {
    ModelError::InvalidDocument(format!("unsupported clipboard fragment node: {reason}"))
}

fn validate_clipboard_fragment(fragment: &ClipboardFragment) -> Result<(), ModelError> {
    if fragment.blocks.is_empty() {
        return Err(invalid_fragment("fragment must contain at least one block"));
    }
    if fragment.shape == ClipboardFragmentShape::Inline {
        match fragment.blocks.as_slice() {
            [Block::Paragraph(paragraph)] => {
                if paragraph.inlines.is_empty() {
                    return Err(invalid_fragment(
                        "inline fragment paragraph must not be empty",
                    ));
                }
            }
            _ => {
                return Err(invalid_fragment(
                    "inline fragment must contain exactly one paragraph block",
                ));
            }
        }
    }
    validate_known_blocks(&fragment.blocks)?;
    validate_blocks_as_document(&fragment.blocks)
}

/// 用临时空白文档校验块序列的模型不变量。
///
/// 片段自身不携带文档主题上下文，因此使用 crate 内的结构校验入口：跳过只依赖
/// 目标文档主题的行内环境模板解析，其余身份唯一性、数学树 schema/属性/资源预算、
/// 语言标签、环境属性和嵌套深度等约束全部执行。
fn validate_blocks_as_document(blocks: &[Block]) -> Result<(), ModelError> {
    let mut document = Document::blank();
    document.body = blocks.to_vec();
    document.validate_for_safe_preview()
}

/// 递归拒绝类型化模型无法完整表达的节点与字段。
fn validate_known_blocks(blocks: &[Block]) -> Result<(), ModelError> {
    for block in blocks {
        validate_known_block(block)?;
    }
    Ok(())
}

fn validate_known_block(block: &Block) -> Result<(), ModelError> {
    match block {
        Block::Paragraph(paragraph) => validate_known_paragraph(paragraph),
        Block::Equation(equation) => {
            ensure_empty_extra("equation", &equation.extra)?;
            validate_known_math_tree(&equation.math_tree)
        }
        Block::Align(align) => validate_known_align(align),
        Block::Environment(environment) => {
            ensure_empty_extra("block environment", &environment.extra)?;
            if let Some(title) = &environment.title {
                validate_known_paragraph(title)?;
            }
            validate_known_blocks(&environment.children)
        }
        Block::Table(table) => {
            for row in &table.rows {
                for cell in &row.cells {
                    validate_known_blocks(&cell.blocks)?;
                }
            }
            Ok(())
        }
        Block::ExplicitBreak(explicit_break) => {
            ensure_empty_extra("explicit break", &explicit_break.extra)
        }
        Block::Opaque(_) => Err(unsupported_fragment_node("opaque block")),
    }
}

fn validate_known_paragraph(paragraph: &Paragraph) -> Result<(), ModelError> {
    ensure_empty_extra("paragraph", &paragraph.extra)?;
    if let Some(layout) = &paragraph.layout {
        ensure_empty_extra("paragraph layout", &layout.extra)?;
        if let Some(line_spacing) = &layout.line_spacing {
            validate_known_line_spacing(line_spacing)?;
        }
    }
    for content in &paragraph.inlines {
        validate_known_paragraph_content(content)?;
    }
    Ok(())
}

fn validate_known_line_spacing(line_spacing: &ParagraphLineSpacing) -> Result<(), ModelError> {
    let extra = match line_spacing {
        ParagraphLineSpacing::Natural { extra }
        | ParagraphLineSpacing::Multiple { extra, .. }
        | ParagraphLineSpacing::AtLeast { extra, .. }
        | ParagraphLineSpacing::Exactly { extra, .. } => extra,
    };
    ensure_empty_extra("paragraph line spacing", extra)
}

fn validate_known_paragraph_content(content: &ParagraphContent) -> Result<(), ModelError> {
    match content {
        ParagraphContent::Text(text) => validate_known_text_flow(text),
        ParagraphContent::Math(math) => {
            ensure_empty_extra("inline math", &math.extra)?;
            validate_known_math_tree(&math.math_tree)
        }
        ParagraphContent::Environment(environment) => {
            validate_known_inline_environment(environment, validate_known_paragraph_content)
        }
        ParagraphContent::SoftBreak(soft_break) => {
            ensure_empty_extra("soft break", &soft_break.extra)
        }
        ParagraphContent::Opaque(_) => Err(unsupported_fragment_node("opaque paragraph content")),
    }
}

fn validate_known_math_tree(tree: &MathTree) -> Result<(), ModelError> {
    ensure_empty_extra("math tree", &tree.extra)?;
    validate_known_math_flow(&tree.root)
}

fn validate_known_math_flow(flow: &MathFlow) -> Result<(), ModelError> {
    ensure_empty_extra("math flow", &flow.extra)?;
    for content in &flow.content {
        validate_known_math_content(content)?;
    }
    Ok(())
}

fn validate_known_math_content(content: &MathContent) -> Result<(), ModelError> {
    match content {
        MathContent::Math(node) => {
            ensure_empty_extra("math node", &node.extra)?;
            for child in &node.children {
                validate_known_math_flow(child)?;
            }
            Ok(())
        }
        MathContent::Text(text) => validate_known_text_flow(text),
        MathContent::Environment(environment) => {
            validate_known_inline_environment(environment, validate_known_math_content)
        }
        MathContent::SoftBreak(soft_break) => {
            ensure_empty_extra("math soft break", &soft_break.extra)
        }
        MathContent::Opaque(_) => Err(unsupported_fragment_node("opaque math content")),
    }
}

fn validate_known_align(align: &Align) -> Result<(), ModelError> {
    ensure_empty_extra("align", &align.extra)?;
    for row in &align.rows {
        validate_known_align_row(row)?;
    }
    Ok(())
}

fn validate_known_align_row(row: &MathAlignRow) -> Result<(), ModelError> {
    ensure_empty_extra("align row", &row.extra)?;
    for cell in &row.cells {
        validate_known_math_tree(cell)?;
    }
    Ok(())
}

fn validate_known_text_flow(flow: &TextFlow) -> Result<(), ModelError> {
    ensure_empty_extra("text flow", &flow.extra)?;
    for run in &flow.runs {
        ensure_empty_extra("run", &run.extra)?;
    }
    Ok(())
}

fn validate_known_inline_environment<Child>(
    environment: &InlineEnvironment<Child>,
    mut validate_child: impl FnMut(&Child) -> Result<(), ModelError>,
) -> Result<(), ModelError> {
    ensure_empty_extra("inline environment", &environment.extra)?;
    for child in &environment.children {
        validate_child(child)?;
    }
    Ok(())
}

fn ensure_empty_extra(node: &str, extra: &BTreeMap<String, Value>) -> Result<(), ModelError> {
    if extra.is_empty() {
        return Ok(());
    }
    let fields = extra.keys().cloned().collect::<Vec<_>>().join(", ");
    Err(unsupported_fragment_node(format!(
        "{node} has unknown fields: {fields}"
    )))
}

fn regenerate_block_ids(block: &mut Block) {
    match block {
        Block::Paragraph(paragraph) => regenerate_paragraph_ids(paragraph),
        Block::Equation(equation) => {
            equation.id = NodeId::generate();
            regenerate_math_tree_ids(&mut equation.math_tree);
        }
        Block::Align(align) => {
            align.id = NodeId::generate();
            for row in &mut align.rows {
                row.id = NodeId::generate();
                for cell in &mut row.cells {
                    regenerate_math_tree_ids(cell);
                }
            }
        }
        Block::Environment(environment) => {
            environment.id = NodeId::generate();
            if let Some(title) = &mut environment.title {
                regenerate_paragraph_ids(title);
            }
            for child in &mut environment.children {
                regenerate_block_ids(child);
            }
        }
        Block::Table(table) => {
            table.id = NodeId::generate();
            for row in &mut table.rows {
                row.id = NodeId::generate();
                for cell in &mut row.cells {
                    cell.id = NodeId::generate();
                    for child in &mut cell.blocks {
                        regenerate_block_ids(child);
                    }
                }
            }
        }
        Block::ExplicitBreak(_) | Block::Opaque(_) => {}
    }
}

fn regenerate_paragraph_ids(paragraph: &mut Paragraph) {
    paragraph.id = NodeId::generate();
    for content in &mut paragraph.inlines {
        regenerate_paragraph_content_ids(content);
    }
}

fn regenerate_paragraph_content_ids(content: &mut ParagraphContent) {
    match content {
        ParagraphContent::Math(math) => {
            math.id = NodeId::generate();
            regenerate_math_tree_ids(&mut math.math_tree);
        }
        ParagraphContent::Environment(environment) => {
            environment.id = NodeId::generate();
            for child in &mut environment.children {
                regenerate_paragraph_content_ids(child);
            }
        }
        ParagraphContent::Text(_)
        | ParagraphContent::SoftBreak(_)
        | ParagraphContent::Opaque(_) => {}
    }
}

fn regenerate_math_tree_ids(tree: &mut MathTree) {
    regenerate_math_flow_ids(&mut tree.root);
}

fn regenerate_math_flow_ids(flow: &mut MathFlow) {
    flow.id = NodeId::generate();
    for content in &mut flow.content {
        regenerate_math_content_ids(content);
    }
}

fn regenerate_math_content_ids(content: &mut MathContent) {
    match content {
        MathContent::Math(node) => {
            node.id = NodeId::generate();
            for child in &mut node.children {
                regenerate_math_flow_ids(child);
            }
        }
        MathContent::Environment(environment) => {
            environment.id = NodeId::generate();
            for child in &mut environment.children {
                regenerate_math_content_ids(child);
            }
        }
        MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
    }
}

fn block_contains_id(block: &Block, id: &NodeId) -> bool {
    match block {
        Block::Paragraph(paragraph) => paragraph_contains_id(paragraph, id),
        Block::Equation(equation) => {
            equation.id == *id || math_tree_contains_id(&equation.math_tree, id)
        }
        Block::Align(align) => {
            align.id == *id
                || align.rows.iter().any(|row| {
                    row.id == *id || row.cells.iter().any(|cell| math_tree_contains_id(cell, id))
                })
        }
        Block::Environment(environment) => {
            environment.id == *id
                || environment
                    .title
                    .as_ref()
                    .is_some_and(|title| paragraph_contains_id(title, id))
                || environment
                    .children
                    .iter()
                    .any(|child| block_contains_id(child, id))
        }
        Block::Table(table) => {
            table.id == *id
                || table.rows.iter().any(|row| {
                    row.id == *id
                        || row.cells.iter().any(|cell| {
                            cell.id == *id
                                || cell.blocks.iter().any(|child| block_contains_id(child, id))
                        })
                })
        }
        Block::ExplicitBreak(_) | Block::Opaque(_) => false,
    }
}

fn paragraph_contains_id(paragraph: &Paragraph, id: &NodeId) -> bool {
    paragraph.id == *id
        || paragraph
            .inlines
            .iter()
            .any(|content| paragraph_content_contains_id(content, id))
}

fn paragraph_content_contains_id(content: &ParagraphContent, id: &NodeId) -> bool {
    match content {
        ParagraphContent::Math(math) => {
            math.id == *id || math_tree_contains_id(&math.math_tree, id)
        }
        ParagraphContent::Environment(environment) => {
            environment.id == *id
                || environment
                    .children
                    .iter()
                    .any(|child| paragraph_content_contains_id(child, id))
        }
        ParagraphContent::Text(_)
        | ParagraphContent::SoftBreak(_)
        | ParagraphContent::Opaque(_) => false,
    }
}

fn math_tree_contains_id(tree: &MathTree, id: &NodeId) -> bool {
    math_flow_contains_id(&tree.root, id)
}

fn math_flow_contains_id(flow: &MathFlow, id: &NodeId) -> bool {
    flow.id == *id
        || flow
            .content
            .iter()
            .any(|content| math_content_contains_id(content, id))
}

fn math_content_contains_id(content: &MathContent, id: &NodeId) -> bool {
    match content {
        MathContent::Math(node) => {
            node.id == *id
                || node
                    .children
                    .iter()
                    .any(|child| math_flow_contains_id(child, id))
        }
        MathContent::Environment(environment) => {
            environment.id == *id
                || environment
                    .children
                    .iter()
                    .any(|child| math_content_contains_id(child, id))
        }
        MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BlockEnvironment, BlockEnvironmentKind, Equation, InlineEnvironmentKind, InlineMath,
        MATH_TREE_SCHEMA_VERSION, MathNode, MathNodeKind, OpaqueNode, ParagraphLayoutOverrides,
        Run, SoftBreak,
    };
    use serde_json::json;
    use std::collections::BTreeSet;

    fn id(name: &str) -> NodeId {
        NodeId::parse(name.to_owned())
            .unwrap_or_else(|error| panic!("test node id should be valid: {error}"))
    }

    fn run(text: &str) -> Run {
        Run {
            text: text.to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }

    fn text(runs: Vec<Run>) -> ParagraphContent {
        ParagraphContent::Text(TextFlow {
            runs,
            extra: BTreeMap::new(),
        })
    }

    fn paragraph(identifier: &str, inlines: Vec<ParagraphContent>) -> Block {
        let mut paragraph = Paragraph::empty(id(identifier));
        paragraph.inlines = inlines;
        Block::Paragraph(paragraph)
    }

    fn math_symbol_tree(identifier: &str) -> MathTree {
        MathTree {
            schema_version: MATH_TREE_SCHEMA_VERSION,
            root: MathFlow {
                id: id(&format!("{identifier}-flow")),
                content: vec![MathContent::Math(MathNode {
                    id: id(&format!("{identifier}-symbol")),
                    kind: MathNodeKind::Symbol,
                    text: Some("x".to_owned()),
                    attributes: BTreeMap::new(),
                    children: Vec::new(),
                    extra: BTreeMap::new(),
                })],
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        }
    }

    fn inline_math(identifier: &str) -> ParagraphContent {
        ParagraphContent::Math(InlineMath {
            id: id(&format!("{identifier}-inline")),
            math_tree: math_symbol_tree(identifier),
            extra: BTreeMap::new(),
        })
    }

    fn bold_environment(identifier: &str, definition_id: Option<&str>) -> ParagraphContent {
        ParagraphContent::Environment(InlineEnvironment {
            id: id(&format!("{identifier}-environment")),
            kind: InlineEnvironmentKind::Bold,
            definition_id: definition_id.map(str::to_owned),
            props: BTreeMap::new(),
            children: vec![text(vec![run("bold")])],
            extra: BTreeMap::new(),
        })
    }

    fn equation(identifier: &str) -> Block {
        Block::Equation(Equation {
            id: id(&format!("{identifier}-equation")),
            math_tree: math_symbol_tree(identifier),
            extra: BTreeMap::new(),
        })
    }

    fn align(identifier: &str) -> Block {
        Block::Align(Align {
            id: id(&format!("{identifier}-align")),
            rows: vec![MathAlignRow {
                id: id(&format!("{identifier}-align-row")),
                cells: vec![math_symbol_tree(&format!("{identifier}-cell"))],
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        })
    }

    fn environment(identifier: &str) -> Block {
        let mut title = Paragraph::empty(id(&format!("{identifier}-title")));
        title.inlines = vec![text(vec![run("Heading")])];
        Block::Environment(BlockEnvironment {
            id: id(&format!("{identifier}-environment")),
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: BTreeMap::new(),
            title: Some(title),
            children: vec![paragraph(
                &format!("{identifier}-child"),
                vec![text(vec![run("inside")])],
            )],
            extra: BTreeMap::new(),
        })
    }

    fn sample_blocks_fragment() -> ClipboardFragment {
        let mut styled_run = run("Hello ");
        styled_run.props.insert("underline".to_owned(), json!(true));
        ClipboardFragment {
            shape: ClipboardFragmentShape::Blocks,
            start_clipped: true,
            end_clipped: false,
            blocks: vec![
                paragraph(
                    "first-paragraph",
                    vec![text(vec![styled_run]), inline_math("inline-math")],
                ),
                paragraph(
                    "second-paragraph",
                    vec![bold_environment("bold", Some("body-bold/strong"))],
                ),
                equation("equation"),
                align("align"),
                environment("section"),
            ],
        }
    }

    fn sample_inline_fragment() -> ClipboardFragment {
        ClipboardFragment {
            shape: ClipboardFragmentShape::Inline,
            start_clipped: true,
            end_clipped: true,
            blocks: vec![paragraph(
                "inline-paragraph",
                vec![text(vec![run("clipped selection")])],
            )],
        }
    }

    fn valid_blocks_envelope() -> Value {
        json!({
            "format": "caret.fragment",
            "version": 1,
            "shape": "blocks",
            "startClipped": false,
            "endClipped": false,
            "blocks": [{
                "type": "paragraph",
                "id": "paragraph-one",
                "inlines": [{
                    "type": "text",
                    "runs": [{"text": "hello"}]
                }]
            }]
        })
    }

    fn decode_value(value: &Value) -> Result<ClipboardFragment, ModelError> {
        decode_clipboard_fragment_json(&value.to_string())
    }

    fn expect_decode_error(value: &Value) -> ModelError {
        match decode_value(value) {
            Ok(fragment) => panic!("expected decode to fail, got {fragment:?}"),
            Err(error) => error,
        }
    }

    fn collect_json_ids(value: &Value, ids: &mut Vec<String>) {
        match value {
            Value::Object(object) => {
                if let Some(Value::String(identifier)) = object.get("id") {
                    ids.push(identifier.clone());
                }
                for child in object.values() {
                    collect_json_ids(child, ids);
                }
            }
            Value::Array(values) => {
                for child in values {
                    collect_json_ids(child, ids);
                }
            }
            _ => {}
        }
    }

    fn fragment_json_ids(blocks: &[Block]) -> Vec<String> {
        let value = serde_json::to_value(blocks)
            .unwrap_or_else(|error| panic!("blocks should serialize: {error}"));
        let mut ids = Vec::new();
        collect_json_ids(&value, &mut ids);
        ids
    }

    #[test]
    fn blocks_fragment_round_trips_text_runs_environments_and_math() -> Result<(), ModelError> {
        let fragment = sample_blocks_fragment();
        let encoded = encode_clipboard_fragment_json(&fragment)?;
        let value: Value = serde_json::from_str(&encoded)
            .map_err(|error| ModelError::InvalidDocument(error.to_string()))?;
        assert_eq!(value["format"], "caret.fragment");
        assert_eq!(value["version"], 1);
        assert_eq!(value["shape"], "blocks");
        assert_eq!(value["startClipped"], true);
        assert_eq!(value["endClipped"], false);
        assert_eq!(
            value["blocks"].as_array().map(Vec::len).unwrap_or_default(),
            fragment.blocks.len()
        );

        let decoded = decode_clipboard_fragment_json(&encoded)?;
        assert_eq!(decoded, fragment);
        Ok(())
    }

    #[test]
    fn inline_fragment_round_trips_clipped_flags() -> Result<(), ModelError> {
        let fragment = sample_inline_fragment();
        let encoded = encode_clipboard_fragment_json(&fragment)?;
        let value: Value = serde_json::from_str(&encoded)
            .map_err(|error| ModelError::InvalidDocument(error.to_string()))?;
        assert_eq!(value["shape"], "inline");
        assert_eq!(value["startClipped"], true);
        assert_eq!(value["endClipped"], true);
        assert_eq!(decode_clipboard_fragment_json(&encoded)?, fragment);
        Ok(())
    }

    #[test]
    fn regenerate_replaces_every_identity_with_fresh_unique_ids() -> Result<(), ModelError> {
        let fragment = sample_blocks_fragment();
        let original = fragment_json_ids(&fragment.blocks);
        let mut blocks = fragment.blocks.clone();
        regenerate_clipboard_ids(&mut blocks);
        let regenerated = fragment_json_ids(&blocks);

        assert_eq!(original.len(), regenerated.len());
        assert!(
            original
                .iter()
                .all(|identifier| !regenerated.contains(identifier))
        );
        let unique = regenerated.iter().collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), regenerated.len());

        let regenerated_fragment = ClipboardFragment {
            shape: ClipboardFragmentShape::Blocks,
            start_clipped: true,
            end_clipped: false,
            blocks,
        };
        // 重映射后的片段仍完整满足编码的模型与身份约束。
        let encoded = encode_clipboard_fragment_json(&regenerated_fragment)?;
        assert_eq!(
            decode_clipboard_fragment_json(&encoded)?,
            regenerated_fragment
        );
        Ok(())
    }

    #[test]
    fn fragment_identity_lookup_finds_nested_nodes_only() {
        let fragment = sample_blocks_fragment();
        assert!(clipboard_fragment_contains_id(
            &fragment.blocks,
            &id("first-paragraph")
        ));
        assert!(clipboard_fragment_contains_id(
            &fragment.blocks,
            &id("bold-environment")
        ));
        assert!(clipboard_fragment_contains_id(
            &fragment.blocks,
            &id("equation-symbol")
        ));
        assert!(clipboard_fragment_contains_id(
            &fragment.blocks,
            &id("align-cell-flow")
        ));
        assert!(clipboard_fragment_contains_id(
            &fragment.blocks,
            &id("section-title")
        ));
        assert!(!clipboard_fragment_contains_id(
            &fragment.blocks,
            &id("missing-node")
        ));
    }

    #[test]
    fn strict_decode_rejects_wrong_envelope_format_and_version() {
        let mut wrong_format = valid_blocks_envelope();
        wrong_format["format"] = json!("caret.other");
        assert!(
            expect_decode_error(&wrong_format)
                .to_string()
                .contains("unsupported fragment format")
        );

        let mut wrong_version = valid_blocks_envelope();
        wrong_version["version"] = json!(2);
        assert!(
            expect_decode_error(&wrong_version)
                .to_string()
                .contains("unsupported fragment version")
        );

        let mut wrong_shape = valid_blocks_envelope();
        wrong_shape["shape"] = json!("math");
        assert!(
            expect_decode_error(&wrong_shape)
                .to_string()
                .contains("clipboard fragment")
        );
    }

    #[test]
    fn strict_decode_rejects_unknown_envelope_fields_and_malformed_json() {
        let mut envelope = valid_blocks_envelope();
        envelope["futureEnvelopeField"] = json!(true);
        assert!(decode_value(&envelope).is_err());

        assert!(decode_clipboard_fragment_json("{\"format\":").is_err());
        assert!(decode_clipboard_fragment_json("[]").is_err());
        assert!(decode_clipboard_fragment_json("").is_err());
    }

    #[test]
    fn strict_decode_rejects_unknown_node_fields_at_every_depth() {
        for mutate in [
            |envelope: &mut Value| {
                envelope["blocks"][0]["futureBlockField"] = json!(true);
            },
            |envelope: &mut Value| {
                envelope["blocks"][0]["inlines"][0]["futureFlowField"] = json!(true);
            },
            |envelope: &mut Value| {
                envelope["blocks"][0]["inlines"][0]["runs"][0]["futureRunField"] = json!(true);
            },
            |envelope: &mut Value| {
                envelope["blocks"][0]["layout"] = json!({"futureLayoutField": true});
            },
            |envelope: &mut Value| {
                envelope["blocks"][0]["layout"] =
                    json!({"lineSpacing": {"kind": "multiple", "percent": 120, "future": 1}});
            },
            |envelope: &mut Value| {
                envelope["blocks"][0]["inlines"] = json!([{"type": "softBreak", "future": 1}]);
            },
            |envelope: &mut Value| {
                envelope["blocks"][0] = json!({
                    "type": "equation",
                    "id": "equation",
                    "mathTree": {
                        "schemaVersion": MATH_TREE_SCHEMA_VERSION,
                        "root": {
                            "id": "root",
                            "content": [{
                                "type": "math",
                                "id": "symbol",
                                "kind": "symbol",
                                "text": "x",
                                "futureMathNodeField": 1
                            }]
                        }
                    }
                });
            },
            |envelope: &mut Value| {
                envelope["blocks"][0] = json!({
                    "type": "equation",
                    "id": "equation",
                    "mathTree": {
                        "schemaVersion": MATH_TREE_SCHEMA_VERSION,
                        "root": {"id": "root", "futureMathFlowField": 1, "content": []}
                    }
                });
            },
            |envelope: &mut Value| {
                envelope["blocks"][0] = json!({
                    "type": "environment",
                    "scope": "block",
                    "kind": "section",
                    "id": "environment",
                    "futureEnvironmentField": 1,
                    "children": []
                });
            },
        ] {
            let mut envelope = valid_blocks_envelope();
            mutate(&mut envelope);
            let error = expect_decode_error(&envelope);
            assert!(
                error.to_string().contains("unknown fields"),
                "unexpected error for unknown node field: {error}"
            );
        }
    }

    #[test]
    fn strict_decode_rejects_opaque_blocks_and_contents() {
        // 未知块类型保留为 Opaque。
        let mut envelope = valid_blocks_envelope();
        envelope["blocks"] = json!([{"type": "futureBlock", "id": "future"}]);
        assert!(
            expect_decode_error(&envelope)
                .to_string()
                .contains("opaque block")
        );

        // 旧式/未知文本载体保留为 Opaque paragraph content。
        let mut envelope = valid_blocks_envelope();
        envelope["blocks"][0]["inlines"] = json!([{"type": "text", "text": "legacy"}]);
        assert!(
            expect_decode_error(&envelope)
                .to_string()
                .contains("opaque paragraph content")
        );

        // 未知数学节点类别使整个行内数学载体保留为 Opaque。
        let mut envelope = valid_blocks_envelope();
        envelope["blocks"][0]["inlines"] = json!([{
            "type": "inlineMath",
            "id": "inline",
            "mathTree": {
                "schemaVersion": MATH_TREE_SCHEMA_VERSION,
                "root": {
                    "id": "root",
                    "content": [{
                        "type": "math",
                        "id": "future-node",
                        "kind": "futureKind",
                        "children": []
                    }]
                }
            }
        }]);
        assert!(
            expect_decode_error(&envelope)
                .to_string()
                .contains("opaque paragraph content")
        );

        // 未知 kind 的行内环境同样保留为 Opaque。
        let mut envelope = valid_blocks_envelope();
        envelope["blocks"][0]["inlines"] = json!([{
            "type": "environment",
            "scope": "inline",
            "kind": "futureStyle",
            "id": "future-style",
            "children": []
        }]);
        assert!(
            expect_decode_error(&envelope)
                .to_string()
                .contains("opaque paragraph content")
        );
    }

    #[test]
    fn strict_decode_rejects_future_math_tree_schema_versions() {
        for version in [
            u64::from(MATH_TREE_SCHEMA_VERSION) + 1,
            u64::from(MATH_TREE_SCHEMA_VERSION) - 1,
        ] {
            let mut envelope = valid_blocks_envelope();
            envelope["blocks"][0] = json!({
                "type": "equation",
                "id": "equation",
                "mathTree": {
                    "schemaVersion": version,
                    "root": {"id": "root", "content": []}
                }
            });
            assert!(decode_value(&envelope).is_err());
        }
    }

    #[test]
    fn strict_decode_rejects_duplicate_and_invalid_attributes() {
        let mut duplicate_ids = valid_blocks_envelope();
        duplicate_ids["blocks"] = json!([
            {"type": "paragraph", "id": "duplicate", "inlines": []},
            {"type": "paragraph", "id": "duplicate", "inlines": []}
        ]);
        assert!(matches!(
            decode_value(&duplicate_ids),
            Err(ModelError::DuplicateNodeId(identifier)) if identifier == "duplicate"
        ));

        // 已知数学节点带非法属性：类型解码阶段即拒绝。
        let mut invalid_math_attributes = valid_blocks_envelope();
        invalid_math_attributes["blocks"][0] = json!({
            "type": "equation",
            "id": "equation",
            "mathTree": {
                "schemaVersion": MATH_TREE_SCHEMA_VERSION,
                "root": {
                    "id": "root",
                    "content": [{
                        "type": "math",
                        "id": "fraction",
                        "kind": "fraction",
                        "attributes": {"style": "bogus"},
                        "children": [
                            {"id": "numerator", "content": []},
                            {"id": "denominator", "content": []}
                        ]
                    }]
                }
            }
        });
        assert!(decode_value(&invalid_math_attributes).is_err());

        // 结构合法但模型非法：文档校验拒绝直接段落格式。
        let mut invalid_model = valid_blocks_envelope();
        invalid_model["blocks"][0]["props"] = json!({"fontFamily": "Ignored"});
        assert!(matches!(
            decode_value(&invalid_model),
            Err(ModelError::ForbiddenDirectFormat { .. })
        ));
    }

    #[test]
    fn strict_decode_rejects_invalid_inline_shapes_and_empty_payloads() {
        let mut empty_blocks = valid_blocks_envelope();
        empty_blocks["blocks"] = json!([]);
        assert!(
            expect_decode_error(&empty_blocks)
                .to_string()
                .contains("at least one block")
        );

        let mut inline_two_paragraphs = valid_blocks_envelope();
        inline_two_paragraphs["shape"] = json!("inline");
        inline_two_paragraphs["blocks"] = json!([
            {"type": "paragraph", "id": "first", "inlines": []},
            {"type": "paragraph", "id": "second", "inlines": []}
        ]);
        assert!(
            expect_decode_error(&inline_two_paragraphs)
                .to_string()
                .contains("exactly one paragraph block")
        );

        let mut inline_equation = valid_blocks_envelope();
        inline_equation["shape"] = json!("inline");
        inline_equation["blocks"] = json!([{
            "type": "equation",
            "id": "equation",
            "mathTree": {
                "schemaVersion": MATH_TREE_SCHEMA_VERSION,
                "root": {"id": "root", "content": []}
            }
        }]);
        assert!(
            expect_decode_error(&inline_equation)
                .to_string()
                .contains("exactly one paragraph block")
        );

        let mut inline_empty_paragraph = valid_blocks_envelope();
        inline_empty_paragraph["shape"] = json!("inline");
        inline_empty_paragraph["blocks"] =
            json!([{"type": "paragraph", "id": "empty", "inlines": []}]);
        assert!(
            expect_decode_error(&inline_empty_paragraph)
                .to_string()
                .contains("must not be empty")
        );
    }

    #[test]
    fn encode_rejects_fragments_that_cannot_be_represented() {
        let opaque = ClipboardFragment {
            shape: ClipboardFragmentShape::Blocks,
            start_clipped: false,
            end_clipped: false,
            blocks: vec![Block::Opaque(OpaqueNode {
                raw: json!({"type": "futureBlock"}),
            })],
        };
        assert!(encode_clipboard_fragment_json(&opaque).is_err());

        let mut extra = sample_blocks_fragment();
        extra.blocks[0] = {
            let mut paragraph = Paragraph::empty(id("extra-paragraph"));
            paragraph
                .extra
                .insert("futureField".to_owned(), json!(true));
            Block::Paragraph(paragraph)
        };
        assert!(encode_clipboard_fragment_json(&extra).is_err());

        let empty = ClipboardFragment {
            shape: ClipboardFragmentShape::Blocks,
            start_clipped: false,
            end_clipped: false,
            blocks: Vec::new(),
        };
        assert!(encode_clipboard_fragment_json(&empty).is_err());

        let inline_equation = ClipboardFragment {
            shape: ClipboardFragmentShape::Inline,
            start_clipped: false,
            end_clipped: false,
            blocks: vec![equation("inline-equation")],
        };
        assert!(encode_clipboard_fragment_json(&inline_equation).is_err());

        // 程序化构造的未来数学树 schema 也由模型校验拒绝。
        let mut future_schema = sample_blocks_fragment();
        if let Block::Equation(equation) = &mut future_schema.blocks[2] {
            equation.math_tree.schema_version = MATH_TREE_SCHEMA_VERSION + 1;
        }
        assert!(encode_clipboard_fragment_json(&future_schema).is_err());
    }

    #[test]
    fn strict_decode_accepts_soft_break_and_block_environment_nodes() -> Result<(), ModelError> {
        let mut envelope = valid_blocks_envelope();
        envelope["blocks"] = json!([
            {
                "type": "paragraph",
                "id": "paragraph-with-break",
                "inlines": [{"type": "softBreak"}]
            },
            {
                "type": "environment",
                "scope": "block",
                "kind": "section",
                "id": "section",
                "title": {
                    "id": "title",
                    "inlines": [{"type": "text", "runs": [{"text": "Title"}]}]
                },
                "children": [{
                    "type": "paragraph",
                    "id": "child",
                    "inlines": [{"type": "text", "runs": [{"text": "Child"}]}]
                }]
            }
        ]);
        let fragment = decode_value(&envelope)?;
        assert_eq!(fragment.shape, ClipboardFragmentShape::Blocks);
        let encoded = encode_clipboard_fragment_json(&fragment)?;
        assert_eq!(decode_clipboard_fragment_json(&encoded)?, fragment);
        Ok(())
    }

    #[test]
    fn soft_break_nodes_have_no_identity() -> Result<(), Box<dyn std::error::Error>> {
        let fragment = ClipboardFragment {
            shape: ClipboardFragmentShape::Blocks,
            start_clipped: false,
            end_clipped: false,
            blocks: vec![paragraph(
                "soft-break-paragraph",
                vec![ParagraphContent::SoftBreak(SoftBreak {
                    extra: BTreeMap::new(),
                })],
            )],
        };
        let encoded = encode_clipboard_fragment_json(&fragment)?;
        let value: Value = serde_json::from_str(&encoded)?;
        assert!(value["blocks"][0]["inlines"][0].get("id").is_none());

        let original = fragment_json_ids(&fragment.blocks);
        let mut blocks = fragment.blocks.clone();
        regenerate_clipboard_ids(&mut blocks);
        let regenerated = fragment_json_ids(&blocks);
        assert_eq!(original.len(), 1);
        assert_eq!(regenerated.len(), original.len());
        assert!(
            original
                .iter()
                .all(|identifier| !regenerated.contains(identifier))
        );
        assert_eq!(decode_clipboard_fragment_json(&encoded)?, fragment);
        Ok(())
    }

    #[test]
    fn paragraph_layout_overrides_round_trip() -> Result<(), ModelError> {
        let mut blocks = vec![paragraph(
            "layout-paragraph",
            vec![text(vec![run("layout")])],
        )];
        if let Block::Paragraph(paragraph) = &mut blocks[0] {
            paragraph.layout = Some(ParagraphLayoutOverrides::default());
        }
        let fragment = ClipboardFragment {
            shape: ClipboardFragmentShape::Blocks,
            start_clipped: false,
            end_clipped: false,
            blocks,
        };
        let encoded = encode_clipboard_fragment_json(&fragment)?;
        assert_eq!(decode_clipboard_fragment_json(&encoded)?, fragment);
        Ok(())
    }
}
