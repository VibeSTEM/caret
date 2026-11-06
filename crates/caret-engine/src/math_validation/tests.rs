//! 数学源码投影与持久化树校验的回归测试。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;
use caret_doc::{InlineMath, NodeId, OpenedDocument};
use std::collections::BTreeMap;

fn parse_inline(source: &str) -> MathTree {
    match caret_math::parse_projection(source, MathInputKind::Inline) {
        Ok(projection) => match projection.into_parts().0 {
            caret_math::ProjectedMath::Tree(tree) => tree,
            caret_math::ProjectedMath::Align(_) => panic!("expected an inline tree"),
        },
        Err(error) => panic!("test formula should parse: {error}"),
    }
}

fn projection_for_tree(tree: &MathTree, source: &str) -> caret_math::MathSourceMap<'static> {
    let projection = caret_math::parse_projection(source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("test formula should project: {error}"));
    let (caret_math::ProjectedMath::Tree(parsed), mut source_map) = projection.into_parts() else {
        panic!("test formula should produce a tree");
    };
    let candidate = caret_math::reconcile_tree_ranges(tree, &parsed)
        .unwrap_or_else(|| panic!("test target should reconcile with its source"));
    assert!(bind_tree_sidecar(
        &mut source_map,
        &parsed,
        &candidate,
        None
    ));
    source_map
}

#[test]
fn source_projection_is_not_stored_in_the_persistent_tree() {
    let tree = parse_inline("x");
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
fn edit_projection_is_regenerated_from_the_persistent_tree() {
    let source = r"\frac {x}{\text{t}}";
    let mut tree = parse_inline(source);
    let canonical = caret_math::serialize_tree(&tree);
    let Some(MathContent::Math(fraction)) = tree.root.content.first_mut() else {
        panic!("fraction should be the root formula content");
    };
    let Some(MathContent::Math(_)) = fraction
        .children
        .first()
        .and_then(|flow| flow.content.first())
    else {
        panic!("fraction numerator should be a math node");
    };
    let Some(MathContent::Text(text)) = fraction
        .children
        .get_mut(1)
        .and_then(|flow| flow.content.first_mut())
    else {
        panic!("fraction denominator should contain text");
    };
    if let Some(run) = text.runs.first_mut() {
        run.props
            .insert("underline".to_owned(), serde_json::json!(true));
        run.extra
            .insert("vendorRun".to_owned(), serde_json::json!("kept"));
    }
    let before = serde_json::to_value(&tree)
        .unwrap_or_else(|error| panic!("math tree should serialize: {error}"));

    let (source_map, projected) = project_tree_for_edit(&tree, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("matching noncanonical source should project: {error}"));

    assert_eq!(source_map.source(), canonical);
    assert_ne!(source_map.source(), source);
    assert_eq!(
        caret_math::serialize_tree(&projected),
        caret_math::serialize_tree(&tree)
    );
    assert_eq!(
        serde_json::to_value(&projected).ok(),
        serde_json::to_value(&tree).ok()
    );
    let projected_text = projected
        .root
        .content
        .iter()
        .find_map(|content| match content {
            MathContent::Math(node) => node
                .children
                .iter()
                .flat_map(|flow| &flow.content)
                .find_map(|content| match content {
                    MathContent::Text(text) => Some(text),
                    _ => None,
                }),
            _ => None,
        })
        .unwrap_or_else(|| panic!("projected formula should retain its text flow"));
    let text_map = source_map
        .tree()
        .and_then(|tree| tree.root().content().first())
        .and_then(caret_math::MathContentSourceMap::operands)
        .and_then(|operands| operands.get(1))
        .and_then(|flow| flow.content().first())
        .and_then(caret_math::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("projected text flow should have a sidecar"));
    assert!(text_map.range().is_some());
    assert_eq!(
        projected_text.runs[0].props["underline"],
        serde_json::json!(true)
    );
    assert_eq!(
        projected_text.runs[0].extra["vendorRun"],
        serde_json::json!("kept")
    );
    assert_eq!(
        before,
        serde_json::to_value(&tree)
            .unwrap_or_else(|error| panic!("math tree should serialize: {error}"))
    );
    let history_tree = tree_for_history_projection(&tree, &canonical, MathInputKind::Inline)
        .unwrap_or_else(|| panic!("canonical history source should validate the sidecar"));
    assert_eq!(
        serde_json::to_value(&history_tree).ok(),
        serde_json::to_value(&tree).ok()
    );
    assert!(matches!(
        history_tree.root.content.first(),
        Some(MathContent::Math(node)) if node.children.get(1).is_some_and(|flow| matches!(flow.content.first(), Some(MathContent::Text(_))))
    ));
}

#[test]
fn edit_projection_retains_document_font_family_not_represented_by_tex() {
    let mut tree = parse_inline(r"\text{abcd}");
    let Some(MathContent::Text(text)) = tree.root.content.pop() else {
        panic!("formula should contain a text flow");
    };
    tree.root
        .content
        .push(MathContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: caret_doc::InlineEnvironmentKind::Font,
            definition_id: None,
            props: BTreeMap::from([("fontFamily".to_owned(), serde_json::json!("Noto Serif"))]),
            children: vec![MathContent::Text(text)],
            extra: BTreeMap::new(),
        }));

    let (projection, projected) = project_tree_for_edit(&tree, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("font family should survive location projection: {error}"));
    assert_eq!(projection.source(), r"\text{\textrm{abcd}}");
    let Some(MathContent::Environment(environment)) = projected.root.content.first() else {
        panic!("font environment should remain in the document tree");
    };
    assert_eq!(
        environment.props.get("fontFamily"),
        Some(&serde_json::json!("Noto Serif"))
    );
}

