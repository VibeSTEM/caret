//! 验证鼠标范围与键盘导航共用同一结构选区提升判定。

use super::super::{Command, Direction, Pos, Selection, Session, StoryId};
use crate::VisualAffinity;
use crate::editing::{Affinity, DocumentRangeEndpoint};
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
    assert_eq!(session.document().body.len(), 2);
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
