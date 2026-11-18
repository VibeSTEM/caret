//! 公式编辑测试：symbol input。

use super::*;

#[test]
fn formula_cursor_uses_the_same_grapheme_boundaries_as_text() {
    let source = "\\alpha a\u{301}β";
    let mut stops = vec![0];
    let mut cursor = 0;
    while let Some(next) = next_grapheme_end(source, cursor) {
        stops.push(next);
        cursor = next;
    }
    assert_eq!(stops, vec![0, 1, 2, 3, 4, 5, 6, 7, 10, 12]);
    assert_eq!(previous_grapheme_start(source, 10), Some(7));
}

#[test]
fn arrow_treats_a_rendered_control_symbol_as_one_caret_stop() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\alpha".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(6));

    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    Ok(())
}

#[test]
fn backspace_deletes_one_grapheme_from_formula_source() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath("abc".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(inline_source(&session).as_deref(), Some("ab"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    Ok(())
}

#[test]
fn unsupported_host_commands_do_not_escape_formula_editing() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::InsertText("host".to_owned()))?;
    session.run(Command::InsertInlineMath(r"\frac{p}{q}".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let [ParagraphContent::Environment(environment)] = paragraph.inlines.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(environment.kind, InlineEnvironmentKind::Bold);
    assert!(
        environment
            .children
            .iter()
            .any(|content| matches!(content, ParagraphContent::Math(_)))
    );

    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    for command in [Command::InsertSoftBreak, Command::InsertParagraphBreak] {
        assert_eq!(session.run(command), Err(EditError::UnsupportedNewline));
        assert_eq!(session.document(), &before_document);
        assert_eq!(session.math_edit(), before_edit.as_ref());
        assert_eq!(session.document_generation(), before_generation);
    }
    Ok(())
}

#[test]
fn math_command_line_keeps_mod_backspace_as_a_keyboard_noop() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::BeginCommandLine)?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_command_line = session.command_line().cloned();

    let effect = session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(!effect.document_changed);
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.command_line(), before_command_line.as_ref());
    Ok(())
}

#[test]
fn mod_backspace_rejects_selection_that_becomes_mid_grapheme_in_source_text()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(
        "\\frac{x}{\\text{\u{301}x}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 2,
    })?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();

    assert_eq!(
        session.run(Command::EditorKey(EditorKey::ModBackspace)),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn mod_backspace_snaps_collapsed_caret_downstream_when_text_graphemes_join() -> Result<(), EditError>
{
    let mut session = session_with_text("\u{301}").map_err(test_error)?;
    let mut start = session.selection().focus.clone();
    start.offset = 0;
    start.affinity = Affinity::Downstream;
    session.run(Command::SetSelection(Some(Selection {
        anchor: start.clone(),
        focus: start,
    })))?;
    session.run(Command::InsertInlineMath("a".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;

    let before = session.document().clone();
    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert_eq!(first_paragraph_text(&session), "a\u{301}");
    assert_eq!(session.selection().anchor, session.selection().focus);
    assert_eq!(session.selection().focus.offset, "a\u{301}".len());
    session.run(Command::Undo)?;
    assert_eq!(session.document(), &before);
    session.run(Command::Redo)?;
    assert_eq!(session.selection().focus.offset, "a\u{301}".len());
    session.run(Command::InsertText("!".to_owned()))?;
    assert_eq!(first_paragraph_text(&session), "a\u{301}!");
    Ok(())
}

#[test]
fn deleting_empty_math_snaps_caret_when_adjacent_text_joins_a_grapheme() -> Result<(), EditError> {
    let caret_math::ParsedMath::Tree(math_tree) =
        caret_math::parse("", caret_math::MathInputKind::Inline).map_err(test_error)?
    else {
        return Err(EditError::InvalidMath("expected inline math".to_owned()));
    };
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![
        ParagraphContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: "a".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Math(InlineMath {
            id: NodeId::generate(),
            math_tree,
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: "\u{301}".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }),
    ];
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session = Session::new(document)?;
    let mut after_math = session.selection().focus.clone();
    after_math.offset = "a".len() + OBJECT_REPLACEMENT.len_utf8();
    session.run(Command::SetSelection(Some(Selection {
        anchor: after_math.clone(),
        focus: after_math.clone(),
    })))?;
    let mut before_atom = after_math.clone();
    before_atom.offset -= OBJECT_REPLACEMENT.len_utf8();
    before_atom.affinity = Affinity::Upstream;
    let mut after_atom = after_math.clone();
    after_atom.affinity = Affinity::Downstream;
    session.run(Command::EnterMath(Direction::Backward))?;

    let effect = session.run(Command::Delete(Direction::Backward))?;
    let position_map = effect
        .position_map
        .as_ref()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(position_map.map(&before_atom).offset, 0);
    assert_eq!(position_map.map(&after_atom).offset, 3);
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), "a\u{301}");

    session.run(Command::Undo)?;
    assert!(session.math_edit().is_some());
    assert_eq!(
        first_paragraph_text(&session),
        format!("a{OBJECT_REPLACEMENT}\u{301}")
    );
    session.run(Command::Redo)?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.offset, 0);
    assert_eq!(first_paragraph_text(&session), "a\u{301}");
    session.run(Command::InsertText("x".to_owned()))?;
    assert_eq!(first_paragraph_text(&session), "xa\u{301}");
    session.run(Command::InsertText("y".to_owned()))?;
    Ok(())
}

#[test]
fn caret_can_stop_between_inline_math_and_following_combining_text() -> Result<(), EditError> {
    let mut session = session_with_text("a").map_err(test_error)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::InsertText("\u{301}".to_owned()))?;

    let inline_count = match session.document().body.first() {
        Some(Block::Paragraph(paragraph)) => paragraph.inlines.len(),
        _ => return Err(EditError::InvalidPosition),
    };
    assert_eq!(inline_count, 3);
    let atom_end = "a".len() + OBJECT_REPLACEMENT.len_utf8();
    let position = Pos {
        offset: atom_end,
        affinity: Affinity::Downstream,
        ..session.selection().focus.clone()
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: position.clone(),
        focus: position,
    })))?;

    assert_eq!(session.selection().focus.offset, atom_end);
    for _ in 0..4 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert_eq!(session.selection().focus.offset, atom_end + "\u{301}".len());
    Ok(())
}

#[test]
fn text_island_accepts_plain_input_and_rebases_its_path() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    let before_command = session.document().clone();
    let before_focus = session.math_edit().cloned();
    session.run(Command::BeginCommandLine)?;
    assert_eq!(
        session.command_line().map(|line| line.mode),
        Some(CommandLineMode::Math)
    );
    session.run(Command::CancelCommandLine)?;
    assert_eq!(session.document(), &before_command);
    assert_eq!(session.math_edit(), before_focus.as_ref());

    session.run(Command::InsertText("X".to_owned()))?;

    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abcX}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(4));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abc}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    Ok(())
}

#[test]
fn math_text_command_replaces_root_selection_and_undo_restores_its_tree_focus()
-> Result<(), EditError> {
    for (anchor_offset, focus_offset) in [(0, 2), (2, 0)] {
        let mut session = session_with_text("").map_err(test_error)?;
        session.begin_inline_math()?;
        session.run(Command::InsertText("xy".to_owned()))?;
        let mut edit = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Inline { math_id, .. } = &edit.target else {
            return Err(EditError::InvalidPosition);
        };
        let path = MathSequencePath::root();
        let anchor = MathTreePosition {
            host: MathPositionHost::InlineMath {
                owner_id: math_id.clone(),
            },
            path: path.clone(),
            offset: anchor_offset,
            affinity: MathPositionAffinity::Downstream,
        };
        edit.cursor = focus_offset;
        edit.branch = MathEditBranch::Tree { path, anchor };
        session.math_edit = Some(edit);

        accept(&mut session, "text")?;
        assert!(matches!(
            session.document().body.first(),
            Some(Block::Paragraph(Paragraph { inlines, .. }))
                if matches!(inlines.as_slice(), [ParagraphContent::Text(text)]
                    if text.runs.iter().all(|run| run.text.is_empty()))
        ));

        session.run(Command::Undo)?;
        assert_eq!(committed_inline_source(&session).as_deref(), Some("xy"));
        assert!(matches!(
            session.math_edit().map(|edit| (&edit.branch, edit.cursor)),
            Some((MathEditBranch::Tree { path, anchor }, cursor))
                if path.steps().is_empty()
                    && anchor.offset == anchor_offset
                    && cursor == focus_offset
        ));
    }
    Ok(())
}

