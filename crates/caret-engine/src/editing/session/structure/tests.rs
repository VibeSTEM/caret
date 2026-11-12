//! 验证结构命令跨段落导航及离开嵌套环境的可观察行为。

use super::*;
use crate::editing::{DocumentRangeEndpoint, EditorKey, StructureSelection};
use caret_doc::{
    Align, Block, BlockEnvironment, BlockEnvironmentKind, Document, Equation, MathAlignRow,
    MathContent, MathFlow, MathNode, MathNodeKind, MathTree, NodeId, Paragraph, ParagraphContent,
    Run, SoftBreak, TextFlow,
};
use std::collections::BTreeMap;

fn paragraph() -> Paragraph {
    Paragraph::empty(NodeId::generate())
}

fn paragraph_with_text(text: &str) -> Paragraph {
    let mut paragraph = paragraph();
    paragraph.inlines.push(text_flow(Run {
        text: text.to_owned(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }));
    paragraph
}

fn paragraph_text(paragraph: &Paragraph) -> String {
    fn collect(nodes: &[ParagraphContent], text: &mut String) {
        for node in nodes {
            match node {
                ParagraphContent::Text(flow) => {
                    for run in &flow.runs {
                        text.push_str(&run.text);
                    }
                }
                ParagraphContent::Environment(environment) => collect(&environment.children, text),
                ParagraphContent::SoftBreak(_)
                | ParagraphContent::Opaque(_)
                | ParagraphContent::Math(_) => {}
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

fn math_tree(text: Option<&str>) -> MathTree {
    MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: MathFlow {
            id: NodeId::generate(),
            content: text
                .map(|text| {
                    vec![MathContent::Math(MathNode {
                        id: NodeId::generate(),
                        kind: MathNodeKind::Symbol,
                        text: Some(text.to_owned()),
                        attributes: BTreeMap::new(),
                        children: Vec::new(),
                        extra: BTreeMap::new(),
                    })]
                })
                .unwrap_or_default(),
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }
}

fn formula_environment(empty_formula: bool, use_align: bool) -> Block {
    let text = (!empty_formula).then_some("x");
    let equation = Block::Equation(Equation {
        id: NodeId::generate(),
        math_tree: math_tree(text),
        extra: BTreeMap::new(),
    });
    let align = Block::Align(Align {
        id: NodeId::generate(),
        rows: vec![MathAlignRow {
            id: NodeId::generate(),
            cells: vec![math_tree(text)],
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    });
    let formula = if use_align { align } else { equation };
    let nested = environment(BlockEnvironmentKind::Custom, vec![formula]);
    let mut outer = match environment(BlockEnvironmentKind::Section, vec![nested]) {
        Block::Environment(environment) => environment,
        _ => unreachable!(),
    };
    outer.title = Some(paragraph());
    Block::Environment(outer)
}

fn text_flow(run: Run) -> ParagraphContent {
    ParagraphContent::Text(TextFlow {
        runs: vec![run],
        extra: Default::default(),
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
fn current_environment_view_keeps_parent_after_virtual_inner_block_boundary_stop()
-> Result<(), Box<dyn std::error::Error>> {
    let inner_id = NodeId::generate();
    let mut section = match environment(
        BlockEnvironmentKind::Section,
        vec![Block::Environment(BlockEnvironment {
            id: inner_id.clone(),
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children: vec![Block::Paragraph(paragraph())],
            extra: BTreeMap::new(),
        })],
    ) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    section.title = Some(paragraph());
    let section_id = section.id.clone();
    let mut session = Session::new(Document {
        body: vec![Block::Environment(section)],
        ..Document::blank()
    })?;

    session.run(Command::MoveStructure(Direction::Forward))?;
    assert!(matches!(
        session.active_host_caret_position(),
        Some(crate::editing::HostCaretPosition::Inside {
            host_id,
            local: crate::editing::HostCaretLocalPosition::Paragraph(_),
            ..
        }) if host_id == inner_id
    ));
    session.run(Command::ExitBlockEnvironment(Direction::Forward))?;
    assert_eq!(
        session.block_boundary_exit().map(|(id, _)| id),
        Some(inner_id.clone())
    );

    let view = session.current_environment_view()?;
    assert_eq!(view.block().len(), 1);
    assert_eq!(view.block()[0].id.as_ref(), Some(&section_id));
    assert_eq!(
        view.block()[0].kind,
        crate::editing::EnvironmentKind::Section
    );
    assert!(view.inline().is_empty());
    Ok(())
}

#[test]
fn formula_block_exit_uses_host_parent_not_hidden_paragraph_anchor()
-> Result<(), Box<dyn std::error::Error>> {
    let mut first_section = match environment(
        BlockEnvironmentKind::Section,
        vec![Block::Paragraph(paragraph())],
    ) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    first_section.title = Some(paragraph());
    let first_section_id = first_section.id.clone();

    let formula_id = NodeId::generate();
    let second_section = match environment(
        BlockEnvironmentKind::Section,
        vec![
            Block::Equation(Equation {
                id: formula_id.clone(),
                math_tree: math_tree(None),
                extra: BTreeMap::new(),
            }),
            Block::Paragraph(paragraph()),
        ],
    ) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    let second_section_id = second_section.id.clone();
    let mut session = Session::new(Document {
        body: vec![
            Block::Environment(first_section),
            Block::Environment(second_section),
        ],
        ..Document::blank()
    })?;
    session.host_caret_position = Some(crate::editing::HostCaretPosition::outside(
        formula_id,
        Direction::Forward,
    ));

    let view = session.current_environment_view()?;
    assert_eq!(view.block().len(), 1);
    assert_eq!(view.block()[0].id.as_ref(), Some(&second_section_id));
    assert_ne!(view.block()[0].id.as_ref(), Some(&first_section_id));
    Ok(())
}

#[test]
fn active_host_position_projects_section_internal_position()
-> Result<(), Box<dyn std::error::Error>> {
    let section = match environment(
        BlockEnvironmentKind::Section,
        vec![Block::Paragraph(paragraph())],
    ) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    let section_id = section.id.clone();
    let mut session = Session::new(Document {
        body: vec![Block::Environment(section)],
        ..Document::blank()
    })?;
    session.run(Command::MoveStructure(Direction::Forward))?;
    assert!(matches!(
        session.active_host_caret_position(),
        Some(crate::editing::HostCaretPosition::Inside {
            host_id,
            local: crate::editing::HostCaretLocalPosition::Paragraph(_),
            ..
        }) if host_id == section_id
    ));
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
    assert!(session.block_boundary_exit().is_some());

    session.run(Command::Undo)?;
    assert!(session.block_boundary_exit().is_none());
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
fn composition_at_block_boundary_stop_is_one_undoable_edit()
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
    let [ParagraphContent::Environment(font)] = paragraph.inlines.as_slice() else {
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
fn composition_preview_at_block_boundary_tracks_each_inserted_line()
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
            let crate::editing::CompositionRange::Paragraph(range) = range else {
                panic!("paragraph preedit should return a paragraph range");
            };
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
    let selection_before_delete = session.selection().clone();

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
    assert_eq!(session.selection(), &selection_before_delete);
    assert!(session.selected_block_environment().is_none());

    session.run(Command::Redo)?;
    assert_eq!(session.document.body.len(), 2);
    assert!(matches!(session.document.body[0], Block::Paragraph(_)));
    assert!(matches!(session.document.body[1], Block::Paragraph(_)));
    Ok(())
}

#[test]
fn delete_key_deletes_selected_environment_and_mod_backspace_keeps_structure_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let first_body = paragraph_with_text("first");
    let first_body_id = first_body.id.clone();
    let first_environment = match environment(
        BlockEnvironmentKind::Section,
        vec![Block::Paragraph(first_body)],
    ) {
        Block::Environment(mut environment) => {
            environment.title = Some(paragraph_with_text("First"));
            Block::Environment(environment)
        }
        _ => unreachable!(),
    };
    let first_environment_id = match &first_environment {
        Block::Environment(environment) => environment.id.clone(),
        _ => unreachable!(),
    };
    let second_environment = environment(
        BlockEnvironmentKind::Section,
        vec![Block::Paragraph(paragraph_with_text("second"))],
    );
    let second_environment_id = match &second_environment {
        Block::Environment(environment) => environment.id.clone(),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![first_environment, second_environment];
    let mut session = Session::new(document)?;
    let first_body_position = Pos {
        story: StoryId::Body,
        paragraph_id: first_body_id,
        offset: 2,
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: first_body_position.clone(),
        focus: first_body_position.clone(),
    })))?;
    session.select_block_environment(second_environment_id.clone())?;

    let empty_undo = session.run(Command::Undo)?;
    assert!(!empty_undo.document_changed);
    assert_eq!(
        session.selected_block_environment(),
        Some(&second_environment_id)
    );

    let unchanged = session.run(Command::EditorKey(EditorKey::ModBackspace))?;

    assert!(!unchanged.document_changed);
    assert_eq!(session.document.body.len(), 2);
    assert_eq!(
        session.selected_block_environment(),
        Some(&second_environment_id)
    );
    assert_eq!(session.selection().focus, first_body_position);
    assert_eq!(
        session
            .structure_selection_range()
            .ok_or("结构选区应保留")?
            .0,
        &crate::editing::DocumentRangeEndpoint::BlockEdge {
            block_id: second_environment_id.clone(),
            side: Direction::Backward,
        }
    );

    let deleted = session.run(Command::Delete(Direction::Forward))?;

    assert!(deleted.document_changed);
    assert_eq!(session.document.body.len(), 1);
    assert!(matches!(
        &session.document.body[0],
        Block::Environment(environment) if environment.id == first_environment_id
    ));
    session.run(Command::Undo)?;
    assert_eq!(session.document.body.len(), 2);
    assert_eq!(session.selection().focus, first_body_position);
    session.run(Command::Redo)?;
    assert_eq!(session.document.body.len(), 1);
    Ok(())
}

#[test]
fn undo_notifies_when_only_visual_affinity_changes() -> Result<(), Box<dyn std::error::Error>> {
    let environment = environment(
        BlockEnvironmentKind::Section,
        vec![Block::Paragraph(paragraph_with_text("inside"))],
    );
    let environment_id = match &environment {
        Block::Environment(environment) => environment.id.clone(),
        _ => unreachable!(),
    };
    let following = paragraph_with_text("after");
    let following_id = following.id.clone();
    let mut document = Document::blank();
    document.body = vec![environment, Block::Paragraph(following)];
    let mut session = Session::new(document)?;
    let following_start = Pos {
        story: StoryId::Body,
        paragraph_id: following_id,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: following_start.clone(),
        focus: following_start.clone(),
    })))?;
    session.selection_visual_affinity = crate::VisualAffinity::Upstream;
    session.select_block_environment(environment_id)?;
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(session.selection().focus, following_start);
    assert_eq!(
        session.selection_visual_affinity(),
        crate::VisualAffinity::Downstream
    );

    let undone = session.run(Command::Undo)?;

    assert!(undone.selection_changed);
    assert_eq!(session.selection().focus, following_start);
    assert_eq!(
        session.selection_visual_affinity(),
        crate::VisualAffinity::Upstream
    );
    Ok(())
}

#[test]
fn failed_navigation_selection_preserves_the_current_structure_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let inside = paragraph_with_text("inside");
    let inside_id = inside.id.clone();
    let environment = environment(BlockEnvironmentKind::Custom, vec![Block::Paragraph(inside)]);
    let environment_id = match &environment {
        Block::Environment(environment) => environment.id.clone(),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(before), environment];
    let mut session = Session::new(document)?;
    session.select_block_environment(environment_id.clone())?;
    let selection_before = session.selection().clone();
    let selected_environments_before = session.selected_block_environments().to_vec();
    let structure_selection_before = session
        .structure_selection_range()
        .map(|(anchor, focus)| (anchor.clone(), focus.clone()));
    let invalid_position = Pos {
        story: StoryId::Body,
        paragraph_id: inside_id,
        offset: "inside".len() + 1,
        affinity: Affinity::Downstream,
    };

    assert!(matches!(
        session.set_navigation_selection(
            Selection {
                anchor: invalid_position.clone(),
                focus: invalid_position,
            },
            VisualAffinity::Downstream,
            crate::editing::NavigationIntent::Horizontal,
        ),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(
        session.selected_block_environments(),
        selected_environments_before
    );
    assert_eq!(
        session.structure_selection_range(),
        structure_selection_before
            .as_ref()
            .map(|(anchor, focus)| (anchor, focus))
    );
    assert_eq!(session.selected_block_environment(), Some(&environment_id));
    Ok(())
}

#[test]
fn shifted_keyboard_navigation_into_block_keeps_anchor_visual_affinity()
-> Result<(), Box<dyn std::error::Error>> {
    let before_text = "word ".repeat(80);
    let before = paragraph_with_text(&before_text);
    let before_id = before.id.clone();
    let inside = paragraph_with_text("inside");
    let inside_id = inside.id.clone();
    let after = paragraph_with_text("after");
    let after_id = after.id.clone();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        environment(BlockEnvironmentKind::Custom, vec![Block::Paragraph(inside)]),
        Block::Paragraph(after),
    ];
    let mut session = Session::new(document)?;
    let anchor = Pos {
        story: StoryId::Body,
        paragraph_id: before_id,
        offset: before_text.len() / 2,
        affinity: Affinity::Downstream,
    };
    session.set_selection_with_visual_affinities(
        Some(Selection {
            anchor: anchor.clone(),
            focus: anchor.clone(),
        }),
        VisualAffinity::Upstream,
        VisualAffinity::Upstream,
    )?;
    let focus = Pos {
        story: StoryId::Body,
        paragraph_id: inside_id,
        offset: "inside".len(),
        affinity: Affinity::Downstream,
    };

    let generation = crate::editing::NavigationGeneration {
        session_id: session.session_id().clone(),
        document_generation: session.document_generation(),
        font_generation: 0,
    };
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: Some(crate::editing::NavigationCandidate::Document {
            focus: focus.clone(),
            visual_affinity: VisualAffinity::Downstream,
            generation: generation.clone(),
        }),
        extend: true,
    }))?;

    let next_focus = Pos {
        story: StoryId::Body,
        paragraph_id: after_id,
        offset: "after".len(),
        affinity: Affinity::Downstream,
    };
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: Some(crate::editing::NavigationCandidate::Document {
            focus: next_focus.clone(),
            visual_affinity: VisualAffinity::Downstream,
            generation,
        }),
        extend: true,
    }))?;

    assert_eq!(session.selection().anchor, next_focus);
    assert_eq!(session.selection().focus, next_focus);
    assert_eq!(
        session.selection_anchor_visual_affinity(),
        VisualAffinity::Upstream
    );
    assert_eq!(
        session.selection_visual_affinity(),
        VisualAffinity::Downstream
    );
    let (structure_anchor, structure_focus) = session
        .structure_selection()
        .ok_or("keyboard extension should retain its structure selection range")?;
    assert_eq!(structure_anchor, &anchor);
    assert_eq!(structure_focus, &next_focus);
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
        .push(ParagraphContent::SoftBreak(caret_doc::SoftBreak {
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
        assert_eq!(
            session.selected_block_environments(),
            std::slice::from_ref(&environment_id)
        );

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
fn formulas_keep_empty_block_environments_from_backspace_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    for empty_formula in [false, true] {
        for use_align in [false, true] {
            let environment = formula_environment(empty_formula, use_align);
            let mut document = Document::blank();
            document.body = vec![environment.clone()];

            // Backspace at an empty environment title may unwrap only a truly empty shell.
            let mut session = Session::new(document.clone())?;
            let selection = session.selection().clone();
            let generation = session.document_generation();
            let effect = session.run(Command::Delete(Direction::Backward))?;
            assert!(!effect.document_changed);
            assert_eq!(session.document, document);
            assert_eq!(*session.selection(), selection);
            assert_eq!(session.document_generation(), generation);

            // Backspace at the following paragraph start uses the same content rule.
            let outside = paragraph();
            let outside_id = outside.id.clone();
            let mut adjacent_document = document.clone();
            adjacent_document.body.push(Block::Paragraph(outside));
            let mut adjacent_session = Session::new(adjacent_document.clone())?;
            let position = Pos {
                story: StoryId::Body,
                paragraph_id: outside_id,
                offset: 0,
                affinity: Affinity::Downstream,
            };
            adjacent_session.run(Command::SetSelection(Some(Selection {
                anchor: position.clone(),
                focus: position,
            })))?;
            let selection = adjacent_session.selection().clone();
            let generation = adjacent_session.document_generation();
            let effect = adjacent_session.run(Command::Delete(Direction::Backward))?;
            assert!(!effect.document_changed);
            assert_eq!(adjacent_session.document, adjacent_document);
            assert_eq!(*adjacent_session.selection(), selection);
            assert_eq!(adjacent_session.document_generation(), generation);

            // A virtual stop outside the environment must not delete its formula subtree.
            let mut exit_session = Session::new(document.clone())?;
            exit_session.run(Command::ExitBlockEnvironment(Direction::Backward))?;
            let exit = exit_session.block_boundary_exit();
            assert!(exit.is_some());
            let selection = exit_session.selection().clone();
            let generation = exit_session.document_generation();
            let effect = exit_session.run(Command::Delete(Direction::Forward))?;
            assert!(!effect.document_changed);
            assert_eq!(exit_session.document, document);
            assert_eq!(*exit_session.selection(), selection);
            assert_eq!(exit_session.block_boundary_exit(), exit);
            assert_eq!(exit_session.document_generation(), generation);
        }
    }
    Ok(())
}

#[test]
fn deleting_formula_environment_undo_redo_restores_exact_tree_and_focus()
-> Result<(), Box<dyn std::error::Error>> {
    for use_align in [false, true] {
        let mut original = Document::blank();
        let environment = formula_environment(false, use_align);
        let Block::Environment(block_environment) = &environment else {
            unreachable!();
        };
        let environment_id = block_environment.id.clone();
        original.body = vec![environment];

        let mut session = Session::new(original.clone())?;
        session.select_block_environment(environment_id.clone())?;
        let selection = session.selection().clone();
        assert_eq!(
            session.selected_block_environments(),
            std::slice::from_ref(&environment_id)
        );

        let deleted = session.run(Command::Delete(Direction::Backward))?;
        assert!(deleted.document_changed);
        assert_ne!(session.document(), &original);
        let deleted_document = session.document().clone();

        session.run(Command::Undo)?;
        assert_eq!(session.document(), &original);
        assert_eq!(session.selection(), &selection);

        session.run(Command::Redo)?;
        assert_eq!(session.document(), &deleted_document);
        session.run(Command::Undo)?;
        assert_eq!(session.document(), &original);
        assert_eq!(session.selection(), &selection);
    }
    Ok(())
}

#[test]
fn environment_content_classifies_each_block_variant_conservatively()
-> Result<(), Box<dyn std::error::Error>> {
    let empty_tree = math_tree(None);
    let cases = [
        (Block::Paragraph(paragraph()), false),
        (
            Block::Paragraph({
                let mut paragraph = paragraph();
                paragraph
                    .inlines
                    .push(ParagraphContent::SoftBreak(SoftBreak::default()));
                paragraph
            }),
            true,
        ),
        (
            Block::Equation(Equation {
                id: NodeId::generate(),
                math_tree: empty_tree.clone(),
                extra: BTreeMap::new(),
            }),
            true,
        ),
        (
            Block::Align(Align {
                id: NodeId::generate(),
                rows: vec![MathAlignRow {
                    id: NodeId::generate(),
                    cells: vec![empty_tree],
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            }),
            true,
        ),
        (environment(BlockEnvironmentKind::Custom, Vec::new()), false),
        (
            environment(
                BlockEnvironmentKind::Custom,
                vec![Block::Paragraph(paragraph_with_text("nested"))],
            ),
            true,
        ),
        (
            Block::ExplicitBreak(caret_doc::ExplicitBreak::default()),
            true,
        ),
        (
            Block::Opaque(caret_doc::OpaqueNode {
                raw: serde_json::json!({"type": "future_block"}),
            }),
            true,
        ),
    ];

    for (block, expected) in cases {
        let mut container = match environment(BlockEnvironmentKind::Custom, Vec::new()) {
            Block::Environment(environment) => environment,
            _ => unreachable!(),
        };
        container.children.push(block);
        assert_eq!(super::environment_has_content(&container)?, expected);
    }
    Ok(())
}

#[test]
fn unsupported_inline_content_keeps_backspace_failure_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    let content_paragraph = paragraph();
    let mut section = match environment(
        BlockEnvironmentKind::Section,
        vec![
            Block::Paragraph(content_paragraph),
            Block::Equation(Equation {
                id: NodeId::generate(),
                math_tree: math_tree(Some("x")),
                extra: BTreeMap::new(),
            }),
        ],
    ) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    section.title = Some(paragraph());
    let mut document = Document::blank();
    document.body = vec![Block::Environment(section)];
    let mut session = Session::new(document)?;

    // Inject content unsupported by TextMap after Session built its position indexes, so
    // the failure occurs inside the cleanup transaction rather than during initialization.
    let Block::Environment(section) = &mut session.document.body[0] else {
        unreachable!();
    };
    let Block::Paragraph(paragraph) = &mut section.children[0] else {
        unreachable!();
    };
    paragraph
        .inlines
        .push(ParagraphContent::Opaque(caret_doc::OpaqueNode {
            raw: serde_json::json!({"type": "future_inline"}),
        }));

    let document = session.document.clone();
    let selection = session.selection().clone();
    let generation = session.document_generation();
    assert!(matches!(
        session.run(Command::Delete(Direction::Backward)),
        Err(EditError::UnsupportedOpaqueContent)
    ));
    assert_eq!(session.document, document);
    assert_eq!(*session.selection(), selection);
    assert_eq!(session.document_generation(), generation);
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
    empty_session.run(Command::Redo)?;
    assert!(matches!(
        empty_session.document.body[0],
        Block::Paragraph(_)
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
fn typing_at_block_boundary_stop_materializes_one_outside_paragraph()
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
    assert!(session.block_boundary_exit().is_some());

    session.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    let inserted = session.run(Command::InsertText("outside".to_owned()))?;
    assert!(inserted.document_changed);
    assert_eq!(session.document.body.len(), 2);
    let paragraph = match &session.document.body[0] {
        Block::Paragraph(paragraph) => paragraph,
        _ => return Err("环境外输入应在 Section 前创建段落".into()),
    };
    let [ParagraphContent::Environment(font)] = paragraph.inlines.as_slice() else {
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
fn enter_at_block_boundary_stop_materializes_outside_paragraphs()
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
            assert!(session.block_boundary_exit().is_some());
            let effect = session.run(Command::EditorKey(key.clone()))?;

            assert!(effect.document_changed);
            assert!(session.block_boundary_exit().is_none());
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
                (EditorKey::Enter, []) | (EditorKey::ShiftEnter, [ParagraphContent::SoftBreak(_)])
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
    content.inlines.push(text_flow(caret_doc::Run {
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

fn equation_block() -> Block {
    Block::Equation(Equation {
        id: NodeId::generate(),
        math_tree: math_tree(Some("x")),
        extra: BTreeMap::new(),
    })
}

fn align_block() -> Block {
    Block::Align(Align {
        id: NodeId::generate(),
        rows: vec![MathAlignRow {
            id: NodeId::generate(),
            cells: vec![math_tree(Some("y"))],
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    })
}

fn block_id(block: &Block) -> NodeId {
    match block {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        Block::Environment(environment) => environment.id.clone(),
        Block::Equation(equation) => equation.id.clone(),
        Block::Align(align) => align.id.clone(),
        Block::ExplicitBreak(_) | Block::Opaque(_) => {
            unreachable!("测试块均有稳定身份")
        }
    }
}

fn body_position(paragraph: &Paragraph, offset: usize, affinity: Affinity) -> Pos {
    Pos {
        story: StoryId::Body,
        paragraph_id: paragraph.id.clone(),
        offset,
        affinity,
    }
}

fn select_text_endpoints(session: &mut Session, anchor: Pos, focus: Pos) -> Result<(), EditError> {
    session.install_structure_selection(StructureSelection {
        anchor: DocumentRangeEndpoint::Text(anchor),
        focus: DocumentRangeEndpoint::Text(focus),
    })
}

fn select_block_object(session: &mut Session, block_id: NodeId) -> Result<(), EditError> {
    session.install_structure_selection(StructureSelection {
        anchor: DocumentRangeEndpoint::BlockEdge {
            block_id: block_id.clone(),
            side: Direction::Backward,
        },
        focus: DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Forward,
        },
    })
}

fn paragraph_at_block(block: &Block) -> &Paragraph {
    match block {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("期望段落块"),
    }
}

#[test]
fn deleting_cross_host_range_removes_math_block_and_range_text_in_one_step()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("ab");
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let after = paragraph_with_text("cd");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    let original_selection = session.selection().clone();
    select_text_endpoints(
        &mut session,
        body_position(&before, 1, Affinity::Downstream),
        body_position(&after, 1, Affinity::Upstream),
    )?;
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );

    let effect = session.run(Command::Delete(Direction::Backward))?;

    assert!(effect.document_changed);
    assert!(effect.selection_changed);
    // 两端文本段落按纯文本跨段删除语义合并，公式块与范围中间的段落一并移除。
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "ad"
    );
    assert_eq!(
        session.selection().focus,
        body_position(&before, 1, Affinity::Downstream)
    );
    assert!(!session.has_structure_selection());
    assert!(session.selected_math_blocks().is_empty());
    assert!(session.selected_block_environments().is_empty());
    assert_eq!(session.document_generation(), 1);

    let generation_after_delete = session.document_generation();
    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    assert_eq!(session.selection(), &original_selection);
    assert!(!session.has_structure_selection());
    assert_ne!(session.document_generation(), generation_after_delete);
    session.run(Command::Redo)?;
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "ad"
    );
    assert_eq!(
        session.selection().focus,
        body_position(&before, 1, Affinity::Downstream)
    );
    Ok(())
}

#[test]
fn deleting_range_spanning_equation_align_and_section_undoes_as_one_operation()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("ab");
    let equation = equation_block();
    let align = align_block();
    let equation_id = block_id(&equation);
    let align_id = block_id(&align);
    let mut section = match environment(
        BlockEnvironmentKind::Section,
        vec![Block::Paragraph(paragraph_with_text("body"))],
    ) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    section.title = Some(paragraph_with_text("title"));
    let section_id = section.id.clone();
    let after = paragraph_with_text("cd");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Environment(section),
        align,
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    select_text_endpoints(
        &mut session,
        body_position(&before, 1, Affinity::Downstream),
        body_position(&after, 1, Affinity::Upstream),
    )?;
    assert_eq!(session.selected_math_blocks(), &[equation_id, align_id]);
    assert_eq!(
        session.selected_block_environments(),
        std::slice::from_ref(&section_id)
    );

    session.run(Command::Delete(Direction::Backward))?;

    // 公式块与环境删除后，两端文本段落按纯文本跨段删除语义合并。
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "ad"
    );

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    session.run(Command::Redo)?;
    assert_eq!(session.document().body.len(), 1);
    Ok(())
}

#[test]
fn deleting_cross_host_range_with_equation_and_middle_paragraph_merges_like_text_deletion()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("abc");
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let middle = paragraph_with_text("mid");
    let after = paragraph_with_text("xyz");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Paragraph(middle.clone()),
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    let original_selection = session.selection().clone();
    select_text_endpoints(
        &mut session,
        body_position(&before, 1, Affinity::Downstream),
        body_position(&after, 1, Affinity::Upstream),
    )?;
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    let generation_before_delete = session.document_generation();

    session.run(Command::Delete(Direction::Backward))?;

    // 两端文本段落合并为一段，中间段落与公式块一并移除。
    assert_eq!(session.document().body.len(), 1);
    let merged = paragraph_at_block(&session.document().body[0]);
    assert_eq!(merged.id, before.id);
    assert_eq!(paragraph_text(merged), "ayz");
    assert_eq!(
        session.selection().focus,
        body_position(&before, 1, Affinity::Downstream)
    );
    assert!(session.math_edit().is_none());
    assert!(!session.has_structure_selection());
    assert!(session.selected_math_blocks().is_empty());
    assert!(session.selected_block_environments().is_empty());
    assert_eq!(session.document_generation(), generation_before_delete + 1);

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    assert_eq!(session.selection(), &original_selection);
    assert!(!session.has_structure_selection());
    session.run(Command::Redo)?;
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "ayz"
    );
    assert_eq!(
        session.selection().focus,
        body_position(&before, 1, Affinity::Downstream)
    );
    Ok(())
}

#[test]
fn deleting_cross_host_range_merges_endpoints_and_removes_all_middle_paragraphs()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("left");
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let align = align_block();
    let align_id = block_id(&align);
    let first_middle = paragraph_with_text("m1");
    let second_middle = paragraph_with_text("m2");
    let after = paragraph_with_text("right");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Paragraph(first_middle),
        align,
        Block::Paragraph(second_middle),
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    select_text_endpoints(
        &mut session,
        body_position(&before, 2, Affinity::Downstream),
        body_position(&after, 3, Affinity::Upstream),
    )?;
    assert_eq!(session.selected_math_blocks(), &[equation_id, align_id]);
    let generation_before_delete = session.document_generation();

    session.run(Command::Delete(Direction::Backward))?;

    // 所有中间段落（包括两个公式块之间与之后的段落）都随合并移除。
    assert_eq!(session.document().body.len(), 1);
    let merged = paragraph_at_block(&session.document().body[0]);
    assert_eq!(merged.id, before.id);
    assert_eq!(paragraph_text(merged), "leht");
    assert_eq!(
        session.selection().focus,
        body_position(&before, 2, Affinity::Downstream)
    );
    assert_eq!(session.document_generation(), generation_before_delete + 1);

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    assert!(!session.can_undo());
    session.run(Command::Redo)?;
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "leht"
    );
    Ok(())
}

#[test]
fn incompatible_endpoint_paragraph_styles_keep_structure_deletion_atomic()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("abc");
    let equation = equation_block();
    let middle = paragraph_with_text("mid");
    let mut after = paragraph_with_text("xyz");
    // 端点段落样式不一致时沿用纯文本合并的兼容规则。
    after
        .mark_style
        .insert("bold".to_owned(), serde_json::Value::Bool(true));
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Paragraph(middle),
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    select_text_endpoints(
        &mut session,
        body_position(&before, 1, Affinity::Downstream),
        body_position(&after, 1, Affinity::Upstream),
    )?;
    let selected_document = session.document().clone();
    let selected_selection = session.selection().clone();
    let selected_generation = session.document_generation();
    let selected_math = session.selected_math_blocks().to_vec();
    let selected_environments = session.selected_block_environments().to_vec();

    assert_eq!(
        session.run(Command::Delete(Direction::Backward)),
        Err(EditError::IncompatibleParagraphMerge)
    );

    assert_eq!(session.document(), &selected_document);
    assert_eq!(session.selection(), &selected_selection);
    assert_eq!(session.document_generation(), selected_generation);
    assert_eq!(session.selected_math_blocks(), selected_math.as_slice());
    assert_eq!(
        session.selected_block_environments(),
        selected_environments.as_slice()
    );
    assert!(session.has_structure_selection());
    assert!(!session.can_undo());
    Ok(())
}

