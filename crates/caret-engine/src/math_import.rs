//! 将 `caret-math` 语法树导入 `caret-doc` 持久化模型。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{
    Align, Equation, InlineMath, MATH_TREE_SCHEMA_VERSION, MathAlignRow, MathNode, MathNodeKind,
    MathTree, NodeId,
};
use caret_math::{
    MathInputKind, MathSyntaxConversionError, MathSyntaxKind, MathSyntaxNode, ParsedMath,
};
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

/// 从 LaTeX 导入后可插入文档的 Caret 公式节点。
#[derive(Debug, Clone, PartialEq)]
pub enum ImportedMath {
    /// 段内公式。
    Inline(InlineMath),
    /// 单行显示公式。
    Equation(Equation),
    /// 多行对齐公式。
    Align(Align),
}

/// 公式中间语法树不能无损转成当前持久化模型时的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathImportError<'source> {
    /// Parser AST 到 `caret-math` 中间树转换失败。
    Syntax {
        /// 转换原因。
        error: MathSyntaxConversionError,
        /// 失败公式的完整原始输入。
        source: &'source str,
        /// 当前 parser 能提供的最精确 UTF-8 源范围。
        source_range: std::ops::Range<usize>,
    },
    /// 当前 `Align` schema 还不能持久化环境级横线或 intertext 行。
    UnsupportedAlignRow {
        /// 触发拒绝的 Caret 语法节点类别。
        row_kind: MathSyntaxKind,
        /// 导入的完整原始公式。
        source: &'source str,
        /// 当前 parser 能提供的最精确 UTF-8 源范围。
        source_range: std::ops::Range<usize>,
    },
}

impl Display for MathImportError<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax { error, .. } => Display::fmt(error, formatter),
            Self::UnsupportedAlignRow { row_kind, .. } => {
                write!(formatter, "math.import_unsupported_align_row: {row_kind:?}")
            }
        }
    }
}

impl std::error::Error for MathImportError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Syntax { error, .. } => Some(error),
            Self::UnsupportedAlignRow { .. } => None,
        }
    }
}

/// 将已解析公式映射到 Caret 自有 schema。
///
/// `caret-math` 不依赖文档 crate；只有这一层创建稳定文档 ID。Align 的完整原始输入保存在
/// `Align.source`，但上游 parser 不提供单元格源码跨度，所以每个 cell 的 `MathTree.source`
/// 暂为空串且不写 `sourceRange`。导出和细粒度源码编辑须等待 parser adapter 提供节点跨度。
pub fn import_parsed_math<'source>(
    parsed: &ParsedMath<'source>,
) -> Result<ImportedMath, MathImportError<'source>> {
    let syntax = parsed
        .syntax_tree()
        .map_err(|error| MathImportError::Syntax {
            error,
            source: parsed.source(),
            source_range: parsed.source_range(),
        })?;
    match syntax.input_kind {
        MathInputKind::Inline => Ok(ImportedMath::Inline(InlineMath {
            id: NodeId::generate(),
            math_tree: into_math_tree(syntax.root, parsed.source()),
            extra: BTreeMap::new(),
        })),
        MathInputKind::Equation => Ok(ImportedMath::Equation(Equation {
            id: NodeId::generate(),
            math_tree: into_math_tree(syntax.root, parsed.source()),
            extra: BTreeMap::new(),
        })),
        MathInputKind::Align => import_align(syntax.root, parsed.source(), parsed.source_range()),
    }
}

fn import_align<'source>(
    root: MathSyntaxNode,
    source: &'source str,
    source_range: std::ops::Range<usize>,
) -> Result<ImportedMath, MathImportError<'source>> {
    if root.kind != MathSyntaxKind::Matrix
        || root.attributes.get("style").map(String::as_str) != Some("aligned")
    {
        return Err(MathImportError::UnsupportedAlignRow {
            row_kind: root.kind,
            source,
            source_range,
        });
    }
    let mut rows = Vec::with_capacity(root.children.len());
    for row in root.children {
        if row.kind != MathSyntaxKind::Row {
            return Err(MathImportError::UnsupportedAlignRow {
                row_kind: row.kind,
                source,
                source_range,
            });
        }
        let cells = row
            .children
            .into_iter()
            .map(|cell| into_math_tree(cell, ""))
            .collect();
        rows.push(MathAlignRow {
            id: NodeId::generate(),
            cells,
            extra: BTreeMap::new(),
        });
    }
    Ok(ImportedMath::Align(Align {
        id: NodeId::generate(),
        source: source.to_owned(),
        rows,
        extra: BTreeMap::new(),
    }))
}

