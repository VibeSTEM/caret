//! 验证鼠标范围与键盘导航共用同一结构选区提升判定。

use super::super::{Command, Direction, Pos, Selection, Session, StoryId};
use crate::VisualAffinity;
use crate::editing::session::math_edit::{
    MathPositionAffinity, MathPositionHost, MathSequencePath, MathTreePosition,
};
use crate::editing::{Affinity, DocumentRangeEndpoint, EditorKey, StructureSelection};
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, Equation, MathContent, MathFlow,
    MathNode, MathNodeKind, MathTree, NodeId, Paragraph, ParagraphContent, Run, Table, TableCell,
    TableRow, TextFlow,
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
    assert_eq!(session.selected_atomic_blocks(), &[]);
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
        session.selected_atomic_blocks(),
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
    assert!(session.selected_atomic_blocks().is_empty());
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

#[test]
fn pointer_range_promotes_the_table_at_the_first_diverging_cell()
-> Result<(), Box<dyn std::error::Error>> {
    let nested_text = paragraph_with_text("nested");
    let same_outer_cell = paragraph_with_text("same outer cell");
    let next_outer_cell = paragraph_with_text("next outer cell");
    let nested_id = nested_text.id.clone();
    let same_outer_id = same_outer_cell.id.clone();
    let next_outer_id = next_outer_cell.id.clone();
    let table = |id: &NodeId, cells: Vec<Vec<Block>>| {
        Block::Table(Table {
            id: id.clone(),
            rows: vec![TableRow {
                id: NodeId::generate(),
                cells: cells
                    .into_iter()
                    .map(|blocks| TableCell {
                        id: NodeId::generate(),
                        blocks,
                    })
                    .collect(),
            }],
        })
    };
    let inner_id = NodeId::generate();
    let outer_id = NodeId::generate();
    let inner = table(&inner_id, vec![vec![Block::Paragraph(nested_text)]]);
    let mut document = Document::blank();
    document.body = vec![table(
        &outer_id,
        vec![
            vec![inner, Block::Paragraph(same_outer_cell)],
            vec![Block::Paragraph(next_outer_cell)],
        ],
    )];
    let mut session = Session::new(document.clone())?;
    let position = |session: &Session, paragraph_id: &NodeId| -> Result<Pos, crate::EditError> {
        Ok(Pos {
            story: session
                .story_for_paragraph(paragraph_id)
                .ok_or(crate::EditError::InvalidPosition)?,
            paragraph_id: paragraph_id.clone(),
            offset: 1,
            affinity: Affinity::Downstream,
        })
    };
    let anchor = position(&session, &nested_id)?;
    for (focus, expected_table) in [(&same_outer_id, &inner_id), (&next_outer_id, &outer_id)] {
        session.set_pointer_selection_range(
            anchor.clone(),
            position(&session, focus)?,
            VisualAffinity::Downstream,
            VisualAffinity::Downstream,
        )?;
        assert_eq!(
            session.selected_atomic_blocks(),
            std::slice::from_ref(expected_table)
        );
        assert_eq!(session.document(), &document);
    }
    Ok(())
}

