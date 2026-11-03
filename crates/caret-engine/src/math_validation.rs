//! 数学源码与持久化树的一致性校验，放在依赖解析后端的 editor/service 边界。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{Align, InlineEnvironment, MathContent, MathFlow, MathNode, MathTree, TextFlow};
#[cfg(test)]
use caret_doc::{Block, Document, FlowContent, ParagraphContent};
use caret_math::MathInputKind;

pub(crate) fn project_tree_for_edit(
    tree: &MathTree,
    kind: MathInputKind,
) -> Result<(String, MathTree), String> {
    let source = caret_math::serialize_tree(tree);
    let caret_math::ParsedMath::Tree(mut projected) = caret_math::parse(&source, kind)
        .map_err(|error| format!("could not project formula tree to TeX: {error}"))?
    else {
        return Err("formula tree projected to a different formula kind".to_owned());
    };
    if !transfer_tree_metadata(tree, &mut projected) {
        return Err("formula tree cannot be projected without losing structure".to_owned());
    }
    projected.validate().map_err(|error| error.to_string())?;
    Ok((source, projected))
}

pub(crate) fn project_align_for_edit(align: &Align) -> Result<(String, Align), String> {
    let source = caret_math::serialize_align(align);
    let caret_math::ParsedMath::Align(mut projected) =
        caret_math::parse(&source, MathInputKind::Align)
            .map_err(|error| format!("could not project align tree to TeX: {error}"))?
    else {
        return Err("align tree projected to a different formula kind".to_owned());
    };
    if align.rows.len() != projected.rows.len() {
        return Err("align projection changed its row structure".to_owned());
    }
    projected.id.clone_from(&align.id);
    projected.extra.clone_from(&align.extra);
    for (previous_row, projected_row) in align.rows.iter().zip(&mut projected.rows) {
        if previous_row.cells.len() != projected_row.cells.len() {
            return Err("align projection changed its cell structure".to_owned());
        }
        projected_row.id.clone_from(&previous_row.id);
        projected_row.extra.clone_from(&previous_row.extra);
        for (previous_cell, projected_cell) in
            previous_row.cells.iter().zip(&mut projected_row.cells)
        {
            if !transfer_tree_metadata(previous_cell, projected_cell) {
                return Err("align cell cannot be projected without losing structure".to_owned());
            }
        }
    }
    projected.validate().map_err(|error| error.to_string())?;
    Ok((source, projected))
}

fn transfer_tree_metadata(previous: &MathTree, projected: &mut MathTree) -> bool {
    if previous.schema_version != projected.schema_version {
        return false;
    }
    projected.extra.clone_from(&previous.extra);
    transfer_flow_metadata(&previous.root, &mut projected.root)
}

fn transfer_flow_metadata(previous: &MathFlow, projected: &mut MathFlow) -> bool {
    if previous.content.len() != projected.content.len() {
        return false;
    }
    projected.id.clone_from(&previous.id);
    projected.extra.clone_from(&previous.extra);
    for (previous, projected) in previous.content.iter().zip(&mut projected.content) {
        if !transfer_content_metadata(previous, projected) {
            return false;
        }
    }
    true
}

fn transfer_content_metadata(previous: &MathContent, projected: &mut MathContent) -> bool {
    match (previous, projected) {
        (MathContent::Text(previous), MathContent::Text(projected)) => {
            if !same_display_text(previous, projected) {
                return false;
            }
            if let (Some(previous), Some(projected)) =
                (&previous.source_range, &mut projected.source_range)
            {
                projected.extra.clone_from(&previous.extra);
            }
            if previous.source_map.len() != projected.source_map.len()
                && previous
                    .source_map
                    .iter()
                    .any(|entry| !entry.extra.is_empty() || !entry.source_range.extra.is_empty())
            {
                return false;
            }
            for (previous, projected) in previous.source_map.iter().zip(&mut projected.source_map) {
                if (previous.text_start, previous.text_end)
                    != (projected.text_start, projected.text_end)
                {
                    if !previous.extra.is_empty() || !previous.source_range.extra.is_empty() {
                        return false;
                    }
                    continue;
                }
                projected.extra.clone_from(&previous.extra);
                projected
                    .source_range
                    .extra
                    .clone_from(&previous.source_range.extra);
            }
            projected.runs.clone_from(&previous.runs);
            projected.extra.clone_from(&previous.extra);
            true
        }
        (MathContent::Math(previous), MathContent::Math(projected)) => {
            transfer_node_metadata(previous, projected)
        }
        (MathContent::Environment(previous), MathContent::Environment(projected)) => {
            transfer_environment_metadata(previous, projected)
        }
        (MathContent::SoftBreak(previous), MathContent::SoftBreak(projected)) => {
            projected.extra.clone_from(&previous.extra);
            true
        }
        (MathContent::Opaque(previous), MathContent::Opaque(projected)) => {
            previous.raw == projected.raw
        }
        _ => false,
    }
}

