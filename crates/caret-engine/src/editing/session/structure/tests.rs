//! 验证结构命令跨段落导航及离开嵌套环境的可观察行为。

use super::*;
use crate::editing::EditorKey;
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, InlineNode, NodeId, Paragraph, Run,
};
use std::collections::BTreeMap;

fn paragraph() -> Paragraph {
    Paragraph::empty(NodeId::generate())
}

fn paragraph_with_text(text: &str) -> Paragraph {
    let mut paragraph = paragraph();
    paragraph.inlines.push(InlineNode::Run(Run {
        text: text.to_owned(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }));
    paragraph
}

fn paragraph_text(paragraph: &Paragraph) -> String {
    fn collect(nodes: &[InlineNode], text: &mut String) {
        for node in nodes {
            match node {
                InlineNode::Run(run) => text.push_str(&run.text),
                InlineNode::Environment(environment) => collect(&environment.children, text),
                InlineNode::SoftBreak(_) | InlineNode::Opaque(_) => {}
            }
        }
    }

    let mut text = String::new();
    collect(&paragraph.inlines, &mut text);
    text
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
fn structure_selection_expands_from_paragraph_to_containing_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let first = paragraph_with_text("alpha");
    let first_id = first.id.clone();
    let second = paragraph_with_text("beta");
    let second_id = second.id.clone();
    let mut document = Document::blank();
    document.body = vec![
        environment(
            BlockEnvironmentKind::Custom,
            vec![Block::Paragraph(first), Block::Paragraph(second)],
        ),
        Block::Paragraph(paragraph_with_text("outside")),
    ];
    let mut session = Session::new(document)?;

    let paragraph_effect = session.run(Command::ExpandStructureSelection)?;
    assert!(paragraph_effect.selection_changed);
    assert_eq!(session.selection().anchor.paragraph_id, first_id);
    assert_eq!(session.selection().anchor.offset, 0);
    assert_eq!(session.selection().focus.paragraph_id, first_id);
    assert_eq!(session.selection().focus.offset, "alpha".len());

    let environment_effect = session.run(Command::ExpandStructureSelection)?;
    assert!(environment_effect.selection_changed);
    assert_eq!(session.selection().anchor.paragraph_id, first_id);
    assert_eq!(session.selection().anchor.offset, 0);
    assert_eq!(session.selection().focus.paragraph_id, second_id);
    assert_eq!(session.selection().focus.offset, "beta".len());

    let unchanged = session.selection().clone();
    let effect = session.run(Command::ExpandStructureSelection)?;
    assert!(!effect.selection_changed);
    assert_eq!(*session.selection(), unchanged);
    Ok(())
}

#[test]
fn right_arrow_exits_section_and_backspace_removes_only_empty_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let mut section = match environment(BlockEnvironmentKind::Section, Vec::new()) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    section.title = Some(paragraph());
    let mut document = Document::blank();
    document.body = vec![
        Block::Environment(section),
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let mut session = Session::new(document)?;

    let exit = session.run(Command::ExitBlockEnvironment(Direction::Forward))?;
    assert!(exit.selection_changed);
    assert!(!exit.document_changed);
    assert!(matches!(session.document.body[1], Block::Paragraph(_)));
    assert_eq!(session.selection().focus.offset, 0);

    let deletion = session.run(Command::Delete(Direction::Backward))?;
    assert!(deletion.document_changed);
    assert!(
        !session
            .document
            .body
            .iter()
            .any(|block| matches!(block, Block::Environment(_)))
    );
    session.run(Command::Undo)?;
    assert!(matches!(session.document.body[0], Block::Environment(_)));
    Ok(())
}

#[test]
fn undoing_environment_creation_clears_its_virtual_exit_before_typing()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::new(Document::blank())?;
    session.run(Command::CreateBlockEnvironment(
        BlockEnvironmentKind::Section,
    ))?;
    session.run(Command::ExitBlockEnvironment(Direction::Forward))?;
    assert!(session.block_environment_exit().is_some());

    session.run(Command::Undo)?;
    assert!(session.block_environment_exit().is_none());
    let inserted = session.run(Command::InsertText("first".to_owned()))?;
    assert!(inserted.document_changed);
    assert_eq!(
        paragraph_text(match &session.document.body[0] {
            Block::Paragraph(paragraph) => paragraph,
            _ => return Err("撤销环境创建后应能在正文输入".into()),
        }),
        "first"
    );
    Ok(())
}

