//! 为 Caret 隔离 LaTeX 数学解析器版本、输入预算与导入诊断。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use latex_rust::{
    EnvRow, EqNumber, MathNode, MatrixStyle, ParseError, ParseOptions, TextItem, parse_with_options,
};
use std::fmt::{Display, Formatter};
use std::ops::Range;

pub use math_syntax::{MathSyntaxConversionError, MathSyntaxKind, MathSyntaxNode, MathSyntaxTree};

mod layout;
mod math_syntax;

pub use latex_rust::{
    STIX_TWO_MATH_NAME as MATH_FONT_NAME, STIX_TWO_MATH_OTF as MATH_FONT_BYTES,
    STIX_TWO_MATH_SHA256 as MATH_FONT_SHA256,
};
pub use layout::{
    MathLayoutBackend, MathLayoutDiagnostic, MathLayoutError, MathLayoutItem, MathLayoutSnapshot,
    MathNodeGeometry, MathTextMetrics, MathTextShaper, MathTextStyle,
};

/// 单个公式允许的最大 UTF-8 源码长度。
pub const MAX_MATH_SOURCE_BYTES: usize = 1024 * 1024;
/// Caret 文档模型允许的最大数学树嵌套预算。
pub const MAX_MATH_NESTING_DEPTH: usize = 32;
/// 单次解析允许的最大转换语法节点数，限制临时树的内存放大。
pub const MAX_MATH_SYNTAX_NODES: usize = 65_536;

/// 调用方文档节点要求的公式形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathInputKind {
    /// 段落中的行内公式。
    Inline,
    /// 单行无编号显示公式。
    Equation,
    /// 多行无编号对齐公式。
    Align,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExplicitFenceKind {
    Inline,
    Display,
}

fn explicit_fence_kind(source: &str) -> Option<ExplicitFenceKind> {
    let source = source.trim();
    let has_fence = |open: &str, close: &str| {
        source
            .strip_prefix(open)
            .is_some_and(|inner| inner.ends_with(close) && inner.len() > close.len())
    };

    if has_fence("$$", "$$") || has_fence(r"\[", r"\]") {
        Some(ExplicitFenceKind::Display)
    } else if has_fence("$", "$") || has_fence(r"\(", r"\)") {
        Some(ExplicitFenceKind::Inline)
    } else {
        None
    }
}

/// 通过语法与输入种类校验的公式及其原始导入文本范围。
///
/// 上游 AST 仅在该值的私有生命周期内保留，不通过公开 API 暴露或写入 `.cdoc`。
#[derive(Debug)]
pub struct ParsedMath<'source> {
    source: &'source str,
    source_range: Range<usize>,
    normalized_range: Range<usize>,
    input_kind: MathInputKind,
    ast: MathNode,
}

impl<'source> ParsedMath<'source> {
    /// 返回导入时的原始公式源码。
    pub fn source(&self) -> &'source str {
        self.source
    }

    /// 返回完整原始公式在输入缓冲区中的 UTF-8 字节范围。
    ///
    /// latex-rust 3.0.1 不公开 token 或 AST 节点的源码跨度，因此当前范围
    /// 仅精确到整段公式。细粒度节点跨度须在 parser adapter patch 中补齐。
    pub fn source_range(&self) -> Range<usize> {
        self.source_range.clone()
    }

    /// 返回去除首尾空白后交给解析器的字节范围。
    pub fn normalized_range(&self) -> Range<usize> {
        self.normalized_range.clone()
    }

    /// 将第三方 parser AST 转成 `caret-math` 自有的中间数学树。
    ///
    /// 中间树只含稳定值类型，不暴露 `latex-rust` 类型。转换在需要新节点
    /// kind 时显式失败，避免静默丢弃未来语法。
    pub fn syntax_tree(&self) -> Result<MathSyntaxTree, MathSyntaxConversionError> {
        let mut tree = math_syntax::convert(self.input_kind, &self.ast)?;
        tree.root.source_range = Some(self.source_range.clone());
        Ok(tree)
    }
}

