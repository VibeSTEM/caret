//! 验证编辑会话缓存与修订代际。

use super::*;
use crate::editing::EditorKey;
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, InlineEnvironment,
    InlineEnvironmentKind, InlineNode, NodeId, Paragraph, Run,
};
use std::collections::BTreeMap;
use unicode_segmentation::UnicodeSegmentation;

fn paragraph_text(session: &Session, block_index: usize) -> String {
    let Some(Block::Paragraph(paragraph)) = session.document.body.get(block_index) else {
        panic!("expected paragraph at block index {block_index}");
    };
    TextMap::new(paragraph)
        .unwrap_or_else(|error| panic!("paragraph text map failed: {error}"))
        .text
}

#[test]
fn current_inline_environment_respects_nested_scope_and_boundary_affinity() {
    let mut document = Document::blank();
    let outer_id = NodeId::generate();
    let inner_id = NodeId::generate();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines = vec![InlineNode::Environment(InlineEnvironment {
        id: outer_id,
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::new(),
        children: vec![InlineNode::Environment(InlineEnvironment {
            id: inner_id.clone(),
            kind: InlineEnvironmentKind::Italic,
            definition_id: None,
            props: BTreeMap::new(),
            children: vec![InlineNode::Run(Run {
                text: "inside".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    })];
    let mut session =
        Session::new(document).unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let focus = session.selection().focus.clone();
    let paragraph_id = focus.paragraph_id.clone();

    let inside = session
        .current_inline_environment()
        .unwrap_or_else(|error| panic!("inline environment lookup failed: {error}"))
        .unwrap_or_else(|| panic!("caret should inherit nested inline environments"));

    assert_eq!(inside.environment_id, inner_id);
    assert_eq!(inside.paragraph_id, paragraph_id);
    assert_eq!(inside.start_utf8, 0);
    assert_eq!(inside.end_utf8, "inside".len());

    let outside_start = Pos {
        affinity: Affinity::Upstream,
        ..focus.clone()
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: outside_start.clone(),
            focus: outside_start,
        })))
        .unwrap_or_else(|error| panic!("moving outside the environment failed: {error}"));
    assert!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("inline environment lookup failed: {error}"))
            .is_none()
    );
}

#[test]
fn text_map_cache_keeps_both_paragraphs_of_a_cross_paragraph_selection() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    session
        .run(Command::InsertParagraphBreak)
        .unwrap_or_else(|error| panic!("paragraph split failed: {error}"));
    let Block::Paragraph(first) = &session.document.body[0] else {
        panic!("first block should be a paragraph");
    };
    let Block::Paragraph(second) = &session.document.body[1] else {
        panic!("second block should be a paragraph");
    };
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    let first_position = Pos {
        story: StoryId::Body,
        paragraph_id: first_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let second_position = Pos {
        paragraph_id: second_id.clone(),
        ..first_position.clone()
    };

    session
        .text_map_for(&first_position)
        .unwrap_or_else(|error| panic!("first text map failed: {error}"));
    session
        .text_map_for(&second_position)
        .unwrap_or_else(|error| panic!("second text map failed: {error}"));
    session
        .text_map_for(&first_position)
        .unwrap_or_else(|error| panic!("cached first text map failed: {error}"));

    assert_eq!(session.text_map_cache.len(), TEXT_MAP_CACHE_CAPACITY);
    assert!(
        session
            .text_map_cache
            .iter()
            .any(|(id, _, _)| id == &first_id)
    );
    assert!(
        session
            .text_map_cache
            .iter()
            .any(|(id, _, _)| id == &second_id)
    );
}

#[test]
fn revision_overflow_rotates_session_identity_before_reusing_generation_zero() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    let old_session_id = session.session_id().clone();
    session.document_generation = u64::MAX;

    let effect = session
        .run(Command::InsertText("new revision".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));

    assert!(effect.document_changed);
    assert_eq!(session.document_generation(), 0);
    assert_ne!(session.session_id(), &old_session_id);
    assert_eq!(session.selection().focus.offset, "new revision".len());
}