#[test]
fn deleting_block_edge_range_removes_middle_paragraphs_instead_of_clearing_them()
-> Result<(), Box<dyn std::error::Error>> {
    let first = environment(
        BlockEnvironmentKind::Custom,
        vec![Block::Paragraph(paragraph_with_text("first"))],
    );
    let first_id = block_id(&first);
    let middle = paragraph_with_text("middle");
    let middle_id = middle.id.clone();
    let second = environment(
        BlockEnvironmentKind::Custom,
        vec![Block::Paragraph(paragraph_with_text("second"))],
    );
    let second_id = block_id(&second);
    let mut document = Document::blank();
    document.body = vec![first, Block::Paragraph(middle), second];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    session.install_structure_selection(StructureSelection {
        anchor: DocumentRangeEndpoint::BlockEdge {
            block_id: first_id.clone(),
            side: Direction::Backward,
        },
        focus: DocumentRangeEndpoint::BlockEdge {
            block_id: second_id.clone(),
            side: Direction::Forward,
        },
    })?;
    assert_eq!(
        session.selected_block_environments(),
        &[first_id, second_id]
    );

    session.run(Command::Delete(Direction::Backward))?;

    // 块外侧边界端点保持对象删除语义；范围中间的普通段落整段移除而不是清空。
    assert_eq!(session.document().body.len(), 1);
    let Block::Paragraph(placeholder) = &session.document().body[0] else {
        return Err("删除全部块后应保留可编辑空段落".into());
    };
    assert!(TextMap::new(placeholder)?.text.is_empty());
    assert_ne!(placeholder.id, middle_id);
    assert!(!session.has_structure_selection());

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    session.run(Command::Redo)?;
    assert_eq!(session.document().body.len(), 1);
    assert!(matches!(session.document().body[0], Block::Paragraph(_)));
    Ok(())
}