#[test]
fn composition_at_virtual_environment_exit_is_one_undoable_edit()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::new(Document::blank())?;
    session.run(Command::CreateBlockEnvironment(
        BlockEnvironmentKind::Section,
    ))?;
    session.run(Command::ExitBlockEnvironment(Direction::Forward))?;
    session.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    let generation_before_composition = session.document_generation();
    session.run(Command::UpdateComposition {
        id: 42,
        preedit: "候选".to_owned(),
    })?;

    let committed = session.run(Command::CommitComposition {
        id: 42,
        text: "你好".to_owned(),
    })?;
    assert!(committed.document_changed);
    assert_eq!(
        session.document_generation(),
        generation_before_composition + 1
    );
    assert_eq!(session.document.body.len(), 2);
    let paragraph = match &session.document.body[1] {
        Block::Paragraph(paragraph) => paragraph,
        _ => return Err("组字提交应在环境外创建段落".into()),
    };
    let [InlineNode::Environment(font)] = paragraph.inlines.as_slice() else {
        return Err("虚拟环境外的组字提交应继承待输入字体环境".into());
    };
    assert_eq!(font.kind, caret_doc::InlineEnvironmentKind::Font);
    assert_eq!(
        font.props
            .get("fontFamily")
            .and_then(serde_json::Value::as_str),
        Some("Noto Serif")
    );
    assert_eq!(
        paragraph_text(&Paragraph {
            inlines: font.children.clone(),
            ..Paragraph::empty(NodeId::generate())
        }),
        "你好"
    );

    session.run(Command::Undo)?;
    assert_eq!(session.document.body.len(), 1);
    assert!(matches!(session.document.body[0], Block::Environment(_)));
    session.run(Command::Redo)?;
    assert_eq!(session.document.body.len(), 2);
    assert_eq!(
        paragraph_text(match &session.document.body[1] {
            Block::Paragraph(paragraph) => paragraph,
            _ => return Err("重做应恢复环境外的输入".into()),
        }),
        "你好"
    );
    Ok(())
}

#[test]
fn composition_preview_at_environment_exit_tracks_each_inserted_line()
-> Result<(), Box<dyn std::error::Error>> {
    for direction in [Direction::Backward, Direction::Forward] {
        let mut session = Session::new(Document::blank())?;
        session.run(Command::CreateBlockEnvironment(
            BlockEnvironmentKind::Section,
        ))?;
        session.run(Command::ExitBlockEnvironment(direction))?;
        session.run(Command::UpdateComposition {
            id: 43,
            preedit: "甲\n😀乙\n末".to_owned(),
        })?;
        for (cursor, expected_text, expected_offset) in [(0, "甲", 0), (4, "😀乙", 4), (7, "末", 3)]
        {
            let (preview, range) = session.composition_preview_with_range(cursor)?;
            let focus = &preview.selection().focus;
            let location = preview
                .paragraph_index
                .get(&focus.paragraph_id)
                .unwrap_or_else(|| panic!("preedit focus paragraph must be indexed"));
            let paragraph = paragraph_at_location(&preview.document.body, location)
                .unwrap_or_else(|| panic!("preedit focus paragraph must exist in the document"));
            assert_eq!(paragraph_text(paragraph), expected_text);
            assert_eq!(focus.offset, expected_offset);
            assert_eq!(range.anchor.paragraph_id, focus.paragraph_id);
            assert_eq!(range.focus.paragraph_id, focus.paragraph_id);
            assert_eq!(range.anchor.offset, 0);
            assert_eq!(range.focus.offset, expected_text.len());
        }
        assert_eq!(session.document.body.len(), 1);
    }
    Ok(())
}

