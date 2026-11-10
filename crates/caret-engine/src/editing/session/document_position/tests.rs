//! 验证跨宿主位置的校验与文档顺序。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::*;
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, InlineEnvironment,
    InlineEnvironmentKind, MathContent, NodeId, Paragraph, ParagraphContent, Run, TextFlow,
};

fn text_flow(run: Run) -> ParagraphContent {
    ParagraphContent::Text(TextFlow {
        runs: vec![run],
        extra: Default::default(),
    })
}

fn caret_snapshot(session: &Session, position: DocumentCaretPosition) -> SessionCaretSnapshot {
    SessionCaretSnapshot {
        session_id: session.session_id.clone(),
        document_generation: session.document_generation,
        position,
    }
}

fn collect_math_text_paths(
    contents: &[MathContent],
    environment_children: bool,
    path: &mut Vec<MathTextFlowPathStep>,
    paths: &mut Vec<(Vec<MathTextFlowPathStep>, String)>,
) {
    for (index, content) in contents.iter().enumerate() {
        path.push(if environment_children {
            MathTextFlowPathStep::EnvironmentChild(index)
        } else {
            MathTextFlowPathStep::Content(index)
        });
        match content {
            MathContent::Text(text) => paths.push((
                path.clone(),
                text.runs.iter().map(|run| run.text.as_str()).collect(),
            )),
            MathContent::Math(node) => {
                for (operand, flow) in node.children.iter().enumerate() {
                    path.push(MathTextFlowPathStep::Operand(operand));
                    collect_math_text_paths(&flow.content, false, path, paths);
                    path.pop();
                }
            }
            MathContent::Environment(environment) => {
                collect_math_text_paths(&environment.children, true, path, paths);
            }
            MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
        }
        path.pop();
    }
}

#[test]
fn session_caret_comparison_orders_valid_text_positions_and_rejects_stale_snapshots() {
    let first = Paragraph::empty(NodeId::generate());
    let first_id = first.id.clone();
    let second = Paragraph::empty(NodeId::generate());
    let second_id = second.id.clone();
    let mut session = Session::new(Document {
        body: vec![Block::Paragraph(first), Block::Paragraph(second)],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let text_position = |paragraph_id| {
        DocumentCaretPosition::Text(Pos {
            story: StoryId::Body,
            paragraph_id,
            offset: 0,
            affinity: Affinity::Downstream,
        })
    };
    let first = caret_snapshot(&session, text_position(first_id));
    let second = caret_snapshot(&session, text_position(second_id));
    assert_eq!(
        session.compare_caret_snapshots(&first, &second),
        Ok(std::cmp::Ordering::Less)
    );

    let mut stale = second.clone();
    stale.document_generation += 1;
    assert_eq!(
        session.compare_caret_snapshots(&first, &stale),
        Err(EditError::InvalidPosition)
    );
    session.document_generation += 1;
    assert_eq!(
        session.compare_caret_snapshots(&first, &second),
        Err(EditError::InvalidPosition)
    );
}

#[test]
fn session_caret_comparison_orders_two_edges_of_a_valid_block_host() {
    let equation_id = NodeId::generate();
    let paragraph = Paragraph::empty(NodeId::generate());
    let paragraph_id = paragraph.id.clone();
    let session = Session::new(Document {
        body: vec![
            Block::Equation(caret_doc::Equation {
                id: equation_id.clone(),
                math_tree: caret_doc::MathTree {
                    schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                    root: caret_doc::MathFlow {
                        id: NodeId::generate(),
                        content: Vec::new(),
                        extra: Default::default(),
                    },
                    extra: Default::default(),
                },
                extra: Default::default(),
            }),
            Block::Paragraph(paragraph),
        ],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let before = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::Before {
            host_id: equation_id.clone(),
            kind: HostCaretKind::Block,
        }),
    );
    let after = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::After {
            host_id: equation_id,
            kind: HostCaretKind::Block,
        }),
    );
    assert_eq!(
        session.compare_caret_snapshots(&before, &after),
        Ok(std::cmp::Ordering::Less)
    );
    let text_start_upstream = caret_snapshot(
        &session,
        DocumentCaretPosition::Text(Pos {
            story: StoryId::Body,
            paragraph_id,
            offset: 0,
            affinity: Affinity::Upstream,
        }),
    );
    assert_eq!(
        session.compare_caret_snapshots(&before, &text_start_upstream),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&after, &text_start_upstream),
        Ok(std::cmp::Ordering::Equal)
    );
}

#[test]
fn session_caret_comparison_preserves_order_through_an_empty_paragraph() {
    let first_id = NodeId::generate();
    let last_id = NodeId::generate();
    let empty_paragraph_id = NodeId::generate();
    let equation = |id| {
        Block::Equation(caret_doc::Equation {
            id,
            math_tree: caret_doc::MathTree {
                schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                root: caret_doc::MathFlow {
                    id: NodeId::generate(),
                    content: Vec::new(),
                    extra: Default::default(),
                },
                extra: Default::default(),
            },
            extra: Default::default(),
        })
    };
    let session = Session::new(Document {
        body: vec![
            equation(first_id.clone()),
            Block::Paragraph(Paragraph::empty(empty_paragraph_id.clone())),
            equation(last_id.clone()),
        ],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let host_edge = |host_id, after| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(if after {
                HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Block,
                }
            } else {
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Block,
                }
            }),
        )
    };
    let paragraph_caret = |affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Text(Pos {
                story: StoryId::Body,
                paragraph_id: empty_paragraph_id.clone(),
                offset: 0,
                affinity,
            }),
        )
    };
    let first_after = host_edge(first_id, true);
    let empty_upstream = paragraph_caret(Affinity::Upstream);
    let empty_downstream = paragraph_caret(Affinity::Downstream);
    let last_before = host_edge(last_id, false);
    assert_eq!(
        session.compare_caret_snapshots(&first_after, &empty_upstream),
        Ok(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        session.compare_caret_snapshots(&empty_upstream, &empty_downstream),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&empty_downstream, &last_before),
        Ok(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_after, &last_before),
        Ok(std::cmp::Ordering::Less)
    );
}