#[test]
fn deleting_selected_block_object_keeps_preceding_empty_paragraph_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let preceding = paragraph();
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(preceding.clone()), equation];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    let original_selection = session.selection().clone();
    select_block_object(&mut session, equation_id.clone())?;
    let generation_before_delete = session.document_generation();

    session.run(Command::Delete(Direction::Backward))?;

    // 块对象删除只移除对象本身；范围外的空段落必须保留原 ID 和位置。
    assert_eq!(session.document().body.len(), 1);
    assert!(matches!(
        &session.document().body[0],
        Block::Paragraph(paragraph) if paragraph.id == preceding.id
    ));
    // caret 沿用对象删除规则：折叠到范围起点一侧段落的末尾边界。
    assert_eq!(
        session.selection().focus,
        body_position(&preceding, 0, Affinity::Upstream)
    );
    assert!(!session.has_structure_selection());
    assert!(session.selected_math_blocks().is_empty());
    assert_eq!(session.document_generation(), generation_before_delete + 1);

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    assert_eq!(session.selection(), &original_selection);
    session.run(Command::Redo)?;
    assert_eq!(session.document().body.len(), 1);
    assert!(matches!(
        &session.document().body[0],
        Block::Paragraph(paragraph) if paragraph.id == preceding.id
    ));
    Ok(())
}

