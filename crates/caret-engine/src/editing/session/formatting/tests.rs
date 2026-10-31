//! 验证格式命令的用户可见范围、撤销和待输入行为。

use super::*;
use crate::editing::{Command, Direction, Selection, StoryId};
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, InlineEnvironment,
    InlineEnvironmentKind, InlineNode, NodeId, Paragraph, Run,
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

fn visible_text(nodes: &[InlineNode]) -> String {
    nodes
        .iter()
        .map(|node| match node {
            InlineNode::Run(run) => run.text.clone(),
            InlineNode::Math(_) => "\u{fffc}".to_owned(),
            InlineNode::Environment(environment) => visible_text(&environment.children),
            InlineNode::SoftBreak(_) | InlineNode::Opaque(_) => String::new(),
        })
        .collect()
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

fn inline_environment(
    id: NodeId,
    kind: InlineEnvironmentKind,
    children: Vec<InlineNode>,
) -> InlineNode {
    InlineNode::Environment(InlineEnvironment {
        id,
        kind,
        definition_id: None,
        props: BTreeMap::from([("enabled".to_owned(), Value::Bool(true))]),
        children,
        extra: BTreeMap::new(),
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

fn assert_all_underline(session: &Session, paragraph_id: &NodeId, expected: bool) {
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
    let text_spans = spans
        .iter()
        .filter_map(|span| span.underline)
        .collect::<Vec<_>>();
    assert!(!text_spans.is_empty());
    assert!(text_spans.iter().all(|value| *value == expected));
    assert!(paragraph.inlines.iter().all(|node| {
        match node {
            InlineNode::Run(run) => {
                run.props
                    .get("underline")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    == expected
            }
            InlineNode::Environment(environment) => {
                environment.children.iter().all(|child| match child {
                    InlineNode::Run(run) => {
                        run.props
                            .get("underline")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                            == expected
                    }
                    _ => true,
                })
            }
            _ => true,
        }
    }));
}

#[test]
fn underline_toggle_ignores_soft_breaks_when_deciding_mixed_selection_state() {
    let mut model = paragraph("a");
    let InlineNode::Run(first_run) = &mut model.inlines[0] else {
        unreachable!();
    };
    first_run
        .props
        .insert("underline".to_owned(), Value::Bool(true));
    model
        .inlines
        .push(InlineNode::SoftBreak(caret_doc::SoftBreak::default()));
    let mut second = run("b");
    let InlineNode::Run(second_run) = &mut second else {
        unreachable!();
    };
    second_run
        .props
        .insert("underline".to_owned(), Value::Bool(true));
    model.inlines.push(second);
    let paragraph_id = model.id.clone();
    let mut session = session_with_paragraphs(vec![model]);
    let start = Pos {
        offset: 0,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    let end = Pos {
        paragraph_id: paragraph_id.clone(),
        offset: 3,
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);

    session
        .run(Command::ToggleRunUnderline)
        .unwrap_or_else(|error| panic!("underline removal across soft break failed: {error}"));
    assert_all_underline(&session, &paragraph_id, false);
    let paragraph = session
        .paragraph_for(&Pos {
            paragraph_id: paragraph_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
            ..session.selection.focus.clone()
        })
        .unwrap_or_else(|error| panic!("formatted paragraph lookup failed: {error}"));
    assert!(matches!(
        paragraph.inlines.get(1),
        Some(InlineNode::SoftBreak(_))
    ));
}

#[test]
fn removing_run_underline_accepts_a_boundary_inside_a_cross_run_grapheme() {
    let mut model = paragraph("");
    model.inlines = vec![
        InlineNode::Run(Run {
            text: "a".into(),
            language: None,
            props: BTreeMap::from([("underline".into(), Value::Bool(true))]),
            extra: BTreeMap::new(),
        }),
        run("\u{0301}"),
    ];
    let paragraph_id = model.id.clone();
    let mut session = session_with_paragraphs(vec![model]);
    let mut focus = session.selection().focus.clone();
    focus.paragraph_id = paragraph_id;
    focus.offset = 0;
    focus.affinity = Affinity::Downstream;
    select(&mut session, focus.clone(), focus);

    session
        .run(Command::RemoveRunUnderlinePreservingContent)
        .unwrap_or_else(|error| panic!("underline removal should accept a Run boundary: {error}"));
    let paragraph = session
        .paragraph_for(&session.selection().focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    assert_eq!(visible_text(&paragraph.inlines), "a\u{0301}");
    let InlineNode::Run(first) = &paragraph.inlines[0] else {
        panic!("underlined content should remain a Run");
    };
    assert_ne!(first.props.get("underline"), Some(&Value::Bool(true)));

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("underline removal should undo: {error}"));
    let paragraph = session
        .paragraph_for(&session.selection().focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed after undo: {error}"));
    let InlineNode::Run(first) = &paragraph.inlines[0] else {
        panic!("undo should retain the Run");
    };
    assert_eq!(first.props.get("underline"), Some(&Value::Bool(true)));
    assert_eq!(visible_text(&paragraph.inlines), "a\u{0301}");
}

#[test]
fn underline_command_applies_to_selected_runs_and_undo_restores_direct_format() {
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
        .run(Command::ToggleRunUnderline)
        .unwrap_or_else(|error| panic!("underline command failed: {error}"));
    assert!(effect.document_changed);
    assert_eq!(
        session
            .selected_text()
            .unwrap_or_else(|error| panic!("selected text query failed: {error}")),
        "alpha"
    );
    assert_all_underline(&session, &paragraph_id, true);

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    assert_all_underline(&session, &paragraph_id, false);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo failed: {error}"));
    assert_all_underline(&session, &paragraph_id, true);
}

#[test]
fn underline_command_unifies_mixed_selection_and_supports_pending_typing() {
    let mut first = paragraph("on");
    let InlineNode::Run(run) = &mut first.inlines[0] else {
        unreachable!();
    };
    run.props.insert("underline".to_owned(), Value::Bool(true));
    let first_id = first.id.clone();
    let second = paragraph("off");
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
        .run(Command::ToggleRunUnderline)
        .unwrap_or_else(|error| panic!("mixed underline command failed: {error}"));
    assert_all_underline(&session, &first_id, true);
    assert_all_underline(&session, &second_id, true);

    let mut session = session_with_paragraphs(vec![Paragraph::empty(NodeId::generate())]);
    session
        .run(Command::ToggleRunUnderline)
        .unwrap_or_else(|error| panic!("pending underline command failed: {error}"));
    session
        .run(Command::InsertText("typed".to_owned()))
        .unwrap_or_else(|error| panic!("typing with pending underline failed: {error}"));
    assert_all_underline(&session, &session.selection.focus.paragraph_id, true);
}

#[test]
fn pending_underline_survives_undo_redo_and_ime_commit_without_mutating_during_preedit() {
    let mut session = session_with_paragraphs(vec![Paragraph::empty(NodeId::generate())]);
    let paragraph_id = session.selection.focus.paragraph_id.clone();
    session
        .run(Command::ToggleRunUnderline)
        .unwrap_or_else(|error| panic!("pending underline toggle failed: {error}"));
    assert_eq!(session.pending_inline_style.underline(), Some(true));

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("pending underline undo failed: {error}"));
    assert_eq!(session.pending_inline_style.underline(), None);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("pending underline redo failed: {error}"));
    assert_eq!(session.pending_inline_style.underline(), Some(true));

    session
        .run(Command::UpdateComposition {
            id: 81,
            preedit: "ni".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition update failed: {error}"));
    assert_eq!(
        session.run(Command::ToggleRunUnderline),
        Err(EditError::CompositionPending)
    );
    assert_eq!(session.pending_inline_style.underline(), Some(true));

    let (preview, _) = session
        .composition_preview_with_range(2)
        .unwrap_or_else(|error| panic!("underline composition preview failed: {error}"));
    let preview_paragraph = preview
        .paragraph_for(&preview.selection.focus)
        .unwrap_or_else(|error| panic!("preview paragraph lookup failed: {error}"));
    assert!(
        format_spans(&preview_paragraph.inlines)
            .unwrap_or_else(|error| panic!("preview formatting query failed: {error}"))
            .iter()
            .any(|span| span.underline == Some(true))
    );

    session
        .run(Command::CommitComposition {
            id: 81,
            text: "你".to_owned(),
        })
        .unwrap_or_else(|error| panic!("underline composition commit failed: {error}"));
    assert_all_underline(&session, &paragraph_id, true);
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
fn mod_backspace_unwraps_nested_inline_environments_one_layer_at_a_time() {
    let italic_id = NodeId::generate();
    let bold_id = NodeId::generate();
    let model = Paragraph {
        inlines: vec![inline_environment(
            italic_id,
            InlineEnvironmentKind::Italic,
            vec![inline_environment(
                bold_id,
                InlineEnvironmentKind::Bold,
                vec![run("text")],
            )],
        )],
        ..Paragraph::empty(NodeId::generate())
    };
    let paragraph_id = model.id.clone();
    let mut session = session_with_paragraphs(vec![model]);
    let original_document = session.document().clone();
    let mut caret = session.selection.focus.clone();
    caret.offset = 2;
    select(&mut session, caret.clone(), caret);

    session
        .run(Command::EditorKey(crate::editing::EditorKey::ModBackspace))
        .unwrap_or_else(|error| panic!("first inline environment removal failed: {error}"));
    assert_eq!(
        session
            .selected_text()
            .unwrap_or_else(|error| panic!("visible text query failed: {error}")),
        ""
    );
    let position = Pos {
        paragraph_id: paragraph_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    let current = session
        .paragraph_for(&position)
        .unwrap_or_else(|error| panic!("paragraph query failed: {error}"));
    assert_eq!(visible_text(&current.inlines), "text");
    assert_eq!(
        format_spans(&current.inlines)
            .unwrap_or_else(|error| panic!("format query failed: {error}"))
            .iter()
            .map(|span| (format_value(span, InlineEnvironmentKind::Bold), span.italic))
            .collect::<Vec<_>>(),
        vec![(false, true)]
    );
    let first_unwrapped_document = session.document().clone();

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo inline environment removal failed: {error}"));
    assert_eq!(session.document(), &original_document);
    let restored = session
        .paragraph_for(&position)
        .unwrap_or_else(|error| panic!("restored paragraph query failed: {error}"));
    let restored_spans = format_spans(&restored.inlines)
        .unwrap_or_else(|error| panic!("restored format query failed: {error}"));
    assert_eq!(restored_spans.len(), 1);
    assert!(format_value(
        &restored_spans[0],
        InlineEnvironmentKind::Bold
    ));
    assert!(restored_spans[0].italic);

    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo inline environment removal failed: {error}"));
    assert_eq!(session.document(), &first_unwrapped_document);
    session
        .run(Command::EditorKey(crate::editing::EditorKey::ModBackspace))
        .unwrap_or_else(|error| panic!("outer inline environment removal failed: {error}"));
    let fully_unwrapped_document = session.document().clone();
    let unformatted = session
        .paragraph_for(&position)
        .unwrap_or_else(|error| panic!("unformatted paragraph query failed: {error}"));
    assert_eq!(visible_text(&unformatted.inlines), "text");
    assert_all_format(&session, &paragraph_id, InlineEnvironmentKind::Bold, false);
    assert_eq!(
        format_spans(&unformatted.inlines)
            .unwrap_or_else(|error| panic!("unformatted format query failed: {error}"))
            .iter()
            .map(|span| span.italic)
            .collect::<Vec<_>>(),
        vec![false]
    );
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo outer inline environment removal failed: {error}"));
    assert_eq!(session.document(), &first_unwrapped_document);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo outer inline environment removal failed: {error}"));
    assert_eq!(session.document(), &fully_unwrapped_document);
}

#[test]
fn mod_backspace_unwraps_environment_before_clearing_nested_run_underline() {
    let mut child = run("text");
    let InlineNode::Run(run) = &mut child else {
        unreachable!();
    };
    run.props.insert("underline".to_owned(), Value::Bool(true));
    let paragraph_model = Paragraph {
        inlines: vec![inline_environment(
            NodeId::generate(),
            InlineEnvironmentKind::Bold,
            vec![child],
        )],
        ..Paragraph::empty(NodeId::generate())
    };
    let paragraph_id = paragraph_model.id.clone();
    let mut session = session_with_paragraphs(vec![paragraph_model]);
    let original = session.document().clone();
    let caret = Pos {
        offset: 2,
        ..session.selection.focus.clone()
    };
    select(&mut session, caret.clone(), caret);

    session
        .run(Command::EditorKey(crate::editing::EditorKey::ModBackspace))
        .unwrap_or_else(|error| panic!("environment unwrap failed: {error}"));
    let position = Pos {
        paragraph_id: paragraph_id.clone(),
        offset: 2,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    let paragraph = session
        .paragraph_for(&position)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    assert!(
        matches!(paragraph.inlines.as_slice(), [InlineNode::Run(run)]
        if run.text == "text"
            && run.props.get("underline") == Some(&Value::Bool(true)))
    );
    let unwrapped = session.document().clone();

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("environment unwrap undo failed: {error}"));
    assert_eq!(session.document(), &original);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("environment unwrap redo failed: {error}"));
    assert_eq!(session.document(), &unwrapped);

    session
        .run(Command::EditorKey(crate::editing::EditorKey::ModBackspace))
        .unwrap_or_else(|error| panic!("direct underline removal failed: {error}"));
    let paragraph = session
        .paragraph_for(&position)
        .unwrap_or_else(|error| panic!("paragraph lookup after direct formatting failed: {error}"));
    assert!(
        matches!(paragraph.inlines.as_slice(), [InlineNode::Run(run)]
        if run.text == "text"
            && run.props.get("underline") == Some(&Value::Bool(false)))
    );
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("direct underline undo failed: {error}"));
    assert_eq!(session.document(), &unwrapped);
}

#[test]
fn navigation_ignores_direct_run_boundaries_inside_active_environment() {
    let mut underlined = run("bc");
    let InlineNode::Run(run_node) = &mut underlined else {
        unreachable!();
    };
    run_node
        .props
        .insert("underline".to_owned(), Value::Bool(true));
    let paragraph_model = Paragraph {
        inlines: vec![inline_environment(
            NodeId::generate(),
            InlineEnvironmentKind::Bold,
            vec![run("a"), underlined, run("d")],
        )],
        ..Paragraph::empty(NodeId::generate())
    };
    let mut session = session_with_paragraphs(vec![paragraph_model]);
    let mut caret = session.selection.focus.clone();
    caret.offset = 0;
    select(&mut session, caret.clone(), caret);

    session
        .run(Command::MoveCaret(crate::editing::Direction::Forward))
        .unwrap_or_else(|error| panic!("navigation into nested run failed: {error}"));
    assert_eq!(session.selection.focus.offset, 1);
    assert_eq!(session.selection.focus.affinity, Affinity::Downstream);
    assert!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("environment query failed: {error}"))
            .is_some()
    );
}

#[test]
fn mod_backspace_does_not_unwrap_inline_environment_during_ime_or_command_input() {
    let original = Paragraph {
        inlines: vec![inline_environment(
            NodeId::generate(),
            InlineEnvironmentKind::Bold,
            vec![run("plain")],
        )],
        ..Paragraph::empty(NodeId::generate())
    };
    let original_id = original.id.clone();
    let mut session = session_with_paragraphs(vec![original]);
    let before = session.document().clone();

    session
        .run(Command::EditorKey(crate::editing::EditorKey::ModBackspace))
        .unwrap_or_else(|error| panic!("inline environment Mod-Backspace failed: {error}"));
    assert_ne!(session.document(), &before);
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("inline environment restoration failed: {error}"));
    assert_eq!(session.document(), &before);

    session
        .run(Command::UpdateComposition {
            id: 91,
            preedit: "preedit".to_owned(),
        })
        .unwrap_or_else(|error| panic!("IME preedit setup failed: {error}"));
    session
        .run(Command::EditorKey(crate::editing::EditorKey::ModBackspace))
        .unwrap_or_else(|error| panic!("IME Mod-Backspace failed: {error}"));
    assert_eq!(session.document(), &before);
    assert!(session.composition().is_some());

    session
        .run(Command::CancelComposition { id: 91 })
        .unwrap_or_else(|error| panic!("IME cancellation failed: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command input setup failed: {error}"));
    session
        .run(Command::EditorKey(crate::editing::EditorKey::ModBackspace))
        .unwrap_or_else(|error| panic!("command input Mod-Backspace failed: {error}"));
    assert_eq!(session.document(), &before);
    assert!(session.command_line().is_some());

    let position = Pos {
        paragraph_id: original_id,
        offset: 0,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    assert!(session.paragraph_for(&position).is_ok());
}

#[test]
fn mod_backspace_removes_direct_underline_and_undo_restores_it() {
    let model = Paragraph {
        inlines: vec![InlineNode::Run(Run {
            text: "underlined".to_owned(),
            language: None,
            props: BTreeMap::from([("underline".to_owned(), Value::Bool(true))]),
            extra: BTreeMap::new(),
        })],
        ..Paragraph::empty(NodeId::generate())
    };
    let paragraph_id = model.id.clone();
    let mut session = session_with_paragraphs(vec![model]);
    let mut caret = session.selection.focus.clone();
    caret.offset = 4;
    select(&mut session, caret.clone(), caret);
    let original = session.document().clone();

    let effect = session
        .run(Command::EditorKey(crate::editing::EditorKey::ModBackspace))
        .unwrap_or_else(|error| panic!("removing direct underline failed: {error}"));
    assert!(effect.document_changed);
    let position = Pos {
        paragraph_id: paragraph_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    let cleared = session
        .paragraph_for(&position)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    assert_eq!(visible_text(&cleared.inlines), "underlined");
    assert!(
        format_spans(&cleared.inlines)
            .unwrap_or_else(|error| panic!("format query failed: {error}"))
            .iter()
            .all(|span| span.underline == Some(false))
    );

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo direct underline removal failed: {error}"));
    assert_eq!(session.document(), &original);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo direct underline removal failed: {error}"));
    let redone = session
        .paragraph_for(&position)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    assert!(
        format_spans(&redone.inlines)
            .unwrap_or_else(|error| panic!("format query failed: {error}"))
            .iter()
            .all(|span| span.underline == Some(false))
    );
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

fn collect_font_segments(nodes: &[InlineNode]) -> Vec<(String, Option<String>)> {
    fn collect(
        nodes: &[InlineNode],
        inherited: Option<String>,
        segments: &mut Vec<(String, Option<String>)>,
    ) {
        for node in nodes {
            match node {
                InlineNode::Run(run) => segments.push((run.text.clone(), inherited.clone())),
                InlineNode::Math(_) => segments.push(("\u{fffc}".to_owned(), inherited.clone())),
                InlineNode::SoftBreak(_) => {}
                InlineNode::Opaque(_) => panic!("font formatting must preserve known content"),
                InlineNode::Environment(environment) => {
                    let family = if environment.kind == InlineEnvironmentKind::Font {
                        environment
                            .props
                            .get("fontFamily")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                            .or_else(|| inherited.clone())
                    } else {
                        inherited.clone()
                    };
                    collect(&environment.children, family, segments);
                }
            }
        }
    }

    let mut segments = Vec::new();
    collect(nodes, None, &mut segments);
    segments
}

fn font_environment(text: &str, family: &str, extra: BTreeMap<String, Value>) -> InlineNode {
    InlineNode::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Font,
        definition_id: None,
        props: BTreeMap::from([("fontFamily".to_owned(), Value::String(family.to_owned()))]),
        children: vec![run(text)],
        extra,
    })
}

#[test]
fn selected_text_gets_a_local_font_environment_without_changing_other_text() {
    let mut session = session_with_paragraphs(vec![paragraph("before target after")]);
    let start = Pos {
        offset: 7,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    let end = Pos {
        offset: 13,
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);

    let effect = session
        .run(Command::SetInlineFontFamily("Noto Serif".to_owned()))
        .unwrap_or_else(|error| panic!("font formatting failed: {error}"));

    assert!(effect.document_changed);
    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    assert_eq!(
        collect_font_segments(&paragraph.inlines),
        vec![
            ("before ".to_owned(), None),
            ("target".to_owned(), Some("Noto Serif".to_owned())),
            (" after".to_owned(), None),
        ]
    );
    assert_eq!(
        session
            .selected_text()
            .unwrap_or_else(|error| panic!("selected text query failed: {error}")),
        "target"
    );
    let serialized = serde_json::to_value(session.document())
        .unwrap_or_else(|error| panic!("document serialization failed: {error}"));
    let reopened: Document = serde_json::from_value(serialized)
        .unwrap_or_else(|error| panic!("document deserialization failed: {error}"));
    assert_eq!(
        collect_font_segments(
            reopened
                .body
                .iter()
                .find_map(|block| match block {
                    Block::Paragraph(paragraph) => Some(paragraph.inlines.as_slice()),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("serialized document lost its paragraph")),
        ),
        vec![
            ("before ".to_owned(), None),
            ("target".to_owned(), Some("Noto Serif".to_owned())),
            (" after".to_owned(), None),
        ]
    );
}

#[test]
fn nested_font_environment_overrides_only_its_selected_range_and_undo_restores_it() {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![font_environment("abcdef", "Outer Family", BTreeMap::new())];
    let mut session = session_with_paragraphs(vec![paragraph]);
    let start = Pos {
        offset: 2,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    let end = Pos {
        offset: 4,
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);

    session
        .run(Command::SetInlineFontFamily("Inner Family".to_owned()))
        .unwrap_or_else(|error| panic!("nested font formatting failed: {error}"));
    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    assert_eq!(
        collect_font_segments(&paragraph.inlines),
        vec![
            ("ab".to_owned(), Some("Outer Family".to_owned())),
            ("cd".to_owned(), Some("Inner Family".to_owned())),
            ("ef".to_owned(), Some("Outer Family".to_owned())),
        ]
    );

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    assert_eq!(
        collect_font_segments(&paragraph.inlines),
        vec![("abcdef".to_owned(), Some("Outer Family".to_owned()))]
    );
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo failed: {error}"));
    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    assert_eq!(
        collect_font_segments(&paragraph.inlines),
        vec![
            ("ab".to_owned(), Some("Outer Family".to_owned())),
            ("cd".to_owned(), Some("Inner Family".to_owned())),
            ("ef".to_owned(), Some("Outer Family".to_owned())),
        ]
    );
}

#[test]
fn cross_paragraph_font_format_preserves_selection_and_is_one_undoable_change() {
    let first = paragraph("alpha");
    let second = paragraph("beta");
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    let mut session = session_with_paragraphs(vec![first, second]);
    let before = session.document().clone();
    let anchor = Pos {
        paragraph_id: first_id,
        offset: 2,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    let focus = Pos {
        paragraph_id: second_id,
        offset: 2,
        affinity: Affinity::Upstream,
        ..anchor.clone()
    };
    select(&mut session, focus, anchor);

    let effect = session
        .run(Command::SetInlineFontFamily("Noto Serif".to_owned()))
        .unwrap_or_else(|error| panic!("cross-paragraph font formatting failed: {error}"));
    assert!(effect.document_changed);
    assert_eq!(
        session
            .selected_text()
            .unwrap_or_else(|error| panic!("selected text query failed: {error}")),
        "pha\nbe"
    );
    let first_position = Pos {
        story: StoryId::Body,
        paragraph_id: before
            .body
            .iter()
            .find_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph.id.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("first paragraph missing")),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    assert_eq!(
        collect_font_segments(
            &session
                .paragraph_for(&first_position)
                .unwrap_or_else(|error| panic!("first paragraph lookup failed: {error}"))
                .inlines
        ),
        vec![
            ("al".to_owned(), None),
            ("pha".to_owned(), Some("Noto Serif".to_owned())),
        ]
    );
    let second_position = session
        .document()
        .body
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph.id.clone()),
            _ => None,
        })
        .nth(1)
        .map(|paragraph_id| Pos {
            story: StoryId::Body,
            paragraph_id,
            offset: 0,
            affinity: Affinity::Downstream,
        })
        .unwrap_or_else(|| panic!("second paragraph missing"));
    assert_eq!(
        collect_font_segments(
            &session
                .paragraph_for(&second_position)
                .unwrap_or_else(|error| panic!("second paragraph lookup failed: {error}"))
                .inlines
        ),
        vec![
            ("be".to_owned(), Some("Noto Serif".to_owned())),
            ("ta".to_owned(), None),
        ]
    );
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    assert_eq!(session.document(), &before);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo failed: {error}"));
    assert!(session.document() != &before);
}

#[test]
fn pending_font_environment_applies_to_consecutive_input_and_merges() {
    let mut session = session_with_paragraphs(vec![Paragraph::empty(NodeId::generate())]);
    session
        .run(Command::SetInlineFontFamily("Noto Serif".to_owned()))
        .unwrap_or_else(|error| panic!("pending font style failed: {error}"));
    for character in ["a", "b", "c"] {
        session
            .run(Command::InsertText(character.to_owned()))
            .unwrap_or_else(|error| panic!("font-styled input failed: {error}"));
    }

    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    let [InlineNode::Environment(font)] = paragraph.inlines.as_slice() else {
        panic!(
            "consecutive font-styled input should share one environment: {:#?}",
            paragraph.inlines
        );
    };
    assert_eq!(font.kind, InlineEnvironmentKind::Font);
    assert_eq!(
        font.props.get("fontFamily").and_then(Value::as_str),
        Some("Noto Serif")
    );
    assert_eq!(
        collect_font_segments(&paragraph.inlines),
        vec![("abc".to_owned(), Some("Noto Serif".to_owned()))]
    );
}

#[test]
fn font_command_treats_same_offset_with_different_affinity_as_a_caret() {
    let mut session = session_with_paragraphs(vec![Paragraph::empty(NodeId::generate())]);
    let mut anchor = session.selection.focus.clone();
    anchor.affinity = Affinity::Upstream;
    let mut focus = anchor.clone();
    focus.affinity = Affinity::Downstream;
    session
        .run(Command::SetSelection(Some(Selection { anchor, focus })))
        .unwrap_or_else(|error| panic!("collapsed selection setup failed: {error}"));

    session
        .run(Command::SetInlineFontFamily("Noto Serif".to_owned()))
        .unwrap_or_else(|error| panic!("pending font style failed: {error}"));
    session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("font-styled input failed: {error}"));

    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    assert_eq!(
        collect_font_segments(&paragraph.inlines),
        vec![("x".to_owned(), Some("Noto Serif".to_owned()))]
    );
}

#[test]
fn pending_font_environment_is_used_by_ime_preview_and_commit() {
    let mut session = session_with_paragraphs(vec![Paragraph::empty(NodeId::generate())]);
    session
        .run(Command::SetInlineFontFamily("Noto Serif CJK".to_owned()))
        .unwrap_or_else(|error| panic!("pending font style failed: {error}"));
    session
        .run(Command::UpdateComposition {
            id: 73,
            preedit: "你好".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition update failed: {error}"));

    let (preview, _) = session
        .composition_preview_with_range("你好".len())
        .unwrap_or_else(|error| panic!("composition preview failed: {error}"));
    let preview_paragraph = preview
        .paragraph_for(&preview.selection.focus)
        .unwrap_or_else(|error| panic!("preview paragraph lookup failed: {error}"));
    assert_eq!(
        collect_font_segments(&preview_paragraph.inlines),
        vec![("你好".to_owned(), Some("Noto Serif CJK".to_owned()))]
    );

    session
        .run(Command::CommitComposition {
            id: 73,
            text: "你好".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition commit failed: {error}"));
    session
        .run(Command::InsertText("世界".to_owned()))
        .unwrap_or_else(|error| panic!("typing after composition failed: {error}"));
    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    assert_eq!(
        collect_font_segments(&paragraph.inlines),
        vec![("你好世界".to_owned(), Some("Noto Serif CJK".to_owned()))]
    );
}

#[test]
fn document_default_font_setting_is_undoable_and_blank_values_are_rejected() {
    let mut session = session_with_paragraphs(vec![Paragraph::empty(NodeId::generate())]);
    let generation = session.document_generation();
    let effect = session
        .run(Command::SetDefaultFontFamily(Some("Noto Serif".to_owned())))
        .unwrap_or_else(|error| panic!("default font update failed: {error}"));
    assert!(effect.document_changed);
    assert_eq!(
        session
            .document()
            .global_settings
            .default_font_family
            .as_deref(),
        Some("Noto Serif")
    );
    assert!(session.document_generation() > generation);

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    assert_eq!(session.document().global_settings.default_font_family, None);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo failed: {error}"));
    assert_eq!(
        session
            .document()
            .global_settings
            .default_font_family
            .as_deref(),
        Some("Noto Serif")
    );

    let before = session.document().clone();
    assert!(matches!(
        session.run(Command::SetDefaultFontFamily(Some("  ".to_owned()))),
        Err(EditError::InvalidFontFamily)
    ));
    assert_eq!(session.document(), &before);
}

#[test]
fn invalid_document_font_language_target_is_rejected_without_mutation() {
    let mut session = session_with_paragraphs(vec![Paragraph::empty(NodeId::generate())]);
    let before = session.document().clone();
    assert!(matches!(
        session.run(Command::SetDocumentFontFamily {
            target: caret_doc::DocumentFontTarget::ScriptLanguage {
                script: "Latn".to_owned(),
                language: "en-a".to_owned(),
            },
            family: Some("Example Font".to_owned()),
        }),
        Err(EditError::InvalidFontFamily)
    ));
    assert_eq!(session.document(), &before);
}

#[test]
fn latin_font_update_and_undo_preserve_legacy_script_override_behavior() {
    let mut document = Document::blank();
    document.global_settings.default_font_family = Some("Document Default".to_owned());
    document.global_settings.script_font_families =
        BTreeMap::from([("Latn".to_owned(), "Legacy Latin".to_owned())]);
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("valid legacy font document rejected: {error}"));
    let target = caret_doc::DocumentFontTarget::Script("latn".to_owned());

    assert!(
        session
            .run(Command::SetDocumentFontFamily {
                target: target.clone(),
                family: Some("Updated Latin".to_owned()),
            })
            .is_ok()
    );
    assert_eq!(
        session.document().global_settings.font_family(&target),
        Some("Updated Latin")
    );
    assert!(session.run(Command::Undo).is_ok());
    assert_eq!(
        session.document().global_settings.font_family(&target),
        Some("Legacy Latin")
    );
    assert!(session.document().validate().is_ok());
}

#[test]
fn clearing_legacy_latin_font_restores_both_fields_through_undo_and_redo() {
    let mut document = Document::blank();
    document.global_settings.default_font_family = Some("Document Default".to_owned());
    document.global_settings.script_font_families =
        BTreeMap::from([("Latn".to_owned(), "Legacy Latin".to_owned())]);
    let target = caret_doc::DocumentFontTarget::Script("Latn".to_owned());
    let mut session = Session::new(document.clone())
        .unwrap_or_else(|error| panic!("valid legacy font document rejected: {error}"));

    assert!(
        session
            .run(Command::SetDocumentFontFamily {
                target: target.clone(),
                family: None,
            })
            .is_ok()
    );
    assert_eq!(
        session.document().global_settings.font_family(&target),
        None
    );
    assert!(session.run(Command::Undo).is_ok());
    assert_eq!(
        session
            .document()
            .global_settings
            .default_font_family
            .as_deref(),
        Some("Document Default")
    );
    assert_eq!(
        session.document().global_settings.script_font_families["Latn"],
        "Legacy Latin"
    );
    assert!(session.run(Command::Redo).is_ok());
    assert_eq!(
        session.document().global_settings.font_family(&target),
        None
    );

    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("valid legacy font document rejected: {error}"));
    assert!(
        session
            .run(Command::SetDefaultFontFamily(Some(
                "New Default".to_owned()
            )))
            .is_ok()
    );
    assert_eq!(
        session.document().global_settings.font_family(&target),
        Some("New Default")
    );
    assert!(
        session
            .document()
            .global_settings
            .script_font_families
            .is_empty()
    );
    assert!(session.run(Command::Undo).is_ok());
    assert_eq!(
        session
            .document()
            .global_settings
            .default_font_family
            .as_deref(),
        Some("Document Default")
    );
    assert_eq!(
        session.document().global_settings.script_font_families["Latn"],
        "Legacy Latin"
    );
    assert!(session.run(Command::Redo).is_ok());
    assert_eq!(
        session.document().global_settings.font_family(&target),
        Some("New Default")
    );
}

#[test]
fn font_environment_rejects_nesting_overflow_without_mutating_the_document() {
    fn nested_custom_environment(remaining: usize, child: Block) -> Block {
        if remaining == 0 {
            return child;
        }
        Block::Environment(BlockEnvironment {
            id: NodeId::generate(),
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children: vec![nested_custom_environment(remaining - 1, child)],
            extra: BTreeMap::new(),
        })
    }

    let mut document = Document::blank();
    let paragraph = paragraph("x");
    let paragraph_id = paragraph.id.clone();
    document.body = vec![nested_custom_environment(63, Block::Paragraph(paragraph))];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("valid deep document rejected: {error}"));
    let start = session.selection.focus.clone();
    let end = Pos {
        offset: 1,
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);
    let before = session.document().clone();
    let generation = session.document_generation();

    assert!(matches!(
        session.run(Command::SetInlineFontFamily("Noto Serif".to_owned())),
        Err(EditError::NestingLimit)
    ));
    assert_eq!(session.document(), &before);
    assert_eq!(session.document_generation(), generation);
    assert_eq!(session.selection.focus.paragraph_id, paragraph_id);
}

#[test]
fn inline_format_rejects_nesting_overflow_without_mutating_the_document() {
    fn nested_custom_environment(remaining: usize, child: Block) -> Block {
        if remaining == 0 {
            return child;
        }
        Block::Environment(BlockEnvironment {
            id: NodeId::generate(),
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children: vec![nested_custom_environment(remaining - 1, child)],
            extra: BTreeMap::new(),
        })
    }

    let mut document = Document::blank();
    let paragraph = paragraph("x");
    let paragraph_id = paragraph.id.clone();
    document.body = vec![nested_custom_environment(63, Block::Paragraph(paragraph))];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("valid deep document rejected: {error}"));
    let start = session.selection.focus.clone();
    let end = Pos {
        offset: 1,
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);
    let before = session.document().clone();
    let generation = session.document_generation();

    assert!(matches!(
        session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold)),
        Err(EditError::NestingLimit)
    ));
    assert_eq!(session.document(), &before);
    assert_eq!(session.document_generation(), generation);
    assert_eq!(session.selection.focus.paragraph_id, paragraph_id);
}

#[test]
fn toggling_inline_format_updates_the_selected_environment_without_growing_depth() {
    let mut session = session_with_paragraphs(vec![paragraph("x")]);
    let start = session.selection.focus.clone();
    let end = Pos {
        offset: 1,
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);

    fn environment_count(nodes: &[InlineNode]) -> usize {
        nodes
            .iter()
            .map(|node| match node {
                InlineNode::Environment(environment) => {
                    1 + environment_count(&environment.children)
                }
                InlineNode::Run(_)
                | InlineNode::Math(_)
                | InlineNode::SoftBreak(_)
                | InlineNode::Opaque(_) => 0,
            })
            .sum()
    }

    for index in 0..128 {
        session
            .run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))
            .unwrap_or_else(|error| panic!("bold toggle failed: {error}"));
        session
            .document()
            .validate()
            .unwrap_or_else(|error| panic!("toggle produced invalid document: {error}"));
        let paragraph = session
            .paragraph_for(&session.selection.focus)
            .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
        assert_eq!(environment_count(&paragraph.inlines), 1);
        assert_all_format(
            &session,
            &session.selection.focus.paragraph_id,
            InlineEnvironmentKind::Bold,
            index % 2 == 0,
        );
    }
}

