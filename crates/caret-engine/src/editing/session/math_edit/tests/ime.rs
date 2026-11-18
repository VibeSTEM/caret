//! 公式编辑测试：ime。

use super::*;

#[test]
fn math_text_command_splits_inline_math_and_accepts_ime_input() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("xy".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));

    accept(&mut session, "text")
        .map_err(|error| panic!("accepting the \\text command failed: {error:?}"))?;

    assert!(session.math_edit().is_none());
    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(matches!(
        paragraph.inlines.as_slice(),
        [ParagraphContent::Math(_), ParagraphContent::Text(text), ParagraphContent::Math(_)]
            if text.runs.iter().all(|run| run.text.is_empty())
    ));
    session
        .run(Command::InsertText("中文".to_owned()))
        .map_err(|error| panic!("typing in the promoted text island failed: {error:?}"))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::InsertText("tail".to_owned()))?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}中文{OBJECT_REPLACEMENT}tail")
    );
    let mut start = session.selection().focus.clone();
    start.offset = OBJECT_REPLACEMENT.len_utf8();
    let mut end = start.clone();
    end.offset += "中文".len();
    session.run(Command::SetSelection(Some(Selection {
        anchor: start,
        focus: end,
    })))?;
    let delete_effect = session.run(Command::Delete(Direction::Backward))?;
    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(matches!(
        paragraph.inlines.as_slice(),
        [ParagraphContent::Math(_), ParagraphContent::Text(text)]
            if text.runs.iter().map(|run| run.text.as_str()).collect::<String>() == "tail"
    ));
    assert_eq!(committed_inline_source(&session).as_deref(), Some("xy"));
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}tail")
    );
    let mut old_tail_position = session.selection().focus.clone();
    old_tail_position.offset = 15;
    old_tail_position.affinity = Affinity::Upstream;
    let mapped_tail_position = delete_effect
        .position_map
        .as_ref()
        .ok_or(EditError::InvalidPosition)?
        .map(&old_tail_position);
    assert_eq!(mapped_tail_position.offset, 6);
    assert_eq!(mapped_tail_position.affinity, Affinity::Upstream);
    for (offset, affinity) in [
        (3, Affinity::Upstream),
        (3, Affinity::Downstream),
        (9, Affinity::Upstream),
        (12, Affinity::Downstream),
    ] {
        let mut old_seam_position = old_tail_position.clone();
        old_seam_position.offset = offset;
        old_seam_position.affinity = affinity;
        let mapped = delete_effect
            .position_map
            .as_ref()
            .ok_or(EditError::InvalidPosition)?
            .map(&old_seam_position);
        assert_eq!(mapped.offset, 3);
        assert_eq!(mapped.affinity, affinity);
    }
    assert!(matches!(
        session.math_edit().map(|edit| (&edit.branch, edit.cursor)),
        Some((MathEditBranch::Tree { path, .. }, 1)) if path.steps().is_empty()
    ));
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: false,
    }))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: false,
    }))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));

    session.run(Command::Undo)?;
    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(matches!(
        paragraph.inlines.as_slice(),
        [
            ParagraphContent::Math(_),
            ParagraphContent::Text(_),
            ParagraphContent::Math(_),
            ParagraphContent::Text(_)
        ]
    ));
    Ok(())
}

#[test]
fn math_text_command_splits_an_empty_inline_math_into_ime_text() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;

    accept(&mut session, "text")?;
    assert!(session.math_edit().is_none());
    session.run(Command::InsertText("中文".to_owned()))?;
    assert_eq!(first_paragraph_text(&session), "中文");
    Ok(())
}

#[test]
fn math_text_command_enters_ime_text_in_equation_and_align_blocks() -> Result<(), EditError> {
    for block_command in ["equation", "align"] {
        let mut session = session_with_text("").map_err(test_error)?;
        accept(&mut session, block_command)?;
        accept(&mut session, "text")?;

        assert!(matches!(
            session.math_edit().map(|edit| (&edit.target, &edit.branch)),
            Some((MathEditTarget::Block { .. }, MathEditBranch::Text { .. }))
        ));
        assert!(session.math_text_input_context().is_some());
        session.run(Command::InsertText("中文".to_owned()))?;
        assert!(inline_source(&session).is_some_and(|source| source.contains("中文")));
    }
    Ok(())
}