#[test]
fn backspace_in_paragraph_after_nonempty_section_deletes_text()
-> Result<(), Box<dyn std::error::Error>> {
    let mut section = match environment(
        BlockEnvironmentKind::Section,
        vec![Block::Paragraph(paragraph_with_text("body"))],
    ) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    section.title = Some(paragraph_with_text("title"));
    let paragraph = paragraph_with_text("after");
    let paragraph_id = paragraph.id.clone();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        Block::Environment(section),
        Block::Paragraph(paragraph),
    ];
    let mut session = Session::new(document)?;
    let position = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 3,
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: position.clone(),
        focus: position,
    })))?;

    let effect = session.run(Command::Delete(Direction::Backward))?;

    assert!(effect.document_changed);
    assert_eq!(
        paragraph_text(match &session.document.body[2] {
            Block::Paragraph(paragraph) => paragraph,
            _ => return Err("Section 后方的普通段落应保留".into()),
        }),
        "afer"
    );
    assert!(matches!(session.document.body[1], Block::Environment(_)));
    Ok(())
}

#[test]
fn backspace_at_paragraph_start_adjacent_to_nonempty_section_preserves_both_blocks()
-> Result<(), Box<dyn std::error::Error>> {
    let mut section = match environment(
        BlockEnvironmentKind::Section,
        vec![Block::Paragraph(paragraph_with_text("body"))],
    ) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    section.title = Some(paragraph_with_text("title"));
    let paragraph = paragraph_with_text("after");
    let paragraph_id = paragraph.id.clone();
    let mut document = Document::blank();
    document.body = vec![Block::Environment(section), Block::Paragraph(paragraph)];
    let mut session = Session::new(document)?;
    let position = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: position.clone(),
        focus: position.clone(),
    })))?;

    let effect = session.run(Command::Delete(Direction::Backward))?;

    assert!(!effect.document_changed);
    assert_eq!(session.selection().focus, position);
    assert_eq!(session.document.body.len(), 2);
    assert!(matches!(session.document.body[0], Block::Environment(_)));
    assert_eq!(
        paragraph_text(match &session.document.body[1] {
            Block::Paragraph(paragraph) => paragraph,
            _ => return Err("Section 后方的普通段落应保留".into()),
        }),
        "after"
    );
    Ok(())
}

#[test]
fn backspace_on_selected_section_deletes_nested_subtree_and_undo_redo_restore_it()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let title = paragraph_with_text("heading");
    let body = paragraph_with_text("body");
    let nested = environment(
        BlockEnvironmentKind::Custom,
        vec![Block::Paragraph(paragraph_with_text("nested"))],
    );
    let section = match environment(
        BlockEnvironmentKind::Section,
        vec![Block::Paragraph(body), nested],
    ) {
        Block::Environment(mut section) => {
            section.title = Some(title);
            Block::Environment(section)
        }
        _ => unreachable!(),
    };
    let section_id = match &section {
        Block::Environment(section) => section.id.clone(),
        _ => unreachable!(),
    };
    let after = paragraph_with_text("after");
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(before), section, Block::Paragraph(after)];
    let mut session = Session::new(document)?;
    session.select_block_environment(section_id.clone())?;

    let deleted = session.run(Command::Delete(Direction::Backward))?;

    assert!(deleted.document_changed);
    assert!(deleted.selection_changed);
    assert_eq!(session.document_generation(), 1);
    assert!(session.selected_block_environment().is_none());
    assert_eq!(session.document.body.len(), 2);
    assert!(matches!(session.document.body[0], Block::Paragraph(_)));
    assert!(matches!(session.document.body[1], Block::Paragraph(_)));
    assert_eq!(session.selection().focus.offset, "before".len());

    session.run(Command::Undo)?;
    assert_eq!(session.document.body.len(), 3);
    let Block::Environment(restored_section) = &session.document.body[1] else {
        return Err("撤销后应恢复 Section 环境".into());
    };
    assert_eq!(restored_section.id, section_id);
    assert!(matches!(
        restored_section.children[1],
        Block::Environment(_)
    ));
    assert_eq!(
        session.selection().focus.paragraph_id,
        session.selection().anchor.paragraph_id
    );

    session.run(Command::Redo)?;
    assert_eq!(session.document.body.len(), 2);
    assert!(matches!(session.document.body[0], Block::Paragraph(_)));
    assert!(matches!(session.document.body[1], Block::Paragraph(_)));
    Ok(())
}

