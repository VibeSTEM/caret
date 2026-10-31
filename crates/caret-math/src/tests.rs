//! 验证公式解析适配、中间树转换与预算边界。

use super::math_syntax::{AstBudgetError, check_ast_budget};
use super::{
    MAX_MATH_NESTING_DEPTH, MAX_MATH_SOURCE_BYTES, MAX_MATH_SYNTAX_NODES, MathInputKind,
    MathInputStatus, MathParseError, MathSyntaxKind, MathSyntaxNode, classify_input,
    is_incomplete_input_prefix, parse,
};

#[test]
fn incomplete_input_prefix_detection_only_tracks_open_latex_boundaries() {
    for source in ["\\", r"\alph", r"\frac{a}{", r"x + \sqrt{"] {
        assert!(
            is_incomplete_input_prefix(source),
            "expected incomplete: {source}"
        );
    }
    for source in ["x", r"\{x\}", r"\alpha ", r"x + y"] {
        assert!(
            !is_incomplete_input_prefix(source),
            "unexpected incomplete: {source}"
        );
    }
    assert_eq!(
        classify_input(r"\frac{x}", MathInputKind::Inline),
        MathInputStatus::Incomplete
    );
    assert_eq!(
        classify_input(r"\unknown{x}", MathInputKind::Inline),
        MathInputStatus::Invalid
    );
    assert_eq!(
        classify_input(r"\unknown{", MathInputKind::Inline),
        MathInputStatus::Invalid
    );
    assert_eq!(
        classify_input("x}{", MathInputKind::Inline),
        MathInputStatus::Invalid
    );
    assert_eq!(
        classify_input(r"\unknown{x}\alpha", MathInputKind::Inline),
        MathInputStatus::Invalid
    );
    assert_eq!(
        classify_input(r"x}\alpha", MathInputKind::Inline),
        MathInputStatus::Invalid
    );
    assert_eq!(
        classify_input(r"\unknown{x}\", MathInputKind::Inline),
        MathInputStatus::Invalid
    );
    assert_eq!(
        classify_input("x}\\", MathInputKind::Inline),
        MathInputStatus::Invalid
    );
    assert_eq!(
        classify_input("\\", MathInputKind::Inline),
        MathInputStatus::Incomplete
    );
    assert_eq!(
        classify_input(r"\frac{a}{", MathInputKind::Inline),
        MathInputStatus::Incomplete
    );
    assert_eq!(
        classify_input(r"\{x\}", MathInputKind::Inline),
        MathInputStatus::Complete
    );
    assert_eq!(
        classify_input(r"\frac{x}{y}", MathInputKind::Inline),
        MathInputStatus::Complete
    );
    assert_eq!(
        classify_input(r"\left(x", MathInputKind::Inline),
        MathInputStatus::Incomplete
    );
    assert_eq!(
        classify_input(r"\leftarrow\leftarrow\right)", MathInputKind::Inline),
        MathInputStatus::Invalid
    );
    assert_eq!(
        classify_input(r"\right)\left(x", MathInputKind::Inline),
        MathInputStatus::Invalid
    );
}

#[test]
fn parses_inline_and_display_math_with_the_locked_depth_budget() {
    let inline = parse(r"$x^2 + 1$", MathInputKind::Inline);
    assert!(inline.is_ok());

    let aligned = parse(
        r"\[\begin{aligned}a &= b \\ c &= d\end{aligned}\]",
        MathInputKind::Align,
    );
    assert!(aligned.is_ok());

    let too_deep = format!(
        "{}x{}",
        r"\frac{1}{".repeat(MAX_MATH_NESTING_DEPTH),
        "}".repeat(MAX_MATH_NESTING_DEPTH)
    );
    assert!(matches!(
        parse(&too_deep, MathInputKind::Equation),
        Err(MathParseError::Parse { source, .. }) if source == too_deep
    ));
}

#[test]
fn parser_errors_keep_source_and_exact_whole_formula_byte_range() {
    let source = r"x + \unknown";
    let Err(MathParseError::Parse {
        source: preserved,
        source_range,
        ..
    }) = parse(source, MathInputKind::Inline)
    else {
        panic!("unknown command must fail parsing");
    };
    assert_eq!(preserved, source);
    assert_eq!(source_range, 0..source.len());
}

#[test]
fn rejects_numbering_semantics_and_overlong_source_without_loss() {
    let source = r"x \label{eq:x}";
    assert!(matches!(
        parse(source, MathInputKind::Equation),
        Err(MathParseError::UnsupportedNumbering {
            source: preserved,
            command,
            ..
        }) if preserved == source && command == "label"
    ));

    let source = "x".repeat(MAX_MATH_SOURCE_BYTES + 1);
    assert!(matches!(
        parse(&source, MathInputKind::Inline),
        Err(MathParseError::InputTooLong { source: preserved, .. }) if preserved == source
    ));
}

#[test]
fn rejects_automatically_numbered_environments_and_nested_references() {
    for source in [
        r"\begin{equation}x\end{equation}",
        r"\begin{align}x &= y\end{align}",
        r"\text{$\ref{eq:x}$}",
    ] {
        assert!(matches!(
            parse(source, MathInputKind::Equation),
            Err(MathParseError::UnsupportedNumbering { source: preserved, .. })
                if preserved == source
        ));
    }

    assert!(
        parse(
            r"\[\begin{aligned}x &= y\end{aligned}\]",
            MathInputKind::Align
        )
        .is_ok()
    );
    assert!(matches!(
        parse("x", MathInputKind::Align),
        Err(MathParseError::InputKindMismatch { .. })
    ));
    let aligned = r"\begin{aligned}a &= b \\ c &= d\end{aligned}";
    assert!(matches!(
        parse(aligned, MathInputKind::Inline),
        Err(MathParseError::InputKindMismatch { .. })
    ));
    assert!(matches!(
        parse(aligned, MathInputKind::Equation),
        Err(MathParseError::InputKindMismatch { .. })
    ));
}

#[test]
fn explicit_math_fences_must_match_requested_input_kind() {
    for (source, kind) in [
        ("$$x$$", MathInputKind::Inline),
        (r"\[x\]", MathInputKind::Inline),
        ("$x$", MathInputKind::Equation),
        (r"\(x\)", MathInputKind::Equation),
        (
            r"\(\begin{aligned}x &= y\end{aligned}\)",
            MathInputKind::Align,
        ),
    ] {
        assert!(matches!(
            parse(source, kind),
            Err(MathParseError::InputKindMismatch { source: preserved, .. })
                if preserved == source
        ));
    }

    assert!(parse("$$x$$", MathInputKind::Equation).is_ok());
    assert!(
        parse(
            r"\[\begin{aligned}x &= y\end{aligned}\]",
            MathInputKind::Align
        )
        .is_ok()
    );
    assert!(parse("$x$", MathInputKind::Inline).is_ok());
    assert!(parse(r"\(x\)", MathInputKind::Inline).is_ok());
    assert!(parse("x", MathInputKind::Equation).is_ok());
}

#[test]
fn normalized_range_maps_trimmed_unicode_input_to_utf8_bytes() {
    let source = "  α + x  ";
    let Ok(parsed) = parse(source, MathInputKind::Inline) else {
        panic!("input should parse");
    };
    assert_eq!(&source[parsed.normalized_range()], "α + x");
    assert_eq!(parsed.source_range(), 0..source.len());
}

#[test]
fn converts_parser_nodes_into_backend_neutral_tree() {
    let Ok(parsed) = parse(r"\frac{a}{\sqrt[3]{x}}", MathInputKind::Inline) else {
        panic!("formula should parse");
    };
    let Ok(tree) = parsed.syntax_tree() else {
        panic!("formula should convert");
    };
    assert_eq!(tree.input_kind, MathInputKind::Inline);
    assert_eq!(tree.root.kind, MathSyntaxKind::Fraction);
    assert_eq!(
        tree.root.attributes.get("style").map(String::as_str),
        Some("frac")
    );
    assert_eq!(tree.root.children[0].kind, MathSyntaxKind::Symbol);
    let radical = &tree.root.children[1];
    assert_eq!(radical.kind, MathSyntaxKind::Radical);
    assert_eq!(radical.children[0].kind, MathSyntaxKind::Symbol);
    assert_eq!(radical.children[1].kind, MathSyntaxKind::Symbol);
}

#[test]
fn retains_aligned_rows_and_cell_boundaries() {
    let Ok(parsed) = parse(
        r"\begin{aligned}a &= b \\ c &= d\end{aligned}",
        MathInputKind::Align,
    ) else {
        panic!("aligned formula should parse");
    };
    let Ok(tree) = parsed.syntax_tree() else {
        panic!("aligned formula should convert");
    };
    assert_eq!(tree.root.kind, MathSyntaxKind::Matrix);
    assert_eq!(
        tree.root.attributes.get("style").map(String::as_str),
        Some("aligned")
    );
    assert_eq!(tree.root.children.len(), 2);
    assert!(
        tree.root
            .children
            .iter()
            .all(|row| row.kind == MathSyntaxKind::Row)
    );
    assert!(tree.root.children.iter().all(|row| row.children.len() == 2));
}

#[test]
fn retains_text_spaces_and_nested_styles() {
    let Ok(parsed) = parse(r"\text{hello \textbf{world}}", MathInputKind::Inline) else {
        panic!("text formula should parse");
    };
    let Ok(tree) = parsed.syntax_tree() else {
        panic!("text formula should convert");
    };
    assert_eq!(tree.root.kind, MathSyntaxKind::Text);
    assert!(
        tree.root
            .children
            .iter()
            .any(|child| child.kind == MathSyntaxKind::TextSpace)
    );
    let Some(nested) = tree
        .root
        .children
        .iter()
        .find(|child| child.kind == MathSyntaxKind::TextStyle)
    else {
        panic!("nested text style should be retained");
    };
    assert_eq!(
        nested.attributes.get("style").map(String::as_str),
        Some("bf")
    );
    assert_eq!(nested.children[0].kind, MathSyntaxKind::TextContent);
}

#[test]
fn retains_explicit_math_spacing_and_subscript_superscript() {
    let Ok(parsed) = parse(r"x_i^2\,y", MathInputKind::Inline) else {
        panic!("formula should parse");
    };
    let Ok(tree) = parsed.syntax_tree() else {
        panic!("formula should convert");
    };
    assert_eq!(tree.root.kind, MathSyntaxKind::Row);
    assert_eq!(tree.root.children[0].kind, MathSyntaxKind::SubSup);
    assert_eq!(tree.root.children[1].kind, MathSyntaxKind::Space);
    assert_eq!(
        tree.root.children[1]
            .attributes
            .get("spaceKind")
            .map(String::as_str),
        Some("thin")
    );
}

#[test]
fn rejects_deep_ast_created_by_repeated_script_application() {
    let source = format!("x{}", "'^1".repeat(MAX_MATH_NESTING_DEPTH + 1));
    assert!(matches!(
        parse(&source, MathInputKind::Inline),
        Err(MathParseError::InputTooDeep {
            source: preserved,
            limit: MAX_MATH_NESTING_DEPTH,
            ..
        }) if preserved == source
    ));
}

#[test]
fn accepts_limit_depth_and_rejects_one_level_beyond() {
    let atom = || latex_rust::MathNode::Atom('x', latex_rust::AtomKind::Ord);
    let mut at_limit = atom();
    for _ in 1..MAX_MATH_NESTING_DEPTH {
        at_limit = latex_rust::MathNode::Superscript(Box::new(at_limit), Box::new(atom()));
    }
    assert_eq!(
        check_ast_budget(&at_limit, MAX_MATH_NESTING_DEPTH, usize::MAX),
        Ok(())
    );

    let over_limit = latex_rust::MathNode::Superscript(Box::new(at_limit), Box::new(atom()));
    assert_eq!(
        check_ast_budget(&over_limit, MAX_MATH_NESTING_DEPTH, usize::MAX),
        Err(AstBudgetError::TooDeep)
    );
}

#[test]
fn counts_synthetic_matrix_rows_and_optional_placeholders_in_depth_budget() {
    let atom = || latex_rust::MathNode::Atom('x', latex_rust::AtomKind::Ord);
    let nested = |wrappers| {
        let mut node = atom();
        for _ in 0..wrappers {
            node = latex_rust::MathNode::Superscript(Box::new(node), Box::new(atom()));
        }
        node
    };
    let matrix_at_limit = latex_rust::MathNode::Matrix(
        latex_rust::MatrixStyle::Matrix,
        Vec::new(),
        vec![latex_rust::EnvRow::Cells {
            cells: vec![nested(29)],
            number: latex_rust::EqNumber::Default,
            labels: Vec::new(),
        }],
    );
    assert_eq!(
        check_ast_budget(&matrix_at_limit, MAX_MATH_NESTING_DEPTH, usize::MAX),
        Ok(())
    );
    let matrix_over_limit = latex_rust::MathNode::Matrix(
        latex_rust::MatrixStyle::Matrix,
        Vec::new(),
        vec![latex_rust::EnvRow::Cells {
            cells: vec![nested(30)],
            number: latex_rust::EqNumber::Default,
            labels: Vec::new(),
        }],
    );
    assert_eq!(
        check_ast_budget(&matrix_over_limit, MAX_MATH_NESTING_DEPTH, usize::MAX),
        Err(AstBudgetError::TooDeep)
    );

    let mut operator_at_limit = latex_rust::MathNode::Sum(None, None);
    for _ in 0..(MAX_MATH_NESTING_DEPTH - 1) {
        operator_at_limit =
            latex_rust::MathNode::Superscript(Box::new(operator_at_limit), Box::new(atom()));
    }
    assert_eq!(
        check_ast_budget(&operator_at_limit, MAX_MATH_NESTING_DEPTH, usize::MAX),
        Err(AstBudgetError::TooDeep)
    );
}

#[test]
fn rejects_syntax_tree_larger_than_node_budget() {
    let accepted_source = "x".repeat(MAX_MATH_SYNTAX_NODES - 1);
    assert!(parse(&accepted_source, MathInputKind::Inline).is_ok());

    let source = "x".repeat(MAX_MATH_SYNTAX_NODES);
    assert!(matches!(
        parse(&source, MathInputKind::Inline),
        Err(MathParseError::SyntaxTreeTooLarge {
            source: preserved,
            limit: MAX_MATH_SYNTAX_NODES,
            ..
        }) if preserved == source
    ));
}

#[test]
fn syntax_budget_matches_converted_matrix_text_and_placeholder_trees() {
    for (source, kind) in [
        (
            r"\begin{aligned}a &= b \\ \intertext{because} \\ \hline c &= d\end{aligned}",
            MathInputKind::Align,
        ),
        (r"\text{outer \textbf{bold $x^2$}}", MathInputKind::Inline),
        (r"\lim x + \sqrt{x}", MathInputKind::Inline),
    ] {
        let Ok(parsed) = parse(source, kind) else {
            panic!("representative syntax should parse: {source}");
        };
        let Ok(tree) = parsed.syntax_tree() else {
            panic!("representative syntax should convert: {source}");
        };
        let (node_count, depth) = count_nodes_and_depth(&tree.root);
        assert_eq!(
            check_ast_budget(&parsed.ast, depth, node_count),
            Ok(()),
            "budget should allow the exact converted tree size for {source}"
        );
        assert_eq!(
            check_ast_budget(&parsed.ast, depth, node_count - 1),
            Err(AstBudgetError::TooManyNodes),
            "one fewer node should reject {source}"
        );
        assert_eq!(
            check_ast_budget(&parsed.ast, depth - 1, node_count),
            Err(AstBudgetError::TooDeep),
            "one fewer depth level should reject {source}"
        );
    }
}

fn count_nodes_and_depth(root: &MathSyntaxNode) -> (usize, usize) {
    let mut pending = vec![(root, 1usize)];
    let mut count = 0;
    let mut max_depth = 0;
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        max_depth = max_depth.max(depth);
        pending.extend(node.children.iter().map(|child| (child, depth + 1)));
    }
    (count, max_depth)
}

