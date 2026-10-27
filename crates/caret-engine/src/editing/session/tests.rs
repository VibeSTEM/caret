//! 验证编辑会话缓存与修订代际。

use super::*;
use caret_doc::Block;

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
