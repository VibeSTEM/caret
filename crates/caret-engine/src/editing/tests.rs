//! 覆盖文本编辑、段落操作、位置映射和 IME 行为。

use super::text_map::TextMap;
use super::*;
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, InlineEnvironment,
    InlineEnvironmentKind, NodeId, Orientation, PageGeometry, PageMargins, PaperSize, Paragraph,
    ParagraphContent, Run, TextFlow,
};
use caret_layout::VisualAffinity;

fn run(text: &str) -> ParagraphContent {
    text_flow(Run {
        text: text.to_owned(),
        language: None,
        props: Default::default(),
        extra: Default::default(),
    })
}

fn session_with(inlines: Vec<ParagraphContent>) -> Session {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        unreachable!();
    };
    paragraph.inlines = inlines;
    match Session::new(document) {
        Ok(session) => session,
        Err(error) => panic!("invalid test document: {error}"),
    }
}

fn paragraph_text(paragraph: &Paragraph) -> String {
    match TextMap::new(paragraph) {
        Ok(map) => map.text,
        Err(error) => panic!("failed to map paragraph text: {error}"),
    }
}

fn text_flow(run: Run) -> ParagraphContent {
    ParagraphContent::Text(TextFlow {
        runs: vec![run],
        source_range: None,
        extra: Default::default(),
    })
}

#[test]
fn grapheme_delete_crosses_run_and_environment_boundaries() {
    let mut session = session_with(vec![
        run("e"),
        ParagraphContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            source_range: None,
            props: Default::default(),
            children: vec![run("\u{301}")],
            extra: Default::default(),
        }),
    ]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let end = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 3,
        affinity: Affinity::Upstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: end.clone(),
                focus: end,
            })))
            .is_ok()
    );
    assert!(session.run(Command::Delete(Direction::Backward)).is_ok());
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        unreachable!();
    };
    let map = match TextMap::new(paragraph) {
        Ok(map) => map,
        Err(error) => panic!("failed to map paragraph: {error}"),
    };
    assert_eq!(map.text, "");
    assert_eq!(paragraph.inlines.len(), 2);
}

#[test]
fn text_flow_extension_fields_allow_mapping_but_block_lossy_edits() {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    let ParagraphContent::Text(mut flow) = run("abc") else {
        unreachable!();
    };
    flow.extra.insert(
        "futureTextFlowField".to_owned(),
        serde_json::json!({"version": 2}),
    );
    paragraph.inlines = vec![ParagraphContent::Text(flow)];
    let paragraph_id = paragraph.id.clone();
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("extension fields are valid document data: {error}"));

    session
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| panic!("position mapping should ignore extension data: {error}"));
    assert_eq!(session.selection().focus.offset, 1);

    let before_document = session.document().clone();
    let before_selection = session.selection().clone();
    assert_eq!(
        session.run(Command::InsertText("x".to_owned())),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.selection(), &before_selection);
    assert_eq!(session.document_generation(), 0);

    let Block::Paragraph(restored) = &session.document().body[0] else {
        panic!("document should retain the paragraph");
    };
    assert_eq!(
        TextMap::new(restored)
            .unwrap_or_else(|error| panic!("mapping remains available: {error}"))
            .text,
        "abc"
    );
    assert_eq!(restored.id, paragraph_id);
}

#[test]
fn invalid_mid_grapheme_position_is_rejected_without_mutation() {
    let mut session = session_with(vec![run("e\u{301}")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let pos = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 1,
        affinity: Affinity::Downstream,
    };
    let before = session.document.clone();
    assert_eq!(
        session.run(Command::SetSelection(Some(Selection {
            anchor: pos.clone(),
            focus: pos,
        }))),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.document, before);
}

#[test]
fn composition_update_is_transient_and_commit_is_idempotent() {
    let mut session = session_with(Vec::new());
    assert!(
        session
            .run(Command::UpdateComposition {
                id: 7,
                preedit: "に".to_owned(),
            })
            .is_ok()
    );
    let before_commit = session.document.clone();
    assert_eq!(
        session.composition().map(|value| value.preedit.as_str()),
        Some("に")
    );
    assert_eq!(session.document, before_commit);
    assert!(
        session
            .run(Command::CommitComposition {
                id: 7,
                text: "に".to_owned(),
            })
            .is_ok()
    );
    assert!(
        session
            .run(Command::CommitComposition {
                id: 7,
                text: "に".to_owned(),
            })
            .is_ok()
    );
    let paragraph = match session.paragraph_for(&session.selection.focus) {
        Ok(paragraph) => paragraph,
        Err(error) => panic!("missing paragraph after IME commit: {error}"),
    };
    let map = match TextMap::new(paragraph) {
        Ok(map) => map,
        Err(error) => panic!("failed to map committed text: {error}"),
    };
    assert_eq!(map.text, "に");
}

#[test]
fn ime_replacement_maps_utf16_and_snaps_to_grapheme_boundaries() {
    let mut session = session_with(vec![run("A😀B")]);
    let end = Pos {
        story: StoryId::Body,
        paragraph_id: session.selection.focus.paragraph_id.clone(),
        offset: "A😀B".len(),
        affinity: Affinity::Downstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: end.clone(),
                focus: end,
            })))
            .is_ok()
    );
    assert!(session.set_composition_replacement_range(-3, 2).is_ok());
    assert_eq!(session.selection.anchor.offset, 1);
    assert_eq!(session.selection.focus.offset, 5);

    let before = session.selection.clone();
    assert!(session.set_composition_replacement_range(-2, 1).is_err());
    assert_eq!(session.selection, before);
}

#[test]
fn multiline_paste_replaces_selection_and_splits_paragraphs_atomically() {
    let mut session = session_with(vec![run("before after")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let anchor = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 7,
        affinity: Affinity::Downstream,
    };
    let focus = Pos {
        offset: 12,
        ..anchor.clone()
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection { anchor, focus })))
            .is_ok()
    );

    let effect = session.run(Command::PasteText("one\r\ntwo\n".to_owned()));
    assert!(effect.is_ok());
    assert!(effect.is_ok_and(|effect| effect.document_changed && effect.position_map.is_some()));
    let paragraphs = session
        .document()
        .body
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph_text(paragraph)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(paragraphs, ["before one", "two", ""]);
    assert_eq!(session.selection.focus.offset, 0);
    assert_ne!(session.selection.focus.paragraph_id, paragraph_id);
    assert_eq!(session.document_generation(), 1);
}

#[test]
fn multiline_paste_replaces_a_reversed_cross_paragraph_selection() -> Result<(), EditError> {
    let mut document = Document::blank();
    let paragraphs = ["alpha", "beta", "gamma"]
        .into_iter()
        .map(|text| {
            let mut paragraph = Paragraph::empty(NodeId::generate());
            paragraph.inlines = vec![run(text)];
            paragraph
        })
        .collect::<Vec<_>>();
    let first_id = paragraphs[0].id.clone();
    let last_id = paragraphs[2].id.clone();
    document.body = paragraphs.into_iter().map(Block::Paragraph).collect();
    let mut session = Session::new(document)?;
    session.run(Command::SetSelection(Some(Selection {
        anchor: Pos {
            story: StoryId::Body,
            paragraph_id: last_id,
            offset: 2,
            affinity: Affinity::Upstream,
        },
        focus: Pos {
            story: StoryId::Body,
            paragraph_id: first_id.clone(),
            offset: 2,
            affinity: Affinity::Downstream,
        },
    })))?;
    assert_eq!(session.selected_text()?, "pha\nbeta\nga");

    let effect = session.run(Command::PasteText("中\r\n文\rlast".to_owned()))?;
    assert!(effect.document_changed);
    assert!(effect.position_map.is_some());
    let paragraphs = session
        .document()
        .body
        .iter()
        .map(|block| match block {
            Block::Paragraph(paragraph) => paragraph_text(paragraph),
            _ => panic!("paste should leave only paragraphs"),
        })
        .collect::<Vec<_>>();
    assert_eq!(paragraphs, ["al中", "文", "lastmma"]);
    assert_eq!(session.document_generation(), 1);
    assert_eq!(session.selection().focus.offset, "last".len());
    assert_eq!(session.selected_text()?, "");
    Ok(())
}