#[test]
fn failed_editor_command_keeps_the_active_command_query() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("sec".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    let expected_command_line = session
        .command_line()
        .cloned()
        .unwrap_or_else(|| panic!("command query should remain active"));
    let invalid_position = Pos {
        offset: usize::MAX,
        ..session.selection().focus.clone()
    };

    let result = session.run(Command::SetSelection(Some(Selection {
        anchor: invalid_position.clone(),
        focus: invalid_position,
    })));

    assert!(matches!(result, Err(EditError::InvalidPosition)));
    assert_eq!(session.command_line(), Some(&expected_command_line));
}

#[test]
fn escape_cancels_command_query_without_changing_document_or_structure() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("sec".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    let revision = session.document_generation();

    let effect = session
        .run(Command::EditorKey(EditorKey::Escape))
        .unwrap_or_else(|error| panic!("Escape failed: {error}"));

    assert!(session.command_line().is_none());
    assert!(!effect.document_changed);
    assert_eq!(session.document_generation(), revision);
    assert_eq!(paragraph_text(&session, 0), "");
}

#[test]
fn enter_accepts_command_query_while_shift_enter_cannot_edit_behind_it() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("newparagraph".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    let revision = session.document_generation();

    let shifted = session
        .run(Command::EditorKey(EditorKey::ShiftEnter))
        .unwrap_or_else(|error| panic!("Shift+Enter dispatch failed: {error}"));
    assert!(!shifted.document_changed);
    assert_eq!(session.document_generation(), revision);
    assert!(session.command_line().is_some());

    let accepted = session
        .run(Command::EditorKey(EditorKey::Enter))
        .unwrap_or_else(|error| panic!("Enter dispatch failed: {error}"));
    assert!(accepted.document_changed);
    assert!(session.command_line().is_none());
    assert_eq!(session.document().body.len(), 2);
}

#[test]
fn escape_cancels_ime_preedit_before_collapsing_an_existing_selection() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("hello".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    let focus = session.selection().focus.clone();
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                offset: 0,
                ..focus.clone()
            },
            focus: focus.clone(),
        })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));
    let selected = session.selection().clone();
    session
        .run(Command::UpdateComposition {
            id: 4,
            preedit: "にほん".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition update failed: {error}"));

    let effect = session
        .run(Command::EditorKey(EditorKey::Escape))
        .unwrap_or_else(|error| panic!("Escape failed: {error}"));

    assert!(effect.composition_changed);
    assert!(!effect.selection_changed);
    assert!(session.composition().is_none());
    assert_eq!(*session.selection(), selected);
    assert_eq!(paragraph_text(&session, 0), "hello");
}

#[test]
fn tab_completes_a_filtered_command_without_editing_the_document() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("sec".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    let command_line = session
        .command_line()
        .unwrap_or_else(|| panic!("command query should remain active"));
    assert_eq!(
        command_line
            .selected_candidate()
            .map(|candidate| candidate.alias),
        Some(std::borrow::Cow::Borrowed("\\section"))
    );
    let revision = session.document_generation();

    session
        .run(Command::TabCommandLine)
        .unwrap_or_else(|error| panic!("Tab completion failed: {error}"));

    let command_line = session
        .command_line()
        .unwrap_or_else(|| panic!("Tab should keep command mode active"));
    assert_eq!(command_line.query, "section");
    assert_eq!(session.document_generation(), revision);
    assert_eq!(paragraph_text(&session, 0), "");

    session
        .run(Command::TypeCommandLine(" ".to_owned()))
        .unwrap_or_else(|error| panic!("space handling failed: {error}"));
    assert_eq!(
        session
            .command_line()
            .map(|command_line| command_line.query.as_str()),
        Some("section")
    );
}

#[test]
fn command_line_preedit_blocks_tab_and_cancel_reports_composition_change() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .input_method_command_line("ni".to_owned(), String::new(), 0, 0, 2)
        .unwrap_or_else(|error| panic!("command-line preedit failed: {error}"));
    let generation = session.document_generation();

    session
        .run(Command::TabCommandLine)
        .unwrap_or_else(|error| panic!("Tab during preedit should be ignored: {error}"));

    let line = session
        .command_line()
        .unwrap_or_else(|| panic!("preedit should keep command line active"));
    assert_eq!(line.preedit, "ni");
    assert_eq!(line.query, "");
    assert_eq!(session.document_generation(), generation);
    assert_eq!(paragraph_text(&session, 0), "");

    let effect = session
        .run(Command::CancelCommandLine)
        .unwrap_or_else(|error| panic!("command-line cancellation failed: {error}"));
    assert!(effect.composition_changed);
    assert!(session.command_line().is_none());
    assert_eq!(session.document_generation(), generation);
}

