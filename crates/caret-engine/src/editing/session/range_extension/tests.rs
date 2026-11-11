//! 验证跨段落、公式块与行内公式的 Shift 扩选行为。

use super::super::Session;
use crate::editing::{
    Affinity, Command, Direction, DocumentRangeEndpoint, EditError, EditorKey, MathEditBranch,
    MathPositionHost, NavigationCandidate, NavigationGeneration, Pos, Selection, StoryId,
    StructureSelection,
};
use caret_doc::{
    Align, Block, BlockEnvironment, BlockEnvironmentKind, Document, Equation,
    InlineEnvironmentKind, MathAlignRow, MathContent, MathFlow, MathNode, MathNodeKind, MathTree,
    NodeId, Paragraph, ParagraphContent, Run, TextFlow,
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

fn math_tree(text: &str) -> MathTree {
    MathTree {
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
    }
}

fn equation(text: &str) -> Block {
    Block::Equation(Equation {
        id: NodeId::generate(),
        math_tree: math_tree(text),
        extra: BTreeMap::new(),
    })
}

fn align(text: &str) -> Block {
    align_with_cells(&[text])
}

fn align_with_cells(texts: &[&str]) -> Block {
    Block::Align(Align {
        id: NodeId::generate(),
        rows: vec![MathAlignRow {
            id: NodeId::generate(),
            cells: texts.iter().map(|text| math_tree(text)).collect(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    })
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

fn environment_id(block: &Block) -> NodeId {
    match block {
        Block::Environment(environment) => environment.id.clone(),
        _ => panic!("the block is not an environment"),
    }
}

fn title_end(paragraph: &Paragraph, environment_id: &NodeId) -> Pos {
    Pos {
        story: StoryId::EnvironmentTitle(environment_id.clone()),
        paragraph_id: paragraph.id.clone(),
        offset: paragraph_text_len(paragraph),
        affinity: Affinity::Upstream,
    }
}

fn align_cell_ids(block: &Block) -> Vec<(NodeId, NodeId)> {
    let Block::Align(align) = block else {
        panic!("the block is not an align block");
    };
    align
        .rows
        .iter()
        .flat_map(|row| {
            row.cells
                .iter()
                .map(move |cell| (row.id.clone(), cell.root.id.clone()))
        })
        .collect()
}

/// 当前 MathTree 焦点所在的 Align 行/单元格身份。
fn current_align_cell(session: &Session) -> Option<(NodeId, NodeId)> {
    let MathEditBranch::Tree { anchor, .. } = &session.math_edit()?.branch else {
        return None;
    };
    match &anchor.host {
        MathPositionHost::AlignCell { row_id, cell_id } => Some((row_id.clone(), cell_id.clone())),
        _ => None,
    }
}

fn block_id(block: &Block) -> NodeId {
    match block {
        Block::Equation(equation) => equation.id.clone(),
        Block::Align(align) => align.id.clone(),
        _ => panic!("the block is not a math block"),
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

fn paragraph_text_len(paragraph: &Paragraph) -> usize {
    paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            ParagraphContent::Text(flow) => {
                Some(flow.runs.iter().map(|run| run.text.len()).sum::<usize>())
            }
            ParagraphContent::Math(_) => Some("\u{FFFC}".len()),
            _ => None,
        })
        .sum()
}

/// 从公式/前一块进入段落时 `paragraph_beside_block` 给出的落点。
fn parked_start(paragraph: &Paragraph) -> Pos {
    Pos {
        story: StoryId::Body,
        paragraph_id: paragraph.id.clone(),
        offset: 0,
        affinity: Affinity::Upstream,
    }
}

/// 从公式/后一块方向进入段落末尾时 `paragraph_beside_block` 给出的落点。
fn parked_end(paragraph: &Paragraph) -> Pos {
    Pos {
        story: StoryId::Body,
        paragraph_id: paragraph.id.clone(),
        offset: paragraph_text_len(paragraph),
        affinity: Affinity::Downstream,
    }
}

fn paragraph_end(paragraph: &Paragraph) -> Pos {
    let offset = paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            ParagraphContent::Text(flow) => {
                Some(flow.runs.iter().map(|run| run.text.len()).sum::<usize>())
            }
            ParagraphContent::Math(_) => Some("\u{FFFC}".len()),
            _ => None,
        })
        .sum();
    Pos {
        story: StoryId::Body,
        paragraph_id: paragraph.id.clone(),
        offset,
        affinity: Affinity::Upstream,
    }
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

fn shift_right(session: &mut Session) {
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-right failed: {error}"));
}

fn shift_left(session: &mut Session) {
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-left failed: {error}"));
}

fn text_endpoint(endpoint: &DocumentRangeEndpoint) -> &Pos {
    match endpoint {
        DocumentRangeEndpoint::Text(position) => position,
        DocumentRangeEndpoint::BlockEdge { .. } => panic!("expected text endpoint: {endpoint:?}"),
    }
}

#[test]
fn shift_right_covers_adjacent_equation_then_extends_into_following_text() {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let equation_id = block_id(&equation_block);
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after.clone()),
        ],
        paragraph_end(&before),
    );

    shift_right(&mut session);

    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    assert_eq!(session.selected_block_environments(), &[]);
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    assert_eq!(text_endpoint(anchor), &paragraph_end(&before));
    assert!(matches!(
        focus,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Forward,
        } if block_id == &equation_id
    ));
    assert_eq!(session.selection().focus, parked_start(&after));
    assert_eq!(
        session.block_boundary_exit(),
        Some((equation_id.clone(), Direction::Forward))
    );

    shift_right(&mut session);

    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    assert_eq!(text_endpoint(anchor), &paragraph_end(&before));
    assert_eq!(text_endpoint(focus), &paragraph_start(&after));
    assert_eq!(session.selection().focus, paragraph_start(&after));
    assert_eq!(session.selected_math_blocks(), &[equation_id]);
    assert!(session.block_boundary_exit().is_none());

    shift_right(&mut session);

    let (_, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    let focus = text_endpoint(focus);
    assert_eq!(focus.paragraph_id, after.id);
    assert_eq!(focus.offset, 1);
    assert_eq!(session.selection().focus.offset, 1);
}

#[test]
fn shift_left_covers_adjacent_equation_then_continues_into_previous_text() {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let equation_id = block_id(&equation_block);
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after.clone()),
        ],
        paragraph_start(&after),
    );

    shift_left(&mut session);

    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    // 反向选区保留 anchor/focus 方向：锚点在后方段落，焦点在公式前边界。
    assert_eq!(text_endpoint(anchor), &paragraph_start(&after));
    assert!(matches!(
        focus,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Backward,
        } if block_id == &equation_id
    ));
    assert_eq!(session.selection().focus, parked_end(&before));
    assert_eq!(
        session.block_boundary_exit(),
        Some((equation_id.clone(), Direction::Backward))
    );

    shift_left(&mut session);

    assert_eq!(session.selected_math_blocks(), &[equation_id]);
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    assert_eq!(text_endpoint(anchor), &paragraph_start(&after));
    assert_eq!(text_endpoint(focus), &paragraph_end(&before));
    assert!(session.block_boundary_exit().is_none());
}

