//! LaTeX 数学子集解析器，直接构造 `caret-doc` 数学树。
//!
//! 解析过程只做一次自顶向下扫描：数学模式跳过空白与 `%` 注释，文本模式逐字符
//! 保留空格。所有节点记录相对输入的 UTF-8 字节范围；源码空花括号产生的占位节点记录花括号范围，
//! 由结构补齐的占位节点没有范围。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::collections::BTreeMap;
use std::ops::Range;

use caret_doc::{
    Align, InlineEnvironment, InlineEnvironmentKind, MATH_TREE_SCHEMA_VERSION,
    MAX_MATH_SOURCE_BYTES, MAX_MATH_TREE_DEPTH, MAX_MATH_TREE_NODES, MathAlignRow, MathContent,
    MathFlow, MathSourceRange, MathTree, NodeId, Run, TextFlow,
};
use serde_json::Value;

use crate::backend::BackendNodeKind as MathNodeKind;
use crate::symbols::{
    NAMED_COLORS, accent_attribute, encode_delimiter, literal_class, named_symbol,
    text_operator_limits,
};
use crate::{MathInputKind, MathInputStatus, MathParseError, ParsedMath};

type ParseResult<T> = Result<T, MathParseError>;

/// LaTeX parser 暂存用节点；解析完成后立即降为持久化内容流模型。
#[derive(Debug, Clone)]
struct MathNode {
    id: NodeId,
    kind: MathNodeKind,
    text: Option<String>,
    attributes: BTreeMap<String, String>,
    source_range: Option<MathSourceRange>,
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
            let tree = MathTree {
                schema_version: MATH_TREE_SCHEMA_VERSION,
                source: source.to_owned(),
                root: parser_root_to_flow(root),
                extra: BTreeMap::new(),
            };
            tree.validate().map_err(invalid_tree)?;
            Ok(ParsedMath::Tree(tree))
        }
        MathInputKind::Align => parse_align(&mut parser, source, body),
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

fn parse_align(
    parser: &mut Parser<'_>,
    source: &str,
    body: Range<usize>,
) -> ParseResult<ParsedMath> {
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
    let mut rows = parser.parse_matrix_rows("aligned")?;
    let env_end = parser.pos;
    parser.skip_space();
    if parser.pos < body.end {
        return Err(MathParseError::InputKindMismatch {
            expected: MathInputKind::Align,
        });
    }
    if rows.is_empty() {
        let placeholder = parser.placeholder();
        rows.push(MatrixRow::Cells {
            cells: vec![(placeholder, env_end..env_end)],
            range: env_end..env_end,
        });
    }
    let mut align_rows = Vec::new();
    for row in rows {
        let MatrixRow::Cells { cells, .. } = row else {
            return Err(parser.error_at(
                env_start,
                "aligned rows cannot contain \\hline or \\intertext",
            ));
        };
        let mut trees = Vec::new();
        for (mut node, span) in cells {
            rebase(&mut node, span.start);
            check_depth(&node)?;
            let tree = MathTree {
                schema_version: MATH_TREE_SCHEMA_VERSION,
                source: source[span].to_owned(),
                root: parser_root_to_flow(node),
                extra: BTreeMap::new(),
            };
            tree.validate().map_err(invalid_tree)?;
            trees.push(tree);
        }
        align_rows.push(MathAlignRow {
            id: NodeId::generate(),
            cells: trees,
            extra: BTreeMap::new(),
        });
    }
    let align = Align {
        id: NodeId::generate(),
        source: source[env_start..env_end].to_owned(),
        rows: align_rows,
        extra: BTreeMap::new(),
    };
    align.validate().map_err(invalid_tree)?;
    Ok(ParsedMath::Align(align))
}

fn parser_root_to_flow(root: MathNode) -> MathFlow {
    if root.kind == MathNodeKind::Row {
        return parser_row_to_flow(root);
    }
    let id = NodeId::generate();
    let mut content = Vec::new();
    append_parser_node(&mut content, root);
    MathFlow {
        id,
        content,
        source_range: None,
        extra: BTreeMap::new(),
    }
}

fn parser_slot_to_flow(node: MathNode) -> MathFlow {
    if node.kind == MathNodeKind::Row {
        return parser_row_to_flow(node);
    }
    let source_range = node.source_range.clone();
    let id = NodeId::generate();
    let mut content = Vec::new();
    append_parser_node(&mut content, node);
    MathFlow {
        id,
        content,
        source_range,
        extra: BTreeMap::new(),
    }
}

