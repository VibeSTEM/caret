//! Caret 的数学公式支持：LaTeX 子集解析、文档数学树序列化，以及基于 OpenType
//! MATH 表的公式布局。
//!
//! 解析器直接产出 `caret-doc` 的 [`MathTree`] / [`Align`]，不经过中间语法树；
//! 序列化器把文档树写回 LaTeX，使树被编辑后 `source` 仍描述当前内容。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod backend;
pub mod layout;
mod parser;
mod serialize;
pub mod symbols;

use std::{fmt, ops::Range};

use caret_doc::{Align, MathTree};

pub use parser::{classify_input, is_incomplete_input_prefix, parse};
pub use serialize::{serialize_align, serialize_node, serialize_tree};

/// 公式输入所属的文档对象。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MathInputKind {
    /// 段落内的行内公式，可带 `$…$` 或 `\(…\)`。
    Inline,
    /// 单行显示公式，可带 `$$…$$` 或 `\[…\]`。
    Equation,
    /// 多行对齐公式，内容必须是一个 `aligned` 环境，可带显示公式定界符。
    Align,
}

/// 返回解析目标剥除可选外层公式定界符后的源码范围。
/// 编辑器用此范围设置初始插入点，避免在 `$…$` / `\(…\)` 等定界符外输入。
pub fn input_body_range(source: &str, kind: MathInputKind) -> Result<Range<usize>, MathParseError> {
    parser::input_body_range(source, kind)
}

/// 解析结果。行内与单行公式共用 [`MathTree`]，对齐公式按单元格拆分。
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedMath {
    Tree(MathTree),
    Align(Align),
}

/// 输入框实时分类：完整、可继续输入的前缀、或无法补救的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathInputStatus {
    Complete,
    Incomplete,
    Invalid,
}

/// 解析失败原因。`message` 是稳定的英文诊断短语，界面层负责本地化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathParseError {
    InputTooLong {
        len: usize,
        max: usize,
    },
    InputTooDeep {
        max: usize,
    },
    SyntaxTreeTooLarge {
        max: usize,
    },
    /// 编号相关命令（`\label`、`\tag`、`align` 等）由文档编号系统负责，公式内不接受。
    UnsupportedNumbering {
        command: String,
    },
    /// 定界符或环境与目标对象类型不符，例如在行内公式里写 `\[…\]`。
    InputKindMismatch {
        expected: MathInputKind,
    },
    Parse {
        message: String,
        /// 出错位置相对输入的 UTF-8 字节偏移。
        offset: usize,
        /// 输入在当前位置提前结束；继续输入可能使其合法。
        incomplete: bool,
    },
}

impl fmt::Display for MathParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLong { len, max } => {
                write!(f, "math input is {len} bytes, limit is {max}")
            }
            Self::InputTooDeep { max } => write!(f, "math input nests deeper than {max}"),
            Self::SyntaxTreeTooLarge { max } => write!(f, "math input exceeds {max} nodes"),
            Self::UnsupportedNumbering { command } => {
                write!(
                    f,
                    "numbering command `{command}` is not supported in formulas"
                )
            }
            Self::InputKindMismatch { expected } => {
                write!(f, "math input does not match {expected:?}")
            }
            Self::Parse {
                message, offset, ..
            } => write!(f, "{message} at byte {offset}"),
        }
    }
}

impl std::error::Error for MathParseError {}

impl MathParseError {
    /// 继续输入是否可能让当前文本变得合法。
    pub fn is_incomplete(&self) -> bool {
        matches!(
            self,
            Self::Parse {
                incomplete: true,
                ..
            }
        )
    }
}

#[cfg(test)]
mod tests;
