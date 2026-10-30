//! 验证格式命令的用户可见范围、撤销和待输入行为。

use super::*;
use crate::editing::{Command, Direction, Selection, StoryId};
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, InlineEnvironment, InlineNode, NodeId,
    Paragraph, Run,
};
use serde_json::Value;
use std::collections::BTreeMap;

fn run(text: &str) -> InlineNode {
    InlineNode::Run(Run {
        text: text.to_owned(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    })
}

fn session_with_paragraphs(paragraphs: Vec<Paragraph>) -> Session {
    let mut document = Document::blank();
    document.body = paragraphs.into_iter().map(Block::Paragraph).collect();
    Session::new(document).unwrap_or_else(|error| panic!("session creation failed: {error}"))
}

fn paragraph(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![run(text)];
    paragraph
}

fn environment(id: NodeId, text: &str, extra: BTreeMap<String, Value>) -> InlineNode {
    InlineNode::Environment(InlineEnvironment {
        id,
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::from([("enabled".to_owned(), Value::Bool(true))]),
        children: vec![run(text)],
        extra,
    })
}

fn select(session: &mut Session, anchor: Pos, focus: Pos) {
    session
        .run(Command::SetSelection(Some(Selection { anchor, focus })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));
}

fn assert_all_format(
    session: &Session,
    paragraph_id: &NodeId,
    kind: InlineEnvironmentKind,
    value: bool,
) {
    let position = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let paragraph = session
        .paragraph_for(&position)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    let spans = format_spans(&paragraph.inlines)
        .unwrap_or_else(|error| panic!("format resolution failed: {error}"));
    assert!(!spans.is_empty());
    assert!(
        spans.iter().all(|span| format_value(span, kind) == value),
        "unexpected {kind:?} spans: {spans:?}"
    );
}

#[test]
fn selection_format_preserves_text_and_undo_restores_the_original_format_tree() {
    let mut session = session_with_paragraphs(vec![paragraph("alpha")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let start = session.selection.focus.clone();
    let end = Pos {
        offset: 5,
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);

    let effect = session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))
        .unwrap_or_else(|error| panic!("bold command failed: {error}"));
    assert!(effect.document_changed);
    assert_eq!(
        session
            .selected_text()
            .unwrap_or_else(|error| panic!("selected text query failed: {error}")),
        "alpha"
    );
    assert_all_format(&session, &paragraph_id, InlineEnvironmentKind::Bold, true);

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    assert_eq!(
        session
            .selected_text()
            .unwrap_or_else(|error| panic!("selected text query failed: {error}")),
        "alpha"
    );
    assert_all_format(&session, &paragraph_id, InlineEnvironmentKind::Bold, false);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo failed: {error}"));
    assert_all_format(&session, &paragraph_id, InlineEnvironmentKind::Bold, true);
}

#[test]
fn bold_command_candidate_applies_to_the_existing_selection() {
    let mut session = session_with_paragraphs(vec![paragraph("selected")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let start = session.selection.focus.clone();
    let end = Pos {
        offset: "selected".len(),
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command mode did not open: {error}"));
    session
        .run(Command::TypeCommandLine("bold".to_owned()))
        .unwrap_or_else(|error| panic!("bold command query failed: {error}"));
    assert!(session.command_line_candidates().iter().any(|candidate| {
        candidate.alias == "\\bold" && candidate.command_id == "text.format.bold"
    }));

    let effect = session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("bold command acceptance failed: {error}"));

    assert!(effect.document_changed);
    assert!(session.command_line().is_none());
    assert_eq!(
        session
            .selected_text()
            .unwrap_or_else(|error| panic!("selected text query failed: {error}")),
        "selected"
    );
    assert_all_format(&session, &paragraph_id, InlineEnvironmentKind::Bold, true);
}

#[test]
fn mixed_selection_applies_bold_to_the_entire_range() {
    let mut first = Paragraph::empty(NodeId::generate());
    let original_bold = InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::from([("enabled".to_owned(), Value::Bool(true))]),
        children: vec![run("a")],
        extra: BTreeMap::new(),
    };
    first.inlines = vec![InlineNode::Environment(original_bold), run("bc")];
    let mut session = session_with_paragraphs(vec![first]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let start = session.selection.focus.clone();
    let end = Pos {
        offset: 3,
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);

    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))
        .unwrap_or_else(|error| panic!("bold command failed: {error}"));

    assert_all_format(&session, &paragraph_id, InlineEnvironmentKind::Bold, true);
}

#[test]
fn toggling_a_fully_bold_range_creates_an_explicit_local_off_state() {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![InlineNode::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::new(),
        children: vec![run("text")],
        extra: BTreeMap::new(),
    })];
    let mut session = session_with_paragraphs(vec![paragraph]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let start = session.selection.focus.clone();
    let end = Pos {
        offset: 4,
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);

    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))
        .unwrap_or_else(|error| panic!("bold command failed: {error}"));

    assert_all_format(&session, &paragraph_id, InlineEnvironmentKind::Bold, false);
}