fn parser_row_to_flow(row: MathNode) -> MathFlow {
    let mut content = Vec::new();
    for node in row.children {
        append_parser_node(&mut content, node);
    }
    MathFlow {
        id: row.id,
        content,
        source_range: row.source_range,
        extra: row.extra,
    }
}

fn append_parser_node(content: &mut Vec<MathContent>, node: MathNode) {
    match node.kind {
        MathNodeKind::Row => {
            for child in node.children {
                append_parser_node(content, child);
            }
        }
        MathNodeKind::TextContent => {
            push_math_text(content, node.text.unwrap_or_default(), node.source_range);
        }
        MathNodeKind::TextSpace => {
            let text = match node.text.as_deref() {
                Some("no_break_space") => "\u{00a0}",
                Some("control_space") | Some("space") | None => " ",
                Some(_) => " ",
            };
            push_math_text(content, text.to_owned(), node.source_range);
        }
        MathNodeKind::TextSymbol => {
            let text = node.text.unwrap_or_default();
            push_math_text(content, text, node.source_range);
        }
        MathNodeKind::Text
            if node
                .attributes
                .get("style")
                .is_some_and(|style| style == "text") =>
        {
            for child in node.children {
                append_parser_node(content, child);
            }
        }
        MathNodeKind::TextStyle
            if node
                .attributes
                .get("style")
                .is_some_and(|style| matches!(style.as_str(), "bf" | "it" | "sf" | "tt")) =>
        {
            let style = node
                .attributes
                .get("style")
                .map(String::as_str)
                .unwrap_or_default();
            let (kind, props) = match style {
                "bf" => (InlineEnvironmentKind::Bold, BTreeMap::new()),
                "it" => (InlineEnvironmentKind::Italic, BTreeMap::new()),
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
                _ => unreachable!(),
            };
            let mut children = Vec::new();
            for child in node.children {
                append_parser_node(&mut children, child);
            }
            content.push(MathContent::Environment(InlineEnvironment {
                id: node.id,
                kind,
                definition_id: None,
                source_range: node.source_range,
                props,
                children,
                extra: node.extra,
            }));
        }
        _ => {
            let Some(kind) = node.kind.persisted() else {
                match node.kind {
                    MathNodeKind::TextContent | MathNodeKind::TextSymbol => {
                        push_math_text(content, node.text.unwrap_or_default(), node.source_range);
                    }
                    MathNodeKind::TextSpace => {
                        push_math_text(content, " ".to_owned(), node.source_range);
                    }
                    MathNodeKind::TextStyle => {
                        for child in node.children {
                            append_parser_node(content, child);
                        }
                    }
                    _ => {}
                }
                return;
            };
            let children = node.children.into_iter().map(parser_slot_to_flow).collect();
            content.push(MathContent::Math(caret_doc::MathNode {
                id: node.id,
                kind,
                text: node.text,
                attributes: node.attributes,
                source_range: node.source_range,
                children,
                extra: node.extra,
            }));
        }
    }
}