#[test]
fn math_text_input_preserves_run_boundaries_when_text_flow_has_extensions() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\frac{x}{\text{ab}}".to_owned()))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;

    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditTarget::Inline { math_id, .. } = edit.target else {
        return Err(EditError::InvalidPosition);
    };
    let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
        return Err(EditError::InvalidPosition);
    };
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let text =
        first_math_text_flow_mut(&mut math.math_tree.root).ok_or(EditError::InvalidPosition)?;
    text.runs = vec![
        caret_doc::Run {
            text: "a".to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        },
        caret_doc::Run {
            text: "b".to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        },
    ];
    text.extra
        .insert("vendorTextFlow".to_owned(), serde_json::json!("keep"));

    session.run(Command::InsertText("x".to_owned()))?;
    let inserted = active_math_text_flow(&session)?;
    assert_eq!(
        inserted
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "xb"]
    );
    assert_eq!(
        inserted.extra.get("vendorTextFlow"),
        Some(&serde_json::json!("keep"))
    );

    session.run(Command::Undo)?;
    let undone = active_math_text_flow(&session)?;
    assert_eq!(undone.runs[0].text, "a");
    assert_eq!(undone.runs[1].text, "b");
    assert_eq!(undone.runs.len(), 2);
    assert_eq!(
        undone.extra.get("vendorTextFlow"),
        Some(&serde_json::json!("keep"))
    );
    Ok(())
}

#[test]
fn math_text_pending_underline_survives_sequential_input_until_explicit_move()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::ToggleRunUnderline)?;
    session.run(Command::InsertText("X".to_owned()))?;
    session.run(Command::InsertText("Y".to_owned()))?;

    let flow = active_math_text_flow(&session)?;
    assert!(flow.runs.iter().any(|run| {
        run.text.contains('X')
            && run.text.contains('Y')
            && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{aXb}"));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{ab}"));
    session.run(Command::Redo)?;
    session.run(Command::Redo)?;
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text.contains('X')
            && run.text.contains('Y')
            && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));

    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(!session.pending_inline_style.is_active());
    Ok(())
}

#[test]
fn pending_run_property_covers_a_grapheme_spanning_math_textflows_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, "\\text{a}\\text{}")?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let path = path::root_text_flow_path(tree, false, None).ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 0);
    session.run(Command::ToggleRunUnderline)?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    session.run(Command::InsertText("\u{301}".to_owned()))?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(
        tree,
        match &edit.branch {
            MathEditBranch::Text { path, .. } => path,
            MathEditBranch::Source => return Err(EditError::InvalidPosition.into()),
            MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition.into()),
        },
    )
    .ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "a\u{301}");
    for candidate in &island.paths {
        let text = candidate
            .resolve_structurally(tree)
            .ok_or(EditError::InvalidPosition)?;
        assert!(text.runs.iter().all(|run| {
            run.text.is_empty()
                || run.props.get("underline") == Some(&serde_json::Value::Bool(true))
        }));
        assert_eq!(
            super::text_inline_environment::inherited_environment_value(
                tree,
                candidate,
                InlineEnvironmentKind::Bold,
            ),
            Some(true)
        );
        assert_eq!(
            super::text_inline_environment::inherited_math_text_font_family(tree, candidate)
                .flatten()
                .as_deref(),
            Some("Noto Serif")
        );
    }
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{a}"));
    session.run(Command::Redo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    let path = match &edit.branch {
        MathEditBranch::Text { path, .. } => path,
        MathEditBranch::Source => return Err(EditError::InvalidPosition.into()),
        MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition.into()),
    };
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "a\u{301}");
    Ok(())
}

#[test]
fn pending_run_property_covers_a_regional_indicator_pair_across_textflows()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, "\\text{🇦}\\text{}")?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let path = path::root_text_flow_path(tree, false, None).ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 0);
    session.run(Command::ToggleRunUnderline)?;
    session.run(Command::InsertText("🇽".to_owned()))?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    let path = match &edit.branch {
        MathEditBranch::Text { path, .. } => path,
        MathEditBranch::Source => return Err(EditError::InvalidPosition.into()),
        MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition.into()),
    };
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "🇦🇽");
    let first = island.paths[0]
        .resolve_structurally(tree)
        .ok_or(EditError::InvalidPosition)?;
    assert!(first.runs.iter().any(|run| {
        run.text == "🇦" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    let second = island.paths[1]
        .resolve_structurally(tree)
        .ok_or(EditError::InvalidPosition)?;
    assert!(second.runs.iter().any(|run| {
        run.text == "🇽" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    Ok(())
}

#[test]
fn cross_flow_grapheme_format_rejects_unknown_run_metadata_without_partial_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, "\\text{a}\\text{}")?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let path = path::root_text_flow_path(session.math_tree_for_edit(&edit, None)?, false, None)
        .ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 0);
    let Some(Block::Equation(equation)) = session.document.body.first_mut() else {
        return Err(EditError::InvalidPosition.into());
    };
    let adjacent = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(0)], None);
    adjacent
        .resolve_structurally_mut(&mut equation.math_tree)
        .and_then(|text| text.runs.first_mut())
        .ok_or(EditError::InvalidPosition)?
        .extra
        .insert("futureRunField".to_owned(), serde_json::json!("opaque"));
    session.run(Command::ToggleRunUnderline)?;
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();

    assert!(matches!(
        session.run(Command::InsertText("\u{301}".to_owned())),
        Err(EditError::UnsupportedOpaqueContent)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.document_generation(), generation_before);
    Ok(())
}

#[test]
fn math_text_input_context_exposes_the_whole_active_island_range()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;

    let context = session
        .math_text_input_context()
        .ok_or(EditError::InvalidPosition)?;
    assert!(matches!(
        &context.target,
        MathEditTarget::Block { align: false, .. }
    ));
    assert_eq!(context.island_start.offset, 0);
    assert_eq!(context.island_end.offset, context.surrounding_text.len());
    assert_eq!(context.island_start.path, context.island_end.path);
    assert_eq!(context.surrounding_text, "abcd");
    assert_eq!(session.current_math_edit_target(), Some(&context.target));
    Ok(())
}