#[test]
fn multiline_paste_rejects_unsafe_run_split_without_mutation() {
    let mut session = session_with(vec![text_flow(Run {
        text: "ab".to_owned(),
        language: None,
        props: Default::default(),
        extra: std::collections::BTreeMap::from([("opaque".to_owned(), serde_json::json!(true))]),
    })]);
    let position = Pos {
        story: StoryId::Body,
        paragraph_id: session.selection.focus.paragraph_id.clone(),
        offset: 1,
        affinity: Affinity::Downstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: position.clone(),
                focus: position,
            })))
            .is_ok()
    );
    let document = session.document().clone();
    let selection = session.selection.clone();

    assert_eq!(
        session.run(Command::PasteText("x\ny".to_owned())),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &document);
    assert_eq!(session.selection, selection);
    assert_eq!(session.document_generation(), 0);
}

#[test]
fn page_geometry_command_is_atomic_and_idempotent() {
    let mut session = session_with(Vec::new());
    let target = PageGeometry {
        paper_size: PaperSize::A3,
        orientation: Orientation::Landscape,
        margins: PageMargins {
            top: 36.0,
            right: 54.0,
            bottom: 72.0,
            left: 90.0,
        },
    };
    let effect = session.run(Command::SetPageGeometry(target));
    assert!(effect.is_ok());
    assert!(effect.is_ok_and(|effect| effect.document_changed));
    assert_eq!(session.document.page_sections[0].geometry(), Ok(target));

    let unchanged = session.run(Command::SetPageGeometry(target));
    assert!(unchanged.is_ok_and(|effect| !effect.document_changed));

    let before = session.document.clone();
    let invalid = PageGeometry {
        margins: PageMargins {
            left: 500.0,
            right: 200.0,
            ..PageMargins::default()
        },
        ..PageGeometry::default()
    };
    assert!(matches!(
        session.run(Command::SetPageGeometry(invalid)),
        Err(EditError::InvalidPageGeometry(_))
    ));
    assert_eq!(session.document, before);
}

#[test]
fn paragraph_split_then_backspace_merges_compatible_siblings() {
    let mut session = session_with(vec![run("ab")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let split = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 1,
        affinity: Affinity::Downstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: split.clone(),
                focus: split,
            })))
            .is_ok()
    );
    assert!(session.run(Command::InsertParagraphBreak).is_ok());
    assert_eq!(session.document.body.len(), 2);
    assert!(session.run(Command::Delete(Direction::Backward)).is_ok());
    assert_eq!(session.document.body.len(), 1);
    let paragraph = match &session.document.body[0] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("merged block should be a paragraph"),
    };
    let map = match TextMap::new(paragraph) {
        Ok(map) => map,
        Err(error) => panic!("failed to map merged paragraph: {error}"),
    };
    assert_eq!(map.text, "ab");
}

#[test]
fn section_title_enter_moves_suffix_to_new_first_body_paragraph() {
    let mut document = Document::blank();
    let title_id = NodeId::generate();
    let environment_id = NodeId::generate();
    let existing = Paragraph {
        id: NodeId::generate(),
        language: None,
        layout: None,
        inlines: vec![run("body")],
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    document.body = vec![Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: Default::default(),
        title: Some(Paragraph {
            id: title_id.clone(),
            language: None,
            layout: None,
            inlines: vec![run("ab")],
            mark_style: Default::default(),
            props: Default::default(),
            extra: Default::default(),
        }),
        children: vec![Block::Paragraph(existing)],
        extra: Default::default(),
    })];
    let mut session = match Session::new(document) {
        Ok(session) => session,
        Err(error) => panic!("invalid section document: {error}"),
    };
    let caret = Pos {
        story: StoryId::EnvironmentTitle(environment_id.clone()),
        paragraph_id: title_id.clone(),
        offset: 1,
        affinity: Affinity::Downstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: caret.clone(),
                focus: caret,
            })))
            .is_ok()
    );
    assert!(session.run(Command::InsertParagraphBreak).is_ok());
    let Block::Environment(environment) = &session.document.body[0] else {
        panic!("section should remain in place");
    };
    let title = match &environment.title {
        Some(title) => title,
        None => panic!("section title should remain present"),
    };
    let title_text = match TextMap::new(title) {
        Ok(map) => map.text,
        Err(error) => panic!("failed to map title: {error}"),
    };
    assert_eq!(title_text, "a");
    assert_eq!(environment.children.len(), 2);
    let Block::Paragraph(suffix) = &environment.children[0] else {
        panic!("title suffix should become first body paragraph");
    };
    let suffix_text = match TextMap::new(suffix) {
        Ok(map) => map.text,
        Err(error) => panic!("failed to map suffix: {error}"),
    };
    assert_eq!(suffix_text, "b");
}

#[test]
fn section_title_enter_at_end_keeps_nested_body_order_and_creates_empty_paragraph() {
    let mut document = Document::blank();
    let environment_id = NodeId::generate();
    let title_id = NodeId::generate();
    let nested_id = NodeId::generate();
    let nested_paragraph = Paragraph {
        id: NodeId::generate(),
        language: None,
        layout: None,
        inlines: vec![run("nested body")],
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    document.body = vec![Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: Default::default(),
        title: Some(Paragraph {
            id: title_id.clone(),
            language: None,
            layout: None,
            inlines: vec![run("end")],
            mark_style: Default::default(),
            props: Default::default(),
            extra: Default::default(),
        }),
        children: vec![Block::Environment(BlockEnvironment {
            id: nested_id.clone(),
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: Default::default(),
            title: None,
            children: vec![Block::Paragraph(nested_paragraph)],
            extra: Default::default(),
        })],
        extra: Default::default(),
    })];
    let mut session = match Session::new(document) {
        Ok(session) => session,
        Err(error) => panic!("invalid section document: {error}"),
    };
    let caret = Pos {
        story: StoryId::EnvironmentTitle(environment_id.clone()),
        paragraph_id: title_id,
        offset: "end".len(),
        affinity: Affinity::Downstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: caret.clone(),
                focus: caret,
            })))
            .is_ok()
    );
    assert!(session.run(Command::InsertParagraphBreak).is_ok());

    let Block::Environment(section) = &session.document.body[0] else {
        panic!("section should remain in place");
    };
    assert_eq!(section.children.len(), 2);
    let Block::Paragraph(suffix) = &section.children[0] else {
        panic!("an empty suffix paragraph should be inserted first");
    };
    assert!(suffix.inlines.is_empty());
    let Block::Environment(nested) = &section.children[1] else {
        panic!("existing nested content should retain its order");
    };
    assert_eq!(nested.id, nested_id);
}

#[test]
fn selection_rejects_cross_title_body_range_before_installation() {
    let mut document = Document::blank();
    let title_id = NodeId::generate();
    let environment_id = NodeId::generate();
    let body_id = NodeId::generate();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: Default::default(),
        title: Some(Paragraph {
            id: title_id.clone(),
            language: None,
            layout: None,
            inlines: vec![run("title")],
            mark_style: Default::default(),
            props: Default::default(),
            extra: Default::default(),
        }),
        children: vec![Block::Paragraph(Paragraph {
            id: body_id.clone(),
            language: None,
            layout: None,
            inlines: vec![run("body")],
            mark_style: Default::default(),
            props: Default::default(),
            extra: Default::default(),
        })],
        extra: Default::default(),
    })];
    let mut session = match Session::new(document) {
        Ok(session) => session,
        Err(error) => panic!("invalid section document: {error}"),
    };
    let selection = Selection {
        anchor: Pos {
            story: StoryId::EnvironmentTitle(environment_id),
            paragraph_id: title_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        },
        focus: Pos {
            story: StoryId::Body,
            paragraph_id: body_id,
            offset: 4,
            affinity: Affinity::Upstream,
        },
    };
    let before = session.document.clone();
    let before_selection = session.selection.clone();
    assert_eq!(
        session.run(Command::SetSelection(Some(selection))),
        Err(EditError::CrossParagraphSelection)
    );
    assert_eq!(session.document, before);
    assert_eq!(session.selection, before_selection);
    assert_eq!(session.document_generation(), 0);
}

