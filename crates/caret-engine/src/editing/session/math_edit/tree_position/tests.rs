//! 验证数学树结构位置的查找、偏移映射与边界行为。

use super::*;
use caret_doc::{
    Align, InlineEnvironment, InlineEnvironmentKind, MathAlignRow, MathFlow, MathNode,
    MathNodeKind, MathPathStep, Run, TextFlow,
};
use std::collections::BTreeMap;

fn flow(content: Vec<MathContent>) -> MathFlow {
    MathFlow {
        id: NodeId::generate(),
        content,
        extra: BTreeMap::new(),
    }
}

fn tree(content: Vec<MathContent>) -> MathTree {
    MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: flow(content),
        extra: BTreeMap::new(),
    }
}

fn node(kind: MathNodeKind, children: Vec<MathFlow>) -> MathContent {
    MathContent::Math(MathNode {
        id: NodeId::generate(),
        kind,
        text: None,
        attributes: BTreeMap::new(),
        children,
        extra: BTreeMap::new(),
    })
}

fn text(value: &str) -> MathContent {
    MathContent::Text(TextFlow {
        runs: vec![Run {
            text: value.to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    })
}

fn position(path: Vec<MathSequenceStep>, offset: usize) -> MathTreePosition {
    MathTreePosition {
        host: MathPositionHost::InlineMath {
            owner_id: NodeId::generate(),
        },
        path: MathSequencePath::new(path),
        offset,
        affinity: MathPositionAffinity::Downstream,
    }
}

#[test]
fn resolves_root_boundaries() {
    let tree = tree(vec![node(MathNodeKind::Row, vec![])]);
    assert_eq!(
        position(vec![], 0)
            .resolve(&tree)
            .unwrap_or_else(|_| panic!("root start resolves"))
            .len(),
        1
    );
    assert_eq!(
        position(vec![], 1)
            .resolve(&tree)
            .unwrap_or_else(|_| panic!("root end resolves"))
            .len(),
        1
    );
}

#[test]
fn resolves_nested_operand_and_empty_fixed_slot() {
    let tree = tree(vec![node(
        MathNodeKind::Fraction,
        vec![flow(vec![node(MathNodeKind::Row, vec![])]), flow(vec![])],
    )]);
    let numerator = position(
        vec![MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }],
        1,
    );
    assert_eq!(
        numerator
            .resolve(&tree)
            .unwrap_or_else(|_| panic!("numerator resolves"))
            .len(),
        1
    );
    let denominator = position(
        vec![MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        }],
        0,
    );
    assert!(
        denominator
            .resolve(&tree)
            .unwrap_or_else(|_| panic!("denominator resolves"))
            .is_empty()
    );
}

#[test]
fn resolves_environment_children() {
    let tree = tree(vec![MathContent::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::new(),
        children: vec![node(MathNodeKind::Row, vec![])],
        extra: BTreeMap::new(),
    })]);
    let position = position(
        vec![MathSequenceStep::EnvironmentChildren { content_index: 0 }],
        1,
    );
    assert_eq!(
        position
            .resolve(&tree)
            .unwrap_or_else(|_| panic!("environment resolves"))
            .len(),
        1
    );
}

#[test]
fn rejects_invalid_container_path_and_offset() {
    let tree = tree(vec![node(MathNodeKind::Row, vec![])]);
    assert_eq!(
        position(
            vec![MathSequenceStep::EnvironmentChildren { content_index: 0 }],
            0
        )
        .resolve(&tree),
        Err(MathTreePositionError::InvalidPath { depth: 0 })
    );
    assert_eq!(
        position(
            vec![MathSequenceStep::Operand {
                content_index: 4,
                operand_index: 0,
            }],
            0
        )
        .resolve(&tree),
        Err(MathTreePositionError::InvalidPath { depth: 0 })
    );
    assert_eq!(
        position(vec![], 2).resolve(&tree),
        Err(MathTreePositionError::OffsetOutOfBounds {
            offset: 2,
            content_len: 1,
        })
    );
}

#[test]
fn converts_root_text_flow_boundaries_without_source_offsets() {
    let tree = tree(vec![text("abc")]);
    let owner_id = NodeId::generate();
    let start = MathTreePosition::from_text_flow_boundary(
        &tree,
        MathPositionHost::InlineMath {
            owner_id: owner_id.clone(),
        },
        &[MathPathStep::Content(0)],
        None,
        MathTextFlowBoundary::Start,
        MathPositionAffinity::Downstream,
    )
    .unwrap_or_else(|_| panic!("root TextFlow start should resolve"));
    let end = MathTreePosition::from_text_flow_boundary(
        &tree,
        MathPositionHost::InlineMath { owner_id },
        &[MathPathStep::Content(0)],
        None,
        MathTextFlowBoundary::End,
        MathPositionAffinity::Upstream,
    )
    .unwrap_or_else(|_| panic!("root TextFlow end should resolve"));
    assert_eq!(start.path.steps(), &[]);
    assert_eq!(start.offset, 0);
    assert_eq!(start.affinity, MathPositionAffinity::Downstream);
    assert_eq!(end.offset, 1);
    assert_eq!(end.affinity, MathPositionAffinity::Upstream);
}

