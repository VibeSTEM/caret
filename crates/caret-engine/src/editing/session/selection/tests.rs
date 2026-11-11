//! 验证鼠标范围与键盘导航共用同一结构选区提升判定。

use super::super::{Command, Direction, Pos, Selection, Session, StoryId};
use crate::VisualAffinity;
use crate::editing::{Affinity, DocumentRangeEndpoint, EditorKey, StructureSelection};
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, Equation, MathContent, MathFlow,
    MathNode, MathNodeKind, MathTree, NodeId, Paragraph, ParagraphContent, Run, TextFlow,
};
use std::collections::BTreeMap;

fn paragraph_with_text(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(ParagraphContent::Text(TextFlow {
        runs: vec![Run {
            text: text.to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: Default::default(),
    }));
    paragraph
}

fn paragraph_end(paragraph: &Paragraph) -> Pos {
    Pos {
        story: StoryId::Body,
        paragraph_id: paragraph.id.clone(),
        offset: paragraph
            .inlines
            .iter()
            .filter_map(|inline| match inline {
                ParagraphContent::Text(flow) => {
                    Some(flow.runs.iter().map(|run| run.text.len()).sum::<usize>())
                }
                _ => None,
            })
            .sum::<usize>(),
        affinity: Affinity::Upstream,
    }
}

fn paragraph_start(paragraph: &Paragraph) -> Pos {
    Pos {
        story: StoryId::Body,
        paragraph_id: paragraph.id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    }
}

fn equation(text: &str) -> Block {
    Block::Equation(Equation {
        id: NodeId::generate(),
        math_tree: MathTree {
            schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
            root: MathFlow {
                id: NodeId::generate(),
                content: vec![MathContent::Math(MathNode {
                    id: NodeId::generate(),
                    kind: MathNodeKind::Symbol,
                    text: Some(text.to_owned()),
                    attributes: BTreeMap::new(),
                    children: Vec::new(),
                    extra: BTreeMap::new(),
                })],
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    })
}

fn session_with_body(body: Vec<Block>, caret: Pos) -> Session {
    let mut session = Session::new(Document {
        body,
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: caret.clone(),
            focus: caret,
        })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));
    session
}

#[test]
fn pointer_plain_text_cross_paragraph_range_stays_normal_selection() {
    let first = paragraph_with_text("first");
    let second = paragraph_with_text("second");
    let anchor = paragraph_end(&first);
    let focus = paragraph_start(&second);
    let mut session = session_with_body(
        vec![Block::Paragraph(first), Block::Paragraph(second)],
        anchor.clone(),
    );

    let effect = session
        .set_pointer_selection_range(
            anchor.clone(),
            focus.clone(),
            VisualAffinity::Upstream,
            VisualAffinity::Downstream,
        )
        .unwrap_or_else(|error| panic!("pointer range failed: {error}"));

    assert!(effect.selection_changed);
    assert!(!session.has_structure_selection());
    assert!(session.structure_selection_range().is_none());
    assert_eq!(session.selection().anchor, anchor);
    assert_eq!(session.selection().focus, focus);
    assert_eq!(session.selected_math_blocks(), &[]);
}

#[test]
fn pointer_range_across_equation_promotes_and_delete_removes_it_atomically() {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let equation_id = match &equation_block {
        Block::Equation(equation) => equation.id.clone(),
        _ => unreachable!(),
    };
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after.clone()),
        ],
        paragraph_end(&before),
    );
    let generation_before = session.document_generation();

    session
        .set_pointer_selection_range(
            paragraph_end(&before),
            paragraph_start(&after),
            VisualAffinity::Upstream,
            VisualAffinity::Downstream,
        )
        .unwrap_or_else(|error| panic!("pointer range failed: {error}"));

    assert!(session.has_structure_selection());
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    assert!(matches!(anchor, DocumentRangeEndpoint::Text(_)));
    assert!(matches!(focus, DocumentRangeEndpoint::Text(_)));
    let text = session
        .selected_text()
        .unwrap_or_else(|error| panic!("structure selection text failed: {error}"));
    assert!(
        text.contains('\u{FFFC}'),
        "selected text should keep the equation placeholder: {text:?}"
    );

    let deleted = session
        .run(Command::Delete(Direction::Forward))
        .unwrap_or_else(|error| panic!("structure range delete failed: {error}"));

    assert!(deleted.document_changed);
    assert_eq!(session.document_generation(), generation_before + 1);
    assert!(
        !session
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Equation(equation) if equation.id == equation_id))
    );
    // 两端都是文本端点时按纯文本跨段删除语义合并：公式块与终点段落一并移除。
    assert_eq!(session.document().body.len(), 1);
    let Block::Paragraph(merged) = &session.document().body[0] else {
        panic!("删除后应只剩合并段落");
    };
    let merged_text = merged
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            ParagraphContent::Text(flow) => Some(
                flow.runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(merged_text, "beforeafter");
    assert!(session.selected_math_blocks().is_empty());
    assert!(!session.has_structure_selection());
    assert!(session.can_undo());

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    assert!(
        session
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Equation(equation) if equation.id == equation_id))
    );
}

