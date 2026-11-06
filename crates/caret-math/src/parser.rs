//! LaTeX 数学子集解析器，直接构造 `caret-doc` 数学树。
//!
//! 解析过程只做一次自顶向下扫描：数学模式跳过空白与 `%` 注释，文本模式逐字符
//! 保留空格。所有节点记录相对输入的 UTF-8 字节范围；源码空花括号产生的占位节点记录花括号范围，
//! 由结构补齐的占位节点没有范围。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

use caret_doc::{
    Align, InlineEnvironment, InlineEnvironmentKind, MATH_TREE_SCHEMA_VERSION, MAX_MATH_TREE_DEPTH,
    MAX_MATH_TREE_NODES, MathAlignRow, MathContent, MathFlow, MathTree, NodeId, Run, TextFlow,
};
use serde_json::Value;

use crate::backend::BackendNodeKind as MathNodeKind;
use crate::projection::AlignSourceRanges;
use crate::symbols::{
    NAMED_COLORS, accent_attribute, encode_delimiter, literal_class, named_symbol,
    text_operator_limits,
};
use crate::{
    MAX_MATH_SOURCE_BYTES, MathInputKind, MathInputStatus, MathParseError, MathProjection,
    ParsedMath,
};
use crate::{
    MathContentSourceMap, MathFlowSourceMap, MathRangeSourceMap, MathTextSourceMap,
    MathTextSourceSegment, MathTreeSourceMap,
};

type ParseResult<T> = Result<T, MathParseError>;

/// LaTeX parser 暂存用节点；解析完成后立即降为持久化内容流模型。
#[derive(Debug, Clone)]
struct MathNode {
    id: NodeId,
    kind: MathNodeKind,
    text: Option<String>,
    attributes: BTreeMap<String, String>,
    source_range: Option<Range<usize>>,
    children: Vec<MathNode>,
    extra: BTreeMap<String, Value>,
}

/// 递归下降的嵌套上限。文档树深度另行检查，这里只防止栈被恶意输入耗尽。
const MAX_RECURSION: usize = MAX_MATH_TREE_DEPTH * 4;

/// 由编号系统负责、公式内拒绝的命令。
const NUMBERING_COMMANDS: &[&str] = &["label", "tag", "ref", "eqref", "nonumber", "notag"];
/// 由编号系统负责、公式内拒绝的环境。
const NUMBERING_ENVIRONMENTS: &[&str] = &[
    "align",
    "align*",
    "equation",
    "equation*",
    "gather",
    "gather*",
    "multline",
    "multline*",
    "flalign",
    "flalign*",
    "alignat",
    "alignat*",
];
/// 公式内支持的矩阵类环境，名称即文档属性 `style`。
const MATRIX_ENVIRONMENTS: &[&str] = &[
    "matrix", "pmatrix", "bmatrix", "Bmatrix", "vmatrix", "Vmatrix", "cases", "array", "aligned",
    "split",
];

/// 把输入解析为目标对象类型的文档数学树。
pub fn parse(source: &str, kind: MathInputKind) -> ParseResult<ParsedMath> {
    parse_projection_with_source(source, kind, Cow::Borrowed(source), false)
        .map(MathProjection::into_parsed)
}

/// 解析源码并把它与树及原始字节映射一起返回。
///
/// 本函数会复制并由投影持有源码；超过源码字节预算或语法不受支持时返回解析错误。
/// 位置映射由投影独立持有，不写入返回的文档树。
pub fn parse_projection(source: &str, kind: MathInputKind) -> ParseResult<MathProjection<'static>> {
    if source.len() > MAX_MATH_SOURCE_BYTES {
        return Err(MathParseError::InputTooLong {
            len: source.len(),
            max: MAX_MATH_SOURCE_BYTES,
        });
    }
    parse_projection_with_source(source, kind, Cow::Owned(source.to_owned()), true)
}

fn parse_projection_with_source<'source>(
    source: &str,
    kind: MathInputKind,
    projection_source: Cow<'source, str>,
    capture_source_map: bool,
) -> ParseResult<MathProjection<'source>> {
    if source.len() > MAX_MATH_SOURCE_BYTES {
        return Err(MathParseError::InputTooLong {
            len: source.len(),
            max: MAX_MATH_SOURCE_BYTES,
        });
    }
    let body = strip_fences(source, kind)?;
    let mut parser = Parser::new(source, body.clone());
    match kind {
        MathInputKind::Inline | MathInputKind::Equation => {
            let (items, _) = parser.parse_list(Stops::NONE)?;
            let root = match items.len() {
                1 => items
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| parser.row(Vec::new(), None)),
                _ => {
                    let range = Some(0..source.len());
                    parser.row(items, range)
                }
            };
            check_depth(&root)?;
            let (root, root_map) = parser_root_to_flow(root, capture_source_map, 0, source)
                .ok_or(MathParseError::InvalidSourceMap)?;
            let tree = MathTree {
                schema_version: MATH_TREE_SCHEMA_VERSION,
                root,
                extra: BTreeMap::new(),
            };
            tree.validate().map_err(invalid_tree)?;
            let source_map = root_map
                .map(|root| MathTreeSourceMap::from_parser_tree(root, &tree, capture_source_map));
            MathProjection::tree(projection_source, kind, body, tree, source_map)
                .ok_or(MathParseError::InvalidSourceMap)
        }
        MathInputKind::Align => {
            parse_align(&mut parser, projection_source, body, capture_source_map)
        }
    }
}

/// 输入框实时分类。尾部未写完的控制词（如 `\fra`）视为可继续输入。
pub fn classify_input(source: &str, kind: MathInputKind) -> MathInputStatus {
    match parse(source, kind) {
        Ok(_) if trailing_control_word_start(source).is_some() => MathInputStatus::Incomplete,
        Ok(_) => MathInputStatus::Complete,
        Err(error) if error.is_incomplete() => MathInputStatus::Incomplete,
        Err(MathParseError::Parse { .. }) if trailing_control_word_is_viable(source, kind) => {
            MathInputStatus::Incomplete
        }
        Err(_) => MathInputStatus::Invalid,
    }
}

/// 纯文本层面判断输入是否停在未闭合的花括号、孤立反斜杠或未写完的控制词上。
pub fn is_incomplete_input_prefix(source: &str) -> bool {
    let mut brace_depth = 0usize;
    let mut backslashes = 0usize;
    for character in source.chars() {
        if character == '\\' {
            backslashes = backslashes.saturating_add(1);
            continue;
        }
        if backslashes.is_multiple_of(2) {
            match character {
                '{' => brace_depth = brace_depth.saturating_add(1),
                '}' if brace_depth > 0 => brace_depth -= 1,
                _ => {}
            }
        }
        backslashes = 0;
    }
    brace_depth > 0
        || !backslashes.is_multiple_of(2)
        || trailing_control_word_start(source).is_some()
}

/// 尾部控制词（反斜杠后全是 ASCII 字母）的反斜杠位置。
fn trailing_control_word_start(source: &str) -> Option<usize> {
    let start = source.rfind('\\')?;
    let word = &source[start + 1..];
    if word.is_empty() || !word.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let preceding = source[..start]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count();
    preceding.is_multiple_of(2).then_some(start)
}

/// 去掉尾部控制词后，前缀本身仍可继续输入时，尾部控制词才算可补全。
fn trailing_control_word_is_viable(source: &str, kind: MathInputKind) -> bool {
    let Some(start) = trailing_control_word_start(source) else {
        return false;
    };
    let prefix = &source[..start];
    match parse(prefix, kind) {
        Ok(_) => true,
        Err(error) => error.is_incomplete(),
    }
}

fn invalid_tree(error: caret_doc::ModelError) -> MathParseError {
    MathParseError::Parse {
        message: format!("invalid math tree: {error}"),
        offset: 0,
        incomplete: false,
    }
}

fn check_depth(root: &MathNode) -> ParseResult<()> {
    let mut stack = vec![(root, 0usize)];
    while let Some((node, depth)) = stack.pop() {
        if depth >= MAX_MATH_TREE_DEPTH {
            return Err(MathParseError::InputTooDeep {
                max: MAX_MATH_TREE_DEPTH,
            });
        }
        stack.extend(node.children.iter().map(|child| (child, depth + 1)));
    }
    Ok(())
}

/// 按对象类型剥离外层定界符，返回内容的字节范围。
fn strip_fences(source: &str, kind: MathInputKind) -> ParseResult<Range<usize>> {
    let leading = source.len() - source.trim_start().len();
    let trimmed = source.trim();
    let end = leading + trimmed.len();
    let fence = [("$$", "$$"), ("\\[", "\\]"), ("\\(", "\\)"), ("$", "$")]
        .into_iter()
        .find(|(open, _)| trimmed.starts_with(open));
    let Some((open, close)) = fence else {
        return Ok(0..source.len());
    };
    let display = matches!(open, "$$" | "\\[");
    let expected_display = !matches!(kind, MathInputKind::Inline);
    if display != expected_display {
        return Err(MathParseError::InputKindMismatch { expected: kind });
    }
    let inner_start = leading + open.len();
    if trimmed.len() < open.len() + close.len() || !trimmed.ends_with(close) {
        return Err(MathParseError::Parse {
            message: format!("missing closing `{close}`"),
            offset: end,
            incomplete: true,
        });
    }
    Ok(inner_start..end - close.len())
}

pub(super) fn input_body_range(source: &str, kind: MathInputKind) -> ParseResult<Range<usize>> {
    strip_fences(source, kind)
}

pub(super) fn input_body_range_for_edit(
    source: &str,
    kind: MathInputKind,
) -> ParseResult<Range<usize>> {
    match strip_fences(source, kind) {
        Ok(range) => Ok(range),
        Err(MathParseError::Parse {
            incomplete: true, ..
        }) => {
            let leading = source.len() - source.trim_start().len();
            let trimmed = &source[leading..];
            let opener = ["$$", "\\[", "\\(", "$"]
                .into_iter()
                .find(|opener| trimmed.starts_with(opener));
            let Some(opener) = opener else {
                return Ok(0..source.len());
            };
            let is_display = matches!(opener, "$$" | "\\[");
            if is_display != !matches!(kind, MathInputKind::Inline) {
                return Err(MathParseError::InputKindMismatch { expected: kind });
            }
            // 源码在定界符未闭合时暂时按原始文本编辑，允许光标和删除键修复开头。
            Ok(0..source.len())
        }
        Err(error) => Err(error),
    }
}

