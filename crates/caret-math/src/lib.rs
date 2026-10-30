//! 为 Caret 隔离 LaTeX 数学解析器版本、输入预算与导入诊断。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use latex_rust::{
    EnvRow, EqNumber, MathNode, MatrixStyle, ParseError, ParseOptions, TextItem, parse_with_options,
};
use std::fmt::{Display, Formatter};
use std::ops::Range;

/// 单个公式允许的最大 UTF-8 源码长度。
pub const MAX_MATH_SOURCE_BYTES: usize = 1024 * 1024;
/// Caret 文档模型允许的最大数学树嵌套预算。
pub const MAX_MATH_NESTING_DEPTH: usize = 32;

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
/// 上游 AST 在 `caret-math` 内完成校验后销毁，不通过公开 API 暴露或写入 `.cdoc`。
#[derive(Debug)]
pub struct ParsedMath<'source> {
    source: &'source str,
    source_range: Range<usize>,
    normalized_range: Range<usize>,
}

impl ParsedMath<'_> {
    /// 返回导入时的原始公式源码。
    pub fn source(&self) -> &str {
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
                return Some(format!("begin{{{}}}", matrix_style_name(*style)));
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

fn matrix_style_name(style: MatrixStyle) -> &'static str {
    match style {
        MatrixStyle::Align => "align",
        MatrixStyle::Equation => "equation",
        MatrixStyle::Gather => "gather",
        MatrixStyle::Multline => "multline",
        MatrixStyle::Matrix => "matrix",
        MatrixStyle::Pmatrix => "pmatrix",
        MatrixStyle::Bmatrix => "bmatrix",
        MatrixStyle::Vmatrix => "vmatrix",
        MatrixStyle::VVmatrix => "Vmatrix",
        MatrixStyle::BBmatrix => "Bmatrix",
        MatrixStyle::Cases => "cases",
        MatrixStyle::Array => "array",
        MatrixStyle::Aligned => "aligned",
        MatrixStyle::Split => "split",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_MATH_NESTING_DEPTH, MAX_MATH_SOURCE_BYTES, MathInputKind, MathParseError, parse,
    };

    #[test]
    fn parses_inline_and_display_math_with_the_locked_depth_budget() {
        let inline = parse(r"$x^2 + 1$", MathInputKind::Inline);
        assert!(inline.is_ok());

        let aligned = parse(
            r"\[\begin{aligned}a &= b \\ c &= d\end{aligned}\]",
            MathInputKind::Align,
        );
        assert!(aligned.is_ok());

        let too_deep = format!(
            "{}x{}",
            r"\frac{1}{".repeat(MAX_MATH_NESTING_DEPTH),
            "}".repeat(MAX_MATH_NESTING_DEPTH)
        );
        assert!(matches!(
            parse(&too_deep, MathInputKind::Equation),
            Err(MathParseError::Parse { source, .. }) if source == too_deep
        ));
    }

    #[test]
    fn parser_errors_keep_source_and_exact_whole_formula_byte_range() {
        let source = r"x + \unknown";
        let Err(MathParseError::Parse {
            source: preserved,
            source_range,
            ..
        }) = parse(source, MathInputKind::Inline)
        else {
            panic!("unknown command must fail parsing");
        };
        assert_eq!(preserved, source);
        assert_eq!(source_range, 0..source.len());
    }

    #[test]
    fn rejects_numbering_semantics_and_overlong_source_without_loss() {
        let source = r"x \label{eq:x}";
        assert!(matches!(
            parse(source, MathInputKind::Equation),
            Err(MathParseError::UnsupportedNumbering {
                source: preserved,
                command,
                ..
            }) if preserved == source && command == "label"
        ));

        let source = "x".repeat(MAX_MATH_SOURCE_BYTES + 1);
        assert!(matches!(
            parse(&source, MathInputKind::Inline),
            Err(MathParseError::InputTooLong { source: preserved, .. }) if preserved == source
        ));
    }

    #[test]
    fn rejects_automatically_numbered_environments_and_nested_references() {
        for source in [
            r"\begin{equation}x\end{equation}",
            r"\begin{align}x &= y\end{align}",
            r"\text{$\ref{eq:x}$}",
        ] {
            assert!(matches!(
                parse(source, MathInputKind::Equation),
                Err(MathParseError::UnsupportedNumbering { source: preserved, .. })
                    if preserved == source
            ));
        }

        assert!(
            parse(
                r"\[\begin{aligned}x &= y\end{aligned}\]",
                MathInputKind::Align
            )
            .is_ok()
        );
        assert!(matches!(
            parse("x", MathInputKind::Align),
            Err(MathParseError::InputKindMismatch { .. })
        ));
        let aligned = r"\begin{aligned}a &= b \\ c &= d\end{aligned}";
        assert!(matches!(
            parse(aligned, MathInputKind::Inline),
            Err(MathParseError::InputKindMismatch { .. })
        ));
        assert!(matches!(
            parse(aligned, MathInputKind::Equation),
            Err(MathParseError::InputKindMismatch { .. })
        ));
    }

    #[test]
    fn normalized_range_maps_trimmed_unicode_input_to_utf8_bytes() {
        let source = "  α + x  ";
        let Ok(parsed) = parse(source, MathInputKind::Inline) else {
            panic!("input should parse");
        };
        assert_eq!(&source[parsed.normalized_range()], "α + x");
        assert_eq!(parsed.source_range(), 0..source.len());
    }
}