fn section(title: &str, children: Vec<Block>) -> Block {
    Block::Environment(BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(paragraph_with_text(title)),
        children,
        extra: BTreeMap::new(),
    })
}

#[test]
fn pointer_drag_out_of_section_promotes_and_normalizes_environment_range() {
    let inside = paragraph_with_text("inside");
    let section = section("Heading", vec![Block::Paragraph(inside.clone())]);
    let section_id = match &section {
        Block::Environment(environment) => environment.id.clone(),
        _ => unreachable!(),
    };
    let outside = paragraph_with_text("outside");
    let mut session = session_with_body(
        vec![section, Block::Paragraph(outside.clone())],
        paragraph_end(&inside),
    );

    session
        .set_pointer_selection_range(
            paragraph_end(&inside),
            paragraph_start(&outside),
            VisualAffinity::Upstream,
            VisualAffinity::Downstream,
        )
        .unwrap_or_else(|error| panic!("pointer range failed: {error}"));

    assert_eq!(
        session.selected_block_environments(),
        std::slice::from_ref(&section_id)
    );
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    // 结构范围端点扩展到 Section 外前边界，与派生环境列表描述同一范围。
    assert!(matches!(
        anchor,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Backward,
        } if block_id == &section_id
    ));
    assert!(matches!(
        focus,
        DocumentRangeEndpoint::Text(position) if position.paragraph_id == outside.id
    ));
    let (start, end) = session
        .structure_selection_text_bounds()
        .unwrap_or_else(|| panic!("text bounds missing"));
    assert!(start.paragraph_id != inside.id && end.paragraph_id == outside.id);

    let generation_before = session.document_generation();
    session
        .run(Command::Delete(Direction::Forward))
        .unwrap_or_else(|error| panic!("delete failed: {error}"));
    assert_eq!(session.document_generation(), generation_before + 1);
    assert!(!session.document().body.iter().any(
        |block| matches!(block, Block::Environment(environment) if environment.id == section_id)
    ));
    assert!(session.selected_block_environments().is_empty());
}

fn document_range_selection(
    anchor: DocumentRangeEndpoint,
    focus: DocumentRangeEndpoint,
) -> StructureSelection {
    StructureSelection { anchor, focus }
}

fn select_cross_equation_range(
    session: &mut Session,
    start: Pos,
    end: Pos,
) -> Result<(), Box<dyn std::error::Error>> {
    session.install_structure_selection(document_range_selection(
        DocumentRangeEndpoint::Text(start),
        DocumentRangeEndpoint::Text(end),
    ))?;
    Ok(())
}

#[test]
fn plain_right_collapses_structure_selection_to_its_document_end()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after.clone()),
        ],
        paragraph_end(&before),
    );
    // 反向选择：anchor 在终点、focus 在起点，折叠必须按文档顺序取终点而不是焦点端。
    select_cross_equation_range(&mut session, paragraph_end(&after), paragraph_end(&before))?;
    assert!(session.has_structure_selection());

    let jumped_candidate = Pos {
        offset: 4,
        affinity: Affinity::Downstream,
        ..paragraph_end(&after)
    };
    let generation = crate::editing::NavigationGeneration {
        session_id: session.session_id().clone(),
        document_generation: session.document_generation(),
        font_generation: 0,
    };
    let effect = session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: Some(crate::editing::NavigationCandidate::Document {
            focus: jumped_candidate.clone(),
            visual_affinity: VisualAffinity::Downstream,
            generation,
        }),
        extend: false,
    }))?;

    assert!(effect.selection_changed);
    assert!(!session.has_structure_selection());
    assert!(session.selected_math_blocks().is_empty());
    assert_eq!(session.selection().anchor, paragraph_end(&after));
    assert_eq!(session.selection().focus, paragraph_end(&after));
    assert_ne!(session.selection().focus, jumped_candidate);
    Ok(())
}