#[test]
fn cross_environment_selection_rejects_delete_and_replacement_atomically() -> Result<(), EditError>
{
    let paragraph = |text: &str| Paragraph {
        id: NodeId::generate(),
        language: None,
        layout: None,
        inlines: vec![run(text)],
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let custom_environment = |paragraph: Paragraph| {
        Block::Environment(BlockEnvironment {
            id: NodeId::generate(),
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: Default::default(),
            title: None,
            children: vec![Block::Paragraph(paragraph)],
            extra: Default::default(),
        })
    };
    let first = paragraph("first");
    let second = paragraph("second");
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    let mut document = Document::blank();
    document.body = vec![custom_environment(first), custom_environment(second)];
    let mut session = Session::new(document)?;
    session.run(Command::SetSelection(Some(Selection {
        anchor: Pos {
            story: StoryId::Body,
            paragraph_id: first_id,
            offset: 1,
            affinity: Affinity::Downstream,
        },
        focus: Pos {
            story: StoryId::Body,
            paragraph_id: second_id,
            offset: 2,
            affinity: Affinity::Upstream,
        },
    })))?;
    let before_document = session.document().clone();
    let before_selection = session.selection().clone();

    for command in [
        Command::Delete(Direction::Backward),
        Command::InsertText("replacement".to_owned()),
    ] {
        assert_eq!(
            session.run(command),
            Err(EditError::CrossParagraphSelection)
        );
        assert_eq!(session.document(), &before_document);
        assert_eq!(session.selection(), &before_selection);
        assert_eq!(session.document_generation(), 0);
    }
    Ok(())
}

#[test]
fn paragraph_range_merge_rejects_unknown_fields_without_mutation() -> Result<(), EditError> {
    let mut document = Document::blank();
    let Block::Paragraph(first) = &mut document.body[0] else {
        unreachable!();
    };
    first.inlines = vec![run("left")];
    first
        .extra
        .insert("futureParagraphField".to_owned(), serde_json::json!(true));
    let first_id = first.id.clone();
    let mut second = Paragraph::empty(NodeId::generate());
    second.inlines = vec![run("right")];
    let second_id = second.id.clone();
    document.body.push(Block::Paragraph(second));
    let mut session = Session::new(document)?;
    session.run(Command::SetSelection(Some(Selection {
        anchor: Pos {
            story: StoryId::Body,
            paragraph_id: first_id,
            offset: 2,
            affinity: Affinity::Downstream,
        },
        focus: Pos {
            story: StoryId::Body,
            paragraph_id: second_id,
            offset: 2,
            affinity: Affinity::Upstream,
        },
    })))?;
    let before_document = session.document().clone();
    let before_selection = session.selection().clone();
    assert_eq!(
        session.run(Command::Delete(Direction::Backward)),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.selection(), &before_selection);
    assert_eq!(session.document_generation(), 0);
    Ok(())
}

#[test]
fn environment_end_affinity_has_a_visible_transition() {
    let mut session = session_with(vec![
        run("A"),
        ParagraphContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            source_range: None,
            props: Default::default(),
            children: vec![run("B")],
            extra: Default::default(),
        }),
        run("C"),
    ]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let inside_end = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 2,
        affinity: Affinity::Upstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: inside_end.clone(),
                focus: inside_end,
            })))
            .is_ok()
    );
    assert!(session.run(Command::MoveCaret(Direction::Forward)).is_ok());
    assert_eq!(session.selection.focus.offset, 2);
    assert_eq!(session.selection.focus.affinity, Affinity::Downstream);

    let boundary = Pos {
        offset: 2,
        affinity: Affinity::Upstream,
        ..session.selection.focus.clone()
    };
    assert!(
        session
            .set_navigation_selection(
                Selection {
                    anchor: boundary.clone(),
                    focus: boundary,
                },
                caret_layout::VisualAffinity::Downstream,
            )
            .is_ok()
    );
    assert_eq!(session.selection.anchor, session.selection.focus);
    assert_eq!(session.selection.focus.affinity, Affinity::Downstream);

    let next = Pos {
        offset: 3,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    assert!(
        session
            .set_navigation_selection(
                Selection {
                    anchor: next.clone(),
                    focus: next,
                },
                caret_layout::VisualAffinity::Downstream,
            )
            .is_ok()
    );
    assert_eq!(session.selection.anchor, session.selection.focus);
    assert_eq!(session.selection.focus.offset, 3);
}

#[test]
fn soft_break_inside_an_environment_keeps_its_scope() {
    let environment_id = NodeId::generate();
    let mut session = session_with(vec![
        run("A"),
        ParagraphContent::Environment(InlineEnvironment {
            id: environment_id.clone(),
            kind: InlineEnvironmentKind::Italic,
            definition_id: None,
            source_range: None,
            props: Default::default(),
            children: vec![run("BC")],
            extra: Default::default(),
        }),
    ]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let caret = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 2,
        affinity: Affinity::Downstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: caret.clone(),
                focus: caret,
            })))
            .is_ok()
    );
    assert!(session.run(Command::InsertSoftBreak).is_ok());
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("paragraph should remain in place");
    };
    let map = match TextMap::new(paragraph) {
        Ok(map) => map,
        Err(error) => panic!("failed to map soft break: {error}"),
    };
    assert_eq!(map.text, "AB\nC");
    let ParagraphContent::Environment(environment) = &paragraph.inlines[1] else {
        panic!("soft break should stay inside the italic environment");
    };
    assert_eq!(environment.id, environment_id);
    assert!(matches!(
        environment.children[1],
        ParagraphContent::SoftBreak(_)
    ));
}

#[test]
fn utf16_mapping_rejects_half_of_a_surrogate_pair() {
    let text = "a🧪b";
    assert_eq!(utf16_offset_to_utf8(text, 1), Ok(1));
    assert_eq!(
        utf16_offset_to_utf8(text, 2),
        Err(EditError::InvalidUtf16Offset)
    );
    assert_eq!(utf16_offset_to_utf8(text, 3), Ok(5));
    assert_eq!(utf8_offset_to_utf16(text, 5), Ok(3));
}

#[test]
fn grapheme_boundaries_are_transparent_to_runs_and_environments() {
    let paragraph = Paragraph {
        id: NodeId::generate(),
        language: None,
        layout: None,
        inlines: vec![
            run("👩"),
            ParagraphContent::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Bold,
                definition_id: None,
                source_range: None,
                props: Default::default(),
                children: vec![run("🏽‍")],
                extra: Default::default(),
            }),
            ParagraphContent::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Font,
                definition_id: None,
                source_range: None,
                props: Default::default(),
                children: vec![run("💻")],
                extra: Default::default(),
            }),
            run("क्ष بّ 中文"),
        ],
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let map = match TextMap::new(&paragraph) {
        Ok(map) => map,
        Err(error) => panic!("failed to map mixed-script paragraph: {error}"),
    };
    let emoji_end = "👩🏽‍💻".len();
    assert!(map.grapheme_boundaries.contains(&emoji_end));
    assert!(!map.grapheme_boundaries.contains(&"👩".len()));
    assert!(!map.grapheme_boundaries.contains(&"👩🏽‍".len()));
    assert!(map.text.contains("क्ष بّ 中文"));
    let indic_start = emoji_end;
    let indic_end = indic_start + "क्ष".len();
    assert!(map.grapheme_boundaries.contains(&indic_end));
    assert!(!map.grapheme_boundaries.contains(&(indic_start + "क्".len())));
    let arabic_start = indic_end + " ".len();
    assert!(map.grapheme_boundaries.contains(&arabic_start));
    assert!(
        !map.grapheme_boundaries
            .contains(&("ب".len() + arabic_start))
    );
    let cjk_start = arabic_start + "بّ ".len();
    assert!(map.grapheme_boundaries.contains(&cjk_start));
    assert!(map.grapheme_boundaries.contains(&(cjk_start + "中".len())));
    assert!(map.grapheme_boundaries.contains(&map.text.len()));
    assert!(
        map.grapheme_boundaries
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );
}

#[test]
fn input_keeps_original_unicode_codepoint_sequence() {
    let original = "e\u{301}";
    let mut session = session_with(Vec::new());
    assert!(
        session
            .run(Command::InsertText(original.to_owned()))
            .is_ok()
    );

    let paragraph = match &session.document.body[0] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("document should contain a paragraph"),
    };
    let map = match TextMap::new(paragraph) {
        Ok(map) => map,
        Err(error) => panic!("failed to map inserted text: {error}"),
    };
    assert_eq!(map.text, original);
    assert_eq!(map.text.as_bytes(), original.as_bytes());
    assert_eq!(session.selection.focus.offset, original.len());
}

#[test]
fn typing_compacts_adjacent_default_runs_and_empty_placeholders() {
    let mut session = session_with(vec![run("Heading"), run(""), run("")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let end = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: "Heading".len(),
        affinity: Affinity::Downstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: end.clone(),
                focus: end,
            })))
            .is_ok()
    );

    assert!(session.run(Command::InsertText("a".to_owned())).is_ok());
    assert!(session.run(Command::InsertText("b".to_owned())).is_ok());

    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("edited body should remain a paragraph");
    };
    assert_eq!(paragraph_text(paragraph), "Headingab");
    assert_eq!(paragraph.inlines.len(), 1);
    assert_eq!(paragraph.inlines[0], run("Headingab"));
}