#[test]
fn shift_arrows_extend_inside_equation_tree_then_convert_to_block_range() {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let equation_id = block_id(&equation_block);
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after.clone()),
        ],
        paragraph_end(&before),
    );
    session
        .run(Command::EnterMath(Direction::Forward))
        .unwrap_or_else(|error| panic!("entering block math failed: {error}"));
    let math_edit = session
        .math_edit()
        .unwrap_or_else(|| panic!("math edit missing"));
    let MathEditBranch::Tree { anchor, .. } = &math_edit.branch else {
        panic!("expected math tree branch");
    };
    assert_eq!(anchor.offset, 0);

    shift_right(&mut session);

    let math_edit = session
        .math_edit()
        .unwrap_or_else(|| panic!("local extension left the host"));
    let MathEditBranch::Tree { anchor, .. } = &math_edit.branch else {
        panic!("expected math tree branch");
    };
    assert_eq!(anchor.offset, 0);
    assert_eq!(math_edit.cursor, 1);
    assert!(session.structure_selection_range().is_none());

    shift_right(&mut session);

    assert!(session.math_edit().is_none());
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    assert!(matches!(
        anchor,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Backward,
        } if block_id == &equation_id
    ));
    assert!(matches!(
        focus,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Forward,
        } if block_id == &equation_id
    ));
    assert_eq!(session.selection().focus, parked_start(&after));
    assert_eq!(
        session.block_boundary_exit(),
        Some((equation_id, Direction::Forward))
    );
}