#[test]
fn font_picker_cursor_never_stops_inside_a_combining_grapheme() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("font".to_owned()))
        .unwrap_or_else(|error| panic!("font command query failed: {error}"));
    session
        .run_command_line_with_font_families(
            Command::AcceptCommandLine,
            vec!["Avenir Next".to_owned()],
        )
        .unwrap_or_else(|error| panic!("font picker did not open: {error}"));
    session
        .run(Command::TypeCommandLine("\u{301}".to_owned()))
        .unwrap_or_else(|error| panic!("combining mark input failed: {error}"));
    session
        .run(Command::MoveCommandLineCursor(Direction::Backward))
        .unwrap_or_else(|error| panic!("font query cursor move failed: {error}"));
    session
        .run(Command::TypeCommandLine("e".to_owned()))
        .unwrap_or_else(|error| panic!("base character input failed: {error}"));

    let line = session
        .command_line()
        .unwrap_or_else(|| panic!("font picker should remain active"));
    assert_eq!(line.query, "e\u{301}");
    assert_eq!(line.cursor, line.query.len());
    assert!(line.query.is_char_boundary(line.cursor));
    assert!(!line.query[..line.cursor].is_empty());
    assert_eq!(line.query[..line.cursor].graphemes(true).count(), 1);
}

#[test]
fn font_command_opens_unicode_family_picker_and_applies_selected_family() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("font".to_owned()))
        .unwrap_or_else(|error| panic!("font command query failed: {error}"));

    session
        .run_command_line_with_font_families(
            Command::TabCommandLine,
            vec!["Avenir Next".to_owned(), "思源 宋体".to_owned()],
        )
        .unwrap_or_else(|error| panic!("font command alias completion failed: {error}"));
    assert_eq!(
        session.command_line().map(|line| line.mode),
        Some(crate::editing::CommandLineMode::Command)
    );

    session
        .run_command_line_with_font_families(
            Command::AcceptCommandLine,
            vec!["Avenir Next".to_owned(), "思源 宋体".to_owned()],
        )
        .unwrap_or_else(|error| panic!("font picker did not open: {error}"));
    let command_line = session
        .command_line()
        .unwrap_or_else(|| panic!("font picker should stay active"));
    assert_eq!(
        command_line.mode,
        crate::editing::CommandLineMode::FontFamily
    );
    assert_eq!(command_line.prefix(), "\\font ");

    session
        .run(Command::TypeCommandLine("\\".to_owned()))
        .unwrap_or_else(|error| panic!("font picker slash input failed: {error}"));
    assert_eq!(
        session.command_line().map(|line| line.query.as_str()),
        Some("\\")
    );
    session
        .run(Command::BackspaceCommandLine)
        .unwrap_or_else(|error| panic!("font picker slash removal failed: {error}"));

    session
        .input_method_command_line("拼音".to_owned(), String::new(), 0, 0, 1)
        .unwrap_or_else(|error| panic!("font picker preedit failed: {error}"));
    assert_eq!(
        session.command_line().map(|line| line.preedit.as_str()),
        Some("拼音")
    );
    session
        .input_method_command_line(String::new(), "思源 宋体".to_owned(), 0, 0, 0)
        .unwrap_or_else(|error| panic!("font picker IME commit failed: {error}"));
    assert_eq!(
        session.command_line().map(|line| line.query.as_str()),
        Some("思源 宋体")
    );

    let candidate = session
        .command_line_candidates()
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("matching family should be suggested"));
    assert_eq!(candidate.command_id, "text.format.font_family");
    assert_eq!(candidate.value.as_deref(), Some("思源 宋体"));

    session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("font family command failed: {error}"));
    assert!(session.command_line().is_none());
    session
        .run(Command::InsertText("字".to_owned()))
        .unwrap_or_else(|error| panic!("font-styled input failed: {error}"));
    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    let Some(InlineNode::Environment(environment)) = paragraph.inlines.first() else {
        panic!("font command should create a Font environment");
    };
    assert_eq!(environment.kind, InlineEnvironmentKind::Font);
}