#[test]
fn session_caret_comparison_accepts_real_section_paragraph_focus() {
    let section_id = NodeId::generate();
    let child = Paragraph {
        inlines: vec![text_flow(Run {
            text: "正文".to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        })],
        ..Paragraph::empty(NodeId::generate())
    };
    let section = BlockEnvironment {
        id: section_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: Default::default(),
        title: None,
        children: vec![Block::Paragraph(child.clone())],
        extra: Default::default(),
    };
    let mut session = Session::new(Document {
        body: vec![Block::Environment(section)],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let focus = Pos {
        story: StoryId::Body,
        paragraph_id: child.id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    session.selection.focus = focus.clone();
    let inside = session
        .current_caret_snapshot()
        .unwrap_or_else(|error| panic!("section focus snapshot failed: {error}"));
    assert!(matches!(
        inside.position(),
        DocumentCaretPosition::Host(HostCaretPosition::Inside {
            kind: HostCaretKind::Block,
            local: HostCaretLocalPosition::Paragraph(_),
            ..
        })
    ));
    assert_eq!(
        session.compare_caret_snapshots(&inside, &inside),
        Ok(std::cmp::Ordering::Equal)
    );
    let outside_before = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::Before {
            host_id: section_id.clone(),
            kind: HostCaretKind::Block,
        }),
    );
    let outside_after = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::After {
            host_id: section_id,
            kind: HostCaretKind::Block,
        }),
    );
    assert_eq!(
        session.compare_caret_snapshots(&outside_before, &inside),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&inside, &outside_after),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &inside,
            &caret_snapshot(&session, DocumentCaretPosition::Text(focus))
        ),
        Ok(std::cmp::Ordering::Equal)
    );
}

#[test]
fn session_caret_comparison_uses_the_validated_math_tree_adapter() {
    let block_id = NodeId::generate();
    let tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: ["a", "b"]
                .into_iter()
                .map(|text| {
                    caret_doc::MathContent::Math(caret_doc::MathNode {
                        id: NodeId::generate(),
                        kind: caret_doc::MathNodeKind::Symbol,
                        text: Some(text.to_owned()),
                        attributes: Default::default(),
                        children: Vec::new(),
                        extra: Default::default(),
                    })
                })
                .collect(),
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    let mut session = Session::new(Document {
        body: vec![
            Block::Equation(caret_doc::Equation {
                id: block_id.clone(),
                math_tree: tree,
                extra: Default::default(),
            }),
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
        ],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let target = MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let path = MathSequencePath::root();
    let anchor = MathTreePosition {
        host: MathPositionHost::Equation { owner_id: block_id },
        path: path.clone(),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    session.math_edit = Some(MathEdit {
        target,
        source_projection: None,
        source_draft: None,
        revision: session.current_math_edit_revision(),
        cursor: 0,
        branch: MathEditBranch::Tree { path, anchor },
    });
    let start = session
        .current_caret_snapshot()
        .unwrap_or_else(|error| panic!("math caret snapshot failed: {error}"));
    if let Some(edit) = session.math_edit.as_mut() {
        edit.cursor = 2;
    } else {
        panic!("math edit state should remain active");
    }
    let end = session
        .current_caret_snapshot()
        .unwrap_or_else(|error| panic!("math caret snapshot failed: {error}"));
    assert_eq!(
        session.compare_caret_snapshots(&start, &end),
        Ok(std::cmp::Ordering::Less)
    );
}

#[test]
fn session_caret_comparison_orders_distinct_math_text_flows_in_one_island() {
    let block_id = NodeId::generate();
    let text_flow = |text: &str| {
        caret_doc::MathContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: text.to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }],
            extra: Default::default(),
        })
    };
    let tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![
                caret_doc::MathContent::Math(caret_doc::MathNode {
                    id: NodeId::generate(),
                    kind: caret_doc::MathNodeKind::Symbol,
                    text: Some("x".to_owned()),
                    attributes: Default::default(),
                    children: Vec::new(),
                    extra: Default::default(),
                }),
                text_flow("a"),
                text_flow("b"),
            ],
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![
            Block::Equation(caret_doc::Equation {
                id: block_id.clone(),
                math_tree: tree,
                extra: Default::default(),
            }),
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
        ],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let target = MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let caret = |index, offset, affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: block_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: target.clone(),
                    coordinate: MathCaretCoordinate::TextFlowUtf8 {
                        path: MathTextFlowPath::from_interaction_path(
                            vec![MathTextFlowPathStep::Content(index)],
                            None,
                        ),
                        offset,
                        affinity,
                    },
                }),
            }),
        )
    };
    let first_start = caret(1, 0, MathPositionAffinity::Downstream);
    let first_end = caret(1, 1, MathPositionAffinity::Upstream);
    let second_start = caret(2, 0, MathPositionAffinity::Downstream);
    let second_end = caret(2, 1, MathPositionAffinity::Upstream);
    let tree_boundary = |index, affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: block_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: target.clone(),
                    coordinate: MathCaretCoordinate::TreeBoundary {
                        host: MathPositionHost::Equation {
                            owner_id: block_id.clone(),
                        },
                        path: MathSequencePath::root(),
                        index,
                        affinity,
                    },
                }),
            }),
        )
    };

    assert_eq!(
        session.compare_caret_snapshots(&first_start, &second_end),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_end, &second_start),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&second_end, &first_start),
        Ok(std::cmp::Ordering::Greater)
    );
    let first_end_upstream = caret(1, 1, MathPositionAffinity::Upstream);
    let second_start_upstream = caret(2, 0, MathPositionAffinity::Upstream);
    assert_eq!(
        session.compare_caret_snapshots(&first_end_upstream, &second_start_upstream),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &tree_boundary(2, MathPositionAffinity::Upstream),
            &first_end_upstream,
        ),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &tree_boundary(2, MathPositionAffinity::Upstream),
            &second_start_upstream,
        ),
        Err(EditError::InvalidPosition)
    );
    let first_end_downstream = caret(1, 1, MathPositionAffinity::Downstream);
    let second_start_downstream = caret(2, 0, MathPositionAffinity::Downstream);
    assert_eq!(
        session.compare_caret_snapshots(
            &tree_boundary(2, MathPositionAffinity::Downstream),
            &first_end_downstream,
        ),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &tree_boundary(2, MathPositionAffinity::Downstream),
            &second_start_downstream,
        ),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &tree_boundary(0, MathPositionAffinity::Upstream),
            &caret(1, 1, MathPositionAffinity::Upstream),
        ),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &caret(1, 1, MathPositionAffinity::Upstream),
            &tree_boundary(0, MathPositionAffinity::Upstream),
        ),
        Ok(std::cmp::Ordering::Greater)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &tree_boundary(3, MathPositionAffinity::Downstream),
            &caret(2, 0, MathPositionAffinity::Downstream),
        ),
        Ok(std::cmp::Ordering::Greater)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &caret(2, 0, MathPositionAffinity::Downstream),
            &tree_boundary(3, MathPositionAffinity::Downstream),
        ),
        Ok(std::cmp::Ordering::Less)
    );
}