#[test]
fn shift_left_selects_document_start_equation_without_previous_paragraph() {
    let equation_block = equation("x");
    let equation_id = block_id(&equation_block);
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![equation_block, Block::Paragraph(after.clone())],
        paragraph_start(&after),
    );

    shift_left(&mut session);

    assert_eq!(session.selected_math_blocks(), &[equation_id]);
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    assert_eq!(text_endpoint(anchor), &paragraph_start(&after));
    assert!(matches!(
        focus,
        DocumentRangeEndpoint::BlockEdge {
            side: Direction::Backward,
            ..
        }
    ));
    let (start, end) = session
        .structure_selection_text_bounds()
        .unwrap_or_else(|| panic!("text bounds should resolve for a document-start equation"));
    assert_eq!(start.paragraph_id, after.id);
    assert_eq!(end.paragraph_id, after.id);
}

#[test]
fn shift_right_covers_adjacent_equations_in_document_order() {
    let before = paragraph_with_text("before");
    let first = equation("x");
    let second = align("y");
    let first_id = block_id(&first);
    let second_id = block_id(&second);
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            first,
            second,
            Block::Paragraph(after),
        ],
        paragraph_end(&before),
    );

    shift_right(&mut session);
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&first_id)
    );

    shift_right(&mut session);
    assert_eq!(
        session.selected_math_blocks(),
        &[first_id, second_id.clone()]
    );
    let (_, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    assert!(matches!(
        focus,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Forward,
        } if block_id == &second_id
    ));
}

#[test]
fn invalid_block_edge_range_is_rejected_without_side_effects() {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after),
        ],
        paragraph_end(&before),
    );
    const UNREACHABLE: &str = "invalid range must be rejected";
    let document_before = session.document().clone();
    let selection_before = session.selection().clone();
    let generation_before = session.document_generation();
    let structure_before = session
        .structure_selection_range()
        .map(|(anchor, focus)| (anchor.clone(), focus.clone()));
    let can_undo_before = session.can_undo();

    let foreign_block = NodeId::generate();
    assert_eq!(
        session.install_structure_selection(StructureSelection {
            anchor: DocumentRangeEndpoint::Text(paragraph_end(&before)),
            focus: DocumentRangeEndpoint::BlockEdge {
                block_id: foreign_block,
                side: Direction::Forward,
            },
        }),
        Err(EditError::InvalidPosition),
        "{UNREACHABLE}"
    );
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(
        session.structure_selection_range(),
        structure_before
            .as_ref()
            .map(|(anchor, focus)| (anchor, focus))
    );
    assert_eq!(session.can_undo(), can_undo_before);

    let invalid_text = Pos {
        story: StoryId::Body,
        paragraph_id: before.id.clone(),
        offset: "before".len() + 1,
        affinity: Affinity::Downstream,
    };
    assert_eq!(
        session.install_structure_selection(StructureSelection {
            anchor: DocumentRangeEndpoint::Text(invalid_text),
            focus: DocumentRangeEndpoint::Text(paragraph_start(&before)),
        }),
        Err(EditError::InvalidPosition),
        "{UNREACHABLE}"
    );
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
}

#[test]
fn shift_left_at_document_start_keeps_selection_and_history_unchanged() {
    let first = paragraph_with_text("first");
    let mut session = session_with_body(
        vec![Block::Paragraph(first.clone())],
        paragraph_start(&first),
    );
    let selection_before = session.selection().clone();
    let generation_before = session.document_generation();

    let effect = session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-left at document start failed: {error}"));

    assert!(!effect.selection_changed);
    assert!(!effect.document_changed);
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.document_generation(), generation_before);
    assert!(!session.has_structure_selection());
    assert!(!session.can_undo());
}

