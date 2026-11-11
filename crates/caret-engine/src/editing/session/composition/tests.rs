//! 验证数学组字替换范围与事务失败原子性。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::{
    Affinity, Command, Direction, EditError, MathEditBranch, Pos, Selection, Session, StoryId,
};
use crate::editing::EditorKey;
use caret_doc::{Block, Document, InlineMath, NodeId, ParagraphContent};

fn math_text_session(source: &str) -> Result<Session, EditError> {
    let mut session = Session::new(Document::blank())?;
    session
        .run(Command::InsertInlineMath(source.to_owned()))
        .unwrap_or_else(|error| panic!("insert math source: {error:?}"));
    session
        .run(Command::EnterMath(Direction::Backward))
        .unwrap_or_else(|error| panic!("enter math: {error:?}"));
    for _ in 0..64 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            return Ok(session);
        }
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    Err(EditError::InvalidPosition)
}

fn select_math_text(session: &mut Session, anchor: usize, focus: usize) -> Result<(), EditError> {
    session
        .run(Command::SetMathTextSelection { anchor, focus })
        .unwrap_or_else(|error| panic!("select math text {anchor}..{focus}: {error:?}"));
    Ok(())
}

fn surrounding_text(session: &Session) -> String {
    session
        .math_text_input_context()
        .unwrap_or_else(|| panic!("math TextFlow input context should be available"))
        .surrounding_text
}

fn inline_math_source(session: &Session) -> String {
    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        panic!("first block should be a paragraph");
    };
    let Some(ParagraphContent::Math(InlineMath { math_tree, .. })) = paragraph
        .inlines
        .iter()
        .find(|content| matches!(content, ParagraphContent::Math(_)))
    else {
        panic!("paragraph should contain inline math");
    };
    caret_math::serialize_tree(math_tree)
}

#[test]
fn math_replacement_deletes_selection_before_applying_utf16_range() -> Result<(), EditError> {
    let mut right = math_text_session(r"\frac{1}{\text{abcdef}}")?;
    select_math_text(&mut right, 1, 2)?;
    let generation_before = right.document_generation();
    let original_selection = right
        .math_text_input_context()
        .ok_or(EditError::InvalidPosition)?;

    right
        .commit_composition_replacement(None, 10, 11, 2, 1, "X")
        .unwrap_or_else(|error| panic!("right replacement: {error:?}"));
    assert_eq!(surrounding_text(&right), "acdXf");
    assert_eq!(right.document_generation(), generation_before + 1);
    right.run(Command::Undo)?;
    assert_eq!(surrounding_text(&right), "abcdef");
    let restored_selection = right
        .math_text_input_context()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        restored_selection.anchor_utf16,
        original_selection.anchor_utf16
    );
    assert_eq!(
        restored_selection.focus_utf16,
        original_selection.focus_utf16
    );

    let mut left = math_text_session(r"\frac{1}{\text{abcdef}}")?;
    select_math_text(&mut left, 1, 2)?;
    left.commit_composition_replacement(None, 12, 13, -1, 1, "X")
        .unwrap_or_else(|error| panic!("left replacement: {error:?}"));
    assert_eq!(surrounding_text(&left), "Xcdef");
    Ok(())
}

#[test]
fn empty_math_commit_records_selection_deletion_and_keeps_empty_commit_a_noop()
-> Result<(), EditError> {
    let mut selected = math_text_session(r"\frac{1}{\text{abc}}")?;
    select_math_text(&mut selected, 1, 2)?;
    let generation_before = selected.document_generation();
    selected.commit_composition_replacement(None, 14, 15, 0, 0, "")?;
    assert_eq!(surrounding_text(&selected), "ac");
    assert_eq!(selected.document_generation(), generation_before + 1);
    selected.run(Command::Undo)?;
    assert_eq!(surrounding_text(&selected), "abc");
    assert_eq!(
        selected
            .math_text_input_context()
            .map(|context| (context.anchor_utf16, context.focus_utf16)),
        Some((1, 2))
    );

    let mut collapsed = math_text_session(r"\frac{1}{\text{abc}}")?;
    let generation_before = collapsed.document_generation();
    collapsed.commit_composition_replacement(None, 16, 17, 0, 0, "")?;
    assert_eq!(collapsed.document_generation(), generation_before);
    assert_eq!(surrounding_text(&collapsed), "abc");
    Ok(())
}

