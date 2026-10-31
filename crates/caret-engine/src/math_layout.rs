//! 将持久化数学树投影为带稳定 provenance 的布局输入。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{
    Align, MAX_MATH_SOURCE_BYTES, MAX_MATH_TREE_PAYLOAD_BYTES, MathNode, MathNodeKind,
    MathSourceRange, MathTree,
};
use caret_math::{
    MAX_MATH_SYNTAX_NODES, MathInputKind, MathLayoutBackend, MathLayoutError, MathLayoutSnapshot,
    MathSyntaxKind, MathSyntaxNode, MathSyntaxTree, MathTextShaper,
};

/// 根据文档模型布局公式，并将节点 ID 映射到布局几何。
/// 有效源码范围会校验并传入临时语法树，但当前输出快照不包含源码范围。
pub fn layout_math_tree(
    backend: &MathLayoutBackend,
    tree: &MathTree,
    input_kind: MathInputKind,
    font_size_centipoints: u16,
    text_shaper: &dyn MathTextShaper,
) -> Result<MathLayoutSnapshot, MathLayoutError> {
    tree.validate()
        .map_err(|error| MathLayoutError::UnsupportedTree(error.to_string()))?;
    let syntax = syntax_tree_from_model(tree, input_kind)?;
    backend.layout(&syntax, font_size_centipoints, text_shaper)
}

/// 根据持久化多行对齐模型生成布局快照，并保留公式和单元格的来源 ID。
/// 上游将环境行折叠为无身份的 `EnvRow`，所以快照暂不提供行 ID 几何。
/// 文档模型只在 engine 边界解释；`caret-math` 仍仅接收后端无关的语法树。
pub fn layout_math_align(
    backend: &MathLayoutBackend,
    align: &Align,
    font_size_centipoints: u16,
    text_shaper: &dyn MathTextShaper,
) -> Result<MathLayoutSnapshot, MathLayoutError> {
    validate_align_layout_input(align)?;
    let mut rows = Vec::with_capacity(align.rows.len());
    for row in &align.rows {
        let children = row
            .cells
            .iter()
            .map(|cell| syntax_node_from_model(&cell.root, &cell.source))
            .collect::<Result<Vec<_>, _>>()?;
        rows.push(MathSyntaxNode {
            kind: MathSyntaxKind::Row,
            text: None,
            attributes: Default::default(),
            source_range: None,
            provenance: None,
            provenance_aliases: Vec::new(),
            children,
        });
    }

    let syntax = MathSyntaxTree {
        input_kind: MathInputKind::Align,
        root: MathSyntaxNode {
            kind: MathSyntaxKind::Matrix,
            text: None,
            // 转换器要求矩阵携带该字段；aligned 后端自行决定各列的左右对齐方式。
            attributes: std::collections::BTreeMap::from([
                ("style".into(), "aligned".into()),
                ("columns".into(), String::new()),
            ]),
            source_range: None,
            provenance: Some(align.id.to_string()),
            provenance_aliases: Vec::new(),
            children: rows,
        },
    };
    backend.layout(&syntax, font_size_centipoints, text_shaper)
}

fn validate_align_layout_input(align: &Align) -> Result<(), MathLayoutError> {
    if align.source.len() > MAX_MATH_SOURCE_BYTES {
        return Err(MathLayoutError::UnsupportedTree(
            "align source exceeds the configured byte limit".into(),
        ));
    }
    let mut node_count = 1usize; // 合成的 aligned Matrix 根。
    let mut payload_bytes = align.id.as_str().len();
    for row in &align.rows {
        node_count = node_count
            .checked_add(1)
            .ok_or(MathLayoutError::GeometryOverflow)?;
        if node_count > MAX_MATH_SYNTAX_NODES {
            return Err(align_node_budget_error());
        }
        if row.cells.len() > MAX_MATH_SYNTAX_NODES.saturating_sub(node_count) {
            return Err(align_node_budget_error());
        }
        for cell in &row.cells {
            if cell.source.len() > MAX_MATH_SOURCE_BYTES {
                return Err(MathLayoutError::UnsupportedTree(
                    "math source exceeds the configured byte limit".into(),
                ));
            }
            let mut pending = vec![&cell.root];
            while let Some(node) = pending.pop() {
                node_count = node_count
                    .checked_add(1)
                    .ok_or(MathLayoutError::GeometryOverflow)?;
                if node_count > MAX_MATH_SYNTAX_NODES {
                    return Err(align_node_budget_error());
                }
                payload_bytes = payload_bytes
                    .saturating_add(node.id.as_str().len())
                    .saturating_add(node.text.as_ref().map_or(0, String::len))
                    .saturating_add(node.attributes.iter().fold(0usize, |total, (key, value)| {
                        total.saturating_add(key.len()).saturating_add(value.len())
                    }));
                if payload_bytes > MAX_MATH_TREE_PAYLOAD_BYTES {
                    return Err(align_payload_budget_error());
                }
                if node.attributes.contains_key("atomClass") {
                    let remaining = MAX_MATH_SYNTAX_NODES.saturating_sub(node_count);
                    let extra_atoms = node.text.as_deref().map_or(0, |text| {
                        text.chars()
                            .take(remaining.saturating_add(2))
                            .count()
                            .saturating_sub(1)
                    });
                    if extra_atoms > remaining {
                        return Err(align_node_budget_error());
                    }
                    node_count = node_count
                        .checked_add(extra_atoms)
                        .ok_or(MathLayoutError::GeometryOverflow)?;
                }
                if node_count
                    .saturating_add(pending.len())
                    .saturating_add(node.children.len())
                    > MAX_MATH_SYNTAX_NODES
                {
                    return Err(align_node_budget_error());
                }
                pending.extend(node.children.iter());
            }
        }
    }
    if payload_bytes > MAX_MATH_TREE_PAYLOAD_BYTES {
        return Err(align_payload_budget_error());
    }
    // 先用不分配 ID 集合的布局预算预扫，避免无界模型消耗布局验证资源。
    align
        .validate()
        .map_err(|error| MathLayoutError::UnsupportedTree(error.to_string()))?;
    Ok(())
}

fn align_node_budget_error() -> MathLayoutError {
    MathLayoutError::UnsupportedTree("align exceeds the configured aggregate node budget".into())
}

fn align_payload_budget_error() -> MathLayoutError {
    MathLayoutError::UnsupportedTree("align exceeds the configured aggregate payload budget".into())
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
    let source_range = node
        .source_range
        .as_ref()
        .map(|range| checked_source_range(range, source))
        .transpose()?;
    if node.kind == MathNodeKind::Group && node.children.len() == 1 {
        let mut child = syntax_node_from_model(&node.children[0], source)?;
        child.provenance_aliases.push(node.id.to_string());
        return Ok(child);
    }
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
        // 暂存分组边界，避免单字形分组被压成行后丢失原子类别与斜体修正量。
        MathNodeKind::Group => MathSyntaxKind::Group,
        MathNodeKind::Style => {
            return Err(MathLayoutError::UnsupportedTree(format!(
                "persistent node kind {:?} has no layout mapping",
                node.kind
            )));
        }
    };
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
        provenance_aliases: Vec::new(),
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