#[test]
fn dimensions_keep_exact_rational_values() {
    let Ok(parsed) = parse(r"\hspace{1mu}", MathInputKind::Inline) else {
        panic!("formula should parse");
    };
    let Ok(tree) = parsed.syntax_tree() else {
        panic!("formula should convert");
    };
    assert_eq!(
        tree.root
            .attributes
            .get("valueNumerator")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        tree.root
            .attributes
            .get("valueDenominator")
            .map(String::as_str),
        Some("18")
    );
    assert_eq!(
        tree.root.attributes.get("valueUnit").map(String::as_str),
        Some("em")
    );

    let Ok(parsed) = parse(r"\rule{0em}{1mu}", MathInputKind::Inline) else {
        panic!("strut formula should parse");
    };
    let Ok(tree) = parsed.syntax_tree() else {
        panic!("strut formula should convert");
    };
    assert_eq!(tree.root.kind, MathSyntaxKind::Strut);
    assert_eq!(
        tree.root
            .attributes
            .get("heightNumerator")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        tree.root
            .attributes
            .get("heightDenominator")
            .map(String::as_str),
        Some("18")
    );
}

#[test]
fn distinguishes_character_and_named_vertical_delimiters() {
    let Ok(single) = parse(r"\left|x\right|", MathInputKind::Inline) else {
        panic!("single bar formula should parse");
    };
    let Ok(double) = parse(r"\left\|x\right\|", MathInputKind::Inline) else {
        panic!("double bar formula should parse");
    };
    let Ok(single_tree) = single.syntax_tree() else {
        panic!("single bar formula should convert");
    };
    let Ok(double_tree) = double.syntax_tree() else {
        panic!("double bar formula should convert");
    };
    assert_eq!(
        single_tree.root.attributes.get("left").map(String::as_str),
        Some("char:|")
    );
    assert_eq!(
        double_tree.root.attributes.get("left").map(String::as_str),
        Some("named:|")
    );

    let Ok(empty) = parse(r"\left.x\right.", MathInputKind::Inline) else {
        panic!("empty delimiter formula should parse");
    };
    let Ok(empty_tree) = empty.syntax_tree() else {
        panic!("empty delimiter formula should convert");
    };
    assert_eq!(
        empty_tree.root.attributes.get("left").map(String::as_str),
        Some("empty")
    );

    let Ok(sized) = parse(r"\big|x", MathInputKind::Inline) else {
        panic!("sized delimiter formula should parse");
    };
    let Ok(sized_tree) = sized.syntax_tree() else {
        panic!("sized delimiter formula should convert");
    };
    let sized_delimiter = if sized_tree.root.kind == MathSyntaxKind::SizedDelimiter {
        &sized_tree.root
    } else {
        &sized_tree.root.children[0]
    };
    assert_eq!(sized_delimiter.kind, MathSyntaxKind::SizedDelimiter);
    assert_eq!(sized_delimiter.text.as_deref(), Some("char:|"));
    assert_eq!(
        sized_delimiter.attributes.get("size").map(String::as_str),
        Some("big")
    );
}
