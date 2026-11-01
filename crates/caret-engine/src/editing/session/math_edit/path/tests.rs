//! 验证数学文本路径跨行内环境与固定操作数的定位。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::MathTextFlowPathStep;
use super::*;

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