#[test]
fn math_text_selection_rejects_non_grapheme_positions_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath("\\text{a\u{301}b}".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    assert_eq!(
        session.run(Command::SetMathTextSelection {
            anchor: 1,
            focus: 3
        }),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn deletion_treats_empty_and_combining_textflows_as_one_logical_grapheme() -> Result<(), EditError>
{
    let source = "\\text{a}\\text{}\\text{\u{301}}";
    let mut backward = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut backward, source)?;
    move_into_math_text(&mut backward)?;
    backward.run(Command::Delete(Direction::Backward))?;
    let edit = backward.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = backward.math_tree_for_edit(edit, None)?;
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(|content| match content {
                MathContent::Text(text) => Some(
                    text.runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["", "", ""]
    );
    backward.run(Command::Undo)?;
    let edit = backward.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = backward.math_tree_for_edit(edit, None)?;
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(|content| match content {
                MathContent::Text(text) => Some(
                    text.runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["a", "", "\u{301}"]
    );

    let mut forward = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut forward, source)?;
    move_into_math_text(&mut forward)?;
    forward.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(forward.math_edit().map(|edit| edit.cursor), Some(0));
    forward.run(Command::Delete(Direction::Forward))?;
    let edit = forward.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = forward.math_tree_for_edit(edit, None)?;
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(|content| match content {
                MathContent::Text(text) => Some(
                    text.runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["", "", ""]
    );
    Ok(())
}

#[test]
fn caret_navigation_never_stops_inside_a_grapheme_split_across_textflows() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, "\\text{a}\\text{}\\text{\u{301}}")?;
    move_into_math_text(&mut session)?;

    session.run(Command::MoveCaret(Direction::Backward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let text = path
        .resolve_structurally(session.math_tree_for_edit(edit, None)?)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "a"
    );
    assert_eq!(edit.cursor, 0);

    session.run(Command::MoveCaret(Direction::Forward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let text = path
        .resolve_structurally(session.math_tree_for_edit(edit, None)?)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "\u{301}"
    );
    assert_eq!(edit.cursor, "\u{301}".len());
    Ok(())
}

#[test]
fn insertion_snaps_to_the_logical_grapheme_across_textflows_and_undoes() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, "\\text{}\\text{\u{301}}")?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let first = path::root_text_flow_path(tree, true, None).ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(first, 0);
    let document_before = session.document().clone();

    session.run(Command::InsertText("a".to_owned()))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let text = path
        .resolve_structurally(session.math_tree_for_edit(edit, None)?)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "\u{301}"
    );
    assert_eq!(edit.cursor, "\u{301}".len());

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &document_before);
    Ok(())
}

#[test]
fn math_text_selection_rejects_an_offset_inside_a_cross_flow_grapheme() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, "\\text{a}\\text{\u{301}}")?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let first = path::root_text_flow_path(tree, true, None).ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(first, 0);
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();

    assert!(matches!(
        session.run(Command::SetMathTextSelection {
            anchor: 1,
            focus: 1,
        }),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.document_generation(), generation_before);
    Ok(())
}

#[test]
fn parsed_math_text_uses_the_same_grapheme_editing_as_tree_input() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, "xy")?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));

    session.run(Command::MoveCaret(Direction::Backward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(edit.cursor, 1);
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(inline_source(&session).as_deref(), Some("y"));
    Ok(())
}

#[test]
fn tree_operator_combining_mark_is_one_editable_grapheme() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("+".to_owned()))?;
    session.run(Command::InsertText("\u{0301}".to_owned()))?;
    let (paragraph_id, math_id) = {
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        (paragraph_id.clone(), math_id.clone())
    };
    let tree = &find_inline_math_in_blocks(&session.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree;
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(
        literal_symbol_text(&tree.root.content[0]),
        Some("+\u{0301}")
    );
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    session.run(Command::Delete(Direction::Backward))?;
    assert!(inline_source(&session).is_some_and(|source| source.is_empty()));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some("+\u{0301}"));
    session.run(Command::Redo)?;
    assert!(inline_source(&session).is_some_and(|source| source.is_empty()));
    Ok(())
}

#[test]
fn tree_literal_prepend_merge_updates_atom_class_and_round_trips() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("+".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::InsertText("\u{0600}".to_owned()))?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &edit.target
    else {
        return Err(EditError::InvalidPosition);
    };
    let tree = &find_inline_math_in_blocks(&session.document().body, paragraph_id, math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree;
    assert_eq!(tree.root.content.len(), 1);
    let MathContent::Math(symbol) = &tree.root.content[0] else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(symbol.text.as_deref(), Some("\u{0600}+"));
    assert_eq!(
        symbol.attributes.get("atomClass").map(String::as_str),
        Some("ord")
    );

    let serialized = caret_math::serialize_tree(tree);
    let parsed =
        caret_math::parse(&serialized, caret_math::MathInputKind::Inline).map_err(test_error)?;
    let reparsed = match parsed {
        caret_math::ParsedMath::Tree(tree) => tree,
        caret_math::ParsedMath::Align(_) => return Err(EditError::InvalidPosition),
    };
    let MathContent::Math(reparsed_symbol) = &reparsed.root.content[0] else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(reparsed_symbol.text.as_deref(), Some("\u{0600}+"));
    assert_eq!(
        reparsed_symbol
            .attributes
            .get("atomClass")
            .map(String::as_str),
        Some("ord")
    );
    Ok(())
}

#[test]
fn tree_native_literal_symbols_join_combining_and_regional_graphemes() -> Result<(), EditError> {
    for (first, second, expected) in [("a", "\u{301}", "a\u{301}"), ("🇦", "🇧", "🇦🇧")] {
        let mut session = session_with_text("").map_err(test_error)?;
        session.begin_inline_math()?;
        session.run(Command::InsertText(first.to_owned()))?;
        session.run(Command::InsertText(second.to_owned()))?;
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &session
            .math_edit()
            .ok_or(EditError::InvalidPosition)?
            .target
        else {
            return Err(EditError::InvalidPosition);
        };
        let tree = find_inline_math_in_blocks(&session.document().body, paragraph_id, math_id)
            .ok_or(EditError::InvalidPosition)?
            .math_tree
            .clone();
        assert_eq!(tree.root.content.len(), 1);
        assert_eq!(literal_symbol_text(&tree.root.content[0]), Some(expected));
        session.run(Command::MoveCaret(Direction::Backward))?;
        assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
        session.run(Command::MoveCaret(Direction::Forward))?;
        assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
        session.run(Command::Delete(Direction::Backward))?;
        assert!(inline_source(&session).is_some_and(|source| source.is_empty()));
    }
    Ok(())
}

