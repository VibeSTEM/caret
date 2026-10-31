//! 将持久化数学树投影为带稳定 provenance 的布局输入。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{MathNode, MathNodeKind, MathSourceRange, MathTree};
use caret_math::{
    MathInputKind, MathLayoutBackend, MathLayoutError, MathLayoutSnapshot, MathSyntaxKind,
    MathSyntaxNode, MathSyntaxTree, MathTextShaper,
};

/// 根据文档模型布局公式；文档节点 ID 和源码范围映射进同一输出快照。
pub fn layout_math_tree(
    backend: &MathLayoutBackend,
    tree: &MathTree,
    input_kind: MathInputKind,
    font_size_centipoints: u16,
    text_shaper: &dyn MathTextShaper,
) -> Result<MathLayoutSnapshot, MathLayoutError> {
    let syntax = syntax_tree_from_model(tree, input_kind)?;
    backend.layout(&syntax, font_size_centipoints, text_shaper)
}

pub(crate) fn syntax_tree_from_model(
    tree: &MathTree,
    input_kind: MathInputKind,
) -> Result<MathSyntaxTree, MathLayoutError> {
    Ok(MathSyntaxTree {
        input_kind,
        root: syntax_node_from_model(&tree.root, &tree.source)?,
    })
}

fn syntax_node_from_model(
    node: &MathNode,
    source: &str,
) -> Result<MathSyntaxNode, MathLayoutError> {
    let kind = match node.kind {
        MathNodeKind::Row => MathSyntaxKind::Row,
        MathNodeKind::Symbol => MathSyntaxKind::Symbol,
        MathNodeKind::Operator => MathSyntaxKind::Operator,
        MathNodeKind::Fraction => MathSyntaxKind::Fraction,
        MathNodeKind::Radical => MathSyntaxKind::Radical,
        MathNodeKind::Superscript => MathSyntaxKind::Superscript,
        MathNodeKind::Subscript => MathSyntaxKind::Subscript,
        MathNodeKind::SubSup => MathSyntaxKind::SubSup,
        MathNodeKind::Delimited => MathSyntaxKind::Delimited,
        MathNodeKind::SizedDelimiter => MathSyntaxKind::SizedDelimiter,
        MathNodeKind::Matrix => MathSyntaxKind::Matrix,
        MathNodeKind::Substack => MathSyntaxKind::Substack,
        MathNodeKind::Intertext => MathSyntaxKind::Intertext,
        MathNodeKind::Hline => MathSyntaxKind::Hline,
        MathNodeKind::Sum => MathSyntaxKind::Sum,
        MathNodeKind::Integral => MathSyntaxKind::Integral,
        MathNodeKind::Product => MathSyntaxKind::Product,
        MathNodeKind::Limit => MathSyntaxKind::Limit,
        MathNodeKind::OverUnder => MathSyntaxKind::OverUnder,
        MathNodeKind::CancelTo => MathSyntaxKind::CancelTo,
        MathNodeKind::Accent => MathSyntaxKind::Accent,
        MathNodeKind::Space => MathSyntaxKind::Space,
        MathNodeKind::Text => MathSyntaxKind::Text,
        MathNodeKind::TextContent => MathSyntaxKind::TextContent,
        MathNodeKind::TextSpace => MathSyntaxKind::TextSpace,
        MathNodeKind::TextSymbol => MathSyntaxKind::TextSymbol,
        MathNodeKind::TextStyle => MathSyntaxKind::TextStyle,
        MathNodeKind::Color => MathSyntaxKind::Color,
        MathNodeKind::TextColor => MathSyntaxKind::TextColor,
        MathNodeKind::ColorBox => MathSyntaxKind::ColorBox,
        MathNodeKind::FColorBox => MathSyntaxKind::FColorBox,
        MathNodeKind::Strut => MathSyntaxKind::Strut,
        MathNodeKind::Phantom => MathSyntaxKind::Phantom,
        MathNodeKind::Placeholder => MathSyntaxKind::Placeholder,
        MathNodeKind::Style | MathNodeKind::Group => {
            return Err(MathLayoutError::UnsupportedTree(format!(
                "persistent node kind {:?} has no layout mapping",
                node.kind
            )));
        }
    };
    let source_range = node
        .source_range
        .as_ref()
        .map(|range| checked_source_range(range, source))
        .transpose()?;
    let children = node
        .children
        .iter()
        .map(|child| syntax_node_from_model(child, source))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(MathSyntaxNode {
        kind,
        text: node.text.clone(),
        attributes: node.attributes.clone(),
        source_range,
        provenance: Some(node.id.to_string()),
        children,
    })
}

fn checked_source_range(
    range: &MathSourceRange,
    source: &str,
) -> Result<std::ops::Range<usize>, MathLayoutError> {
    let start = usize::try_from(range.start)
        .map_err(|_| MathLayoutError::UnsupportedTree("source range overflow".into()))?;
    let end = usize::try_from(range.end)
        .map_err(|_| MathLayoutError::UnsupportedTree("source range overflow".into()))?;
    if start > end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return Err(MathLayoutError::UnsupportedTree(
            "source range is outside the math source".into(),
        ));
    }
    Ok(start..end)
}

#[cfg(test)]
mod tests;