/// 解析或输入约束错误；错误保留完整原文及其可定位字节范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathParseError<'source> {
    /// 输入超过单公式字节数限制。
    InputTooLong {
        /// 超出限制的原始文本。
        source: &'source str,
        /// 原文 UTF-8 字节范围。
        source_range: Range<usize>,
        /// 配置的最大字节数。
        limit: usize,
    },
    /// parser 输出树超过 Caret 支持的最大嵌套深度。
    InputTooDeep {
        /// 超出限制的原始文本。
        source: &'source str,
        /// 原文 UTF-8 字节范围。
        source_range: Range<usize>,
        /// 配置的最大嵌套深度。
        limit: usize,
    },
    /// 转换后的语法树超过节点预算。
    SyntaxTreeTooLarge {
        /// 超出限制的原始文本。
        source: &'source str,
        /// 原文 UTF-8 字节范围。
        source_range: Range<usize>,
        /// 允许的最大节点数。
        limit: usize,
    },
    /// 首版不支持编号、交叉引用或标签语义。
    UnsupportedNumbering {
        /// 含不支持结构的完整原始文本。
        source: &'source str,
        /// 原文 UTF-8 字节范围；当前 parser API 只能定位整段公式。
        source_range: Range<usize>,
        /// 触发拒绝的 LaTeX 命令。
        command: String,
    },
    /// 输入树结构与调用方要求的公式种类不匹配。
    InputKindMismatch {
        /// 原始文本。
        source: &'source str,
        /// 原文 UTF-8 字节范围。
        source_range: Range<usize>,
        /// 期待的 Caret 公式类型。
        expected: MathInputKind,
    },
    /// 上游解析器拒绝输入。
    Parse {
        /// 解析失败时保留的完整原始文本。
        source: &'source str,
        /// 原文 UTF-8 字节范围；当前 parser API 只能定位整段公式。
        source_range: Range<usize>,
        /// 上游解析器的诊断。
        message: String,
        /// parser 的明确语法边界表明继续追加输入仍可能完成表达式。
        incomplete: bool,
    },
}

/// 单次公式输入目前能否提交、继续输入或需要错误提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathInputStatus {
    /// 当前完整输入可由 parser 接受。
    Complete,
    /// parser 明确指出仍缺少输入，或输入停在可延长的 LaTeX 前缀。
    Incomplete,
    /// 当前输入被 parser 拒绝，追加内容不能按已知语法边界安全推断为补全。
    Invalid,
}

impl Display for MathParseError<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputTooLong { limit, .. } => {
                write!(formatter, "math.input_too_long: maximum {limit} bytes")
            }
            Self::InputTooDeep { limit, .. } => {
                write!(formatter, "math.input_too_deep: maximum depth {limit}")
            }
            Self::SyntaxTreeTooLarge { limit, .. } => {
                write!(
                    formatter,
                    "math.syntax_tree_too_large: maximum nodes {limit}"
                )
            }
            Self::UnsupportedNumbering { command, .. } => {
                write!(formatter, "math.unsupported_numbering: \\{command}")
            }
            Self::InputKindMismatch { expected, .. } => {
                write!(formatter, "math.input_kind_mismatch: {expected:?}")
            }
            Self::Parse { message, .. } => write!(formatter, "math.parse: {message}"),
        }
    }
}

impl std::error::Error for MathParseError<'_> {}

