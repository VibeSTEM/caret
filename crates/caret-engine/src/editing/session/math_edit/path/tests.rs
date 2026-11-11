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

fn text_content(value: &str) -> MathContent {
    MathContent::Text(caret_doc::TextFlow {
        runs: vec![caret_doc::Run {
            text: value.to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        }],
        extra: Default::default(),
    })
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
fn text_position_order_validates_shared_flow_grapheme_boundaries() {
    let mut tree = parsed_tree(r"\text{ab}");
    let path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(0)], None);
    let Some(text) = path.resolve_structurally_mut(&mut tree) else {
        panic!("TextFlow path should resolve");
    };
    text.runs = vec![
        caret_doc::Run {
            text: "a".to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        },
        caret_doc::Run {
            text: "\u{301}b".to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        },
    ];
    let start = MathTextPosition::new(path.clone(), 0, MathPositionAffinity::Downstream);
    let end = MathTextPosition::new(path.clone(), 4, MathPositionAffinity::Upstream);
    let inside_combining_cluster =
        MathTextPosition::new(path.clone(), 1, MathPositionAffinity::Downstream);

    assert_eq!(
        start.compare_in_same_flow(&end, &tree),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        start.compare_in_same_flow(&inside_combining_cluster, &tree),
        None
    );
    assert_eq!(
        MathTextPosition::new(path.clone(), 3, MathPositionAffinity::Upstream)
            .compare_in_same_flow(
                &MathTextPosition::new(path, 3, MathPositionAffinity::Downstream),
                &tree,
            ),
        Some(std::cmp::Ordering::Less)
    );
}

#[test]
fn text_position_order_compares_distinct_flows_in_the_same_text_island() {
    let mut tree = parsed_tree(r"\text{a}");
    tree.root.content = vec![text_content("a"), text_content("b")];
    let first_path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(0)], None);
    let second_path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(1)], None);
    let first_start =
        MathTextPosition::new(first_path.clone(), 0, MathPositionAffinity::Downstream);
    let first_end = MathTextPosition::new(first_path, 1, MathPositionAffinity::Upstream);
    let second_start =
        MathTextPosition::new(second_path.clone(), 0, MathPositionAffinity::Downstream);
    let second_end = MathTextPosition::new(second_path, 1, MathPositionAffinity::Upstream);

    assert_eq!(
        first_start.compare_in_same_text_island(&second_end, &tree),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        first_end.compare_in_same_text_island(&second_start, &tree),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        second_end.compare_in_same_text_island(&first_start, &tree),
        Some(std::cmp::Ordering::Greater)
    );
}

#[test]
fn text_position_order_keeps_empty_flows_between_neighboring_text_positions() {
    let mut tree = parsed_tree(r"\text{a}");
    tree.root.content = vec![text_content("a"), text_content(""), text_content("b")];
    let path = |index| MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(index)], None);
    let previous_end = MathTextPosition::new(path(0), 1, MathPositionAffinity::Upstream);
    let empty_upstream = MathTextPosition::new(path(1), 0, MathPositionAffinity::Upstream);
    let empty_downstream = MathTextPosition::new(path(1), 0, MathPositionAffinity::Downstream);
    let next_start = MathTextPosition::new(path(2), 0, MathPositionAffinity::Downstream);

    assert_eq!(
        previous_end.compare_in_same_text_island(&empty_upstream, &tree),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        empty_upstream.compare_in_same_text_island(&empty_downstream, &tree),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        empty_downstream.compare_in_same_text_island(&next_start, &tree),
        Some(std::cmp::Ordering::Less)
    );
}

#[test]
fn text_position_order_rejects_grapheme_split_across_environment_paths() {
    let mut tree = parsed_tree(r"\text{a}");
    tree.root.content = vec![
        text_content("a"),
        MathContent::Environment(caret_doc::InlineEnvironment {
            id: caret_doc::NodeId::generate(),
            kind: caret_doc::InlineEnvironmentKind::Bold,
            definition_id: None,
            props: Default::default(),
            children: vec![text_content("\u{301}b")],
            extra: Default::default(),
        }),
    ];
    let first_path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(0)], None);
    let second_path = MathTextFlowPath::new(
        vec![
            MathTextFlowPathStep::Content(1),
            MathTextFlowPathStep::EnvironmentChild(0),
        ],
        None,
    );
    let first_flow_edge =
        MathTextPosition::new(first_path.clone(), 1, MathPositionAffinity::Upstream);
    let second_flow_edge =
        MathTextPosition::new(second_path.clone(), 0, MathPositionAffinity::Downstream);
    let start = MathTextPosition::new(first_path, 0, MathPositionAffinity::Downstream);
    let end = MathTextPosition::new(second_path, 3, MathPositionAffinity::Upstream);

    assert_eq!(first_flow_edge.compare_in_same_flow(&start, &tree), None);
    assert_eq!(second_flow_edge.compare_in_same_flow(&end, &tree), None);
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