#[test]
fn regional_indicator_resegmentation_keeps_tree_caret_and_history_consistent()
-> Result<(), EditError> {
    let mut insertion = session_with_text("").map_err(test_error)?;
    insertion.begin_inline_math()?;
    insertion.run(Command::InsertText("🇦🇧".to_owned()))?;
    insertion.run(Command::MoveCaret(Direction::Backward))?;
    insertion.run(Command::InsertText("🇨".to_owned()))?;

    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &insertion
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .target
    else {
        return Err(EditError::InvalidPosition);
    };
    let paragraph_id = paragraph_id.clone();
    let math_id = math_id.clone();
    let tree = find_inline_math_in_blocks(&insertion.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    let symbols = tree
        .root
        .content
        .iter()
        .filter_map(literal_symbol_text)
        .collect::<Vec<_>>();
    assert_eq!(symbols, ["🇨🇦", "🇧"]);
    assert_eq!(insertion.math_edit().map(|edit| edit.cursor), Some(1));

    insertion.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(insertion.math_edit().map(|edit| edit.cursor), Some(0));
    insertion.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(insertion.math_edit().map(|edit| edit.cursor), Some(1));
    insertion.run(Command::Undo)?;
    let tree = find_inline_math_in_blocks(&insertion.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(literal_symbol_text(&tree.root.content[0]), Some("🇦🇧"));
    insertion.run(Command::Redo)?;
    let tree = find_inline_math_in_blocks(&insertion.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(literal_symbol_text)
            .collect::<Vec<_>>(),
        ["🇨🇦", "🇧"]
    );

    let mut attached_mark = session_with_text("").map_err(test_error)?;
    attached_mark.begin_inline_math()?;
    attached_mark.run(Command::InsertText("🇦🇧\u{301}".to_owned()))?;
    attached_mark.run(Command::MoveCaret(Direction::Backward))?;
    attached_mark.run(Command::InsertText("🇨".to_owned()))?;
    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &attached_mark
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .target
    else {
        return Err(EditError::InvalidPosition);
    };
    let tree = find_inline_math_in_blocks(&attached_mark.document().body, paragraph_id, math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(literal_symbol_text)
            .collect::<Vec<_>>(),
        ["🇨🇦", "🇧\u{301}"]
    );
    assert_eq!(attached_mark.math_edit().map(|edit| edit.cursor), Some(1));

    let mut deletion = session_with_text("").map_err(test_error)?;
    deletion.begin_inline_math()?;
    deletion.run(Command::InsertText("🇦x🇧".to_owned()))?;
    deletion.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(deletion.math_edit().map(|edit| edit.cursor), Some(2));
    deletion.run(Command::Delete(Direction::Backward))?;

    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &deletion
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .target
    else {
        return Err(EditError::InvalidPosition);
    };
    let paragraph_id = paragraph_id.clone();
    let math_id = math_id.clone();
    let tree = find_inline_math_in_blocks(&deletion.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(literal_symbol_text(&tree.root.content[0]), Some("🇦🇧"));
    assert_eq!(deletion.math_edit().map(|edit| edit.cursor), Some(1));
    deletion.run(Command::Undo)?;
    let tree = find_inline_math_in_blocks(&deletion.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(literal_symbol_text)
            .collect::<Vec<_>>(),
        ["🇦", "x", "🇧"]
    );
    deletion.run(Command::Redo)?;
    let tree = find_inline_math_in_blocks(&deletion.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(literal_symbol_text(&tree.root.content[0]), Some("🇦🇧"));
    Ok(())
}

#[test]
fn regional_indicator_resegmentation_rejects_opaque_node_changes_atomically()
-> Result<(), EditError> {
    let mut insertion = session_with_text("").map_err(test_error)?;
    insertion.begin_inline_math()?;
    insertion.run(Command::InsertText("🇦🇧".to_owned()))?;
    let (paragraph_id, math_id) = {
        let edit = insertion.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        (paragraph_id.clone(), math_id.clone())
    };
    let paragraph = insertion
        .document
        .body
        .iter_mut()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) if paragraph.id == paragraph_id => Some(paragraph),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let Some(MathContent::Math(opaque)) = math.math_tree.root.content.first_mut() else {
        return Err(EditError::InvalidPosition);
    };
    opaque
        .extra
        .insert("futureSymbolValue".to_owned(), serde_json::json!(true));
    insertion.run(Command::MoveCaret(Direction::Backward))?;

    let document_before = insertion.document().clone();
    let edit_before = insertion.math_edit().cloned();
    let selection_before = insertion.selection().clone();
    let generation_before = insertion.document_generation();
    let history_before = insertion.history_state_id();
    assert_eq!(
        insertion.run(Command::InsertText("🇨".to_owned())),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(insertion.document(), &document_before);
    assert_eq!(insertion.math_edit(), edit_before.as_ref());
    assert_eq!(insertion.selection(), &selection_before);
    assert_eq!(insertion.document_generation(), generation_before);
    assert_eq!(insertion.history_state_id(), history_before);

    let mut deletion = session_with_text("").map_err(test_error)?;
    deletion.begin_inline_math()?;
    deletion.run(Command::InsertText("🇨x🇦🇧".to_owned()))?;
    let (paragraph_id, math_id) = {
        let edit = deletion.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        (paragraph_id.clone(), math_id.clone())
    };
    let paragraph = deletion
        .document
        .body
        .iter_mut()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) if paragraph.id == paragraph_id => Some(paragraph),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let Some(MathContent::Math(opaque)) = math
        .math_tree
        .root
        .content
        .iter_mut()
        .find(|content| symbol_text(content).is_some_and(|text| text == "🇦🇧"))
    else {
        return Err(EditError::InvalidPosition);
    };
    opaque
        .extra
        .insert("futureSymbolValue".to_owned(), serde_json::json!(true));
    deletion.run(Command::MoveCaret(Direction::Backward))?;

    let document_before = deletion.document().clone();
    let edit_before = deletion.math_edit().cloned();
    let selection_before = deletion.selection().clone();
    let generation_before = deletion.document_generation();
    let history_before = deletion.history_state_id();
    assert_eq!(
        deletion.run(Command::Delete(Direction::Backward)),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(deletion.document(), &document_before);
    assert_eq!(deletion.math_edit(), edit_before.as_ref());
    assert_eq!(deletion.selection(), &selection_before);
    assert_eq!(deletion.document_generation(), generation_before);
    assert_eq!(deletion.history_state_id(), history_before);
    Ok(())
}

#[test]
fn tree_symbol_deletion_resegments_combining_graphemes_and_history() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("e+".to_owned()))?;
    append_test_literal_symbol(&mut session, "\u{301}")?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    let before_deletion = session.document().clone();

    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::Delete(Direction::Forward))?;

    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .target
    else {
        return Err(EditError::InvalidPosition);
    };
    let paragraph_id = paragraph_id.clone();
    let math_id = math_id.clone();
    let tree = find_inline_math_in_blocks(&session.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(literal_symbol_text(&tree.root.content[0]), Some("e\u{301}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));

    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    session.run(Command::Undo)?;
    assert_eq!(session.document(), &before_deletion);
    session.run(Command::Redo)?;
    let tree = find_inline_math_in_blocks(&session.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(literal_symbol_text(&tree.root.content[0]), Some("e\u{301}"));

    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::Delete(Direction::Backward))?;
    assert!(inline_source(&session).is_some_and(|source| source.is_empty()));
    session.run(Command::Undo)?;
    let tree = find_inline_math_in_blocks(&session.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(literal_symbol_text(&tree.root.content[0]), Some("e\u{301}"));
    session.run(Command::Redo)?;
    assert!(inline_source(&session).is_some_and(|source| source.is_empty()));
    Ok(())
}

#[test]
fn tree_symbol_deletion_rejects_opaque_resegmentation_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("e+".to_owned()))?;
    append_test_literal_symbol(&mut session, "\u{301}")?;
    let (paragraph_id, math_id) = {
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        (paragraph_id.clone(), math_id.clone())
    };
    let paragraph = session
        .document
        .body
        .iter_mut()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) if paragraph.id == paragraph_id => Some(paragraph),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let Some(MathContent::Math(opaque)) = math.math_tree.root.content.get_mut(2) else {
        return Err(EditError::InvalidPosition);
    };
    opaque
        .extra
        .insert("futureSymbolValue".to_owned(), serde_json::json!(true));

    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let selection_before = session.selection().clone();
    let generation_before = session.document_generation();
    let history_before = session.history_state_id();
    assert_eq!(
        session.run(Command::Delete(Direction::Forward)),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_before);
    Ok(())
}

#[test]
fn tree_native_large_regional_indicator_input_is_batched() -> Result<(), EditError> {
    const REGIONAL_INDICATOR_COUNT: usize = 2_048;
    let source = "🇦".repeat(REGIONAL_INDICATOR_COUNT);
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText(source))?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &edit.target
    else {
        return Err(EditError::InvalidPosition);
    };
    let tree = find_inline_math_in_blocks(&session.document().body, paragraph_id, math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), REGIONAL_INDICATOR_COUNT / 2);
    assert!(
        tree.root
            .content
            .iter()
            .all(|content| literal_symbol_text(content) == Some("🇦🇦"))
    );
    Ok(())
}

