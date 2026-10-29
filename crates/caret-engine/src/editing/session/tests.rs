//! 验证编辑会话缓存与修订代际。

use super::*;
use crate::editing::EditorKey;
use caret_doc::{Block, BlockEnvironment, BlockEnvironmentKind, NodeId, Paragraph};
use std::collections::BTreeMap;

fn paragraph_text(session: &Session, block_index: usize) -> String {
    let Some(Block::Paragraph(paragraph)) = session.document.body.get(block_index) else {
        panic!("expected paragraph at block index {block_index}");
    };
    TextMap::new(paragraph)
        .unwrap_or_else(|error| panic!("paragraph text map failed: {error}"))
        .text
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
        Some("\\section")
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