#[test]
fn pointer_text_to_inline_math_tree_across_table_cells_promotes_table_range() {
    let left = paragraph_with_text("left");
    let left_id = left.id.clone();
    let math_id = NodeId::generate();
    let mut right = Paragraph::empty(NodeId::generate());
    right
        .inlines
        .push(ParagraphContent::Math(caret_doc::InlineMath {
            id: math_id.clone(),
            math_tree: MathTree {
                schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                root: MathFlow {
                    id: NodeId::generate(),
                    content: vec![MathContent::Math(MathNode {
                        id: NodeId::generate(),
                        kind: MathNodeKind::Group,
                        text: None,
                        attributes: BTreeMap::new(),
                        children: vec![MathFlow {
                            id: NodeId::generate(),
                            content: vec![MathContent::Text(caret_doc::TextFlow {
                                runs: vec![Run {
                                    text: "x".to_owned(),
                                    language: None,
                                    props: BTreeMap::new(),
                                    extra: BTreeMap::new(),
                                }],
                                extra: BTreeMap::new(),
                            })],
                            extra: BTreeMap::new(),
                        }],
                        extra: BTreeMap::new(),
                    })],
                    extra: BTreeMap::new(),
                },
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        }));
    let right_id = right.id.clone();
    let outside_math_id = NodeId::generate();
    let mut outside = Paragraph::empty(NodeId::generate());
    outside
        .inlines
        .push(ParagraphContent::Math(caret_doc::InlineMath {
            id: outside_math_id.clone(),
            math_tree: MathTree {
                schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                root: MathFlow {
                    id: NodeId::generate(),
                    content: vec![MathContent::Math(MathNode {
                        id: NodeId::generate(),
                        kind: MathNodeKind::Symbol,
                        text: Some("y".to_owned()),
                        attributes: BTreeMap::new(),
                        children: Vec::new(),
                        extra: BTreeMap::new(),
                    })],
                    extra: BTreeMap::new(),
                },
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        }));
    let outside_id = outside.id.clone();
    let middle = paragraph_with_text("middle");
    let middle_id = middle.id.clone();
    let row_id = NodeId::generate();
    let left_cell_id = NodeId::generate();
    let right_cell_id = NodeId::generate();
    let table = Table {
        id: NodeId::generate(),
        rows: vec![TableRow {
            id: row_id.clone(),
            cells: vec![
                TableCell {
                    id: left_cell_id.clone(),
                    blocks: vec![Block::Paragraph(left)],
                },
                TableCell {
                    id: right_cell_id,
                    blocks: vec![Block::Paragraph(right)],
                },
            ],
        }],
    };
    let table_id = table.id.clone();
    let left_end = Pos {
        story: StoryId::TableCell {
            table_id: table_id.clone(),
            row_id,
            cell_id: left_cell_id,
        },
        paragraph_id: left_id,
        offset: "left".len(),
        affinity: Affinity::Upstream,
    };
    let mut session = Session::new(Document {
        body: vec![
            Block::Table(table),
            Block::Paragraph(middle),
            Block::Paragraph(outside),
        ],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let tree_end = DocumentRangeEndpoint::InlineMathTree {
        paragraph_id: right_id.clone(),
        position: MathTreePosition {
            host: MathPositionHost::InlineMath {
                owner_id: math_id.clone(),
            },
            path: MathSequencePath::root(),
            offset: 1,
            affinity: MathPositionAffinity::Downstream,
        },
        entry_direction: Direction::Forward,
        entry_math_forward: true,
    };
    let math_text = DocumentRangeEndpoint::InlineMathText {
        paragraph_id: right_id.clone(),
        math_id: math_id.clone(),
        position: crate::MathTextPosition::new(
            crate::MathTextFlowPath::from_interaction_path(
                vec![
                    caret_doc::MathPathStep::Content(0),
                    caret_doc::MathPathStep::Operand(0),
                    caret_doc::MathPathStep::Content(0),
                ],
                None,
            ),
            1,
            MathPositionAffinity::Downstream,
        ),
        entry_direction: Direction::Forward,
        entry_math_forward: true,
    };
    let outside_tree_end = DocumentRangeEndpoint::InlineMathTree {
        paragraph_id: outside_id,
        position: MathTreePosition {
            host: MathPositionHost::InlineMath {
                owner_id: outside_math_id,
            },
            path: MathSequencePath::root(),
            offset: 1,
            affinity: MathPositionAffinity::Downstream,
        },
        entry_direction: Direction::Forward,
        entry_math_forward: true,
    };

    session
        .set_pointer_inline_math_selection(
            StructureSelection {
                anchor: DocumentRangeEndpoint::Text(left_end.clone()),
                focus: tree_end.clone(),
            },
            VisualAffinity::Upstream,
            VisualAffinity::Downstream,
        )
        .unwrap_or_else(|error| panic!("cross-cell drag failed: {error}"));

    assert_eq!(
        session.selected_atomic_blocks(),
        std::slice::from_ref(&table_id)
    );
    assert_eq!(
        session
            .structure_selection_range()
            .map(|(anchor, focus)| (anchor.clone(), focus.clone())),
        Some((
            DocumentRangeEndpoint::BlockEdge {
                block_id: table_id.clone(),
                side: Direction::Backward,
            },
            DocumentRangeEndpoint::BlockEdge {
                block_id: table_id.clone(),
                side: Direction::Forward,
            },
        ))
    );

    let previous_range = session
        .structure_selection_range()
        .map(|(anchor, focus)| (anchor.clone(), focus.clone()));
    let previous_generation = session.document_generation();
    let invalid_math_endpoint = DocumentRangeEndpoint::InlineMathTree {
        paragraph_id: right_id.clone(),
        position: MathTreePosition {
            host: MathPositionHost::InlineMath {
                owner_id: NodeId::generate(),
            },
            path: MathSequencePath::root(),
            offset: 0,
            affinity: MathPositionAffinity::Downstream,
        },
        entry_direction: Direction::Forward,
        entry_math_forward: true,
    };
    assert!(
        session
            .set_pointer_inline_math_selection(
                StructureSelection {
                    anchor: DocumentRangeEndpoint::Text(left_end.clone()),
                    focus: invalid_math_endpoint,
                },
                VisualAffinity::Upstream,
                VisualAffinity::Downstream,
            )
            .is_err()
    );
    assert_eq!(
        session
            .structure_selection_range()
            .map(|(anchor, focus)| (anchor.clone(), focus.clone())),
        previous_range
    );
    assert_eq!(session.document_generation(), previous_generation);

    session
        .set_pointer_inline_math_selection(
            StructureSelection {
                anchor: tree_end,
                focus: DocumentRangeEndpoint::Text(left_end.clone()),
            },
            VisualAffinity::Downstream,
            VisualAffinity::Upstream,
        )
        .unwrap_or_else(|error| panic!("reverse cross-cell drag failed: {error}"));
    assert_eq!(
        session
            .structure_selection_range()
            .map(|(anchor, focus)| (anchor.clone(), focus.clone())),
        Some((
            DocumentRangeEndpoint::BlockEdge {
                block_id: table_id.clone(),
                side: Direction::Forward,
            },
            DocumentRangeEndpoint::BlockEdge {
                block_id: table_id.clone(),
                side: Direction::Backward,
            },
        ))
    );

    session
        .set_pointer_inline_math_selection(
            StructureSelection {
                anchor: DocumentRangeEndpoint::Text(left_end.clone()),
                focus: math_text,
            },
            VisualAffinity::Upstream,
            VisualAffinity::Downstream,
        )
        .unwrap_or_else(|error| panic!("math-text cross-cell drag failed: {error}"));
    assert_eq!(
        session.selected_atomic_blocks(),
        std::slice::from_ref(&table_id)
    );

    session
        .set_pointer_inline_math_selection(
            StructureSelection {
                anchor: DocumentRangeEndpoint::Text(left_end.clone()),
                focus: outside_tree_end.clone(),
            },
            VisualAffinity::Upstream,
            VisualAffinity::Downstream,
        )
        .unwrap_or_else(|error| panic!("table-to-outside-math drag failed: {error}"));
    assert_eq!(
        session
            .structure_selection_range()
            .map(|(anchor, focus)| (anchor.clone(), focus.clone())),
        Some((
            DocumentRangeEndpoint::BlockEdge {
                block_id: table_id.clone(),
                side: Direction::Backward,
            },
            outside_tree_end.clone(),
        ))
    );
    assert_eq!(
        session.selected_atomic_blocks(),
        std::slice::from_ref(&table_id)
    );
    let selected = session
        .selected_text()
        .unwrap_or_else(|error| panic!("selected text failed: {error}"));
    assert!(selected.contains("middle"), "selected text: {selected:?}");
    session
        .run(Command::Delete(Direction::Forward))
        .unwrap_or_else(|error| panic!("cross-table range delete failed: {error}"));
    assert!(
        !session
            .document()
            .body
            .iter()
            .any(|block| { matches!(block, Block::Table(table) if table.id == table_id) })
    );
    assert!(!session.document().body.iter().any(|block| {
        matches!(block, Block::Paragraph(paragraph) if paragraph.id == middle_id
            && paragraph.inlines.iter().any(|inline| matches!(inline, ParagraphContent::Text(flow)
                if flow.runs.iter().any(|run| run.text.contains("middle")))))
    }));
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("cross-table range undo failed: {error}"));
    assert!(
        session
            .document()
            .body
            .iter()
            .any(|block| { matches!(block, Block::Table(table) if table.id == table_id) })
    );
    assert!(session.document().body.iter().any(|block| {
        matches!(block, Block::Paragraph(paragraph) if paragraph.id == middle_id
            && paragraph.inlines.iter().any(|inline| matches!(inline, ParagraphContent::Text(flow)
                if flow.runs.iter().any(|run| run.text.contains("middle")))))
    }));

    session
        .set_pointer_inline_math_selection(
            StructureSelection {
                anchor: outside_tree_end.clone(),
                focus: DocumentRangeEndpoint::Text(left_end.clone()),
            },
            VisualAffinity::Downstream,
            VisualAffinity::Upstream,
        )
        .unwrap_or_else(|error| panic!("outside-math-to-table drag failed: {error}"));
    assert_eq!(
        session
            .structure_selection_range()
            .map(|(anchor, focus)| (anchor.clone(), focus.clone())),
        Some((
            outside_tree_end,
            DocumentRangeEndpoint::BlockEdge {
                block_id: table_id.clone(),
                side: Direction::Backward,
            },
        ))
    );
    assert_eq!(
        session.selected_atomic_blocks(),
        std::slice::from_ref(&table_id)
    );

    let (right_before, _) = session
        .inline_math_pointer_bounds(
            &crate::editing::session::math_edit::MathEditTarget::Inline {
                paragraph_id: right_id,
                math_id,
            },
        )
        .unwrap_or_else(|error| panic!("inline math bounds failed: {error}"));
    session
        .set_pointer_selection_range(
            left_end.clone(),
            right_before.clone(),
            VisualAffinity::Upstream,
            VisualAffinity::Downstream,
        )
        .unwrap_or_else(|error| panic!("text cross-cell drag failed: {error}"));
    assert_eq!(
        session.selected_atomic_blocks(),
        std::slice::from_ref(&table_id)
    );
    session
        .set_pointer_selection_range(
            left_end.clone(),
            left_end.clone(),
            VisualAffinity::Upstream,
            VisualAffinity::Upstream,
        )
        .unwrap_or_else(|error| panic!("cell caret setup failed: {error}"));
    session
        .run(Command::UpdateComposition {
            id: 87,
            preedit: "候选".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition setup failed: {error}"));
    let composition_generation = session.document_generation();
    let invalid_left = Pos {
        offset: left_end.offset + 1,
        ..left_end.clone()
    };
    assert!(
        session
            .set_pointer_selection_range(
                invalid_left,
                right_before.clone(),
                VisualAffinity::Upstream,
                VisualAffinity::Downstream,
            )
            .is_err()
    );
    assert_eq!(
        session.composition().map(|composition| composition.id),
        Some(87)
    );
    assert_eq!(session.document_generation(), composition_generation);
    let effect = session
        .set_pointer_selection_range(
            left_end,
            right_before,
            VisualAffinity::Upstream,
            VisualAffinity::Downstream,
        )
        .unwrap_or_else(|error| panic!("composition cross-cell drag failed: {error}"));
    assert!(effect.composition_changed);
    assert!(session.composition().is_none());
}

#[test]
fn mixed_inline_math_range_deletes_block_covered_only_by_structure_projection() {
    let equation_block = equation("x");
    let equation_id = match &equation_block {
        Block::Equation(equation) => equation.id.clone(),
        _ => unreachable!(),
    };
    let math_id = NodeId::generate();
    let text_path = crate::MathTextFlowPath::from_interaction_path(
        vec![
            caret_doc::MathPathStep::Content(0),
            caret_doc::MathPathStep::Operand(0),
            caret_doc::MathPathStep::Content(0),
        ],
        None,
    );
    let mut paragraph = paragraph_with_text("");
    paragraph
        .inlines
        .push(ParagraphContent::Math(caret_doc::InlineMath {
            id: math_id.clone(),
            math_tree: MathTree {
                schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                root: MathFlow {
                    id: NodeId::generate(),
                    content: vec![MathContent::Math(MathNode {
                        id: NodeId::generate(),
                        kind: MathNodeKind::Group,
                        text: None,
                        attributes: BTreeMap::new(),
                        children: vec![MathFlow {
                            id: NodeId::generate(),
                            content: vec![MathContent::Text(caret_doc::TextFlow {
                                runs: vec![Run {
                                    text: "ab".to_owned(),
                                    language: None,
                                    props: BTreeMap::new(),
                                    extra: BTreeMap::new(),
                                }],
                                extra: Default::default(),
                            })],
                            extra: BTreeMap::new(),
                        }],
                        extra: BTreeMap::new(),
                    })],
                    extra: BTreeMap::new(),
                },
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        }));
    let paragraph_id = paragraph.id.clone();
    let mut session = Session::new(Document {
        body: vec![equation_block, Block::Paragraph(paragraph)],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let selection = StructureSelection {
        anchor: DocumentRangeEndpoint::BlockEdge {
            block_id: equation_id.clone(),
            side: Direction::Backward,
        },
        focus: DocumentRangeEndpoint::InlineMathText {
            paragraph_id: paragraph_id.clone(),
            math_id: math_id.clone(),
            position: crate::MathTextPosition::new(
                text_path,
                0,
                crate::MathPositionAffinity::Downstream,
            ),
            entry_direction: Direction::Forward,
            entry_math_forward: true,
        },
    };
    session
        .set_pointer_inline_math_selection(
            selection,
            VisualAffinity::Upstream,
            VisualAffinity::Downstream,
        )
        .unwrap_or_else(|error| panic!("mixed selection setup failed: {error}"));
    assert_eq!(
        session.selected_atomic_blocks(),
        std::slice::from_ref(&equation_id)
    );

    let effect = session
        .run(Command::Delete(Direction::Forward))
        .unwrap_or_else(|error| panic!("mixed range delete failed: {error}"));

    assert!(effect.document_changed);
    assert!(session.document().body.iter().all(|block| {
        !matches!(block, Block::Equation(equation) if equation.id == equation_id)
    }));
    assert!(session.document().body.iter().any(|block| {
        matches!(block, Block::Paragraph(paragraph) if paragraph.id == paragraph_id
            && paragraph.inlines.iter().any(|inline| matches!(inline, ParagraphContent::Math(math) if math.id == math_id)))
    }));
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("mixed delete undo failed: {error}"));
    assert!(
        session.document().body.iter().any(|block| {
            matches!(block, Block::Equation(equation) if equation.id == equation_id)
        })
    );
}

#[test]
fn mixed_inline_math_delete_rejects_unknown_math_metadata_atomically() {
    let math_id = NodeId::generate();
    let paragraph = Paragraph {
        id: NodeId::generate(),
        inlines: vec![ParagraphContent::Math(caret_doc::InlineMath {
            id: math_id.clone(),
            math_tree: MathTree {
                schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                root: MathFlow {
                    id: NodeId::generate(),
                    content: vec![MathContent::Math(MathNode {
                        id: NodeId::generate(),
                        kind: MathNodeKind::Symbol,
                        text: Some("x".to_owned()),
                        attributes: BTreeMap::new(),
                        children: Vec::new(),
                        extra: BTreeMap::from([(
                            "future-field".to_owned(),
                            serde_json::json!("preserve"),
                        )]),
                    })],
                    extra: BTreeMap::new(),
                },
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        })],
        ..Paragraph::empty(NodeId::generate())
    };
    let paragraph_id = paragraph.id.clone();
    let mut session = Session::new(Document {
        body: vec![Block::Paragraph(paragraph)],
        ..Document::blank()
    })
    .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let endpoint = |offset| DocumentRangeEndpoint::InlineMathTree {
        paragraph_id: paragraph_id.clone(),
        position: MathTreePosition {
            host: MathPositionHost::InlineMath {
                owner_id: math_id.clone(),
            },
            path: MathSequencePath::root(),
            offset,
            affinity: MathPositionAffinity::Downstream,
        },
        entry_direction: Direction::Forward,
        entry_math_forward: true,
    };
    let selection = StructureSelection {
        anchor: endpoint(0),
        focus: endpoint(1),
    };
    session
        .set_pointer_inline_math_selection(
            selection.clone(),
            VisualAffinity::Upstream,
            VisualAffinity::Downstream,
        )
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));
    let before_document = session.document().clone();
    let before_generation = session.document_generation();

    assert!(matches!(
        session.run(Command::Delete(Direction::Forward)),
        Err(crate::editing::EditError::UnsupportedOpaqueContent)
    ));
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.document_generation(), before_generation);
    assert_eq!(
        session.structure_selection_range(),
        Some((&selection.anchor, &selection.focus))
    );
    assert!(!session.can_undo());
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
    assert!(session.selected_atomic_blocks().is_empty());
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
        session.selected_atomic_blocks(),
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

    let stale = crate::editing::NavigationGeneration {
        document_generation: generation.document_generation + 1,
        ..generation.clone()
    };
    let stale_edge = Command::EditorKey(EditorKey::UpArrow {
        candidate: Some(crate::editing::NavigationCandidate::BlockEdge {
            block_id: equation_id.clone(),
            side: Direction::Backward,
            generation: stale,
        }),
        extend: true,
        structure: false,
    });
    let previous = session.structure_selection.clone();
    assert!(session.run(stale_edge).is_err());
    assert_eq!(session.structure_selection, previous);

    session.run(Command::EditorKey(EditorKey::DownArrow {
        candidate: Some(crate::editing::NavigationCandidate::Document {
            focus: focus.clone(),
            visual_affinity: VisualAffinity::Downstream,
            generation,
        }),
        extend: true,
        structure: false,
    }))?;

    assert_eq!(
        session.structure_selection_range(),
        Some((
            &DocumentRangeEndpoint::BlockEdge {
                block_id: equation_id.clone(),
                side: Direction::Backward
            },
            &DocumentRangeEndpoint::Text(focus.clone()),
        ))
    );
    assert!(session.has_structure_selection());
    assert_eq!(
        session.selected_atomic_blocks(),
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
            session.selected_atomic_blocks(),
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
            session.selected_atomic_blocks(),
            std::slice::from_ref(&equation_id)
        );
        assert_eq!(session.selection().focus.paragraph_id, after.id);
    }
    Ok(())
}