#[test]
fn tree_grapheme_merge_preserves_unknown_symbol_metadata_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("👩👩".to_owned()))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &edit.target
    else {
        return Err(EditError::InvalidPosition);
    };
    let paragraph_id = paragraph_id.clone();
    let math_id = math_id.clone();
    let paragraph = session
        .document
        .body
        .iter_mut()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) if paragraph.id == paragraph_id => Some(paragraph),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let Some(MathContent::Math(right)) = math.math_tree.root.content.get_mut(1) else {
        return Err(EditError::InvalidPosition);
    };
    right
        .extra
        .insert("futureSymbolValue".to_owned(), serde_json::json!(true));
    session.run(Command::MoveCaret(Direction::Backward))?;

    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();
    let history_before = session.history_state_id();
    assert_eq!(
        session.run(Command::InsertText("\u{200d}".to_owned())),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_before);
    Ok(())
}

#[test]
fn tree_grapheme_merge_keeps_command_symbols_atomic() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\%".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let (paragraph_id, math_id) = {
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert!(matches!(edit.branch, MathEditBranch::Tree { .. }));
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        (paragraph_id.clone(), math_id.clone())
    };
    let symbol_id = find_inline_math_in_blocks(&session.document().body, &paragraph_id, &math_id)
        .and_then(|math| math.math_tree.root.content.first())
        .and_then(|content| match content {
            MathContent::Math(node) => Some(node.id.clone()),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;

    session.run(Command::InsertText("\u{301}".to_owned()))?;
    let math = find_inline_math_in_blocks(&session.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(math.math_tree.root.content.len(), 2);
    assert!(matches!(
        math.math_tree.root.content.first(),
        Some(MathContent::Math(node)) if node.id == symbol_id && node.text.as_deref() == Some("%")
    ));
    assert!(matches!(
        math.math_tree.root.content.get(1),
        Some(MathContent::Math(node)) if node.text.as_deref() == Some("\u{301}")
    ));
    Ok(())
}

#[test]
fn tree_native_backspace_at_subsup_sibling_end_deletes_only_the_last_symbol()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.math_insert_template(r"sada_{a}^{}a")?;
    let edit = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Tree { anchor, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let path = MathSequencePath::root();
    let position = MathTreePosition {
        host: anchor.host.clone(),
        path: path.clone(),
        offset: 5,
        affinity: MathPositionAffinity::Downstream,
    };
    edit.cursor = position.offset;
    edit.branch = MathEditBranch::Tree {
        path,
        anchor: position,
    };

    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"sada_{a}^{}"));
    Ok(())
}

#[test]
fn tree_native_mod_backspace_round_trips_escaped_graphemes_and_literal_apostrophe()
-> Result<(), EditError> {
    for input in [
        "\u{600}#x'",
        "#\u{301}",
        "'\u{301}",
        "%\u{301}",
        "&\u{301}",
        "$\u{301}",
        "_\u{301}",
        "{\u{301}",
        "}\u{301}",
        "\\\u{301}",
        "~\u{301}",
        "^\u{301}",
    ] {
        let mut session = session_with_text("").map_err(test_error)?;
        session.begin_inline_math()?;
        session.run(Command::InsertText(input.to_owned()))?;
        let source = inline_source(&session).ok_or(EditError::InvalidPosition)?;
        let parsed =
            caret_math::parse(&source, caret_math::MathInputKind::Inline).map_err(test_error)?;
        assert!(matches!(parsed, caret_math::ParsedMath::Tree(_)));

        session.run(Command::EditorKey(EditorKey::ModBackspace))?;
        assert!(session.math_edit().is_none());
        assert_eq!(first_paragraph_text(&session), source);
    }
    Ok(())
}

#[test]
fn tree_boundary_delete_removes_one_text_grapheme_and_preserves_text_metadata()
-> Result<(), EditError> {
    let Block::Equation(mut equation) = display_math_block(false, r"x+\text{abc}+y")? else {
        return Err(EditError::InvalidPosition);
    };
    let text_index = equation
        .math_tree
        .root
        .content
        .iter()
        .position(|content| matches!(content, MathContent::Text(_)))
        .ok_or(EditError::InvalidPosition)?;
    let Some(MathContent::Text(text)) = equation.math_tree.root.content.get_mut(text_index) else {
        return Err(EditError::InvalidPosition);
    };
    text.extra.insert(
        "vendorTextFlow".to_owned(),
        serde_json::json!({"keep": true}),
    );
    let equation_id = equation.id.clone();
    let mut document = Document::blank();
    document.body.push(Block::Equation(equation));
    let mut session = Session::new(document)?;
    let position = MathTreePosition {
        host: MathPositionHost::Equation {
            owner_id: equation_id.clone(),
        },
        path: MathSequencePath::root(),
        offset: text_index,
        affinity: MathPositionAffinity::Downstream,
    };
    session.math_edit = Some(MathEdit {
        target: MathEditTarget::Block {
            block_id: equation_id.clone(),
            align: false,
        },
        source_projection: None,
        source_draft: None,
        revision: session.current_math_edit_revision(),
        cursor: text_index,
        branch: MathEditBranch::Tree {
            path: MathSequencePath::root(),
            anchor: position,
        },
    });

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    let text_path = tree_position::text_path_for_content(
        match &edit.branch {
            MathEditBranch::Tree { anchor, .. } => anchor,
            _ => return Err(EditError::InvalidPosition),
        },
        text_index,
        None,
    );
    assert!(text_path.resolve_structurally(tree).is_some());
    assert!(path::contiguous_text_flow_text(tree, &text_path).is_some());

    session.run(Command::Delete(Direction::Forward))?;
    let Some(Block::Equation(updated)) = find_block_by_id(&session.document().body, &equation_id)
    else {
        return Err(EditError::InvalidPosition);
    };
    let Some(MathContent::Text(updated_text)) = updated.math_tree.root.content.get(text_index)
    else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(
        updated_text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "bc"
    );
    assert_eq!(
        updated_text.extra["vendorTextFlow"],
        serde_json::json!({"keep": true})
    );
    assert_eq!(
        caret_math::serialize_tree(&updated.math_tree),
        r"x+\text{bc}+y"
    );
    Ok(())
}

#[test]
fn text_island_escapes_literal_tex_characters_and_keeps_atomic_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::InsertText("%  \t\u{00a0}".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{abc\%\textchar{20}\textchar{20}\textchar{9}~}")
    );
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    let text = path
        .resolve_structurally(&equation.math_tree)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abc%  \t\u{00a0}"
    );
    assert_eq!(
        session.math_edit().map(|edit| edit.cursor),
        Some("abc%  \t\u{00a0}".len())
    );

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abc}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::Redo)?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{abc\%\textchar{20}\textchar{20}\textchar{9}~}")
    );
    Ok(())
}

#[test]
fn text_input_is_not_limited_by_latex_source_budget() -> Result<(), EditError> {
    let prefix = r"\text{";
    let suffix = "}";
    let content_len = caret_math::MAX_MATH_SOURCE_BYTES - prefix.len() - suffix.len() - 1;
    let source = format!("{prefix}{}{suffix}", "a".repeat(content_len));
    assert_eq!(source.len(), caret_math::MAX_MATH_SOURCE_BYTES - 1);

    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, &source)?;
    move_into_math_text(&mut session)?;
    session.run(Command::InsertText("%".to_owned()))?;
    assert!(
        inline_source(&session)
            .ok_or(EditError::InvalidPosition)?
            .len()
            > caret_math::MAX_MATH_SOURCE_BYTES
    );
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();
    let selection_before = session.selection().clone();
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.document_generation(), generation_before);
    assert_ne!(session.selection(), &selection_before);
    assert!(edit_before.is_some());

    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Text { .. })
    }));
    let text_before_reentry_insert = active_math_text_flow(&session)?
        .runs
        .iter()
        .map(|run| run.text.len())
        .sum::<usize>();
    session.run(Command::InsertText("z".to_owned()))?;
    let reentered_text = active_math_text_flow(&session)?
        .runs
        .iter()
        .map(|run| run.text.len())
        .sum::<usize>();
    assert_eq!(reentered_text, text_before_reentry_insert + 1);
    Ok(())
}

