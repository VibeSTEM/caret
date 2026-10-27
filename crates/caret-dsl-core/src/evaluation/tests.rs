//! 验证 DSL 求值、错误传播和资源限制。

#![allow(clippy::expect_used)]

use super::{EvaluatedValue, MAX_DEPENDENCY_DEPTH, evaluate_variables};

fn evaluate(values: &str) -> Result<super::EvaluatedVariables, crate::Diagnostics> {
    evaluate_variables(&format!("version: 1\nkind: variables\n\nvalues:\n{values}"))
}

#[test]
fn evaluates_forward_references_and_arithmetic() {
    let result = evaluate(
            "  total: int = width * 2\n  width: int = 12\n  half: real = total / 4\n  scaled: real = half + 0.5\n",
        )
        .expect("valid expressions");
    assert_eq!(result.get("total"), Some(&EvaluatedValue::Int(24)));
    assert_eq!(result.get("half"), Some(&EvaluatedValue::Real(6.0)));
    assert_eq!(result.get("scaled"), Some(&EvaluatedValue::Real(6.5)));
}

#[test]
fn accepts_typed_literals_and_escapes() {
    let result = evaluate(
            "  enabled: bool = true\n  label: string = \"feed\\nport\"\n  mesh: resource = project:/assets/mesh.vtu\n  promoted: real = 2\n",
        )
        .expect("valid literals");
    assert_eq!(result.get("enabled"), Some(&EvaluatedValue::Bool(true)));
    assert_eq!(
        result.get("label"),
        Some(&EvaluatedValue::String("feed\nport".to_owned()))
    );
    assert_eq!(
        result.get("mesh"),
        Some(&EvaluatedValue::Resource(
            "project:/assets/mesh.vtu".to_owned()
        ))
    );
    assert_eq!(result.get("promoted"), Some(&EvaluatedValue::Real(2.0)));
}

#[test]
fn rejects_cycles_unknown_references_type_mismatches_and_division_by_zero() {
    for (source, expected) in [
        (
            "  first: int = second\n  second: int = first\n",
            "variables.dependency_cycle",
        ),
        ("  first: int = missing\n", "variables.unknown_reference"),
        ("  first: int = 1 / 2\n", "variables.type_mismatch"),
        ("  first: real = 1 / 0\n", "variables.division_by_zero"),
        ("  first: bool = yes\n", "variables.literal_syntax"),
    ] {
        let error = evaluate(source).expect_err("invalid variables");
        assert!(
            error
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == expected),
            "expected {expected}: {error}"
        );
    }
}

#[test]
fn reports_source_spans_for_expression_errors() {
    let source =
        "version: 1\nkind: variables\n\nvalues:\n  good: int = 1\n  bad: int = good + unknown\n";
    let error = evaluate_variables(source).expect_err("unknown variable");
    let diagnostic = error
        .diagnostics
        .iter()
        .find(|item| item.code == "variables.unknown_reference")
        .expect("reference diagnostic");
    assert_eq!(
        &source[diagnostic.start_offset..diagnostic.end_offset],
        "unknown"
    );
    assert_eq!(diagnostic.line, 6);
}

#[test]
fn supports_signed_minimum_integer_and_canonical_string_quoting() {
    let source = "version: 1\nkind: variables\n\nvalues:\n  minimum: int = -9223372036854775808\n  label: string = \"feed port\"\n";
    let result = evaluate_variables(source).expect("valid boundaries");
    assert_eq!(result.get("minimum"), Some(&EvaluatedValue::Int(i64::MIN)));
    let formatted = crate::format_source(source).expect("format source");
    assert_eq!(formatted, source);
    assert_eq!(evaluate_variables(&formatted).expect("re-evaluate"), result);
}

#[test]
fn preserves_escaped_trailing_space_and_valid_unicode_diagnostic_spans() {
    let result = evaluate("  label: string = end\\ \n").expect("escaped trailing space");
    assert_eq!(
        result.get("label"),
        Some(&EvaluatedValue::String("end ".to_owned()))
    );

    let source = "version: 1\nkind: variables\n\nvalues:\n  label: string = \\\u{4e2d}\n";
    let error = evaluate_variables(source).expect_err("unsupported Unicode escape");
    let diagnostic = error
        .diagnostics
        .iter()
        .find(|item| item.code == "variables.invalid_escape")
        .expect("escape diagnostic");
    assert_eq!(
        &source[diagnostic.start_offset..diagnostic.end_offset],
        "\\中"
    );
}

#[test]
fn variable_names_true_and_false_are_references_in_numeric_expressions() {
    let result = evaluate("  result: int = true\n  true: int = 5\n")
        .expect("boolean-looking variable name is a reference");
    assert_eq!(result.get("result"), Some(&EvaluatedValue::Int(5)));
}

#[test]
fn rejects_excessive_operators_before_building_a_deep_tree() {
    let expression = std::iter::repeat_n("1", 2_000)
        .collect::<Vec<_>>()
        .join(" * ");
    let error = evaluate(&format!("  result: int = {expression}\n"))
        .expect_err("flat expression exceeds the operator limit");
    assert!(
        error
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "variables.expression_syntax")
    );

    let terms = std::iter::repeat_n("+1", 64)
        .collect::<Vec<_>>()
        .join(" + ");
    let result = evaluate(&format!("  result: int = + {terms}\n"))
        .expect("the expression operator limit is inclusive");
    assert_eq!(result.get("result"), Some(&EvaluatedValue::Int(64)));

    let terms = std::iter::repeat_n("+1", 65)
        .collect::<Vec<_>>()
        .join(" + ");
    let error = evaluate(&format!("  result: int = {terms}\n"))
        .expect_err("unary operators count toward the expression limit");
    assert!(
        error
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "variables.expression_syntax")
    );
}

#[test]
#[cfg_attr(
    miri,
    ignore = "依赖深度与求值预算压力样本的解释执行成本过高；普通测试仍验证预算边界"
)]
fn rejects_dependency_depth_and_operation_budget_overruns() {
    let mut dependencies = String::new();
    for index in 0..=MAX_DEPENDENCY_DEPTH {
        dependencies.push_str(&format!("  v-{index}: int = v-{}\n", index + 1));
    }
    dependencies.push_str(&format!("  v-{}: int = 1\n", MAX_DEPENDENCY_DEPTH + 1));
    let error = evaluate(&dependencies).expect_err("dependency depth is bounded");
    assert!(
        error
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "variables.dependency_depth")
    );

    let expression = std::iter::repeat_n("1", 50).collect::<Vec<_>>().join(" + ");
    let mut operations = String::new();
    for index in 0..1_100 {
        operations.push_str(&format!("  value-{index}: int = {expression}\n"));
    }
    let error = evaluate(&operations).expect_err("evaluation work is bounded");
    assert!(
        error
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "variables.evaluation_limit")
    );
}