#[test]
fn plain_left_collapses_structure_selection_to_its_document_start()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after.clone()),
        ],
        paragraph_end(&after),
    );
    select_cross_equation_range(&mut session, paragraph_end(&before), paragraph_end(&after))?;

    let jumped_candidate = Pos {
        offset: 1,
        affinity: Affinity::Upstream,
        ..paragraph_end(&before)
    };
    let generation = crate::editing::NavigationGeneration {
        session_id: session.session_id().clone(),
        document_generation: session.document_generation(),
        font_generation: 0,
    };
    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: Some(crate::editing::NavigationCandidate::Document {
            focus: jumped_candidate.clone(),
            visual_affinity: VisualAffinity::Upstream,
            generation,
        }),
        extend: false,
    }))?;

    assert!(!session.has_structure_selection());
    assert_eq!(session.selection().anchor, paragraph_end(&before));
    assert_eq!(session.selection().focus, paragraph_end(&before));
    assert_ne!(session.selection().focus, jumped_candidate);
    Ok(())
}

#[test]
fn plain_right_collapses_block_edge_structure_selection_to_the_brace_after_the_block()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let equation_id = match &equation_block {
        Block::Equation(equation) => equation.id.clone(),
        _ => unreachable!(),
    };
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after.clone()),
        ],
        paragraph_end(&before),
    );
    session.install_structure_selection(document_range_selection(
        DocumentRangeEndpoint::BlockEdge {
            block_id: equation_id.clone(),
            side: Direction::Backward,
        },
        DocumentRangeEndpoint::BlockEdge {
            block_id: equation_id,
            side: Direction::Forward,
        },
    ))?;

    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: false,
    }))?;

    // 块外侧边界终点由共享停靠策略投影到相邻段落起点，并保留块外后停靠。
    assert!(!session.has_structure_selection());
    assert_eq!(session.selection().anchor, session.selection().focus);
    assert_eq!(session.selection().focus.paragraph_id, after.id);
    assert_eq!(session.selection().focus.offset, 0);
    assert!(session.block_boundary_exit().is_some());
    Ok(())
}

fn block_edge_equation_selection(equation_id: &NodeId) -> StructureSelection {
    document_range_selection(
        DocumentRangeEndpoint::BlockEdge {
            block_id: equation_id.clone(),
            side: Direction::Backward,
        },
        DocumentRangeEndpoint::BlockEdge {
            block_id: equation_id.clone(),
            side: Direction::Forward,
        },
    )
}

#[test]
fn vertical_extend_from_block_edge_anchor_keeps_the_covered_equation()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let equation_id = match &equation_block {
        Block::Equation(equation) => equation.id.clone(),
        _ => unreachable!(),
    };
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after.clone()),
        ],
        paragraph_end(&before),
    );
    session.install_structure_selection(block_edge_equation_selection(&equation_id))?;
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    let generation = crate::editing::NavigationGeneration {
        session_id: session.session_id().clone(),
        document_generation: session.document_generation(),
        font_generation: 0,
    };
    let focus = Pos {
        offset: 2,
        affinity: Affinity::Downstream,
        ..paragraph_start(&after)
    };

    session.run(Command::EditorKey(EditorKey::DownArrow {
        candidate: Some(crate::editing::NavigationCandidate::Document {
            focus: focus.clone(),
            visual_affinity: VisualAffinity::Downstream,
            generation,
        }),
        extend: true,
        structure: false,
    }))?;

    // 块外侧边界锚点投影到块前文本位置后仍完整覆盖公式对象，不得清掉结构范围。
    assert!(session.has_structure_selection());
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    assert_eq!(session.selection().focus, focus);
    Ok(())
}

