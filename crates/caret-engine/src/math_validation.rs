//! 数学源码与持久化树的一致性校验，放在依赖解析后端的 editor/service 边界。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{Align, Block, Document, InlineNode, MathNode, MathTree};
use caret_math::MathInputKind;

/// 验证文档中每棵数学树都与其 LaTeX 源码表达相同语义。
/// `caret-doc` 只校验稳定 schema；这里依赖解析器，因此不反向依赖 `caret-math`。
pub fn validate_document_math_sources(document: &Document) -> Result<(), String> {
    fn validate_inlines(nodes: &[InlineNode]) -> Result<(), String> {
        for node in nodes {
            match node {
                InlineNode::Math(math) => {
                    validate_tree(&math.math_tree, MathInputKind::Inline, &math.id.to_string())?;
                }
                InlineNode::Environment(environment) => validate_inlines(&environment.children)?,
                InlineNode::Run(_) | InlineNode::SoftBreak(_) | InlineNode::Opaque(_) => {}
            }
        }
        Ok(())
    }

    fn validate_blocks(blocks: &[Block]) -> Result<(), String> {
        for block in blocks {
            match block {
                Block::Paragraph(paragraph) => validate_inlines(&paragraph.inlines)?,
                Block::Equation(equation) => validate_tree(
                    &equation.math_tree,
                    MathInputKind::Equation,
                    &equation.id.to_string(),
                )?,
                Block::Align(align) => validate_align(align)?,
                Block::Environment(environment) => {
                    if let Some(title) = &environment.title {
                        validate_inlines(&title.inlines)?;
                    }
                    validate_blocks(&environment.children)?;
                }
                Block::ExplicitBreak(_) | Block::Opaque(_) => {}
            }
        }
        Ok(())
    }

    validate_blocks(&document.body)
}

fn validate_tree(tree: &MathTree, kind: MathInputKind, id: &str) -> Result<(), String> {
    let parsed = caret_math::parse(&tree.source, kind)
        .map_err(|error| format!("math {id} source does not parse: {error}"))?;
    let caret_math::ParsedMath::Tree(parsed) = parsed else {
        return Err(format!(
            "math {id} source parsed as a different formula kind"
        ));
    };
    if same_node_semantics(&tree.root, &parsed.root) {
        Ok(())
    } else {
        Err(format!("math {id} source and tree disagree"))
    }
}

fn validate_align(align: &Align) -> Result<(), String> {
    let parsed = caret_math::parse(&align.source, MathInputKind::Align)
        .map_err(|error| format!("align {} source does not parse: {error}", align.id))?;
    let caret_math::ParsedMath::Align(parsed) = parsed else {
        return Err(format!(
            "align {} source parsed as a different formula kind",
            align.id
        ));
    };
    if parsed.rows.len() != align.rows.len()
        || parsed.rows.iter().zip(&align.rows).any(|(parsed, stored)| {
            parsed.cells.len() != stored.cells.len()
                || parsed
                    .cells
                    .iter()
                    .zip(&stored.cells)
                    .any(|(parsed, stored)| !same_node_semantics(&parsed.root, &stored.root))
        })
    {
        return Err(format!("align {} source and cells disagree", align.id));
    }
    for row in &align.rows {
        for cell in &row.cells {
            if is_empty_align_placeholder(cell) {
                continue;
            }
            validate_tree(cell, MathInputKind::Inline, &row.id.to_string())?;
        }
    }
    Ok(())
}

fn is_empty_align_placeholder(tree: &MathTree) -> bool {
    tree.source.is_empty()
        && tree.root.kind == caret_doc::MathNodeKind::Placeholder
        && tree.root.text.is_none()
        && tree.root.attributes.is_empty()
        && tree.root.children.is_empty()
}

fn same_node_semantics(left: &MathNode, right: &MathNode) -> bool {
    left.kind == right.kind
        && left.text == right.text
        && left.attributes == right.attributes
        && source_range_span(left) == source_range_span(right)
        && left.children.len() == right.children.len()
        && left
            .children
            .iter()
            .zip(&right.children)
            .all(|(left, right)| same_node_semantics(left, right))
}

fn source_range_span(node: &MathNode) -> Option<(u32, u32)> {
    node.source_range
        .as_ref()
        .map(|range| (range.start, range.end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use caret_doc::{InlineMath, NodeId};

    fn parse_inline(source: &str) -> MathTree {
        match caret_math::parse(source, MathInputKind::Inline) {
            Ok(caret_math::ParsedMath::Tree(tree)) => tree,
            Ok(caret_math::ParsedMath::Align(_)) => panic!("expected an inline tree"),
            Err(error) => panic!("test formula should parse: {error}"),
        }
    }

    #[test]
    fn mismatched_math_source_and_tree_cannot_enter_an_edit_session() {
        let mut tree = parse_inline("x");
        tree.source = "y".to_owned();
        let paragraph = match Document::blank().body.into_iter().next() {
            Some(Block::Paragraph(mut paragraph)) => {
                paragraph.inlines = vec![InlineNode::Math(InlineMath {
                    id: NodeId::generate(),
                    math_tree: tree,
                    extra: Default::default(),
                })];
                paragraph
            }
            _ => panic!("blank document should contain a paragraph"),
        };
        let mut document = Document::blank();
        document.body = vec![Block::Paragraph(paragraph)];

        let Err(error) = validate_document_math_sources(&document) else {
            panic!("mismatched source and tree must be rejected");
        };
        assert!(error.contains("source and tree disagree"));
        assert!(crate::Session::new(document).is_err());
    }

    #[test]
    fn parsed_formula_source_and_tree_are_accepted() {
        let tree = parse_inline("\\frac{a}{b}");
        let mut document = Document::blank();
        if let Some(Block::Paragraph(paragraph)) = document.body.first_mut() {
            paragraph.inlines = vec![InlineNode::Math(InlineMath {
                id: NodeId::generate(),
                math_tree: tree,
                extra: Default::default(),
            })];
        }

        assert_eq!(validate_document_math_sources(&document), Ok(()));
        assert!(crate::Session::new(document).is_ok());
    }

    #[test]
    fn align_cell_source_and_node_ranges_must_match_the_parsed_tree() {
        let mut align =
            match caret_math::parse(r"\begin{aligned}xy\end{aligned}", MathInputKind::Align) {
                Ok(caret_math::ParsedMath::Align(align)) => align,
                Ok(caret_math::ParsedMath::Tree(_)) => panic!("expected aligned formula"),
                Err(error) => panic!("test align formula should parse: {error}"),
            };
        align.rows[0].cells[0].source = "y".to_owned();
        let Err(error) = validate_align(&align) else {
            panic!("mismatched align source and cells must be rejected");
        };
        assert!(error.contains("source and tree disagree"));

        let mut tree = parse_inline("xy");
        let Some(range) = tree.root.source_range.as_mut() else {
            panic!("parsed row should retain its source range");
        };
        range.start = 1;
        let Err(error) = validate_tree(&tree, MathInputKind::Inline, "test") else {
            panic!("incorrect source range must be rejected");
        };
        assert!(error.contains("source and tree disagree"));
    }
}