#[test]
fn math_text_command_line_can_split_out_inline_math() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;

    session.run(Command::BeginCommandLine)?;
    assert_eq!(
        session.command_line().map(|line| line.mode),
        Some(CommandLineMode::Math)
    );
    session.run(Command::TypeCommandLine("math".to_owned()))?;
    let candidate = session
        .command_line_candidates()
        .into_iter()
        .find(|candidate| candidate.command_id == "math.split_inline")
        .ok_or(EditError::InvalidPosition)?;
    assert!(candidate.available);
    session.run(Command::AcceptCommandLine)?;

    assert!(session.command_line().is_none());
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { path, .. }) if path.steps().is_empty()
    ));
    let Block::Equation(equation) = session
        .document()
        .body
        .first()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    assert!(matches!(
        equation.math_tree.root.content.as_slice(),
        [
            MathContent::Text(left),
            MathContent::Math(node),
            MathContent::Text(right),
        ] if text_flow_display(left) == "a"
            && node.kind == MathNodeKind::Placeholder
            && text_flow_display(right) == "b"
    ));

    session.run(Command::Undo)?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Text { .. }))
    );
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "ab"
    );
    session.run(Command::Redo)?;
    assert!(session.math_edit().is_some_and(|edit| {
        matches!(edit.branch, MathEditBranch::Tree { .. })
            && session.math_tree_for_edit(edit, None).is_ok_and(|tree| {
                tree.root.content.iter().any(|content| {
                    matches!(
                        content,
                        MathContent::Math(node) if node.kind == MathNodeKind::Placeholder
                    )
                })
            })
    }));
    Ok(())
}

#[test]
fn math_split_command_is_unavailable_for_cross_text_flow_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{ab}+\text{cd}")?;
    move_into_math_text(&mut session)?;

    let Block::Equation(equation) = session
        .document()
        .body
        .first()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = equation.math_tree.clone();
    let edit = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, anchor, .. } = &mut edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let other_steps = if path.steps() == [MathTextFlowPathStep::Content(0)] {
        vec![MathTextFlowPathStep::Content(2)]
    } else {
        vec![MathTextFlowPathStep::Content(0)]
    };
    anchor.path = MathTextFlowPath::new(other_steps, path.align_cell());
    assert!(anchor.path.resolve_structurally(&tree).is_some());

    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("math".to_owned()))?;
    let candidate = session
        .command_line_candidates()
        .into_iter()
        .find(|candidate| candidate.command_id == "math.split_inline")
        .ok_or(EditError::InvalidPosition)?;
    assert!(!candidate.available);
    Ok(())
}

#[test]
fn math_text_math_commandline_templates_enter_the_nested_math_node()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("frac".to_owned()))?;
    session.run(Command::AcceptCommandLine)?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(matches!(
        &edit.branch,
        MathEditBranch::Tree { path, .. }
            if matches!(path.steps(), [MathSequenceStep::Operand { operand_index: 0, .. }])
    ));
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(
        caret_math::serialize_tree(&equation.math_tree),
        r"\text{a}\frac{}{}\text{b}"
    );
    assert_eq!(edit.cursor, 0);
    session.run(Command::InsertText("1".to_owned()))?;
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(
        caret_math::serialize_tree(&equation.math_tree),
        r"\text{a}\frac{1}{}\text{b}"
    );
    session.run(Command::Undo)?;
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(
        caret_math::serialize_tree(&equation.math_tree),
        r"\text{a}\frac{}{}\text{b}"
    );
    session.run(Command::Undo)?;
    assert!(session.math_edit().is_some_and(|edit| {
        matches!(edit.branch, MathEditBranch::Text { .. }) && edit.source_projection.is_none()
    }));
    assert!(matches!(
        session.document().body.first(),
        Some(Block::Equation(equation))
            if caret_math::serialize_tree(&equation.math_tree) == r"\text{ab}"
    ));
    Ok(())
}

#[test]
fn math_text_command_line_double_backslash_inserts_literal_backslash()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{a}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("\\".to_owned()))?;
    assert!(session.command_line_candidates().is_empty());
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    let flow = active_math_text_flow(&session)?;
    assert!(flow.runs.iter().any(|run| run.text.contains('\\')));
    Ok(())
}

#[test]
fn pending_math_text_formats_continue_across_sequential_input()
-> Result<(), Box<dyn std::error::Error>> {
    let mut bold = session_with_text("")?;
    insert_equation_text(&mut bold, r"\text{a}")?;
    move_into_math_text(&mut bold)?;
    bold.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    bold.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    bold.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))?;
    for character in ["b", "c", "d"] {
        bold.run(Command::InsertText(character.to_owned()))?;
    }
    assert_eq!(
        inline_source(&bold).as_deref(),
        Some(r"\text{a\textbf{\textit{bcd}}}")
    );

    let mut disable = session_with_text("")?;
    insert_equation_text(&mut disable, r"\text{\textbf{a}}")?;
    move_into_math_text(&mut disable)?;
    disable.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    disable.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    for character in ["b", "c"] {
        disable.run(Command::InsertText(character.to_owned()))?;
    }
    assert_eq!(
        active_math_text_bold_chars(&disable)?,
        vec![('a', true), ('b', false), ('c', false)]
    );
    let serialized = inline_source(&disable).ok_or(EditError::InvalidPosition)?;
    let mut reopened = session_with_text("")?;
    insert_equation_text(&mut reopened, &serialized)?;
    move_into_math_text(&mut reopened)?;
    assert_eq!(
        active_math_text_bold_chars(&reopened)?,
        active_math_text_bold_chars(&disable)?
    );

    bold.run(Command::Undo)?;
    assert_eq!(
        inline_source(&bold).as_deref(),
        Some(r"\text{a\textbf{\textit{bc}}}")
    );
    bold.run(Command::Redo)?;
    assert_eq!(
        inline_source(&bold).as_deref(),
        Some(r"\text{a\textbf{\textit{bcd}}}")
    );
    Ok(())
}

#[test]
fn tree_native_insert_snaps_after_a_grapheme_join() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, "\\text{\u{301}}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 0,
    })?;
    session.run(Command::InsertText("a".to_owned()))?;

    let flow = active_math_text_flow(&session)?;
    let text = flow
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    assert_eq!(text, "a\u{301}");
    let cursor = session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .cursor;
    assert_eq!(cursor, text.len());
    assert!(is_grapheme_boundary(&text, cursor));
    Ok(())
}

#[test]
fn regional_indicator_insert_uses_the_text_island_for_caret_and_formatting()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{🇦}\text{🇧🇨}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 4,
    })?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::InsertText("🇩🇪".to_owned()))?;

    let source = inline_source(&session).unwrap_or_default();
    assert!(
        source.contains(r"\text{🇦🇧\textbf{🇩🇪}🇨}"),
        "source: {source}"
    );
    let flow = active_math_text_flow(&session)?;
    assert_eq!(
        flow.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "🇩🇪"
    );
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(8));

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{🇦🇧🇨}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(4));
    session.run(Command::Redo)?;
    assert!(
        inline_source(&session)
            .is_some_and(|source| { source.contains(r"\text{🇦🇧\textbf{🇩🇪}🇨}") })
    );
    Ok(())
}

#[test]
fn regional_indicator_pairing_uses_text_island_context_for_navigation_and_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{🇦}\text{🇧🇨🇩}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 4,
    })?;

    // 完整文本岛中，🇦🇧 后的合法位置落在第二个流局部的 🇧🇨 配对内部。
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                })
            if path.steps() == [MathTextFlowPathStep::Content(0)]
    ));

    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(4));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                })
            if path.steps() == [MathTextFlowPathStep::Content(1)]
    ));
    Ok(())
}