#[test]
fn source_projection_rejects_a_different_font_style_class_atomically() {
    let mut tree = parse_inline(r"\text{abcd}");
    let Some(MathContent::Text(text)) = tree.root.content.pop() else {
        panic!("formula should contain a text flow");
    };
    tree.root
        .content
        .push(MathContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: caret_doc::InlineEnvironmentKind::Font,
            definition_id: None,
            props: BTreeMap::from([("fontFamily".to_owned(), serde_json::json!("Noto Serif"))]),
            children: vec![MathContent::Text(text)],
            extra: BTreeMap::new(),
        }));
    let before = serde_json::to_value(&tree)
        .unwrap_or_else(|error| panic!("math tree should serialize: {error}"));

    assert!(
        tree_for_history_projection_with_map(&tree, r"\text{\textsf{abcd}}", MathInputKind::Inline)
            .is_none()
    );
    assert_eq!(serde_json::to_value(&tree).ok(), Some(before));
}

#[test]
fn history_projection_rejects_a_missing_empty_math_anchor_in_a_text_flow() {
    let mut tree = parse_inline(r"\text{abcd}");
    let Some(MathContent::Text(text)) = tree.root.content.first_mut() else {
        panic!("formula should contain a text flow");
    };
    text.runs[0]
        .props
        .insert("underline".to_owned(), serde_json::json!(true));
    text.runs[0]
        .extra
        .insert("vendorRun".to_owned(), serde_json::json!("kept"));
    let persistent_before = serde_json::to_value(&tree)
        .unwrap_or_else(|error| panic!("math tree should serialize: {error}"));

    for source in [r"\text{$$abcd}", r"\text{ab$$cd}", r"\text{abcd$$}"] {
        assert!(
            tree_for_source_history_projection(&tree, source, MathInputKind::Inline).is_none(),
            "source anchor must not map to a tree without a matching placeholder: {source}"
        );
    }
    assert_eq!(serde_json::to_value(&tree).ok(), Some(persistent_before));
    let Some(MathContent::Text(unchanged)) = tree.root.content.first() else {
        panic!("projection failure must leave the persisted text flow intact");
    };
    assert_eq!(unchanged.runs[0].text, "abcd");
    assert_eq!(
        unchanged.runs[0].props["underline"],
        serde_json::json!(true)
    );
    assert_eq!(
        unchanged.runs[0].extra["vendorRun"],
        serde_json::json!("kept")
    );
}

#[test]
fn history_projection_preserves_an_explicit_empty_math_anchor_in_text() {
    fn placeholder_id_in_content(content: &MathContent) -> Option<NodeId> {
        match content {
            MathContent::Math(node) if node.kind == caret_doc::MathNodeKind::Placeholder => {
                Some(node.id.clone())
            }
            MathContent::Math(node) => node.children.iter().find_map(placeholder_id),
            MathContent::Environment(environment) => environment
                .children
                .iter()
                .find_map(placeholder_id_in_content),
            MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => None,
        }
    }

    fn placeholder_id(flow: &MathFlow) -> Option<NodeId> {
        flow.content.iter().find_map(placeholder_id_in_content)
    }

    let tree = parse_inline(r"\text{ab$$cd}");
    let expected_id = placeholder_id(&tree.root)
        .unwrap_or_else(|| panic!("the source contains an explicit empty math anchor"));
    let source = caret_math::serialize_tree(&tree);
    let projected = tree_for_source_history_projection(&tree, &source, MathInputKind::Inline)
        .unwrap_or_else(|| panic!("the persisted anchor tree should reproject"));

    assert_eq!(placeholder_id(&projected.root), Some(expected_id));
    assert_eq!(
        serde_json::to_value(&projected).ok(),
        serde_json::to_value(&tree).ok()
    );
}