fn push_math_text(
    content: &mut Vec<MathContent>,
    text: String,
    source_range: Option<MathSourceRange>,
) {
    if let Some(MathContent::Text(flow)) = content.last_mut() {
        let contiguous = flow
            .source_range
            .as_ref()
            .zip(source_range.as_ref())
            .is_some_and(|(previous, next)| previous.end == next.start);
        if contiguous {
            flow.runs.push(Run {
                text,
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            });
            if let (Some(previous), Some(next)) = (&mut flow.source_range, source_range) {
                previous.end = next.end;
            }
            return;
        }
    }
    content.push(MathContent::Text(TextFlow {
        runs: vec![Run {
            text,
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        source_range,
        extra: BTreeMap::new(),
    }));
}

/// 把节点范围平移为相对单元格源码。
fn rebase(node: &mut MathNode, offset: usize) {
    if let Some(range) = node.source_range.as_mut() {
        range.start -= offset as u32;
        range.end -= offset as u32;
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
        cells: Vec<(MathNode, Range<usize>)>,
        range: Range<usize>,
    },
    Hline(Range<usize>),
    Intertext(MathNode, Range<usize>),
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
            source_range: range.map(|range| MathSourceRange {
                start: range.start as u32,
                end: range.end as u32,
                extra: BTreeMap::new(),
            }),
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
        Ok((coalesce(items), ended))
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
                }
                Some('_') => {
                    if subscript.is_some() {
                        return Err(self.error_at(self.pos, "double subscript"));
                    }
                    self.bump();
                    subscript = Some(self.parse_argument()?);
                }
                _ => break,
            }
            // `\limits` 只影响显示，不进入文档树。
            self.skip_space();
            while matches!(self.peek_command(), Some("limits" | "nolimits")) {
                self.read_command();
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
        let range = Some(start..self.pos);
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
            base.source_range = range.map(|range| MathSourceRange {
                start: range.start as u32,
                end: range.end as u32,
                extra: BTreeMap::new(),
            });
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
            Some(_) => self.parse_atom(),
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
                let rows = self.parse_matrix_rows(style)?;
                let mut children = Vec::new();
                for row in rows {
                    children.push(match row {
                        MatrixRow::Cells { cells, range } => {
                            let cells = cells.into_iter().map(|(cell, _)| cell).collect();
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
            "text" | "mbox" | "textrm" | "textbf" | "textit" | "textsf" | "texttt" => {
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
                    K::Text,
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
        let mut run_start: Option<usize> = None;
        let mut depth = 0usize;
        loop {
            self.check_budget()?;
            let start = self.pos;
            let Some(character) = self.peek() else {
                return Err(self.incomplete("missing `}`"));
            };
            let breaks_run =
                character.is_whitespace() || matches!(character, '{' | '}' | '~' | '$' | '\\');
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
                if matches!(character, '^' | '_' | '&' | '#' | '%') {
                    return Err(self.error_at(start, "unexpected character in text"));
                }
                run_start.get_or_insert(start);
                self.bump();
                continue;
            }
            match character {
                '}' => {
                    self.bump();
                    if depth == 0 {
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
                    let node = if math.len() == 1 {
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
                        "textrm" | "textbf" | "textit" | "textsf" | "texttt" => {
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
    fn parse_matrix_rows(&mut self, name: &str) -> ParseResult<Vec<MatrixRow>> {
        let mut rows = Vec::new();
        loop {
            self.check_budget()?;
            self.skip_space();
            let row_start = self.pos;
            match self.peek_command() {
                Some("end") => {
                    self.read_command();
                    let end_name = self.read_raw_group()?;
                    if end_name != name {
                        return Err(self.error_at(row_start, "mismatched \\end"));
                    }
                    return Ok(rows);
                }
                Some("hline") => {
                    self.read_command();
                    rows.push(MatrixRow::Hline(row_start..self.pos));
                    continue;
                }
                Some("intertext") => {
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
            loop {
                self.skip_space();
                let cell_start = self.pos;
                let (items, ended) = self.parse_list(stops)?;
                let cell_end = match ended {
                    Ended::Ampersand => self.pos - 1,
                    Ended::RowBreak => self.pos - 2,
                    _ => self.pos,
                };
                let cell_end = cell_start + self.src[cell_start..cell_end].trim_end().len();
                row_end = cell_end;
                let cell = if items.len() == 1 {
                    items
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| self.placeholder())
                } else {
                    self.row(items, Some(cell_start..cell_end))
                };
                cells.push((cell, cell_start..cell_end));
                if ended != Ended::Ampersand {
                    break;
                }
            }
            rows.push(MatrixRow::Cells {
                cells,
                range: row_start..row_end,
            });
        }
    }
}

/// 列表节点覆盖的源范围。
fn span_of(items: &[MathNode]) -> Option<Range<usize>> {
    let ranges = items.iter().filter_map(|item| item.source_range.as_ref());
    let start = ranges.clone().map(|range| range.start).min()?;
    let end = ranges.map(|range| range.end).max()?;
    Some(start as usize..end as usize)
}

/// 字面普通字符：由解析器直接从源字符生成、带 `atomClass=ord` 的符号。
fn is_literal_ord(node: &MathNode) -> bool {
    node.kind == MathNodeKind::Symbol
        && node.children.is_empty()
        && node.attributes.get("atomClass").map(String::as_str) == Some("ord")
        && node.attributes.len() == 1
}

/// 合并相邻的字面普通字符，使 `xy12` 成为一个可整体编辑的符号。
fn coalesce(items: Vec<MathNode>) -> Vec<MathNode> {
    let mut output: Vec<MathNode> = Vec::with_capacity(items.len());
    for item in items {
        if let Some(previous) = output.last_mut()
            && is_literal_ord(previous)
            && is_literal_ord(&item)
            && let (Some(previous_range), Some(range)) =
                (previous.source_range.as_mut(), item.source_range.as_ref())
        {
            previous_range.end = range.end;
            if let (Some(text), Some(next)) = (previous.text.as_mut(), item.text.as_ref()) {
                text.push_str(next);
            }
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