#[test]
fn session_caret_comparison_orders_text_islands_by_shared_math_flow_sequence() {
    let block_id = NodeId::generate();
    let text_flow = |text: &str| {
        caret_doc::MathContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: text.to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }],
            extra: Default::default(),
        })
    };
    let tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![
                text_flow("é"),
                caret_doc::MathContent::Math(caret_doc::MathNode {
                    id: NodeId::generate(),
                    kind: caret_doc::MathNodeKind::Symbol,
                    text: Some("+".to_owned()),
                    attributes: Default::default(),
                    children: Vec::new(),
                    extra: Default::default(),
                }),
                text_flow("b"),
            ],
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![
            Block::Equation(caret_doc::Equation {
                id: block_id.clone(),
                math_tree: tree,
                extra: Default::default(),
            }),
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
        ],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let target = MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let caret = |index, offset, affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: block_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: target.clone(),
                    coordinate: MathCaretCoordinate::TextFlowUtf8 {
                        path: MathTextFlowPath::from_interaction_path(
                            vec![MathTextFlowPathStep::Content(index)],
                            None,
                        ),
                        offset,
                        affinity,
                    },
                }),
            }),
        )
    };
    let tree_boundary = |index, affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: block_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: target.clone(),
                    coordinate: MathCaretCoordinate::TreeBoundary {
                        host: MathPositionHost::Equation {
                            owner_id: block_id.clone(),
                        },
                        path: MathSequencePath::root(),
                        index,
                        affinity,
                    },
                }),
            }),
        )
    };
    let before_separator = caret(0, 2, MathPositionAffinity::Upstream);
    let after_separator = caret(2, 0, MathPositionAffinity::Downstream);
    assert_eq!(
        session.compare_caret_snapshots(&before_separator, &after_separator),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&after_separator, &before_separator),
        Ok(std::cmp::Ordering::Greater)
    );
    let boundary_after_separator = tree_boundary(2, MathPositionAffinity::Upstream);
    let after_separator_upstream = caret(2, 0, MathPositionAffinity::Upstream);
    assert_eq!(
        session.compare_caret_snapshots(&before_separator, &boundary_after_separator),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&boundary_after_separator, &after_separator_upstream),
        Ok(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        session.compare_caret_snapshots(&before_separator, &after_separator_upstream),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &caret(0, 0, MathPositionAffinity::Downstream),
            &caret(2, 1, MathPositionAffinity::Upstream),
        ),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &caret(0, 1, MathPositionAffinity::Downstream),
            &after_separator,
        ),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &before_separator,
            &caret(2, 2, MathPositionAffinity::Upstream),
        ),
        Err(EditError::InvalidPosition)
    );
}

#[test]
fn session_caret_comparison_orders_empty_math_text_island_edges_by_structure() {
    let block_id = NodeId::generate();
    let tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![
                caret_doc::MathContent::Text(caret_doc::TextFlow::default()),
                caret_doc::MathContent::SoftBreak(caret_doc::SoftBreak::default()),
                caret_doc::MathContent::Text(caret_doc::TextFlow {
                    runs: vec![Run {
                        text: "b".to_owned(),
                        language: None,
                        props: Default::default(),
                        extra: Default::default(),
                    }],
                    extra: Default::default(),
                }),
            ],
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![
            Block::Equation(caret_doc::Equation {
                id: block_id.clone(),
                math_tree: tree,
                extra: Default::default(),
            }),
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
        ],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let target = MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let caret = |index, offset, affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: block_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: target.clone(),
                    coordinate: MathCaretCoordinate::TextFlowUtf8 {
                        path: MathTextFlowPath::from_interaction_path(
                            vec![MathTextFlowPathStep::Content(index)],
                            None,
                        ),
                        offset,
                        affinity,
                    },
                }),
            }),
        )
    };
    let empty_upstream = caret(0, 0, MathPositionAffinity::Upstream);
    let empty_downstream = caret(0, 0, MathPositionAffinity::Downstream);
    let next_start = caret(2, 0, MathPositionAffinity::Upstream);
    let next_end = caret(2, 1, MathPositionAffinity::Downstream);

    assert_eq!(
        session.compare_caret_snapshots(&empty_upstream, &next_start),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&empty_downstream, &next_end),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&next_start, &empty_downstream),
        Ok(std::cmp::Ordering::Greater)
    );
}

#[test]
fn session_caret_comparison_orders_cross_operand_text_islands_by_tree_order() {
    let block_id = NodeId::generate();
    let flow = |text: &str| caret_doc::MathFlow {
        id: NodeId::generate(),
        content: vec![caret_doc::MathContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: text.to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }],
            extra: Default::default(),
        })],
        extra: Default::default(),
    };
    let tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![caret_doc::MathContent::Math(caret_doc::MathNode {
                id: NodeId::generate(),
                kind: caret_doc::MathNodeKind::Fraction,
                text: None,
                attributes: std::collections::BTreeMap::from([(
                    "style".to_owned(),
                    "frac".to_owned(),
                )]),
                children: vec![flow("numerator"), flow("denominator")],
                extra: Default::default(),
            })],
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![
            Block::Equation(caret_doc::Equation {
                id: block_id.clone(),
                math_tree: tree,
                extra: Default::default(),
            }),
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
        ],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let target = MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let caret = |operand_index| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: block_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: target.clone(),
                    coordinate: MathCaretCoordinate::TextFlowUtf8 {
                        path: MathTextFlowPath::from_interaction_path(
                            vec![
                                MathTextFlowPathStep::Content(0),
                                MathTextFlowPathStep::Operand(operand_index),
                                MathTextFlowPathStep::Content(0),
                            ],
                            None,
                        ),
                        offset: 0,
                        affinity: MathPositionAffinity::Downstream,
                    },
                }),
            }),
        )
    };
    let root_boundary = |index| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: block_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: target.clone(),
                    coordinate: MathCaretCoordinate::TreeBoundary {
                        host: MathPositionHost::Equation {
                            owner_id: block_id.clone(),
                        },
                        path: MathSequencePath::root(),
                        index,
                        affinity: MathPositionAffinity::Upstream,
                    },
                }),
            }),
        )
    };

    assert_eq!(
        session.compare_caret_snapshots(&caret(0), &caret(1)),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&caret(1), &caret(0)),
        Ok(std::cmp::Ordering::Greater)
    );
    assert_eq!(
        session.compare_caret_snapshots(&root_boundary(0), &caret(0)),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&caret(1), &root_boundary(1)),
        Ok(std::cmp::Ordering::Less)
    );
}