#[test]
fn horizontal_candidate_crossing_equation_keeps_the_host_covered() {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let equation_id = block_id(&equation_block);
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after.clone()),
        ],
        paragraph_end(&before),
    );
    let generation = NavigationGeneration {
        session_id: session.session_id().clone(),
        document_generation: session.document_generation(),
        font_generation: 0,
    };

    let effect = session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(NavigationCandidate::Document {
                focus: paragraph_start(&after),
                visual_affinity: crate::VisualAffinity::Downstream,
                generation,
            }),
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("candidate extension failed: {error}"));

    assert!(effect.selection_changed);
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    assert_eq!(text_endpoint(anchor), &paragraph_end(&before));
    assert_eq!(text_endpoint(focus), &paragraph_start(&after));
}

#[test]
fn vertical_shift_down_covers_adjacent_equation_without_layout_candidate() {
    let before = paragraph_with_text("before");
    let equation_block = equation("x");
    let equation_id = block_id(&equation_block);
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after.clone()),
        ],
        paragraph_end(&before),
    );

    session
        .run(Command::EditorKey(EditorKey::DownArrow {
            candidate: None,
            extend: true,
            structure: false,
        }))
        .unwrap_or_else(|error| panic!("shift-down failed: {error}"));

    assert_eq!(session.selected_math_blocks(), &[equation_id]);
    assert_eq!(session.selection().focus, parked_start(&after));
    assert!(session.structure_selection_range().is_some());

    session
        .run(Command::EditorKey(EditorKey::UpArrow {
            candidate: None,
            extend: false,
            structure: false,
        }))
        .unwrap_or_else(|error| panic!("up failed: {error}"));
    assert!(session.math_edit().is_some());
}

#[test]
fn shift_right_walks_align_cells_then_converts_to_whole_block_range() {
    let before = paragraph_with_text("before");
    let align_block = align_with_cells(&["x", "y"]);
    let align_id = block_id(&align_block);
    let cells = align_cell_ids(&align_block);
    assert_eq!(cells.len(), 2);
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            align_block,
            Block::Paragraph(after),
        ],
        paragraph_end(&before),
    );
    session
        .run(Command::EnterMath(Direction::Forward))
        .unwrap_or_else(|error| panic!("entering align math failed: {error}"));
    let edit = session
        .math_edit()
        .unwrap_or_else(|| panic!("align math edit missing"));
    let MathEditBranch::Tree { anchor, .. } = &edit.branch else {
        panic!("expected align math tree branch");
    };
    assert_eq!(anchor.offset, 0);
    assert_eq!(edit.cursor, 0);
    assert_eq!(current_align_cell(&session), Some(cells[0].clone()));

    // 第一步只在本单元格内扩展光标，锚点留在原位。
    shift_right(&mut session);
    let edit = session
        .math_edit()
        .unwrap_or_else(|| panic!("first shift left the align host"));
    let MathEditBranch::Tree { anchor, .. } = &edit.branch else {
        panic!("expected align math tree branch");
    };
    assert_eq!(current_align_cell(&session), Some(cells[0].clone()));
    assert_eq!(anchor.offset, 0);
    assert_eq!(edit.cursor, 1);
    assert!(session.structure_selection_range().is_none());

    // 第二步越过当前单元格边界，进入下一个单元格起点（重新坍缩）。
    shift_right(&mut session);
    let edit = session
        .math_edit()
        .unwrap_or_else(|| panic!("second shift should move to the next align cell"));
    let MathEditBranch::Tree { anchor, .. } = &edit.branch else {
        panic!("expected align math tree branch");
    };
    assert_eq!(current_align_cell(&session), Some(cells[1].clone()));
    assert_eq!(anchor.offset, 0);
    assert_eq!(edit.cursor, 0);

    // 第三步在最后一个单元格内扩展，仍不产生文档结构范围。
    shift_right(&mut session);
    let edit = session
        .math_edit()
        .unwrap_or_else(|| panic!("third shift left the last align cell"));
    let MathEditBranch::Tree { anchor, .. } = &edit.branch else {
        panic!("expected align math tree branch");
    };
    assert_eq!(current_align_cell(&session), Some(cells[1].clone()));
    assert_eq!(anchor.offset, 0);
    assert_eq!(edit.cursor, 1);
    assert!(session.structure_selection_range().is_none());

    // 第四步到达最后一个单元格的宿主边界，转换为整块 Align 结构范围。
    shift_right(&mut session);
    assert!(session.math_edit().is_none());
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&align_id)
    );
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    assert!(matches!(
        anchor,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Backward,
        } if block_id == &align_id
    ));
    assert!(matches!(
        focus,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Forward,
        } if block_id == &align_id
    ));
}