#[test]
fn deleting_text_to_block_edge_range_keeps_preceding_empty_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let preceding = paragraph();
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(preceding.clone()), equation];
    let mut session = Session::new(document)?;
    session.install_structure_selection(StructureSelection {
        anchor: DocumentRangeEndpoint::Text(body_position(&preceding, 0, Affinity::Downstream)),
        focus: DocumentRangeEndpoint::BlockEdge {
            block_id: equation_id.clone(),
            side: Direction::Forward,
        },
    })?;
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );

    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(session.document().body.len(), 1);
    assert!(matches!(
        &session.document().body[0],
        Block::Paragraph(paragraph) if paragraph.id == preceding.id
    ));
    Ok(())
}

#[test]
fn deleting_block_edge_to_text_range_keeps_following_empty_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let following = paragraph();
    let mut document = Document::blank();
    document.body = vec![equation, Block::Paragraph(following.clone())];
    let mut session = Session::new(document)?;
    session.install_structure_selection(StructureSelection {
        anchor: DocumentRangeEndpoint::BlockEdge {
            block_id: equation_id.clone(),
            side: Direction::Backward,
        },
        focus: DocumentRangeEndpoint::Text(body_position(&following, 0, Affinity::Downstream)),
    })?;

    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(session.document().body.len(), 1);
    assert!(matches!(
        &session.document().body[0],
        Block::Paragraph(paragraph) if paragraph.id == following.id
    ));
    Ok(())
}