#[test]
fn literal_slash_at_empty_query_does_not_open_highlighted_font_picker() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    let font_candidate_index = session
        .command_line_candidates()
        .iter()
        .position(|candidate| candidate.command_id == "text.format.font")
        .unwrap_or_else(|| panic!("font command candidate is missing"));
    session
        .run(Command::SelectCommandLineCandidate(font_candidate_index))
        .unwrap_or_else(|error| panic!("font candidate selection failed: {error}"));

    session
        .run_command_line_with_font_families(
            Command::TypeCommandLine("\\".to_owned()),
            vec!["Avenir Next".to_owned()],
        )
        .unwrap_or_else(|error| panic!("literal slash input failed: {error}"));

    assert!(session.command_line().is_none());
    assert_eq!(paragraph_text(&session, 0), "\\");
}

#[test]
fn font_picker_ime_replacement_replaces_the_requested_query_range() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run_command_line_with_font_families(
            Command::BeginCommandLine,
            vec!["Avenir Next".to_owned()],
        )
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("font".to_owned()))
        .unwrap_or_else(|error| panic!("font command query failed: {error}"));
    session
        .run_command_line_with_font_families(
            Command::AcceptCommandLine,
            vec!["Avenir Next".to_owned()],
        )
        .unwrap_or_else(|error| panic!("font picker did not open: {error}"));
    session
        .run(Command::TypeCommandLine("ABCD".to_owned()))
        .unwrap_or_else(|error| panic!("font picker query failed: {error}"));
    session
        .run(Command::MoveCommandLineCursor(Direction::Backward))
        .unwrap_or_else(|error| panic!("font picker cursor move failed: {error}"));
    session
        .input_method_command_line(String::new(), "界".to_owned(), -1, 1, 0)
        .unwrap_or_else(|error| panic!("font picker replacement failed: {error}"));

    assert_eq!(
        session.command_line().map(|line| line.query.as_str()),
        Some("AB界D")
    );
}

#[test]
fn unavailable_candidates_are_hidden_until_prefix_matches_and_cannot_execute() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    let empty_candidates = session
        .command_line()
        .unwrap_or_else(|| panic!("command line should remain active"))
        .candidates();
    assert!(empty_candidates.iter().all(|candidate| candidate.available));
    assert!(
        !empty_candidates
            .iter()
            .any(|candidate| candidate.alias == "\\chapter")
    );

    session
        .run(Command::TypeCommandLine("chapter".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    let command_line = session
        .command_line()
        .unwrap_or_else(|| panic!("command query should remain active"));
    let candidate = command_line
        .selected_candidate()
        .unwrap_or_else(|| panic!("matching unavailable candidate should be visible"));
    assert_eq!(candidate.alias, "\\chapter");
    assert!(!candidate.available);
    let generation = session.document_generation();

    session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("disabled command should be safely ignored: {error}"));

    assert!(session.command_line().is_some());
    assert_eq!(session.document_generation(), generation);
    assert_eq!(paragraph_text(&session, 0), "");
}

#[test]
fn custom_environment_is_not_registered_as_a_user_command_candidate() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank document rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    assert!(
        session
            .command_line_candidates()
            .iter()
            .all(|candidate| candidate.alias != "\\environment")
    );
    session
        .run(Command::TypeCommandLine("env".to_owned()))
        .unwrap_or_else(|error| panic!("command prefix failed: {error}"));
    assert!(
        session
            .command_line_candidates()
            .iter()
            .all(|candidate| candidate.alias != "\\environment")
    );
}

#[test]
fn environment_creation_candidates_are_disabled_in_section_titles() {
    let mut document = Document::blank();
    let title = Paragraph::empty(NodeId::generate());
    let title_id = title.id.clone();
    let section = BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(title),
        children: vec![Block::Paragraph(Paragraph::empty(NodeId::generate()))],
        extra: BTreeMap::new(),
    };
    document.body = vec![Block::Environment(section)];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("valid section document rejected: {error}"));
    assert_eq!(session.selection().focus.paragraph_id, title_id);

    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("sec".to_owned()))
        .unwrap_or_else(|error| panic!("command prefix failed: {error}"));

    let candidate = session
        .command_line_candidates()
        .into_iter()
        .find(|candidate| candidate.alias == "\\section")
        .unwrap_or_else(|| panic!("section command is missing from prefix results"));
    assert!(!candidate.available);
    let document_generation = session.document_generation();
    session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("disabled command acceptance failed: {error}"));
    assert!(session.command_line().is_some());
    assert_eq!(session.document_generation(), document_generation);
}