#[test]
fn deleting_nested_environments_with_identical_paragraph_ranges_is_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    let leaf = paragraph_with_text("nested content");
    let leaf_id = leaf.id.clone();
    let inner = environment(BlockEnvironmentKind::Custom, vec![Block::Paragraph(leaf)]);
    let outer = environment(BlockEnvironmentKind::Custom, vec![inner]);
    let outer_id = match &outer {
        Block::Environment(environment) => environment.id.clone(),
        _ => unreachable!(),
    };
    let inner_id = match &outer {
        Block::Environment(environment) => match &environment.children[0] {
            Block::Environment(inner) => inner.id.clone(),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![outer];
    let mut session = Session::new(document)?;
    let anchor = Pos {
        story: StoryId::Body,
        paragraph_id: leaf_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let focus = Pos {
        offset: "nested content".len(),
        affinity: Affinity::Upstream,
        ..anchor.clone()
    };
    let before = session.document().clone();

    session.select_block_environments_from_range(anchor, focus, VisualAffinity::Downstream)?;

    assert_eq!(session.selected_block_environments(), &[outer_id]);
    assert_ne!(session.selected_block_environments(), &[inner_id]);
    let effect = session.run(Command::Delete(Direction::Backward))?;
    assert!(effect.document_changed);
    assert_eq!(session.document().body.len(), 1);
    assert!(matches!(session.document().body[0], Block::Paragraph(_)));
    assert!(session.can_undo());

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &before);
    session.run(Command::Redo)?;
    assert!(matches!(session.document().body[0], Block::Paragraph(_)));
    Ok(())
}

#[test]
fn failed_structure_deletion_does_not_partially_edit_earlier_paragraphs()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let before_id = before.id.clone();
    let environment = environment(
        BlockEnvironmentKind::Custom,
        vec![Block::Paragraph(paragraph_with_text("inside"))],
    );
    let environment_id = match &environment {
        Block::Environment(environment) => environment.id.clone(),
        _ => unreachable!(),
    };
    let mut after = paragraph_with_text("after");
    let after_id = after.id.clone();
    after
        .inlines
        .push(InlineNode::SoftBreak(caret_doc::SoftBreak {
            extra: BTreeMap::from([("futureMetadata".to_owned(), serde_json::Value::Bool(true))]),
        }));
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        environment,
        Block::Paragraph(after),
    ];
    let mut session = Session::new(document)?;
    session.select_block_environments_from_range(
        Pos {
            story: StoryId::Body,
            paragraph_id: before_id,
            offset: 0,
            affinity: Affinity::Downstream,
        },
        Pos {
            story: StoryId::Body,
            paragraph_id: after_id,
            offset: "after\n".len(),
            affinity: Affinity::Upstream,
        },
        VisualAffinity::Downstream,
    )?;
    let selected_generation = session.document_generation();
    let selected_document = session.document().clone();
    let selected_selection = session.selection().clone();
    assert!(
        session
            .selected_block_environments()
            .iter()
            .any(|id| id == &environment_id)
    );

    assert!(session.run(Command::Delete(Direction::Backward)).is_err());
    assert_eq!(session.document(), &selected_document);
    assert_eq!(session.selection(), &selected_selection);
    assert_eq!(session.document_generation(), selected_generation);
    assert!(!session.can_undo());
    Ok(())
}