#[test]
fn shift_right_at_section_child_end_selects_whole_section() {
    let body = paragraph_with_text("body");
    let section = section("Heading", vec![Block::Paragraph(body.clone())]);
    let section_id = environment_id(&section);
    let mut session = session_with_body(vec![section], paragraph_end(&body));

    shift_right(&mut session);

    assert_eq!(
        session.selected_block_environments(),
        std::slice::from_ref(&section_id)
    );
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    assert!(matches!(
        anchor,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Backward,
        } if block_id == &section_id
    ));
    assert!(matches!(
        focus,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Forward,
        } if block_id == &section_id
    ));
    assert_eq!(
        session.block_boundary_exit(),
        Some((section_id, Direction::Forward))
    );
}

#[test]
fn shift_left_at_section_first_child_start_selects_whole_section() {
    let body = paragraph_with_text("body");
    let section = section("Heading", vec![Block::Paragraph(body.clone())]);
    let section_id = environment_id(&section);
    let mut session = session_with_body(vec![section], paragraph_start(&body));

    shift_left(&mut session);

    assert_eq!(
        session.selected_block_environments(),
        std::slice::from_ref(&section_id)
    );
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    assert!(matches!(
        anchor,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Forward,
        } if block_id == &section_id
    ));
    assert!(matches!(
        focus,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Backward,
        } if block_id == &section_id
    ));
}

#[test]
fn shift_right_from_before_section_selects_it_wholly() {
    let before = paragraph_with_text("before");
    let body = paragraph_with_text("body");
    let section = section("Heading", vec![Block::Paragraph(body)]);
    let section_id = environment_id(&section);
    let after = paragraph_with_text("after");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            section,
            Block::Paragraph(after.clone()),
        ],
        paragraph_end(&before),
    );

    shift_right(&mut session);

    assert_eq!(
        session.selected_block_environments(),
        std::slice::from_ref(&section_id)
    );
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    // 锚点仍在前方段落内，焦点已到环境外后边界，环境整体进入范围。
    assert_eq!(text_endpoint(anchor), &paragraph_end(&before));
    assert!(matches!(
        focus,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Forward,
        } if block_id == &section_id
    ));
}

#[test]
fn shift_right_from_section_title_end_selects_whole_section() {
    let body = paragraph_with_text("body");
    let section = section("Heading", vec![Block::Paragraph(body)]);
    let section_id = environment_id(&section);
    let title = match &section {
        Block::Environment(environment) => environment
            .title
            .clone()
            .unwrap_or_else(|| panic!("section title missing")),
        _ => unreachable!(),
    };
    let mut session = session_with_body(vec![section], title_end(&title, &section_id));

    shift_right(&mut session);

    assert_eq!(
        session.selected_block_environments(),
        std::slice::from_ref(&section_id)
    );
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("structural range missing"));
    assert!(matches!(
        anchor,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Backward,
        } if block_id == &section_id
    ));
    assert!(matches!(
        focus,
        DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Forward,
        } if block_id == &section_id
    ));
}

#[test]
fn object_only_structure_range_keeps_ordered_text_bounds() -> Result<(), Box<dyn std::error::Error>>
{
    let before = paragraph_with_text("before");
    let after = paragraph_with_text("after");
    let custom = Block::Environment(BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Custom,
        definition_id: None,
        props: BTreeMap::new(),
        title: None,
        children: Vec::new(),
        extra: BTreeMap::new(),
    });
    let custom_id = environment_id(&custom);
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        custom,
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    session.select_block_environment(custom_id.clone())?;

    let (start, end) = session
        .structure_selection_text_bounds()
        .unwrap_or_else(|| panic!("text bounds missing"));

    // 范围只有块对象、没有可截断文字时，两个文本边界必须按文档顺序折叠，
    // 不能返回起点在后、终点在前的倒置范围。
    assert!(
        start.paragraph_id != after.id || end.paragraph_id != before.id,
        "object-only bounds must not invert: {start:?}..{end:?}"
    );
    assert_eq!(start, end);
    assert_eq!(session.selected_block_environments(), &[custom_id]);
    Ok(())
}

