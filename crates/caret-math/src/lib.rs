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

mod math_syntax;

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
        math_syntax::convert(self.input_kind, &self.ast)
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
    },
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
    let (ast, _) = parse_with_options(source, &options).map_err(|error| MathParseError::Parse {
        source,
        source_range: full_range.clone(),
        message: parse_error_message(error),
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
    let is_aligned = matches!(&ast, MathNode::Matrix(MatrixStyle::Aligned, _, _));
    if (kind == MathInputKind::Align) != is_aligned {
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