#[test]
fn deleting_the_only_selected_environment_keeps_an_editable_blank_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let custom = environment(
        BlockEnvironmentKind::Custom,
        vec![Block::Paragraph(paragraph_with_text("content"))],
    );
    let custom_id = match &custom {
        Block::Environment(environment) => environment.id.clone(),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![custom];
    let mut session = Session::new(document)?;
    session.select_block_environment(custom_id.clone())?;

    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(session.document.body.len(), 1);
    let Block::Paragraph(placeholder) = &session.document.body[0] else {
        return Err("删除唯一环境后应保留可编辑空段落".into());
    };
    assert!(TextMap::new(placeholder)?.text.is_empty());
    assert_eq!(session.selection().focus.paragraph_id, placeholder.id);

    session.run(Command::Undo)?;
    assert!(matches!(session.document.body[0], Block::Environment(_)));
    session.run(Command::Redo)?;
    let Block::Paragraph(placeholder) = &session.document.body[0] else {
        return Err("重做后应再次保留空段落".into());
    };
    assert!(TextMap::new(placeholder)?.text.is_empty());
    Ok(())
}

#[test]
fn deleting_last_environment_preserves_breaks_and_restores_inserted_fallback()
-> Result<(), Box<dyn std::error::Error>> {
    for break_before_environment in [false, true] {
        let content = paragraph_with_text("content");
        let content_id = content.id.clone();
        let environment = environment(
            BlockEnvironmentKind::Custom,
            vec![Block::Paragraph(content)],
        );
        let environment_id = match &environment {
            Block::Environment(environment) => environment.id.clone(),
            _ => unreachable!(),
        };
        let page_break = Block::ExplicitBreak(caret_doc::ExplicitBreak::default());
        let mut document = Document::blank();
        document.body = if break_before_environment {
            vec![page_break, environment]
        } else {
            vec![environment, page_break]
        };
        let mut session = Session::new(document)?;
        let original = session.document().clone();
        session.select_block_environments_from_range(
            Pos {
                story: StoryId::Body,
                paragraph_id: content_id.clone(),
                offset: 0,
                affinity: Affinity::Downstream,
            },
            Pos {
                story: StoryId::Body,
                paragraph_id: content_id,
                offset: "content".len(),
                affinity: Affinity::Upstream,
            },
            VisualAffinity::Downstream,
        )?;
        assert_eq!(session.selected_block_environments(), &[environment_id]);

        session.run(Command::Delete(Direction::Backward))?;
        assert!(
            session
                .document()
                .body
                .iter()
                .any(|block| matches!(block, Block::Paragraph(_)))
        );
        assert!(
            session
                .document()
                .body
                .iter()
                .any(|block| matches!(block, Block::ExplicitBreak(_)))
        );
        let after_delete = session.document().clone();

        session.run(Command::Undo)?;
        assert_eq!(session.document(), &original);
        session.run(Command::Redo)?;
        assert_eq!(session.document(), &after_delete);
    }
    Ok(())
}

#[test]
fn explicit_environment_removal_preserves_content_and_is_undoable()
-> Result<(), Box<dyn std::error::Error>> {
    let mut section = match environment(
        BlockEnvironmentKind::Section,
        vec![Block::Paragraph(paragraph_with_text("body"))],
    ) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    section.title = Some(paragraph_with_text("title"));
    let mut document = Document::blank();
    document.body = vec![Block::Environment(section)];
    let mut session = Session::new(document)?;

    let effect = session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(effect.document_changed);
    assert_eq!(session.document.body.len(), 2);
    assert_eq!(
        paragraph_text(match &session.document.body[0] {
            Block::Paragraph(paragraph) => paragraph,
            _ => return Err("标题应提升为普通段落".into()),
        }),
        "title"
    );
    assert_eq!(
        paragraph_text(match &session.document.body[1] {
            Block::Paragraph(paragraph) => paragraph,
            _ => return Err("正文应保留为普通段落".into()),
        }),
        "body"
    );
    session.run(Command::Undo)?;
    assert!(matches!(session.document.body[0], Block::Environment(_)));
    Ok(())
}