fn transfer_node_metadata(previous: &MathNode, projected: &mut MathNode) -> bool {
    if previous.kind != projected.kind
        || previous.text != projected.text
        || previous.attributes != projected.attributes
        || previous.children.len() != projected.children.len()
    {
        return false;
    }
    projected.id.clone_from(&previous.id);
    projected.extra.clone_from(&previous.extra);
    for (previous, projected) in previous.children.iter().zip(&mut projected.children) {
        if !transfer_flow_metadata(previous, projected) {
            return false;
        }
    }
    true
}

fn transfer_environment_metadata(
    previous: &InlineEnvironment<MathContent>,
    projected: &mut InlineEnvironment<MathContent>,
) -> bool {
    if previous.kind != projected.kind || previous.children.len() != projected.children.len() {
        return false;
    }
    projected.id.clone_from(&previous.id);
    projected.definition_id.clone_from(&previous.definition_id);
    projected.props.clone_from(&previous.props);
    projected.extra.clone_from(&previous.extra);
    for (previous, projected) in previous.children.iter().zip(&mut projected.children) {
        if !transfer_content_metadata(previous, projected) {
            return false;
        }
    }
    true
}

fn same_display_text(previous: &TextFlow, projected: &TextFlow) -> bool {
    previous
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>()
        == projected
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()
}