#[test]
fn cross_paragraph_selection_formats_each_selected_text_fragment() {
    let first = paragraph("one");
    let second = paragraph("two");
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    let mut session = session_with_paragraphs(vec![first, second]);
    let anchor = Pos {
        story: StoryId::Body,
        paragraph_id: first_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let focus = Pos {
        story: StoryId::Body,
        paragraph_id: second_id.clone(),
        offset: 3,
        affinity: Affinity::Upstream,
    };
    select(&mut session, anchor, focus);

    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))
        .unwrap_or_else(|error| panic!("italic command failed: {error}"));

    assert_all_format(&session, &first_id, InlineEnvironmentKind::Italic, true);
    assert_all_format(&session, &second_id, InlineEnvironmentKind::Italic, true);
}

#[test]
fn selection_format_applies_inside_a_block_environment() {
    let child = paragraph("inside section");
    let paragraph_id = child.id.clone();
    let mut document = Document::blank();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: None,
        children: vec![Block::Paragraph(child)],
        extra: BTreeMap::new(),
    })];
    let mut session =
        Session::new(document).unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let start = session.selection.focus.clone();
    let end = Pos {
        offset: "inside section".len(),
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);

    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))
        .unwrap_or_else(|error| panic!("bold command failed: {error}"));

    assert_all_format(&session, &paragraph_id, InlineEnvironmentKind::Bold, true);
}

#[test]
fn empty_selection_formats_future_input_until_caret_moves() {
    let mut session = session_with_paragraphs(vec![Paragraph::empty(NodeId::generate())]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))
        .unwrap_or_else(|error| panic!("pending bold toggle failed: {error}"));
    session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("formatted input failed: {error}"));
    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))
        .unwrap_or_else(|error| panic!("pending bold disable failed: {error}"));
    session
        .run(Command::InsertText("y".to_owned()))
        .unwrap_or_else(|error| panic!("plain input failed: {error}"));
    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("caret movement failed: {error}"));

    let position = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let paragraph = session
        .paragraph_for(&position)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    assert_eq!(
        TextMap::new(paragraph)
            .unwrap_or_else(|error| panic!("text map creation failed: {error}"))
            .text,
        "xy"
    );
    let spans = format_spans(&paragraph.inlines)
        .unwrap_or_else(|error| panic!("format resolution failed: {error}"));
    assert_eq!(spans.len(), 2);
    assert!(spans[0].bold);
    assert!(!spans[1].bold);
}