#[test]
fn accepting_the_newline_command_inserts_a_soft_break_and_closes_the_query() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("hello".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("new".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    session
        .run(Command::SelectCommandLineCandidate(1))
        .unwrap_or_else(|error| panic!("newline candidate selection failed: {error}"));

    let effect = session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("newline command failed: {error}"));

    assert!(effect.document_changed);
    assert_eq!(paragraph_text(&session, 0), "hello\n");
    assert!(session.command_line().is_none());

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undoing the newline command failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hello");

    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redoing the newline command failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hello\n");
}

#[test]
fn accepting_newparagraph_command_creates_a_paragraph_and_is_one_undo_step() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("hello".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("newparagraph".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    session
        .run(Command::SelectCommandLineCandidate(0))
        .unwrap_or_else(|error| panic!("new paragraph candidate selection failed: {error}"));

    let effect = session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("new paragraph command failed: {error}"));

    assert!(effect.document_changed);
    assert_eq!(paragraph_text(&session, 0), "hello");
    assert_eq!(paragraph_text(&session, 1), "");
    assert!(session.command_line().is_none());

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undoing new paragraph failed: {error}"));
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(paragraph_text(&session, 0), "hello");

    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redoing new paragraph failed: {error}"));
    assert_eq!(paragraph_text(&session, 1), "");
}

#[test]
fn first_line_indent_command_is_available_anywhere_and_is_undoable() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("hello".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    let text_before = paragraph_text(&session, 0);
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("firstlineindent".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    assert!(
        session
            .command_line_candidates()
            .iter()
            .any(|candidate| { candidate.alias == "\\firstlineindent" && candidate.available })
    );
    let applied = session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("first-line indent command failed: {error}"));

    assert!(applied.document_changed);
    assert_eq!(paragraph_text(&session, 0), text_before);
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph");
    };
    assert_eq!(
        paragraph
            .layout
            .as_ref()
            .and_then(|layout| layout.first_line_indent_chars),
        Some(2)
    );

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph");
    };
    assert_eq!(paragraph.layout, None);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo failed: {error}"));
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph");
    };
    assert_eq!(
        paragraph
            .layout
            .as_ref()
            .and_then(|layout| layout.first_line_indent_chars),
        Some(2)
    );
}

#[test]
fn tab_inserts_two_character_width_spaces_at_the_current_caret_position() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("abcd".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("first caret move failed: {error}"));
    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("second caret move failed: {error}"));
    let effect = session
        .run(Command::EditorKey(EditorKey::Tab))
        .unwrap_or_else(|error| panic!("Tab insertion failed: {error}"));
    assert!(effect.document_changed);
    assert_eq!(paragraph_text(&session, 0), "ab\u{2003}\u{2003}cd");
}

#[test]
fn width_commands_insert_spaces_with_their_named_widths() {
    for (query, expected_text) in [
        ("singlecharspace", "\u{2003}"),
        ("doublecharspace", "\u{2003}\u{2003}"),
    ] {
        let mut session = Session::new(Document::blank())
            .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
        session
            .run(Command::BeginCommandLine)
            .unwrap_or_else(|error| panic!("command line did not start: {error}"));
        session
            .run(Command::TypeCommandLine(query.to_owned()))
            .unwrap_or_else(|error| panic!("space command query failed: {error}"));
        let candidate = session
            .command_line_candidates()
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("\\{query} candidate missing"));
        assert!(candidate.available);
        session
            .run(Command::AcceptCommandLine)
            .unwrap_or_else(|error| panic!("{query} command failed: {error}"));
        assert_eq!(paragraph_text(&session, 0), expected_text);
    }
}