#[test]
fn exiting_environment_requires_collapsed_selection_at_paragraph_edge()
-> Result<(), Box<dyn std::error::Error>> {
    let title = paragraph_with_text("title");
    let title_id = title.id.clone();
    let body = paragraph_with_text("body");
    let body_id = body.id.clone();
    let mut section = match environment(BlockEnvironmentKind::Section, vec![Block::Paragraph(body)])
    {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    section.title = Some(title);
    let mut document = Document::blank();
    document.body = vec![Block::Environment(section)];
    let mut session = Session::new(document)?;

    let middle = Pos {
        story: StoryId::Body,
        paragraph_id: body_id.clone(),
        offset: 1,
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: middle.clone(),
        focus: middle.clone(),
    })))?;
    let unchanged = session.document_generation();
    let effect = session.run(Command::ExitBlockEnvironment(Direction::Forward))?;
    assert!(!effect.document_changed);
    assert_eq!(session.document_generation(), unchanged);

    let start = Pos {
        offset: 0,
        ..middle.clone()
    };
    let end = Pos {
        offset: "body".len(),
        affinity: Affinity::Upstream,
        ..middle
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: start,
        focus: end,
    })))?;
    let unchanged = session.document_generation();
    let effect = session.run(Command::ExitBlockEnvironment(Direction::Forward))?;
    assert!(!effect.document_changed);
    assert_eq!(session.document_generation(), unchanged);
    assert_eq!(session.selection().focus.paragraph_id, body_id);
    assert_ne!(session.selection().focus.paragraph_id, title_id);
    Ok(())
}

#[test]
fn unwrapping_from_body_maps_the_environment_title_story() -> Result<(), Box<dyn std::error::Error>>
{
    let title = paragraph_with_text("title");
    let title_id = title.id.clone();
    let body = paragraph_with_text("body");
    let body_id = body.id.clone();
    let mut section = match environment(BlockEnvironmentKind::Section, vec![Block::Paragraph(body)])
    {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    let environment_id = section.id.clone();
    section.title = Some(title);
    let mut document = Document::blank();
    document.body = vec![Block::Environment(section)];
    let mut session = Session::new(document)?;
    let body_position = Pos {
        story: StoryId::Body,
        paragraph_id: body_id,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: body_position.clone(),
        focus: body_position,
    })))?;

    let effect = session.run(Command::RemoveBlockEnvironmentPreservingContent)?;
    let title_position = Pos {
        story: StoryId::EnvironmentTitle(environment_id),
        paragraph_id: title_id,
        offset: 2,
        affinity: Affinity::Downstream,
    };
    let mapped = effect
        .position_map
        .ok_or("解除环境时应返回标题位置映射")?
        .map(&title_position);
    assert_eq!(mapped.story, StoryId::Body);
    assert_eq!(mapped.offset, 2);
    Ok(())
}

#[test]
fn backspace_during_composition_does_not_unwrap_empty_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let mut section = match environment(BlockEnvironmentKind::Section, Vec::new()) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    section.title = Some(paragraph());
    let mut document = Document::blank();
    document.body = vec![Block::Environment(section)];
    let mut session = Session::new(document)?;
    session.run(Command::UpdateComposition {
        id: 7,
        preedit: "候选".to_owned(),
    })?;

    assert!(matches!(
        session.run(Command::Delete(Direction::Backward)),
        Err(EditError::CompositionPending)
    ));
    assert!(session.composition().is_some());
    assert!(matches!(session.document.body[0], Block::Environment(_)));
    Ok(())
}

