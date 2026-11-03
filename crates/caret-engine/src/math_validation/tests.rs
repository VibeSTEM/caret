//! 数学源码投影与持久化树校验的回归测试。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

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
    let original_text_range = tree.root.content.iter().find_map(|content| match content {
        MathContent::Math(node) => node
            .children
            .iter()
            .flat_map(|flow| &flow.content)
            .find_map(|content| match content {
                MathContent::Text(text) => text
                    .source_range
                    .as_ref()
                    .map(|range| (range.start, range.end)),
                _ => None,
            }),
        _ => None,
    });
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
    if let Some(range) = &mut text.source_range {
        range
            .extra
            .insert("vendorRange".to_owned(), serde_json::json!(7));
    }
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
    assert_ne!(
        projected_text
            .source_range
            .as_ref()
            .map(|range| (range.start, range.end)),
        original_text_range
    );
    assert_eq!(
        projected_text
            .source_range
            .as_ref()
            .map(|range| &range.extra["vendorRange"]),
        Some(&serde_json::json!(7))
    );
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
        .unwrap_or_else(|| panic!("canonical history source should rebind transient ranges"));
    assert_eq!(
        serde_json::to_value(&history_tree).ok(),
        serde_json::to_value(&tree).ok()
    );
    let history_text = history_tree
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
        .unwrap_or_else(|| panic!("history formula should retain its text flow"));
    assert_eq!(history_text.source_range, projected_text.source_range);
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
    text.source_range = Some(MathSourceRange {
        start: 6,
        end: 10,
        extra: BTreeMap::from([("vendorRange".to_owned(), serde_json::json!(9))]),
    });
    text.source_map = vec![caret_doc::TextFlowSourceMapEntry {
        text_start: 0,
        text_end: 4,
        source_range: MathSourceRange {
            start: 6,
            end: 10,
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }];
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
    assert_eq!(
        unchanged
            .source_range
            .as_ref()
            .map(|range| range.start..range.end),
        Some(6..10)
    );
    assert_eq!(unchanged.source_map.len(), 1);
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
        source_range: None,
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
        source_range: Some(MathSourceRange {
            start: 0,
            end: 0,
            extra: BTreeMap::from([("vendorRange".to_owned(), serde_json::json!(3))]),
        }),
        children: Vec::new(),
        extra: BTreeMap::new(),
    };
    let original_range = placeholder.source_range.clone();
    tree.root
        .content
        .push(MathContent::Math(placeholder.clone()));

    assert!(tree_for_history_projection(&tree, "x", MathInputKind::Inline).is_none());
    let Some(MathContent::Math(unchanged)) = tree.root.content.first() else {
        panic!("failed projection must leave the document placeholder intact");
    };
    assert_eq!(unchanged.kind, caret_doc::MathNodeKind::Placeholder);
    assert_eq!(unchanged.id, placeholder.id);
    assert_eq!(unchanged.source_range, original_range);
}

#[test]
fn source_history_rejects_semantic_equivalence_with_stale_text_ranges() {
    let tree = parse_inline(r"\text{a}\text{b}");
    assert_eq!(caret_math::serialize_tree(&tree), r"\text{ab}");

    assert!(
        tree_for_source_history_projection(&tree, r"\text{ab}", MathInputKind::Inline).is_none()
    );
}

#[test]
fn duplicate_empty_source_map_ranges_keep_extensions_one_to_one() {
    let make_entry = |extension: &str, source_extension: &str| caret_doc::TextFlowSourceMapEntry {
        text_start: 0,
        text_end: 0,
        source_range: MathSourceRange {
            start: 6,
            end: 6,
            extra: BTreeMap::from([(
                "rangeExtension".to_owned(),
                serde_json::json!(source_extension),
            )]),
        },
        extra: BTreeMap::from([("entryExtension".to_owned(), serde_json::json!(extension))]),
    };
    let mut text = TextFlow {
        runs: vec![caret_doc::Run {
            text: String::new(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        source_range: None,
        source_map: vec![
            make_entry("first", "first-range"),
            make_entry("second", "second-range"),
        ],
        extra: BTreeMap::new(),
    };
    let refreshed = vec![
        make_entry("parsed-first", "parsed-first-range"),
        make_entry("parsed-second", "parsed-second-range"),
    ];

    assert!(refresh_text_ranges_from_map(&mut text, None, refreshed));
    assert_eq!(text.source_map[0].extra["entryExtension"], "first");
    assert_eq!(text.source_map[1].extra["entryExtension"], "second");
    assert_eq!(
        text.source_map[0].source_range.extra["rangeExtension"],
        "first-range"
    );
    assert_eq!(
        text.source_map[1].source_range.extra["rangeExtension"],
        "second-range"
    );
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
    let tree = parse_inline(r"\text{\textsf{x}}");
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
fn align_cell_source_and_node_ranges_must_match_the_parsed_tree() {
    let mut align = match caret_math::parse(r"\begin{aligned}xy\end{aligned}", MathInputKind::Align)
    {
        Ok(caret_math::ParsedMath::Align(align)) => align,
        Ok(caret_math::ParsedMath::Tree(_)) => panic!("expected aligned formula"),
        Err(error) => panic!("test align formula should parse: {error}"),
    };
    let Some(MathContent::Math(node)) = align.rows[0].cells[0].root.content.first_mut() else {
        panic!("parsed cell should retain a ranged node");
    };
    let Some(range) = node.source_range.as_mut() else {
        panic!("parsed cell node should retain its projection range");
    };
    range.end = range.end.saturating_add(1);
    let Err(error) = validate_align(&align) else {
        panic!("mismatched align source and cells must be rejected");
    };
    assert!(!error.is_empty());

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
    assert!(validate_align(&align).is_ok());

    let align = match caret_math::parse(
        r"\begin{aligned}\text{\textsf{x}}\end{aligned}",
        MathInputKind::Align,
    ) {
        Ok(caret_math::ParsedMath::Align(align)) => align,
        Ok(caret_math::ParsedMath::Tree(_)) => panic!("expected aligned formula"),
        Err(error) => panic!("test align formula should parse: {error}"),
    };
    assert!(validate_align(&align).is_ok());
}