#[test]
fn reversed_selection_replacement_uses_the_ordered_text_range() {
    let mut session = session_with(vec![run("abcd")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let anchor = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 3,
        affinity: Affinity::Downstream,
    };
    let focus = Pos {
        offset: 1,
        affinity: Affinity::Upstream,
        ..anchor.clone()
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection { anchor, focus })))
            .is_ok()
    );
    assert!(session.run(Command::InsertText("X".to_owned())).is_ok());

    let paragraph = match &session.document.body[0] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("document should contain a paragraph"),
    };
    let map = match TextMap::new(paragraph) {
        Ok(map) => map,
        Err(error) => panic!("failed to map replaced text: {error}"),
    };
    assert_eq!(map.text, "aXd");
}

#[test]
fn replacement_stays_at_the_original_cut_when_deletion_joins_graphemes() {
    let mut session = session_with(vec![run("🇦_🇧")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let start = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: "🇦".len(),
        affinity: Affinity::Downstream,
    };
    let end = Pos {
        offset: "🇦_".len(),
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: start,
                focus: end,
            })))
            .is_ok()
    );
    assert!(session.run(Command::InsertText("X".to_owned())).is_ok());

    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("document should contain a paragraph");
    };
    assert_eq!(paragraph_text(paragraph), "🇦X🇧");
    assert_eq!(session.selection.focus.offset, "🇦X".len());
}

#[test]
fn soft_break_replacement_stays_at_the_original_cut_when_graphemes_would_join() {
    let mut session = session_with(vec![run("🇦_🇧")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let start = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: "🇦".len(),
        affinity: Affinity::Downstream,
    };
    let end = Pos {
        offset: "🇦_".len(),
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: start,
                focus: end,
            })))
            .is_ok()
    );
    assert!(session.run(Command::InsertSoftBreak).is_ok());

    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("document should contain a paragraph");
    };
    assert_eq!(paragraph_text(paragraph), "🇦\n🇧");
    assert_eq!(session.selection.focus.offset, "🇦\n".len());
}

#[test]
fn reversed_selection_split_uses_the_ordered_range() {
    let mut session = session_with(vec![run("abc")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let anchor = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 2,
        affinity: Affinity::Downstream,
    };
    let focus = Pos {
        offset: 1,
        affinity: Affinity::Upstream,
        ..anchor.clone()
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection { anchor, focus })))
            .is_ok()
    );
    assert!(session.run(Command::InsertParagraphBreak).is_ok());

    assert_eq!(session.document.body.len(), 2);
    let left = match &session.document.body[0] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("left split should be a paragraph"),
    };
    let right = match &session.document.body[1] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("right split should be a paragraph"),
    };
    assert_eq!(paragraph_text(left), "a");
    assert_eq!(paragraph_text(right), "c");
}

#[test]
fn split_after_range_deletion_keeps_original_cut_when_graphemes_would_join() {
    let mut session = session_with(vec![run("🇦_🇧")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let start = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: "🇦".len(),
        affinity: Affinity::Upstream,
    };
    let end = Pos {
        offset: "🇦_".len(),
        affinity: Affinity::Downstream,
        ..start.clone()
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: start.clone(),
                focus: end,
            })))
            .is_ok()
    );
    let effect = match session.run(Command::InsertParagraphBreak) {
        Ok(effect) => effect,
        Err(error) => panic!("selection split should succeed: {error}"),
    };

    let left = match &session.document.body[0] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("left split should be a paragraph"),
    };
    let right = match &session.document.body[1] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("right split should be a paragraph"),
    };
    assert_eq!(paragraph_text(left), "🇦");
    assert_eq!(paragraph_text(right), "🇧");
    let mapped = match effect.position_map {
        Some(map) => map.map(&start),
        None => panic!("split should return a position map"),
    };
    assert_eq!(mapped.paragraph_id, left.id);
    assert_eq!(mapped.offset, "🇦".len());
}

#[test]
fn cached_text_map_does_not_accept_a_wrong_story() {
    let mut session = session_with(vec![run("text")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let valid = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: valid.clone(),
                focus: valid,
            })))
            .is_ok()
    );
    let invalid = Pos {
        story: StoryId::EnvironmentTitle(NodeId::generate()),
        paragraph_id,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    assert_eq!(
        session.run(Command::SetSelection(Some(Selection {
            anchor: invalid.clone(),
            focus: invalid,
        }))),
        Err(EditError::WrongStory)
    );
}

#[test]
fn inserted_caret_and_position_map_snap_to_new_grapheme_boundaries() {
    let mut session = session_with(vec![run("\u{301}")]);
    let old_position = session.selection.focus.clone();
    let effect = match session.run(Command::InsertText("e".to_owned())) {
        Ok(effect) => effect,
        Err(error) => panic!("insert should succeed: {error}"),
    };
    assert_eq!(session.selection.focus.offset, "e\u{301}".len());
    assert!(session.run(Command::MoveCaret(Direction::Backward)).is_ok());
    assert_eq!(session.selection.focus.offset, 0);
    let mapped = match effect.position_map {
        Some(map) => map.map(&old_position),
        None => panic!("insert should return a position map"),
    };
    assert_eq!(mapped.offset, "e\u{301}".len());
}

#[test]
fn text_insertion_before_soft_break_keeps_document_order() {
    let mut session = session_with(vec![
        ParagraphContent::SoftBreak(Default::default()),
        run("b"),
    ]);
    assert!(session.run(Command::InsertText("a".to_owned())).is_ok());
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("document should contain a paragraph");
    };
    assert_eq!(paragraph_text(paragraph), "a\nb");
    assert!(matches!(paragraph.inlines[0], ParagraphContent::Text(_)));
    assert!(matches!(
        paragraph.inlines[1],
        ParagraphContent::SoftBreak(_)
    ));
}

#[test]
fn soft_break_insertion_before_soft_break_keeps_document_order() {
    let mut session = session_with(vec![
        ParagraphContent::SoftBreak(Default::default()),
        run("b"),
    ]);
    assert!(session.run(Command::InsertSoftBreak).is_ok());
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("document should contain a paragraph");
    };
    assert_eq!(paragraph_text(paragraph), "\n\nb");
    assert!(matches!(
        paragraph.inlines[0],
        ParagraphContent::SoftBreak(_)
    ));
    assert!(matches!(
        paragraph.inlines[1],
        ParagraphContent::SoftBreak(_)
    ));
}

#[test]
fn upstream_insertion_at_paragraph_start_stays_before_existing_content() {
    let mut session = session_with(vec![run("B")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let caret = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 0,
        affinity: Affinity::Upstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: caret.clone(),
                focus: caret,
            })))
            .is_ok()
    );
    assert!(session.run(Command::InsertText("X".to_owned())).is_ok());
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("document should contain a paragraph");
    };
    assert_eq!(paragraph_text(paragraph), "XB");
}

#[test]
fn upstream_insertion_at_environment_start_stays_outside_the_environment() {
    let mut session = session_with(vec![ParagraphContent::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        source_range: None,
        props: Default::default(),
        children: vec![run("B")],
        extra: Default::default(),
    })]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let caret = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 0,
        affinity: Affinity::Upstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: caret.clone(),
                focus: caret,
            })))
            .is_ok()
    );
    assert!(session.run(Command::InsertText("X".to_owned())).is_ok());
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("document should contain a paragraph");
    };
    assert_eq!(paragraph_text(paragraph), "XB");
    assert!(matches!(paragraph.inlines[0], ParagraphContent::Text(_)));
    assert!(matches!(
        paragraph.inlines[1],
        ParagraphContent::Environment(_)
    ));
}

#[test]
fn soft_break_boundary_affinity_preserves_environment_scope() {
    let mut session = session_with(vec![
        ParagraphContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            source_range: None,
            props: Default::default(),
            children: vec![ParagraphContent::SoftBreak(Default::default())],
            extra: Default::default(),
        }),
        ParagraphContent::SoftBreak(Default::default()),
        run("z"),
    ]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let caret = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 1,
        affinity: Affinity::Upstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: caret.clone(),
                focus: caret,
            })))
            .is_ok()
    );
    assert!(session.run(Command::InsertText("X".to_owned())).is_ok());
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("document should contain a paragraph");
    };
    let ParagraphContent::Environment(bold) = &paragraph.inlines[0] else {
        panic!("upstream insertion should remain inside the inner environment");
    };
    assert!(matches!(bold.children[1], ParagraphContent::Text(_)));
    assert!(matches!(
        paragraph.inlines[1],
        ParagraphContent::SoftBreak(_)
    ));
}