#[test]
fn session_caret_comparison_uses_parsed_fraction_operand_order()
-> Result<(), Box<dyn std::error::Error>> {
    let parsed = caret_math::parse_projection(
        r"\frac{\text{n}}{\text{d}}",
        caret_math::MathInputKind::Equation,
    )?;
    let (caret_math::ProjectedMath::Tree(tree), _) = parsed.into_parts() else {
        return Err("fraction should parse to a MathTree".into());
    };
    let mut paths = Vec::new();
    collect_math_text_paths(&tree.root.content, false, &mut Vec::new(), &mut paths);
    let numerator_path = paths
        .iter()
        .find(|(path, text)| path.contains(&MathTextFlowPathStep::Operand(0)) && text == "n")
        .map(|(path, _)| path.clone())
        .ok_or("parsed fraction has no numerator TextFlow")?;
    let denominator_path = paths
        .iter()
        .find(|(path, text)| path.contains(&MathTextFlowPathStep::Operand(1)) && text == "d")
        .map(|(path, _)| path.clone())
        .ok_or("parsed fraction has no denominator TextFlow")?;

    let block_id = NodeId::generate();
    let session = Session::new(Document {
        body: vec![
            Block::Equation(caret_doc::Equation {
                id: block_id.clone(),
                math_tree: tree,
                extra: Default::default(),
            }),
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
        ],
        ..Document::blank()
    })?;
    let target = MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let caret = |path| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: block_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: target.clone(),
                    coordinate: MathCaretCoordinate::TextFlowUtf8 {
                        path: MathTextFlowPath::from_interaction_path(path, None),
                        offset: 0,
                        affinity: MathPositionAffinity::Downstream,
                    },
                }),
            }),
        )
    };

    assert_eq!(
        session.compare_caret_snapshots(&caret(numerator_path), &caret(denominator_path)),
        Ok(std::cmp::Ordering::Less)
    );
    Ok(())
}

#[test]
fn session_caret_comparison_orders_math_tree_boundaries_with_text_flow_positions() {
    let block_id = NodeId::generate();
    let tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![caret_doc::MathContent::Text(caret_doc::TextFlow {
                runs: vec![Run {
                    text: "abc".to_owned(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                }],
                extra: Default::default(),
            })],
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![
            Block::Equation(caret_doc::Equation {
                id: block_id.clone(),
                math_tree: tree,
                extra: Default::default(),
            }),
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
        ],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let target = MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let text_path =
        MathTextFlowPath::from_interaction_path(vec![MathTextFlowPathStep::Content(0)], None);
    let tree_boundary = |index, affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: block_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: target.clone(),
                    coordinate: MathCaretCoordinate::TreeBoundary {
                        host: MathPositionHost::Equation {
                            owner_id: block_id.clone(),
                        },
                        path: MathSequencePath::root(),
                        index,
                        affinity,
                    },
                }),
            }),
        )
    };
    let text_position = |offset, affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: block_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: target.clone(),
                    coordinate: MathCaretCoordinate::TextFlowUtf8 {
                        path: text_path.clone(),
                        offset,
                        affinity,
                    },
                }),
            }),
        )
    };

    assert_eq!(
        session.compare_caret_snapshots(
            &tree_boundary(0, MathPositionAffinity::Downstream),
            &text_position(1, MathPositionAffinity::Downstream),
        ),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &text_position(1, MathPositionAffinity::Downstream),
            &tree_boundary(1, MathPositionAffinity::Downstream),
        ),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &tree_boundary(0, MathPositionAffinity::Downstream),
            &text_position(0, MathPositionAffinity::Downstream),
        ),
        Ok(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        session.compare_caret_snapshots(
            &tree_boundary(1, MathPositionAffinity::Downstream),
            &text_position(3, MathPositionAffinity::Upstream),
        ),
        Ok(std::cmp::Ordering::Greater)
    );
}

#[test]
fn session_caret_comparison_keeps_empty_math_text_positions_transitive() {
    let block_id = NodeId::generate();
    let tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![caret_doc::MathContent::Text(caret_doc::TextFlow {
                runs: Vec::new(),
                extra: Default::default(),
            })],
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![
            Block::Equation(caret_doc::Equation {
                id: block_id.clone(),
                math_tree: tree,
                extra: Default::default(),
            }),
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
        ],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let target = MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let text_path =
        MathTextFlowPath::from_interaction_path(vec![MathTextFlowPathStep::Content(0)], None);
    let tree_boundary = |index, affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: block_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: target.clone(),
                    coordinate: MathCaretCoordinate::TreeBoundary {
                        host: MathPositionHost::Equation {
                            owner_id: block_id.clone(),
                        },
                        path: MathSequencePath::root(),
                        index,
                        affinity,
                    },
                }),
            }),
        )
    };
    let empty_text = |affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: block_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: target.clone(),
                    coordinate: MathCaretCoordinate::TextFlowUtf8 {
                        path: text_path.clone(),
                        offset: 0,
                        affinity,
                    },
                }),
            }),
        )
    };
    let before_upstream = tree_boundary(0, MathPositionAffinity::Upstream);
    let before_downstream = tree_boundary(0, MathPositionAffinity::Downstream);
    let after_upstream = tree_boundary(1, MathPositionAffinity::Upstream);
    let after_downstream = tree_boundary(1, MathPositionAffinity::Downstream);
    let empty_upstream = empty_text(MathPositionAffinity::Upstream);
    let empty_downstream = empty_text(MathPositionAffinity::Downstream);

    assert_eq!(
        session.compare_caret_snapshots(&before_upstream, &empty_upstream),
        Ok(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        session.compare_caret_snapshots(&before_downstream, &empty_downstream),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&empty_upstream, &after_upstream),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&empty_downstream, &after_downstream),
        Ok(std::cmp::Ordering::Equal)
    );
}

