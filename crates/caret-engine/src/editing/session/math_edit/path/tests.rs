//! 验证数学文本路径跨行内环境与固定操作数的定位。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::MathTextFlowPathStep;
use super::*;
use caret_doc::{MathContent, OpaqueNode, SoftBreak};

fn parsed_tree(source: &str) -> MathTree {
    match caret_math::parse_projection(source, caret_math::MathInputKind::Inline) {
        Ok(projection) => match projection.into_parts().0 {
            caret_math::ProjectedMath::Tree(tree) => tree,
            caret_math::ProjectedMath::Align(_) => panic!("expected a formula tree"),
        },
        Err(error) => panic!("test formula should parse: {error}"),
    }
}

fn projected_tree_map(source: &str) -> caret_math::MathTreeSourceMap {
    match caret_math::parse_projection(source, caret_math::MathInputKind::Inline) {
        Ok(projection) => projection
            .source_map()
            .tree()
            .cloned()
            .unwrap_or_else(|| panic!("test formula should have a source map")),
        Err(error) => panic!("test formula should project: {error}"),
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
        None,
    );
    let Some(text) = path.resolve_structurally_mut(&mut tree) else {
        panic!("mutable structural path should resolve the same TextFlow");
    };
    text.runs[0].text = "c".to_owned();
    assert_eq!(
        path.resolve(&tree).map(|flow| flow.runs[0].text.as_str()),
        Some("c")
    );
}

#[test]
fn text_flow_path_uses_structure_and_rejects_wrong_container_steps() {
    let tree = parsed_tree(r"\text{x}");
    let path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(0)], None);
    assert_eq!(
        path.resolve(&tree).map(|flow| flow.runs[0].text.as_str()),
        Some("x")
    );
    let wrong_step = MathTextFlowPath::new(vec![MathTextFlowPathStep::Operand(0)], None);
    assert!(wrong_step.resolve(&tree).is_none());
}

#[test]
fn source_map_rejects_display_segments_inside_a_utf8_code_point() {
    let (content, projection) =
        match caret_math::parse_projection(r"\text{a\%b}", caret_math::MathInputKind::Inline) {
            Ok(projection) => projection.into_parts(),
            Err(error) => panic!("escaped text should project: {error}"),
        };
    let caret_math::ProjectedMath::Tree(mut tree) = content else {
        panic!("inline source should project to a math tree");
    };
    let Some(MathContent::Text(text)) = tree.root.content.first_mut() else {
        panic!("source should project to a TextFlow");
    };
    let path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(0)], None);
    text.runs = vec![caret_doc::Run {
        text: "éx".to_owned(),
        language: None,
        props: Default::default(),
        extra: Default::default(),
    }];

    assert!(path.resolve_in_projection(&tree, &projection).is_none());
}

#[test]
fn adjacent_text_paths_keep_structure_and_align_cell_identity() {
    let tree = parsed_tree(r"\text{a}\text{b}");
    let source_map = projected_tree_map(r"\text{a}\text{b}");
    let second = path_at_source_boundary(&source_map, 15, false, Some((0, 1)))
        .unwrap_or_else(|| panic!("second text flow should have a source boundary"));
    let first = adjacent_text_flow_path(&tree, &second, false)
        .unwrap_or_else(|| panic!("first text flow should be adjacent"));
    assert_eq!(first.align_cell(), Some((0, 1)));
    assert_ne!(first, second);
}

#[test]
fn adjacent_text_navigation_stops_at_breaks_opaque_nodes_and_math_slots() {
    let mut with_break = parsed_tree(r"\text{a}\text{b}");
    let source_map = projected_tree_map(r"\text{a}\text{b}");
    with_break
        .root
        .content
        .insert(1, MathContent::SoftBreak(SoftBreak::default()));
    let second = path_at_source_boundary(&source_map, 15, false, None)
        .unwrap_or_else(|| panic!("second text flow should have a source boundary"));
    assert!(adjacent_text_flow_path(&with_break, &second, false).is_none());

    let mut with_opaque = parsed_tree(r"\text{a}\text{b}");
    let source_map = projected_tree_map(r"\text{a}\text{b}");
    with_opaque.root.content.insert(
        1,
        MathContent::Opaque(OpaqueNode {
            raw: serde_json::json!({"type": "future"}),
        }),
    );
    let second = path_at_source_boundary(&source_map, 15, false, None)
        .unwrap_or_else(|| panic!("second text flow should have a source boundary"));
    assert!(adjacent_text_flow_path(&with_opaque, &second, false).is_none());

    let fraction_source = r"\frac{\text{a}}{\text{b}}";
    let fraction = parsed_tree(fraction_source);
    let source_map = projected_tree_map(fraction_source);
    let denominator_end = fraction_source
        .rfind('b')
        .map(|offset| offset + 1)
        .unwrap_or_else(|| panic!("denominator text should exist"));
    let second = path_at_source_boundary(&source_map, denominator_end, false, None)
        .unwrap_or_else(|| panic!("denominator text flow should have a source boundary"));
    assert!(adjacent_text_flow_path(&fraction, &second, false).is_none());
}

#[test]
fn align_second_cell_rebinds_unicode_text_ranges_from_the_sidecar() {
    let source = r"\begin{aligned}x^2   &=\text{中\%文}\\a&=z\end{aligned}";
    let (content, projection) =
        match caret_math::parse_projection(source, caret_math::MathInputKind::Align) {
            Ok(projection) => projection.into_parts(),
            Err(error) => panic!("Align source should project: {error}"),
        };
    let caret_math::ProjectedMath::Align(align) = content else {
        panic!("Align source should produce an Align tree");
    };
    let cell = (0, 1);
    let tree = align
        .rows
        .get(cell.0)
        .and_then(|row| row.cells.get(cell.1))
        .unwrap_or_else(|| panic!("second cell should exist"));
    let align_map = projection
        .align()
        .unwrap_or_else(|| panic!("Align projection map should exist"));
    let cell_map = align_map
        .cell_tree(cell.0, cell.1)
        .unwrap_or_else(|| panic!("second cell map should exist"));
    let source_offset = source
        .find("文")
        .map(|offset| offset + "文".len())
        .unwrap_or_else(|| panic!("Unicode text should exist"));
    let path = path_at_source_boundary(cell_map, source_offset, false, Some(cell))
        .unwrap_or_else(|| panic!("Unicode TextFlow should have a source path"));

    assert_eq!(path.align_cell(), Some(cell));
    assert!(path.resolve_in_projection(tree, &projection).is_some());
}