#[test]
fn shift_right_from_empty_paragraph_deletes_block_object_without_the_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let preceding = paragraph();
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(preceding.clone()), equation];
    let mut session = Session::new(document)?;
    let caret = body_position(&preceding, 0, Affinity::Downstream);
    session.run(Command::SetSelection(Some(Selection {
        anchor: caret.clone(),
        focus: caret,
    })))?;

    // 真实手势：空段落段首 Shift+Right 选中紧随其后的块对象。
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: true,
    }))?;
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    assert_eq!(
        session.structure_selection_anchor(),
        Some(&body_position(&preceding, 0, Affinity::Downstream))
    );

    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(session.document().body.len(), 1);
    assert!(matches!(
        &session.document().body[0],
        Block::Paragraph(paragraph) if paragraph.id == preceding.id
    ));
    Ok(())
}

#[test]
fn shift_left_from_empty_paragraph_deletes_block_object_without_the_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let following = paragraph();
    let mut document = Document::blank();
    document.body = vec![equation, Block::Paragraph(following.clone())];
    let mut session = Session::new(document)?;
    let caret = body_position(&following, 0, Affinity::Downstream);
    session.run(Command::SetSelection(Some(Selection {
        anchor: caret.clone(),
        focus: caret,
    })))?;

    // 真实手势：空段落段首 Shift+Left 选中前一个块对象。
    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: true,
    }))?;
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );

    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(session.document().body.len(), 1);
    assert!(matches!(
        &session.document().body[0],
        Block::Paragraph(paragraph) if paragraph.id == following.id
    ));
    Ok(())
}