#[test]
fn visual_line_edge_extend_from_block_edge_anchor_keeps_the_covered_equation()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let equation_id = match &equation_block {
        Block::Equation(equation) => equation.id.clone(),
        _ => unreachable!(),
    };
    let after = paragraph_with_text("after");
    let cases = [
        (
            true,
            Pos {
                offset: "after".len(),
                affinity: Affinity::Upstream,
                ..paragraph_start(&after)
            },
        ),
        (false, paragraph_start(&after)),
    ];
    for (end, focus) in cases {
        let mut session = session_with_body(
            vec![
                Block::Paragraph(before.clone()),
                equation_block.clone(),
                Block::Paragraph(after.clone()),
            ],
            paragraph_end(&before),
        );
        session.install_structure_selection(block_edge_equation_selection(&equation_id))?;
        let generation = crate::editing::NavigationGeneration {
            session_id: session.session_id().clone(),
            document_generation: session.document_generation(),
            font_generation: 0,
        };
        let candidate = Some(crate::editing::NavigationCandidate::Document {
            focus: focus.clone(),
            visual_affinity: VisualAffinity::Downstream,
            generation,
        });
        let key = if end {
            EditorKey::End {
                candidate,
                extend: true,
            }
        } else {
            EditorKey::Home {
                candidate,
                extend: true,
            }
        };
        session.run(Command::EditorKey(key))?;

        assert!(session.has_structure_selection());
        assert_eq!(
            session.selected_math_blocks(),
            std::slice::from_ref(&equation_id)
        );
        assert_eq!(session.selection().focus, focus);
    }
    Ok(())
}

#[test]
fn plain_left_collapses_block_edge_range_to_the_paragraph_before_the_block()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let equation_id = match &equation_block {
        Block::Equation(equation) => equation.id.clone(),
        _ => unreachable!(),
    };
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after),
        ],
        paragraph_end(&before),
    );
    session.install_structure_selection(block_edge_equation_selection(&equation_id))?;

    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: false,
    }))?;

    // 起点侧是块外前边界：折叠落点为块前段落末尾并保留块外前停靠。
    assert!(!session.has_structure_selection());
    assert_eq!(session.selection().anchor, session.selection().focus);
    assert_eq!(session.selection().focus.paragraph_id, before.id);
    assert_eq!(session.selection().focus.offset, "before".len());
    assert_eq!(
        session.block_boundary_exit(),
        Some((equation_id, Direction::Backward))
    );
    Ok(())
}

#[test]
fn block_at_document_start_keeps_covered_equation_when_anchor_has_no_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let equation_block = equation("x");
    let equation_id = match &equation_block {
        Block::Equation(equation) => equation.id.clone(),
        _ => unreachable!(),
    };
    let after = paragraph_with_text("after");
    for vertical in [true, false] {
        let mut session = Session::new(Document {
            body: vec![equation_block.clone(), Block::Paragraph(after.clone())],
            ..Document::blank()
        })?;
        session.install_structure_selection(block_edge_equation_selection(&equation_id))?;
        let generation = crate::editing::NavigationGeneration {
            session_id: session.session_id().clone(),
            document_generation: session.document_generation(),
            font_generation: 0,
        };
        let focus = Pos {
            offset: 2,
            affinity: Affinity::Downstream,
            ..paragraph_start(&after)
        };
        let candidate = Some(crate::editing::NavigationCandidate::Document {
            focus: focus.clone(),
            visual_affinity: VisualAffinity::Downstream,
            generation,
        });
        let key = if vertical {
            EditorKey::DownArrow {
                candidate,
                extend: true,
                structure: false,
            }
        } else {
            EditorKey::End {
                candidate,
                extend: true,
            }
        };
        session.run(Command::EditorKey(key))?;

        // 块前没有段落时锚点无法投影到所求侧：退回文档单元扩展而不是丢失公式覆盖。
        assert!(session.has_structure_selection());
        assert_eq!(
            session.selected_math_blocks(),
            std::slice::from_ref(&equation_id)
        );
        assert_eq!(session.selection().focus.paragraph_id, after.id);
    }
    Ok(())
}