#[test]
fn converts_environment_and_fixed_operand_text_boundaries() {
    let tree = tree(vec![
        MathContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: BTreeMap::new(),
            children: vec![text("bold")],
            extra: BTreeMap::new(),
        }),
        node(
            MathNodeKind::Fraction,
            vec![flow(vec![]), flow(vec![text("denominator")])],
        ),
    ]);
    let environment = MathTreePosition::from_text_flow_boundary(
        &tree,
        MathPositionHost::Equation {
            owner_id: NodeId::generate(),
        },
        &[MathPathStep::Content(0), MathPathStep::EnvironmentChild(0)],
        None,
        MathTextFlowBoundary::End,
        MathPositionAffinity::Upstream,
    )
    .unwrap_or_else(|_| panic!("environment TextFlow end should resolve"));
    assert_eq!(
        environment.path.steps(),
        &[MathSequenceStep::EnvironmentChildren { content_index: 0 }]
    );
    assert_eq!(environment.offset, 1);

    let denominator = MathTreePosition::from_text_flow_boundary(
        &tree,
        MathPositionHost::Equation {
            owner_id: NodeId::generate(),
        },
        &[
            MathPathStep::Content(1),
            MathPathStep::Operand(1),
            MathPathStep::Content(0),
        ],
        None,
        MathTextFlowBoundary::Start,
        MathPositionAffinity::Downstream,
    )
    .unwrap_or_else(|_| panic!("fixed-slot TextFlow start should resolve"));
    assert_eq!(
        denominator.path.steps(),
        &[MathSequenceStep::Operand {
            content_index: 1,
            operand_index: 1,
        }]
    );
    assert_eq!(denominator.offset, 0);
}