#[test]
fn cross_paragraph_inline_depth_failure_does_not_commit_earlier_paragraphs() {
    fn nested_custom_environment(remaining: usize, child: Block) -> Block {
        if remaining == 0 {
            return child;
        }
        Block::Environment(BlockEnvironment {
            id: NodeId::generate(),
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children: vec![nested_custom_environment(remaining - 1, child)],
            extra: BTreeMap::new(),
        })
    }

    let first = paragraph("first");
    let first_id = first.id.clone();
    let second = paragraph("second");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(first),
        nested_custom_environment(63, Block::Paragraph(second)),
    ];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("valid deep document rejected: {error}"));
    session
        .run(Command::InsertText("!".to_owned()))
        .unwrap_or_else(|error| panic!("history setup edit failed: {error}"));

    let anchor = Pos {
        story: StoryId::Body,
        paragraph_id: first_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let focus = Pos {
        story: StoryId::Body,
        paragraph_id: match &session.document().body[1] {
            Block::Environment(environment) => {
                let mut node = environment;
                loop {
                    match node.children.first() {
                        Some(Block::Environment(child)) => node = child,
                        Some(Block::Paragraph(paragraph)) => break paragraph.id.clone(),
                        _ => panic!("expected the nested paragraph"),
                    }
                }
            }
            _ => panic!("expected the nested environment"),
        },
        offset: 6,
        affinity: Affinity::Upstream,
    };
    select(&mut session, anchor, focus);
    let before = session.document().clone();
    let before_generation = session.document_generation();
    let before_state_id = session.history_state_id();

    assert!(matches!(
        session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold)),
        Err(EditError::NestingLimit)
    ));
    assert_eq!(session.document(), &before);
    assert_eq!(session.document_generation(), before_generation);
    assert_eq!(session.history_state_id(), before_state_id);

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo after rejected format failed: {error}"));
    let first_paragraph = match &session.document().body[0] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("expected the first paragraph"),
    };
    assert_eq!(visible_text(&first_paragraph.inlines), "first");
}