#[test]
fn backspace_removes_only_an_empty_environment_at_its_start()
-> Result<(), Box<dyn std::error::Error>> {
    let mut empty_section = match environment(BlockEnvironmentKind::Section, Vec::new()) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    empty_section.title = Some(paragraph());
    let mut empty_document = Document::blank();
    empty_document.body = vec![Block::Environment(empty_section)];
    let mut empty_session = Session::new(empty_document)?;

    let removed = empty_session.run(Command::Delete(Direction::Backward))?;
    assert!(removed.document_changed);
    assert!(matches!(
        empty_session.document.body[0],
        Block::Paragraph(_)
    ));
    empty_session.run(Command::Undo)?;
    assert!(matches!(
        empty_session.document.body[0],
        Block::Environment(_)
    ));

    let mut nonempty_section = match environment(BlockEnvironmentKind::Section, Vec::new()) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    nonempty_section.title = Some(paragraph_with_text("title"));
    let mut nonempty_document = Document::blank();
    nonempty_document.body = vec![Block::Environment(nonempty_section)];
    let mut nonempty_session = Session::new(nonempty_document)?;

    let unchanged = nonempty_session.run(Command::Delete(Direction::Backward))?;
    assert!(!unchanged.document_changed);
    assert!(matches!(
        nonempty_session.document.body[0],
        Block::Environment(_)
    ));
    let exit = nonempty_session.run(Command::ExitBlockEnvironment(Direction::Backward))?;
    assert!(exit.selection_changed);
    assert!(!exit.document_changed);
    assert!(matches!(
        nonempty_session.document.body[0],
        Block::Environment(_)
    ));
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
fn exiting_the_last_environment_creates_an_undoable_outside_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let inside = paragraph_with_text("inside");
    let inside_id = inside.id.clone();
    let mut document = Document::blank();
    document.body = vec![environment(
        BlockEnvironmentKind::Custom,
        vec![Block::Paragraph(inside)],
    )];
    let mut session = Session::new(document)?;

    let effect = session.run(Command::ExitStructure)?;

    assert!(effect.document_changed);
    assert!(effect.selection_changed);
    assert_eq!(session.document().body.len(), 2);
    let Block::Paragraph(outside) = &session.document().body[1] else {
        panic!("exiting the trailing environment should create a sibling paragraph");
    };
    assert!(outside.inlines.is_empty());
    assert_eq!(session.selection().focus.paragraph_id, outside.id);
    assert_eq!(
        session
            .structure_index()
            .at(&StructurePosition {
                paragraph_id: outside.id.clone(),
                offset_utf8: 0,
            })
            .len(),
        1
    );

    session.run(Command::Undo)?;
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(session.selection().focus.paragraph_id, inside_id);
    session.run(Command::Redo)?;
    assert_eq!(session.document().body.len(), 2);
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
fn section_command_on_existing_text_creates_a_separate_empty_section()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph_with_text("existing text"))];
    let mut session = Session::new(document)?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("section".to_owned()))?;
    let effect = session.run(Command::AcceptCommandLine)?;

    assert!(effect.document_changed);
    assert_eq!(session.document.body.len(), 2);
    assert_eq!(
        paragraph_text(match &session.document.body[0] {
            Block::Paragraph(paragraph) => paragraph,
            _ => return Err("现有文本应保留为普通段落".into()),
        }),
        "existing text"
    );
    let Block::Environment(section) = &session.document.body[1] else {
        return Err("命令应在新段落位置创建 Section".into());
    };
    assert_eq!(section.kind, BlockEnvironmentKind::Section);
    assert_eq!(
        section.title.as_ref().map(paragraph_text),
        Some(String::new())
    );

    session.run(Command::Undo)?;
    assert_eq!(session.document.body.len(), 1);
    assert_eq!(
        paragraph_text(match &session.document.body[0] {
            Block::Paragraph(paragraph) => paragraph,
            _ => return Err("撤销后原文本段落应保留".into()),
        }),
        "existing text"
    );
    Ok(())
}

#[test]
fn typing_at_virtual_environment_exit_materializes_one_outside_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let mut section = match environment(BlockEnvironmentKind::Section, Vec::new()) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    section.title = Some(paragraph_with_text("title"));
    let mut document = Document::blank();
    document.body = vec![Block::Environment(section)];
    let mut session = Session::new(document)?;

    let generation = session.document_generation();
    let exit = session.run(Command::ExitBlockEnvironment(Direction::Backward))?;
    assert!(exit.selection_changed);
    assert!(!exit.document_changed);
    assert_eq!(session.document_generation(), generation);
    assert_eq!(session.document.body.len(), 1);
    assert!(session.block_environment_exit().is_some());

    session.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    let inserted = session.run(Command::InsertText("outside".to_owned()))?;
    assert!(inserted.document_changed);
    assert_eq!(session.document.body.len(), 2);
    let paragraph = match &session.document.body[0] {
        Block::Paragraph(paragraph) => paragraph,
        _ => return Err("环境外输入应在 Section 前创建段落".into()),
    };
    let [InlineNode::Environment(font)] = paragraph.inlines.as_slice() else {
        return Err("虚拟环境外输入应继承待输入字体环境".into());
    };
    assert_eq!(font.kind, caret_doc::InlineEnvironmentKind::Font);
    assert_eq!(
        font.props
            .get("fontFamily")
            .and_then(serde_json::Value::as_str),
        Some("Noto Serif")
    );
    assert_eq!(
        paragraph_text(&Paragraph {
            inlines: font.children.clone(),
            ..Paragraph::empty(NodeId::generate())
        }),
        "outside"
    );
    session.run(Command::Undo)?;
    assert_eq!(session.document.body.len(), 1);
    assert!(matches!(session.document.body[0], Block::Environment(_)));
    Ok(())
}