#[test]
fn history_projection_rejects_unmatched_persistent_nodes_atomically() {
    let tree = parse_inline("x");
    let before = serde_json::to_value(&tree)
        .unwrap_or_else(|error| panic!("math tree should serialize: {error}"));

    assert!(tree_for_history_projection(&tree, "y", MathInputKind::Inline).is_none());
    assert_eq!(
        serde_json::to_value(&tree)
            .unwrap_or_else(|error| panic!("math tree should serialize: {error}")),
        before
    );
}

#[test]
fn history_projection_does_not_expand_placeholders_into_math_nodes() {
    let mut tree = parse_inline("");
    tree.root.content.push(MathContent::Math(MathNode {
        id: NodeId::generate(),
        kind: caret_doc::MathNodeKind::Placeholder,
        text: None,
        attributes: BTreeMap::new(),
        children: Vec::new(),
        extra: BTreeMap::new(),
    }));
    for source in ["x", r"\frac{a}{b}"] {
        assert!(
            tree_for_history_projection(&tree, source, MathInputKind::Inline).is_none(),
            "placeholder must not be rebound to {source:?}"
        );
    }
}

#[test]
fn source_history_rejects_uncommitted_placeholder_shape() {
    let mut tree = parse_inline("");
    let placeholder = MathNode {
        id: NodeId::generate(),
        kind: caret_doc::MathNodeKind::Placeholder,
        text: None,
        attributes: BTreeMap::new(),
        children: Vec::new(),
        extra: BTreeMap::new(),
    };
    tree.root
        .content
        .push(MathContent::Math(placeholder.clone()));

    assert!(tree_for_history_projection(&tree, "x", MathInputKind::Inline).is_none());
    let Some(MathContent::Math(unchanged)) = tree.root.content.first() else {
        panic!("failed projection must leave the document placeholder intact");
    };
    assert_eq!(unchanged.kind, caret_doc::MathNodeKind::Placeholder);
    assert_eq!(unchanged.id, placeholder.id);
    assert_eq!(unchanged.extra, placeholder.extra);
}

#[test]
fn source_history_rebinds_text_ranges_across_canonical_text_flow_merge() -> Result<(), String> {
    let tree = parse_inline(r"\text{a}\text{b}");
    assert_eq!(caret_math::serialize_tree(&tree), r"\text{ab}");
    let stored_projection = projection_for_tree(&tree, r"\text{ab}");

    let (updated, source_map) = tree_for_source_history_projection_with_map(
        &tree,
        &stored_projection,
        MathInputKind::Inline,
    )
    .ok_or_else(|| "canonical source should preserve text-flow identity and ranges".to_owned())?;
    assert_eq!(updated, tree);
    let canonical_tree_map = stored_projection
        .tree()
        .ok_or_else(|| "bound canonical source should expose its tree map".to_owned())?;
    assert_eq!(&source_map, canonical_tree_map);
    Ok(())
}

#[test]
fn source_history_rebinds_text_ranges_inside_inline_environment() -> Result<(), String> {
    let source = r"\text{\textbf{ab}}";
    let mut tree = parse_inline(source);
    let Some(MathContent::Environment(environment)) = tree.root.content.first_mut() else {
        return Err("formatted formula should start with an inline environment".to_owned());
    };
    let Some(MathContent::Text(text)) = environment.children.first() else {
        return Err("formatted environment should contain a text flow".to_owned());
    };
    let mut first = text.clone();
    let mut second = text.clone();
    let Some(run) = first.runs.first_mut() else {
        return Err("text flow should contain a run".to_owned());
    };
    run.text = "a".to_owned();
    let Some(run) = second.runs.first_mut() else {
        return Err("text flow should contain a run".to_owned());
    };
    run.text = "b".to_owned();
    environment.children = vec![MathContent::Text(first), MathContent::Text(second)];
    let stored_projection = projection_for_tree(&tree, source);

    let (updated, source_map) = tree_for_source_history_projection_with_map(
        &tree,
        &stored_projection,
        MathInputKind::Inline,
    )
    .ok_or_else(|| "nested canonical projection should refresh text ranges".to_owned())?;
    let Some(MathContent::Environment(environment)) = updated.root.content.first() else {
        return Err("updated formula should preserve its inline environment".to_owned());
    };
    if environment.children.len() != 2 {
        return Err("history projection should preserve both text-flow identities".to_owned());
    }
    let canonical_tree_map = stored_projection
        .tree()
        .ok_or_else(|| "bound canonical source should expose its tree map".to_owned())?;
    if &source_map != canonical_tree_map || updated != tree {
        return Err("nested range refresh should match canonical source locations".to_owned());
    }
    Ok(())
}