#[test]
fn deleting_selected_custom_environment_keeps_preceding_empty_paragraph_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let preceding = paragraph();
    let custom = environment(
        BlockEnvironmentKind::Custom,
        vec![Block::Paragraph(paragraph_with_text("content"))],
    );
    let custom_id = block_id(&custom);
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(preceding.clone()), custom];
    let mut session = Session::new(document)?;
    let original = session.document().clone();

    session.select_block_environment(custom_id.clone())?;
    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(session.document().body.len(), 1);
    assert!(matches!(
        &session.document().body[0],
        Block::Paragraph(paragraph) if paragraph.id == preceding.id
    ));
    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    Ok(())
}

#[test]
fn deleting_selected_block_object_keeps_empty_paragraphs_on_both_sides()
-> Result<(), Box<dyn std::error::Error>> {
    let preceding = paragraph();
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let following = paragraph();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(preceding.clone()),
        equation,
        Block::Paragraph(following.clone()),
    ];
    let mut session = Session::new(document)?;
    select_block_object(&mut session, equation_id)?;

    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(session.document().body.len(), 2);
    assert!(matches!(
        &session.document().body[0],
        Block::Paragraph(paragraph) if paragraph.id == preceding.id
    ));
    assert!(matches!(
        &session.document().body[1],
        Block::Paragraph(paragraph) if paragraph.id == following.id
    ));
    Ok(())
}

#[test]
fn deleting_selected_math_block_object_keeps_neighbouring_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let after = paragraph_with_text("after");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Paragraph(after),
    ];
    let mut session = Session::new(document)?;
    select_block_object(&mut session, equation_id)?;
    assert!(session.selected_block_environments().is_empty());

    session.run(Command::Delete(Direction::Forward))?;

    assert_eq!(session.document().body.len(), 2);
    // 对象删除只移除公式块，落点为相邻段落末尾而不是切分文字。
    assert_eq!(
        session.selection().focus,
        body_position(&before, "before".len(), Affinity::Upstream)
    );
    Ok(())
}

#[test]
fn failed_math_range_deletion_keeps_document_generation_and_history()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let equation = equation_block();
    let mut after = paragraph_with_text("after");
    after
        .inlines
        .push(ParagraphContent::SoftBreak(caret_doc::SoftBreak {
            extra: BTreeMap::from([("futureMetadata".to_owned(), serde_json::Value::Bool(true))]),
        }));
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    select_text_endpoints(
        &mut session,
        body_position(&before, 0, Affinity::Downstream),
        body_position(&after, "after\n".len(), Affinity::Upstream),
    )?;
    let selected_document = session.document().clone();
    let selected_selection = session.selection().clone();
    let selected_generation = session.document_generation();
    let selected_math = session.selected_math_blocks().to_vec();

    assert_eq!(
        session.run(Command::Delete(Direction::Backward)),
        Err(EditError::UnsupportedOpaqueContent)
    );

    assert_eq!(session.document(), &selected_document);
    assert_eq!(session.selection(), &selected_selection);
    assert_eq!(session.document_generation(), selected_generation);
    assert_eq!(session.selected_math_blocks(), selected_math.as_slice());
    assert!(session.has_structure_selection());
    assert!(!session.can_undo());
    Ok(())
}

#[test]
fn typing_over_cross_host_structure_selection_replaces_range_in_one_step()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("ab");
    let equation = equation_block();
    let after = paragraph_with_text("cd");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    select_text_endpoints(
        &mut session,
        body_position(&before, 1, Affinity::Downstream),
        body_position(&after, 1, Affinity::Upstream),
    )?;

    let effect = session.run(Command::InsertText("Z".to_owned()))?;

    assert!(effect.document_changed);
    // 范围先按纯文本跨段删除合并为 "ad"，再从合并落点插入替换文字。
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "aZd"
    );
    assert!(!session.has_structure_selection());
    assert!(session.selected_math_blocks().is_empty());

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    assert!(!session.has_structure_selection());
    session.run(Command::Redo)?;
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "aZd"
    );
    Ok(())
}

#[test]
fn pasting_multiline_text_over_structure_selection_uses_shared_paste_path()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("ab");
    let equation = equation_block();
    let after = paragraph_with_text("cd");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    select_text_endpoints(
        &mut session,
        body_position(&before, 1, Affinity::Downstream),
        body_position(&after, 1, Affinity::Upstream),
    )?;

    session.run(Command::PasteText("1\n2".to_owned()))?;

    // 范围先合并为 "ad"，多行粘贴再在合并落点拆分为两段。
    assert_eq!(session.document().body.len(), 2);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "a1"
    );
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[1])),
        "2d"
    );
    assert!(!session.has_structure_selection());

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    Ok(())
}

#[test]
fn copied_structure_range_keeps_block_math_as_object_and_environment_text()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("ab");
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let after = paragraph_with_text("cd");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    select_text_endpoints(
        &mut session,
        body_position(&before, 1, Affinity::Downstream),
        body_position(&after, 1, Affinity::Upstream),
    )?;
    assert_eq!(session.selected_text()?, "b\n\u{fffc}\nc");

    select_block_object(&mut session, equation_id)?;
    assert_eq!(session.selected_text()?, "\u{fffc}");

    let mut section = match environment(
        BlockEnvironmentKind::Section,
        vec![Block::Paragraph(paragraph_with_text("body text"))],
    ) {
        Block::Environment(section) => section,
        _ => unreachable!(),
    };
    section.title = Some(paragraph_with_text("Section title"));
    let section_id = section.id.clone();
    let mut document = Document::blank();
    document.body = vec![Block::Environment(section)];
    let mut session = Session::new(document)?;
    session.select_block_environment(section_id)?;
    assert_eq!(session.selected_text()?, "Section title\nbody text");
    Ok(())
}

#[test]
fn escape_clears_structure_selection_while_block_stop_is_active()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(before.clone()), equation];
    let mut session = Session::new(document)?;
    select_block_object(&mut session, equation_id.clone())?;
    session.selection = Selection {
        anchor: body_position(&before, "before".len(), Affinity::Upstream),
        focus: body_position(&before, "before".len(), Affinity::Upstream),
    };
    session.host_caret_position = Some(HostCaretPosition::outside(equation_id, Direction::Forward));

    let effect = session.run(Command::EditorKey(EditorKey::Escape))?;

    assert!(effect.selection_changed);
    assert!(!session.has_structure_selection());
    assert!(session.selected_block_environments().is_empty());
    assert!(session.selected_math_blocks().is_empty());
    assert!(session.block_boundary_exit().is_some());
    Ok(())
}

#[test]
fn deleting_document_start_math_block_falls_back_to_the_following_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let following = paragraph_with_text("after");
    let mut document = Document::blank();
    document.body = vec![equation, Block::Paragraph(following.clone())];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    select_block_object(&mut session, equation_id)?;

    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(session.document().body.len(), 1);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "after"
    );
    // 文档开头没有前置段落，落点退到范围终点一侧的段落起点。
    assert_eq!(
        session.selection().focus,
        body_position(&following, 0, Affinity::Downstream)
    );

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    session.run(Command::Redo)?;
    assert_eq!(session.document().body.len(), 1);
    Ok(())
}

#[test]
fn typing_over_selected_math_block_inserts_at_the_neighbour_and_keeps_one_undo_step()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(before.clone()), equation];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    select_block_object(&mut session, equation_id)?;

    session.run(Command::InsertText("Z".to_owned()))?;

    assert_eq!(session.document().body.len(), 1);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "beforeZ"
    );
    assert!(!session.has_structure_selection());
    assert!(session.selected_math_blocks().is_empty());

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    Ok(())
}