#[test]
fn consecutive_typed_characters_share_one_inline_environment() {
    let mut session = session_with_paragraphs(vec![Paragraph::empty(NodeId::generate())]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))
        .unwrap_or_else(|error| panic!("pending bold toggle failed: {error}"));
    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))
        .unwrap_or_else(|error| panic!("pending italic toggle failed: {error}"));

    let mut first_environment_id = None;
    for (index, character) in ["a", "b", "c"].into_iter().enumerate() {
        session
            .run(Command::InsertText(character.to_owned()))
            .unwrap_or_else(|error| panic!("typing bold text failed: {error}"));
        let active = session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("inline environment lookup failed: {error}"))
            .unwrap_or_else(|| {
                panic!("typing should keep the caret inside the active environment")
            });
        assert_eq!(active.start_utf8, 0);
        assert_eq!(active.end_utf8, index + 1);
        if index == 0 {
            let paragraph = session
                .paragraph_for(&session.selection.focus)
                .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
            let Some(InlineNode::Environment(environment)) = paragraph.inlines.first() else {
                panic!("first formatted character should create an environment");
            };
            first_environment_id = Some(environment.id.clone());
        }
    }

    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    let [InlineNode::Environment(bold)] = paragraph.inlines.as_slice() else {
        panic!(
            "consecutive text should share one inline environment: {:#?}",
            paragraph.inlines
        );
    };
    assert_eq!(bold.kind, InlineEnvironmentKind::Bold);
    assert_eq!(
        bold.id,
        first_environment_id
            .unwrap_or_else(|| panic!("first environment id should have been recorded"))
    );
    let [InlineNode::Environment(italic)] = bold.children.as_slice() else {
        panic!(
            "nested italic text should share one environment: {:#?}",
            bold.children
        );
    };
    assert_eq!(italic.kind, InlineEnvironmentKind::Italic);
    assert_eq!(italic.children.len(), 1);
    assert!(matches!(
        italic.children.first(),
        Some(InlineNode::Run(run)) if run.text == "abc"
    ));
    assert_all_format(&session, &paragraph_id, InlineEnvironmentKind::Bold, true);
    assert_all_format(&session, &paragraph_id, InlineEnvironmentKind::Italic, true);

    session
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| panic!("moving out of the environment failed: {error}"));
    assert!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("inline environment lookup failed: {error}"))
            .is_none()
    );
}

#[test]
fn pending_format_expands_to_keep_combining_marks_in_the_same_grapheme() {
    let mut session = session_with_paragraphs(vec![paragraph("e")]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    let end = Pos {
        offset: 1,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    select(&mut session, end.clone(), end);
    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))
        .unwrap_or_else(|error| panic!("pending bold toggle failed: {error}"));
    session
        .run(Command::InsertText("\u{301}".to_owned()))
        .unwrap_or_else(|error| panic!("combining mark insertion failed: {error}"));

    let position = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let paragraph = session
        .paragraph_for(&position)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    assert_eq!(
        TextMap::new(paragraph)
            .unwrap_or_else(|error| panic!("text map creation failed: {error}"))
            .text,
        "e\u{301}"
    );
    assert_all_format(&session, &paragraph_id, InlineEnvironmentKind::Bold, true);
}

#[test]
fn input_method_preview_keeps_pending_format_highlight_and_commit_continuable() {
    let mut session = session_with_paragraphs(vec![Paragraph::empty(NodeId::generate())]);
    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))
        .unwrap_or_else(|error| panic!("pending bold toggle failed: {error}"));
    session
        .run(Command::UpdateComposition {
            id: 42,
            preedit: "nihao".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition update failed: {error}"));
    for cursor_position in [0, 2, 5] {
        let (mut preview, _) = session
            .composition_preview_with_range(cursor_position)
            .unwrap_or_else(|error| panic!("composition preview failed: {error}"));
        let preview_range = preview
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("preview environment lookup failed: {error}"))
            .unwrap_or_else(|| panic!("preedit caret should remain inside the pending format"));
        assert_eq!((preview_range.start_utf8, preview_range.end_utf8), (0, 5));
    }

    session
        .run(Command::CommitComposition {
            id: 42,
            text: "你好".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition commit failed: {error}"));
    let committed_range = session
        .current_inline_environment()
        .unwrap_or_else(|error| panic!("committed environment lookup failed: {error}"))
        .unwrap_or_else(|| panic!("committed caret should remain inside the pending format"));
    assert_eq!(committed_range.end_utf8, "你好".len());
    session
        .run(Command::InsertText("!".to_owned()))
        .unwrap_or_else(|error| panic!("post-composition typing failed: {error}"));
    assert_eq!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("environment lookup failed: {error}"))
            .unwrap_or_else(|| panic!("typing after composition should remain formatted"))
            .end_utf8,
        "你好!".len()
    );
}

