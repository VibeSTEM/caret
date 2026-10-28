//! 验证编辑会话缓存与修订代际。

use super::*;
use caret_doc::Block;

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
        .run(Command::SplitParagraph)
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
        .run(Command::SplitParagraph)
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
        .run(Command::SplitParagraph)
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