fn parse_align<'source>(
    parser: &mut Parser<'_>,
    source: Cow<'source, str>,
    body: Range<usize>,
    capture_source_map: bool,
) -> ParseResult<MathProjection<'source>> {
    parser.skip_space();
    let env_start = parser.pos;
    let is_aligned = parser.peek_command() == Some("begin") && {
        let saved = parser.pos;
        parser.read_command();
        let name = parser.read_raw_group();
        parser.pos = saved;
        matches!(name.as_deref(), Ok("aligned"))
    };
    if !is_aligned {
        // 先让解析器报告编号环境等更具体的错误。
        parser.parse_list(Stops::NONE)?;
        return Err(MathParseError::InputKindMismatch {
            expected: MathInputKind::Align,
        });
    }
    parser.read_command();
    parser.read_raw_group()?;
    let environment_content_start = parser.pos;
    let parsed_rows = parser.parse_matrix_rows("aligned", true)?;
    let environment_end_start = parsed_rows.environment_end_start;
    let environment_content_end = parsed_rows.environment_content_end;
    let environment_end = parsed_rows.environment_end;
    let mut rows = parsed_rows.rows;
    parser.skip_space();
    if parser.pos < body.end {
        return Err(MathParseError::InputKindMismatch {
            expected: MathInputKind::Align,
        });
    }
    if rows.is_empty() {
        let placeholder = parser.placeholder();
        rows.push(MatrixRow::Cells {
            cells: vec![(
                placeholder,
                environment_end_start..environment_end_start,
                environment_content_start..environment_end_start,
            )],
            range: environment_end_start..environment_end_start,
        });
    }
    let mut align_rows = Vec::new();
    let mut cell_ranges = Vec::new();
    let mut cell_navigation_ranges = Vec::new();
    let mut cell_source_maps = Vec::new();
    for row in rows {
        let MatrixRow::Cells { cells, .. } = row else {
            return Err(parser.error_at(
                env_start,
                "aligned rows cannot contain \\hline or \\intertext",
            ));
        };
        let mut trees = Vec::new();
        let mut row_ranges = Vec::new();
        let mut row_navigation_ranges = Vec::new();
        let mut row_source_maps = Vec::new();
        for (mut node, span, navigation_range) in cells {
            rebase(&mut node, span.start);
            check_depth(&node)?;
            let (root, root_map) =
                parser_root_to_flow(node, capture_source_map, span.start, source.as_ref())
                    .ok_or(MathParseError::InvalidSourceMap)?;
            let tree = MathTree {
                schema_version: MATH_TREE_SCHEMA_VERSION,
                root,
                extra: BTreeMap::new(),
            };
            tree.validate().map_err(invalid_tree)?;
            trees.push(tree);
            if let Some(root_map) = root_map {
                let tree = trees.last().ok_or(MathParseError::InvalidSourceMap)?;
                row_source_maps.push(MathTreeSourceMap::from_parser_tree(
                    root_map,
                    tree,
                    capture_source_map,
                ));
            }
            row_ranges.push(span);
            row_navigation_ranges.push(navigation_range);
        }
        cell_ranges.push(row_ranges);
        cell_navigation_ranges.push(row_navigation_ranges);
        if capture_source_map {
            cell_source_maps.push(row_source_maps);
        }
        align_rows.push(MathAlignRow {
            id: NodeId::generate(),
            cells: trees,
            extra: BTreeMap::new(),
        });
    }
    let align = Align {
        id: NodeId::generate(),
        rows: align_rows,
        extra: BTreeMap::new(),
    };
    align.validate().map_err(invalid_tree)?;
    MathProjection::align(
        source,
        body,
        align,
        AlignSourceRanges {
            environment: env_start..environment_end,
            content: environment_content_start..environment_content_end,
            cells: cell_ranges,
            navigation: cell_navigation_ranges,
        },
        capture_source_map.then_some(cell_source_maps),
    )
    .ok_or(MathParseError::InvalidSourceMap)
}

fn parser_root_to_flow(
    root: MathNode,
    capture_source_map: bool,
    source_offset: usize,
    source: &str,
) -> Option<(MathFlow, Option<MathFlowSourceMap>)> {
    if root.kind == MathNodeKind::Row {
        return parser_row_to_flow(root, capture_source_map, source_offset, source);
    }
    let id = NodeId::generate();
    let mut content = Vec::new();
    let mut mapped_content = capture_source_map.then(Vec::new);
    let mut text_state = None;
    append_parser_node(
        &mut content,
        &mut mapped_content,
        root,
        source_offset,
        source,
        &mut text_state,
    )?;
    let flow = MathFlow {
        id,
        content,
        extra: BTreeMap::new(),
    };
    let flow_map = mapped_content.map(|content| {
        MathFlowSourceMap::from_parser_parts(MathRangeSourceMap::default(), content)
    });
    Some((flow, flow_map))
}

fn parser_slot_to_flow(
    node: MathNode,
    capture_source_map: bool,
    source_offset: usize,
    source: &str,
) -> Option<(MathFlow, Option<MathFlowSourceMap>)> {
    if node.kind == MathNodeKind::Row {
        return parser_row_to_flow(node, capture_source_map, source_offset, source);
    }
    let source_range = node.source_range.clone();
    let id = NodeId::generate();
    let mut content = Vec::new();
    let mut mapped_content = capture_source_map.then(Vec::new);
    let mut text_state = None;
    append_parser_node(
        &mut content,
        &mut mapped_content,
        node,
        source_offset,
        source,
        &mut text_state,
    )?;
    let flow = MathFlow {
        id,
        content,
        extra: BTreeMap::new(),
    };
    let flow_map = if let Some(content) = mapped_content {
        Some(MathFlowSourceMap::from_parser_parts(
            map_range(source_range.as_ref(), source_offset)?,
            content,
        ))
    } else {
        None
    };
    Some((flow, flow_map))
}

fn parser_row_to_flow(
    row: MathNode,
    capture_source_map: bool,
    source_offset: usize,
    source: &str,
) -> Option<(MathFlow, Option<MathFlowSourceMap>)> {
    let mut content = Vec::new();
    let mut mapped_content = capture_source_map.then(Vec::new);
    let mut text_state = None;
    for node in row.children {
        append_parser_node(
            &mut content,
            &mut mapped_content,
            node,
            source_offset,
            source,
            &mut text_state,
        )?;
    }
    let range = row.source_range.clone();
    let flow = MathFlow {
        id: row.id,
        content,
        extra: row.extra,
    };
    let flow_map = if let Some(content) = mapped_content {
        Some(MathFlowSourceMap::from_parser_parts(
            map_range(range.as_ref(), source_offset)?,
            content,
        ))
    } else {
        None
    };
    Some((flow, flow_map))
}

fn append_parser_node(
    content: &mut Vec<MathContent>,
    mapped_content: &mut Option<Vec<MathContentSourceMap>>,
    node: MathNode,
    source_offset: usize,
    source: &str,
    text_state: &mut Option<(usize, usize)>,
) -> Option<()> {
    match node.kind {
        MathNodeKind::Row => {
            for child in node.children {
                append_parser_node(
                    content,
                    mapped_content,
                    child,
                    source_offset,
                    source,
                    text_state,
                )?;
            }
        }
        MathNodeKind::TextContent => {
            push_math_text(
                content,
                mapped_content,
                node.text.unwrap_or_default(),
                node.source_range,
                source_offset,
                source,
                text_state,
            )?;
        }
        MathNodeKind::TextSpace => {
            let text = match node.text.as_deref() {
                Some("no_break_space") => "\u{00a0}",
                Some("control_space") | Some("space") | None => " ",
                Some(_) => " ",
            };
            push_math_text(
                content,
                mapped_content,
                text.to_owned(),
                node.source_range,
                source_offset,
                source,
                text_state,
            )?;
        }
        MathNodeKind::TextSymbol => {
            let text = node.text.unwrap_or_default();
            push_math_text(
                content,
                mapped_content,
                text,
                node.source_range,
                source_offset,
                source,
                text_state,
            )?;
        }
        MathNodeKind::Text
            if node
                .attributes
                .get("style")
                .is_some_and(|style| style == "text") =>
        {
            if node.children.is_empty() {
                push_math_text(
                    content,
                    mapped_content,
                    String::new(),
                    node.source_range,
                    source_offset,
                    source,
                    text_state,
                )?;
            }
            for child in node.children {
                append_parser_node(
                    content,
                    mapped_content,
                    child,
                    source_offset,
                    source,
                    text_state,
                )?;
            }
        }
        MathNodeKind::Text
            if node
                .attributes
                .get("style")
                .is_some_and(|style| text_environment_style(style).is_some()) =>
        {
            append_text_environment(content, mapped_content, node, source_offset, source)?;
            *text_state = None;
        }
        MathNodeKind::TextStyle
            if node.attributes.get("style").is_some_and(|style| {
                matches!(
                    style.as_str(),
                    "rm" | "bf" | "it" | "md" | "up" | "sf" | "tt"
                )
            }) =>
        {
            append_text_environment(content, mapped_content, node, source_offset, source)?;
            *text_state = None;
        }
        _ => {
            let Some(persisted_kind) = node.kind.persisted() else {
                match node.kind {
                    MathNodeKind::TextContent | MathNodeKind::TextSymbol => {
                        push_math_text(
                            content,
                            mapped_content,
                            node.text.unwrap_or_default(),
                            node.source_range,
                            source_offset,
                            source,
                            text_state,
                        )?;
                    }
                    MathNodeKind::TextSpace => {
                        push_math_text(
                            content,
                            mapped_content,
                            " ".to_owned(),
                            node.source_range,
                            source_offset,
                            source,
                            text_state,
                        )?;
                    }
                    MathNodeKind::TextStyle => {
                        for child in node.children {
                            append_parser_node(
                                content,
                                mapped_content,
                                child,
                                source_offset,
                                source,
                                text_state,
                            )?;
                        }
                    }
                    _ => {}
                }
                return Some(());
            };
            *text_state = None;
            let mut children = Vec::with_capacity(node.children.len());
            let mut operands = Vec::with_capacity(node.children.len());
            for child in node.children {
                let (flow, source_map) =
                    parser_slot_to_flow(child, mapped_content.is_some(), source_offset, source)?;
                children.push(flow);
                if let Some(source_map) = source_map {
                    operands.push(source_map);
                }
            }
            let source_range = node.source_range.clone();
            let node_kind = node.kind;
            content.push(MathContent::Math(caret_doc::MathNode {
                id: node.id,
                kind: persisted_kind,
                text: node.text,
                attributes: node.attributes,
                children,
                extra: node.extra,
            }));
            if let Some(mapped_content) = mapped_content {
                mapped_content.push(MathContentSourceMap::Math {
                    range: map_range(source_range.as_ref(), source_offset)?,
                    operands,
                    placeholder: node_kind == MathNodeKind::Placeholder,
                });
            }
        }
    }
    Some(())
}

