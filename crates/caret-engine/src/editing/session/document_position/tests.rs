//! 验证跨宿主位置的校验与文档顺序。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::*;
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, InlineEnvironment,
    InlineEnvironmentKind, NodeId, Paragraph, ParagraphContent, Run, TextFlow,
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
    let first_after = edge(first_math_id, true);
    let second_before = edge(second_math_id, false);
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