#[test]
fn horizontal_shift_in_paragraph_keeps_inline_environment_without_structure_selection() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session creation failed: {error}"));
    session
        .run(Command::InsertText("abc".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    let focus = Pos {
        story: StoryId::Body,
        paragraph_id: session.selection().focus.paragraph_id.clone(),
        offset: 1,
        affinity: Affinity::Downstream,
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: focus.clone(),
            focus,
        })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));
    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))
        .unwrap_or_else(|error| panic!("inline format failed: {error}"));
    session
        .run(Command::InsertText("xy".to_owned()))
        .unwrap_or_else(|error| panic!("formatted text insertion failed: {error}"));
    let view = session
        .current_environment_view()
        .unwrap_or_else(|error| panic!("environment view failed: {error}"));
    assert!(
        !view.inline().is_empty(),
        "caret should start inside the inline environment"
    );

    // 从环境终点向环境内部扩选一格：仍是纯文本步骤。
    shift_left(&mut session);

    assert!(!session.has_structure_selection());
    assert!(session.structure_selection_range().is_none());
    assert!(session.selected_math_blocks().is_empty());
    let view = session
        .current_environment_view()
        .unwrap_or_else(|error| panic!("environment view failed: {error}"));
    assert!(
        !view.inline().is_empty(),
        "pure text extension must not hide the inline environment highlight"
    );
}

#[test]
fn vertical_shift_up_in_paragraph_middle_does_not_fake_a_range() {
    let equation_block = equation("x");
    let paragraph = paragraph_with_text("abcdef");
    let mut session = session_with_body(
        vec![equation_block, Block::Paragraph(paragraph.clone())],
        Pos {
            offset: 2,
            ..paragraph_start(&paragraph)
        },
    );
    let selection_before = session.selection().clone();
    let generation_before = session.document_generation();

    let effect = session
        .run(Command::EditorKey(EditorKey::UpArrow {
            candidate: None,
            extend: true,
            structure: false,
        }))
        .unwrap_or_else(|error| panic!("shift-up failed: {error}"));

    assert!(!effect.selection_changed);
    assert!(!effect.document_changed);
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.document_generation(), generation_before);
    assert!(!session.has_structure_selection());
}

fn paragraph_text_of(session: &Session, paragraph_id: &NodeId) -> Option<String> {
    session
        .document()
        .body
        .iter()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) if &paragraph.id == paragraph_id => {
                let mut text = String::new();
                for inline in &paragraph.inlines {
                    if let ParagraphContent::Text(flow) = inline {
                        for run in &flow.runs {
                            text.push_str(&run.text);
                        }
                    }
                }
                Some(text)
            }
            _ => None,
        })
}

/// 复现 Critical 手势：从公式 MathTree 反向转换后正向扩选，不得留下空覆盖隐藏范围。
fn session_with_selected_equation_then_reversed_range() -> Session {
    let before = paragraph_with_text("AB");
    let equation_block = equation("x");
    let after = paragraph_with_text("BC");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after.clone()),
        ],
        paragraph_end(&before),
    );
    session
        .run(Command::EnterMath(Direction::Forward))
        .unwrap_or_else(|error| panic!("entering equation failed: {error}"));
    shift_left(&mut session);
    session
}