#[test]
fn session_caret_comparison_orders_block_math_with_surrounding_document_positions() {
    let section_id = NodeId::generate();
    let first_equation_id = NodeId::generate();
    let second_equation_id = NodeId::generate();
    let inline_math_id = NodeId::generate();
    let before_paragraph = Paragraph {
        inlines: vec![text_flow(Run {
            text: "a".to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        })],
        ..Paragraph::empty(NodeId::generate())
    };
    let before_paragraph_id = before_paragraph.id.clone();
    let between_paragraph = Paragraph {
        inlines: vec![
            text_flow(Run {
                text: "b".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }),
            ParagraphContent::Math(caret_doc::InlineMath {
                id: inline_math_id.clone(),
                math_tree: caret_doc::MathTree {
                    schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                    root: caret_doc::MathFlow {
                        id: NodeId::generate(),
                        content: Vec::new(),
                        extra: Default::default(),
                    },
                    extra: Default::default(),
                },
                extra: Default::default(),
            }),
        ],
        ..Paragraph::empty(NodeId::generate())
    };
    let between_paragraph_id = between_paragraph.id.clone();
    let tree = || caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![caret_doc::MathContent::Math(caret_doc::MathNode {
                id: NodeId::generate(),
                kind: caret_doc::MathNodeKind::Symbol,
                text: Some("x".to_owned()),
                attributes: Default::default(),
                children: Vec::new(),
                extra: Default::default(),
            })],
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    let section = BlockEnvironment {
        id: section_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: Default::default(),
        title: None,
        children: vec![
            Block::Paragraph(before_paragraph),
            Block::Equation(caret_doc::Equation {
                id: first_equation_id.clone(),
                math_tree: tree(),
                extra: Default::default(),
            }),
            Block::Paragraph(between_paragraph),
            Block::Equation(caret_doc::Equation {
                id: second_equation_id.clone(),
                math_tree: tree(),
                extra: Default::default(),
            }),
        ],
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![Block::Environment(section)],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let math_inside = |equation_id: NodeId| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: equation_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: MathEditTarget::Block {
                        block_id: equation_id.clone(),
                        align: false,
                    },
                    coordinate: MathCaretCoordinate::TreeBoundary {
                        host: MathPositionHost::Equation {
                            owner_id: equation_id,
                        },
                        path: MathSequencePath::root(),
                        index: 0,
                        affinity: MathPositionAffinity::Downstream,
                    },
                }),
            }),
        )
    };
    let inline_math_inside = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::Inside {
            host_id: inline_math_id.clone(),
            kind: HostCaretKind::Inline,
            local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                target: MathEditTarget::Inline {
                    paragraph_id: between_paragraph_id.clone(),
                    math_id: inline_math_id.clone(),
                },
                coordinate: MathCaretCoordinate::TreeBoundary {
                    host: MathPositionHost::InlineMath {
                        owner_id: inline_math_id.clone(),
                    },
                    path: MathSequencePath::root(),
                    index: 0,
                    affinity: MathPositionAffinity::Downstream,
                },
            }),
        }),
    );
    let text_position = |paragraph_id, offset| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Text(Pos {
                story: StoryId::Body,
                paragraph_id,
                offset,
                affinity: Affinity::Downstream,
            }),
        )
    };
    let section_before = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::Before {
            host_id: section_id.clone(),
            kind: HostCaretKind::Block,
        }),
    );
    let section_after = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::After {
            host_id: section_id,
            kind: HostCaretKind::Block,
        }),
    );
    let before_text = text_position(before_paragraph_id, 1);
    let between_text = text_position(between_paragraph_id, 0);
    let first_math = math_inside(first_equation_id.clone());
    let second_math = math_inside(second_equation_id.clone());
    let second_before = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::Before {
            host_id: second_equation_id,
            kind: HostCaretKind::Block,
        }),
    );
    assert_eq!(
        session.compare_caret_snapshots(&section_before, &first_math),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&before_text, &first_math),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_math, &between_text),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_math, &second_before),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_math, &second_math),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_math, &inline_math_inside),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&inline_math_inside, &second_math),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&second_math, &section_after),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_math, &before_text),
        Ok(std::cmp::Ordering::Greater)
    );
}

#[test]
fn session_caret_comparison_orders_align_cells_by_row_and_column() {
    let align_id = NodeId::generate();
    let first_row_id = NodeId::generate();
    let second_row_id = NodeId::generate();
    let first_cell_id = NodeId::generate();
    let second_cell_id = NodeId::generate();
    let third_cell_id = NodeId::generate();
    let math_tree = |root_id| caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: root_id,
            content: vec![caret_doc::MathContent::Math(caret_doc::MathNode {
                id: NodeId::generate(),
                kind: caret_doc::MathNodeKind::Symbol,
                text: Some("x".to_owned()),
                attributes: Default::default(),
                children: Vec::new(),
                extra: Default::default(),
            })],
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    let mut session = Session::new(Document {
        body: vec![
            Block::Align(caret_doc::Align {
                id: align_id.clone(),
                rows: vec![
                    caret_doc::MathAlignRow {
                        id: first_row_id.clone(),
                        cells: vec![
                            math_tree(first_cell_id.clone()),
                            math_tree(second_cell_id.clone()),
                        ],
                        extra: Default::default(),
                    },
                    caret_doc::MathAlignRow {
                        id: second_row_id.clone(),
                        cells: vec![math_tree(third_cell_id.clone())],
                        extra: Default::default(),
                    },
                ],
                extra: Default::default(),
            }),
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
        ],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let cell_caret = |row_id, cell_id| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: align_id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: MathEditTarget::Block {
                        block_id: align_id.clone(),
                        align: true,
                    },
                    coordinate: MathCaretCoordinate::TreeBoundary {
                        host: MathPositionHost::AlignCell { row_id, cell_id },
                        path: MathSequencePath::root(),
                        index: 0,
                        affinity: MathPositionAffinity::Downstream,
                    },
                }),
            }),
        )
    };
    let first = cell_caret(first_row_id.clone(), first_cell_id.clone());
    let second = cell_caret(first_row_id.clone(), second_cell_id.clone());
    let third = cell_caret(second_row_id.clone(), third_cell_id.clone());
    assert_eq!(
        session.compare_caret_snapshots(&first, &second),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&second, &third),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&third, &first),
        Ok(std::cmp::Ordering::Greater)
    );

    let target = MathEditTarget::Block {
        block_id: align_id,
        align: true,
    };
    let root_path = MathSequencePath::root();
    session.math_edit = Some(MathEdit {
        target: target.clone(),
        source_projection: None,
        source_draft: None,
        revision: session.current_math_edit_revision(),
        cursor: 0,
        branch: MathEditBranch::Tree {
            path: root_path.clone(),
            anchor: MathTreePosition {
                host: MathPositionHost::AlignCell {
                    row_id: first_row_id.clone(),
                    cell_id: first_cell_id,
                },
                path: root_path.clone(),
                offset: 0,
                affinity: MathPositionAffinity::Downstream,
            },
        },
    });
    let first_current = session
        .current_caret_snapshot()
        .unwrap_or_else(|error| panic!("Align 首单元格焦点快照失败：{error}"));
    session.math_edit = Some(MathEdit {
        target,
        source_projection: None,
        source_draft: None,
        revision: session.current_math_edit_revision(),
        cursor: 0,
        branch: MathEditBranch::Tree {
            path: root_path.clone(),
            anchor: MathTreePosition {
                host: MathPositionHost::AlignCell {
                    row_id: second_row_id,
                    cell_id: third_cell_id,
                },
                path: root_path,
                offset: 0,
                affinity: MathPositionAffinity::Downstream,
            },
        },
    });
    let third_current = session
        .current_caret_snapshot()
        .unwrap_or_else(|error| panic!("Align 末单元格焦点快照失败：{error}"));
    assert_eq!(
        session.compare_caret_snapshots(&first_current, &third_current),
        Ok(std::cmp::Ordering::Less)
    );
}