#[test]
fn math_replacement_preserves_text_between_selection_and_range_across_flows()
-> Result<(), EditError> {
    let mut session = math_text_session(r"\frac{1}{\text{ab}\textbf{cd}}")?;
    for _ in 0..3 {
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    select_math_text(&mut session, 1, 2)?;

    session
        .commit_composition_replacement(None, 20, 21, 1, 1, "X")
        .unwrap_or_else(|error| panic!("cross-flow replacement: {error:?}"));
    assert_eq!(surrounding_text(&session), "acX");
    assert!(inline_math_source(&session).contains(r"\textbf{cX}"));
    session.run(Command::Undo)?;
    assert_eq!(surrounding_text(&session), "abcd");
    assert!(inline_math_source(&session).contains(r"\textbf{cd}"));
    Ok(())
}

#[test]
fn invalid_math_replacement_rolls_back_selection_deletion_and_composition() -> Result<(), EditError>
{
    let mut session = math_text_session(r"\frac{1}{\text{abcdef}}")?;
    select_math_text(&mut session, 1, 2)?;
    session.run(Command::UpdateComposition {
        id: 30,
        preedit: "preedit".to_owned(),
    })?;
    let original_document = session.document().clone();
    let original_edit = session.math_edit().cloned();
    let original_context = session
        .math_text_input_context()
        .ok_or(EditError::InvalidPosition)?;
    let original_generation = session.document_generation();
    let original_history_state = session.history_state_id();

    assert_eq!(
        session.commit_composition_replacement(Some(30), 31, 32, -200, 1, "X"),
        Err(EditError::InvalidUtf16Offset)
    );
    assert_eq!(session.document(), &original_document);
    assert_eq!(session.math_edit(), original_edit.as_ref());
    assert_eq!(session.math_text_input_context(), Some(original_context));
    assert_eq!(session.document_generation(), original_generation);
    assert_eq!(session.history_state_id(), original_history_state);
    assert_eq!(
        session.composition().map(|composition| composition.id),
        Some(30)
    );
    Ok(())
}

#[test]
fn stale_math_revision_rejects_ime_replacement_before_selection_changes() -> Result<(), EditError> {
    let mut session = math_text_session(r"\frac{1}{\text{abc}}")?;
    select_math_text(&mut session, 0, 2)?;
    let document_before = session.document().clone();
    let selection_before = session.selection().clone();
    let generation_before = session.document_generation();
    let history_state_before = session.history_state_id();
    let can_undo_before = session.can_undo();
    let can_redo_before = session.can_redo();
    let composition_before = session.composition().cloned();
    let stale_session_id = NodeId::generate();
    let math_edit = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    math_edit.revision.session_id = stale_session_id.clone();
    let math_edit_before = math_edit.clone();

    assert_eq!(
        session.commit_composition_replacement(None, 40, 41, 0, 1, "X"),
        Err(EditError::InvalidPosition)
    );

    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_state_before);
    assert_eq!(session.can_undo(), can_undo_before);
    assert_eq!(session.can_redo(), can_redo_before);
    assert_eq!(session.composition(), composition_before.as_ref());
    assert_eq!(session.math_edit(), Some(&math_edit_before));
    assert_eq!(
        session
            .math_edit
            .as_ref()
            .map(|edit| edit.revision.session_id.clone()),
        Some(stale_session_id)
    );
    assert_eq!(session.math_text_input_context(), None);
    Ok(())
}