#[test]
fn formatting_one_range_keeps_unedited_adjacent_environment_ids() {
    let first_id = NodeId::generate();
    let second_id = NodeId::generate();
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![
        environment(first_id.clone(), "a", BTreeMap::new()),
        environment(second_id.clone(), "b", BTreeMap::new()),
        run("target"),
    ];
    let mut session = session_with_paragraphs(vec![paragraph]);
    let start = Pos {
        offset: 2,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    let end = Pos {
        offset: 8,
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);
    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))
        .unwrap_or_else(|error| panic!("italic range formatting failed: {error}"));

    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    let [
        InlineNode::Environment(first),
        InlineNode::Environment(second),
        _,
    ] = paragraph.inlines.as_slice()
    else {
        panic!("formatting a later range must preserve earlier environment nodes");
    };
    assert_eq!(first.id, first_id);
    assert_eq!(second.id, second_id);
}

#[test]
fn formatting_does_not_merge_environments_with_unknown_extra_fields() {
    let first_id = NodeId::generate();
    let second_id = NodeId::generate();
    let extra = BTreeMap::from([("futureHint".to_owned(), serde_json::json!({"keep": true}))]);
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![
        environment(first_id.clone(), "a", extra.clone()),
        environment(second_id.clone(), "b", extra),
    ];
    let mut session = session_with_paragraphs(vec![paragraph]);
    let start = session.selection.focus.clone();
    let end = Pos {
        offset: 2,
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);
    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))
        .unwrap_or_else(|error| panic!("italic range formatting failed: {error}"));

    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    let [
        InlineNode::Environment(first),
        InlineNode::Environment(second),
    ] = paragraph.inlines.as_slice()
    else {
        panic!("unknown environment fields must keep adjacent nodes separate");
    };
    assert_eq!(first.id, first_id);
    assert_eq!(second.id, second_id);
    assert_eq!(first.extra, second.extra);
}

#[test]
fn typing_does_not_merge_runs_with_unknown_extra_fields() {
    let extra = BTreeMap::from([("futureHint".to_owned(), serde_json::json!({"keep": true}))]);
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![
        InlineNode::Run(Run {
            text: "a".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: extra.clone(),
        }),
        InlineNode::Run(Run {
            text: "b".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra,
        }),
    ];
    let mut session = session_with_paragraphs(vec![paragraph]);
    let position = Pos {
        offset: 1,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    select(&mut session, position.clone(), position);
    session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("typing between preserved runs failed: {error}"));

    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    let [InlineNode::Run(first), InlineNode::Run(second)] = paragraph.inlines.as_slice() else {
        panic!("unknown run fields must keep adjacent runs separate");
    };
    assert_eq!(first.text, "a");
    assert_eq!(second.text, "xb");
    assert_eq!(first.extra, second.extra);
}

#[test]
fn pending_style_toggle_is_undoable_without_changing_document_generation() {
    let mut session = session_with_paragraphs(vec![Paragraph::empty(NodeId::generate())]);
    let generation = session.document_generation();
    session
        .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))
        .unwrap_or_else(|error| panic!("pending italic toggle failed: {error}"));
    assert_eq!(session.pending_inline_style.italic, Some(true));
    assert_eq!(session.document_generation(), generation);

    let effect = session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("pending italic undo failed: {error}"));
    assert!(!effect.document_changed);
    assert_eq!(session.pending_inline_style.italic, None);
    assert_eq!(session.document_generation(), generation);

    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("pending italic redo failed: {error}"));
    assert_eq!(session.pending_inline_style.italic, Some(true));
}