#[test]
fn multiline_paste_rebuilds_structure_navigation_for_created_paragraphs() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));

    session
        .run(Command::PasteText("first\nsecond".to_owned()))
        .unwrap_or_else(|error| panic!("multiline paste failed: {error}"));

    let Block::Paragraph(first) = &session.document.body[0] else {
        panic!("first pasted block should be a paragraph");
    };
    let Block::Paragraph(second) = &session.document.body[1] else {
        panic!("second pasted block should be a paragraph");
    };
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    assert_eq!(
        session.structure_index().navigation_paragraphs(),
        &[first_id.clone(), second_id]
    );

    session
        .run(Command::MoveStructure(Direction::Backward))
        .unwrap_or_else(|error| panic!("structure navigation after paste failed: {error}"));
    assert_eq!(session.selection().focus.paragraph_id, first_id);
}

#[test]
fn structure_navigation_ends_the_current_deletion_undo_group() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertParagraphBreak)
        .unwrap_or_else(|error| panic!("paragraph split failed: {error}"));
    session
        .run(Command::InsertText("ab".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    let Block::Paragraph(second) = &session.document.body[1] else {
        panic!("second block should be a paragraph");
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                paragraph_id: second.id.clone(),
                offset: 0,
                ..session.selection().focus.clone()
            },
            focus: Pos {
                paragraph_id: second.id.clone(),
                offset: 0,
                ..session.selection().focus.clone()
            },
        })))
        .unwrap_or_else(|error| {
            panic!("moving the caret to the second paragraph start failed: {error}")
        });
    session
        .run(Command::Delete(Direction::Forward))
        .unwrap_or_else(|error| panic!("first forward deletion failed: {error}"));
    session
        .run(Command::MoveStructure(Direction::Backward))
        .unwrap_or_else(|error| panic!("moving to the previous paragraph failed: {error}"));
    session
        .run(Command::MoveStructure(Direction::Forward))
        .unwrap_or_else(|error| panic!("moving to the next paragraph failed: {error}"));
    session
        .run(Command::Delete(Direction::Forward))
        .unwrap_or_else(|error| panic!("second forward deletion failed: {error}"));

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undoing the second deletion failed: {error}"));

    assert_eq!(paragraph_text(&session, 1), "b");
}

#[test]
fn empty_command_tab_inserts_a_literal_prefix() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    let revision = session.document_generation();

    session
        .run(Command::TabCommandLine)
        .unwrap_or_else(|error| panic!("empty-query Tab failed: {error}"));

    assert_eq!(paragraph_text(&session, 0), "\\");
    assert!(session.command_line().is_none());
    assert_eq!(session.document_generation(), revision + 1);
}

#[test]
fn repeating_the_command_prefix_with_an_empty_query_inserts_a_literal_prefix() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));

    session
        .run(Command::TypeCommandLine("\\".to_owned()))
        .unwrap_or_else(|error| panic!("repeated command prefix failed: {error}"));

    assert_eq!(paragraph_text(&session, 0), "\\");
    assert!(session.command_line().is_none());
}

#[test]
fn command_line_does_not_start_during_input_composition() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::UpdateComposition {
            id: 1,
            preedit: "にほん".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition update failed: {error}"));

    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("begin command during composition failed: {error}"));

    assert!(session.command_line().is_none());
    assert_eq!(
        session
            .composition()
            .map(|composition| composition.preedit.as_str()),
        Some("にほん")
    );
    assert_eq!(paragraph_text(&session, 0), "");
}

#[test]
fn undo_and_redo_restore_typed_text_and_selection() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("hello".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    session
        .run(Command::InsertText(" world".to_owned()))
        .unwrap_or_else(|error| panic!("second insertion failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hello world");

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "");
    assert_eq!(session.selection().focus.offset, 0);

    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hello world");
    assert_eq!(session.selection().focus.offset, "hello world".len());
}

#[test]
fn coalesced_typing_counts_retained_history_memory_instead_of_temporary_snapshots() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    for _ in 0..1_000 {
        session
            .run(Command::InsertText("x".to_owned()))
            .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    }
    assert!(session.history_estimated_bytes < history::history_memory_limit());

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("coalesced typing undo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "");
}