#[test]
fn missing_soft_break_side_falls_back_outside_the_environment() {
    let make_session = || {
        session_with(vec![ParagraphContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            source_range: None,
            props: Default::default(),
            children: vec![ParagraphContent::SoftBreak(Default::default())],
            extra: Default::default(),
        })])
    };
    let mut at_start = make_session();
    let paragraph_id = at_start.selection.focus.paragraph_id.clone();
    let start = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 0,
        affinity: Affinity::Upstream,
    };
    assert!(
        at_start
            .run(Command::SetSelection(Some(Selection {
                anchor: start.clone(),
                focus: start,
            })))
            .is_ok()
    );
    assert!(at_start.run(Command::InsertText("X".to_owned())).is_ok());
    let Block::Paragraph(start_paragraph) = &at_start.document.body[0] else {
        panic!("document should contain a paragraph");
    };
    assert_eq!(paragraph_text(start_paragraph), "X\n");
    assert!(matches!(
        start_paragraph.inlines[0],
        ParagraphContent::Text(_)
    ));

    let mut at_end = make_session();
    let paragraph_id = at_end.selection.focus.paragraph_id.clone();
    let end = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 1,
        affinity: Affinity::Downstream,
    };
    assert!(
        at_end
            .run(Command::SetSelection(Some(Selection {
                anchor: end.clone(),
                focus: end,
            })))
            .is_ok()
    );
    assert!(at_end.run(Command::InsertText("X".to_owned())).is_ok());
    let Block::Paragraph(end_paragraph) = &at_end.document.body[0] else {
        panic!("document should contain a paragraph");
    };
    assert_eq!(paragraph_text(end_paragraph), "\nX");
    assert!(matches!(
        end_paragraph.inlines[1],
        ParagraphContent::Text(_)
    ));
}

#[test]
fn deleting_soft_break_snaps_caret_and_mapped_positions_after_regional_indicator_join() {
    let mut session = session_with(vec![
        run("🇺"),
        ParagraphContent::SoftBreak(Default::default()),
        run("🇸"),
    ]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let caret = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: "🇺\n".len(),
        affinity: Affinity::Upstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: caret.clone(),
                focus: caret.clone(),
            })))
            .is_ok()
    );
    let effect = match session.run(Command::Delete(Direction::Backward)) {
        Ok(effect) => effect,
        Err(error) => panic!("soft break deletion should succeed: {error}"),
    };
    assert_eq!(session.selection.focus.offset, 0);
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("document should contain a paragraph");
    };
    assert_eq!(paragraph_text(paragraph), "🇺🇸");
    let mapped = match effect.position_map {
        Some(map) => map.map(&caret),
        None => panic!("delete should return a position map"),
    };
    assert_eq!(mapped.offset, 0);
}

#[test]
fn paragraph_merge_snaps_caret_when_text_joins_into_a_grapheme() {
    let mut document = Document::blank();
    let left_id = NodeId::generate();
    let right_id = NodeId::generate();
    let paragraph = |id, text: &str| {
        Block::Paragraph(Paragraph {
            id,
            language: None,
            layout: None,
            inlines: vec![run(text)],
            mark_style: Default::default(),
            props: Default::default(),
            extra: Default::default(),
        })
    };
    document.body = vec![
        paragraph(left_id.clone(), "🇺"),
        paragraph(right_id.clone(), "🇸"),
    ];
    let mut session = match Session::new(document) {
        Ok(session) => session,
        Err(error) => panic!("invalid test document: {error}"),
    };
    let right = Pos {
        story: StoryId::Body,
        paragraph_id: right_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: right.clone(),
                focus: right,
            })))
            .is_ok()
    );
    let effect = match session.run(Command::Delete(Direction::Backward)) {
        Ok(effect) => effect,
        Err(error) => panic!("paragraph merge should succeed: {error}"),
    };
    assert_eq!(session.selection.focus.paragraph_id, left_id);
    assert_eq!(session.selection.focus.offset, "🇺🇸".len());
    let mapped = match effect.position_map {
        Some(map) => map.map(&Pos {
            story: StoryId::Body,
            paragraph_id: left_id,
            offset: "🇺".len(),
            affinity: Affinity::Downstream,
        }),
        None => panic!("merge should return a position map"),
    };
    assert_eq!(mapped.offset, "🇺🇸".len());
}

#[test]
fn text_edits_preserve_unknown_run_fields() {
    let mut text_run = Run {
        text: "ab".to_owned(),
        language: None,
        props: Default::default(),
        extra: Default::default(),
    };
    text_run
        .extra
        .insert("futureHint".to_owned(), serde_json::json!({"keep": true}));
    let mut session = session_with(vec![text_flow(text_run)]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let caret = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 1,
        affinity: Affinity::Downstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: caret.clone(),
                focus: caret,
            })))
            .is_ok()
    );
    assert!(session.run(Command::InsertText("X".to_owned())).is_ok());

    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("document should contain a paragraph");
    };
    let ParagraphContent::Text(flow) = &paragraph.inlines[0] else {
        panic!("insertion should preserve the original run");
    };
    let text_run = flow
        .runs
        .first()
        .unwrap_or_else(|| panic!("edited TextFlow should retain its Run"));
    assert_eq!(text_run.text, "aXb");
    assert_eq!(
        text_run.extra.get("futureHint"),
        Some(&serde_json::json!({"keep": true}))
    );
}

#[test]
fn split_refuses_unknown_paragraph_fields_without_changing_session() {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        unreachable!();
    };
    paragraph.inlines = vec![run("ab")];
    paragraph.extra.insert(
        "futureParagraphData".to_owned(),
        serde_json::json!("preserve"),
    );
    let mut session = match Session::new(document) {
        Ok(session) => session,
        Err(error) => panic!("invalid test document: {error}"),
    };
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let caret = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 1,
        affinity: Affinity::Downstream,
    };
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: caret.clone(),
                focus: caret,
            })))
            .is_ok()
    );
    let before_document = session.document.clone();
    let before_selection = session.selection.clone();

    assert_eq!(
        session.run(Command::InsertParagraphBreak),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document, before_document);
    assert_eq!(session.selection, before_selection);
}

#[test]
fn position_maps_preserve_affinity_and_compose_replacements_with_splits() {
    let source_id = NodeId::generate();
    let right_id = NodeId::generate();
    let upstream = Pos {
        story: StoryId::Body,
        paragraph_id: source_id.clone(),
        offset: 2,
        affinity: Affinity::Upstream,
    };
    let downstream = Pos {
        affinity: Affinity::Downstream,
        ..upstream.clone()
    };
    let split = PositionMap::ParagraphSplit {
        source_id: source_id.clone(),
        right_id: right_id.clone(),
        split_offset: 2,
        source_story: StoryId::Body,
        right_story: StoryId::Body,
    };
    assert_eq!(split.map(&upstream), upstream);
    assert_eq!(split.map(&downstream).paragraph_id, right_id);
    assert_eq!(split.map(&downstream).offset, 0);

    let composite = PositionMap::Composite(vec![
        PositionMap::TextEdit {
            paragraph_id: source_id.clone(),
            start: 1,
            end: 3,
            inserted_len: 0,
            snap_graphemes: true,
            result_boundaries: vec![0, 1, 2, 3],
        },
        PositionMap::ParagraphSplit {
            source_id,
            right_id: right_id.clone(),
            split_offset: 1,
            source_story: StoryId::Body,
            right_story: StoryId::Body,
        },
    ]);
    let later = Pos {
        offset: 4,
        ..downstream
    };
    let mapped = composite.map(&later);
    assert_eq!(mapped.paragraph_id, right_id);
    assert_eq!(mapped.offset, 1);
}

#[test]
fn ime_cancel_is_transient_and_enter_is_rejected_while_composing() {
    let mut session = session_with(vec![run("title")]);
    let before = session.document.clone();
    assert!(
        session
            .run(Command::UpdateComposition {
                id: 12,
                preedit: "候補".to_owned(),
            })
            .is_ok()
    );
    let before_selection = session.selection.clone();
    let mut moved = before_selection.clone();
    moved.anchor.offset = 2;
    moved.focus.offset = 2;
    assert_eq!(
        session.run(Command::SetSelection(Some(moved))),
        Err(EditError::CompositionPending)
    );
    assert_eq!(session.selection, before_selection);
    assert_eq!(
        session.run(Command::InsertParagraphBreak),
        Err(EditError::CompositionPending)
    );
    assert_eq!(session.document, before);
    assert!(session.composition().is_some());
    assert!(session.run(Command::CancelComposition { id: 12 }).is_ok());
    assert_eq!(session.document, before);
    assert!(session.composition().is_none());
}