fn append_text_environment(
    content: &mut Vec<MathContent>,
    mapped_content: &mut Option<Vec<MathContentSourceMap>>,
    node: MathNode,
    source_offset: usize,
    source: &str,
) -> Option<()> {
    let style = node
        .attributes
        .get("style")
        .map(String::as_str)
        .unwrap_or_default();
    let Some((kind, props)) = text_environment_style(style) else {
        return Some(());
    };
    let mut children = Vec::new();
    let mut mapped_children = mapped_content.as_ref().map(|_| Vec::new());
    let mut text_state = None;
    if node.children.is_empty() {
        push_math_text(
            &mut children,
            &mut mapped_children,
            String::new(),
            node.source_range.clone(),
            source_offset,
            source,
            &mut text_state,
        )?;
    }
    for child in node.children {
        append_parser_node(
            &mut children,
            &mut mapped_children,
            child,
            source_offset,
            source,
            &mut text_state,
        )?;
    }
    let source_range = node.source_range.clone();
    content.push(MathContent::Environment(InlineEnvironment {
        id: node.id,
        kind,
        definition_id: None,
        props,
        children,
        extra: node.extra,
    }));
    if let Some(mapped_content) = mapped_content {
        mapped_content.push(MathContentSourceMap::Environment {
            range: map_range(source_range.as_ref(), source_offset)?,
            children: mapped_children.unwrap_or_default(),
        });
    }
    Some(())
}

fn text_environment_style(style: &str) -> Option<(InlineEnvironmentKind, BTreeMap<String, Value>)> {
    Some(match style {
        "rm" => (
            InlineEnvironmentKind::Font,
            BTreeMap::from([("fontFamily".to_owned(), Value::String("serif".to_owned()))]),
        ),
        "bf" => (InlineEnvironmentKind::Bold, BTreeMap::new()),
        "it" => (InlineEnvironmentKind::Italic, BTreeMap::new()),
        "md" => (
            InlineEnvironmentKind::Bold,
            BTreeMap::from([("enabled".to_owned(), Value::Bool(false))]),
        ),
        "up" => (
            InlineEnvironmentKind::Italic,
            BTreeMap::from([("enabled".to_owned(), Value::Bool(false))]),
        ),
        "sf" => (
            InlineEnvironmentKind::Font,
            BTreeMap::from([(
                "fontFamily".to_owned(),
                Value::String("sans-serif".to_owned()),
            )]),
        ),
        "tt" => (
            InlineEnvironmentKind::Font,
            BTreeMap::from([(
                "fontFamily".to_owned(),
                Value::String("monospace".to_owned()),
            )]),
        ),
        _ => return None,
    })
}

fn push_math_text(
    content: &mut Vec<MathContent>,
    mapped_content: &mut Option<Vec<MathContentSourceMap>>,
    text: String,
    source_range: Option<Range<usize>>,
    source_offset: usize,
    source: &str,
    text_state: &mut Option<(usize, usize)>,
) -> Option<()> {
    if let Some(MathContent::Text(flow)) = content.last_mut() {
        let adjacent_or_commented = text_state.as_ref().zip(source_range.as_ref()).is_some_and(
            |((previous_end, _), next)| {
                *previous_end == next.start
                    || source_gap_is_tex_comment(source, source_offset, *previous_end, next.start)
            },
        );
        if adjacent_or_commented {
            let text_start = text_state.as_ref()?.1;
            let text_end = text_start.checked_add(text.len())?;
            u32::try_from(text_end).ok()?;
            if let Some(mapped_content) = mapped_content {
                let Some(MathContentSourceMap::Text(mapped_text)) = mapped_content.last_mut()
                else {
                    return None;
                };
                if let Some(next_range) = source_range
                    .as_ref()
                    .and_then(|range| map_range(Some(range), source_offset))
                    .and_then(|range| range.range())
                {
                    let source_range = map_range(source_range.as_ref(), source_offset)?;
                    if source_range.range() != Some(next_range.clone()) {
                        return None;
                    }
                    mapped_text.append_parser_segment(text_start..text_end, source_range)?;
                }
            }
            flow.runs.push(Run {
                text,
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            });
            *text_state = source_range.as_ref().map(|range| (range.end, text_end));
            return Some(());
        }
    }
    let display_len = text.len();
    u32::try_from(display_len).ok()?;
    *text_state = source_range.as_ref().map(|range| (range.end, display_len));
    let mapped_range = if mapped_content.is_some() {
        Some(map_range(source_range.as_ref(), source_offset)?)
    } else {
        None
    };
    content.push(MathContent::Text(TextFlow {
        runs: vec![Run {
            text,
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    }));
    if let Some(mapped_content) = mapped_content {
        let range = mapped_range?;
        let segments = source_range
            .as_ref()
            .map(|_| {
                vec![MathTextSourceSegment::from_parser_parts(
                    0..display_len,
                    range.clone(),
                )]
            })
            .unwrap_or_default();
        mapped_content.push(MathContentSourceMap::Text(
            MathTextSourceMap::from_parser_parts(range, segments),
        ));
    }
    Some(())
}

/// 判断两个文本片段间的源码是否完全由 TeX 行注释及其换行组成。
fn source_gap_is_tex_comment(source: &str, offset: usize, start: usize, end: usize) -> bool {
    let Some(start) = offset.checked_add(start) else {
        return false;
    };
    let Some(end) = offset.checked_add(end) else {
        return false;
    };
    let Some(mut remaining) = source.get(start..end) else {
        return false;
    };
    while !remaining.is_empty() {
        let Some(comment) = remaining.strip_prefix('%') else {
            return false;
        };
        let line_end = comment.find(['\r', '\n']).unwrap_or(comment.len());
        remaining = &comment[line_end..];
        if let Some(after_cr) = remaining.strip_prefix('\r') {
            remaining = after_cr.strip_prefix('\n').unwrap_or(after_cr);
        } else if let Some(after_lf) = remaining.strip_prefix('\n') {
            remaining = after_lf;
        }
    }
    true
}

fn map_range(range: Option<&Range<usize>>, offset: usize) -> Option<MathRangeSourceMap> {
    let Some(range) = range else {
        return Some(MathRangeSourceMap::default());
    };
    Some(MathRangeSourceMap::from_parser_range(
        Some(offset.checked_add(range.start)?..offset.checked_add(range.end)?),
        BTreeMap::new(),
    ))
}

/// 把节点范围平移为相对单元格源码。
fn rebase(node: &mut MathNode, offset: usize) {
    if let Some(range) = node.source_range.as_mut() {
        range.start -= offset;
        range.end -= offset;
    }
    for child in &mut node.children {
        rebase(child, offset);
    }
}

/// 列表解析遇到的终止符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    Eof,
    Brace,
    Bracket,
    Right,
    Ampersand,
    RowBreak,
    End,
    Dollar,
}

/// 当前列表允许的终止符集合。
#[derive(Debug, Clone, Copy, Default)]
struct Stops {
    brace: bool,
    bracket: bool,
    right: bool,
    cell: bool,
    row_break: bool,
    dollar: bool,
}

impl Stops {
    const NONE: Self = Self {
        brace: false,
        bracket: false,
        right: false,
        cell: false,
        row_break: false,
        dollar: false,
    };
    const BRACE: Self = Self {
        brace: true,
        ..Self::NONE
    };
}

/// 矩阵环境的一行。
enum MatrixRow {
    Cells {
        cells: Vec<(MathNode, Range<usize>, Range<usize>)>,
        range: Range<usize>,
    },
    Hline(Range<usize>),
    Intertext(MathNode, Range<usize>),
}

struct MatrixRows {
    rows: Vec<MatrixRow>,
    environment_end_start: usize,
    environment_content_end: usize,
    environment_end: usize,
}

struct Parser<'s> {
    src: &'s str,
    pos: usize,
    end: usize,
    recursion: usize,
    nodes: usize,
}

impl<'s> Parser<'s> {
    fn new(src: &'s str, body: Range<usize>) -> Self {
        Self {
            src,
            pos: body.start,
            end: body.end,
            recursion: 0,
            nodes: 0,
        }
    }

    fn error_at(&self, offset: usize, message: &str) -> MathParseError {
        MathParseError::Parse {
            message: message.to_owned(),
            offset,
            incomplete: false,
        }
    }

    fn incomplete(&self, message: &str) -> MathParseError {
        MathParseError::Parse {
            message: message.to_owned(),
            offset: self.pos,
            incomplete: true,
        }
    }

    fn peek(&self) -> Option<char> {
        if self.pos >= self.end {
            return None;
        }
        self.src[self.pos..self.end].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.pos += character.len_utf8();
        Some(character)
    }