#[test]
fn undoing_a_paragraph_split_restores_the_original_paragraph_id_and_text() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("alpha beta".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    let original_id = session.selection().focus.paragraph_id.clone();
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                offset: 5,
                ..session.selection().focus.clone()
            },
            focus: Pos {
                offset: 5,
                ..session.selection().focus.clone()
            },
        })))
        .unwrap_or_else(|error| panic!("selection failed: {error}"));
    session
        .run(Command::InsertParagraphBreak)
        .unwrap_or_else(|error| panic!("paragraph split failed: {error}"));
    assert_eq!(session.document.body.len(), 2);

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("split undo failed: {error}"));
    assert_eq!(session.document.body.len(), 1);
    assert_eq!(paragraph_text(&session, 0), "alpha beta");
    assert_eq!(session.selection().focus.paragraph_id, original_id);

    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("split redo failed: {error}"));
    assert_eq!(session.document.body.len(), 2);
    assert_eq!(paragraph_text(&session, 0), "alpha");
    assert_eq!(paragraph_text(&session, 1), " beta");
}

#[test]
fn editing_after_undo_keeps_the_old_redo_branch() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("first".to_owned()))
        .unwrap_or_else(|error| panic!("first insertion failed: {error}"));
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    session
        .run(Command::InsertText("second".to_owned()))
        .unwrap_or_else(|error| panic!("branch insertion failed: {error}"));

    assert_eq!(paragraph_text(&session, 0), "second");
    assert!(session.edit_history.branches().count() > 0);
}

#[test]
fn undo_then_new_typing_does_not_merge_into_the_old_branch_ancestor() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("a".to_owned()))
        .unwrap_or_else(|error| panic!("first insertion failed: {error}"));
    session
        .run(Command::Delete(Direction::Backward))
        .unwrap_or_else(|error| panic!("deletion failed: {error}"));
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("deletion undo failed: {error}"));
    session
        .run(Command::InsertText("b".to_owned()))
        .unwrap_or_else(|error| panic!("branch insertion failed: {error}"));

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("branch insertion undo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "a");
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("ancestor insertion undo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "");
    assert!(session.edit_history.branches().count() > 0);
}

#[test]
fn ime_replacement_is_one_undoable_transaction_that_restores_replaced_text() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("hello".to_owned()))
        .unwrap_or_else(|error| panic!("initial insertion failed: {error}"));
    let position = session.selection().focus.clone();
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                offset: 1,
                ..position.clone()
            },
            focus: Pos {
                offset: 4,
                ..position
            },
        })))
        .unwrap_or_else(|error| panic!("selection failed: {error}"));

    session
        .commit_composition_replacement(None, 10, 11, 0, 0, "i")
        .unwrap_or_else(|error| panic!("IME replacement failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hio");

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("IME replacement undo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hello");

    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("IME replacement redo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hio");
}

#[test]
fn undo_restores_a_cross_paragraph_deletion_and_original_paragraph_ids() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("alpha".to_owned()))
        .unwrap_or_else(|error| panic!("first insertion failed: {error}"));
    session
        .run(Command::InsertParagraphBreak)
        .unwrap_or_else(|error| panic!("paragraph split failed: {error}"));
    session
        .run(Command::InsertText("gamma".to_owned()))
        .unwrap_or_else(|error| panic!("second insertion failed: {error}"));
    let first_id = match &session.document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => panic!("first block should be a paragraph"),
    };
    let second_id = match &session.document.body[1] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => panic!("second block should be a paragraph"),
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                paragraph_id: first_id.clone(),
                offset: 2,
                ..session.selection().focus.clone()
            },
            focus: Pos {
                paragraph_id: second_id.clone(),
                offset: 3,
                ..session.selection().focus.clone()
            },
        })))
        .unwrap_or_else(|error| panic!("cross-paragraph selection failed: {error}"));
    session
        .run(Command::Delete(Direction::Backward))
        .unwrap_or_else(|error| panic!("cross-paragraph delete failed: {error}"));

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("cross-paragraph undo failed: {error}"));
    assert_eq!(session.document.body.len(), 2);
    assert_eq!(paragraph_text(&session, 0), "alpha");
    assert_eq!(paragraph_text(&session, 1), "gamma");
    assert!(matches!(&session.document.body[0], Block::Paragraph(p) if p.id == first_id));
    assert!(matches!(&session.document.body[1], Block::Paragraph(p) if p.id == second_id));
}

#[test]
fn undo_is_rejected_while_input_composition_is_active() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::UpdateComposition {
            id: 1,
            preedit: "にほん".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition update failed: {error}"));
    assert_eq!(
        session.run(Command::Undo),
        Err(EditError::CompositionPending)
    );
    assert_eq!(paragraph_text(&session, 0), "");
}
