//! 数学源码与持久化树的一致性校验，放在依赖解析后端的 editor/service 边界。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{
    Align, Block, Document, FlowContent, MathContent, MathFlow, MathNode, MathTree,
    ParagraphContent,
};
use caret_math::MathInputKind;

/// 验证文档中每棵数学树都与其 LaTeX 源码表达相同语义。
/// `caret-doc` 只校验稳定 schema；这里依赖解析器，因此不反向依赖 `caret-math`。
pub fn validate_document_math_sources(document: &Document) -> Result<(), String> {
    fn validate_inlines(nodes: &[ParagraphContent]) -> Result<(), String> {
        for node in nodes {
            match node {
                FlowContent::Math(math) => {
                    validate_tree(&math.math_tree, MathInputKind::Inline, &math.id.to_string())?;
                }
                FlowContent::Environment(environment) => validate_inlines(&environment.children)?,
                FlowContent::Text(_) | FlowContent::SoftBreak(_) | FlowContent::Opaque(_) => {}
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
    if same_flow_semantics(&tree.root, &parsed.root) {
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
                    .any(|(parsed, stored)| !same_flow_semantics(&parsed.root, &stored.root))
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
    tree.source.is_empty() && tree.root.content.is_empty()
}

fn same_flow_semantics(left: &MathFlow, right: &MathFlow) -> bool {
    left.source_range.as_ref().map(|r| (r.start, r.end))
        == right.source_range.as_ref().map(|r| (r.start, r.end))
        && left.content.len() == right.content.len()
        && left
            .content
            .iter()
            .zip(&right.content)
            .all(|(left, right)| same_content_semantics(left, right))
}

fn same_content_semantics(left: &MathContent, right: &MathContent) -> bool {
    match (left, right) {
        (MathContent::Text(left), MathContent::Text(right)) => {
            left.source_range
                .as_ref()
                .map(|range| (range.start, range.end))
                == right
                    .source_range
                    .as_ref()
                    .map(|range| (range.start, range.end))
                && left
                    .runs
                    .iter()
                    .flat_map(|run| run.text.chars())
                    .eq(right.runs.iter().flat_map(|run| run.text.chars()))
        }
        (MathContent::Math(left), MathContent::Math(right)) => same_node_semantics(left, right),
        (MathContent::Environment(left), MathContent::Environment(right)) => {
            left.kind == right.kind
                && left.props == right.props
                && left
                    .source_range
                    .as_ref()
                    .map(|range| (range.start, range.end))
                    == right
                        .source_range
                        .as_ref()
                        .map(|range| (range.start, range.end))
                && left.children.len() == right.children.len()
                && left
                    .children
                    .iter()
                    .zip(&right.children)
                    .all(|(l, r)| same_content_semantics(l, r))
        }
        (MathContent::SoftBreak(_), MathContent::SoftBreak(_)) => true,
        (MathContent::Opaque(left), MathContent::Opaque(right)) => left.raw == right.raw,
        _ => false,
    }
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
            .all(|(left, right)| same_flow_semantics(left, right))
}

fn source_range_span(node: &MathNode) -> Option<(u32, u32)> {
    node.source_range
        .as_ref()
        .map(|range| (range.start, range.end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use caret_doc::{InlineMath, NodeId, OpenedDocument};
    use std::collections::BTreeMap;

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
                paragraph.inlines = vec![ParagraphContent::Math(InlineMath {
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
            paragraph.inlines = vec![ParagraphContent::Math(InlineMath {
                id: NodeId::generate(),
                math_tree: tree,
                extra: Default::default(),
            })];
        }

        assert_eq!(validate_document_math_sources(&document), Ok(()));
        assert!(crate::Session::new(document).is_ok());
    }

    #[test]
    fn formula_text_metadata_and_run_splits_do_not_change_source_semantics() {
        let mut tree = parse_inline(r"\text{ab}");
        let Some(MathContent::Text(text)) = tree.root.content.first_mut() else {
            panic!("text formula should start with a text flow");
        };
        text.runs = vec![
            caret_doc::Run {
                text: "a".to_owned(),
                language: Some("en".to_owned()),
                props: BTreeMap::from([("underline".to_owned(), serde_json::json!(true))]),
                extra: BTreeMap::from([("vendorRun".to_owned(), serde_json::json!("kept"))]),
            },
            caret_doc::Run {
                text: "b".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
        ];
        if let Some(range) = &mut text.source_range {
            range
                .extra
                .insert("vendorRange".to_owned(), serde_json::json!(7));
        }
        assert_eq!(validate_tree(&tree, MathInputKind::Inline, "text"), Ok(()));
    }

    #[test]
    fn matrix_flows_with_rows_and_markers_round_trip_through_cdoc() {
        let source = r"\begin{matrix}a \\ \hline \intertext{middle} b\end{matrix}";
        let tree = parse_inline(source);
        let mut document = Document::blank();
        let Some(Block::Paragraph(paragraph)) = document.body.first_mut() else {
            panic!("blank document should have a paragraph");
        };
        paragraph.inlines = vec![ParagraphContent::Math(InlineMath {
            id: NodeId::generate(),
            math_tree: tree,
            extra: Default::default(),
        })];
        let features = document.required_features();
        let envelope = serde_json::json!({
            "format": "caret",
            "version": caret_doc::CURRENT_FORMAT_VERSION,
            "requiredFeatures": features,
            "document": document,
        });
        let opened = match caret_doc::open(envelope.to_string().as_bytes()) {
            Ok(opened) => opened,
            Err(error) => panic!("matrix document should reopen: {error}"),
        };
        let OpenedDocument::SafePreview(preview) = opened else {
            panic!("math remains a safe preview until 044 is integrated");
        };
        assert!(
            !preview
                .reasons
                .contains(&caret_doc::PreviewReason::UnsupportedNodeKind)
        );
        assert!(matches!(preview.document().body[0], Block::Paragraph(_)));
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

        let mut tree = parse_inline("x+y");
        let Some(MathContent::Math(node)) = tree.root.content.first_mut() else {
            panic!("parsed row should retain its first semantic node");
        };
        let Some(range) = node.source_range.as_mut() else {
            panic!("parsed math node should retain its source range");
        };
        range.start = 1;
        let Err(error) = validate_tree(&tree, MathInputKind::Inline, "test") else {
            panic!("incorrect source range must be rejected");
        };
        assert!(error.contains("source and tree disagree"));
    }
}