#[test]
fn backspace_after_shift_extension_consumes_the_range_before_the_math_adjacency_special_case()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("a");
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let following = paragraph();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Paragraph(following.clone()),
    ];
    let mut session = Session::new(document)?;
    let caret = body_position(&before, "a".len(), Affinity::Upstream);
    session.run(Command::SetSelection(Some(Selection {
        anchor: caret.clone(),
        focus: caret,
    })))?;

    // 第一键：整块覆盖公式；第二键：焦点移到公式后的空段落起点。
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: true,
    }))?;
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: true,
    }))?;
    assert_eq!(
        session.selection().focus,
        body_position(&following, 0, Affinity::Downstream)
    );
    let selection_before_delete = session.selection().clone();
    let generation_before_delete = session.document_generation();

    session.run(Command::Delete(Direction::Backward))?;

    // 范围事务而不是公式邻接特例：两端文本段落合并，公式与空终点段落一并移除。
    assert_eq!(session.document().body.len(), 1);
    assert!(matches!(session.document().body[0], Block::Paragraph(_)));
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "a"
    );
    assert!(session.math_edit().is_none());
    assert!(!session.has_structure_selection());
    assert!(session.selected_math_blocks().is_empty());
    assert!(session.selected_block_environments().is_empty());
    assert_eq!(session.document_generation(), generation_before_delete + 1);

    session.run(Command::Undo)?;
    assert_eq!(session.document().body.len(), 3);
    assert!(matches!(
        session.document().body[1],
        Block::Equation(ref equation) if equation.id == equation_id
    ));
    assert_eq!(session.selection(), &selection_before_delete);
    Ok(())
}

#[test]
fn empty_insertion_over_structure_selection_is_a_plain_noop()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("ab");
    let equation = equation_block();
    let after = paragraph_with_text("cd");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    let generation = session.document_generation();
    select_text_endpoints(
        &mut session,
        body_position(&before, 1, Affinity::Downstream),
        body_position(&after, 1, Affinity::Upstream),
    )?;
    let selected_math = session.selected_math_blocks().to_vec();

    let effect = session.run(Command::InsertText(String::new()))?;

    assert!(!effect.document_changed);
    assert_eq!(session.document(), &original);
    assert_eq!(session.document_generation(), generation);
    assert!(session.has_structure_selection());
    assert_eq!(session.selected_math_blocks(), selected_math.as_slice());
    assert!(session.selected_text()?.contains('\u{fffc}'));

    let pasted = session.run(Command::PasteText(String::new()))?;
    assert!(!pasted.document_changed);
    assert_eq!(session.document(), &original);
    assert!(session.has_structure_selection());
    Ok(())
}

#[test]
fn deleting_selected_equation_between_two_math_hosts_keeps_the_neighbour_and_restores_nearest_caret()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("p0");
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let align = align_block();
    let align_id = block_id(&align);
    let after = paragraph_with_text("p1");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        align.clone(),
        Block::Paragraph(after),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    // 相邻公式块的持久化内容必须逐字节保留。
    let align_before = serde_json::to_value(&align)?;
    select_block_object(&mut session, equation_id)?;
    let selection_before_delete = session.selection().clone();
    let generation_before_delete = session.document_generation();

    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(session.document().body.len(), 3);
    assert_eq!(
        serde_json::to_value(&session.document().body[1])?,
        align_before
    );
    assert_eq!(block_id(&session.document().body[1]), align_id);
    assert_eq!(
        session.selection().focus,
        body_position(&before, "p0".len(), Affinity::Upstream)
    );
    assert_eq!(session.document_generation(), generation_before_delete + 1);
    let deleted_document = session.document().clone();
    let selection_after_delete = session.selection().clone();

    // 只有一个历史条目：一次撤销即可完整恢复，且不再有更早的步骤。
    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    assert_eq!(session.selection(), &selection_before_delete);
    assert!(!session.can_undo());
    assert!(session.can_redo());

    session.run(Command::Redo)?;
    assert_eq!(session.document(), &deleted_document);
    assert_eq!(session.selection(), &selection_after_delete);
    Ok(())
}

#[test]
fn deleting_range_covering_adjacent_equation_and_align_removes_both_in_one_transaction()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("p0");
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let align = align_block();
    let align_id = block_id(&align);
    let after = paragraph_with_text("p1");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        align,
        Block::Paragraph(after),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    let caret = body_position(&before, "p0".len(), Affinity::Upstream);
    session.run(Command::SetSelection(Some(Selection {
        anchor: caret.clone(),
        focus: caret,
    })))?;

    // 两键扩选：第一键覆盖 Equation，第二键把相邻 Align 一并纳入同一范围。
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: true,
    }))?;
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: true,
    }))?;
    assert_eq!(session.selected_math_blocks(), &[equation_id, align_id]);
    let generation_before_delete = session.document_generation();

    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(session.document().body.len(), 2);
    assert!(
        session
            .document()
            .body
            .iter()
            .all(|block| matches!(block, Block::Paragraph(_)))
    );
    assert_eq!(session.document_generation(), generation_before_delete + 1);
    let deleted_document = session.document().clone();

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    assert!(!session.can_undo());
    session.run(Command::Redo)?;
    assert_eq!(session.document(), &deleted_document);
    Ok(())
}

#[test]
fn failed_replace_over_structure_selection_rolls_back_every_interaction_field()
-> Result<(), Box<dyn std::error::Error>> {
    // 标题段落的省略多行粘贴会在删除计划已应用后失败（UnsupportedNewline），
    // 用于验证替换事务的完整回滚。
    let title = paragraph_with_text("title");
    let title_id = title.id.clone();
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let section_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: section_id,
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(title),
        children: vec![equation],
        extra: BTreeMap::new(),
    })];
    let mut session = Session::new(document)?;
    // 让待用行内样式处于活动状态，回滚后必须原样保留而不是被静默清空。
    session.run(Command::ToggleInlineFormat(
        caret_doc::InlineEnvironmentKind::Bold,
    ))?;
    assert!(session.has_pending_inline_style());
    select_block_object(&mut session, equation_id.clone())?;
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    assert!(session.selected_block_environments().is_empty());
    let document_before = session.document().clone();
    let selection_before = session.selection().clone();
    let anchor_affinity_before = session.selection_anchor_visual_affinity();
    let visual_affinity_before = session.selection_visual_affinity();
    let structure_index_before = session.structure_index().clone();
    let story_before = session.story_for_paragraph(&title_id);
    let ordinal_before = session.structure_index().paragraph_ordinal(&title_id);
    let host_caret_before = session.block_boundary_exit();
    let generation_before = session.document_generation();
    let state_id_before = session.history_state_id();
    let pending_style_before = session.has_pending_inline_style();
    let can_undo_before = session.can_undo();
    let can_redo_before = session.can_redo();
    assert_eq!(session.selected_text()?, "\u{fffc}");

    assert_eq!(
        session.run(Command::PasteText("a\nb".to_owned())),
        Err(EditError::UnsupportedNewline)
    );

    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(
        session.selection_anchor_visual_affinity(),
        anchor_affinity_before
    );
    assert_eq!(session.selection_visual_affinity(), visual_affinity_before);
    assert_eq!(session.structure_index(), &structure_index_before);
    assert_eq!(session.story_for_paragraph(&title_id), story_before);
    assert_eq!(
        session.structure_index().paragraph_ordinal(&title_id),
        ordinal_before
    );
    assert_eq!(session.block_boundary_exit(), host_caret_before);
    // 历史记账与文档代际同样不得被失败的替换推进。
    assert_eq!(session.history_state_id(), state_id_before);
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.has_pending_inline_style(), pending_style_before);
    assert!(session.has_structure_selection());
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    assert!(session.selected_block_environments().is_empty());
    assert_eq!(session.selected_text()?, "\u{fffc}");
    assert!(session.math_edit().is_none());
    assert_eq!(session.document_generation(), generation_before);
    // 历史可用性保持失败前的状态（此前设置待用样式已经产生一个历史条目）。
    assert_eq!(session.can_undo(), can_undo_before);
    assert_eq!(session.can_redo(), can_redo_before);
    assert!(session.has_pending_inline_style());

    // 回滚后同一结构范围仍可被后续事务正常消费。
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(session.document().body.len(), 1);
    let Block::Environment(section) = &session.document().body[0] else {
        return Err("回滚后删除应保留 Section 环境".into());
    };
    assert!(section.children.is_empty());
    Ok(())
}

