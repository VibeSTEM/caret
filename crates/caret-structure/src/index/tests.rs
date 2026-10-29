//! 验证结构索引的文档顺序、嵌套范围和标题派生范围。

use super::*;
use caret_doc::{
    BlockEnvironment, BlockEnvironmentKind, Document, InlineNode, NodeId, Paragraph, Run,
};
use std::collections::BTreeMap;

fn paragraph(text: &str, level: Option<u64>) -> Paragraph {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(InlineNode::Run(Run {
        text: text.to_owned(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }));
    if let Some(level) = level {
        paragraph
            .props
            .insert("outlineLevel".to_owned(), serde_json::json!(level));
    }
    paragraph
}

#[test]
fn nested_environment_stack_is_stable_id_ordered() {
    let mut document = Document::blank();
    let section_id = NodeId::generate();
    let custom_id = NodeId::generate();
    let text = paragraph("inside", None);
    let text_id = text.id.clone();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: section_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: None,
        children: vec![Block::Environment(BlockEnvironment {
            id: custom_id.clone(),
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children: vec![Block::Paragraph(text)],
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    })];

    let index = StructureIndex::build(&document);
    let stack = index.at(&StructurePosition {
        paragraph_id: text_id.clone(),
        offset_utf8: 3,
    });
    assert!(stack.iter().any(|entry| entry.id == section_id));
    assert!(stack.iter().any(|entry| entry.id == custom_id));
    assert!(index.is_ancestor(&section_id, &custom_id));
    assert!(!index.is_ancestor(&custom_id, &section_id));
    assert_eq!(
        stack.last().and_then(|entry| entry.paragraph_id.as_ref()),
        Some(&text_id)
    );
}

#[test]
fn heading_scope_ends_before_next_same_or_higher_heading() {
    let mut document = Document::blank();
    let first = paragraph("first", Some(1));
    let child = paragraph("child", Some(2));
    let body = paragraph("body", None);
    let second = paragraph("second", Some(1));
    let first_id = first.id.clone();
    let child_id = child.id.clone();
    let body_id = body.id.clone();
    let second_id = second.id.clone();
    document.body = [first, child, body, second]
        .into_iter()
        .map(Block::Paragraph)
        .collect();

    let index = StructureIndex::build(&document);
    let first_scopes = index.structures().iter().find(|entry| {
        entry.id == first_id && matches!(entry.kind, StructureKind::Heading { level: 1 })
    });
    assert!(first_scopes.is_some());
    let Some(first_scope) = first_scopes else {
        return;
    };
    assert!(first_scope.range.paragraph_ordinals.contains(&0));
    assert!(first_scope.range.paragraph_ordinals.contains(&1));
    assert!(first_scope.range.paragraph_ordinals.contains(&2));
    assert!(!first_scope.range.paragraph_ordinals.contains(&3));
    assert_eq!(index.adjacent_paragraph(&first_id, true), Some(&child_id));
    assert_eq!(index.adjacent_paragraph(&body_id, true), Some(&second_id));
}

#[test]
fn structure_stack_orders_heading_and_nested_environment_by_document_tree() {
    let mut document = Document::blank();
    let heading = paragraph("heading", Some(1));
    let heading_id = heading.id.clone();
    let nested_id = NodeId::generate();
    let child = paragraph("child", None);
    let child_id = child.id.clone();
    document.body = vec![
        Block::Paragraph(heading),
        Block::Environment(BlockEnvironment {
            id: nested_id.clone(),
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children: vec![Block::Paragraph(child)],
            extra: BTreeMap::new(),
        }),
    ];

    let index = StructureIndex::build(&document);
    let stack = index.at(&StructurePosition {
        paragraph_id: child_id.clone(),
        offset_utf8: 0,
    });
    let ids = stack
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![heading_id, nested_id, child_id]);
}

#[test]
fn environment_title_is_a_navigation_paragraph_owned_by_its_environment() {
    let mut document = Document::blank();
    let environment_id = NodeId::generate();
    let title = paragraph("title", None);
    let title_id = title.id.clone();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(title),
        children: vec![Block::Paragraph(paragraph("body", None))],
        extra: BTreeMap::new(),
    })];

    let index = StructureIndex::build(&document);
    assert_eq!(index.title_environment_id(&title_id), Some(&environment_id));
    assert_eq!(index.paragraph_id_at(0), Some(&title_id));
    let stack = index.at(&StructurePosition {
        paragraph_id: title_id.clone(),
        offset_utf8: 0,
    });
    assert_eq!(stack.len(), 2);
    assert_eq!(stack[0].id, environment_id);
    assert_eq!(stack[1].id, title_id);
}