#[test]
fn source_history_uses_sidecar_when_the_tree_has_no_temporary_ranges() {
    let source = r"\frac {\text{same}}{\text{same}}";
    let projection = caret_math::parse_projection(source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("noncanonical source should project: {error}"));
    let (caret_math::ProjectedMath::Tree(mut tree), stored_projection) = projection.into_parts()
    else {
        panic!("inline formula should produce a tree");
    };
    let Some(MathContent::Math(fraction)) = tree.root.content.first_mut() else {
        panic!("formula should contain a fraction");
    };
    let fraction_id = fraction.id.clone();
    let Some(MathContent::Text(text)) = fraction
        .children
        .get_mut(1)
        .and_then(|flow| flow.content.first_mut())
    else {
        panic!("second operand should contain text");
    };
    text.runs[0]
        .props
        .insert("underline".to_owned(), serde_json::json!(true));
    text.runs[0]
        .extra
        .insert("vendorRun".to_owned(), serde_json::json!("retained"));
    let before = tree.clone();

    let (restored, map) = tree_for_source_history_projection_with_map(
        &tree,
        &stored_projection,
        MathInputKind::Inline,
    )
    .unwrap_or_else(|| panic!("matching source sidecar should restore the tree"));

    assert_eq!(tree, before);
    assert_eq!(restored, before);
    assert_eq!(Some(&map), stored_projection.tree());
    assert!(
        matches!(restored.root.content.first(), Some(MathContent::Math(node)) if node.id == fraction_id)
    );
}

#[test]
fn source_history_rejects_a_stale_tree_version_without_changing_the_tree() {
    let source = r"\frac{a}{b}";
    let projection = caret_math::parse_projection(source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("formula should project: {error}"));
    let (caret_math::ProjectedMath::Tree(mut tree), stale_projection) = projection.into_parts()
    else {
        panic!("inline formula should produce a tree");
    };
    let Some(MathContent::Math(fraction)) = tree.root.content.first_mut() else {
        panic!("formula should contain a fraction");
    };
    fraction.kind = caret_doc::MathNodeKind::Radical;
    let before = tree.clone();

    assert!(
        tree_for_source_history_projection_with_map(
            &tree,
            &stale_projection,
            MathInputKind::Inline
        )
        .is_none()
    );
    assert_eq!(tree, before);
}

#[test]
fn align_source_history_keeps_cell_identity_without_dto_ranges() {
    let source = r"\begin{aligned}\text{same}&\text{same}\\x&y\end{aligned}";
    let projection = caret_math::parse_projection(source, MathInputKind::Align)
        .unwrap_or_else(|error| panic!("Align source should project: {error}"));
    let (caret_math::ProjectedMath::Align(align), stored_projection) = projection.into_parts()
    else {
        panic!("Align source should produce an Align");
    };
    let before = align.clone();

    let (restored, map) = align_for_source_history_projection_with_map(&align, &stored_projection)
        .unwrap_or_else(|| panic!("matching Align sidecar should restore every cell"));

    assert_eq!(align, before);
    assert_eq!(restored, before);
    assert_eq!(Some(&map), stored_projection.align());
}

#[test]
fn canonical_text_flow_merge_keeps_model_only_run_properties_opaque_to_source_shape() {
    let mut tree = parse_inline(r"\text{a}\text{b}");
    let Some(MathContent::Text(second)) = tree.root.content.get_mut(1) else {
        panic!("second text flow should remain separately addressable before projection");
    };
    let Some(run) = second.runs.first_mut() else {
        panic!("second text flow should contain a run");
    };
    run.props
        .insert("underline".to_owned(), serde_json::json!(true));

    assert!(parse_tree_with_same_semantics(&tree, MathInputKind::Inline).is_some());
}