#[test]
fn block_edge_range_removes_paragraph_between_two_math_blocks()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("p0");
    let first = equation_block();
    let first_id = block_id(&first);
    let middle = paragraph_with_text("middle");
    let middle_id = middle.id.clone();
    let second = equation_block();
    let second_id = block_id(&second);
    let after = paragraph_with_text("p3");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        first,
        Block::Paragraph(middle),
        second,
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    session.install_structure_selection(StructureSelection {
        anchor: DocumentRangeEndpoint::BlockEdge {
            block_id: first_id.clone(),
            side: Direction::Backward,
        },
        focus: DocumentRangeEndpoint::BlockEdge {
            block_id: second_id.clone(),
            side: Direction::Forward,
        },
    })?;

    session.run(Command::Delete(Direction::Backward))?;

    // 双块外侧边界范围没有文本合并目标：被完全覆盖的中间段落整段移除，
    // 而不是清空成空段落占位。
    assert_eq!(session.document().body.len(), 2);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "p0"
    );
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[1])),
        "p3"
    );
    assert!(!session.document().body.iter().any(|block| {
        matches!(block, Block::Paragraph(paragraph) if paragraph.id == middle_id)
    }));
    assert!(!session.document().body.iter().any(|block| {
        matches!(block, Block::Equation(equation)
            if equation.id == first_id || equation.id == second_id)
    }));

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    session.run(Command::Redo)?;
    assert_eq!(session.document().body.len(), 2);
    Ok(())
}

#[test]
fn block_edge_start_removes_paragraphs_before_the_text_focus()
-> Result<(), Box<dyn std::error::Error>> {
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let first = paragraph_with_text("one");
    let first_id = first.id.clone();
    let second = paragraph_with_text("two");
    let second_id = second.id.clone();
    let mut document = Document::blank();
    document.body = vec![
        equation,
        Block::Paragraph(first),
        Block::Paragraph(second.clone()),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    session.install_structure_selection(StructureSelection {
        anchor: DocumentRangeEndpoint::BlockEdge {
            block_id: equation_id.clone(),
            side: Direction::Backward,
        },
        focus: DocumentRangeEndpoint::Text(body_position(&second, 1, Affinity::Downstream)),
    })?;

    session.run(Command::Delete(Direction::Backward))?;

    // 起点是块外侧边界时没有起点段落可保留：公式与焦点段之前的整段段落移除，
    // 焦点段只保留切口之后的文字。
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "wo"
    );
    assert!(
        !session.document().body.iter().any(|block| {
            matches!(block, Block::Paragraph(paragraph) if paragraph.id == first_id)
        })
    );
    assert!(matches!(
        session.document().body.as_slice(),
        [Block::Paragraph(paragraph)] if paragraph.id == second_id
    ));

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    Ok(())
}

#[test]
fn block_edge_end_removes_paragraphs_after_the_text_start() -> Result<(), Box<dyn std::error::Error>>
{
    let first = paragraph_with_text("one");
    let second = paragraph_with_text("two");
    let second_id = second.id.clone();
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(first.clone()),
        Block::Paragraph(second),
        equation,
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    session.install_structure_selection(StructureSelection {
        anchor: DocumentRangeEndpoint::Text(body_position(&first, 1, Affinity::Downstream)),
        focus: DocumentRangeEndpoint::BlockEdge {
            block_id: equation_id.clone(),
            side: Direction::Forward,
        },
    })?;

    session.run(Command::Delete(Direction::Backward))?;

    // 终点是块外侧边界时没有终点段落可合并：起点段保留前缀，中间整段段落与
    // 公式一并移除。
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "o"
    );
    assert!(!session.document().body.iter().any(|block| {
        matches!(block, Block::Paragraph(paragraph) if paragraph.id == second_id)
    }));

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    Ok(())
}

#[test]
fn deleting_equation_range_inside_an_environment_merges_child_paragraphs()
-> Result<(), Box<dyn std::error::Error>> {
    let first = paragraph_with_text("aa");
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let second = paragraph_with_text("bb");
    let environment = match environment(
        BlockEnvironmentKind::Custom,
        vec![
            Block::Paragraph(first.clone()),
            equation,
            Block::Paragraph(second.clone()),
        ],
    ) {
        Block::Environment(environment) => environment,
        _ => unreachable!(),
    };
    let environment_id = environment.id.clone();
    let mut document = Document::blank();
    document.body = vec![Block::Environment(environment)];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    select_text_endpoints(
        &mut session,
        body_position(&first, "aa".len(), Affinity::Downstream),
        body_position(&second, 0, Affinity::Downstream),
    )?;
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );

    session.run(Command::Delete(Direction::Backward))?;

    // 同一环境容器内的两端段落按跨段删除合并，环境本身保留。
    let Block::Environment(environment) = &session.document().body[0] else {
        return Err("删除后应保留 Custom 环境".into());
    };
    assert_eq!(environment.id, environment_id);
    assert_eq!(environment.children.len(), 1);
    assert_eq!(
        paragraph_text(match &environment.children[0] {
            Block::Paragraph(paragraph) => paragraph,
            _ => return Err("环境内应只剩合并段落".into()),
        }),
        "aabb"
    );

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    Ok(())
}

#[test]
fn unmergeable_text_range_falls_back_to_object_deletion_and_keeps_break()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("ab");
    let after = paragraph_with_text("cd");
    let equation = equation_block();
    let equation_id = block_id(&equation);
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        Block::ExplicitBreak(caret_doc::ExplicitBreak::default()),
        equation,
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    select_text_endpoints(
        &mut session,
        body_position(&before, 1, Affinity::Downstream),
        body_position(&after, 1, Affinity::Upstream),
    )?;
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );

    session.run(Command::Delete(Direction::Backward))?;

    // 中间夹有不可合并的 ExplicitBreak：退回对象删除语义，两端段落不合并。
    assert_eq!(session.document().body.len(), 3);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "a"
    );
    assert!(matches!(
        session.document().body[1],
        Block::ExplicitBreak(_)
    ));
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[2])),
        "d"
    );

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    Ok(())
}

#[test]
fn commit_composition_replacing_structure_range_is_one_undoable_edit()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("ab");
    let equation = equation_block();
    let after = paragraph_with_text("cd");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before.clone()),
        equation,
        Block::Paragraph(after.clone()),
    ];
    let mut session = Session::new(document)?;
    let original = session.document().clone();
    let original_selection = session.selection().clone();
    select_text_endpoints(
        &mut session,
        body_position(&before, 1, Affinity::Downstream),
        body_position(&after, 1, Affinity::Upstream),
    )?;
    let generation_before = session.document_generation();
    session.run(Command::UpdateComposition {
        id: 9,
        preedit: "中".to_owned(),
    })?;

    let committed = session.run(Command::CommitComposition {
        id: 9,
        text: "中".to_owned(),
    })?;

    assert!(committed.document_changed);
    assert_eq!(session.document_generation(), generation_before + 1);
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(
        paragraph_text(paragraph_at_block(&session.document().body[0])),
        "a中d"
    );
    assert!(!session.has_structure_selection());

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &original);
    assert_eq!(session.selection(), &original_selection);
    Ok(())
}
