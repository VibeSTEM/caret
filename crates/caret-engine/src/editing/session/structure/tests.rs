//! 验证结构命令跨段落导航及离开嵌套环境的可观察行为。

use super::*;
use crate::editing::EditorKey;
use caret_doc::{Block, BlockEnvironment, BlockEnvironmentKind, Document, NodeId, Paragraph};
use std::collections::BTreeMap;

fn paragraph() -> Paragraph {
    Paragraph::empty(NodeId::generate())
}

fn paragraph_text(paragraph: &Paragraph) -> String {
    paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            caret_doc::InlineNode::Run(run) => Some(run.text.as_str()),
            _ => None,
        })
        .collect()
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
                BlockEnvironmentKind::Custom,
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

#[test]
fn section_command_converts_current_paragraph_and_undo_redo_restore_structure()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let title = paragraph();
    let title_id = title.id.clone();
    document.body = vec![Block::Paragraph(title)];
    let mut session = Session::new(document)?;
    let original_position = session.selection().focus.clone();

    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("section".to_owned()))?;
    assert_eq!(
        session
            .command_line()
            .and_then(|line| line.selected_candidate())
            .map(|candidate| candidate.command_id),
        Some("struct.section.create")
    );
    let effect = session.run(Command::AcceptCommandLine)?;

    assert!(effect.document_changed);
    let Some(position_map) = effect.position_map.as_ref() else {
        return Err("Section creation must publish a position map".into());
    };
    let mapped_position = position_map.map(&original_position);
    assert!(session.command_line().is_none());
    let Block::Environment(section) = &session.document().body[0] else {
        panic!("section command should create a block environment");
    };
    assert_eq!(section.kind, BlockEnvironmentKind::Section);
    assert_eq!(
        section.title.as_ref().map(|paragraph| &paragraph.id),
        Some(&title_id)
    );
    assert!(section.children.is_empty());
    assert_eq!(
        session.selection().focus.story,
        StoryId::EnvironmentTitle(section.id.clone())
    );
    assert_eq!(mapped_position.story, session.selection().focus.story);
    assert_eq!(mapped_position.paragraph_id, title_id);
    assert_eq!(mapped_position.offset, original_position.offset);
    assert_eq!(mapped_position.affinity, original_position.affinity);
    assert_eq!(
        session.structure_index().at(&StructurePosition {
            paragraph_id: title_id.clone(),
            offset_utf8: 0,
        })[0]
            .id,
        section.id
    );

    session.run(Command::Undo)?;
    assert!(matches!(session.document().body[0], Block::Paragraph(_)));
    assert_eq!(session.selection().focus.story, StoryId::Body);
    assert_eq!(session.selection().focus.paragraph_id, title_id);

    session.run(Command::Redo)?;
    assert!(matches!(session.document().body[0], Block::Environment(_)));
    assert_eq!(
        session.selection().focus.story,
        StoryId::EnvironmentTitle(match &session.document().body[0] {
            Block::Environment(environment) => environment.id.clone(),
            _ => unreachable!(),
        })
    );
    Ok(())
}

#[test]
fn custom_environment_command_wraps_current_paragraph_and_focus_index_tracks_it()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let mut content = paragraph();
    content
        .inlines
        .push(caret_doc::InlineNode::Run(caret_doc::Run {
            text: "content".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }));
    let content_id = content.id.clone();
    document.body = vec![Block::Paragraph(content)];
    let mut session = Session::new(document)?;

    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("environment".to_owned()))?;
    let effect = session.run(Command::AcceptCommandLine)?;

    assert!(effect.document_changed);
    let Block::Environment(custom) = &session.document().body[0] else {
        panic!("custom environment command should wrap the current paragraph");
    };
    assert_eq!(custom.kind, BlockEnvironmentKind::Custom);
    assert!(custom.title.is_none());
    assert_eq!(custom.children.len(), 1);
    let Block::Paragraph(child) = &custom.children[0] else {
        panic!("custom environment should preserve the wrapped paragraph");
    };
    assert_eq!(child.id, content_id);
    assert_eq!(paragraph_text(child), "content");
    let stack = session.structure_index().at(&StructurePosition {
        paragraph_id: content_id,
        offset_utf8: 0,
    });
    assert_eq!(stack.len(), 2);
    assert_eq!(stack[0].kind, StructureKind::Custom);

    session.run(Command::Undo)?;
    assert!(matches!(session.document().body[0], Block::Paragraph(_)));
    session.run(Command::Redo)?;
    assert!(matches!(session.document().body[0], Block::Environment(_)));
    Ok(())
}

#[test]
fn environment_creation_rejects_nesting_overflow_without_partial_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut content = Block::Paragraph(paragraph());
    for _ in 0..64 {
        content = environment(BlockEnvironmentKind::Custom, vec![content]);
    }
    let mut document = Document::blank();
    document.body = vec![content];
    let mut session = Session::new(document)?;
    let before_document = session.document().clone();
    let before_selection = session.selection().clone();
    let before_generation = session.document_generation();

    let result = session.run(Command::CreateBlockEnvironment(
        BlockEnvironmentKind::Section,
    ));

    assert!(matches!(result, Err(EditError::InvalidDocument(_))));
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.selection(), &before_selection);
    assert_eq!(session.document_generation(), before_generation);
    assert!(!session.can_undo());
    Ok(())
}