#[test]
fn align_text_positions_keep_stable_cell_identity() {
    let cell_tree = tree(vec![text("cell")]);
    let row_id = NodeId::generate();
    let cell_id = cell_tree.root.id.clone();
    let align = Align {
        id: NodeId::generate(),
        rows: vec![MathAlignRow {
            id: row_id.clone(),
            cells: vec![cell_tree],
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    };
    let tree = &align.rows[0].cells[0];
    let host = MathPositionHost::AlignCell { row_id, cell_id };
    let position = MathTreePosition::from_text_flow_boundary(
        tree,
        host.clone(),
        &[MathPathStep::Content(0)],
        Some((0, 0)),
        MathTextFlowBoundary::End,
        MathPositionAffinity::Upstream,
    )
    .unwrap_or_else(|_| panic!("Align cell TextFlow end should resolve"));
    assert_eq!(position.host, host);
    assert_eq!(position.offset, 1);
}

#[test]
fn movement_enters_nested_operands_and_returns_to_the_parent_sequence() {
    let tree = tree(vec![
        text("a"),
        node(
            MathNodeKind::Fraction,
            vec![flow(vec![text("x")]), flow(vec![text("y")])],
        ),
        text("b"),
    ]);
    let start = MathTreePosition::from_text_flow_boundary(
        &tree,
        MathPositionHost::Equation {
            owner_id: NodeId::generate(),
        },
        &[MathPathStep::Content(0)],
        None,
        MathTextFlowBoundary::End,
        MathPositionAffinity::Downstream,
    )
    .unwrap_or_else(|_| panic!("preceding text boundary should resolve"));
    let MathTreeCaretMove::TextFlow(numerator, numerator_cursor) =
        advance_math_tree_position(&tree, &start, true, None)
            .unwrap_or_else(|_| panic!("forward movement should enter the fraction"))
    else {
        panic!("forward movement should enter the first operand text");
    };
    assert_eq!(numerator_cursor, 0);
    assert_eq!(
        numerator.steps(),
        &[
            MathPathStep::Content(1),
            MathPathStep::Operand(0),
            MathPathStep::Content(0),
        ]
    );

    let operand_end = MathTreePosition::from_text_flow_boundary(
        &tree,
        MathPositionHost::Equation {
            owner_id: NodeId::generate(),
        },
        numerator.steps(),
        None,
        MathTextFlowBoundary::End,
        MathPositionAffinity::Downstream,
    )
    .unwrap_or_else(|_| panic!("operand end should resolve"));
    let MathTreeCaretMove::TextFlow(denominator, denominator_cursor) =
        advance_math_tree_position(&tree, &operand_end, true, None)
            .unwrap_or_else(|_| panic!("forward movement should enter the next operand"))
    else {
        panic!("forward movement should enter the next operand text");
    };
    assert_eq!(
        denominator.steps(),
        &[
            MathPathStep::Content(1),
            MathPathStep::Operand(1),
            MathPathStep::Content(0),
        ]
    );
    assert_eq!(denominator_cursor, 0);
    let denominator_end = MathTreePosition::from_text_flow_boundary(
        &tree,
        MathPositionHost::Equation {
            owner_id: NodeId::generate(),
        },
        denominator.steps(),
        None,
        MathTextFlowBoundary::End,
        MathPositionAffinity::Downstream,
    )
    .unwrap_or_else(|_| panic!("denominator end should resolve"));
    let MathTreeCaretMove::Position(after_fraction) =
        advance_math_tree_position(&tree, &denominator_end, true, None)
            .unwrap_or_else(|_| panic!("operand boundary should return to parent sequence"))
    else {
        panic!("operand boundary should return to the parent sequence");
    };
    assert!(after_fraction.path.steps().is_empty());
    assert_eq!(after_fraction.offset, 2);
    let MathTreeCaretMove::TextFlow(following, following_cursor) =
        advance_math_tree_position(&tree, &after_fraction, true, None)
            .unwrap_or_else(|_| panic!("following text should be enterable"))
    else {
        panic!("following content should be a TextFlow");
    };
    assert_eq!(following.steps(), &[MathPathStep::Content(2)]);
    assert_eq!(following_cursor, 0);
}

#[test]
fn movement_preserves_an_empty_fixed_slot_as_a_structural_caret_stop() {
    let tree = tree(vec![node(
        MathNodeKind::Fraction,
        vec![flow(vec![text("x")]), flow(vec![])],
    )]);
    let numerator_end = MathTreePosition {
        host: MathPositionHost::Equation {
            owner_id: NodeId::generate(),
        },
        path: MathSequencePath::new(vec![MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }]),
        offset: 1,
        affinity: MathPositionAffinity::Downstream,
    };
    let MathTreeCaretMove::Position(after_numerator) =
        advance_math_tree_position(&tree, &numerator_end, true, None)
            .unwrap_or_else(|_| panic!("empty second operand should remain a caret stop"))
    else {
        panic!("empty second operand should retain a structural boundary");
    };
    assert_eq!(
        after_numerator.path.steps(),
        &[MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        }]
    );
    assert_eq!(after_numerator.offset, 0);
    assert!(
        after_numerator
            .resolve(&tree)
            .unwrap_or_else(|_| panic!("empty operand should resolve"))
            .is_empty()
    );
}

#[test]
fn rejects_mismatched_host_and_non_text_paths() {
    let tree = tree(vec![node(MathNodeKind::Row, vec![])]);
    assert_eq!(
        MathTreePosition::from_text_flow_boundary(
            &tree,
            MathPositionHost::AlignCell {
                row_id: NodeId::generate(),
                cell_id: NodeId::generate(),
            },
            &[MathPathStep::Content(0)],
            None,
            MathTextFlowBoundary::Start,
            MathPositionAffinity::Downstream,
        ),
        Err(MathTreePositionError::HostMismatch)
    );
    assert_eq!(
        MathTreePosition::from_text_flow_boundary(
            &tree,
            MathPositionHost::InlineMath {
                owner_id: NodeId::generate(),
            },
            &[MathPathStep::Content(0)],
            None,
            MathTextFlowBoundary::Start,
            MathPositionAffinity::Downstream,
        ),
        Err(MathTreePositionError::InvalidTextFlowPath { depth: 1 })
    );
    assert!(matches!(
        MathTreePosition::from_text_flow_boundary(
            &tree,
            MathPositionHost::InlineMath {
                owner_id: NodeId::generate(),
            },
            &[MathPathStep::Content(1)],
            None,
            MathTextFlowBoundary::End,
            MathPositionAffinity::Upstream,
        ),
        Err(MathTreePositionError::OffsetOutOfBounds { .. })
    ));
}

#[test]
fn rejects_align_host_for_a_different_cell_tree() {
    let tree = tree(vec![text("cell")]);
    assert_eq!(
        MathTreePosition::from_text_flow_boundary(
            &tree,
            MathPositionHost::AlignCell {
                row_id: NodeId::generate(),
                cell_id: NodeId::generate(),
            },
            &[MathPathStep::Content(0)],
            Some((0, 0)),
            MathTextFlowBoundary::Start,
            MathPositionAffinity::Downstream,
        ),
        Err(MathTreePositionError::HostMismatch)
    );
}
