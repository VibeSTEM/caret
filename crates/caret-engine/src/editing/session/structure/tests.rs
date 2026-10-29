//! 验证结构命令跨段落导航及离开嵌套环境的可观察行为。

use super::*;
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