#[test]
fn escape_cancels_math_text_composition_without_exiting_formula() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\frac{1}{\text{ab}}".to_owned()))?;
    move_into_math_text(&mut session)?;
    let before = session.document().clone();
    session.run(Command::UpdateComposition {
        id: 902,
        preedit: "中".to_owned(),
    })?;

    let effect = session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(effect.composition_changed);
    assert_eq!(session.document(), &before);
    assert!(session.composition_preview_with_range(0).is_err());
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| { matches!(edit.branch, MathEditBranch::Text { .. }) })
    );
    Ok(())
}

#[test]
fn math_text_composition_previews_and_commits_through_the_tree_transaction() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\frac{1}{\text{ab}}".to_owned()))?;
    move_into_math_text(&mut session).unwrap_or_else(|error| panic!("enter text flow: {error:?}"));
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    let before = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    let before_generation = session.document_generation();
    session
        .run(Command::UpdateComposition {
            id: 81,
            preedit: "中".to_owned(),
        })
        .unwrap_or_else(|error| panic!("update composition: {error:?}"));

    let (preview, range) = session
        .composition_preview_with_range("中".encode_utf16().count())
        .unwrap_or_else(|error| panic!("preview composition: {error:?}"));
    assert!(matches!(
        range,
        crate::editing::CompositionRange::MathText { .. }
    ));
    assert_eq!(inline_source(&session).as_deref(), Some(before.as_str()));
    assert_eq!(
        inline_source(&preview).as_deref(),
        Some(r"\frac{1}{\text{a中b}}")
    );

    session
        .run(Command::CommitComposition {
            id: 81,
            text: "中".to_owned(),
        })
        .unwrap_or_else(|error| panic!("commit composition: {error:?}"));
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\frac{1}{\text{a中b}}")
    );
    assert_eq!(session.document_generation(), before_generation + 1);
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo composition: {error:?}"));
    assert_eq!(inline_source(&session).as_deref(), Some(before.as_str()));
    Ok(())
}

#[test]
fn math_text_composition_replacement_uses_utf16_and_is_one_undoable_edit() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{A😀B}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 5,
        focus: 5,
    })?;
    let before = inline_source(&session).ok_or(EditError::InvalidPosition)?;

    session.commit_composition_replacement(None, 82, 83, -2, 2, "中")?;
    assert!(session.composition().is_none());
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\frac{1}{\text{A中B}}")
    );
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(before.as_str()));
    Ok(())
}

#[test]
fn math_text_composition_replaces_a_cross_environment_selection_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{ab}\textbf{cd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    let anchor = MathTextPosition::new(island.paths[0].clone(), 1, MathPositionAffinity::Upstream);
    let focus = MathTextPosition::new(island.paths[1].clone(), 1, MathPositionAffinity::Upstream);
    session.set_math_text_selection_positions(anchor, focus)?;
    let generation_before = session.document_generation();

    session.commit_composition_replacement(None, 87, 88, 0, 0, "X")?;
    let (island, _, _) = session
        .math_text_selection_island_offsets()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display(), "aXd");
    assert_eq!(session.document_generation(), generation_before + 1);
    session.run(Command::Undo)?;
    let (island, _, _) = session
        .math_text_selection_island_offsets()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display(), "abcd");
    Ok(())
}

#[test]
fn math_text_selection_api_rejects_pending_composition_and_stale_revision() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\frac{1}{\text{ab}}".to_owned()))?;
    move_into_math_text(&mut session)?;
    let context = session
        .math_text_input_context()
        .ok_or(EditError::InvalidPosition)?;

    session.run(Command::UpdateComposition {
        id: 87,
        preedit: "中".to_owned(),
    })?;
    let pending_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        session.set_math_text_selection_positions(context.anchor.clone(), context.focus.clone()),
        Err(EditError::CompositionPending)
    );
    assert_eq!(session.math_edit(), Some(&pending_edit));
    assert_eq!(
        session.composition().map(|composition| composition.id),
        Some(87)
    );

    session.run(Command::CancelComposition { id: 87 })?;
    let revision = session.current_math_edit_revision();
    let edit = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    edit.revision.document_generation += 1;
    let stale_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    assert!(session.current_math_edit_target().is_none());
    assert_eq!(
        session.set_math_text_selection_positions(context.anchor, context.focus),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.math_edit(), Some(&stale_edit));

    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision = revision;
    assert_eq!(session.current_math_edit_target(), Some(&stale_edit.target));
    Ok(())
}