#[test]
fn session_caret_comparison_rejects_ambiguous_empty_inline_host_boundaries() {
    let paragraph_id = NodeId::generate();
    let outer_id = NodeId::generate();
    let inner_id = NodeId::generate();
    let inner = InlineEnvironment {
        id: inner_id.clone(),
        kind: InlineEnvironmentKind::Italic,
        definition_id: None,
        props: Default::default(),
        children: Vec::new(),
        extra: Default::default(),
    };
    let paragraph = Paragraph {
        id: paragraph_id.clone(),
        inlines: vec![ParagraphContent::Environment(InlineEnvironment {
            id: outer_id.clone(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: Default::default(),
            children: vec![ParagraphContent::Environment(inner)],
            extra: Default::default(),
        })],
        language: None,
        layout: None,
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![Block::Paragraph(paragraph)],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let empty_position = |host_id| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id,
                kind: HostCaretKind::Inline,
                local: HostCaretLocalPosition::Paragraph(Pos {
                    story: StoryId::Body,
                    paragraph_id: paragraph_id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                }),
            }),
        )
    };
    assert_eq!(
        session
            .compare_caret_snapshots(&empty_position(outer_id.clone()), &empty_position(inner_id),),
        Err(EditError::InvalidPosition)
    );
    let before_outer = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::Before {
            host_id: outer_id.clone(),
            kind: HostCaretKind::Inline,
        }),
    );
    let after_outer = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::After {
            host_id: outer_id,
            kind: HostCaretKind::Inline,
        }),
    );
    assert_eq!(
        session.compare_caret_snapshots(&before_outer, &after_outer),
        Ok(std::cmp::Ordering::Less)
    );
}

#[test]
fn session_caret_comparison_orders_empty_inline_edges_by_affinity() {
    let paragraph_id = NodeId::generate();
    let environment_id = NodeId::generate();
    let paragraph = Paragraph {
        id: paragraph_id.clone(),
        inlines: vec![ParagraphContent::Environment(InlineEnvironment {
            id: environment_id.clone(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: Default::default(),
            children: Vec::new(),
            extra: Default::default(),
        })],
        language: None,
        layout: None,
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![Block::Paragraph(paragraph)],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let external_edge = |before| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(if before {
                HostCaretPosition::Before {
                    host_id: environment_id.clone(),
                    kind: HostCaretKind::Inline,
                }
            } else {
                HostCaretPosition::After {
                    host_id: environment_id.clone(),
                    kind: HostCaretKind::Inline,
                }
            }),
        )
    };
    let text = |affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Text(Pos {
                story: StoryId::Body,
                paragraph_id: paragraph_id.clone(),
                offset: 0,
                affinity,
            }),
        )
    };
    let before = external_edge(true);
    let after = external_edge(false);
    let upstream = text(Affinity::Upstream);
    let downstream = text(Affinity::Downstream);
    assert_eq!(
        session.compare_caret_snapshots(&before, &upstream),
        Ok(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        session.compare_caret_snapshots(&before, &downstream),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&upstream, &downstream),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&downstream, &after),
        Ok(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        session.compare_caret_snapshots(&before, &after),
        Ok(std::cmp::Ordering::Less)
    );
}

#[test]
fn session_caret_comparison_rejects_ambiguous_nested_inline_edges() {
    let paragraph_id = NodeId::generate();
    let outer_id = NodeId::generate();
    let inner_id = NodeId::generate();
    let later_math_id = NodeId::generate();
    let paragraph = Paragraph {
        id: paragraph_id.clone(),
        inlines: vec![
            ParagraphContent::Environment(InlineEnvironment {
                id: outer_id.clone(),
                kind: InlineEnvironmentKind::Bold,
                definition_id: None,
                props: Default::default(),
                children: vec![ParagraphContent::Environment(InlineEnvironment {
                    id: inner_id.clone(),
                    kind: InlineEnvironmentKind::Italic,
                    definition_id: None,
                    props: Default::default(),
                    children: vec![text_flow(Run {
                        text: "x".to_owned(),
                        language: None,
                        props: Default::default(),
                        extra: Default::default(),
                    })],
                    extra: Default::default(),
                })],
                extra: Default::default(),
            }),
            ParagraphContent::Math(caret_doc::InlineMath {
                id: later_math_id.clone(),
                math_tree: caret_doc::MathTree {
                    schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                    root: caret_doc::MathFlow {
                        id: NodeId::generate(),
                        content: Vec::new(),
                        extra: Default::default(),
                    },
                    extra: Default::default(),
                },
                extra: Default::default(),
            }),
        ],
        language: None,
        layout: None,
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![Block::Paragraph(paragraph)],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let edge = |host_id, before| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(if before {
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
            } else {
                HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
            }),
        )
    };
    let outer_before = edge(outer_id.clone(), true);
    let inner_before = edge(inner_id.clone(), true);
    let inner_after = edge(inner_id, false);
    let outer_after = edge(outer_id, false);
    assert_eq!(
        session.compare_caret_snapshots(&outer_before, &inner_before),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&inner_after, &outer_after),
        Ok(std::cmp::Ordering::Less)
    );
    let text_start = caret_snapshot(
        &session,
        DocumentCaretPosition::Text(Pos {
            story: StoryId::Body,
            paragraph_id: paragraph_id.clone(),
            offset: 0,
            affinity: Affinity::Upstream,
        }),
    );
    let text_end = caret_snapshot(
        &session,
        DocumentCaretPosition::Text(Pos {
            story: StoryId::Body,
            paragraph_id,
            offset: 1,
            affinity: Affinity::Downstream,
        }),
    );
    assert_eq!(
        session.compare_caret_snapshots(&outer_before, &text_start),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(
        session.compare_caret_snapshots(&inner_before, &text_start),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(
        session.compare_caret_snapshots(&inner_after, &text_end),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(
        session.compare_caret_snapshots(&outer_after, &text_end),
        Err(EditError::InvalidPosition)
    );
    let later_math_before = edge(later_math_id, true);
    assert_eq!(
        session.compare_caret_snapshots(&text_start, &later_math_before),
        Ok(std::cmp::Ordering::Less)
    );
}

#[test]
fn session_caret_comparison_does_not_merge_adjacent_inline_host_edges() {
    let paragraph_id = NodeId::generate();
    let first_math_id = NodeId::generate();
    let second_math_id = NodeId::generate();
    let empty_tree = || caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: Vec::new(),
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    let paragraph = Paragraph {
        id: paragraph_id.clone(),
        inlines: vec![
            ParagraphContent::Math(caret_doc::InlineMath {
                id: first_math_id.clone(),
                math_tree: empty_tree(),
                extra: Default::default(),
            }),
            ParagraphContent::Math(caret_doc::InlineMath {
                id: second_math_id.clone(),
                math_tree: empty_tree(),
                extra: Default::default(),
            }),
        ],
        language: None,
        layout: None,
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![Block::Paragraph(paragraph)],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let edge = |host_id, after| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(if after {
                HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
            } else {
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
            }),
        )
    };
    let math_inside = |math_id: NodeId| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: math_id.clone(),
                kind: HostCaretKind::Inline,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: MathEditTarget::Inline {
                        paragraph_id: paragraph_id.clone(),
                        math_id: math_id.clone(),
                    },
                    coordinate: MathCaretCoordinate::TreeBoundary {
                        host: MathPositionHost::InlineMath { owner_id: math_id },
                        path: MathSequencePath::root(),
                        index: 0,
                        affinity: MathPositionAffinity::Downstream,
                    },
                }),
            }),
        )
    };
    let text = |affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Text(Pos {
                story: StoryId::Body,
                paragraph_id: paragraph_id.clone(),
                offset: 3,
                affinity,
            }),
        )
    };
    let first_inside = math_inside(first_math_id.clone());
    let first_after = edge(first_math_id, true);
    let second_before = edge(second_math_id.clone(), false);
    let second_inside = math_inside(second_math_id);
    let upstream = text(Affinity::Upstream);
    let downstream = text(Affinity::Downstream);
    assert_eq!(
        session.compare_caret_snapshots(&second_before, &upstream),
        Ok(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        session.compare_caret_snapshots(&downstream, &first_after),
        Ok(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        session.compare_caret_snapshots(&second_before, &first_after),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_inside, &second_before),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_after, &second_inside),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&second_before, &second_inside),
        Ok(std::cmp::Ordering::Less)
    );
}