#[test]
fn public_command_ids_and_error_codes_remain_stable() {
    let commands = [
        (Command::InsertText("text".to_owned()), "text.insert"),
        (
            Command::InsertOneCharacterWidthSpace,
            "text.insert.one_character_width_space",
        ),
        (
            Command::InsertTwoCharacterWidthSpace,
            "text.insert.two_character_width_space",
        ),
        (Command::PasteText("text".to_owned()), "clipboard.paste"),
        (Command::InsertSoftBreak, "text.soft_break"),
        (Command::InsertParagraphBreak, "text.split_paragraph"),
        (Command::Delete(Direction::Forward), "text.delete"),
        (Command::MoveCaret(Direction::Backward), "caret.move"),
        (Command::SetSelection(None), "selection.set"),
        (
            Command::UpdateComposition {
                id: 1,
                preedit: String::new(),
            },
            "ime.update",
        ),
        (
            Command::CommitComposition {
                id: 1,
                text: String::new(),
            },
            "ime.commit",
        ),
        (Command::CancelComposition { id: 1 }, "ime.cancel"),
        (
            Command::SetPageGeometry(PageGeometry::default()),
            "page.geometry.set",
        ),
        (
            Command::SetInlineFontFamily("Noto Serif".to_owned()),
            "text.format.font_family",
        ),
        (
            Command::SetDefaultFontFamily(Some("Noto Serif".to_owned())),
            "document.default_font_family.set",
        ),
    ];
    for (command, expected) in commands {
        assert_eq!(command.id(), expected);
    }
    let errors = [
        (
            EditError::InvalidDocument("missing body".to_owned()),
            "edit.invalid_document: missing body",
        ),
        (
            EditError::UnknownParagraph("paragraph-1".to_owned()),
            "edit.unknown_paragraph: paragraph-1",
        ),
        (EditError::WrongStory, "edit.position_story_mismatch"),
        (EditError::InvalidPosition, "edit.invalid_position"),
        (
            EditError::CrossParagraphSelection,
            "edit.cross_paragraph_selection",
        ),
        (
            EditError::UnsupportedOpaqueContent,
            "edit.opaque_content_boundary",
        ),
        (
            EditError::UnsupportedNewline,
            "edit.newline_requires_paragraph_command",
        ),
        (
            EditError::IncompatibleParagraphMerge,
            "edit.incompatible_paragraph_merge",
        ),
        (EditError::InvalidUtf16Offset, "edit.invalid_utf16_offset"),
        (
            EditError::InvalidCompositionId,
            "edit.invalid_composition_id",
        ),
        (EditError::CompositionPending, "edit.composition_pending"),
        (
            EditError::InvalidPageGeometry("invalid margins".to_owned()),
            "edit.invalid_page_geometry: invalid margins",
        ),
        (EditError::UnknownCommand, "edit.unknown_command"),
    ];
    for (error, expected) in errors {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn sessions_reject_invalid_models_and_models_without_an_editable_paragraph() {
    let mut invalid = Document::blank();
    invalid.page_sections.clear();
    assert!(matches!(
        Session::new(invalid),
        Err(EditError::InvalidDocument(message)) if message.contains("PageSection")
    ));
    let mut without_paragraph = Document::blank();
    without_paragraph.body.clear();
    assert!(matches!(
        Session::new(without_paragraph),
        Err(EditError::InvalidDocument(message)) if message == "document has no paragraph"
    ));
}

#[test]
fn caret_movement_uses_graphemes_and_selection_collapse_is_directional() -> Result<(), EditError> {
    let mut session = session_with(vec![run("a🇨🇳e\u{301}z")]);
    let boundaries = [0, 1, 9, 12, 13];
    for offset in boundaries.into_iter().skip(1) {
        let effect = session.run(Command::MoveCaret(Direction::Forward))?;
        assert!(effect.selection_changed);
        assert_eq!(session.selection().focus.offset, offset);
    }
    assert_eq!(
        session.run(Command::MoveCaret(Direction::Forward))?,
        Effect::default()
    );
    for offset in boundaries.into_iter().rev().skip(1) {
        session.run(Command::MoveCaret(Direction::Backward))?;
        assert_eq!(session.selection().focus.offset, offset);
    }
    let start = session.selection().focus.clone();
    let end = Pos {
        offset: 12,
        ..start.clone()
    };
    let reversed = Selection {
        anchor: end.clone(),
        focus: start.clone(),
    };
    session.run(Command::SetSelection(Some(reversed.clone())))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.selection().anchor, start);
    assert_eq!(session.selection().anchor, session.selection().focus);
    session.run(Command::SetSelection(Some(reversed)))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.selection().focus, end);
    session.run(Command::SetSelection(None))?;
    assert_eq!(session.selection().anchor, session.selection().focus);
    assert_eq!(session.document().body.len(), 1);
    Ok(())
}

#[test]
fn cross_paragraph_selection_collapses_in_document_order() -> Result<(), EditError> {
    let mut document = Document::blank();
    let mut first = Paragraph::empty(NodeId::generate());
    first.inlines = vec![run("first")];
    let mut last = Paragraph::empty(NodeId::generate());
    last.inlines = vec![run("last")];
    let first_id = first.id.clone();
    let last_id = last.id.clone();
    document.body = vec![Block::Paragraph(first), Block::Paragraph(last)];
    let mut session = Session::new(document)?;
    let first = Pos {
        story: StoryId::Body,
        paragraph_id: first_id,
        offset: 1,
        affinity: Affinity::Downstream,
    };
    let last = Pos {
        paragraph_id: last_id,
        offset: 2,
        ..first.clone()
    };
    let reversed = Selection {
        anchor: last.clone(),
        focus: first.clone(),
    };
    session.run(Command::SetSelection(Some(reversed.clone())))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.selection().focus, first);
    session.run(Command::SetSelection(Some(reversed)))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.selection().focus, last);
    Ok(())
}

#[test]
fn visual_affinity_is_session_state_outside_the_logical_position() -> Result<(), EditError> {
    let mut session = session_with(vec![run("a")]);
    let selection = session.selection().clone();
    assert_eq!(session.document_generation(), 0);

    let effect = session.set_selection_with_visual_affinity(
        Some(selection.clone()),
        caret_layout::VisualAffinity::Upstream,
    )?;
    assert!(effect.selection_changed);
    assert_eq!(session.selection(), &selection);
    assert_eq!(
        session.selection_visual_affinity(),
        caret_layout::VisualAffinity::Upstream
    );
    assert_eq!(session.document_generation(), 0);
    let fork = session.clone();
    assert_eq!(
        fork.selection_visual_affinity(),
        caret_layout::VisualAffinity::Upstream
    );
    assert_ne!(fork.session_id(), session.session_id());

    let invalid = Selection {
        focus: Pos {
            offset: 2,
            ..selection.focus.clone()
        },
        ..selection.clone()
    };
    assert!(
        session
            .set_selection_with_visual_affinity(
                Some(invalid),
                caret_layout::VisualAffinity::Downstream
            )
            .is_err()
    );
    assert_eq!(session.selection(), &selection);
    assert_eq!(
        session.selection_visual_affinity(),
        caret_layout::VisualAffinity::Upstream
    );
    Ok(())
}

#[test]
fn invalid_selection_and_incompatible_merge_preserve_the_session() -> Result<(), EditError> {
    let mut document = Document::blank();
    let Block::Paragraph(first) = &mut document.body[0] else {
        unreachable!();
    };
    first.inlines = vec![run("left")];
    let first_id = first.id.clone();
    let mut second = Paragraph::empty(NodeId::generate());
    second.inlines = vec![run("right")];
    second.layout = Some(caret_doc::ParagraphLayoutOverrides {
        space_after_centipoints: Some(500),
        ..caret_doc::ParagraphLayoutOverrides::default()
    });
    document.body.push(Block::Paragraph(second));
    let mut session = Session::new(document)?;
    let before_document = session.document().clone();
    let before_selection = session.selection().clone();
    let unknown = Pos {
        paragraph_id: NodeId::generate(),
        ..before_selection.focus.clone()
    };
    assert!(matches!(
        session.run(Command::SetSelection(Some(Selection {
            anchor: unknown.clone(),
            focus: unknown,
        }))),
        Err(EditError::UnknownParagraph(_))
    ));
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.selection(), &before_selection);
    let end = Pos {
        story: StoryId::Body,
        paragraph_id: first_id,
        offset: 4,
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: end.clone(),
        focus: end,
    })))?;
    let before_merge_selection = session.selection().clone();
    assert_eq!(
        session.run(Command::Delete(Direction::Forward)),
        Err(EditError::IncompatibleParagraphMerge)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.selection(), &before_merge_selection);
    Ok(())
}

