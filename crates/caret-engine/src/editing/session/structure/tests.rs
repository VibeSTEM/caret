//! 验证结构命令跨段落导航及离开嵌套环境的可观察行为。

use super::*;
use crate::editing::EditorKey;
use caret_doc::{Block, BlockEnvironment, BlockEnvironmentKind, Document, NodeId, Paragraph};
use std::collections::BTreeMap;

fn paragraph() -> Paragraph {
    Paragraph::empty(NodeId::generate())
}

fn environment(kind: BlockEnvironmentKind, children: Vec<Block>) -> Block {
    Block::Environment(BlockEnvironment {
        id: NodeId::generate(),
        kind,
        definition_id: None,
        props: BTreeMap::new(),
        title: None,
        children,
        extra: BTreeMap::new(),
    })
}

#[test]
fn structural_navigation_moves_between_nested_paragraphs_and_exits_container()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let title = paragraph();
    let nested = paragraph();
    let outside = paragraph();
    let title_id = title.id.clone();
    let nested_id = nested.id.clone();
    let outside_id = outside.id.clone();
    document.body = vec![
        environment(
            BlockEnvironmentKind::Section,
            vec![environment(
                BlockEnvironmentKind::Showcase,
                vec![Block::Paragraph(nested)],
            )],
        ),
        Block::Paragraph(outside),
    ];
    if let Block::Environment(section) = &mut document.body[0] {
        section.title = Some(title);
    }

    let mut session = Session::new(document)?;
    assert_eq!(session.selection().focus.paragraph_id, title_id);
    session.run(Command::MoveStructure(Direction::Forward))?;
    assert_eq!(session.selection().focus.paragraph_id, nested_id);
    session.run(Command::ExitStructure)?;
    assert_eq!(session.selection().focus.paragraph_id, outside_id);
    Ok(())
}

#[test]
fn structural_navigation_at_document_edge_is_a_noop() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::new(Document::blank())?;
    let before = session.selection().clone();
    let effect = session.run(Command::MoveStructure(Direction::Backward))?;
    assert!(!effect.selection_changed);
    assert_eq!(*session.selection(), before);
    Ok(())
}

#[test]
fn exiting_a_trailing_environment_moves_to_the_previous_outside_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let before = paragraph();
    let before_id = before.id.clone();
    let inside = paragraph();
    let inside_id = inside.id.clone();
    document.body = vec![
        Block::Paragraph(before),
        environment(
            BlockEnvironmentKind::Section,
            vec![Block::Paragraph(inside)],
        ),
    ];

    let mut session = Session::new(document)?;
    session.run(Command::MoveStructure(Direction::Forward))?;
    assert_eq!(session.selection().focus.paragraph_id, inside_id);

    session.run(Command::ExitStructure)?;

    assert_eq!(session.selection().focus.paragraph_id, before_id);
    assert_eq!(session.selection().focus.affinity, Affinity::Upstream);
    Ok(())
}

#[test]
fn exiting_with_a_selection_first_collapses_it_to_the_focus()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::new(Document::blank())?;
    session.run(Command::InsertText("text".to_owned()))?;
    let focus = session.selection().focus.clone();
    session.run(Command::SetSelection(Some(Selection {
        anchor: Pos {
            offset: 0,
            ..focus.clone()
        },
        focus: focus.clone(),
    })))?;

    let effect = session.run(Command::ExitStructure)?;

    assert!(effect.selection_changed);
    assert_eq!(session.selection().anchor, focus);
    assert_eq!(session.selection().focus, focus);
    Ok(())
}

#[test]
fn escape_collapses_selection_before_leaving_the_current_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let title = paragraph();
    let body = paragraph();
    let outside = paragraph();
    let body_id = body.id.clone();
    let outside_id = outside.id.clone();
    document.body = vec![
        environment(BlockEnvironmentKind::Section, vec![Block::Paragraph(body)]),
        Block::Paragraph(outside),
    ];
    if let Block::Environment(section) = &mut document.body[0] {
        section.title = Some(title);
    }

    let mut session = Session::new(document)?;
    session.run(Command::MoveStructure(Direction::Forward))?;
    session.run(Command::InsertText("text".to_owned()))?;
    let focus = session.selection().focus.clone();
    session.run(Command::SetSelection(Some(Selection {
        anchor: Pos {
            offset: 0,
            ..focus.clone()
        },
        focus: focus.clone(),
    })))?;

    let collapse = session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(collapse.selection_changed);
    assert_eq!(session.selection().focus.paragraph_id, body_id);
    assert_eq!(session.selection().anchor, focus);
    assert_eq!(session.selection().focus, focus);

    let exit = session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(exit.selection_changed);
    assert_eq!(session.selection().focus.paragraph_id, outside_id);
    Ok(())
}

#[test]
fn escape_cancels_command_input_before_leaving_the_current_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let title = paragraph();
    let body = paragraph();
    let outside = paragraph();
    let body_id = body.id.clone();
    let outside_id = outside.id.clone();
    document.body = vec![
        environment(BlockEnvironmentKind::Section, vec![Block::Paragraph(body)]),
        Block::Paragraph(outside),
    ];
    if let Block::Environment(section) = &mut document.body[0] {
        section.title = Some(title);
    }

    let mut session = Session::new(document)?;
    session.run(Command::MoveStructure(Direction::Forward))?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("section".to_owned()))?;

    let cancel = session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.command_line().is_none());
    assert!(!cancel.selection_changed);
    assert_eq!(session.selection().focus.paragraph_id, body_id);

    let exit = session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(exit.selection_changed);
    assert_eq!(session.selection().focus.paragraph_id, outside_id);
    Ok(())
}