#[test]
fn session_caret_comparison_rejects_ambiguous_empty_inline_inside_block_edge() {
    let equation_id = NodeId::generate();
    let paragraph_id = NodeId::generate();
    let environment_id = NodeId::generate();
    let paragraph = Paragraph {
        id: paragraph_id.clone(),
        inlines: vec![ParagraphContent::Environment(InlineEnvironment {
            id: environment_id.clone(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: Default::default(),
            children: Vec::new(),
            extra: Default::default(),
        })],
        language: None,
        layout: None,
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![
            Block::Equation(caret_doc::Equation {
                id: equation_id.clone(),
                math_tree: caret_doc::MathTree {
                    schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                    root: caret_doc::MathFlow {
                        id: NodeId::generate(),
                        content: Vec::new(),
                        extra: Default::default(),
                    },
                    extra: Default::default(),
                },
                extra: Default::default(),
            }),
            Block::Paragraph(paragraph),
        ],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let equation_after = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::After {
            host_id: equation_id,
            kind: HostCaretKind::Block,
        }),
    );
    let inline_inside = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::Inside {
            host_id: environment_id,
            kind: HostCaretKind::Inline,
            local: HostCaretLocalPosition::Paragraph(Pos {
                story: StoryId::Body,
                paragraph_id,
                offset: 0,
                affinity: Affinity::Upstream,
            }),
        }),
    );
    assert_eq!(
        session.compare_caret_snapshots(&equation_after, &inline_inside),
        Err(EditError::InvalidPosition)
    );
}

#[test]
fn session_caret_comparison_rejects_paragraph_coordinates_for_inline_math() {
    let paragraph_id = NodeId::generate();
    let math_id = NodeId::generate();
    let paragraph = Paragraph {
        id: paragraph_id.clone(),
        inlines: vec![ParagraphContent::Math(caret_doc::InlineMath {
            id: math_id.clone(),
            math_tree: caret_doc::MathTree {
                schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                root: caret_doc::MathFlow {
                    id: NodeId::generate(),
                    content: Vec::new(),
                    extra: Default::default(),
                },
                extra: Default::default(),
            },
            extra: Default::default(),
        })],
        language: None,
        layout: None,
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![Block::Paragraph(paragraph)],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let before = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::Before {
            host_id: math_id.clone(),
            kind: HostCaretKind::Inline,
        }),
    );
    let invalid_inside = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::Inside {
            host_id: math_id,
            kind: HostCaretKind::Inline,
            local: HostCaretLocalPosition::Paragraph(Pos {
                story: StoryId::Body,
                paragraph_id,
                offset: 0,
                affinity: Affinity::Downstream,
            }),
        }),
    );
    assert_eq!(
        session.compare_caret_snapshots(&before, &invalid_inside),
        Err(EditError::InvalidPosition)
    );
}

#[test]
fn session_caret_comparison_rejects_inline_edges_inside_a_grapheme() {
    let paragraph_id = NodeId::generate();
    let environment_id = NodeId::generate();
    let empty_environment_id = NodeId::generate();
    let paragraph = Paragraph {
        id: paragraph_id.clone(),
        inlines: vec![
            text_flow(Run {
                text: "a".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }),
            ParagraphContent::Environment(InlineEnvironment {
                id: empty_environment_id.clone(),
                kind: InlineEnvironmentKind::Italic,
                definition_id: None,
                props: Default::default(),
                children: Vec::new(),
                extra: Default::default(),
            }),
            text_flow(Run {
                text: "\u{301}b".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }),
            ParagraphContent::Environment(InlineEnvironment {
                id: environment_id.clone(),
                kind: InlineEnvironmentKind::Bold,
                definition_id: None,
                props: Default::default(),
                children: vec![text_flow(Run {
                    text: "\u{301}".to_owned(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                })],
                extra: Default::default(),
            }),
        ],
        language: None,
        layout: None,
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![Block::Paragraph(paragraph)],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let edge = |after| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(if after {
                HostCaretPosition::After {
                    host_id: environment_id.clone(),
                    kind: HostCaretKind::Inline,
                }
            } else {
                HostCaretPosition::Before {
                    host_id: environment_id.clone(),
                    kind: HostCaretKind::Inline,
                }
            }),
        )
    };
    let before = edge(false);
    let after = edge(true);
    assert_eq!(
        session.compare_caret_snapshots(&before, &after),
        Err(EditError::InvalidPosition)
    );
    let empty_before = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::Before {
            host_id: empty_environment_id.clone(),
            kind: HostCaretKind::Inline,
        }),
    );
    let empty_after = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::After {
            host_id: empty_environment_id,
            kind: HostCaretKind::Inline,
        }),
    );
    assert_eq!(
        session.compare_caret_snapshots(&empty_before, &empty_after),
        Err(EditError::InvalidPosition)
    );
}