#[test]
fn enter_at_virtual_environment_exit_materializes_outside_paragraphs()
-> Result<(), Box<dyn std::error::Error>> {
    for direction in [Direction::Backward, Direction::Forward] {
        for key in [EditorKey::Enter, EditorKey::ShiftEnter] {
            let mut section = match environment(
                BlockEnvironmentKind::Section,
                vec![Block::Paragraph(paragraph_with_text("inside"))],
            ) {
                Block::Environment(section) => section,
                _ => unreachable!(),
            };
            section.title = Some(paragraph_with_text("title"));
            let environment_id = section.id.clone();
            let title_id = section
                .title
                .as_ref()
                .map(|title| title.id.clone())
                .ok_or_else(|| std::io::Error::other("Section title should exist"))?;
            let body_id = match &section.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => unreachable!(),
            };
            let environment = Block::Environment(section);
            let mut document = Document::blank();
            document.body = vec![environment.clone()];
            let original_document = document.clone();
            let mut session = Session::new(document)?;

            let position = match direction {
                Direction::Backward => Pos {
                    story: StoryId::EnvironmentTitle(environment_id),
                    paragraph_id: title_id,
                    offset: 0,
                    affinity: Affinity::Downstream,
                },
                Direction::Forward => Pos {
                    story: StoryId::Body,
                    paragraph_id: body_id,
                    offset: "inside".len(),
                    affinity: Affinity::Upstream,
                },
            };
            session.run(Command::SetSelection(Some(Selection {
                anchor: position.clone(),
                focus: position,
            })))?;

            session.run(Command::ExitBlockEnvironment(direction))?;
            assert!(session.block_environment_exit().is_some());
            let effect = session.run(Command::EditorKey(key))?;

            assert!(effect.document_changed);
            assert!(session.block_environment_exit().is_none());
            assert_eq!(session.document.body.len(), 2);
            let environment_index = match direction {
                Direction::Backward => 1,
                Direction::Forward => 0,
            };
            assert_eq!(session.document.body[environment_index], environment);
            let outside_index = 1 - environment_index;
            let Block::Paragraph(outside) = &session.document.body[outside_index] else {
                return Err("Enter should materialize a Paragraph outside the environment".into());
            };
            let expected_offset = if key == EditorKey::ShiftEnter { 1 } else { 0 };
            assert_eq!(session.selection().focus.paragraph_id, outside.id);
            assert_eq!(session.selection().focus.offset, expected_offset);
            assert!(matches!(
                (key, outside.inlines.as_slice()),
                (EditorKey::Enter, []) | (EditorKey::ShiftEnter, [InlineNode::SoftBreak(_)])
            ));

            session.run(Command::InsertText("after".to_owned()))?;
            assert_eq!(session.document.body.len(), 2);
            assert_eq!(
                paragraph_text(match &session.document.body[outside_index] {
                    Block::Paragraph(paragraph) => paragraph,
                    _ => unreachable!(),
                }),
                "after"
            );
            session.run(Command::Undo)?;
            session.run(Command::Undo)?;
            assert_eq!(session.document, original_document);
        }
    }
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

    let effect = session.run(Command::CreateBlockEnvironment(
        BlockEnvironmentKind::Custom,
    ))?;

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