#[test]
fn cross_paragraph_font_depth_failure_does_not_partially_change_earlier_paragraphs() {
    fn nested_custom_environment(remaining: usize, child: Block) -> Block {
        if remaining == 0 {
            return child;
        }
        Block::Environment(BlockEnvironment {
            id: NodeId::generate(),
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children: vec![nested_custom_environment(remaining - 1, child)],
            extra: BTreeMap::new(),
        })
    }

    let first = paragraph("first");
    let first_id = first.id.clone();
    let second = paragraph("x");
    let second_id = second.id.clone();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(first),
        nested_custom_environment(63, Block::Paragraph(second)),
    ];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("valid deep document rejected: {error}"));
    let start = Pos {
        paragraph_id: first_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
        ..session.selection.focus.clone()
    };
    let end = Pos {
        paragraph_id: second_id,
        offset: 1,
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    select(&mut session, start, end);
    let before = session.document().clone();
    let generation = session.document_generation();

    assert!(matches!(
        session.run(Command::SetInlineFontFamily("Noto Serif".to_owned())),
        Err(EditError::NestingLimit)
    ));
    assert_eq!(session.document(), &before);
    assert_eq!(session.document_generation(), generation);
    let first_position = Pos {
        story: StoryId::Body,
        paragraph_id: first_id,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let first_paragraph = session
        .paragraph_for(&first_position)
        .unwrap_or_else(|error| panic!("first paragraph lookup failed: {error}"));
    assert_eq!(
        collect_font_segments(&first_paragraph.inlines),
        vec![("first".to_owned(), None)]
    );
}

#[test]
fn font_family_updates_do_not_merge_environments_with_unknown_extra_fields() {
    let extra = BTreeMap::from([("futureHint".to_owned(), serde_json::json!({"keep": true}))]);
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![
        font_environment("a", "Noto Serif", extra.clone()),
        font_environment("b", "Noto Serif", extra),
    ];
    let mut session = session_with_paragraphs(vec![paragraph]);
    let end = Pos {
        offset: 2,
        affinity: Affinity::Upstream,
        ..session.selection.focus.clone()
    };
    let start = session.selection.focus.clone();
    select(&mut session, start, end);
    let effect = session
        .run(Command::SetInlineFontFamily("Noto Serif".to_owned()))
        .unwrap_or_else(|error| panic!("same font application failed: {error}"));
    assert!(!effect.document_changed);

    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    let [
        InlineNode::Environment(first),
        InlineNode::Environment(second),
    ] = paragraph.inlines.as_slice()
    else {
        panic!("unknown extra fields must keep adjacent environments distinct");
    };
    assert_eq!(
        first.extra.get("futureHint"),
        Some(&serde_json::json!({"keep": true}))
    );
    assert_eq!(
        second.extra.get("futureHint"),
        Some(&serde_json::json!({"keep": true}))
    );
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