#[test]
fn run_property_at_cross_flow_regional_indicator_boundary_targets_only_inserted_text()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{🇦}\text{🇧🇨}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 4,
    })?;
    session.run(Command::ToggleRunUnderline)?;
    session.run(Command::InsertText("x".to_owned()))?;

    let flow = active_math_text_flow(&session)?;
    assert!(flow.runs.iter().any(|run| {
        run.text == "x" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    assert!(
        flow.runs
            .iter()
            .all(|run| run.text != "🇨" || !run.props.contains_key("underline"))
    );

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{🇦🇧🇨}"));
    session.run(Command::Redo)?;
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "x" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    Ok(())
}

#[test]
fn run_property_selection_uses_text_island_regional_indicator_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{🇦}\text{🇧🇨}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 8,
    })?;
    session.run(Command::ToggleRunUnderline)?;

    let flow = active_math_text_flow(&session)?;
    assert!(flow.runs.iter().any(|run| {
        run.text == "🇨" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    session.run(Command::Undo)?;
    session.run(Command::Redo)?;
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "🇨" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    Ok(())
}

#[test]
fn math_split_inline_accepts_text_island_regional_indicator_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{🇦}\text{🇧🇨}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 8,
    })?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;

    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { path, .. }) if path.steps().is_empty()
    ));
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert!(
        equation
            .math_tree
            .root
            .content
            .iter()
            .any(|content| { math_content_has_symbol_text(content, "x") })
    );
    Ok(())
}

#[test]
fn deleting_a_regional_indicator_grapheme_across_textflows_is_undoable()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{🇦}\text{🇧🇨🇩}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{🇨🇩}"));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{🇦🇧🇨🇩}"));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{🇨🇩}"));
    Ok(())
}

#[test]
fn math_text_selection_replaces_a_grapheme_spanning_textflows()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{🇦}\text{🇧🇨}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "🇦🇧🇨");
    let anchor = MathTextPosition::new(island.paths[0].clone(), 0, MathPositionAffinity::Upstream);
    let focus = MathTextPosition::new(island.paths[1].clone(), 4, MathPositionAffinity::Downstream);
    session.set_math_text_selection_positions(anchor, focus)?;
    session.run(Command::InsertText("X".to_owned()))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    assert_eq!(
        path::contiguous_text_flow_text(tree, path)
            .ok_or(EditError::InvalidPosition)?
            .display,
        "X🇨"
    );
    session.run(Command::Undo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    assert_eq!(
        path::contiguous_text_flow_text(tree, path)
            .ok_or(EditError::InvalidPosition)?
            .display,
        "🇦🇧🇨"
    );
    Ok(())
}

#[test]
fn math_text_selection_rejects_a_partial_crossflow_grapheme_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{🇦}\text{🇧🇨}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    let before_history = session.history_state_id();
    assert_eq!(
        session.set_math_text_selection_positions(
            MathTextPosition::new(island.paths[0].clone(), 0, MathPositionAffinity::Upstream,),
            MathTextPosition::new(island.paths[1].clone(), 0, MathPositionAffinity::Downstream,),
        ),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    assert_eq!(session.history_state_id(), before_history);
    Ok(())
}

#[test]
fn equation_text_delete_snaps_after_graphemes_join() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "equation")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("\\text{🇦x🇧}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(4));
    session.run(Command::Delete(Direction::Forward))?;

    assert_eq!(inline_source(&session).as_deref(), Some("\\text{🇦🇧}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    Ok(())
}

#[test]
fn cross_run_grapheme_delete_preserves_unknown_run_fields_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, "\\text{a\u{0301}}")?;
    move_into_math_text(&mut session)?;
    let equation = match session.document.body.first_mut() {
        Some(Block::Equation(equation)) => equation,
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let text =
        first_math_text_flow_mut(&mut equation.math_tree.root).ok_or(EditError::InvalidPosition)?;
    text.runs[0].text = "a".to_owned();
    text.runs.push(caret_doc::Run {
        text: "\u{0301}".to_owned(),
        language: None,
        props: Default::default(),
        extra: std::collections::BTreeMap::from([(
            "vendorRun".to_owned(),
            serde_json::Value::Bool(true),
        )]),
    });
    let before = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();

    assert_eq!(
        session.run(Command::Delete(Direction::Backward)),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn math_command_inserts_inline_math_and_sub_commands_edit_it()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("a")?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Backward)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Inline { .. })
    ));
    session.run(Command::InsertText("x^2".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("x^{2}"));
    session.run(Command::InsertText("}".to_owned()))?;
    session.run(Command::BeginCommandLine)?;
    assert_eq!(
        session.command_line().map(|line| line.mode),
        Some(CommandLineMode::Math)
    );
    session.run(Command::TypeCommandLine("frac".to_owned()))?;
    session.run(Command::AcceptCommandLine)?;
    assert_eq!(inline_source(&session).as_deref(), Some("x^{2}\\frac{}{}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(11));
    session.run(Command::InsertText("1".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::InsertText("n".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some("x^{2}\\frac{1}{n}")
    );
    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.offset, 4);
    // 撤销是正文命令：退出公式编辑后按步恢复源码。
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some("x^{2}\\frac{1}{}"));
    Ok(())
}

#[test]
fn undo_and_redo_from_math_command_query_restore_formula_editing() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    accept(&mut session, "math").map_err(test_error)?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("frac".to_owned()))?;

    session.run(Command::Undo)?;
    assert!(session.command_line().is_none());
    assert_eq!(session.math_edit().map(source_text), Some(""));
    assert_eq!(inline_source(&session).as_deref(), Some(""));

    session.run(Command::Redo)?;
    assert!(session.command_line().is_none());
    assert_eq!(session.math_edit().map(source_text), Some("x"));
    assert_eq!(inline_source(&session).as_deref(), Some("x"));
    Ok(())
}

#[test]
fn stale_math_edit_revision_rejects_input_without_losing_the_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let document_before = session.document().clone();
    let revision = session.current_math_edit_revision();
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision
        .document_generation += 1;
    let stale_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let stale_revision = stale_edit.revision.clone();

    assert!(matches!(
        session.run(Command::InsertText("y".to_owned())),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), Some(&stale_edit));
    assert_eq!(
        session.math_edit().map(|edit| edit.revision.clone()),
        Some(stale_revision)
    );

    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision = revision;
    session.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("xy"));
    assert_eq!(
        session.math_edit().map(|edit| edit.revision.clone()),
        Some(session.current_math_edit_revision())
    );
    Ok(())
}

#[test]
fn stale_math_edit_escape_closes_command_query_before_abandoning_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::BeginCommandLine)?;
    let document_before = session.document().clone();
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision
        .document_generation += 1;

    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.command_line().is_none());
    let stale_edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_ne!(stale_edit.revision, session.current_math_edit_revision());
    assert_eq!(session.document(), &document_before);

    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.document(), &document_before);
    Ok(())
}

#[test]
fn stale_math_edit_rejects_command_line_math_template_without_mutating_state()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("frac".to_owned()))?;
    let document_before = session.document().clone();
    let command_line_before = session.command_line().cloned();
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision
        .document_generation += 1;
    let stale_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;

    assert!(matches!(
        session.run_command_line_with_font_choices(
            Command::AcceptCommandLine,
            Vec::new(),
            Vec::new()
        ),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.command_line(), command_line_before.as_ref());
    assert_eq!(session.math_edit(), Some(&stale_edit));
    Ok(())
}

