//! 解析器与序列化器的行为测试。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{MathNode, MathNodeKind as K};

use crate::{
    MathInputKind, MathInputStatus, MathParseError, ParsedMath, classify_input,
    is_incomplete_input_prefix, parse, serialize_align, serialize_tree,
};

/// 去掉 ID 和源范围后的结构描述，用于比较往返结果。
fn shape(node: &MathNode) -> String {
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
                .map(shape)
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push(']');
    }
    out
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
        r#"Row[Symbol("ab12"){"atomClass": "ord"}, Symbol("+"){"atomClass": "bin"}, Symbol("c"){"atomClass": "ord"}]"#
    );
    // 上标只作用于紧邻的字符，因此 `x` 不与前面的 `a` 合并。
    assert_eq!(
        root_shape("ax^2"),
        r#"Row[Symbol("a"){"atomClass": "ord"}, Superscript[Symbol("x"){"atomClass": "ord"}, Symbol("2"){"atomClass": "ord"}]]"#
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
        r#"Delimited{"left": "char:(", "right": "named:}"}[Row[Symbol("x"){"atomClass": "ord"}]]"#
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
        r#"Text{"style": "text"}[TextContent("if"), TextSpace("space"), Symbol("x"){"atomClass": "ord"}, TextSpace("space"), TextContent("ok")]"#
    );
}

#[test]
fn records_source_ranges() {
    let tree = tree(r"\frac{a}{bc}");
    let range = tree.root.children[1]
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
    let fraction = &cell.root.children[1];
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
fn empty_align_keeps_an_editable_placeholder_cell() {
    let source = r"\begin{aligned}\end{aligned}";
    let Ok(ParsedMath::Align(align)) = parse(source, MathInputKind::Align) else {
        panic!("empty aligned environment should parse");
    };
    assert_eq!(align.rows.len(), 1);
    assert_eq!(align.rows[0].cells.len(), 1);
    assert_eq!(align.rows[0].cells[0].source, "");
    assert_eq!(align.rows[0].cells[0].root.kind, K::Placeholder);

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
fn serializes_edited_trees() {
    let mut tree = tree(r"\frac{a}{b}");
    tree.root.children.swap(0, 1);
    assert_eq!(serialize_tree(&tree), r"\frac{b}{a}");
    let mut tree = self::tree("x^2");
    tree.root.children[0].text = Some("ab".to_owned());
    assert_eq!(serialize_tree(&tree), "{ab}^{2}");
    assert_eq!(K::Superscript, tree.root.kind);
}