fn into_math_tree(root: MathSyntaxNode, source: &str) -> MathTree {
    MathTree {
        schema_version: MATH_TREE_SCHEMA_VERSION,
        source: source.to_owned(),
        root: into_math_node(root, true),
        extra: BTreeMap::new(),
    }
}

fn into_math_node(node: MathSyntaxNode, coalesce_row: bool) -> MathNode {
    let kind = document_kind(node.kind);
    let children = if node.kind == MathSyntaxKind::Row && coalesce_row {
        coalesce_atom_runs(node.children)
    } else {
        node.children
    };
    let matrix_rows = node.kind == MathSyntaxKind::Matrix;
    MathNode {
        id: NodeId::generate(),
        kind,
        text: node.text,
        attributes: node.attributes,
        source_range: None,
        children: children
            .into_iter()
            .map(|child| {
                let child_is_matrix_row = matrix_rows && child.kind == MathSyntaxKind::Row;
                into_math_node(child, !child_is_matrix_row)
            })
            .collect(),
        extra: BTreeMap::new(),
    }
}

fn coalesce_atom_runs(children: Vec<MathSyntaxNode>) -> Vec<MathSyntaxNode> {
    let mut output: Vec<MathSyntaxNode> = Vec::with_capacity(children.len());
    for child in children {
        let Some(atom_class @ "ord") = child.attributes.get("atomClass").map(String::as_str) else {
            output.push(child);
            continue;
        };
        if child.kind != MathSyntaxKind::Symbol {
            output.push(child);
            continue;
        }
        let Some(previous) = output.last_mut().filter(|previous| {
            previous.kind == MathSyntaxKind::Symbol
                && previous.attributes.get("atomClass").map(String::as_str) == Some(atom_class)
        }) else {
            output.push(child);
            continue;
        };
        if let Some(text) = child.text {
            previous
                .text
                .get_or_insert_with(String::new)
                .push_str(&text);
        }
    }
    output
}

fn document_kind(kind: MathSyntaxKind) -> MathNodeKind {
    match kind {
        MathSyntaxKind::Row => MathNodeKind::Row,
        MathSyntaxKind::Symbol => MathNodeKind::Symbol,
        MathSyntaxKind::Operator => MathNodeKind::Operator,
        MathSyntaxKind::Fraction => MathNodeKind::Fraction,
        MathSyntaxKind::Radical => MathNodeKind::Radical,
        MathSyntaxKind::Superscript => MathNodeKind::Superscript,
        MathSyntaxKind::Subscript => MathNodeKind::Subscript,
        MathSyntaxKind::SubSup => MathNodeKind::SubSup,
        MathSyntaxKind::Delimited => MathNodeKind::Delimited,
        MathSyntaxKind::SizedDelimiter => MathNodeKind::SizedDelimiter,
        MathSyntaxKind::Matrix => MathNodeKind::Matrix,
        MathSyntaxKind::Substack => MathNodeKind::Substack,
        MathSyntaxKind::Intertext => MathNodeKind::Intertext,
        MathSyntaxKind::Hline => MathNodeKind::Hline,
        MathSyntaxKind::Sum => MathNodeKind::Sum,
        MathSyntaxKind::Integral => MathNodeKind::Integral,
        MathSyntaxKind::Product => MathNodeKind::Product,
        MathSyntaxKind::Limit => MathNodeKind::Limit,
        MathSyntaxKind::OverUnder => MathNodeKind::OverUnder,
        MathSyntaxKind::Accent => MathNodeKind::Accent,
        MathSyntaxKind::CancelTo => MathNodeKind::CancelTo,
        MathSyntaxKind::Text => MathNodeKind::Text,
        MathSyntaxKind::TextContent => MathNodeKind::TextContent,
        MathSyntaxKind::TextSpace => MathNodeKind::TextSpace,
        MathSyntaxKind::TextSymbol => MathNodeKind::TextSymbol,
        MathSyntaxKind::TextStyle => MathNodeKind::TextStyle,
        MathSyntaxKind::Space => MathNodeKind::Space,
        MathSyntaxKind::Color => MathNodeKind::Color,
        MathSyntaxKind::TextColor => MathNodeKind::TextColor,
        MathSyntaxKind::ColorBox => MathNodeKind::ColorBox,
        MathSyntaxKind::FColorBox => MathNodeKind::FColorBox,
        MathSyntaxKind::Strut => MathNodeKind::Strut,
        MathSyntaxKind::Phantom => MathNodeKind::Phantom,
        MathSyntaxKind::Placeholder => MathNodeKind::Placeholder,
    }
}

#[cfg(test)]
mod tests;