#[test]
fn math_replacement_advances_generation_once_across_overflow() -> Result<(), EditError> {
    for (generation_before, generation_after, should_rotate_session) in
        [(u64::MAX - 1, u64::MAX, false), (u64::MAX, 0, true)]
    {
        let mut session = math_text_session(r"\frac{1}{\text{abc}}")?;
        select_math_text(&mut session, 1, 2)?;
        let original_session_id = session.session_id.clone();
        session.document_generation = generation_before;
        let edit = session
            .math_edit
            .as_mut()
            .ok_or(EditError::InvalidPosition)?;
        edit.revision.session_id = original_session_id.clone();
        edit.revision.document_generation = generation_before;

        session.commit_composition_replacement(None, 50, 51, 0, 0, "X")?;

        assert_eq!(surrounding_text(&session), "aXc");
        assert_eq!(session.document_generation(), generation_after);
        assert_eq!(
            session.session_id() != &original_session_id,
            should_rotate_session
        );
        let revision = &session
            .math_edit()
            .ok_or(EditError::InvalidPosition)?
            .revision;
        assert_eq!(revision.session_id, *session.session_id());
        assert_eq!(revision.document_generation, generation_after);
        assert!(session.can_undo());

        session.run(Command::Undo)?;
        assert_eq!(surrounding_text(&session), "abc");
    }
    Ok(())
}

fn paragraph_text(session: &Session) -> String {
    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        panic!("first block should be a paragraph");
    };
    paragraph
        .inlines
        .iter()
        .filter_map(|content| match content {
            ParagraphContent::Text(text) => Some(
                text.runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

#[test]
fn paragraph_replacement_advances_generation_once_across_overflow() -> Result<(), EditError> {
    for (generation_before, generation_after, should_rotate_session) in
        [(u64::MAX - 1, u64::MAX, false), (u64::MAX, 0, true)]
    {
        let mut session = Session::new(Document::blank())?;
        session.run(Command::InsertText("abc".to_owned()))?;
        let mut selection = session.selection().clone();
        selection.anchor.offset = 1;
        selection.anchor.affinity = Affinity::Upstream;
        selection.focus.offset = 2;
        selection.focus.affinity = Affinity::Downstream;
        session.run(Command::SetSelection(Some(selection)))?;

        let original_session_id = session.session_id.clone();
        session.document_generation = generation_before;
        session.commit_composition_replacement(None, 60, 61, 0, 0, "X")?;

        assert_eq!(paragraph_text(&session), "aXc");
        assert_eq!(session.document_generation(), generation_after);
        assert_eq!(
            session.session_id() != &original_session_id,
            should_rotate_session
        );
        assert!(session.can_undo());
        session.run(Command::Undo)?;
        assert_eq!(paragraph_text(&session), "abc");
    }
    Ok(())
}

#[test]
fn paragraph_ime_replacement_orders_a_cross_paragraph_selection() -> Result<(), EditError> {
    let mut session = Session::new(Document::blank())?;
    session.run(Command::InsertText("abc".to_owned()))?;
    session.run(Command::EditorKey(EditorKey::Enter))?;
    session.run(Command::InsertText("def".to_owned()))?;
    let [Block::Paragraph(first), Block::Paragraph(second)] = session.document().body.as_slice()
    else {
        return Err(EditError::InvalidPosition);
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: Pos {
            story: StoryId::Body,
            paragraph_id: first.id.clone(),
            offset: 1,
            affinity: Affinity::Upstream,
        },
        focus: Pos {
            story: StoryId::Body,
            paragraph_id: second.id.clone(),
            offset: 2,
            affinity: Affinity::Downstream,
        },
    })))?;

    session.commit_composition_replacement(None, 30, 31, 0, 0, "X")?;

    assert_eq!(paragraph_text(&session), "aXf");
    Ok(())
}