    /// 数学模式跳过空白和 `%` 注释。
    fn skip_space(&mut self) {
        while let Some(character) = self.peek() {
            if character.is_whitespace() {
                self.bump();
            } else if character == '%' {
                while let Some(next) = self.bump() {
                    if next == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    /// 当前位置的控制序列名，不移动位置。
    fn peek_command(&self) -> Option<&'s str> {
        let rest = &self.src[self.pos..self.end];
        let after = rest.strip_prefix('\\')?;
        let letters = after
            .bytes()
            .take_while(|byte| byte.is_ascii_alphabetic())
            .count();
        if letters > 0 {
            return Some(&after[..letters]);
        }
        after
            .chars()
            .next()
            .map(|character| &after[..character.len_utf8()])
    }

    /// 读取控制序列名。调用前位置必须在反斜杠上。
    fn read_command(&mut self) -> Option<&'s str> {
        let name = self.peek_command()?;
        self.pos += 1 + name.len();
        Some(name)
    }

    fn enter(&mut self) -> ParseResult<()> {
        self.recursion += 1;
        if self.recursion > MAX_RECURSION {
            return Err(MathParseError::InputTooDeep {
                max: MAX_MATH_TREE_DEPTH,
            });
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.recursion -= 1;
    }

    fn node(
        &mut self,
        kind: MathNodeKind,
        text: Option<String>,
        attributes: &[(&str, String)],
        range: Option<Range<usize>>,
        children: Vec<MathNode>,
    ) -> MathNode {
        self.nodes = self.nodes.saturating_add(1);
        MathNode {
            id: NodeId::generate(),
            kind,
            text,
            attributes: attributes
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
            source_range: range,
            children,
            extra: BTreeMap::new(),
        }
    }

    fn check_budget(&self) -> ParseResult<()> {
        if self.nodes > MAX_MATH_TREE_NODES {
            return Err(MathParseError::SyntaxTreeTooLarge {
                max: MAX_MATH_TREE_NODES,
            });
        }
        Ok(())
    }

    fn placeholder(&mut self) -> MathNode {
        self.node(MathNodeKind::Placeholder, None, &[], None, Vec::new())
    }

    /// 源码中的空花括号 `{}`：占位节点记录花括号范围，供公式内光标定位到该空槽。
    fn empty_group(&mut self, range: Range<usize>) -> MathNode {
        self.node(
            MathNodeKind::Placeholder,
            None,
            &[],
            Some(range),
            Vec::new(),
        )
    }

    /// 由列表构造 Row，相邻的字面普通字符合并为一个多字符符号。
    fn row(&mut self, items: Vec<MathNode>, range: Option<Range<usize>>) -> MathNode {
        let range = range.or_else(|| span_of(&items));
        self.node(MathNodeKind::Row, None, &[], range, items)
    }

    /// 单个原子直接作为参数，否则包成 Row。
    fn list_node(&mut self, items: Vec<MathNode>, range: Range<usize>) -> MathNode {
        if items.len() == 1 {
            return items
                .into_iter()
                .next()
                .unwrap_or_else(|| self.placeholder());
        }
        self.row(items, Some(range))
    }

    /// 解析一个原子列表，直到遇到允许的终止符。
    fn parse_list(&mut self, stops: Stops) -> ParseResult<(Vec<MathNode>, Ended)> {
        self.enter()?;
        let result = self.parse_list_inner(stops);
        self.leave();
        let (items, ended) = result?;
        Ok((coalesce(items, self.src), ended))
    }

    fn parse_list_inner(&mut self, stops: Stops) -> ParseResult<(Vec<MathNode>, Ended)> {
        let mut items: Vec<MathNode> = Vec::new();
        loop {
            self.check_budget()?;
            self.skip_space();
            let start = self.pos;
            let Some(character) = self.peek() else {
                if stops.brace || stops.bracket || stops.right || stops.cell || stops.dollar {
                    return Err(self.incomplete("unexpected end of input"));
                }
                return Ok((items, Ended::Eof));
            };
            match character {
                '}' => {
                    if stops.brace {
                        self.bump();
                        return Ok((items, Ended::Brace));
                    }
                    return Err(self.error_at(start, "unexpected `}`"));
                }
                ']' if stops.bracket => {
                    self.bump();
                    return Ok((items, Ended::Bracket));
                }
                '&' => {
                    if stops.cell {
                        self.bump();
                        return Ok((items, Ended::Ampersand));
                    }
                    return Err(self.error_at(start, "unexpected `&`"));
                }
                '$' => {
                    if stops.dollar {
                        self.bump();
                        return Ok((items, Ended::Dollar));
                    }
                    return Err(self.error_at(start, "unexpected `$`"));
                }
                '^' | '_' | '\'' => {
                    let base = self.placeholder();
                    let node = self.parse_scripts(base, start)?;
                    items.push(node);
                    continue;
                }
                '\\' => match self.peek_command() {
                    None => return Err(self.incomplete("trailing backslash")),
                    Some("\\") if stops.row_break => {
                        self.read_command();
                        return Ok((items, Ended::RowBreak));
                    }
                    Some("end") if stops.cell => return Ok((items, Ended::End)),
                    Some("right") if stops.right => {
                        self.read_command();
                        return Ok((items, Ended::Right));
                    }
                    Some("color") => {
                        self.read_command();
                        let color = self.parse_color()?;
                        let (body, ended) = self.parse_list(stops)?;
                        let body_range = span_of(&body).unwrap_or(self.pos..self.pos);
                        let body = self.list_node(body, body_range);
                        let node = self.node(
                            MathNodeKind::Color,
                            None,
                            &[("color", color)],
                            Some(start..self.end_of(ended)),
                            vec![body],
                        );
                        items.push(node);
                        return Ok((items, ended));
                    }
                    Some("limits" | "nolimits") => {
                        self.read_command();
                        continue;
                    }
                    _ => {}
                },
                _ => {}
            }
            let atom = self.parse_atom()?;
            let atom = self.extend_literal_grapheme(atom)?;
            let node = self.parse_scripts(atom, start)?;
            items.push(node);
        }
    }

    /// 终止符之前的位置，用于给 `\color` 等延续到列表末尾的节点定范围。
    fn end_of(&self, ended: Ended) -> usize {
        match ended {
            Ended::Brace | Ended::Bracket | Ended::Ampersand | Ended::Dollar => self.pos - 1,
            Ended::RowBreak => self.pos - 2,
            Ended::Right => self.pos - "\\right".len(),
            Ended::End | Ended::Eof => self.pos,
        }
    }

    /// 解析基底之后的上下标与撇号。
    fn parse_scripts(&mut self, mut base: MathNode, start: usize) -> ParseResult<MathNode> {
        let mut superscript: Option<Vec<MathNode>> = None;
        let mut explicit_superscript = false;
        let mut subscript: Option<MathNode> = None;
        let mut script_end = start;
        loop {
            self.skip_space();
            match self.peek() {
                Some('\'') => {
                    if explicit_superscript {
                        return Err(self.error_at(self.pos, "double superscript"));
                    }
                    let prime_start = self.pos;
                    self.bump();
                    let prime = self.node(
                        MathNodeKind::Symbol,
                        Some("prime".to_owned()),
                        &[],
                        Some(prime_start..self.pos),
                        Vec::new(),
                    );
                    superscript.get_or_insert_with(Vec::new).push(prime);
                    script_end = self.pos;
                }
                Some('^') => {
                    if explicit_superscript {
                        return Err(self.error_at(self.pos, "double superscript"));
                    }
                    self.bump();
                    explicit_superscript = true;
                    let argument = self.parse_argument()?;
                    let items = superscript.get_or_insert_with(Vec::new);
                    if argument.kind == MathNodeKind::Row && !items.is_empty() {
                        items.extend(argument.children);
                    } else {
                        items.push(argument);
                    }
                    script_end = self.pos;
                }
                Some('_') => {
                    if subscript.is_some() {
                        return Err(self.error_at(self.pos, "double subscript"));
                    }
                    self.bump();
                    subscript = Some(self.parse_argument()?);
                    script_end = self.pos;
                }
                _ => break,
            }
            // `\limits` 只影响显示，不进入文档树。
            self.skip_space();
            while matches!(self.peek_command(), Some("limits" | "nolimits")) {
                self.read_command();
                script_end = self.pos;
                self.skip_space();
            }
        }
        let superscript = superscript.map(|items| {
            let range = span_of(&items).unwrap_or(start..start);
            self.list_node(items, range)
        });
        if superscript.is_none() && subscript.is_none() {
            return Ok(base);
        }
        let range = Some(start..script_end);
        if matches!(
            base.kind,
            MathNodeKind::Sum | MathNodeKind::Product | MathNodeKind::Integral
        ) && base
            .children
            .iter()
            .all(|child| child.kind == MathNodeKind::Placeholder)
        {
            if let Some(lower) = subscript {
                base.children[0] = lower;
            }
            if let Some(upper) = superscript {
                base.children[1] = upper;
            }
            base.source_range = range;
            return Ok(base);
        }
        Ok(match (subscript, superscript) {
            (Some(sub), Some(sup)) => {
                self.node(MathNodeKind::SubSup, None, &[], range, vec![base, sub, sup])
            }
            (Some(sub), None) => {
                self.node(MathNodeKind::Subscript, None, &[], range, vec![base, sub])
            }
            (None, Some(sup)) => {
                self.node(MathNodeKind::Superscript, None, &[], range, vec![base, sup])
            }
            (None, None) => base,
        })
    }

    /// 在解析上下标前，单次扫描并合并基底后连续的字面字素部分。
    fn extend_literal_grapheme(&mut self, base: MathNode) -> ParseResult<MathNode> {
        let Some(range) = base.source_range.as_ref() else {
            return Ok(base);
        };
        if !is_literal_symbol(&base, self.src) || range.end != self.pos {
            return Ok(base);
        }

        let Some(text) = base.text.as_deref() else {
            return Ok(base);
        };
        let Some(next) = self
            .src
            .get(self.pos..)
            .and_then(|suffix| suffix.chars().next())
        else {
            return Ok(base);
        };
        if next != '\\' {
            let Some(raw_cluster) = self.src[self.pos..].graphemes(true).next() else {
                return Ok(base);
            };
            if !raw_cluster.chars().all(is_direct_literal) {
                return Ok(base);
            }
            let mut candidate = String::with_capacity(text.len() + raw_cluster.len());
            candidate.push_str(text);
            candidate.push_str(raw_cluster);
            let cluster_length = candidate
                .graphemes(true)
                .next()
                .map(str::len)
                .unwrap_or(text.len());
            let extension_bytes = cluster_length.saturating_sub(text.len());
            if extension_bytes == 0 {
                return Ok(base);
            }
            let merged_text = candidate.get(..cluster_length).unwrap_or(text).to_owned();
            let raw_end = self.pos + extension_bytes;
            let escaped_extension_follows = cluster_length == candidate.len()
                && self.direct_or_escaped_literal_at(raw_end).is_some();
            if !escaped_extension_follows {
                return self.finish_literal_grapheme(base, merged_text, raw_end);
            }
        }

        let mut decoded_suffix = String::new();
        let mut token_lengths = Vec::new();
        let mut scanned_end = self.pos;
        let mut token_limit = 1;
        let cluster_length = loop {
            while token_lengths.len() < token_limit {
                let Some((character, token_bytes)) = self.direct_or_escaped_literal_at(scanned_end)
                else {
                    break;
                };
                decoded_suffix.push(character);
                token_lengths.push((token_bytes, character.len_utf8()));
                scanned_end += token_bytes;
            }
            let mut candidate = String::with_capacity(text.len() + decoded_suffix.len());
            candidate.push_str(text);
            candidate.push_str(&decoded_suffix);
            let cluster_length = candidate
                .graphemes(true)
                .next()
                .map(str::len)
                .unwrap_or(text.len());
            if cluster_length < candidate.len() || scanned_end == self.src.len() {
                break cluster_length;
            }
            let Some((character, token_bytes)) = self.direct_or_escaped_literal_at(scanned_end)
            else {
                break cluster_length;
            };
            decoded_suffix.push(character);
            token_lengths.push((token_bytes, character.len_utf8()));
            scanned_end += token_bytes;
            token_limit = token_limit.saturating_mul(2).max(token_lengths.len() + 1);
        };
        let mut candidate = String::with_capacity(text.len() + decoded_suffix.len());
        candidate.push_str(text);
        candidate.push_str(&decoded_suffix);
        let extension_length = cluster_length.saturating_sub(text.len());
        let mut extension = 0;
        let mut extension_bytes = 0;
        for (token_length, character_length) in token_lengths {
            if extension >= extension_length {
                break;
            }
            extension_bytes += token_length;
            extension += character_length;
        }
        let merged_text = candidate.get(..cluster_length).unwrap_or(text).to_owned();
        let end = self.pos + extension_bytes;
        self.finish_literal_grapheme(base, merged_text, end)
    }

    fn finish_literal_grapheme(
        &mut self,
        mut base: MathNode,
        merged_text: String,
        end: usize,
    ) -> ParseResult<MathNode> {
        if end == self.pos {
            return Ok(base);
        }
        if let Some(range) = base.source_range.as_mut() {
            range.end = end;
        }
        base.text = Some(merged_text);
        self.pos = end;
        Ok(base)
    }

    fn direct_or_escaped_literal_at(&self, offset: usize) -> Option<(char, usize)> {
        let remaining = self.src.get(offset..)?;
        let character = remaining.chars().next()?;
        if is_direct_literal(character) {
            return Some((character, character.len_utf8()));
        }
        if character != '\\' {
            return None;
        }
        if remaining.starts_with("\\char\"") {
            let digits = remaining
                .get(6..)?
                .chars()
                .take_while(char::is_ascii_hexdigit);
            let digits = digits.collect::<String>();
            let character = u32::from_str_radix(&digits, 16)
                .ok()
                .and_then(char::from_u32)?;
            if !is_valid_literal_char(character) {
                return None;
            }
            let mut token_bytes = 6 + digits.len();
            if remaining.get(token_bytes..)?.starts_with(' ') {
                token_bytes += 1;
            }
            return Some((character, token_bytes));
        }
        let escaped = remaining.get(1..)?.chars().next()?;
        matches!(escaped, '%' | '&' | '#' | '_' | '$').then_some((escaped, 1 + escaped.len_utf8()))
    }

    /// 命令参数或上下标：花括号分组，或单个记号构成的原子。
    fn parse_argument(&mut self) -> ParseResult<MathNode> {
        self.skip_space();
        let start = self.pos;
        match self.peek() {
            None => Err(self.incomplete("missing argument")),
            Some('{') => {
                self.bump();
                let (items, _) = self.parse_list(Stops::BRACE)?;
                if items.is_empty() {
                    return Ok(self.empty_group(start..self.pos));
                }
                Ok(self.list_node(items, start..self.pos))
            }
            Some('}' | '&' | '$' | '^' | '_') => Err(self.error_at(start, "missing argument")),
            Some(_) => {
                let atom = self.parse_atom()?;
                self.extend_literal_grapheme(atom)
            }
        }
    }

    /// 花括号内的原始文本，例如环境名或颜色值。
    fn read_raw_group(&mut self) -> ParseResult<String> {
        self.skip_space();
        match self.peek() {
            None => return Err(self.incomplete("missing `{`")),
            Some('{') => {}
            Some(_) => return Err(self.error_at(self.pos, "expected `{`")),
        }
        self.bump();
        let start = self.pos;
        loop {
            match self.bump() {
                None => return Err(self.incomplete("missing `}`")),
                Some('}') => return Ok(self.src[start..self.pos - 1].trim().to_owned()),
                Some('{' | '\\') => return Err(self.error_at(self.pos - 1, "unexpected token")),
                Some(_) => {}
            }
        }
    }

    /// 可选的 `[...]` 原始参数。
    fn read_raw_optional(&mut self) -> ParseResult<Option<String>> {
        self.skip_space();
        if self.peek() != Some('[') {
            return Ok(None);
        }
        self.bump();
        let start = self.pos;
        loop {
            match self.bump() {
                None => return Err(self.incomplete("missing `]`")),
                Some(']') => return Ok(Some(self.src[start..self.pos - 1].trim().to_owned())),
                Some(_) => {}
            }
        }
    }

    /// 解析单个原子（不含上下标）。
    fn parse_atom(&mut self) -> ParseResult<MathNode> {
        self.enter()?;
        let result = self.parse_atom_inner();
        self.leave();
        result
    }

    fn parse_atom_inner(&mut self) -> ParseResult<MathNode> {
        self.skip_space();
        let start = self.pos;
        let Some(character) = self.peek() else {
            return Err(self.incomplete("unexpected end of input"));
        };
        if character == '{' {
            self.bump();
            let (items, _) = self.parse_list(Stops::BRACE)?;
            if items.is_empty() {
                return Ok(self.empty_group(start..self.pos));
            }
            return Ok(self.node(MathNodeKind::Group, None, &[], Some(start..self.pos), items));
        }
        if character == '~' {
            self.bump();
            return Ok(self.space("no_break_space", start));
        }
        if character != '\\' {
            self.bump();
            return Ok(self.node(
                MathNodeKind::Symbol,
                Some(character.to_string()),
                &[("atomClass", literal_class(character).as_str().to_owned())],
                Some(start..self.pos),
                Vec::new(),
            ));
        }
        let Some(name) = self.read_command() else {
            return Err(self.incomplete("trailing backslash"));
        };
        self.parse_command(name, start)
    }

    fn space(&mut self, kind: &str, start: usize) -> MathNode {
        self.node(
            MathNodeKind::Space,
            None,
            &[("spaceKind", kind.to_owned())],
            Some(start..self.pos),
            Vec::new(),
        )
    }

    fn symbol(&mut self, name: &str, class: Option<&str>, start: usize) -> MathNode {
        let attributes: Vec<(&str, String)> = class
            .map(|class| ("atomClass", class.to_owned()))
            .into_iter()
            .collect();
        self.node(
            MathNodeKind::Symbol,
            Some(name.to_owned()),
            &attributes,
            Some(start..self.pos),
            Vec::new(),
        )
    }

    fn parse_command(&mut self, name: &'s str, start: usize) -> ParseResult<MathNode> {
        use MathNodeKind as K;
        if NUMBERING_COMMANDS.contains(&name) {
            return Err(MathParseError::UnsupportedNumbering {
                command: format!("\\{name}"),
            });
        }
        if named_symbol(name).is_some() {
            return Ok(self.symbol(name, None, start));
        }
        if let Some(limits) = text_operator_limits(name) {
            return Ok(self.node(
                K::Operator,
                Some(name.to_owned()),
                &[("limits", limits.to_string())],
                Some(start..self.pos),
                Vec::new(),
            ));
        }
        if let Some(accent) = accent_attribute(name) {
            let body = self.parse_argument()?;
            return Ok(self.node(
                K::Accent,
                None,
                &[("accent", accent.to_owned())],
                Some(start..self.pos),
                vec![body],
            ));
        }
        match name {
            "{" => Ok(self.symbol("lbrace", None, start)),
            "}" => Ok(self.symbol("rbrace", None, start)),
            "|" => Ok(self.symbol("Vert", None, start)),
            "char" => {
                if self.bump() != Some('"') {
                    return Err(self.error_at(start, "expected hexadecimal \\char code"));
                }
                let digits_start = self.pos;
                while self
                    .peek()
                    .is_some_and(|character| character.is_ascii_hexdigit())
                {
                    self.bump();
                }
                let digits = self.src.get(digits_start..self.pos).unwrap_or_default();
                let character = u32::from_str_radix(digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| self.error_at(start, "invalid hexadecimal \\char code"))?;
                if !is_valid_literal_char(character) {
                    return Err(self.error_at(start, "unsupported non-printing \\char code"));
                }
                if self.peek().is_some_and(char::is_whitespace) {
                    self.skip_space();
                }
                Ok(self.node(
                    K::Symbol,
                    Some(character.to_string()),
                    &[("atomClass", literal_class(character).as_str().to_owned())],
                    Some(start..self.pos),
                    Vec::new(),
                ))
            }
            "%" | "&" | "#" | "_" | "$" => Ok(self.symbol(name, Some("ord"), start)),
            "," => Ok(self.space("thin", start)),
            ":" | ">" => Ok(self.space("medium", start)),
            ";" => Ok(self.space("thick", start)),
            "!" => Ok(self.space("negative_thin", start)),
            " " => Ok(self.space("control", start)),
            "quad" => Ok(self.space("quad", start)),
            "qquad" => Ok(self.space("qquad", start)),
            "hspace" => {
                let raw = self.read_raw_group()?;
                let (numerator, denominator) = parse_em(&raw)
                    .ok_or_else(|| self.error_at(start, "unsupported \\hspace dimension"))?;
                Ok(self.node(
                    K::Space,
                    None,
                    &[
                        ("spaceKind", "hspace".to_owned()),
                        ("valueNumerator", numerator.to_string()),
                        ("valueDenominator", denominator.to_string()),
                        ("valueUnit", "em".to_owned()),
                    ],
                    Some(start..self.pos),
                    Vec::new(),
                ))
            }
            "operatorname" => {
                let limits = if self.peek() == Some('*') {
                    self.bump();
                    true
                } else {
                    false
                };
                let text = self.read_raw_group()?;
                if text.is_empty() || !text.chars().all(char::is_alphanumeric) {
                    return Err(self.error_at(start, "unsupported \\operatorname content"));
                }
                Ok(self.node(
                    K::Operator,
                    Some(text),
                    &[("limits", limits.to_string())],
                    Some(start..self.pos),
                    Vec::new(),
                ))
            }
            "sum" | "prod" => {
                let kind = if name == "sum" { K::Sum } else { K::Product };
                let children = vec![self.placeholder(), self.placeholder()];
                Ok(self.node(kind, None, &[], Some(start..self.pos), children))
            }
            "int" | "iint" | "iiint" | "oint" | "oiint" => {
                let children = vec![self.placeholder(), self.placeholder()];
                Ok(self.node(
                    K::Integral,
                    None,
                    &[("integralKind", name.to_owned())],
                    Some(start..self.pos),
                    children,
                ))
            }
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let numerator = self.parse_argument()?;
                let denominator = self.parse_argument()?;
                Ok(self.node(
                    K::Fraction,
                    None,
                    &[("style", name.to_owned())],
                    Some(start..self.pos),
                    vec![numerator, denominator],
                ))
            }
            "sqrt" => {
                self.skip_space();
                let index = if self.peek() == Some('[') {
                    let index_start = self.pos;
                    self.bump();
                    let stops = Stops {
                        bracket: true,
                        ..Stops::NONE
                    };
                    let (items, _) = self.parse_list(stops)?;
                    if items.is_empty() {
                        self.placeholder()
                    } else {
                        self.list_node(items, index_start..self.pos)
                    }
                } else {
                    self.placeholder()
                };
                let body = self.parse_argument()?;
                Ok(self.node(
                    K::Radical,
                    None,
                    &[],
                    Some(start..self.pos),
                    vec![index, body],
                ))
            }
            "left" => {
                let left = self.parse_delimiter()?;
                let body_start = self.pos;
                let stops = Stops {
                    right: true,
                    ..Stops::NONE
                };
                let (items, _) = self.parse_list(stops)?;
                let body_end = self.pos - "\\right".len();
                let right = self.parse_delimiter()?;
                let body = self.row(items, Some(body_start..body_end));
                Ok(self.node(
                    K::Delimited,
                    None,
                    &[("left", left), ("right", right)],
                    Some(start..self.pos),
                    vec![body],
                ))
            }
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "Bigl" | "biggl" | "Biggl" | "bigr"
            | "Bigr" | "biggr" | "Biggr" | "bigm" | "Bigm" | "biggm" | "Biggm" => {
                let (size, class) = match name.as_bytes().last() {
                    Some(b'l') => (&name[..name.len() - 1], "open"),
                    Some(b'r') => (&name[..name.len() - 1], "close"),
                    Some(b'm') => (&name[..name.len() - 1], "rel"),
                    _ => (name, "ord"),
                };
                let delimiter = self.parse_delimiter()?;
                Ok(self.node(
                    K::SizedDelimiter,
                    Some(delimiter),
                    &[("size", size.to_owned()), ("atomClass", class.to_owned())],
                    Some(start..self.pos),
                    Vec::new(),
                ))
            }
            "begin" => {
                let environment = self.read_raw_group()?;
                if NUMBERING_ENVIRONMENTS.contains(&environment.as_str()) {
                    return Err(MathParseError::UnsupportedNumbering {
                        command: format!("\\begin{{{environment}}}"),
                    });
                }
                let Some(style) = MATRIX_ENVIRONMENTS
                    .iter()
                    .copied()
                    .find(|candidate| *candidate == environment)
                else {
                    return Err(self.error_at(start, "unsupported environment"));
                };
                let columns = if style == "array" {
                    let columns: String = self
                        .read_raw_group()?
                        .chars()
                        .filter(|character| !character.is_whitespace())
                        .collect();
                    if !columns
                        .chars()
                        .all(|column| matches!(column, 'l' | 'c' | 'r' | '|'))
                    {
                        return Err(self.error_at(start, "unsupported array column specification"));
                    }
                    columns
                } else {
                    String::new()
                };
                let rows = self.parse_matrix_rows(style, false)?.rows;
                let mut children = Vec::new();
                for row in rows {
                    children.push(match row {
                        MatrixRow::Cells { cells, range } => {
                            let cells = cells.into_iter().map(|(cell, _, _)| cell).collect();
                            self.node(K::Row, None, &[], Some(range), cells)
                        }
                        MatrixRow::Hline(range) => {
                            self.node(K::Hline, None, &[], Some(range), Vec::new())
                        }
                        MatrixRow::Intertext(text, range) => {
                            self.node(K::Intertext, None, &[], Some(range), vec![text])
                        }
                    });
                }
                Ok(self.node(
                    K::Matrix,
                    None,
                    &[("style", style.to_owned()), ("columns", columns)],
                    Some(start..self.pos),
                    children,
                ))
            }
            "substack" => {
                self.skip_space();
                match self.peek() {
                    None => return Err(self.incomplete("missing argument")),
                    Some('{') => {}
                    Some(_) => return Err(self.error_at(self.pos, "expected `{`")),
                }
                self.bump();
                let stops = Stops {
                    brace: true,
                    row_break: true,
                    ..Stops::NONE
                };
                let mut lines = Vec::new();
                loop {
                    let line_start = self.pos;
                    let (items, ended) = self.parse_list(stops)?;
                    let line_end = match ended {
                        Ended::RowBreak => self.pos - 2,
                        _ => self.pos - 1,
                    };
                    let line = self.row(items, Some(line_start..line_end));
                    lines.push(line);
                    if ended == Ended::Brace {
                        break;
                    }
                }
                Ok(self.node(K::Substack, None, &[], Some(start..self.pos), lines))
            }
            "overset" | "stackrel" | "underset" => {
                let script = self.parse_argument()?;
                let base = self.parse_argument()?;
                let empty = self.placeholder();
                let children = if name == "underset" {
                    vec![base, empty, script]
                } else {
                    vec![base, script, empty]
                };
                Ok(self.node(K::OverUnder, None, &[], Some(start..self.pos), children))
            }
            "cancelto" => {
                let value = self.parse_argument()?;
                let body = self.parse_argument()?;
                Ok(self.node(
                    K::CancelTo,
                    None,
                    &[],
                    Some(start..self.pos),
                    vec![value, body],
                ))
            }
            "text" | "mbox" | "textrm" | "textbf" | "textmd" | "textit" | "textup" | "textsf"
            | "texttt" => {
                let style = match name {
                    "text" | "mbox" => "text",
                    other => &other[4..],
                };
                let children = self.parse_text_argument()?;
                Ok(self.node(
                    K::Text,
                    None,
                    &[("style", style.to_owned())],
                    Some(start..self.pos),
                    children,
                ))
            }
            "mathrm" | "mathbf" | "mathit" | "mathsf" | "mathtt" | "mathbb" | "mathcal"
            | "mathfrak" | "mathscr" | "boldsymbol" | "pmb" => {
                let style = name.strip_prefix("math").unwrap_or(name);
                self.skip_space();
                let children = if self.peek() == Some('{') {
                    self.bump();
                    self.parse_list(Stops::BRACE)?.0
                } else {
                    vec![self.parse_atom()?]
                };
                Ok(self.node(
                    K::MathAlphabet,
                    None,
                    &[("style", style.to_owned())],
                    Some(start..self.pos),
                    children,
                ))
            }
            "textcolor" => {
                let color = self.parse_color()?;
                let body = self.parse_argument()?;
                Ok(self.node(
                    K::TextColor,
                    None,
                    &[("color", color)],
                    Some(start..self.pos),
                    vec![body],
                ))
            }
            "colorbox" => {
                let color = self.parse_color()?;
                let body = self.parse_box_text(start)?;
                Ok(self.node(
                    K::ColorBox,
                    None,
                    &[("color", color)],
                    Some(start..self.pos),
                    vec![body],
                ))
            }
            "fcolorbox" => {
                let border = self.parse_color()?;
                let fill = self.parse_color()?;
                let body = self.parse_box_text(start)?;
                Ok(self.node(
                    K::FColorBox,
                    None,
                    &[("borderColor", border), ("fillColor", fill)],
                    Some(start..self.pos),
                    vec![body],
                ))
            }
            "phantom" | "vphantom" | "hphantom" => {
                let body = self.parse_argument()?;
                Ok(self.node(
                    K::Phantom,
                    None,
                    &[("phantomKind", name.to_owned())],
                    Some(start..self.pos),
                    vec![body],
                ))
            }
            "rule" => {
                let raise = match self.read_raw_optional()? {
                    Some(raw) => parse_em(&raw)
                        .ok_or_else(|| self.error_at(start, "unsupported \\rule dimension"))?,
                    None => (0, 1),
                };
                let width = self.read_raw_group()?;
                let total = self.read_raw_group()?;
                let width = parse_em(&width)
                    .ok_or_else(|| self.error_at(start, "unsupported \\rule dimension"))?;
                let total = parse_em(&total)
                    .ok_or_else(|| self.error_at(start, "unsupported \\rule dimension"))?;
                if width.0 != 0 {
                    return Err(self.error_at(start, "only zero-width \\rule struts are supported"));
                }
                // `\rule[r]{0}{t}` 占据基线之上 t + r、之下 -r。
                let height = add_ratio(total, raise);
                let depth = (-raise.0, raise.1);
                Ok(self.node(
                    K::Strut,
                    None,
                    &[
                        ("heightNumerator", height.0.to_string()),
                        ("heightDenominator", height.1.to_string()),
                        ("heightUnit", "em".to_owned()),
                        ("depthNumerator", depth.0.to_string()),
                        ("depthDenominator", depth.1.to_string()),
                        ("depthUnit", "em".to_owned()),
                    ],
                    Some(start..self.pos),
                    Vec::new(),
                ))
            }
            "right" => Err(self.error_at(start, "unmatched \\right")),
            "end" => Err(self.error_at(start, "unmatched \\end")),
            "\\" => Err(self.error_at(start, "unexpected line break")),
            "(" | ")" | "[" | "]" => Err(self.error_at(start, "unexpected math delimiter")),
            _ => Err(self.error_at(start, "unknown command")),
        }
    }

    /// `\left`、`\right` 和 `\big` 系列后的定界符。
    fn parse_delimiter(&mut self) -> ParseResult<String> {
        self.skip_space();
        let start = self.pos;
        let token = match self.peek() {
            None => return Err(self.incomplete("missing delimiter")),
            Some('\\') => {
                let Some(name) = self.read_command() else {
                    return Err(self.incomplete("trailing backslash"));
                };
                format!("\\{name}")
            }
            Some(_) => self.bump().map(String::from).unwrap_or_default(),
        };
        encode_delimiter(&token).ok_or_else(|| self.error_at(start, "unsupported delimiter"))
    }

    /// 颜色参数：`{name}`、`{#rrggbb}` 或 `[HTML]{RRGGBB}`。
    fn parse_color(&mut self) -> ParseResult<String> {
        let start = self.pos;
        let model = self.read_raw_optional()?;
        let value = self.read_raw_group()?;
        let hex = match model.as_deref() {
            Some("HTML") => Some(value.as_str()),
            Some(_) => return Err(self.error_at(start, "unsupported color model")),
            None => value.strip_prefix('#'),
        };
        if let Some(hex) = hex {
            if matches!(hex.len(), 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Ok(format!("#{}", hex.to_ascii_lowercase()));
            }
            return Err(self.error_at(start, "invalid color"));
        }
        NAMED_COLORS
            .iter()
            .find(|(name, _)| *name == value)
            .map(|(_, hex)| (*hex).to_owned())
            .ok_or_else(|| self.error_at(start, "unknown color"))
    }

    /// `\colorbox` 的正文按 LaTeX 语义是文本模式。
    fn parse_box_text(&mut self, start: usize) -> ParseResult<MathNode> {
        let children = self.parse_text_argument()?;
        Ok(self.node(
            MathNodeKind::Text,
            None,
            &[("style", "text".to_owned())],
            Some(start..self.pos),
            children,
        ))
    }

    /// 读取 `{...}` 形式的文本模式参数。
    fn parse_text_argument(&mut self) -> ParseResult<Vec<MathNode>> {
        self.skip_space();
        match self.peek() {
            None => return Err(self.incomplete("missing argument")),
            Some('{') => {}
            Some(_) => return Err(self.error_at(self.pos, "expected `{`")),
        }
        self.bump();
        self.enter()?;
        let result = self.parse_text_body();
        self.leave();
        result
    }

    /// 文本模式内容，直到与开头配对的 `}`。内层花括号只分组，不产生节点。
    fn parse_text_body(&mut self) -> ParseResult<Vec<MathNode>> {
        use MathNodeKind as K;
        let mut items = Vec::new();
        let body_start = self.pos;
        let mut run_start: Option<usize> = None;
        let mut depth = 0usize;
        loop {
            self.check_budget()?;
            let start = self.pos;
            let Some(character) = self.peek() else {
                return Err(self.incomplete("missing `}`"));
            };
            let breaks_run = character.is_whitespace()
                || matches!(character, '{' | '}' | '~' | '$' | '%' | '\\');
            if breaks_run {
                if let Some(run) = run_start.take() {
                    let text = self.src[run..start].to_owned();
                    let node = self.node(
                        K::TextContent,
                        Some(text),
                        &[],
                        Some(run..start),
                        Vec::new(),
                    );
                    items.push(node);
                }
            } else {
                if matches!(character, '^' | '_' | '&' | '#') {
                    return Err(self.error_at(start, "unexpected character in text"));
                }
                run_start.get_or_insert(start);
                self.bump();
                continue;
            }
            match character {
                '%' => {
                    self.bump();
                    while !self.peek().is_none_or(|ch| matches!(ch, '\r' | '\n')) {
                        self.bump();
                    }
                    match self.peek() {
                        Some('\r') => {
                            self.bump();
                            if self.peek() == Some('\n') {
                                self.bump();
                            }
                        }
                        Some('\n') => {
                            self.bump();
                        }
                        _ => {}
                    }
                }
                '}' => {
                    self.bump();
                    if depth == 0 {
                        if items.is_empty() {
                            items.push(self.node(
                                K::TextContent,
                                Some(String::new()),
                                &[],
                                Some(body_start..body_start),
                                Vec::new(),
                            ));
                        }
                        return Ok(items);
                    }
                    depth -= 1;
                }
                '{' => {
                    self.bump();
                    depth += 1;
                }
                '~' => {
                    self.bump();
                    let node = self.text_space("no_break_space", start);
                    items.push(node);
                }
                '$' => {
                    self.bump();
                    let stops = Stops {
                        dollar: true,
                        ..Stops::NONE
                    };
                    let (math, _) = self.parse_list(stops)?;
                    let range = start..self.pos;
                    let node = if math.is_empty() {
                        self.node(K::Placeholder, None, &[], Some(range), Vec::new())
                    } else if math.len() == 1 {
                        math.into_iter()
                            .next()
                            .unwrap_or_else(|| self.placeholder())
                    } else {
                        self.row(math, Some(range))
                    };
                    items.push(node);
                }
                '\\' => {
                    let Some(name) = self.read_command() else {
                        return Err(self.incomplete("trailing backslash"));
                    };
                    match name {
                        " " => {
                            let node = self.text_space("control_space", start);
                            items.push(node);
                        }
                        "%" | "&" | "#" | "_" | "{" | "}" | "$" => {
                            let node = self.node(
                                K::TextSymbol,
                                Some(name.to_owned()),
                                &[],
                                Some(start..self.pos),
                                Vec::new(),
                            );
                            items.push(node);
                        }
                        "textrm" | "textbf" | "textit" | "textmd" | "textup" | "textsf"
                        | "texttt" => {
                            let children = self.parse_text_argument()?;
                            let node = self.node(
                                K::TextStyle,
                                None,
                                &[("style", name[4..].to_owned())],
                                Some(start..self.pos),
                                children,
                            );
                            items.push(node);
                        }
                        "textchar" => {
                            let encoded = self.read_raw_group()?;
                            let character = u32::from_str_radix(&encoded, 16)
                                .ok()
                                .and_then(char::from_u32)
                                .ok_or_else(|| self.error_at(start, "invalid text character"))?;
                            let node = self.node(
                                K::TextSymbol,
                                Some(character.to_string()),
                                &[],
                                Some(start..self.pos),
                                Vec::new(),
                            );
                            items.push(node);
                        }
                        _ => return Err(self.error_at(start, "unknown text command")),
                    }
                }
                _ => {
                    // 连续空白折叠成一个空格。
                    while self.peek().is_some_and(char::is_whitespace) {
                        self.bump();
                    }
                    let node = self.text_space("space", start);
                    items.push(node);
                }
            }
        }
    }

    fn text_space(&mut self, kind: &str, start: usize) -> MathNode {
        self.node(
            MathNodeKind::TextSpace,
            Some(kind.to_owned()),
            &[],
            Some(start..self.pos),
            Vec::new(),
        )
    }

    /// 解析矩阵环境的行，直到与 `name` 配对的 `\end`。
    fn parse_matrix_rows(
        &mut self,
        name: &str,
        preserve_trailing_empty_row: bool,
    ) -> ParseResult<MatrixRows> {
        let mut rows = Vec::new();
        let mut trailing_row_break = false;
        loop {
            self.check_budget()?;
            let navigation_row_start = self.pos;
            self.skip_space();
            let row_start = self.pos;
            match self.peek_command() {
                Some("end") => {
                    let environment_end_start = row_start;
                    self.read_command();
                    let end_name = self.read_raw_group()?;
                    if end_name != name {
                        return Err(self.error_at(row_start, "mismatched \\end"));
                    }
                    // 行分隔符后的 trivia 仍属于新空行，不能随 `\end` 一起丢弃。
                    if preserve_trailing_empty_row && trailing_row_break {
                        let placeholder = self.placeholder();
                        rows.push(MatrixRow::Cells {
                            cells: vec![(
                                placeholder,
                                row_start..row_start,
                                navigation_row_start..row_start,
                            )],
                            range: row_start..row_start,
                        });
                    }
                    return Ok(MatrixRows {
                        rows,
                        environment_end_start,
                        environment_content_end: row_start,
                        environment_end: self.pos,
                    });
                }
                Some("hline") => {
                    trailing_row_break = false;
                    self.read_command();
                    rows.push(MatrixRow::Hline(row_start..self.pos));
                    continue;
                }
                Some("intertext") => {
                    trailing_row_break = false;
                    self.read_command();
                    let children = self.parse_text_argument()?;
                    let text = self.node(
                        MathNodeKind::Text,
                        None,
                        &[("style", "text".to_owned())],
                        Some(row_start..self.pos),
                        children,
                    );
                    rows.push(MatrixRow::Intertext(text, row_start..self.pos));
                    continue;
                }
                _ => {}
            }
            if self.peek().is_none() {
                return Err(self.incomplete("missing \\end"));
            }
            let stops = Stops {
                cell: true,
                row_break: true,
                ..Stops::NONE
            };
            let mut cells = Vec::new();
            let mut row_end;
            let mut row_ended_with_break;
            let mut navigation_cell_start = navigation_row_start;
            loop {
                self.skip_space();
                let cell_start = self.pos;
                let (items, ended) = self.parse_list(stops)?;
                row_ended_with_break = ended == Ended::RowBreak;
                let navigation_cell_end = match ended {
                    Ended::Ampersand => self.pos - 1,
                    Ended::RowBreak => self.pos - 2,
                    _ => self.pos,
                };
                let cell_end =
                    cell_start + self.src[cell_start..navigation_cell_end].trim_end().len();
                row_end = cell_end;
                let cell = if items.len() == 1 {
                    items
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| self.placeholder())
                } else {
                    self.row(items, Some(cell_start..cell_end))
                };
                cells.push((
                    cell,
                    cell_start..cell_end,
                    navigation_cell_start..navigation_cell_end,
                ));
                if ended != Ended::Ampersand {
                    break;
                }
                navigation_cell_start = self.pos;
            }
            rows.push(MatrixRow::Cells {
                cells,
                range: row_start..row_end,
            });
            trailing_row_break = row_ended_with_break;
        }
    }
}

/// 列表节点覆盖的源范围。
fn span_of(items: &[MathNode]) -> Option<Range<usize>> {
    let ranges = items.iter().filter_map(|item| item.source_range.as_ref());
    let start = ranges.clone().map(|range| range.start).min()?;
    let end = ranges.map(|range| range.end).max()?;
    Some(start..end)
}

/// 字面字符：由解析器直接从源字符生成且保留首字符原子类别的符号。
fn is_literal_symbol(node: &MathNode, source: &str) -> bool {
    node.kind == MathNodeKind::Symbol
        && node.children.is_empty()
        && node.attributes.len() == 1
        && node.text.as_ref().is_some_and(|text| {
            node.source_range.as_ref().is_some_and(|range| {
                source
                    .get(range.clone())
                    .is_some_and(|token| token == text || is_canonical_char_token(token, text))
            }) && text.chars().next().is_some_and(|first| {
                node.attributes.get("atomClass").map(String::as_str)
                    == Some(literal_class(first).as_str())
            })
        })
}

fn is_canonical_char_token(token: &str, text: &str) -> bool {
    let Some(character) = text.chars().next().filter(|_| text.chars().count() == 1) else {
        return false;
    };
    token == format!("\\char\"{:X} ", character as u32)
}

/// 返回可直接作为数学字面符号读取的字符；TeX 控制 token 不参与字素合并。
fn is_direct_literal(character: char) -> bool {
    !character.is_whitespace()
        && !matches!(
            character,
            '\\' | '%' | '~' | '{' | '}' | '[' | ']' | '&' | '$' | '^' | '_' | '\''
        )
}

fn is_valid_literal_char(character: char) -> bool {
    !character.is_control() && !character.is_whitespace()
}

/// 合并相邻且共同构成同一扩展字素簇的字面符号。
fn merge_literal_grapheme(previous: &mut MathNode, next: &MathNode, source: &str) -> bool {
    if !is_literal_symbol(previous, source) || !is_literal_symbol(next, source) {
        return false;
    }
    let (Some(previous_range), Some(range)) =
        (previous.source_range.as_ref(), next.source_range.as_ref())
    else {
        return false;
    };
    if previous_range.end != range.start {
        return false;
    }
    let (Some(previous_text), Some(next_text)) = (previous.text.as_ref(), next.text.as_ref())
    else {
        return false;
    };
    let mut combined = String::with_capacity(previous_text.len() + next_text.len());
    combined.push_str(previous_text);
    combined.push_str(next_text);
    if combined.graphemes(true).count() != 1 {
        return false;
    }

    if let Some(previous_range) = previous.source_range.as_mut() {
        previous_range.end = range.end;
    }
    if let Some(previous_text) = previous.text.as_mut() {
        previous_text.push_str(next_text);
    }
    true
}

/// 合并组成同一扩展字素簇的字面字符，保持解析与编辑的光标粒度一致。
fn coalesce(items: Vec<MathNode>, source: &str) -> Vec<MathNode> {
    let mut output: Vec<MathNode> = Vec::with_capacity(items.len());
    for item in items {
        if let Some(previous) = output.last_mut()
            && merge_literal_grapheme(previous, &item, source)
        {
            continue;
        }
        output.push(item);
    }
    output
}

/// 解析 `em` 或 `mu` 长度为有理数 em。
fn parse_em(raw: &str) -> Option<(i128, i128)> {
    let raw = raw.trim();
    let (number, unit) = raw.split_at(raw.find(|c: char| c.is_ascii_alphabetic())?);
    let unit = unit.trim();
    let number = number.trim();
    let (negative, digits) = match number.strip_prefix('-') {
        Some(rest) => (true, rest.trim()),
        None => (false, number.strip_prefix('+').unwrap_or(number).trim()),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() && fraction.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
        || whole.len() + fraction.len() > 18
    {
        return None;
    }
    let mut numerator: i128 = format!("{whole}{fraction}").parse().ok()?;
    let mut denominator: i128 = 10i128.pow(fraction.len() as u32);
    match unit {
        "em" => {}
        "mu" => denominator *= 18,
        _ => return None,
    }
    if negative {
        numerator = -numerator;
    }
    Some(reduce((numerator, denominator)))
}

fn gcd(mut a: i128, mut b: i128) -> i128 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

fn reduce((numerator, denominator): (i128, i128)) -> (i128, i128) {
    let divisor = gcd(numerator, denominator);
    (numerator / divisor, denominator / divisor)
}

fn add_ratio(left: (i128, i128), right: (i128, i128)) -> (i128, i128) {
    reduce((left.0 * right.1 + right.0 * left.1, left.1 * right.1))
}

#[cfg(test)]
mod text_accumulator_tests {
    use super::*;

    #[test]
    fn text_runs_match_with_and_without_source_capture() {
        let source = r"\text{a\%中 b}\text{c}";
        let ParsedMath::Tree(plain) = parse(source, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("text input should parse: {error}"))
        else {
            panic!("inline input should produce a tree");
        };
        let (crate::ProjectedMath::Tree(projected), map) =
            parse_projection(source, MathInputKind::Inline)
                .unwrap_or_else(|error| panic!("text input should project: {error}"))
                .into_parts()
        else {
            panic!("inline input should produce a tree");
        };
        let runs = |tree: &MathTree| {
            tree.root
                .content
                .iter()
                .map(|item| match item {
                    MathContent::Text(flow) => flow
                        .runs
                        .iter()
                        .map(|run| run.text.clone())
                        .collect::<Vec<_>>(),
                    _ => panic!("expected text flow"),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(runs(&plain), runs(&projected));
        assert_eq!(runs(&plain).concat().concat(), "a%中 bc");
        assert!(map.tree().is_some());
    }

    #[test]
    fn parsed_trees_keep_positions_only_in_sidecars() {
        for (source, kind) in [
            (r"\frac{\text{a\%中}}{\textbf{b}}", MathInputKind::Inline),
            (
                r"\begin{aligned}x&\text{a\%中}\end{aligned}",
                MathInputKind::Align,
            ),
        ] {
            let projection = parse_projection(source, kind)
                .unwrap_or_else(|error| panic!("input should project: {error}"));
            match projection.content() {
                crate::ProjectedMath::Tree(tree) => {
                    assert!(tree.validate().is_ok());
                    assert!(projection.source_map().tree().is_some());
                }
                crate::ProjectedMath::Align(align) => {
                    assert!(align.validate().is_ok());
                    let map = projection
                        .source_map()
                        .align()
                        .unwrap_or_else(|| panic!("Align sidecar"));
                    let range = map
                        .cell_range(0, 1)
                        .unwrap_or_else(|| panic!("second cell range"));
                    assert_eq!(&source[range], r"\text{a\%中}");
                    assert!(map.cell_tree(0, 1).is_some());
                }
            }
        }
    }

    #[test]
    fn text_comments_preserve_discontinuous_source_segments() {
        for source in [
            "\\text{ab% skipped by TeX\ncd}",
            "\\text{ab% skipped by TeX\r\ncd}",
        ] {
            let projection = parse_projection(source, MathInputKind::Inline)
                .unwrap_or_else(|error| panic!("commented text should parse: {error}"));
            let crate::ProjectedMath::Tree(tree) = projection.content() else {
                panic!("inline input should produce a tree");
            };
            let [MathContent::Text(flow)] = tree.root.content.as_slice() else {
                panic!("commented text should remain one TextFlow");
            };
            assert_eq!(
                flow.runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
                "abcd"
            );
            let source_map = projection
                .source_map()
                .tree()
                .and_then(|map| map.root().content().first())
                .and_then(MathContentSourceMap::text)
                .unwrap_or_else(|| panic!("TextFlow should have a source sidecar"));
            assert_eq!(source_map.segments().len(), 2);
            let Some(ab_start) = source.find("ab") else {
                panic!("source should contain the first text segment");
            };
            assert_eq!(
                source_map.segments()[0].source(),
                Some(ab_start..ab_start + 2)
            );
            let Some(cd_start) = source.find("cd") else {
                panic!("source should contain the second text segment");
            };
            assert_eq!(
                source_map.segments()[1].source(),
                Some(cd_start..cd_start + 2)
            );
            assert_eq!(source_map.segments()[0].display(), 0..2);
            assert_eq!(source_map.segments()[1].display(), 2..4);
        }
    }

    #[test]
    fn text_comment_boundaries_and_escaped_percent_are_handled() {
        let source = r"\text{a\%b}";
        let projection = parse_projection(source, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("escaped percent should parse: {error}"));
        let crate::ProjectedMath::Tree(tree) = projection.content() else {
            panic!("inline input should produce a tree");
        };
        let [MathContent::Text(flow)] = tree.root.content.as_slice() else {
            panic!("escaped percent should remain one TextFlow");
        };
        assert_eq!(
            flow.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "a%b"
        );

        let trailing_comment = "\\text{ab% trailing comment\r\n}";
        let ParsedMath::Tree(tree) = parse(trailing_comment, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("trailing comment before brace should parse: {error}"))
        else {
            panic!("inline input should produce a tree");
        };
        let [MathContent::Text(flow)] = tree.root.content.as_slice() else {
            panic!("trailing comment should preserve the preceding text");
        };
        assert_eq!(flow.runs[0].text, "ab");

        assert!(matches!(
            parse(r"\text{ab% trailing comment", MathInputKind::Inline),
            Err(MathParseError::Parse {
                incomplete: true,
                ..
            })
        ));
    }
}