#[test]
fn align_command_template_backspace_removes_empty_construct()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    let block_id = match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .target
    {
        MathEditTarget::Block {
            block_id,
            align: true,
        } => block_id.clone(),
        _ => return Err(EditError::InvalidPosition.into()),
    };
    session.run(Command::InsertText("x& ".to_owned()))?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("sqrt".to_owned()))?;
    session.run(Command::AcceptCommandLine)?;

    let inserted = session
        .math_edit()
        .map(source_text)
        .ok_or(EditError::InvalidPosition)?;
    assert!(inserted.contains(r"\sqrt{}"));
    assert!(inserted.starts_with("x& "));
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    assert!(!session.has_uncommitted_math_source_draft());
    let before_delete_generation = session.document_generation();
    let deletion = session.run(Command::Delete(Direction::Backward))?;
    assert!(deletion.document_changed);
    assert!(session.document_generation() > before_delete_generation);
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    let after_delete = session
        .math_edit()
        .map(source_text)
        .ok_or(EditError::InvalidPosition)?;
    assert!(!after_delete.contains(r"\sqrt"));
    assert!(after_delete.starts_with("x& "));
    let deleted_align = first_align(&session)?;
    assert!(!caret_math::serialize_tree(&deleted_align.rows[0].cells[1]).contains(r"\sqrt"));
    session.run(Command::Undo)?;
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| source_text(edit).contains(r"\sqrt{}"))
    );
    let restored_align = first_align(&session)?;
    assert!(caret_math::serialize_tree(&restored_align.rows[0].cells[1]).contains(r"\sqrt"));
    session.run(Command::Redo)?;
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| !source_text(edit).contains(r"\sqrt"))
    );
    let redone_align = first_align(&session)?;
    assert!(!caret_math::serialize_tree(&redone_align.rows[0].cells[1]).contains(r"\sqrt"));
    Ok(())
}

#[test]
fn greek_sub_commands_are_case_sensitive() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    accept(&mut session, "Gamma")?;
    accept(&mut session, "gamma")?;
    assert_eq!(inline_source(&session).as_deref(), Some("\\Gamma\\gamma"));
    Ok(())
}

#[test]
fn repeated_control_word_commands_do_not_consume_tree_depth()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    for _ in 0..=caret_doc::MAX_MATH_TREE_DEPTH {
        accept(&mut session, "alpha")?;
    }
    session.document().validate()?;
    assert_eq!(
        inline_source(&session)
            .as_deref()
            .map(|source| source.matches(r"\alpha").count()),
        Some(caret_doc::MAX_MATH_TREE_DEPTH + 1)
    );
    Ok(())
}

#[test]
fn equation_command_inserts_block_after_text_and_round_trips()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("ab")?;
    accept(&mut session, "equation")?;
    let active_view = session.current_environment_view()?;
    assert_eq!(
        active_view.innermost_block().map(|entry| entry.kind),
        Some(crate::EnvironmentKind::Equation)
    );
    assert_eq!(session.document().body.len(), 3);
    session.run(Command::InsertText("a+b".to_owned()))?;
    let Some(Block::Equation(equation)) = session.document().body.get(1) else {
        panic!("expected equation block");
    };
    assert_eq!(caret_math::serialize_tree(&equation.math_tree), "a+b");
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::EditorKey(EditorKey::Enter))?;
    assert!(session.math_edit().is_none());
    assert_eq!(
        session
            .current_environment_view()?
            .innermost_block()
            .map(|entry| entry.kind),
        None
    );
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    for _ in 0..4 {
        session.run(Command::Delete(Direction::Backward))?;
    }
    assert!(session.math_edit().is_none());
    assert_eq!(session.document().body.len(), 2);
    session.run(Command::Undo)?;
    assert_eq!(session.document().body.len(), 3);
    Ok(())
}

#[test]
fn equation_command_splits_text_at_the_insertion_caret() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("abcd")?;
    let caret = Pos {
        offset: 2,
        ..session.selection().focus.clone()
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: caret.clone(),
        focus: caret,
    })))?;
    accept(&mut session, "equation")?;

    assert_eq!(session.document().body.len(), 3);
    let Some(Block::Paragraph(before)) = session.document().body.first() else {
        panic!("expected paragraph before equation");
    };
    let Some(Block::Equation(_)) = session.document().body.get(1) else {
        panic!("expected equation at caret");
    };
    let Some(Block::Paragraph(after)) = session.document().body.get(2) else {
        panic!("expected paragraph after equation");
    };
    assert_eq!(TextMap::new(before)?.text, "ab");
    assert_eq!(TextMap::new(after)?.text, "cd");

    session.run(Command::Undo)?;
    assert_eq!(session.document().body.len(), 1);
    let Some(Block::Paragraph(restored)) = session.document().body.first() else {
        panic!("undo should restore the original paragraph");
    };
    assert_eq!(TextMap::new(restored)?.text, "abcd");
    Ok(())
}

#[test]
fn backspace_at_a_later_align_textflow_start_deletes_the_previous_text_grapheme()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText(r"\text{a}\text{b}".to_owned()))?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let align_cell = (0, 0);
    let tree = session.math_tree_for_edit(&edit, Some(align_cell))?;
    assert_eq!(tree.root.content.len(), 2);
    let path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(1)], Some(align_cell));
    session.enter_math_text_branch(path, 0);

    session.run(Command::EditorKey(EditorKey::Backspace))?;

    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, Some(align_cell))?;
    assert_eq!(caret_math::serialize_tree(tree), r"\text{b}");
    Ok(())
}

#[test]
fn align_keeps_trailing_space_as_editable_content_for_the_next_input()
-> Result<(), Box<dyn std::error::Error>> {
    for (first, next, expected) in [
        ("x ", "y", "x y"),
        (" ", "y", " y"),
        (r"x\\ ", "y", r"x\\ y"),
    ] {
        let mut session = session_with_text("")?;
        accept(&mut session, "align")?;
        enter_source_edit(&mut session, Direction::Forward)?;
        session.run(Command::InsertText(first.to_owned()))?;
        assert_eq!(session.math_edit().map(source_text), Some(first));
        assert_eq!(
            session.math_edit().map(|edit| edit.cursor),
            Some(first.len())
        );

        session.run(Command::InsertText(next.to_owned()))?;
        assert_eq!(session.math_edit().map(source_text), Some(expected));
        assert_eq!(
            session.math_edit().map(|edit| edit.cursor),
            Some(expected.len())
        );
    }
    Ok(())
}

#[test]
fn rejected_body_command_keeps_the_active_math_edit_target()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    let target_before = session.math_edit().cloned();
    let pending_style_before = session.pending_inline_style.clone();
    let result = session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Font));
    assert!(matches!(result, Err(EditError::UnsupportedInlineFormat)));
    assert_eq!(session.math_edit(), target_before.as_ref());
    assert_eq!(session.pending_inline_style, pending_style_before);
    assert!(matches!(
        session.run(Command::ToggleRunUnderline),
        Err(EditError::UnsupportedInlineFormat)
    ));
    assert!(matches!(
        session.run(Command::SetInlineFontFamily("Noto Serif".to_owned())),
        Err(EditError::UnsupportedInlineFormat)
    ));
    assert_eq!(session.math_edit(), target_before.as_ref());
    assert_eq!(session.pending_inline_style, pending_style_before);

    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("frac".to_owned()))?;
    let command_line_before = session.command_line().cloned();
    let document_before = session.document().clone();
    let history_state_before = session.history_state_id();
    assert_eq!(
        session.run(Command::ToggleRunUnderline),
        Err(EditError::UnsupportedInlineFormat)
    );
    assert_eq!(
        session.run(Command::InsertText("leak".to_owned())),
        Err(EditError::UnknownCommand)
    );
    assert_eq!(session.math_edit(), target_before.as_ref());
    assert_eq!(session.command_line(), command_line_before.as_ref());
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.pending_inline_style, pending_style_before);
    assert_eq!(session.history_state_id(), history_state_before);
    Ok(())
}