#[test]
fn ime_failure_restores_preedit_and_rejects_other_composition_ids() -> Result<(), EditError> {
    let mut session = session_with(vec![run("before")]);
    session.run(Command::UpdateComposition {
        id: 21,
        preedit: "候補".to_owned(),
    })?;
    let before = session.document().clone();
    let selection = session.selection().clone();
    for command in [
        Command::UpdateComposition {
            id: 22,
            preedit: "other".to_owned(),
        },
        Command::CommitComposition {
            id: 22,
            text: "other".to_owned(),
        },
        Command::CancelComposition { id: 22 },
    ] {
        assert_eq!(session.run(command), Err(EditError::InvalidCompositionId));
        assert_eq!(session.composition().map(|value| value.id), Some(21));
    }
    assert_eq!(
        session.run(Command::CommitComposition {
            id: 21,
            text: "line\nbreak".to_owned(),
        }),
        Err(EditError::UnsupportedNewline)
    );
    assert_eq!(session.document(), &before);
    assert_eq!(session.selection(), &selection);
    assert_eq!(
        session.composition().map(|value| value.preedit.as_str()),
        Some("候補")
    );
    for command in [
        Command::InsertText("blocked".to_owned()),
        Command::PasteText("blocked\npaste".to_owned()),
        Command::InsertSoftBreak,
        Command::Delete(Direction::Backward),
        Command::MoveCaret(Direction::Forward),
    ] {
        assert_eq!(session.run(command), Err(EditError::CompositionPending));
    }
    session.run(Command::CommitComposition {
        id: 21,
        text: "確定".to_owned(),
    })?;
    assert!(session.composition().is_none());
    assert_eq!(
        session.run(Command::UpdateComposition {
            id: 21,
            preedit: "duplicate".to_owned(),
        }),
        Err(EditError::InvalidCompositionId)
    );
    assert_eq!(
        session.run(Command::CancelComposition { id: 21 }),
        Err(EditError::InvalidCompositionId)
    );
    Ok(())
}

#[test]
fn soft_break_refuses_to_split_unknown_run_fields_or_delete_unknown_break_fields()
-> Result<(), EditError> {
    let mut unknown_run = match run("ab") {
        ParagraphContent::Text(flow) => flow.runs[0].clone(),
        _ => unreachable!(),
    };
    unknown_run
        .extra
        .insert("future".to_owned(), serde_json::json!(true));
    let mut session = session_with(vec![text_flow(unknown_run)]);
    let caret = Pos {
        offset: 1,
        ..session.selection().focus.clone()
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: caret.clone(),
        focus: caret,
    })))?;
    let before = session.document().clone();
    assert_eq!(
        session.run(Command::InsertSoftBreak),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before);

    let mut soft_break = caret_doc::SoftBreak::default();
    soft_break
        .extra
        .insert("future".to_owned(), serde_json::json!({"mode": 1}));
    let mut session = session_with(vec![
        run("a"),
        ParagraphContent::SoftBreak(soft_break),
        run("b"),
    ]);
    let anchor = Pos {
        offset: 1,
        ..session.selection().focus.clone()
    };
    let focus = Pos {
        offset: 2,
        ..anchor.clone()
    };
    session.run(Command::SetSelection(Some(Selection { anchor, focus })))?;
    let before = session.document().clone();
    let selection = session.selection().clone();
    assert_eq!(
        session.run(Command::Delete(Direction::Forward)),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before);
    assert_eq!(session.selection(), &selection);
    Ok(())
}

#[test]
fn empty_runs_keep_their_order_when_inserting_soft_breaks() -> Result<(), EditError> {
    for (affinity, break_index) in [(Affinity::Upstream, 1), (Affinity::Downstream, 0)] {
        let mut session = session_with(vec![run("")]);
        let caret = Pos {
            affinity,
            ..session.selection().focus.clone()
        };
        session.run(Command::SetSelection(Some(Selection {
            anchor: caret.clone(),
            focus: caret,
        })))?;
        session.run(Command::InsertSoftBreak)?;
        let Block::Paragraph(paragraph) = &session.document().body[0] else {
            unreachable!();
        };
        assert_eq!(paragraph.inlines.len(), 2);
        assert!(matches!(
            paragraph.inlines[break_index],
            ParagraphContent::SoftBreak(_)
        ));
        assert_eq!(paragraph_text(paragraph), "\n");
    }
    Ok(())
}

#[test]
fn backward_movement_at_environment_start_leaves_its_input_scope() -> Result<(), EditError> {
    let mut session = session_with(vec![
        run("a"),
        ParagraphContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Italic,
            definition_id: None,
            source_range: None,
            props: Default::default(),
            children: vec![run("b")],
            extra: Default::default(),
        }),
    ]);
    let inside = Pos {
        offset: 1,
        affinity: Affinity::Downstream,
        ..session.selection().focus.clone()
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: inside.clone(),
        focus: inside,
    })))?;

    let effect = session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(effect.selection_changed);
    assert!(!effect.document_changed);
    assert_eq!(session.selection().focus.offset, 1);
    assert_eq!(session.selection().focus.affinity, Affinity::Upstream);
    session.run(Command::InsertText("outside".to_owned()))?;

    let Block::Paragraph(paragraph) = &session.document().body[0] else {
        unreachable!();
    };
    let ParagraphContent::Environment(environment) = &paragraph.inlines[1] else {
        panic!("italic environment should keep its position");
    };
    assert_eq!(paragraph_text(paragraph), "aoutsideb");
    assert_eq!(environment.children, vec![run("b")]);
    Ok(())
}

#[test]
fn selected_delete_and_forward_paragraph_merge_publish_position_maps() -> Result<(), EditError> {
    let mut session = session_with(vec![run("first")]);
    let paragraph_id = session.selection().focus.paragraph_id.clone();
    let start = Pos {
        offset: 1,
        affinity: Affinity::Upstream,
        ..session.selection().focus.clone()
    };
    let end = Pos {
        offset: 4,
        affinity: Affinity::Downstream,
        ..start.clone()
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: end.clone(),
        focus: start.clone(),
    })))?;
    let deletion = session.run(Command::Delete(Direction::Backward))?;
    assert!(deletion.document_changed);
    assert_eq!(session.selection().focus.offset, 1);
    assert_eq!(session.selection().focus.affinity, Affinity::Upstream);
    let mapped = deletion
        .position_map
        .as_ref()
        .unwrap_or_else(|| panic!("selection deletion needs a position map"))
        .map(&end);
    assert_eq!(mapped.offset, 1);
    assert_eq!(mapped.paragraph_id, paragraph_id);
    session.run(Command::InsertParagraphBreak)?;
    let right = session.selection().focus.clone();
    let left_end = Pos {
        paragraph_id: paragraph_id.clone(),
        offset: 1,
        ..right.clone()
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: left_end.clone(),
        focus: left_end,
    })))?;
    let merge = session.run(Command::Delete(Direction::Forward))?;
    assert!(merge.document_changed);
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(session.selection().focus.paragraph_id, paragraph_id);
    let mapped_right = merge
        .position_map
        .as_ref()
        .unwrap_or_else(|| panic!("paragraph merge needs a position map"))
        .map(&right);
    assert_eq!(mapped_right.offset, 1);
    assert_eq!(mapped_right.paragraph_id, paragraph_id);
    let Block::Paragraph(paragraph) = &session.document().body[0] else {
        unreachable!();
    };
    assert_eq!(paragraph_text(paragraph), "ft");
    Ok(())
}

