//! 解析器与序列化器的行为测试。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::collections::BTreeMap;

use caret_doc::{
    InlineEnvironment, InlineEnvironmentKind, MathContent, MathFlow, MathNode, MathNodeKind as K,
    MathTree, NodeId, Run, TextFlow,
};

use crate::{
    MathInputKind, MathInputStatus, MathParseError, ParsedMath, ProjectedMath, classify_input,
    input_body_range_for_edit, is_incomplete_input_prefix, parse, serialize_align, serialize_tree,
};

/// 去掉 ID 和源范围后的结构描述，用于比较往返结果。
fn shape_node(node: &MathNode) -> String {
    let mut out = format!("{:?}", node.kind);
    if let Some(text) = &node.text {
        out.push_str(&format!("({text:?})"));
    }
    if !node.attributes.is_empty() {
        out.push_str(&format!("{:?}", node.attributes));
    }
    if !node.children.is_empty() {
        out.push('[');
        out.push_str(
            &node
                .children
                .iter()
                .map(shape_flow)
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push(']');
    }
    out
}

fn shape_flow(flow: &MathFlow) -> String {
    flow.content
        .iter()
        .map(|content| match content {
            MathContent::Math(node) => shape_node(node),
            MathContent::Text(text) => format!(
                "Text({:?})",
                text.runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>()
            ),
            MathContent::Environment(environment) => format!(
                "Env({:?})[{}]",
                environment.kind,
                environment
                    .children
                    .iter()
                    .map(shape_content)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            MathContent::SoftBreak(_) => "SoftBreak".to_owned(),
            MathContent::Opaque(value) => format!("Opaque({})", value.raw),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn shape_content(content: &MathContent) -> String {
    match content {
        MathContent::Math(node) => shape_node(node),
        MathContent::Text(text) => format!(
            "Text({:?})",
            text.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
        ),
        MathContent::Environment(environment) => format!(
            "Env({:?})[{}]",
            environment.kind,
            environment
                .children
                .iter()
                .map(shape_content)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        MathContent::SoftBreak(_) => "SoftBreak".to_owned(),
        MathContent::Opaque(value) => format!("Opaque({})", value.raw),
    }
}

fn shape(flow: &MathFlow) -> String {
    shape_flow(flow)
}

fn root_node_mut(tree: &mut caret_doc::MathTree) -> &mut MathNode {
    let Some(MathContent::Math(node)) = tree.root.content.first_mut() else {
        panic!("expected root math node");
    };
    node
}

fn tree(source: &str) -> caret_doc::MathTree {
    match crate::parse_projection(source, MathInputKind::Equation) {
        Ok(projection) => match projection.into_parts().0 {
            ProjectedMath::Tree(tree) => tree,
            ProjectedMath::Align(_) => panic!("{source}: expected a math tree"),
        },
        other => panic!("{source}: {other:?}"),
    }
}

fn root_shape(source: &str) -> String {
    shape(&tree(source).root)
}

#[test]
fn keeps_literal_symbol_stops_at_extended_grapheme_boundaries() {
    assert_eq!(
        root_shape("ab12+c"),
        r#"Symbol("a"){"atomClass": "ord"}, Symbol("b"){"atomClass": "ord"}, Symbol("1"){"atomClass": "ord"}, Symbol("2"){"atomClass": "ord"}, Symbol("+"){"atomClass": "bin"}, Symbol("c"){"atomClass": "ord"}"#
    );
    // 上标只作用于紧邻的字符，因此 `x` 不与前面的 `a` 合并。
    assert_eq!(
        root_shape("ax^2"),
        r#"Symbol("a"){"atomClass": "ord"}, Superscript[Symbol("x"){"atomClass": "ord"}, Symbol("2"){"atomClass": "ord"}]"#
    );
    assert_eq!(
        root_shape("e\u{301}x"),
        r#"Symbol("e\u{301}"){"atomClass": "ord"}, Symbol("x"){"atomClass": "ord"}"#
    );
    assert_eq!(
        root_shape(r"a\%b"),
        r#"Symbol("a"){"atomClass": "ord"}, Symbol("%"){"atomClass": "ord"}, Symbol("b"){"atomClass": "ord"}"#
    );
    assert_eq!(
        root_shape("\\%\u{301}x"),
        r#"Symbol("%"){"atomClass": "ord"}, Symbol("\u{301}"){"atomClass": "ord"}, Symbol("x"){"atomClass": "ord"}"#
    );
    for source in ["xy", "e\u{301}x", "\\%\u{301}x"] {
        let ParsedMath::Tree(ordinary) = parse(source, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("{source}: {error}"))
        else {
            panic!("{source}: expected a math tree");
        };
        let ProjectedMath::Tree(projected) = crate::parse_projection(source, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("{source}: {error}"))
            .into_parts()
            .0
        else {
            panic!("{source}: expected a projected math tree");
        };
        assert_eq!(shape_flow(&ordinary.root), shape_flow(&projected.root));
    }
}

#[test]
fn scripts_bind_to_the_complete_extended_grapheme_base() {
    for (source, script_name, script_value) in [
        ("e\u{301}^2", "superscript", "2"),
        ("e\u{301}_2", "subscript", "2"),
    ] {
        let expected = format!(
            "{}[Symbol(\"e\\u{{301}}\"){{\"atomClass\": \"ord\"}}, Symbol(\"{script_value}\"){{\"atomClass\": \"ord\"}}]",
            if script_name == "superscript" {
                "Superscript"
            } else {
                "Subscript"
            }
        );
        assert_eq!(root_shape(source), expected, "{source}");

        let projection = crate::parse_projection(source, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("{source}: {error}"));
        let ProjectedMath::Tree(parsed_tree) = projection.content() else {
            panic!("{source}: expected a tree projection");
        };
        let first_range = projection
            .source_map()
            .tree()
            .and_then(|map| map.root().content().first())
            .and_then(crate::MathContentSourceMap::range)
            .unwrap_or_else(|| panic!("{source}: missing script source range"));
        assert_eq!(first_range, 0..source.len());

        let serialized = serialize_tree(parsed_tree);
        let reparsed = tree(&serialized);
        assert_eq!(serialize_tree(&reparsed), serialized);
        assert!(serialized.contains("e\u{301}"));
    }

    assert_eq!(
        root_shape("e\u{301}x"),
        r#"Symbol("e\u{301}"){"atomClass": "ord"}, Symbol("x"){"atomClass": "ord"}"#
    );
}

#[test]
fn unbraced_arguments_and_non_ordinary_literals_keep_grapheme_clusters() {
    assert_eq!(
        root_shape("x^e\u{301}"),
        r#"Superscript[Symbol("x"){"atomClass": "ord"}, Symbol("e\u{301}"){"atomClass": "ord"}]"#
    );
    assert_eq!(
        root_shape("\\sqrt e\u{301}"),
        r#"Radical[Placeholder, Symbol("e\u{301}"){"atomClass": "ord"}]"#
    );
    assert_eq!(
        root_shape("(\u{301}x"),
        r#"Symbol("(\u{301}"){"atomClass": "open"}, Symbol("x"){"atomClass": "ord"}"#
    );

    for source in ["x^e\u{301}", "\\sqrt e\u{301}", "(\u{301}x"] {
        let parsed = tree(source);
        let serialized = serialize_tree(&parsed);
        let reparsed = tree(&serialized);
        assert_eq!(shape(&parsed.root), shape(&reparsed.root), "{source}");
    }
}

#[test]
fn serialized_literal_specials_and_apostrophe_preserve_tree_semantics() {
    assert_eq!(serialize_tree(&tree("\\#")), "\\#");
    assert_eq!(serialize_tree(&tree("\\%")), "\\%");
    assert!(parse("\\char\"20 ", MathInputKind::Inline).is_err());
    assert!(parse("\u{600}\\char\"20 ", MathInputKind::Inline).is_err());
    assert!(parse("\\char\"0A ", MathInputKind::Inline).is_err());
    for (source, expected) in [
        ("\u{600}\\#", "\u{600}\\char\"23 "),
        ("\u{600}\u{600}\\#", "\u{600}\u{600}\\char\"23 "),
        ("\u{600}\\char\"27 ", "\u{600}\\char\"27 "),
        ("\u{600}\\char\"7E ", "\u{600}\\char\"7E "),
        ("\u{600}\\char\"5E ", "\u{600}\\char\"5E "),
    ] {
        let original = tree(source);
        let serialized = serialize_tree(&original);
        let reparsed = tree(&serialized);
        assert_eq!(shape(&original.root), shape(&reparsed.root), "{source:?}");
        assert_eq!(serialized, expected);
    }
    for source in [
        "\\char\"25 \u{301}",
        "\\char\"26 \u{301}",
        "\\char\"23 \u{301}",
        "\\char\"5F \u{301}",
        "\\char\"24 \u{301}",
        "\\char\"7B \u{301}",
        "\\char\"7D \u{301}",
        "\\char\"5C \u{301}",
        "\\char\"27 \u{301}",
        "\\char\"7E \u{301}",
        "\\char\"5E \u{301}",
    ] {
        let original = tree(source);
        assert_eq!(original.root.content.len(), 1, "{source:?}");
        let serialized = serialize_tree(&original);
        let reparsed = tree(&serialized);
        assert_eq!(shape(&original.root), shape(&reparsed.root), "{source:?}");
    }

    let literal_apostrophe = tree("x\\char\"27 ");
    let serialized = serialize_tree(&literal_apostrophe);
    assert_eq!(serialized, "x\\char\"27 ");
    assert_eq!(
        shape(&literal_apostrophe.root),
        shape(&tree(&serialized).root)
    );
    assert!(matches!(
        literal_apostrophe.root.content.as_slice(),
        [MathContent::Math(base), MathContent::Math(apostrophe)]
            if base.text.as_deref() == Some("x")
                && apostrophe.text.as_deref() == Some("'")
                && apostrophe.attributes.contains_key("atomClass")
    ));

    let prime = tree("x'");
    assert!(matches!(
        prime.root.content.as_slice(),
        [MathContent::Math(node)] if node.kind == K::Superscript
    ));
    assert_ne!(shape(&prime.root), shape(&literal_apostrophe.root));
}

#[test]
fn long_plain_ascii_input_keeps_each_literal_atom() {
    let source = "a".repeat(20_000);
    let parsed = tree(&source);
    assert_eq!(parsed.root.content.len(), source.len());
    assert_eq!(serialize_tree(&parsed), source);
}

#[test]
fn grapheme_extension_preserves_tex_tokens_and_scales_to_long_clusters() {
    assert_eq!(
        root_shape("\u{600}~"),
        r#"Symbol("\u{600}"){"atomClass": "ord"}, Space{"spaceKind": "no_break_space"}"#
    );
    assert_eq!(
        root_shape("\u{600}% comment\n x"),
        r#"Symbol("\u{600}"){"atomClass": "ord"}, Symbol("x"){"atomClass": "ord"}"#
    );

    let mut source = String::from("x^e");
    source.push_str(&"\u{301}".repeat(4096));
    let parsed = tree(&source);
    let Some(MathContent::Math(script)) = parsed.root.content.first() else {
        panic!("expected a scripted math node");
    };
    let Some(MathContent::Math(base)) =
        script.children.get(1).and_then(|flow| flow.content.first())
    else {
        panic!("missing script base");
    };
    assert_eq!(base.text.as_deref(), Some(&source[2..]));
    let serialized = serialize_tree(&parsed);
    let reparsed = tree(&serialized);
    assert_eq!(shape(&parsed.root), shape(&reparsed.root));
}

#[test]
fn builds_documented_child_order() {
    assert_eq!(
        root_shape(r"\frac{a}{b}"),
        r#"Fraction{"style": "frac"}[Symbol("a"){"atomClass": "ord"}, Symbol("b"){"atomClass": "ord"}]"#
    );
    assert_eq!(
        root_shape(r"\sqrt{x}"),
        r#"Radical[Placeholder, Symbol("x"){"atomClass": "ord"}]"#
    );
    assert_eq!(
        root_shape(r"\sum_{i}^{n}"),
        r#"Sum[Symbol("i"){"atomClass": "ord"}, Symbol("n"){"atomClass": "ord"}]"#
    );
    assert_eq!(
        root_shape(r"x_1^2"),
        r#"SubSup[Symbol("x"){"atomClass": "ord"}, Symbol("1"){"atomClass": "ord"}, Symbol("2"){"atomClass": "ord"}]"#
    );
    assert_eq!(root_shape(r"\alpha"), r#"Symbol("alpha")"#);
    assert_eq!(
        root_shape(r"\overset{a}{b}"),
        r#"OverUnder[Symbol("b"){"atomClass": "ord"}, Symbol("a"){"atomClass": "ord"}, Placeholder]"#
    );
}

#[test]
fn encodes_delimiters_spaces_and_colors() {
    assert_eq!(
        root_shape(r"\left(x\right\}"),
        r#"Delimited{"left": "char:(", "right": "named:}"}[Symbol("x"){"atomClass": "ord"}]"#
    );
    assert_eq!(
        root_shape(r"\hspace{1.5em}"),
        r#"Space{"spaceKind": "hspace", "valueDenominator": "2", "valueNumerator": "3", "valueUnit": "em"}"#
    );
    assert_eq!(
        root_shape(r"\textcolor{red}{x}"),
        r##"TextColor{"color": "#ff0000"}[Symbol("x"){"atomClass": "ord"}]"##
    );
}

#[test]
fn parses_text_mode() {
    assert_eq!(
        root_shape(r"\text{if $x$ ok}"),
        r#"Text("if "), Symbol("x"){"atomClass": "ord"}, Text(" ok")"#
    );
}

#[test]
fn preserves_an_empty_inline_math_anchor_inside_text_mode() {
    assert_eq!(
        root_shape(r"\text{a$$b}"),
        r#"Text("a"), Placeholder, Text("b")"#
    );
}

#[test]
fn preserves_empty_text_flows_as_editable_tree_content() {
    for source in [r"\text{}", r"\text{\textbf{}}"] {
        let original = tree(source);
        let text_flow = match original.root.content.as_slice() {
            [MathContent::Text(flow)] => Some(flow),
            [MathContent::Environment(environment)] => match environment.children.as_slice() {
                [MathContent::Text(flow)] => Some(flow),
                _ => None,
            },
            _ => None,
        }
        .unwrap_or_else(|| panic!("{source} should retain an empty TextFlow"));
        assert_eq!(text_flow.runs.len(), 1);
        assert!(text_flow.runs[0].text.is_empty());

        let serialized = serialize_tree(&original);
        let round_trip = tree(&serialized);
        assert_eq!(shape(&original.root), shape(&round_trip.root));
    }
}

#[test]
fn empty_zero_run_text_flow_does_not_change_serialized_output() {
    let tree = MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: MathFlow {
            id: caret_doc::NodeId::generate(),
            content: vec![MathContent::Text(TextFlow::default())],
            extra: Default::default(),
        },
        extra: Default::default(),
    };

    assert_eq!(serialize_tree(&tree), "");
}

#[test]
fn text_flow_source_map_accounts_for_escaped_characters_and_empty_text() {
    let escaped_source = r"\text{a\%b}";
    let escaped = crate::parse_projection(escaped_source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("escaped text should project: {error}"));
    let ProjectedMath::Tree(escaped_tree) = escaped.content() else {
        panic!("inline text should produce a tree");
    };
    let Some(MathContent::Text(flow)) = escaped_tree.root.content.first() else {
        panic!("text command should produce a TextFlow");
    };
    assert_eq!(
        flow.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "a%b"
    );
    let escaped_map = escaped
        .source_map()
        .tree()
        .and_then(|tree| tree.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(crate::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("text flow should have an independent sidecar"));
    assert_eq!(
        escaped_map
            .segments()
            .iter()
            .map(|segment| (segment.display(), segment.source().unwrap_or_default()))
            .collect::<Vec<_>>(),
        vec![(0..1, 6..7), (1..2, 7..9), (2..3, 9..10)]
    );

    let collapsed_source = r"\text{a   b}";
    let collapsed = crate::parse_projection(collapsed_source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("collapsed whitespace should project: {error}"));
    let ProjectedMath::Tree(collapsed_tree) = collapsed.content() else {
        panic!("inline text should produce a tree");
    };
    let Some(MathContent::Text(flow)) = collapsed_tree.root.content.first() else {
        panic!("text command should produce a TextFlow");
    };
    assert_eq!(
        flow.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "a b"
    );
    let collapsed_map = collapsed
        .source_map()
        .tree()
        .and_then(|tree| tree.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(crate::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("text flow should have an independent sidecar"));
    assert_eq!(
        collapsed_map
            .segments()
            .iter()
            .map(|segment| (segment.display(), segment.source().unwrap_or_default()))
            .collect::<Vec<_>>(),
        vec![(0..1, 6..7), (1..2, 7..10), (2..3, 10..11)]
    );

    let empty = crate::parse_projection(r"\text{}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("empty text should project: {error}"));
    let ProjectedMath::Tree(empty_tree) = empty.content() else {
        panic!("inline text should produce a tree");
    };
    let Some(MathContent::Text(flow)) = empty_tree.root.content.first() else {
        panic!("empty text command should retain a TextFlow");
    };
    assert!(flow.runs.iter().all(|run| run.text.is_empty()));
    let empty_map = empty
        .source_map()
        .tree()
        .and_then(|tree| tree.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(crate::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("empty text flow should have a sidecar"));
    assert_eq!(empty_map.segments().len(), 1);
    assert_eq!(empty_map.segments()[0].display(), 0..0);
    assert_eq!(empty_map.segments()[0].source(), Some(6..6));
}

#[test]
fn records_source_ranges() {
    let source = r"\frac{a}{bc}";
    let projection = crate::parse_projection(source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("fraction should project: {error}"));
    let first_range = projection
        .source_map()
        .tree()
        .and_then(|tree| {
            tree.content_at(&[
                caret_doc::MathPathStep::Content(0),
                caret_doc::MathPathStep::Operand(1),
                caret_doc::MathPathStep::Content(0),
            ])
        })
        .and_then(crate::MathContentSourceMap::range)
        .unwrap_or_else(|| panic!("fraction denominator should map its first symbol"));
    let second_range = projection
        .source_map()
        .tree()
        .and_then(|tree| {
            tree.content_at(&[
                caret_doc::MathPathStep::Content(0),
                caret_doc::MathPathStep::Operand(1),
                caret_doc::MathPathStep::Content(1),
            ])
        })
        .and_then(crate::MathContentSourceMap::range)
        .unwrap_or_else(|| panic!("fraction denominator should map its second symbol"));
    assert_eq!(&source[first_range], "b");
    assert_eq!(&source[second_range], "c");
}

#[test]
fn top_level_text_font_commands_map_to_textflow_environments() {
    for (source, expected_family) in [(r"\textsf{x}", "sans-serif"), (r"\texttt{x}", "monospace")] {
        let ParsedMath::Tree(tree) = parse(source, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("supported text font command should parse: {error}"))
        else {
            panic!("text font command should produce a math tree");
        };
        let [MathContent::Environment(environment)] = tree.root.content.as_slice() else {
            panic!("text font command should become a root TextFlow environment");
        };
        assert_eq!(environment.kind, caret_doc::InlineEnvironmentKind::Font);
        assert_eq!(
            environment.props.get("fontFamily"),
            Some(&serde_json::Value::String(expected_family.to_owned()))
        );
        assert!(matches!(
            environment.children.as_slice(),
            [MathContent::Text(text)] if text.runs.iter().map(|run| run.text.as_str()).collect::<String>() == "x"
        ));
    }

    for (source, expected_style) in [(r"\mathsf{x}", "sf"), (r"\mathtt{x}", "tt")] {
        let ParsedMath::Tree(tree) = parse(source, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("math alphabet command should parse: {error}"))
        else {
            panic!("math alphabet command should produce a math tree");
        };
        assert!(
            matches!(
                tree.root.content.as_slice(),
                [MathContent::Math(node)]
                    if node.kind == K::Text && node.attributes.get("style").is_some_and(|style| style == expected_style)
            ),
            "unexpected math alphabet tree: {:?}",
            tree.root.content
        );
    }
}

#[test]
fn math_alphabet_and_text_font_commands_roundtrip_with_distinct_semantics() {
    for source in [
        r"\mathsf{}",
        r"\mathtt{}",
        r"\mathbf{}",
        r"\mathit{}",
        r"\mathrm{}",
        r"\frac{\mathsf{}}{\mathtt{x}}",
    ] {
        let ParsedMath::Tree(tree) = parse(source, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("math alphabet command should parse: {error}"))
        else {
            panic!("math alphabet command should produce a math tree");
        };
        let serialized = serialize_tree(&tree);
        assert_eq!(
            serialized, source,
            "math alphabet changed during serialization"
        );
        let ParsedMath::Tree(reparsed) = parse(&serialized, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("serialized math alphabet should parse: {error}"))
        else {
            panic!("serialized math alphabet should produce a math tree");
        };
        assert_eq!(shape_flow(&tree.root), shape_flow(&reparsed.root));
    }

    let source = r"\text{\textsf{a\textrm{b}}}";
    let ParsedMath::Tree(tree) = parse(source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("nested text font commands should parse: {error}"))
    else {
        panic!("text font commands should produce a math tree");
    };
    let [MathContent::Environment(outer)] = tree.root.content.as_slice() else {
        panic!("text group should produce a text font environment");
    };
    let [MathContent::Text(first), MathContent::Environment(serif)] = outer.children.as_slice()
    else {
        panic!(
            "nested text font commands should remain separate environments: {:?}",
            outer.children
        );
    };
    assert_eq!(
        first
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "a"
    );
    assert_eq!(outer.kind, caret_doc::InlineEnvironmentKind::Font);
    assert_eq!(
        outer.props.get("fontFamily"),
        Some(&serde_json::Value::String("sans-serif".to_owned()))
    );
    assert_eq!(
        serif.props.get("fontFamily"),
        Some(&serde_json::Value::String("serif".to_owned()))
    );
    assert_eq!(serialize_tree(&tree), source);
}

#[test]
fn strips_fences_and_rejects_mismatches() {
    assert!(parse("$x$", MathInputKind::Inline).is_ok());
    assert!(parse(r"\[x\]", MathInputKind::Equation).is_ok());
    assert_eq!(
        parse(r"\[x\]", MathInputKind::Inline),
        Err(MathParseError::InputKindMismatch {
            expected: MathInputKind::Inline
        })
    );
    assert!(matches!(
        parse("x", MathInputKind::Align),
        Err(MathParseError::InputKindMismatch { .. })
    ));
}

#[test]
fn incomplete_fence_body_range_stays_editable_from_the_delimiter() {
    assert_eq!(
        input_body_range_for_edit("$", MathInputKind::Inline),
        Ok(0..1)
    );
    assert_eq!(
        input_body_range_for_edit(r"\(x", MathInputKind::Inline),
        Ok(0..3)
    );
    assert_eq!(
        input_body_range_for_edit("x\\frac{", MathInputKind::Inline),
        Ok(0..7)
    );
}

#[test]
fn rejects_numbering() {
    for source in [r"x \label{a}", r"\begin{align}x\end{align}", r"x\tag{1}"] {
        assert!(
            matches!(
                parse(source, MathInputKind::Equation),
                Err(MathParseError::UnsupportedNumbering { .. })
            ),
            "{source}"
        );
    }
}

#[test]
fn enforces_depth_budget() {
    let source = "{".repeat(200) + &"}".repeat(200);
    assert!(matches!(
        parse(&source, MathInputKind::Equation),
        Err(MathParseError::InputTooDeep { .. })
    ));
}

#[test]
fn classifies_partial_input() {
    use MathInputStatus::*;
    assert_eq!(
        classify_input(r"\frac{a}{b}", MathInputKind::Inline),
        Complete
    );
    assert_eq!(
        classify_input(r"\frac{a", MathInputKind::Inline),
        Incomplete
    );
    assert_eq!(classify_input(r"x+\fra", MathInputKind::Inline), Incomplete);
    assert_eq!(classify_input(r"x}", MathInputKind::Inline), Invalid);
    assert!(is_incomplete_input_prefix(r"\sqrt{"));
    assert!(!is_incomplete_input_prefix("x+1"));
}

#[test]
fn splits_align_cells_with_rebased_ranges() {
    let source = r"\[\begin{aligned} a &= b+1 \\ c &= \frac{d}{e} \end{aligned}\]";
    let Ok(projection) = crate::parse_projection(source, MathInputKind::Align) else {
        panic!("应解析为对齐公式");
    };
    let ProjectedMath::Align(align) = projection.content() else {
        panic!("应解析为对齐公式");
    };
    let Some(align_map) = projection.source_map().align() else {
        panic!("Align projection should expose cell ranges");
    };
    assert_eq!(align.rows.len(), 2);
    let Some(first_cell) = align_map.cell_range(0, 1) else {
        panic!("first Align cell should have a source range");
    };
    assert_eq!(&source[first_cell], "= b+1");
    let Some(second_cell) = align_map.cell_range(1, 1) else {
        panic!("second Align cell should have a source range");
    };
    assert_eq!(&source[second_cell.clone()], r"= \frac{d}{e}");
    let fraction_index = align.rows[1].cells[1]
        .root
        .content
        .iter()
        .position(|content| {
            matches!(
                content,
                MathContent::Math(node) if node.kind == K::Fraction
            )
        })
        .unwrap_or_else(|| panic!("fraction node should exist in the second Align cell"));
    let range = align_map
        .cell_tree(1, 1)
        .and_then(|cell| cell.content_at(&[caret_doc::MathPathStep::Content(fraction_index)]))
        .and_then(crate::MathContentSourceMap::range)
        .unwrap_or_else(|| panic!("fraction in the second Align cell should be mapped"));
    assert_eq!(&source[range], r"\frac{d}{e}");
    let regenerated = serialize_align(align);
    let Ok(ParsedMath::Align(again)) = parse(&regenerated, MathInputKind::Align) else {
        panic!("序列化结果应可解析：{regenerated}");
    };
    for (left, right) in align.rows.iter().zip(&again.rows) {
        for (a, b) in left.cells.iter().zip(&right.cells) {
            assert_eq!(shape(&a.root), shape(&b.root));
        }
    }
}

#[test]
fn align_cell_sidecar_uses_full_formula_source_coordinates() {
    let source = r"\[\begin{aligned} x &= \frac{y}{z} \end{aligned}\]";
    let projection = match crate::parse_projection(source, MathInputKind::Align) {
        Ok(projection) => projection,
        Err(error) => panic!("valid aligned formula should project: {error}"),
    };
    let cell = projection
        .source_map()
        .align()
        .and_then(|align| align.cell_tree(0, 1))
        .unwrap_or_else(|| panic!("second align cell should have a tree sidecar"));
    let fraction_range = cell
        .root()
        .content()
        .iter()
        .filter_map(|content| match content {
            crate::MathContentSourceMap::Math { range, .. } => range.range(),
            crate::MathContentSourceMap::Text(_)
            | crate::MathContentSourceMap::Environment { .. }
            | crate::MathContentSourceMap::Other => None,
        })
        .find(|range| &source[range.clone()] == r"\frac{y}{z}")
        .unwrap_or_else(|| panic!("cell should contain a mapped fraction"));
    assert_eq!(&source[fraction_range], r"\frac{y}{z}");
}

#[test]
fn parse_projection_owns_source_and_keeps_tree_projection_free() {
    let source = r"\text{a\%b}";
    let projection = match crate::parse_projection(source, MathInputKind::Inline) {
        Ok(projection) => projection,
        Err(error) => panic!("valid text formula should project: {error}"),
    };
    assert_eq!(projection.source_map().source(), source);
    assert_eq!(projection.source_map().edit_source(), source);
    assert_eq!(projection.source_map().kind(), MathInputKind::Inline);
    assert_eq!(projection.source_map().body_range(), 0..source.len());
    let ProjectedMath::Tree(tree) = projection.content() else {
        panic!("inline math should have a tree projection");
    };
    let (content, source_map) = projection.clone().into_parts();
    let ProjectedMath::Tree(projected_tree) = content else {
        panic!("inline projection should split into its tree and map");
    };
    assert_eq!(projected_tree.root.content.len(), tree.root.content.len());
    assert_eq!(source_map.source(), source);
    assert_eq!(source_map.body_range(), 0..source.len());
    let ParsedMath::Tree(restored) = projection.into_parsed() else {
        panic!("inline projection should restore to a tree");
    };
    let parsed = match parse(source, MathInputKind::Inline) {
        Ok(parsed) => parsed,
        Err(error) => panic!("valid inline source should parse: {error}"),
    };
    let ParsedMath::Tree(parsed) = parsed else {
        panic!("inline input should parse to a tree");
    };
    assert_eq!(shape(&restored.root), shape(&parsed.root));
}

#[test]
fn ordinary_parse_returns_math_trees_without_projection_metadata() {
    let ParsedMath::Tree(tree) = parse(
        r"\frac{\text{\textbf{a\textsf{b}}}}{c}",
        MathInputKind::Inline,
    )
    .unwrap_or_else(|error| panic!("valid formula should parse: {error}")) else {
        panic!("inline input should parse to a tree");
    };
    let bytes =
        serde_json::to_vec(&tree).unwrap_or_else(|error| panic!("tree should encode: {error}"));
    let restored: caret_doc::MathTree = serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("tree should decode: {error}"));
    assert_eq!(tree, restored);

    let ParsedMath::Align(align) = parse(
        r"\begin{aligned}\text{\textbf{a}}&=\text{\texttt{b}}\\c&=d\end{aligned}",
        MathInputKind::Align,
    )
    .unwrap_or_else(|error| panic!("valid alignment should parse: {error}")) else {
        panic!("align input should parse to an alignment");
    };
    let bytes =
        serde_json::to_vec(&align).unwrap_or_else(|error| panic!("align should encode: {error}"));
    let restored: caret_doc::Align = serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("align should decode: {error}"));
    assert_eq!(align, restored);
}

#[test]
fn projection_sidecar_is_separate_from_persistent_tree_and_align() {
    let source = r"\frac{\text{\textbf{a\textsf{b}}}}{c}";
    let projection = crate::parse_projection(source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("valid formula should project: {error}"));
    let tree_map = projection
        .source_map()
        .tree()
        .unwrap_or_else(|| panic!("projection should have a tree map"));
    assert!(tree_map.root().content()[0].range().is_some());
    assert!(math_source_map_has_text_segments(tree_map.root()));
    let ProjectedMath::Tree(projected_tree) = projection.content() else {
        panic!("inline projection should contain a MathTree");
    };
    let persistent_tree = serde_json::to_value(projected_tree)
        .unwrap_or_else(|error| panic!("projected tree should encode: {error}"));
    let ParsedMath::Tree(tree) = projection.into_parsed() else {
        panic!("inline projection should return a tree");
    };
    assert_eq!(
        persistent_tree,
        serde_json::to_value(&tree)
            .unwrap_or_else(|error| panic!("parsed tree should encode: {error}"))
    );
    let restored: caret_doc::MathTree = serde_json::from_slice(
        &serde_json::to_vec(&tree).unwrap_or_else(|error| panic!("tree should encode: {error}")),
    )
    .unwrap_or_else(|error| panic!("tree should decode: {error}"));
    assert_eq!(tree, restored);

    let source = r"\begin{aligned}\text{\textbf{a}}&=\text{\textsf{b}}\\c&=d\end{aligned}";
    let projection = crate::parse_projection(source, MathInputKind::Align)
        .unwrap_or_else(|error| panic!("valid alignment should project: {error}"));
    let align_map = projection
        .source_map()
        .align()
        .unwrap_or_else(|| panic!("Align projection should have a sidecar"));
    assert!(align_map.cell_tree(0, 0).is_some());
    let ProjectedMath::Align(projected_align) = projection.content() else {
        panic!("Align projection should contain an Align");
    };
    let persistent_align = serde_json::to_value(projected_align)
        .unwrap_or_else(|error| panic!("projected Align should encode: {error}"));
    let ParsedMath::Align(align) = projection.into_parsed() else {
        panic!("Align projection should return an Align");
    };
    assert_eq!(
        persistent_align,
        serde_json::to_value(&align)
            .unwrap_or_else(|error| panic!("parsed Align should encode: {error}"))
    );
    let restored: caret_doc::Align = serde_json::from_slice(
        &serde_json::to_vec(&align).unwrap_or_else(|error| panic!("align should encode: {error}")),
    )
    .unwrap_or_else(|error| panic!("align should decode: {error}"));
    assert_eq!(align, restored);
}

#[test]
fn parser_builds_independent_tree_and_align_sidecars_during_lowering() {
    let inline_source = r"\frac{\text{A\%中}}{\textsf{xy}}";
    let projection = crate::parse_projection(inline_source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("valid inline formula should project: {error}"));
    let ProjectedMath::Tree(tree) = projection.content() else {
        panic!("inline formula should produce a tree");
    };
    let tree_map = projection
        .source_map()
        .tree()
        .unwrap_or_else(|| panic!("inline formula should have a tree sidecar"));
    assert_eq!(tree_map.root().content().len(), tree.root.content.len());
    assert!(
        tree_map
            .validates_projection(tree, inline_source, None)
            .is_some()
    );

    let source = r"\text{x}";
    let projection = crate::parse_projection(source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("valid text formula should project: {error}"));
    let ProjectedMath::Tree(unanchored_tree) = projection.content() else {
        panic!("inline text should produce a MathTree");
    };
    assert!(
        projection
            .source_map()
            .tree()
            .unwrap_or_else(|| panic!("inline text should have a tree sidecar"))
            .validates_projection(unanchored_tree, source, None)
            .is_some()
    );

    let align_source =
        r"\begin{aligned}x^2   &=\frac{x}{y}\\中_1   &=\texttt{z}\\&=\text{}\end{aligned}";
    let projection = crate::parse_projection(align_source, MathInputKind::Align)
        .unwrap_or_else(|error| panic!("valid Align should project: {error}"));
    let align_map = projection
        .source_map()
        .align()
        .unwrap_or_else(|| panic!("Align should have a source map"));
    let ProjectedMath::Align(align) = projection.content() else {
        panic!("Align input should produce an Align");
    };
    for (row_index, row) in align.rows.iter().enumerate() {
        for (cell_index, tree) in row.cells.iter().enumerate() {
            let range = align_map
                .cell_range(row_index, cell_index)
                .unwrap_or_else(|| panic!("Align cell should have a source range"));
            let sidecar = align_map
                .cell_tree(row_index, cell_index)
                .unwrap_or_else(|| panic!("Align cell should have an independent sidecar"));
            assert!(
                sidecar
                    .validates_projection(tree, projection.source_map().source(), Some(&range))
                    .is_some()
            );
        }
    }
}

#[test]
fn reconciled_text_split_rebinds_complete_tree_sidecar_coordinates() {
    let source = r"\text{a}\text{bc}";
    let previous = crate::parse_projection(r"\text{ab}\text{c}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("valid previous formula should project: {error}"));
    let ProjectedMath::Tree(previous_tree) = previous.content() else {
        panic!("inline formula should produce a tree");
    };
    let previous_map = previous
        .source_map()
        .tree()
        .unwrap_or_else(|| panic!("previous projection should carry a tree sidecar"));
    let projection = crate::parse_projection(source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("valid edited formula should project: {error}"));
    let (ProjectedMath::Tree(parsed), mut source_map) = projection.into_parts() else {
        panic!("inline formula should produce a tree");
    };
    let parsed_map = source_map
        .tree()
        .cloned()
        .unwrap_or_else(|| panic!("edited projection should carry a tree sidecar"));
    let mapped = parsed_map
        .reconcile_for_tree(&parsed, previous_tree, Some(previous_map), source, None)
        .unwrap_or_else(|| panic!("equivalent adjacent text flows should reconcile"));
    assert!(source_map.bind_tree_sidecar_after_validated_edit(previous_tree, mapped));
    let map = source_map
        .tree()
        .unwrap_or_else(|| panic!("validated edit should install a tree sidecar"));
    let first = map
        .content_at(&[caret_doc::MathPathStep::Content(0)])
        .and_then(crate::MathContentSourceMap::text)
        .and_then(crate::MathTextSourceMap::range)
        .unwrap_or_else(|| panic!("first target text flow should have a source range"));
    let second = map
        .content_at(&[caret_doc::MathPathStep::Content(1)])
        .and_then(crate::MathContentSourceMap::text)
        .and_then(crate::MathTextSourceMap::range)
        .unwrap_or_else(|| panic!("second target text flow should have a source range"));
    assert_eq!(&source[first], r"a}\text{b");
    assert_eq!(&source[second], "c");
}

#[test]
fn reconciled_align_cell_split_rebinds_complete_source_coordinates() {
    let source = r"\begin{aligned}\text{a}\text{bc}&x\end{aligned}";
    let previous = crate::parse_projection(
        r"\begin{aligned}\text{ab}\text{c}&x\end{aligned}",
        MathInputKind::Align,
    )
    .unwrap_or_else(|error| panic!("valid previous Align should project: {error}"));
    let ProjectedMath::Align(previous_align) = previous.content() else {
        panic!("Align input should produce an Align");
    };
    let previous_align_map = previous
        .source_map()
        .align()
        .unwrap_or_else(|| panic!("previous Align should carry a sidecar"));
    let projection = crate::parse_projection(source, MathInputKind::Align)
        .unwrap_or_else(|error| panic!("valid edited Align should project: {error}"));
    let (ProjectedMath::Align(parsed), mut source_map) = projection.into_parts() else {
        panic!("Align input should produce an Align");
    };
    let parsed_align_map = source_map
        .align()
        .cloned()
        .unwrap_or_else(|| panic!("edited Align should carry a sidecar"));
    let mut mapped_cells = Vec::with_capacity(previous_align.rows.len());
    for (row_index, row) in previous_align.rows.iter().enumerate() {
        let mut mapped_row = Vec::with_capacity(row.cells.len());
        for (cell_index, target_tree) in row.cells.iter().enumerate() {
            let parsed_tree = &parsed.rows[row_index].cells[cell_index];
            let scope = parsed_align_map
                .cell_range(row_index, cell_index)
                .unwrap_or_else(|| panic!("edited Align cell should have a source range"));
            let old_cell_map = previous_align_map
                .cell_tree(row_index, cell_index)
                .unwrap_or_else(|| panic!("previous Align cell should have a sidecar"));
            let parsed_cell_map = parsed_align_map
                .cell_tree(row_index, cell_index)
                .unwrap_or_else(|| panic!("edited Align cell should have a sidecar"));
            mapped_row.push(
                parsed_cell_map
                    .reconcile_for_tree(
                        parsed_tree,
                        target_tree,
                        Some(old_cell_map),
                        source,
                        Some(&scope),
                    )
                    .unwrap_or_else(|| panic!("equivalent Align cells should reconcile")),
            );
        }
        mapped_cells.push(mapped_row);
    }
    assert!(source_map.bind_align_sidecars_after_validated_edit(previous_align, mapped_cells));
    let cell_map = source_map
        .align()
        .and_then(|align| align.cell_tree(0, 0))
        .unwrap_or_else(|| panic!("first Align cell should have a sidecar"));
    let first = cell_map
        .content_at(&[caret_doc::MathPathStep::Content(0)])
        .and_then(crate::MathContentSourceMap::text)
        .and_then(crate::MathTextSourceMap::range)
        .unwrap_or_else(|| panic!("first target cell text should have a source range"));
    let second = cell_map
        .content_at(&[caret_doc::MathPathStep::Content(1)])
        .and_then(crate::MathContentSourceMap::text)
        .and_then(crate::MathTextSourceMap::range)
        .unwrap_or_else(|| panic!("second target cell text should have a source range"));
    assert_eq!(&source[first], r"a}\text{b");
    assert_eq!(&source[second], "c");
}

fn math_source_map_has_text_segments(flow: &crate::MathFlowSourceMap) -> bool {
    flow.content()
        .iter()
        .any(math_content_map_has_text_segments)
}

fn math_content_map_has_text_segments(content: &crate::MathContentSourceMap) -> bool {
    match content {
        crate::MathContentSourceMap::Text(text) => !text.segments().is_empty(),
        crate::MathContentSourceMap::Math { operands, .. } => {
            operands.iter().any(math_source_map_has_text_segments)
        }
        crate::MathContentSourceMap::Environment { children, .. } => {
            children.iter().any(math_content_map_has_text_segments)
        }
        crate::MathContentSourceMap::Other => false,
    }
}

#[test]
fn projection_sidecar_maps_escaped_text_by_utf8_display_segments() {
    let source = r"\text{a\%中}";
    let projection = match crate::parse_projection(source, MathInputKind::Inline) {
        Ok(projection) => projection,
        Err(error) => panic!("valid escaped text should project: {error}"),
    };
    let tree_map = projection
        .source_map()
        .tree()
        .unwrap_or_else(|| panic!("tree projection should own its range sidecar"));
    let content_map = tree_map
        .content_at(&[caret_doc::MathPathStep::Content(0)])
        .unwrap_or_else(|| panic!("first text path should resolve in the sidecar"));
    let text_map = content_map
        .text()
        .unwrap_or_else(|| panic!("first content should map a text flow"));

    let segments = text_map.segments();
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[0].display(), 0..1);
    assert_eq!(segments[1].display(), 1..2);
    assert_eq!(segments[2].display(), 2..5);
    let source_segments = segments
        .iter()
        .map(|segment| {
            let range = segment
                .source()
                .unwrap_or_else(|| panic!("parsed text segments must have source ranges"));
            &source[range]
        })
        .collect::<Vec<_>>();
    assert_eq!(source_segments, ["a", r"\%", "中"]);
}

#[test]
fn empty_textflow_keeps_a_zero_length_sidecar_anchor() {
    let projection = match crate::parse_projection(r"\text{}", MathInputKind::Inline) {
        Ok(projection) => projection,
        Err(error) => panic!("empty text formula should project: {error}"),
    };
    let text_map = projection
        .source_map()
        .tree()
        .and_then(|tree| tree.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(crate::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("empty textflow should keep its source anchor"));
    let segment = text_map
        .segments()
        .first()
        .unwrap_or_else(|| panic!("empty textflow should keep an empty mapping segment"));
    assert_eq!(segment.display(), 0..0);
    let source_range = segment
        .source()
        .unwrap_or_else(|| panic!("empty textflow anchor should retain a source range"));
    assert_eq!(&projection.source_map().source()[source_range.clone()], "");
    assert!(source_range.start > 0);
}

#[test]
fn failed_sidecar_rebind_keeps_the_previous_valid_map() {
    let projection = match crate::parse_projection(r"\text{x}", MathInputKind::Inline) {
        Ok(projection) => projection,
        Err(error) => panic!("valid formula should project: {error}"),
    };
    let (ProjectedMath::Tree(valid_tree), mut source_map) = projection.into_parts() else {
        panic!("inline formula should have a tree");
    };
    let previous = source_map
        .tree()
        .cloned()
        .unwrap_or_else(|| panic!("parsed tree should carry a sidecar"));
    let incompatible = crate::parse_projection(r"\text{y}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("valid replacement should project: {error}"));
    let incompatible_map = incompatible
        .source_map()
        .tree()
        .cloned()
        .unwrap_or_else(|| panic!("replacement should have a sidecar"));

    assert!(!source_map.bind_tree_sidecar_after_validated_edit(&valid_tree, incompatible_map));
    assert_eq!(source_map.tree(), Some(&previous));
}

#[test]
fn sidecar_rebind_rejects_semantically_different_tree_without_losing_map() {
    let projection = match crate::parse_projection(r"\text{x}", MathInputKind::Inline) {
        Ok(projection) => projection,
        Err(error) => panic!("valid formula should project: {error}"),
    };
    let (ProjectedMath::Tree(mut changed_tree), mut source_map) = projection.into_parts() else {
        panic!("inline formula should have a tree");
    };
    let previous = source_map
        .tree()
        .cloned()
        .unwrap_or_else(|| panic!("parsed tree should carry a sidecar"));
    let Some(MathContent::Text(text)) = changed_tree.root.content.first_mut() else {
        panic!("text command should project to a TextFlow");
    };
    text.runs[0].text = "y".to_owned();

    let mapped = source_map
        .tree()
        .cloned()
        .unwrap_or_else(|| panic!("parsed formula should have a sidecar"));
    assert!(!source_map.bind_tree_sidecar_after_validated_edit(&changed_tree, mapped));
    assert_eq!(source_map.tree(), Some(&previous));
}

#[test]
fn align_projection_keeps_exact_environment_and_cell_ranges() {
    let source = r"\[\begin{aligned} a &= b+1 \\ c &= d\end{aligned}\]";
    let projection = match crate::parse_projection(source, MathInputKind::Align) {
        Ok(projection) => projection,
        Err(error) => panic!("valid aligned formula should project: {error}"),
    };
    assert_eq!(projection.source_map().source(), source);
    let ProjectedMath::Align(align) = projection.content() else {
        panic!("Align input should have an Align projection");
    };
    let Some(align_map) = projection.source_map().align() else {
        panic!("Align projection should expose its source map");
    };
    let environment = align_map.environment_range();
    assert_eq!(
        &source[environment.clone()],
        r"\begin{aligned} a &= b+1 \\ c &= d\end{aligned}"
    );
    assert_eq!(
        projection.source_map().edit_source(),
        r" a &= b+1 \\ c &= d"
    );
    let second_cell_content_len = align.rows[0].cells[1].root.content.len();
    let Some(cell) = align_map.cell_range(0, 1) else {
        panic!("second first-row cell range should exist");
    };
    assert_eq!(&source[cell], "= b+1");
    let ParsedMath::Align(restored) = projection.into_parsed() else {
        panic!("projected Align should restore to an Align DTO");
    };
    assert_eq!(
        restored.rows[0].cells[1].root.content.len(),
        second_cell_content_len
    );
}

#[test]
fn align_projection_keeps_navigation_trivia_separate_from_cell_content() {
    let source = "\\begin{aligned}a  &b  \\\\  % row comment\nc\\end{aligned}";
    let Ok(projection) = crate::parse_projection(source, MathInputKind::Align) else {
        panic!("trivia around Align cells should parse");
    };
    let Some(align) = projection.source_map().align() else {
        panic!("Align projection should expose cell ranges");
    };
    assert_eq!(&source[align.cell_range(0, 0).unwrap_or_default()], "a");
    assert_eq!(&source[align.cell_navigation_ranges()[0][0].clone()], "a  ");
    assert_eq!(&source[align.cell_navigation_ranges()[0][1].clone()], "b  ");
    assert_eq!(
        &source[align.cell_navigation_ranges()[1][0].clone()],
        "  % row comment\nc"
    );
}

#[test]
fn source_map_rebases_inline_and_align_ranges_for_a_text_edit() {
    let inline_source = r"\text{x}";
    let inline = match crate::parse_projection(inline_source, MathInputKind::Inline) {
        Ok(projection) => projection,
        Err(error) => panic!("valid inline text should project: {error}"),
    };
    let source_range = inline
        .source_map()
        .tree()
        .and_then(|tree| tree.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(crate::MathContentSourceMap::text)
        .and_then(crate::MathTextSourceMap::range)
        .unwrap_or_else(|| panic!("root text flow should have a sidecar range"));
    let updated_inline =
        match inline
            .source_map()
            .replace_source_range(source_range.clone(), "long", None)
        {
            Some(updated) => updated,
            None => panic!("inline body ranges should rebase"),
        };
    assert_eq!(updated_inline.source(), r"\text{long}");
    assert_eq!(
        updated_inline.body_range(),
        0..updated_inline.source().len()
    );
    assert_eq!(updated_inline.edit_source(), updated_inline.source());

    let align_source = r"\[\begin{aligned}a&=b\\c&=d\end{aligned}\]";
    let align = match crate::parse_projection(align_source, MathInputKind::Align) {
        Ok(projection) => projection,
        Err(error) => panic!("valid Align input should project: {error}"),
    };
    let Some(align_map) = align.source_map().align() else {
        panic!("Align projection should have cell ranges");
    };
    let Some(cell) = align_map.cell_range(0, 1) else {
        panic!("the second cell should have a range");
    };
    let source_position = cell.start + 1;
    let updated_align = match align.source_map().replace_source_range(
        source_position..source_position,
        "XYZ",
        Some((0, 1)),
    ) {
        Some(updated) => updated,
        None => panic!("Align ranges should rebase around the edited cell"),
    };
    let Some(updated_map) = updated_align.align() else {
        panic!("updated Align map should remain available");
    };
    let Some(edited_cell) = updated_map.cell_range(0, 1) else {
        panic!("edited cell range should remain");
    };
    let Some(following_cell) = updated_map.cell_range(1, 1) else {
        panic!("following cell range should shift");
    };
    assert_eq!(&updated_align.source()[edited_cell], "=XYZb");
    assert_eq!(&updated_align.source()[following_cell.clone()], "=d");
    let navigation_cell = updated_map.cell_navigation_ranges()[0][1].clone();
    assert!(updated_align.source()[navigation_cell].contains("=XYZb"));
    assert!(
        align
            .source_map()
            .replace_source_range(cell.start..following_cell.end, "invalid", Some((0, 1)))
            .is_none()
    );
    assert!(
        align
            .source_map()
            .replace_source_range(source_position..source_position, "x", Some((9, 9)))
            .is_none()
    );
}

#[test]
fn source_map_rejects_edits_that_exceed_the_math_source_budget() {
    let inline_prefix = r"\text{";
    let inline_suffix = "}";
    let inline_source = format!(
        "{inline_prefix}{}{inline_suffix}",
        "a".repeat(crate::MAX_MATH_SOURCE_BYTES - inline_prefix.len() - inline_suffix.len())
    );
    assert_eq!(inline_source.len(), crate::MAX_MATH_SOURCE_BYTES);
    for kind in [MathInputKind::Inline, MathInputKind::Equation] {
        let projection = match crate::parse_projection(&inline_source, kind) {
            Ok(projection) => projection,
            Err(error) => panic!("source at the budget should project: {error}"),
        };
        assert!(
            projection
                .source_map()
                .replace_edit_source_range(0..0, "x", None)
                .is_none()
        );
    }

    let align_prefix = r"\begin{aligned}\text{";
    let align_suffix = r"}\end{aligned}";
    let align_source = format!(
        "{align_prefix}{}{align_suffix}",
        "a".repeat(crate::MAX_MATH_SOURCE_BYTES - align_prefix.len() - align_suffix.len())
    );
    assert_eq!(align_source.len(), crate::MAX_MATH_SOURCE_BYTES);
    let projection = match crate::parse_projection(&align_source, MathInputKind::Align) {
        Ok(projection) => projection,
        Err(error) => panic!("Align source at the budget should project: {error}"),
    };
    let Some(align) = projection.source_map().align() else {
        panic!("Align projection should have cell ranges");
    };
    let Some(cell) = align.cell_range(0, 0) else {
        panic!("Align cell should have a source range");
    };
    assert!(
        projection
            .source_map()
            .replace_source_range(cell.start..cell.start, "x", Some((0, 0)))
            .is_none()
    );
}

#[test]
fn align_projection_maps_empty_cells_before_delimiters() {
    for source in [
        r"\begin{aligned}\end{aligned}",
        r"\[\begin{aligned}&\\&\end{aligned}\]",
    ] {
        let projection = match crate::parse_projection(source, MathInputKind::Align) {
            Ok(projection) => projection,
            Err(error) => panic!("empty Align input should project: {error}"),
        };
        let ProjectedMath::Align(align) = projection.content() else {
            panic!("empty Align input should have an Align tree");
        };
        let Some(align_map) = projection.source_map().align() else {
            panic!("Align projection should expose its source map");
        };
        let environment = align_map.environment_range();
        let end_marker = source[environment.clone()]
            .find(r"\end")
            .map(|offset| environment.start + offset)
            .unwrap_or_else(|| panic!("Align end marker should be in environment range"));
        assert!(!align.rows.is_empty());
        for row in 0..align.rows.len() {
            for cell in 0..align.rows[row].cells.len() {
                let range = align_map
                    .cell_range(row, cell)
                    .unwrap_or_else(|| panic!("cell range should exist"));
                assert!(range.start <= range.end);
                assert!(range.start >= environment.start);
                assert!(range.end <= end_marker);
                assert_eq!(source[range.clone()].trim(), "");
            }
        }
        if !source.contains('&') {
            assert_eq!(
                align_map.cell_range(0, 0).map(|range| range.start),
                Some(end_marker)
            );
        }
    }
}

#[test]
fn align_projection_keeps_duplicate_unicode_cells_and_noncanonical_source_ranges() {
    let source =
        "\\[\\begin { aligned } α &= 同文 % & comment\n \\\\ β &= 同文 \\end {aligned }\\]";
    let projection = match crate::parse_projection(source, MathInputKind::Align) {
        Ok(projection) => projection,
        Err(error) => panic!("valid Unicode Align source should project: {error}"),
    };
    let Some(align_map) = projection.source_map().align() else {
        panic!("Align projection should expose its source map");
    };
    let first = align_map
        .cell_range(0, 1)
        .unwrap_or_else(|| panic!("first duplicate cell should have a range"));
    let second = align_map
        .cell_range(1, 1)
        .unwrap_or_else(|| panic!("second duplicate cell should have a range"));
    assert_eq!(&source[first.clone()], "= 同文 % & comment");
    assert_eq!(&source[second.clone()], "= 同文");
    assert_ne!(first, second);
}

#[test]
fn align_projection_retains_trailing_whitespace_and_comments_in_edit_view() {
    for (source, expected) in [
        (r"\begin{aligned} \end{aligned}", " "),
        (r"\begin{aligned}x\\  \end{aligned}", r"x\\  "),
        (
            "\\[\\begin{aligned}x  % trailing comment\n\\end{aligned}\\]",
            "x  % trailing comment\n",
        ),
    ] {
        let projection = match crate::parse_projection(source, MathInputKind::Align) {
            Ok(projection) => projection,
            Err(error) => panic!("valid Align trivia should project: {error}"),
        };
        assert_eq!(projection.source_map().edit_source(), expected);
    }
}

#[test]
fn parse_projection_preserves_parser_errors() {
    let expected = match parse("x}", MathInputKind::Inline) {
        Ok(_) => panic!("unmatched closing brace should fail"),
        Err(error) => error,
    };
    let actual = match crate::parse_projection("x}", MathInputKind::Inline) {
        Ok(_) => panic!("unmatched closing brace should fail"),
        Err(error) => error,
    };
    assert_eq!(actual, expected);

    let oversized = "x".repeat(crate::MAX_MATH_SOURCE_BYTES + 1);
    let expected = match parse(&oversized, MathInputKind::Inline) {
        Ok(_) => panic!("oversized source should fail"),
        Err(error) => error,
    };
    let actual = match crate::parse_projection(&oversized, MathInputKind::Inline) {
        Ok(_) => panic!("oversized projected source should fail"),
        Err(error) => error,
    };
    assert_eq!(actual, expected);
}

#[test]
fn empty_align_keeps_an_editable_placeholder_cell() {
    let source = r"\begin{aligned}\end{aligned}";
    let Ok(ParsedMath::Align(align)) = parse(source, MathInputKind::Align) else {
        panic!("empty aligned environment should parse");
    };
    assert_eq!(align.rows.len(), 1);
    assert_eq!(align.rows[0].cells.len(), 1);
    assert!(
        matches!(align.rows[0].cells[0].root.content.as_slice(), [MathContent::Math(node)] if node.kind == K::Placeholder)
    );

    let serialized = serialize_align(&align);
    let Ok(ParsedMath::Align(round_trip)) = parse(&serialized, MathInputKind::Align) else {
        panic!("serialized empty aligned environment should parse");
    };
    assert_eq!(round_trip.rows.len(), 1);
    assert_eq!(round_trip.rows[0].cells.len(), 1);
}

#[test]
fn align_preserves_a_trailing_empty_row_after_row_break() {
    let source = "\\begin{aligned}a\\\\ % before\n % after\n\\end{aligned}";
    let projection = crate::parse_projection(source, MathInputKind::Align)
        .unwrap_or_else(|error| panic!("valid trailing empty Align row should parse: {error}"));
    let (ProjectedMath::Align(align), source_map) = projection.into_parts() else {
        panic!("Align input should produce an Align tree");
    };

    assert_eq!(align.rows.len(), 2);
    assert!(matches!(
        align.rows[1].cells[0].root.content.as_slice(),
        [MathContent::Math(node)] if node.kind == K::Placeholder
    ));
    let align_map = source_map
        .align()
        .unwrap_or_else(|| panic!("Align projection should retain its source map"));
    let row_break = align_map
        .row_break_range_after(0)
        .unwrap_or_else(|| panic!("first row should retain its explicit break range"));
    assert_eq!(&source[row_break], r"\\");
    assert_eq!(align_map.row_break_range_after(1), None);
    let semantic = align_map
        .cell_range(1, 0)
        .unwrap_or_else(|| panic!("trailing empty cell should have a semantic range"));
    assert_eq!(&source[semantic.clone()], "");
    let navigation = align_map.cell_navigation_ranges()[1][0].clone();
    assert!(navigation.end > navigation.start);
    assert!(source[navigation].contains("% before"));

    let serialized = crate::serialize_align(&align);
    let reparsed = crate::parse(&serialized, MathInputKind::Align)
        .unwrap_or_else(|error| panic!("serialized trailing empty row should parse: {error}"));
    let ParsedMath::Align(reparsed) = reparsed else {
        panic!("serialized Align input should remain Align");
    };
    assert_eq!(reparsed.rows.len(), 2);
}

#[test]
fn matrix_trailing_row_break_keeps_the_existing_matrix_shape() {
    for (source, without_trailing_break) in [
        (
            r"\begin{matrix}a\\\end{matrix}",
            r"\begin{matrix}a\end{matrix}",
        ),
        (
            r"\begin{array}{cc}a&b\\\end{array}",
            r"\begin{array}{cc}a&b\end{array}",
        ),
        (
            r"\begin{aligned}a\\\end{aligned}",
            r"\begin{aligned}a\end{aligned}",
        ),
    ] {
        let projection = crate::parse_projection(source, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("matrix trailing row break should parse: {error}"));
        let baseline = crate::parse_projection(without_trailing_break, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("matrix without trailing break should parse: {error}"));
        let (ProjectedMath::Tree(tree), _) = projection.into_parts() else {
            panic!("matrix source should lower to a math tree");
        };
        let (ProjectedMath::Tree(baseline), _) = baseline.into_parts() else {
            panic!("baseline source should lower to a math tree");
        };
        assert_eq!(
            crate::serialize_tree(&tree),
            crate::serialize_tree(&baseline)
        );
    }
}

const ROUND_TRIP_CASES: &[&str] = &[
    "x",
    "ab+12",
    r"\alpha+\beta=\gamma",
    r"\frac{a+b}{c}",
    r"\dfrac{1}{2}",
    r"\sqrt[3]{x}",
    r"\sqrt{x^2+y^2}",
    r"x_i^2",
    r"f'(x)",
    r"f''^{2}",
    r"\sum_{i=1}^{n} i",
    r"\prod_{k} a_k",
    r"\oint_C F",
    r"\lim_{x\to 0} \frac{\sin x}{x}",
    r"\operatorname{rank} A",
    r"\operatorname*{argmax}_x f",
    r"\left( \frac{a}{b} \right]",
    r"\left. x \right\|",
    r"\bigl( x \bigr)",
    r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
    r"\begin{array}{c|l} 1 & 2 \\ \hline 3 & 4 \end{array}",
    r"\begin{cases} 1 & x>0 \\ 0 & \text{otherwise} \end{cases}",
    r"\sum_{\substack{i<n \\ j<m}} a",
    r"\overset{!}{=}",
    r"\underset{x}{\max}",
    r"\cancelto{0}{x}",
    r"\hat{x}+\widetilde{abc}+\mathring{A}",
    r"\not{=}",
    r"a\,b\:c\;d\!e\quad f\qquad g\ h~i",
    r"\hspace{0.25em}",
    r"\hspace{3mu}",
    r"\text{a \% b\ c~d \textbf{e}}",
    r"\text{a\textbf{b\textmd{c}}}",
    r"\text{a\textit{b\textup{c}}}",
    r"\text{a\textchar{20}\textchar{20}b\textchar{5C}\textchar{7E}\textchar{5E}}",
    r"\text{first\newline{}second}",
    r"\mathbf{x}+\mathbb{R}+\boldsymbol{\alpha}",
    r"\textcolor{blue}{x}",
    r"\textcolor[HTML]{12AB34}{y}",
    r"{\color{red} x+y}",
    r"\colorbox{yellow}{note}",
    r"\fcolorbox{red}{white}{box}",
    r"\phantom{x}\vphantom{y}",
    r"\rule[-0.5em]{0em}{2em}",
    r"{}^{14}C",
    r"\{ x \} \% \#",
    r"\begin{aligned} a &= b \\ &= c \end{aligned}",
    r"\begin{matrix} a \\ \intertext{and} b \end{matrix}",
];

#[test]
fn serializer_round_trips_parser_output() {
    for source in ROUND_TRIP_CASES {
        let original = tree(source);
        let regenerated = serialize_tree(&original);
        let again = match parse(&regenerated, MathInputKind::Equation) {
            Ok(ParsedMath::Tree(tree)) => tree,
            other => panic!("{source} → {regenerated}: {other:?}"),
        };
        assert_eq!(
            shape(&original.root),
            shape(&again.root),
            "{source} → {regenerated}"
        );
        // 规范形式是不动点。
        assert_eq!(serialize_tree(&again), regenerated, "{source}");
    }
}

#[test]
fn document_softbreaks_round_trip_through_equation_and_align_tex() {
    fn softbreak_tree() -> caret_doc::MathTree {
        caret_doc::MathTree {
            schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
            root: MathFlow {
                id: NodeId::generate(),
                content: vec![MathContent::Environment(InlineEnvironment {
                    id: NodeId::generate(),
                    kind: InlineEnvironmentKind::Font,
                    definition_id: None,
                    props: BTreeMap::new(),
                    children: vec![
                        MathContent::SoftBreak(caret_doc::SoftBreak::default()),
                        MathContent::Text(TextFlow {
                            runs: vec![Run {
                                text: "first".to_owned(),
                                language: None,
                                props: BTreeMap::new(),
                                extra: BTreeMap::new(),
                            }],
                            extra: BTreeMap::new(),
                        }),
                        MathContent::SoftBreak(caret_doc::SoftBreak::default()),
                        MathContent::SoftBreak(caret_doc::SoftBreak::default()),
                        MathContent::Text(TextFlow {
                            runs: vec![Run {
                                text: "second".to_owned(),
                                language: None,
                                props: BTreeMap::new(),
                                extra: BTreeMap::new(),
                            }],
                            extra: BTreeMap::new(),
                        }),
                        MathContent::SoftBreak(caret_doc::SoftBreak::default()),
                    ],
                    extra: BTreeMap::new(),
                })],
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        }
    }

    let equation = softbreak_tree();
    let equation_source = serialize_tree(&equation);
    assert!(equation_source.contains(r"\newline{}"));
    let ParsedMath::Tree(reparsed_equation) = parse(&equation_source, MathInputKind::Equation)
        .unwrap_or_else(|error| panic!("softbreak equation should parse: {error}"))
    else {
        panic!("equation parser should produce a MathTree");
    };
    assert_eq!(shape(&equation.root), shape(&reparsed_equation.root));

    let align = caret_doc::Align {
        id: NodeId::generate(),
        rows: vec![caret_doc::MathAlignRow {
            id: NodeId::generate(),
            cells: vec![softbreak_tree()],
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    };
    let align_source = serialize_align(&align);
    assert!(align_source.contains(r"\newline{}"));
    let ParsedMath::Align(reparsed_align) = parse(&align_source, MathInputKind::Align)
        .unwrap_or_else(|error| panic!("softbreak Align should parse: {error}"))
    else {
        panic!("Align parser should produce an Align model");
    };
    assert_eq!(reparsed_align.rows.len(), 1);
    assert_eq!(
        shape(&align.rows[0].cells[0].root),
        shape(&reparsed_align.rows[0].cells[0].root)
    );
}

#[test]
fn math_font_commands_remain_math_nodes() {
    let plain = tree("x");
    let bold = tree(r"\mathbf{x}");
    assert_ne!(shape(&plain.root), shape(&bold.root));
    assert_eq!(serialize_tree(&bold), r"\mathbf{x}");
}

#[test]
fn serializes_edited_trees() {
    let mut tree = tree(r"\frac{a}{b}");
    root_node_mut(&mut tree).children.swap(0, 1);
    assert_eq!(serialize_tree(&tree), r"\frac{b}{a}");
    let mut tree = self::tree("x^2");
    let root = root_node_mut(&mut tree);
    let Some(MathContent::Math(base)) = root.children[0].content.first_mut() else {
        panic!("expected base node");
    };
    base.text = Some("ab".to_owned());
    assert_eq!(serialize_tree(&tree), "{ab}^{2}");
    assert_eq!(K::Superscript, root_node_mut(&mut tree).kind);
}
