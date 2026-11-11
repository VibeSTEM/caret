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
fn tree_position_order_wraps_nested_operands_and_rejects_other_hosts() {
    let fraction_tree = tree(vec![node(
        MathNodeKind::Fraction,
        vec![flow(vec![node(MathNodeKind::Row, vec![])]), flow(vec![])],
    )]);
    let owner_id = NodeId::generate();
    let host = MathPositionHost::Equation { owner_id };
    let at_start = MathTreePosition {
        host: host.clone(),
        path: MathSequencePath::root(),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    let numerator = MathTreePosition {
        host: host.clone(),
        path: MathSequencePath::new(vec![MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }]),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    let denominator = MathTreePosition {
        host: host.clone(),
        path: MathSequencePath::new(vec![MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        }]),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    let after_fraction = MathTreePosition {
        host,
        path: MathSequencePath::root(),
        offset: 1,
        affinity: MathPositionAffinity::Upstream,
    };

    assert_eq!(
        at_start.compare_in_tree(&numerator, &fraction_tree),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        numerator.compare_in_tree(&denominator, &fraction_tree),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        denominator.compare_in_tree(&after_fraction, &fraction_tree),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        after_fraction.compare_in_tree(&numerator, &fraction_tree),
        Ok(std::cmp::Ordering::Greater)
    );

    let upstream = MathTreePosition {
        affinity: MathPositionAffinity::Upstream,
        ..numerator.clone()
    };
    assert_eq!(
        upstream.compare_in_tree(&numerator, &fraction_tree),
        Ok(std::cmp::Ordering::Less)
    );
    let invalid = MathTreePosition {
        offset: 2,
        ..numerator.clone()
    };
    assert_eq!(
        invalid.compare_in_tree(&denominator, &fraction_tree),
        Err(MathTreePositionError::OffsetOutOfBounds {
            offset: 2,
            content_len: 1,
        })
    );

    let other_host = MathTreePosition {
        host: MathPositionHost::Equation {
            owner_id: NodeId::generate(),
        },
        ..numerator.clone()
    };
    assert_eq!(
        numerator.compare_in_tree(&other_host, &fraction_tree),
        Err(MathTreePositionError::HostMismatch)
    );

    let environment_tree = tree(vec![
        MathContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: BTreeMap::new(),
            children: vec![text("inside")],
            extra: BTreeMap::new(),
        }),
        node(MathNodeKind::Row, vec![]),
    ]);
    let host = MathPositionHost::Equation {
        owner_id: NodeId::generate(),
    };
    let before_environment = MathTreePosition {
        host: host.clone(),
        path: MathSequencePath::root(),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    let inside_environment = MathTreePosition {
        host: host.clone(),
        path: MathSequencePath::new(vec![MathSequenceStep::EnvironmentChildren {
            content_index: 0,
        }]),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    let after_environment = MathTreePosition {
        host,
        path: MathSequencePath::root(),
        offset: 1,
        affinity: MathPositionAffinity::Upstream,
    };
    assert_eq!(
        before_environment.compare_in_tree(&inside_environment, &environment_tree),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        inside_environment.compare_in_tree(&after_environment, &environment_tree),
        Ok(std::cmp::Ordering::Less)
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
fn movement_skips_invisible_optional_placeholder_operands() {
    let radical_tree = tree(vec![node(
        MathNodeKind::Radical,
        vec![
            flow(vec![node(MathNodeKind::Placeholder, vec![])]),
            flow(vec![text("x")]),
        ],
    )]);
    let start = position(Vec::new(), 0);

    let MathTreeCaretMove::TextFlow(radicand, cursor) =
        advance_math_tree_position(&radical_tree, &start, true, None)
            .unwrap_or_else(|_| panic!("movement should enter the visible radicand"))
    else {
        panic!("movement must skip the invisible optional index slot");
    };
    assert_eq!(cursor, 0);
    assert_eq!(
        radicand.steps(),
        &[
            MathPathStep::Content(0),
            MathPathStep::Operand(1),
            MathPathStep::Content(0),
        ]
    );

    let radicand_start = position(
        vec![MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        }],
        0,
    );
    let MathTreeCaretMove::Position(before_radical) =
        advance_math_tree_position(&radical_tree, &radicand_start, false, None)
            .unwrap_or_else(|_| panic!("reverse movement should leave the radical"))
    else {
        panic!("reverse movement must not enter the hidden radical index");
    };
    assert!(before_radical.path.steps().is_empty());
    assert_eq!(before_radical.offset, 0);

    let sum = tree(vec![node(
        MathNodeKind::Sum,
        vec![
            flow(vec![node(MathNodeKind::Placeholder, vec![])]),
            flow(vec![node(MathNodeKind::Placeholder, vec![])]),
            flow(vec![text("body")]),
        ],
    )]);
    let body_start = position(
        vec![MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 2,
        }],
        0,
    );
    let MathTreeCaretMove::Position(before_sum) =
        advance_math_tree_position(&sum, &body_start, false, None)
            .unwrap_or_else(|_| panic!("reverse movement should leave the sum"))
    else {
        panic!("reverse movement must skip both hidden sum limits");
    };
    assert!(before_sum.path.steps().is_empty());
    assert_eq!(before_sum.offset, 0);
}

#[test]
fn reachable_position_validation_rejects_an_optional_placeholder_operand() {
    let radical_tree = tree(vec![node(
        MathNodeKind::Radical,
        vec![
            flow(vec![node(MathNodeKind::Placeholder, vec![])]),
            flow(vec![text("x")]),
        ],
    )]);
    let optional_index = position(
        vec![MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }],
        0,
    );
    assert_eq!(
        optional_index.validate_reachable(&radical_tree),
        Err(MathTreePositionError::UnreachableOptionalPlaceholder { depth: 0 })
    );
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
fn movement_does_not_expose_a_boundary_after_required_placeholder() {
    let fraction_tree = tree(vec![node(
        MathNodeKind::Fraction,
        vec![
            flow(vec![node(MathNodeKind::Placeholder, vec![])]),
            flow(vec![node(MathNodeKind::Placeholder, vec![])]),
        ],
    )]);
    let numerator_start = position(
        vec![MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }],
        0,
    );

    let MathTreeCaretMove::Position(next) =
        advance_math_tree_position(&fraction_tree, &numerator_start, true, None)
            .unwrap_or_else(|_| panic!("right movement should leave the empty numerator"))
    else {
        panic!("right movement should enter the next structural operand");
    };
    assert_eq!(
        next.path.steps(),
        &[MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        }]
    );
    assert_eq!(next.offset, 0);

    let denominator_start = position(
        vec![MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        }],
        0,
    );
    let MathTreeCaretMove::Position(previous) =
        advance_math_tree_position(&fraction_tree, &denominator_start, false, None)
            .unwrap_or_else(|_| panic!("left movement should enter the previous numerator"))
    else {
        panic!("left movement should enter the previous structural operand");
    };
    assert_eq!(
        previous.path.steps(),
        &[MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }]
    );
    assert_eq!(previous.offset, 0);

    let mixed = tree(vec![node(
        MathNodeKind::Fraction,
        vec![
            flow(vec![
                node(MathNodeKind::Placeholder, vec![]),
                node(MathNodeKind::Symbol, vec![]),
            ]),
            flow(vec![]),
        ],
    )]);
    let mixed_start = position(
        vec![MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }],
        0,
    );
    let MathTreeCaretMove::Position(after_symbol) =
        advance_math_tree_position(&mixed, &mixed_start, true, None)
            .unwrap_or_else(|_| panic!("right movement should cross stale placeholder geometry"))
    else {
        panic!("right movement should remain in the mixed operand");
    };
    assert_eq!(after_symbol.offset, 2);
    let MathTreeCaretMove::Position(before_placeholder) =
        advance_math_tree_position(&mixed, &after_symbol, false, None)
            .unwrap_or_else(|_| panic!("left movement should cross stale placeholder geometry"))
    else {
        panic!("left movement should remain in the mixed operand");
    };
    assert_eq!(before_placeholder.offset, 0);
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