#[test]
fn align_edit_projection_keeps_full_source_with_outer_trivia() {
    let source = " % before\n\\begin {aligned}a&b\\end {aligned} % after\n";
    let caret_math::ParsedMath::Align(align) = caret_math::parse(source, MathInputKind::Align)
        .unwrap_or_else(|error| panic!("valid Align source should parse: {error}"))
    else {
        panic!("Align input should parse to an Align tree");
    };

    let (source_map, _projected) = project_align_for_edit(&align)
        .unwrap_or_else(|error| panic!("matching Align source should project: {error}"));

    assert_eq!(source_map.source(), caret_math::serialize_align(&align));
    assert_ne!(source_map.source(), source);
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
    assert_eq!(validate_tree(&tree, MathInputKind::Inline, "text"), Ok(()));
}

#[test]
fn source_history_rejects_stale_escaped_text_tree_without_changing_it() {
    let projection = caret_math::parse_projection(r"\text{a\%b}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("escaped text should project: {error}"));
    let (caret_math::ProjectedMath::Tree(mut tree), stale_projection) = projection.into_parts()
    else {
        panic!("inline source should produce a tree");
    };
    let Some(MathContent::Text(text)) = tree.root.content.first_mut() else {
        panic!("text formula should start with a text flow");
    };
    text.runs[0].text.push('x');
    let before = tree.clone();
    assert!(
        tree_for_source_history_projection_with_map(
            &tree,
            &stale_projection,
            MathInputKind::Inline,
        )
        .is_none()
    );
    assert_eq!(tree, before);
}

#[test]
fn formula_text_environment_semantics_match_source_and_keep_extensions() {
    let mut tree = parse_inline(r"\frac{\text{\textbf{x}}}{1}");
    {
        let Some(MathContent::Math(fraction)) = tree.root.content.first_mut() else {
            panic!("formula should start with a fraction");
        };
        let Some(MathContent::Environment(environment)) = fraction
            .children
            .first_mut()
            .and_then(|flow| flow.content.first_mut())
        else {
            panic!("fraction numerator should contain a text format environment");
        };
        environment
            .props
            .insert("vendorProperty".to_owned(), serde_json::json!("kept"));
    }
    assert_eq!(validate_tree(&tree, MathInputKind::Inline, "text"), Ok(()));
    {
        let Some(MathContent::Math(fraction)) = tree.root.content.first_mut() else {
            panic!("formula should start with a fraction");
        };
        let Some(MathContent::Environment(environment)) = fraction
            .children
            .first_mut()
            .and_then(|flow| flow.content.first_mut())
        else {
            panic!("fraction numerator should contain a text format environment");
        };
        environment
            .props
            .insert("enabled".to_owned(), serde_json::json!(false));
    }
    assert_eq!(validate_tree(&tree, MathInputKind::Inline, "text"), Ok(()));

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
fn edited_text_style_is_the_source_projection_authority() {
    let tree = parse_inline(r"\frac{\text{\textsf{x}}}{1}");
    assert_eq!(validate_tree(&tree, MathInputKind::Inline, "text"), Ok(()));

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
fn align_source_history_rejects_stale_cell_mapping_atomically() {
    let projection =
        caret_math::parse_projection(r"\begin{aligned}xy\end{aligned}", MathInputKind::Align)
            .unwrap_or_else(|error| panic!("test align formula should project: {error}"));
    let (caret_math::ProjectedMath::Align(mut align), stale_projection) = projection.into_parts()
    else {
        panic!("expected aligned formula");
    };
    let Some(MathContent::Math(node)) = align.rows[0].cells[0].root.content.first_mut() else {
        panic!("parsed cell should retain its math node");
    };
    node.kind = caret_doc::MathNodeKind::Radical;
    let before = align.clone();
    assert!(align_for_source_history_projection_with_map(&align, &stale_projection).is_none());
    assert_eq!(align, before);
}

#[test]
fn align_text_style_semantics_must_match_the_source() {
    let mut align = match caret_math::parse_projection(
        r"\begin{aligned}\text{\textbf{x}}\end{aligned}",
        MathInputKind::Align,
    ) {
        Ok(projection) => match projection.into_parts().0 {
            caret_math::ProjectedMath::Align(align) => align,
            caret_math::ProjectedMath::Tree(_) => panic!("expected aligned formula"),
        },
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
    assert!(validate_align(&align).is_ok());

    let align = match caret_math::parse_projection(
        r"\begin{aligned}\text{\textsf{x}}\end{aligned}",
        MathInputKind::Align,
    ) {
        Ok(projection) => match projection.into_parts().0 {
            caret_math::ProjectedMath::Align(align) => align,
            caret_math::ProjectedMath::Tree(_) => panic!("expected aligned formula"),
        },
        Err(error) => panic!("test align formula should parse: {error}"),
    };
    assert!(validate_align(&align).is_ok());
}