/// 使用固定递归预算解析 LaTeX 数学输入。
///
/// 不修改输入、不静默删除编号语义，并在任何解析错误中保留完整源码。
pub fn parse(source: &str, kind: MathInputKind) -> Result<ParsedMath<'_>, MathParseError<'_>> {
    let full_range = 0..source.len();
    if source.len() > MAX_MATH_SOURCE_BYTES {
        return Err(MathParseError::InputTooLong {
            source,
            source_range: full_range,
            limit: MAX_MATH_SOURCE_BYTES,
        });
    }

    let normalized = latex_rust::preprocess(source);
    let start = source.find(&normalized).unwrap_or(0);
    let normalized_range = start..start + normalized.len();
    let options = ParseOptions::new().with_max_depth(MAX_MATH_NESTING_DEPTH);
    let (ast, _) = parse_with_options(source, &options).map_err(|error| {
        let incomplete = parse_error_is_incomplete(source, &error);
        MathParseError::Parse {
            source,
            source_range: full_range.clone(),
            message: parse_error_message(error),
            incomplete,
        }
    })?;

    match math_syntax::check_ast_budget(&ast, MAX_MATH_NESTING_DEPTH, MAX_MATH_SYNTAX_NODES) {
        Ok(()) => {}
        Err(math_syntax::AstBudgetError::TooDeep) => {
            return Err(MathParseError::InputTooDeep {
                source,
                source_range: full_range,
                limit: MAX_MATH_NESTING_DEPTH,
            });
        }
        Err(math_syntax::AstBudgetError::TooManyNodes) => {
            return Err(MathParseError::SyntaxTreeTooLarge {
                source,
                source_range: full_range,
                limit: MAX_MATH_SYNTAX_NODES,
            });
        }
        Err(math_syntax::AstBudgetError::UnsupportedNode) => {
            return Err(MathParseError::Parse {
                source,
                source_range: full_range,
                message: "math.syntax_conversion_unsupported: future latex-rust node".to_owned(),
                incomplete: false,
            });
        }
    }

    if let Some(command) = unsupported_numbering_semantic(&ast) {
        return Err(MathParseError::UnsupportedNumbering {
            source,
            source_range: full_range,
            command,
        });
    }
    let fence_mismatches_kind = matches!(
        (explicit_fence_kind(source), kind),
        (
            Some(ExplicitFenceKind::Inline),
            MathInputKind::Equation | MathInputKind::Align
        ) | (Some(ExplicitFenceKind::Display), MathInputKind::Inline)
    );
    let is_aligned = matches!(&ast, MathNode::Matrix(MatrixStyle::Aligned, _, _));
    if fence_mismatches_kind || (kind == MathInputKind::Align) != is_aligned {
        return Err(MathParseError::InputKindMismatch {
            source,
            source_range: full_range,
            expected: kind,
        });
    }

    Ok(ParsedMath {
        source,
        source_range: full_range,
        normalized_range,
        input_kind: kind,
        ast,
    })
}

/// 判断 parser 拒绝的输入是否还可能因后续键入而成为完整表达式。
///
/// 仅识别输入边界状态，不推断数学语义；最终接受始终由 [`parse`] 决定。
pub fn is_incomplete_input_prefix(source: &str) -> bool {
    let mut brace_depth = 0usize;
    let mut consecutive_backslashes = 0usize;
    for character in source.chars() {
        if character == '\\' {
            consecutive_backslashes = consecutive_backslashes.saturating_add(1);
            continue;
        }
        let escaped = consecutive_backslashes % 2 == 1;
        if !escaped {
            match character {
                '{' => brace_depth = brace_depth.saturating_add(1),
                '}' if brace_depth > 0 => brace_depth -= 1,
                _ => {}
            }
        }
        consecutive_backslashes = 0;
    }
    if brace_depth > 0 || consecutive_backslashes % 2 == 1 {
        return true;
    }

    let Some(command_start) = source.rfind('\\') else {
        return false;
    };
    let command = &source[command_start + 1..];
    if command.is_empty() || !command.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return false;
    }
    let preceding_slashes = source[..command_start]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count();
    preceding_slashes % 2 == 0
}

