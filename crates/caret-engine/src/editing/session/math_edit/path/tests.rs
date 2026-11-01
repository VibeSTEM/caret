//! 验证数学文本路径跨行内环境与固定操作数的定位。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::MathTextFlowPathStep;
use super::*;
use caret_doc::{MathContent, OpaqueNode, SoftBreak};

fn parsed_tree(source: &str) -> MathTree {
    match caret_math::parse(source, caret_math::MathInputKind::Inline) {
        Ok(caret_math::ParsedMath::Tree(tree)) => tree,
        Ok(caret_math::ParsedMath::Align(_)) => panic!("expected a formula tree"),
        Err(error) => panic!("test formula should parse: {error}"),
    }
}

#[test]
fn text_flow_path_resolves_through_math_operand_and_environment() {
    let mut tree = parsed_tree(r"\frac{\text{a\textbf{b}}}{y}");
    let path = MathTextFlowPath::new(
        vec![
            MathTextFlowPathStep::Content(0),
            MathTextFlowPathStep::Operand(0),
            MathTextFlowPathStep::Content(1),
            MathTextFlowPathStep::EnvironmentChild(0),
        ],
        (21, 22),
        None,
    );
    assert_eq!(
        path.resolve(&tree).map(|flow| flow.runs[0].text.as_str()),
        Some("b")
    );
    let Some(text) = path.resolve_mut(&mut tree) else {
        panic!("mutable path should resolve the same TextFlow");
    };
    text.runs[0].text = "c".to_owned();
    assert_eq!(
        path.resolve(&tree).map(|flow| flow.runs[0].text.as_str()),
        Some("c")
    );
}

#[test]
fn text_flow_path_rejects_stale_ranges_and_wrong_container_steps() {
    let tree = parsed_tree(r"\text{x}");
    let wrong_range = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(0)], (0, 1), None);
    assert!(wrong_range.resolve(&tree).is_none());
    let wrong_step = MathTextFlowPath::new(vec![MathTextFlowPathStep::Operand(0)], (6, 7), None);
    assert!(wrong_step.resolve(&tree).is_none());
}

#[test]
fn adjacent_text_paths_are_limited_to_their_flow_and_keep_align_host_offsets() {
    let tree = parsed_tree(r"\text{a}\text{b}");
    let second = path_at_source_boundary(&tree, 15, false, Some((0, 1)))
        .unwrap_or_else(|| panic!("second text flow should have a source boundary"));
    let first = adjacent_text_flow_path(&tree, &second, false)
        .unwrap_or_else(|| panic!("first text flow should be adjacent"));
    assert_eq!(first.align_cell(), Some((0, 1)));
    assert_eq!(first.source_range(), (6, 7));
    assert_eq!(first.host_source_range(), (6, 7));

    let hosted = second
        .with_host_source_base(100)
        .unwrap_or_else(|| panic!("host range should fit"));
    let first = adjacent_text_flow_path(&tree, &hosted, false)
        .unwrap_or_else(|| panic!("hosted first text flow should remain adjacent"));
    assert_eq!(first.host_source_range(), (106, 107));
}

#[test]
fn adjacent_text_navigation_stops_at_breaks_opaque_nodes_and_math_slots() {
    let mut with_break = parsed_tree(r"\text{a}\text{b}");
    with_break
        .root
        .content
        .insert(1, MathContent::SoftBreak(SoftBreak::default()));
    let second = path_at_source_boundary(&with_break, 15, false, None)
        .unwrap_or_else(|| panic!("second text flow should have a source boundary"));
    assert!(adjacent_text_flow_path(&with_break, &second, false).is_none());

    let mut with_opaque = parsed_tree(r"\text{a}\text{b}");
    with_opaque.root.content.insert(
        1,
        MathContent::Opaque(OpaqueNode {
            raw: serde_json::json!({"type": "future"}),
        }),
    );
    let second = path_at_source_boundary(&with_opaque, 15, false, None)
        .unwrap_or_else(|| panic!("second text flow should have a source boundary"));
    assert!(adjacent_text_flow_path(&with_opaque, &second, false).is_none());

    let fraction_source = r"\frac{\text{a}}{\text{b}}";
    let fraction = parsed_tree(fraction_source);
    let denominator_end = fraction_source
        .rfind('b')
        .map(|offset| offset + 1)
        .unwrap_or_else(|| panic!("denominator text should exist"));
    let second = path_at_source_boundary(&fraction, denominator_end, false, None)
        .unwrap_or_else(|| panic!("denominator text flow should have a source boundary"));
    assert!(adjacent_text_flow_path(&fraction, &second, false).is_none());
}