#[test]
fn invalid_math_text_composition_replacement_keeps_the_pending_state() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\frac{1}{\text{ab}}".to_owned()))?;
    move_into_math_text(&mut session)?;
    let before_document = session.document().clone();
    let before_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let before_history = session.history_state_id();
    session.run(Command::UpdateComposition {
        id: 84,
        preedit: "中".to_owned(),
    })?;

    assert_eq!(
        session.commit_composition_replacement(Some(84), 85, 86, -200, 1, "x"),
        Err(EditError::InvalidUtf16Offset)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), Some(&before_edit));
    assert_eq!(session.history_state_id(), before_history);
    assert_eq!(
        session.composition().map(|composition| composition.id),
        Some(84)
    );
    Ok(())
}

#[test]
fn math_source_mode_rejects_ime_without_changing_focus_or_document()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("abc".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    let before_session_id = session.session_id().clone();
    assert_eq!(
        session.run(Command::UpdateComposition {
            id: 73,
            preedit: "X".to_owned(),
        }),
        Err(EditError::CompositionPending)
    );
    assert!(matches!(
        session.composition_update_candidate(73, "X".to_owned(), None, 1),
        Err(EditError::CompositionPending)
    ));
    assert_eq!(
        session.commit_composition_replacement(None, 74, 75, -1, 1, "X"),
        Err(EditError::CompositionPending)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    assert_eq!(session.session_id(), &before_session_id);
    assert!(session.composition().is_none());
    Ok(())
}

#[test]
fn math_command_query_rejects_ime_preedit_and_commit() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("frac".to_owned()))?;
    let query_before = session.command_line().cloned();
    let document_before = session.document().clone();
    let generation_before = session.document_generation();

    assert_eq!(
        session.input_method_command_line("ni".to_owned(), "字".to_owned(), 0, 0, 2),
        Err(EditError::CompositionPending)
    );
    assert_eq!(session.command_line(), query_before.as_ref());
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.document_generation(), generation_before);
    assert!(session.composition().is_none());
    session.run(Command::EditorKey(EditorKey::ShiftEnter))?;
    assert_eq!(session.command_line(), query_before.as_ref());
    assert_eq!(session.document(), &document_before);
    Ok(())
}

#[test]
fn stale_composition_candidate_cannot_replace_a_newer_math_edit_session()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("a")?;
    let (candidate, _, _) = session.composition_update_candidate(73, "X".to_owned(), None, 1)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let document_before = session.document().clone();
    let math_edit_before = session.math_edit().cloned();

    assert!(matches!(
        session.install_composition_candidate(candidate),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), math_edit_before.as_ref());
    assert!(session.composition().is_none());
    Ok(())
}

#[test]
fn stale_composition_candidate_cannot_restore_an_old_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("ab")?;
    let (candidate, _, _) = session.composition_update_candidate(74, "X".to_owned(), None, 1)?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    let document_before = session.document().clone();
    let selection_before = session.selection().clone();

    assert!(matches!(
        session.install_composition_candidate(candidate),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert!(session.composition().is_none());
    Ok(())
}

#[test]
fn stale_composition_candidate_cannot_restore_an_old_save_checkpoint()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("ab")?;
    let (candidate, _, _) = session.composition_update_candidate(75, "X".to_owned(), None, 1)?;
    session.begin_save_checkpoint()?;
    let checkpoint_epoch = session.history_merge_epoch;

    assert!(matches!(
        session.install_composition_candidate(candidate),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.history_merge_epoch, checkpoint_epoch);
    assert!(session.composition().is_none());
    Ok(())
}