/// 用 parser 和有限的输入边界规则分类当前缓冲。
pub fn classify_input(source: &str, kind: MathInputKind) -> MathInputStatus {
    match parse(source, kind) {
        Ok(_) if trailing_control_word_prefix_start(source).is_some() => {
            MathInputStatus::Incomplete
        }
        Ok(_) => MathInputStatus::Complete,
        Err(MathParseError::Parse {
            incomplete: true,
            message,
            ..
        }) if message == "trailing backslash"
            && !source
                .strip_suffix('\\')
                .is_some_and(|prefix| parse_prefix_is_viable(prefix, kind)) =>
        {
            MathInputStatus::Invalid
        }
        Err(MathParseError::Parse {
            incomplete: true, ..
        }) => MathInputStatus::Incomplete,
        Err(MathParseError::Parse { .. })
            if trailing_control_word_prefix_is_viable(source, kind) =>
        {
            MathInputStatus::Incomplete
        }
        Err(_) => MathInputStatus::Invalid,
    }
}

fn trailing_control_word_prefix_start(source: &str) -> Option<usize> {
    let command_start = source.rfind('\\')?;
    let command = &source[command_start + 1..];
    if command.is_empty() || !command.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let preceding_slashes = source[..command_start]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count();
    (preceding_slashes % 2 == 0).then_some(command_start)
}

fn trailing_control_word_prefix_is_viable(source: &str, kind: MathInputKind) -> bool {
    let Some(command_start) = trailing_control_word_prefix_start(source) else {
        return false;
    };
    let prefix = &source[..command_start];
    parse_prefix_is_viable(prefix, kind)
}

fn parse_prefix_is_viable(prefix: &str, kind: MathInputKind) -> bool {
    if prefix.is_empty() {
        return true;
    }
    matches!(
        parse(prefix, kind),
        Ok(_)
            | Err(MathParseError::Parse {
                incomplete: true,
                ..
            })
    )
}

fn parse_error_is_incomplete(source: &str, error: &ParseError) -> bool {
    match error {
        ParseError::TrailingBackslash => true,
        ParseError::Malformed(message) => {
            matches!(
                message.as_str(),
                "missing argument" | "unexpected end of input" | "unmatched '{'"
            )
        }
        ParseError::UnmatchedDelimiter => has_unmatched_left_delimiter(source),
        ParseError::Unknown(_) | ParseError::Unsupported(_) | ParseError::RowBreakOutside => false,
        _ => false,
    }
}

fn has_unmatched_left_delimiter(source: &str) -> bool {
    let mut left_depth = 0usize;
    let mut backslash_run = 0usize;
    let mut characters = source.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if character != '\\' {
            backslash_run = 0;
            continue;
        }
        if backslash_run % 2 == 1 {
            backslash_run += 1;
            continue;
        }
        let mut end = index + character.len_utf8();
        while let Some((next_index, next)) = characters.peek().copied() {
            if !next.is_ascii_alphabetic() {
                break;
            }
            end = next_index + next.len_utf8();
            characters.next();
        }
        match &source[index + 1..end] {
            "left" => {
                left_depth = left_depth.saturating_add(1);
                backslash_run = 0;
            }
            "right" if left_depth > 0 => {
                left_depth -= 1;
                backslash_run = 0;
            }
            "right" => return false,
            _ if end == index + character.len_utf8() => backslash_run = 1,
            _ => backslash_run = 0,
        }
    }
    left_depth > 0
}

fn parse_error_message(error: ParseError) -> String {
    error.to_string()
}