#[test]
fn selected_backspace_merges_paragraphs_and_keeps_unselected_text() -> Result<(), EditError> {
    let mut document = Document::blank();
    let Block::Paragraph(first) = &mut document.body[0] else {
        unreachable!();
    };
    first.inlines = vec![run("你好")];
    let first_id = first.id.clone();
    let second_id = NodeId::generate();
    let mut second = first.clone();
    second.id = second_id.clone();
    second.inlines = vec![run("世界")];
    document.body.push(Block::Paragraph(second));
    let mut session = Session::new(document)?;
    let anchor = Pos {
        paragraph_id: first_id.clone(),
        offset: "你".len(),
        affinity: Affinity::Downstream,
        ..session.selection().focus.clone()
    };
    let focus = Pos {
        paragraph_id: second_id.clone(),
        offset: "世界".len() - "界".len(),
        affinity: Affinity::Upstream,
        story: StoryId::Body,
    };
    session.run(Command::SetSelection(Some(Selection { anchor, focus })))?;

    let effect = session.run(Command::Delete(Direction::Backward))?;
    assert!(effect.document_changed);
    assert_eq!(session.document.body.len(), 1);
    let Block::Paragraph(merged) = &session.document.body[0] else {
        unreachable!();
    };
    assert_eq!(paragraph_text(merged), "你界");
    assert_eq!(merged.id, first_id);
    assert_eq!(session.selection().focus.offset, "你".len());
    assert_eq!(session.selection().anchor, session.selection().focus);
    assert_eq!(
        effect
            .position_map
            .as_ref()
            .map(|map| map.map(&Pos {
                story: StoryId::Body,
                paragraph_id: second_id,
                offset: "世界".len(),
                affinity: Affinity::Downstream,
            }))
            .map(|mapped| (mapped.paragraph_id, mapped.offset)),
        Some((first_id, "你界".len()))
    );
    Ok(())
}

#[test]
fn selected_text_is_extracted_by_the_session_without_layout() -> Result<(), EditError> {
    let mut document = Document::blank();
    let Block::Paragraph(first) = &mut document.body[0] else {
        unreachable!();
    };
    first.inlines = vec![run("你好")];
    let first_id = first.id.clone();
    let second_id = NodeId::generate();
    let mut second = first.clone();
    second.id = second_id.clone();
    second.inlines = vec![run("world")];
    document.body.push(Block::Paragraph(second));
    let mut session = Session::new(document)?;
    session.run(Command::SetSelection(Some(Selection {
        anchor: Pos {
            paragraph_id: second_id,
            offset: 2,
            affinity: Affinity::Downstream,
            story: StoryId::Body,
        },
        focus: Pos {
            paragraph_id: first_id,
            offset: "你".len(),
            affinity: Affinity::Upstream,
            story: StoryId::Body,
        },
    })))?;

    assert_eq!(session.selected_text()?, "好\nwo");
    Ok(())
}

#[test]
fn selected_text_includes_title_and_body_across_environment_stories() -> Result<(), EditError> {
    let title_id = NodeId::generate();
    let body_id = NodeId::generate();
    let environment_id = NodeId::generate();
    let title = Paragraph {
        id: title_id.clone(),
        language: None,
        layout: None,
        inlines: vec![run("Section title")],
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let body = Paragraph {
        id: body_id.clone(),
        language: None,
        layout: None,
        inlines: vec![run("body text")],
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let before = Paragraph {
        id: NodeId::generate(),
        language: None,
        layout: None,
        inlines: vec![run("before")],
        mark_style: Default::default(),
        props: Default::default(),
        extra: Default::default(),
    };
    let before_id = before.id.clone();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        Block::Environment(BlockEnvironment {
            id: environment_id.clone(),
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: Default::default(),
            title: Some(title),
            children: vec![Block::Paragraph(body)],
            extra: Default::default(),
        }),
    ];
    let mut session = Session::new(document)?;

    session.select_block_environments_from_range(
        Pos {
            story: StoryId::EnvironmentTitle(environment_id.clone()),
            paragraph_id: title_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        },
        Pos {
            story: StoryId::Body,
            paragraph_id: body_id,
            offset: "body text".len(),
            affinity: Affinity::Upstream,
        },
        VisualAffinity::Downstream,
    )?;

    assert_eq!(session.selected_text()?, "Section title\nbody text");

    session.select_block_environments_from_range(
        Pos {
            story: StoryId::Body,
            paragraph_id: before_id,
            offset: 0,
            affinity: Affinity::Downstream,
        },
        Pos {
            story: StoryId::EnvironmentTitle(environment_id.clone()),
            paragraph_id: title_id,
            offset: 2,
            affinity: Affinity::Upstream,
        },
        VisualAffinity::Downstream,
    )?;
    assert_eq!(session.selected_text()?, "before\nSection title\nbody text");

    session.select_block_environment(environment_id)?;
    assert_eq!(session.selected_text()?, "Section title\nbody text");
    Ok(())
}

#[test]
fn selection_on_the_same_text_boundary_has_no_copied_text() -> Result<(), EditError> {
    let mut session = session_with(vec![run("ab")]);
    let anchor = Pos {
        offset: 1,
        affinity: Affinity::Upstream,
        ..session.selection().focus.clone()
    };
    let focus = Pos {
        affinity: Affinity::Downstream,
        ..anchor.clone()
    };
    session.run(Command::SetSelection(Some(Selection { anchor, focus })))?;
    assert_eq!(session.selected_text()?, "");
    assert_eq!(session.document_generation(), 0);
    Ok(())
}

#[test]
fn cross_paragraph_selection_is_replaced_by_enter() -> Result<(), EditError> {
    let mut document = Document::blank();
    let Block::Paragraph(first) = &mut document.body[0] else {
        unreachable!();
    };
    first.inlines = vec![run("abc")];
    let first_id = first.id.clone();
    let mut second = first.clone();
    second.id = NodeId::generate();
    second.inlines = vec![run("def")];
    document.body.push(Block::Paragraph(second));
    let mut session = Session::new(document)?;
    let first = Pos {
        paragraph_id: first_id.clone(),
        offset: 1,
        affinity: Affinity::Downstream,
        ..session.selection().focus.clone()
    };
    let second = Pos {
        paragraph_id: session
            .document()
            .body
            .get(1)
            .and_then(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph.id.clone()),
                _ => None,
            })
            .ok_or(EditError::InvalidPosition)?,
        offset: 2,
        affinity: Affinity::Upstream,
        story: StoryId::Body,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: first,
        focus: second,
    })))?;

    let effect = session.run(Command::InsertParagraphBreak)?;
    assert!(effect.document_changed);
    assert_eq!(session.document().body.len(), 2);
    let texts = session
        .document()
        .body
        .iter()
        .map(|block| match block {
            Block::Paragraph(paragraph) => paragraph_text(paragraph),
            _ => panic!("enter replacement should keep only paragraphs"),
        })
        .collect::<Vec<_>>();
    assert_eq!(texts, ["a", "f"]);
    let Block::Paragraph(left) = &session.document().body[0] else {
        unreachable!();
    };
    let Block::Paragraph(right) = &session.document().body[1] else {
        unreachable!();
    };
    assert_eq!(left.id, first_id);
    assert_eq!(session.selection().focus.paragraph_id, right.id);
    assert_eq!(session.selection().focus.offset, 0);
    Ok(())
}

#[test]
fn cross_paragraph_replacement_keeps_the_original_cut_when_graphemes_join() -> Result<(), EditError>
{
    let mut document = Document::blank();
    let Block::Paragraph(first) = &mut document.body[0] else {
        unreachable!();
    };
    first.inlines = vec![run("a")];
    let first_id = first.id.clone();
    let mut second = first.clone();
    second.id = NodeId::generate();
    second.inlines = vec![run("\u{301}b")];
    let second_id = second.id.clone();
    document.body.push(Block::Paragraph(second));
    let mut session = Session::new(document)?;
    let start = Pos {
        paragraph_id: first_id,
        offset: 1,
        affinity: Affinity::Upstream,
        ..session.selection().focus.clone()
    };
    let original_start = start.clone();
    let end = Pos {
        story: StoryId::Body,
        paragraph_id: second_id,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: start,
        focus: end,
    })))?;

    let effect = session.run(Command::InsertText("X".to_owned()))?;
    let mapped_start = effect
        .position_map
        .as_ref()
        .map(|position_map| position_map.map(&original_start))
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(mapped_start.offset, 1);
    assert_eq!(session.document().body.len(), 1);
    let Block::Paragraph(paragraph) = &session.document().body[0] else {
        unreachable!();
    };
    assert_eq!(paragraph_text(paragraph), "aX\u{301}b");
    assert_eq!(session.selection().focus.offset, "aX\u{301}".len());
    Ok(())
}