#[test]
fn shift_right_from_selected_equation_clears_hidden_range_and_keeps_typing_live() {
    let mut session = session_with_selected_equation_then_reversed_range();
    let equation_id = session
        .document()
        .body
        .iter()
        .find_map(|block| match block {
            Block::Equation(equation) => Some(equation.id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("equation missing"));
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );

    // 反向扩选后正向一步：范围两端折叠到公式同一外侧，覆盖为空。
    shift_right(&mut session);

    assert!(!session.has_structure_selection());
    assert!(session.structure_selection_range().is_none());
    assert!(session.selected_math_blocks().is_empty());
    assert!(session.selected_block_environments().is_empty());
    let after_id = session.selection().focus.paragraph_id.clone();
    assert_eq!(
        paragraph_text_of(&session, &after_id).as_deref(),
        Some("BC")
    );

    // 输入不应被隐藏范围拦截；块边界输入按既有规则物化段落，退格能删除刚输入的字符。
    let typed = session
        .run(Command::InsertText("X".to_owned()))
        .unwrap_or_else(|error| panic!("typing after gesture failed: {error}"));
    assert!(typed.document_changed);
    assert!(!session.has_structure_selection());
    assert!(session.structure_selection_range().is_none());
    assert_eq!(session.document().body.len(), 4);
    assert!(
        session
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Equation(equation) if equation.id == equation_id))
    );
    assert_eq!(
        paragraph_text_of(&session, &session.selection().focus.paragraph_id).as_deref(),
        Some("X")
    );

    let erased = session
        .run(Command::Delete(Direction::Backward))
        .unwrap_or_else(|error| panic!("backspace after typing failed: {error}"));
    assert!(erased.document_changed);
    assert!(!session.has_structure_selection());
    assert!(session.structure_selection_range().is_none());
    assert!(!session.document().body.iter().any(|block| {
        matches!(block, Block::Paragraph(paragraph)
            if paragraph_text_of(&session, &paragraph.id).as_deref() == Some("X"))
    }));
    assert!(
        session
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Equation(equation) if equation.id == equation_id))
    );
}

#[test]
fn backspace_after_cleared_hidden_range_matches_plain_caret_rules() {
    let before = paragraph_with_text("AB");
    let equation_block = equation("x");
    let after = paragraph_with_text("BC");
    let body = vec![
        Block::Paragraph(before.clone()),
        equation_block,
        Block::Paragraph(after.clone()),
    ];
    let mut session = session_with_selected_equation_then_reversed_range();
    shift_right(&mut session);
    assert!(!session.has_structure_selection());

    let effect = session
        .run(Command::Delete(Direction::Backward))
        .unwrap_or_else(|error| panic!("backspace after gesture failed: {error}"));

    // 对照：普通光标停在同一段落起点时的既有退格规则。
    let mut plain = session_with_body(body, paragraph_start(&after));
    let plain_effect = plain
        .run(Command::Delete(Direction::Backward))
        .unwrap_or_else(|error| panic!("plain backspace failed: {error}"));

    assert_eq!(effect, plain_effect);
    // 两个会话的节点 ID 不同，只比较可观察的文档形状与落点。
    assert_eq!(session.document().body.len(), plain.document().body.len());
    assert_eq!(
        session
            .document()
            .body
            .iter()
            .filter(|block| matches!(block, Block::Equation(_)))
            .count(),
        1
    );
    assert_eq!(
        session.selection().focus.offset,
        plain.selection().focus.offset
    );
    assert_eq!(
        paragraph_text_of(&session, &session.selection().focus.paragraph_id).as_deref(),
        Some("BC")
    );
    assert!(!session.has_structure_selection());
    assert!(session.structure_selection_range().is_none());
}

#[test]
fn ime_commit_replaces_selected_equation_as_one_transaction() {
    let before = paragraph_with_text("AB");
    let equation_block = equation("x");
    let equation_id = block_id(&equation_block);
    let after = paragraph_with_text("BC");
    let mut session = session_with_body(
        vec![
            Block::Paragraph(before.clone()),
            equation_block,
            Block::Paragraph(after),
        ],
        paragraph_end(&before),
    );
    shift_right(&mut session);
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );

    session
        .run(Command::UpdateComposition {
            id: 1,
            preedit: "X".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition preview failed: {error}"));
    assert!(session.composition().is_some());
    assert!(
        session.has_structure_selection(),
        "IME preview must not drop the structural range"
    );

    let document_before = session.document().clone();
    let generation_before = session.document_generation();
    session
        .run(Command::CommitComposition {
            id: 1,
            text: "X".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition commit failed: {error}"));

    assert!(session.composition().is_none());
    assert!(!session.has_structure_selection());
    assert!(session.selected_math_blocks().is_empty());
    assert_eq!(session.document_generation(), generation_before + 1);
    assert!(
        !session
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Equation(equation) if equation.id == equation_id))
    );
    assert!(session.can_undo());

    // 一次撤销恢复公式与正文，说明替换范围与 IME 提交共用一个历史记录。
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    assert_eq!(session.document(), &document_before);
}
