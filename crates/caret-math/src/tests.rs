//! 解析器与序列化器的行为测试。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{MathContent, MathFlow, MathNode, MathNodeKind as K};

use crate::{
    MathInputKind, MathInputStatus, MathParseError, ParsedMath, ProjectedMath, classify_input,
    is_incomplete_input_prefix, parse, serialize_align, serialize_tree,
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
    match parse(source, MathInputKind::Equation) {
        Ok(ParsedMath::Tree(tree)) => tree,
        other => panic!("{source}: {other:?}"),
    }
}

fn root_shape(source: &str) -> String {
    shape(&tree(source).root)
}

#[test]
fn coalesces_literal_ordinary_characters() {
    assert_eq!(
        root_shape("ab12+c"),
        r#"Symbol("ab12"){"atomClass": "ord"}, Symbol("+"){"atomClass": "bin"}, Symbol("c"){"atomClass": "ord"}"#
    );
    // 上标只作用于紧邻的字符，因此 `x` 不与前面的 `a` 合并。
    assert_eq!(
        root_shape("ax^2"),
        r#"Symbol("a"){"atomClass": "ord"}, Superscript[Symbol("x"){"atomClass": "ord"}, Symbol("2"){"atomClass": "ord"}]"#
    );
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
fn text_flow_source_map_accounts_for_escaped_characters_and_empty_text() {
    let escaped = tree(r"\text{a\%b}");
    let Some(MathContent::Text(flow)) = escaped.root.content.first() else {
        panic!("text command should produce a TextFlow");
    };
    assert_eq!(
        flow.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "a%b"
    );
    assert_eq!(
        flow.source_map
            .iter()
            .map(|entry| (
                entry.text_start,
                entry.text_end,
                entry.source_range.start,
                entry.source_range.end,
            ))
            .collect::<Vec<_>>(),
        vec![(0, 1, 6, 7), (1, 2, 7, 9), (2, 3, 9, 10)]
    );
    let mut invalid_gap = escaped.clone();
    let Some(MathContent::Text(flow)) = invalid_gap.root.content.first_mut() else {
        panic!("text command should produce a TextFlow");
    };
    flow.source_map[1].text_start = 2;
    assert!(invalid_gap.validate().is_ok());

    let collapsed = tree(r"\text{a   b}");
    let Some(MathContent::Text(flow)) = collapsed.root.content.first() else {
        panic!("text command should produce a TextFlow");
    };
    assert_eq!(
        flow.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "a b"
    );
    assert_eq!(
        flow.source_map
            .iter()
            .map(|entry| (
                entry.text_start,
                entry.text_end,
                entry.source_range.start,
                entry.source_range.end,
            ))
            .collect::<Vec<_>>(),
        vec![(0, 1, 6, 7), (1, 2, 7, 10), (2, 3, 10, 11)]
    );

    let mut invalid_boundary = escaped.clone();
    let Some(MathContent::Text(flow)) = invalid_boundary.root.content.first_mut() else {
        panic!("text command should produce a TextFlow");
    };
    flow.source_map[0].text_end = 2;
    assert!(invalid_boundary.validate().is_ok());
    let mut invalid_utf8_boundary = tree(r"\text{é}");
    let Some(MathContent::Text(flow)) = invalid_utf8_boundary.root.content.first_mut() else {
        panic!("text command should produce a TextFlow");
    };
    flow.source_map[0].text_start = 1;
    assert!(invalid_utf8_boundary.validate().is_ok());

    let empty = tree(r"\text{}");
    let Some(MathContent::Text(flow)) = empty.root.content.first() else {
        panic!("empty text command should retain a TextFlow");
    };
    assert_eq!(flow.source_map.len(), 1);
    assert_eq!(flow.source_map[0].text_start, 0);
    assert_eq!(flow.source_map[0].text_end, 0);
    assert_eq!(flow.source_map[0].source_range.start, 6);
    assert_eq!(flow.source_map[0].source_range.end, 6);
}

#[test]
fn records_source_ranges() {
    let tree = tree(r"\frac{a}{bc}");
    let Some(MathContent::Math(fraction)) = tree.root.content.first() else {
        panic!("expected fraction node");
    };
    let range = fraction.children[1]
        .source_range
        .as_ref()
        .unwrap_or_else(|| panic!("缺少源范围"));
    assert_eq!(&tree.source[range.start as usize..range.end as usize], "bc");
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
    let Ok(ParsedMath::Align(align)) = parse(source, MathInputKind::Align) else {
        panic!("应解析为对齐公式");
    };
    assert_eq!(align.rows.len(), 2);
    assert_eq!(align.rows[0].cells[1].source, "= b+1");
    let cell = &align.rows[1].cells[1];
    assert_eq!(cell.source, r"= \frac{d}{e}");
    let fraction = cell
        .root
        .content
        .iter()
        .find_map(|content| match content {
            MathContent::Math(node) if node.kind == K::Fraction => Some(node),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected fraction node"));
    let range = fraction
        .source_range
        .as_ref()
        .unwrap_or_else(|| panic!("缺少源范围"));
    assert_eq!(
        &cell.source[range.start as usize..range.end as usize],
        r"\frac{d}{e}"
    );
    let regenerated = serialize_align(&align);
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
    assert!(tree.source.is_empty());
    let (content, source_map) = projection.clone().into_parts();
    let ProjectedMath::Tree(projected_tree) = content else {
        panic!("inline projection should split into its tree and map");
    };
    assert_eq!(projected_tree.root.content.len(), tree.root.content.len());
    assert_eq!(source_map.source(), source);
    assert_eq!(source_map.body_range(), 0..source.len());
    let ParsedMath::Tree(restored) = projection.into_legacy_parsed() else {
        panic!("inline projection should restore to a tree");
    };
    let parsed = match parse(source, MathInputKind::Inline) {
        Ok(parsed) => parsed,
        Err(error) => panic!("valid inline source should parse: {error}"),
    };
    let ParsedMath::Tree(parsed) = parsed else {
        panic!("inline input should parse to a tree");
    };
    assert_eq!(restored.source, source);
    assert_eq!(shape(&restored.root), shape(&parsed.root));
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
    assert!(align.source.is_empty());
    let Some(cell) = align_map.cell_range(0, 1) else {
        panic!("second first-row cell range should exist");
    };
    assert_eq!(&source[cell], "= b+1");
    let ParsedMath::Align(restored) = projection.into_legacy_parsed() else {
        panic!("projected Align should restore to an Align DTO");
    };
    assert_eq!(restored.source, source[environment].to_owned());
    assert_eq!(restored.rows[0].cells[1].source, "= b+1");
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
    assert_eq!(align.rows[0].cells[0].source, "");
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
fn math_font_commands_remain_math_nodes() {
    let plain = tree("x");
    let bold = tree(r"\mathbf{x}");
    assert_ne!(shape(&plain.root), shape(&bold.root));
    assert_eq!(serialize_tree(&bold), r"\mathbf{x}");
}

#[test]
fn serializes_combined_text_run_styles_as_parseable_tex() {
    let mut math_tree = self::tree(r"\text{x}");
    let Some(MathContent::Text(text)) = math_tree.root.content.first_mut() else {
        panic!("text command should produce a TextFlow");
    };
    let Some(run) = text.runs.first_mut() else {
        panic!("text command should produce a Run");
    };
    run.props
        .insert("bold".to_owned(), serde_json::Value::Bool(true));
    run.props
        .insert("italic".to_owned(), serde_json::Value::Bool(true));

    let serialized = serialize_tree(&math_tree);
    assert_eq!(serialized, r"\text{\textbf{\textit{x}}}");
    let reparsed = self::tree(&serialized);
    assert_eq!(serialize_tree(&reparsed), serialized);
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