/// 验证文档中每棵数学树都与其 LaTeX 源码表达相同语义。
/// `caret-doc` 只校验稳定 schema；这里依赖解析器，因此不反向依赖 `caret-math`。
#[cfg(test)]
fn validate_document_math_sources(document: &Document) -> Result<(), String> {
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

#[cfg(test)]
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

/// 返回源码可表达语义一致的解析树；树独有元数据由编辑事务另行核验。
pub(crate) fn parse_tree_with_same_semantics(
    tree: &MathTree,
    kind: MathInputKind,
) -> Option<MathTree> {
    let caret_math::ParsedMath::Tree(parsed) = caret_math::parse(&tree.source, kind).ok()? else {
        return None;
    };
    same_flow_semantics(&tree.root, &parsed.root).then_some(parsed)
}

#[cfg(test)]
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
                    .any(|(parsed, stored)| !same_flow_semantics(&stored.root, &parsed.root))
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

#[cfg(test)]
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
                && left.source_map.len() == right.source_map.len()
                && left
                    .source_map
                    .iter()
                    .zip(&right.source_map)
                    .all(|(left, right)| {
                        (left.text_start, left.text_end) == (right.text_start, right.text_end)
                            && (left.source_range.start, left.source_range.end)
                                == (right.source_range.start, right.source_range.end)
                    })
        }
        (MathContent::Math(left), MathContent::Math(right)) => same_node_semantics(left, right),
        (MathContent::Environment(left), MathContent::Environment(right)) => {
            left.kind == right.kind
                && source_environment_props_match(left.kind, &left.props, &right.props)
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

fn source_environment_props_match(
    kind: caret_doc::InlineEnvironmentKind,
    stored: &std::collections::BTreeMap<String, serde_json::Value>,
    parsed: &std::collections::BTreeMap<String, serde_json::Value>,
) -> bool {
    // 未知属性可作为元数据保留；已知样式属性必须能由源码准确表达。
    let parsed_props_match = parsed
        .iter()
        .all(|(key, value)| stored.get(key) == Some(value));
    if !parsed_props_match {
        return false;
    }

    let enabled_is_representable = !kind.supports_enabled()
        || match stored.get("enabled") {
            None => true,
            Some(serde_json::Value::Bool(enabled)) => {
                parsed
                    .get("enabled")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true)
                    == *enabled
            }
            Some(_) => false,
        };
    let font_is_representable = !kind.supports_font_family()
        || match stored.get("fontFamily") {
            None => true,
            Some(family) => parsed.get("fontFamily") == Some(family),
        };

    enabled_is_representable && font_is_representable
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
    fn stale_temporary_source_does_not_invalidate_the_persistent_tree() {
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

        assert!(document.validate().is_ok());
        assert!(crate::Session::new(document).is_ok());
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
    fn formula_text_source_map_must_match_the_parser_output() {
        let mut tree = parse_inline(r"\text{a\%b}");
        let Some(MathContent::Text(text)) = tree.root.content.first_mut() else {
            panic!("text formula should start with a text flow");
        };
        text.source_map[1].source_range.start += 1;
        assert!(
            validate_tree(&tree, MathInputKind::Inline, "text")
                .is_err_and(|error| error.contains("source and tree disagree"))
        );
    }

    #[test]
    fn formula_text_environment_semantics_match_source_and_keep_extensions() {
        let mut tree = parse_inline(r"\text{\textbf{x}}");
        {
            let Some(MathContent::Environment(environment)) = tree.root.content.first_mut() else {
                panic!("formatted text formula should start with an environment");
            };
            environment
                .props
                .insert("vendorProperty".to_owned(), serde_json::json!("kept"));
        }
        assert_eq!(validate_tree(&tree, MathInputKind::Inline, "text"), Ok(()));
        {
            let Some(MathContent::Environment(environment)) = tree.root.content.first_mut() else {
                panic!("formatted text formula should start with an environment");
            };
            environment
                .props
                .insert("enabled".to_owned(), serde_json::json!(false));
        }
        assert!(
            validate_tree(&tree, MathInputKind::Inline, "text")
                .is_err_and(|error| error.contains("source and tree disagree"))
        );

        let mut document = Document::blank();
        let Some(Block::Paragraph(paragraph)) = document.body.first_mut() else {
            panic!("blank document should have a paragraph");
        };
        paragraph.inlines = vec![ParagraphContent::Math(InlineMath {
            id: NodeId::generate(),
            math_tree: tree,
            extra: Default::default(),
        })];
        assert!(crate::Session::new(document).is_ok());
    }

    #[test]
    fn formula_text_source_can_reset_bold_and_italic_semantics() {
        let bold_reset = parse_inline(r"\text{a\textbf{b\textmd{c}}}");
        let italic_reset = parse_inline(r"\text{a\textit{b\textup{c}}}");
        for tree in [&bold_reset, &italic_reset] {
            assert_eq!(validate_tree(tree, MathInputKind::Inline, "text"), Ok(()));
        }

        let Some(MathContent::Environment(outer_bold)) = bold_reset.root.content.get(1) else {
            panic!("bold text style should produce an inline environment");
        };
        assert_eq!(outer_bold.kind, caret_doc::InlineEnvironmentKind::Bold);
        let Some(MathContent::Environment(reset_bold)) = outer_bold.children.get(1) else {
            panic!("medium text style should produce a nested inline environment");
        };
        assert_eq!(reset_bold.kind, caret_doc::InlineEnvironmentKind::Bold);
        assert_eq!(
            reset_bold.props.get("enabled"),
            Some(&serde_json::json!(false))
        );

        let Some(MathContent::Environment(outer_italic)) = italic_reset.root.content.get(1) else {
            panic!("italic text style should produce an inline environment");
        };
        assert_eq!(outer_italic.kind, caret_doc::InlineEnvironmentKind::Italic);
        let Some(MathContent::Environment(reset_italic)) = outer_italic.children.get(1) else {
            panic!("upright text style should produce a nested inline environment");
        };
        assert_eq!(reset_italic.kind, caret_doc::InlineEnvironmentKind::Italic);
        assert_eq!(
            reset_italic.props.get("enabled"),
            Some(&serde_json::json!(false))
        );
    }

    #[test]
    fn source_font_command_must_match_the_persisted_text_flow() {
        let mut tree = parse_inline(r"\text{\textsf{x}}");
        tree.source = r"\text{\texttt{x}}".to_owned();
        assert!(
            validate_tree(&tree, MathInputKind::Inline, "text")
                .is_err_and(|error| error.contains("source and tree disagree"))
        );

        let mut document = Document::blank();
        let Some(Block::Paragraph(paragraph)) = document.body.first_mut() else {
            panic!("blank document should have a paragraph");
        };
        paragraph.inlines = vec![ParagraphContent::Math(InlineMath {
            id: NodeId::generate(),
            math_tree: tree,
            extra: Default::default(),
        })];
        assert!(crate::Session::new(document).is_ok());
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

    #[test]
    fn align_text_style_semantics_must_match_the_source() {
        let mut align = match caret_math::parse(
            r"\begin{aligned}\text{\textbf{x}}\end{aligned}",
            MathInputKind::Align,
        ) {
            Ok(caret_math::ParsedMath::Align(align)) => align,
            Ok(caret_math::ParsedMath::Tree(_)) => panic!("expected aligned formula"),
            Err(error) => panic!("test align formula should parse: {error}"),
        };
        let Some(MathContent::Environment(environment)) =
            align.rows[0].cells[0].root.content.first_mut()
        else {
            panic!("formatted text cell should start with an environment");
        };
        environment
            .props
            .insert("enabled".to_owned(), serde_json::json!(false));
        assert!(validate_align(&align).is_err());

        let mut align = match caret_math::parse(
            r"\begin{aligned}\text{\textsf{x}}\end{aligned}",
            MathInputKind::Align,
        ) {
            Ok(caret_math::ParsedMath::Align(align)) => align,
            Ok(caret_math::ParsedMath::Tree(_)) => panic!("expected aligned formula"),
            Err(error) => panic!("test align formula should parse: {error}"),
        };
        align.source = align.source.replace(r"\textsf", r"\texttt");
        assert!(validate_align(&align).is_err());
    }
}