fn unsupported_numbering_semantic(node: &MathNode) -> Option<String> {
    match node {
        MathNode::Ref(_) => return Some("ref".to_owned()),
        MathNode::Label(_) => return Some("label".to_owned()),
        MathNode::Tag { .. } => return Some("tag".to_owned()),
        MathNode::NoNumber => return Some("nonumber".to_owned()),
        MathNode::Matrix(style, _, rows) => {
            let numbered_environment = matches!(
                style,
                MatrixStyle::Align
                    | MatrixStyle::Equation
                    | MatrixStyle::Gather
                    | MatrixStyle::Multline
            );
            if numbered_environment {
                return Some(format!(
                    "begin{{{}}}",
                    math_syntax::matrix_style_name(*style)
                ));
            }
            for row in rows {
                match row {
                    EnvRow::Cells {
                        cells,
                        number,
                        labels,
                    } => {
                        if !matches!(number, EqNumber::Default) {
                            return Some("tag".to_owned());
                        }
                        if !labels.is_empty() {
                            return Some("label".to_owned());
                        }
                        if let Some(command) = cells.iter().find_map(unsupported_numbering_semantic)
                        {
                            return Some(command);
                        }
                    }
                    EnvRow::Intertext(body) => {
                        if let Some(command) = unsupported_numbering_semantic(body) {
                            return Some(command);
                        }
                    }
                    EnvRow::Hline => {}
                }
            }
            return None;
        }
        MathNode::Fraction(_, numerator, denominator)
        | MathNode::Superscript(numerator, denominator)
        | MathNode::Subscript(numerator, denominator)
        | MathNode::CancelTo(numerator, denominator) => {
            return unsupported_numbering_semantic(numerator)
                .or_else(|| unsupported_numbering_semantic(denominator));
        }
        MathNode::Radical(index, body) => {
            return index
                .as_ref()
                .and_then(|index| unsupported_numbering_semantic(index))
                .or_else(|| unsupported_numbering_semantic(body));
        }
        MathNode::SubSup(base, sub, sup) => {
            return unsupported_numbering_semantic(base)
                .or_else(|| unsupported_numbering_semantic(sub))
                .or_else(|| unsupported_numbering_semantic(sup));
        }
        MathNode::Delimited(_, body, _) | MathNode::Intertext(body) => {
            return unsupported_numbering_semantic(body);
        }
        MathNode::Row(items) | MathNode::Substack(items) => {
            return items.iter().find_map(unsupported_numbering_semantic);
        }
        MathNode::Sum(lower, upper) | MathNode::Product(lower, upper) => {
            return lower
                .as_ref()
                .and_then(|lower| unsupported_numbering_semantic(lower))
                .or_else(|| {
                    upper
                        .as_ref()
                        .and_then(|upper| unsupported_numbering_semantic(upper))
                });
        }
        MathNode::Integral(_, lower, upper) => {
            return lower
                .as_ref()
                .and_then(|lower| unsupported_numbering_semantic(lower))
                .or_else(|| {
                    upper
                        .as_ref()
                        .and_then(|upper| unsupported_numbering_semantic(upper))
                });
        }
        MathNode::Limit(lower) => {
            return lower
                .as_ref()
                .and_then(|lower| unsupported_numbering_semantic(lower));
        }
        MathNode::OverUnder(base, over, under) => {
            return unsupported_numbering_semantic(base)
                .or_else(|| {
                    over.as_ref()
                        .and_then(|over| unsupported_numbering_semantic(over))
                })
                .or_else(|| {
                    under
                        .as_ref()
                        .and_then(|under| unsupported_numbering_semantic(under))
                });
        }
        MathNode::Accent(body, _)
        | MathNode::Color(_, body)
        | MathNode::TextColor(_, body)
        | MathNode::ColorBox(_, body)
        | MathNode::FColorBox(_, _, body)
        | MathNode::Phantom(_, body) => {
            return unsupported_numbering_semantic(body);
        }
        MathNode::Text(run, _) => {
            return unsupported_numbering_in_text(run);
        }
        MathNode::Atom(_, _)
        | MathNode::SizedDelim(_, _, _)
        | MathNode::Hline
        | MathNode::Space(_)
        | MathNode::Operator(_, _)
        | MathNode::Symbol(_)
        | MathNode::Strut(_, _) => {}
        _ => {}
    }
    None
}

fn unsupported_numbering_in_text(run: &latex_rust::TextRun) -> Option<String> {
    run.items().iter().find_map(|item| match item {
        TextItem::Math(math) => unsupported_numbering_semantic(math),
        TextItem::Nested(_, nested) => unsupported_numbering_in_text(nested),
        TextItem::Str(_)
        | TextItem::Space
        | TextItem::ControlSpace
        | TextItem::NoBreakSpace
        | TextItem::Symbol(_) => None,
    })
}

#[cfg(test)]
mod tests;