#[test]
fn session_caret_comparison_orders_inline_math_with_paragraph_text_and_neighbors() {
    let paragraph_id = NodeId::generate();
    let first_math_id = NodeId::generate();
    let second_math_id = NodeId::generate();
    let empty_tree = || caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: Vec::new(),
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    let paragraph = Paragraph {
        id: paragraph_id.clone(),
        inlines: vec![
            text_flow(Run {
                text: "a".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }),
            ParagraphContent::Math(caret_doc::InlineMath {
                id: first_math_id.clone(),
                math_tree: empty_tree(),
                extra: Default::default(),
            }),
            text_flow(Run {
                text: "b".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }),
            ParagraphContent::Math(caret_doc::InlineMath {
                id: second_math_id.clone(),
                math_tree: empty_tree(),
                extra: Default::default(),
            }),
            text_flow(Run {
                text: "c".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }),
        ],
        language: None,
        layout: None,
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let session = Session::new(Document {
        body: vec![Block::Paragraph(paragraph)],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let math_inside = |math_id: NodeId| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: math_id.clone(),
                kind: HostCaretKind::Inline,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: MathEditTarget::Inline {
                        paragraph_id: paragraph_id.clone(),
                        math_id: math_id.clone(),
                    },
                    coordinate: MathCaretCoordinate::TreeBoundary {
                        host: MathPositionHost::InlineMath { owner_id: math_id },
                        path: MathSequencePath::root(),
                        index: 0,
                        affinity: MathPositionAffinity::Downstream,
                    },
                }),
            }),
        )
    };
    let text_position = |offset, affinity| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Text(Pos {
                story: StoryId::Body,
                paragraph_id: paragraph_id.clone(),
                offset,
                affinity,
            }),
        )
    };
    let text_before = text_position(0, Affinity::Downstream);
    let first_math = math_inside(first_math_id.clone());
    let text_between = text_position(4, Affinity::Downstream);
    let second_math = math_inside(second_math_id);
    assert_eq!(
        session.compare_caret_snapshots(&text_before, &first_math),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_math, &text_between),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_math, &second_math),
        Ok(std::cmp::Ordering::Less)
    );
    let first_before = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::Before {
            host_id: first_math_id.clone(),
            kind: HostCaretKind::Inline,
        }),
    );
    let text_at_first_start = text_position(1, Affinity::Upstream);
    assert_eq!(
        session.compare_caret_snapshots(&text_at_first_start, &first_before),
        Ok(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_before, &text_position(1, Affinity::Downstream)),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_before, &first_math),
        Ok(std::cmp::Ordering::Less)
    );
    let text_at_first_end_upstream = text_position(4, Affinity::Upstream);
    let text_at_first_end_downstream = text_position(4, Affinity::Downstream);
    let first_after = caret_snapshot(
        &session,
        DocumentCaretPosition::Host(HostCaretPosition::After {
            host_id: first_math_id,
            kind: HostCaretKind::Inline,
        }),
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_after, &first_math),
        Ok(std::cmp::Ordering::Greater)
    );
    assert_eq!(
        session.compare_caret_snapshots(&text_between, &first_after),
        Ok(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        session.compare_caret_snapshots(&first_math, &text_at_first_end_upstream),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&text_at_first_end_upstream, &text_at_first_end_downstream),
        Ok(std::cmp::Ordering::Less)
    );
    assert_eq!(
        session.compare_caret_snapshots(&text_at_first_end_downstream, &first_after),
        Ok(std::cmp::Ordering::Equal)
    );
}

#[test]
fn session_caret_comparison_orders_inline_math_inside_positions_with_other_host_edges() {
    let paragraph_id = NodeId::generate();
    let section_id = NodeId::generate();
    let first_math_id = NodeId::generate();
    let empty_environment_id = NodeId::generate();
    let outer_environment_id = NodeId::generate();
    let nested_math_id = NodeId::generate();
    let empty_tree = || caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: Vec::new(),
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    let math = |id: NodeId| {
        ParagraphContent::Math(caret_doc::InlineMath {
            id,
            math_tree: empty_tree(),
            extra: Default::default(),
        })
    };
    let paragraph = Paragraph {
        id: paragraph_id.clone(),
        inlines: vec![
            math(first_math_id.clone()),
            ParagraphContent::Environment(InlineEnvironment {
                id: empty_environment_id.clone(),
                kind: InlineEnvironmentKind::Bold,
                definition_id: None,
                props: Default::default(),
                children: Vec::new(),
                extra: Default::default(),
            }),
            ParagraphContent::Environment(InlineEnvironment {
                id: outer_environment_id.clone(),
                kind: InlineEnvironmentKind::Italic,
                definition_id: None,
                props: Default::default(),
                children: vec![math(nested_math_id.clone())],
                extra: Default::default(),
            }),
        ],
        ..Paragraph::empty(paragraph_id.clone())
    };
    let session = Session::new(Document {
        body: vec![Block::Environment(BlockEnvironment {
            id: section_id.clone(),
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: Default::default(),
            title: None,
            children: vec![Block::Paragraph(paragraph)],
            extra: Default::default(),
        })],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let math_inside = |math_id: NodeId| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(HostCaretPosition::Inside {
                host_id: math_id.clone(),
                kind: HostCaretKind::Inline,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: MathEditTarget::Inline {
                        paragraph_id: paragraph_id.clone(),
                        math_id: math_id.clone(),
                    },
                    coordinate: MathCaretCoordinate::TreeBoundary {
                        host: MathPositionHost::InlineMath { owner_id: math_id },
                        path: MathSequencePath::root(),
                        index: 0,
                        affinity: MathPositionAffinity::Downstream,
                    },
                }),
            }),
        )
    };
    let inline_edge = |host_id, before| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(if before {
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
            } else {
                HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
            }),
        )
    };
    let section_edge = |before| {
        caret_snapshot(
            &session,
            DocumentCaretPosition::Host(if before {
                HostCaretPosition::Before {
                    host_id: section_id.clone(),
                    kind: HostCaretKind::Block,
                }
            } else {
                HostCaretPosition::After {
                    host_id: section_id.clone(),
                    kind: HostCaretKind::Block,
                }
            }),
        )
    };

    let first_inside = math_inside(first_math_id.clone());
    let empty_before = inline_edge(empty_environment_id.clone(), true);
    let empty_after = inline_edge(empty_environment_id, false);
    let outer_before = inline_edge(outer_environment_id.clone(), true);
    let outer_after = inline_edge(outer_environment_id, false);
    let nested_inside = math_inside(nested_math_id.clone());
    let section_before = section_edge(true);
    let section_after = section_edge(false);

    for (index, (earlier, later)) in [
        (&first_inside, &empty_before),
        (&first_inside, &empty_after),
        (&outer_before, &nested_inside),
        (&nested_inside, &outer_after),
        (&section_before, &nested_inside),
        (&nested_inside, &section_after),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            session.compare_caret_snapshots(earlier, later),
            Ok(std::cmp::Ordering::Less),
            "ordered pair {index}"
        );
        assert_eq!(
            session.compare_caret_snapshots(later, earlier),
            Ok(std::cmp::Ordering::Greater)
        );
    }
    assert_eq!(
        session.compare_caret_snapshots(&empty_before, &empty_after),
        Ok(std::cmp::Ordering::Less)
    );

    let mut invalid_coordinate = nested_inside.clone();
    if let DocumentCaretPosition::Host(HostCaretPosition::Inside {
        local: HostCaretLocalPosition::Math(math),
        ..
    }) = &mut invalid_coordinate.position
        && let MathCaretCoordinate::TreeBoundary { index, .. } = &mut math.coordinate
    {
        *index = 1;
    }
    assert_eq!(
        session.compare_caret_snapshots(&invalid_coordinate, &outer_after),
        Err(EditError::InvalidPosition)
    );

    let mut mismatched_host = nested_inside.clone();
    if let DocumentCaretPosition::Host(HostCaretPosition::Inside { host_id, .. }) =
        &mut mismatched_host.position
    {
        *host_id = first_math_id;
    }
    assert_eq!(
        session.compare_caret_snapshots(&mismatched_host, &outer_after),
        Err(EditError::InvalidPosition)
    );
}
