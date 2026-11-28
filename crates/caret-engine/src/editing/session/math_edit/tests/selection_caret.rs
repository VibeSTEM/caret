//! 公式编辑测试：selection caret。

use super::*;

#[test]
fn caret_snapshot_rejects_a_rebuilt_source_projection_in_the_same_generation()
-> Result<(), EditError> {
    let block_id = NodeId::generate();
    let (block, projection) = display_math_block_with_projection(false, "x", block_id.clone())?;
    let mut document = Document::blank();
    document.body.push(block);
    let mut session = Session::new(document)?;
    session.math_edit = Some(MathEdit::editing_source(
        MathEditTarget::Block {
            block_id: block_id.clone(),
            align: false,
        },
        Some(Arc::new(projection)),
        1,
        session.current_math_edit_revision(),
    ));

    let snapshot = session.current_caret_snapshot()?;
    session.validate_caret_snapshot_revision(&snapshot)?;

    let (_, rebuilt_projection) = display_math_block_with_projection(false, "x", block_id)?;
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .replace_source_projection(Some(Arc::new(rebuilt_projection)));
    assert_eq!(
        session.validate_caret_snapshot_revision(&snapshot),
        Err(EditError::InvalidPosition)
    );
    Ok(())
}

#[test]
fn empty_text_flow_boundary_uses_only_a_zero_width_range() {
    assert_eq!(
        super::text_branch::empty_text_source_boundary(Some(8..8), 5),
        Some(3)
    );
    assert_eq!(
        super::text_branch::empty_text_source_boundary(Some(8..9), 5),
        None
    );
    assert_eq!(
        super::text_branch::empty_text_source_boundary(None, 5),
        None
    );
}

#[test]
fn editing_inline_math_source_promotes_new_root_text_and_moves_focus_to_it() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath("ab".to_owned()))?;
    let original_math_id = match session.document().body.first() {
        Some(Block::Paragraph(paragraph)) => match paragraph.inlines.first() {
            Some(ParagraphContent::Math(math)) => math.id.clone(),
            _ => return Err(EditError::InvalidPosition),
        },
        _ => return Err(EditError::InvalidPosition),
    };
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::InsertText(r"\text{x}".to_owned()))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(matches!(
        paragraph.inlines.as_slice(),
        [
            ParagraphContent::Math(_),
            ParagraphContent::Text(_),
            ParagraphContent::Math(_)
        ]
    ));
    assert!(matches!(
        paragraph.inlines.first(),
        Some(ParagraphContent::Math(math)) if math.id == original_math_id
    ));
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x{OBJECT_REPLACEMENT}")
    );
    assert!(session.math_edit().is_none());
    assert_eq!(
        session.selection().focus.offset,
        OBJECT_REPLACEMENT.len_utf8() + 1
    );

    session.run(Command::InsertText("!".to_owned()))?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x!{OBJECT_REPLACEMENT}")
    );
    session.run(Command::Undo)?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x{OBJECT_REPLACEMENT}")
    );
    session.run(Command::Redo)?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x!{OBJECT_REPLACEMENT}")
    );
    Ok(())
}

#[test]
fn root_math_focus_survives_split_undo_redo_and_further_source_edits() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath("b".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::MoveCaret(Direction::Backward))?;

    session.run(Command::InsertText(r"\text{x}a".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("ab"));
    let split_target = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        caret_math::serialize_tree(session.math_tree_for_edit(&split_target, None)?),
        source_text(&split_target)
    );
    let split_math_id = match &split_target.target {
        MathEditTarget::Inline { math_id, .. } => math_id.clone(),
        MathEditTarget::Block { .. } => return Err(EditError::InvalidPosition),
    };
    assert_eq!(split_target.cursor, 1);
    assert!(matches!(split_target.branch, MathEditBranch::Source));

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some("b"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    session.run(Command::Redo)?;
    let restored = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(inline_source(&session).as_deref(), Some("ab"));
    assert_eq!(restored.target, split_target.target);
    assert_eq!(restored.cursor, split_target.cursor);
    assert_eq!(
        restored.revision.document_generation,
        session.document_generation()
    );

    session.run(Command::InsertText("!".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("a!b"));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some("ab"));
    assert_eq!(
        session.math_edit().map(|edit| &edit.target),
        Some(&MathEditTarget::Inline {
            paragraph_id: match &split_target.target {
                MathEditTarget::Inline { paragraph_id, .. } => paragraph_id.clone(),
                MathEditTarget::Block { .. } => return Err(EditError::InvalidPosition),
            },
            math_id: split_math_id,
        })
    );
    Ok(())
}

#[test]
fn formula_focus_allows_global_settings_and_mouse_selection_exit() -> Result<(), EditError> {
    let mut session = session_with_text("host").map_err(test_error)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let active_edit = session.math_edit().cloned();

    session.run(Command::SetDefaultFontFamily(Some(
        "Example Sans".to_owned(),
    )))?;
    assert_eq!(session.math_edit(), active_edit.as_ref());

    let mut invalid = session.selection().focus.clone();
    invalid.offset = usize::MAX;
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: invalid.clone(),
                focus: invalid,
            })))
            .is_err()
    );
    assert_eq!(session.math_edit(), active_edit.as_ref());

    let mut outside = session.selection().focus.clone();
    outside.offset = 0;
    outside.affinity = Affinity::Downstream;
    session.run(Command::SetSelection(Some(Selection {
        anchor: outside.clone(),
        focus: outside,
    })))?;
    assert!(session.math_edit().is_none());
    assert_eq!(
        session.selection_visual_affinity(),
        VisualAffinity::Downstream
    );
    Ok(())
}

#[test]
fn clearing_host_selection_keeps_math_focus_and_collapses_text_island_selection()
-> Result<(), EditError> {
    let mut source_session = session_with_text("").map_err(test_error)?;
    source_session.run(Command::InsertInlineMath("x".to_owned()))?;
    source_session.run(Command::EnterMath(Direction::Backward))?;
    let source_edit = source_session.math_edit().cloned();
    source_session.run(Command::SetSelection(None))?;
    assert_eq!(source_session.math_edit(), source_edit.as_ref());

    let mut text_session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut text_session, r"\text{abcd}")?;
    move_into_math_text(&mut text_session)?;
    text_session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    text_session.run(Command::SetSelection(None))?;
    let edit = text_session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    assert!(
        matches!(edit.branch, MathEditBranch::Text { path, anchor, ..
            } if anchor.path == path && anchor.offset == edit.cursor)
    );
    assert_eq!(
        inline_source(&text_session).as_deref(),
        Some(r"\text{abcd}")
    );
    Ok(())
}

#[test]
fn mod_backspace_is_a_noop_for_oversized_equation_text_focus() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    accept(&mut session, "equation")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText(r"\text{abc}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| (&edit.target, &edit.branch)),
        Some((
            MathEditTarget::Block { align: false, .. },
            MathEditBranch::Text { .. }
        ))
    ));
    let Some(MathEditTarget::Block { block_id, .. }) = session.math_edit().map(|edit| &edit.target)
    else {
        return Err(EditError::InvalidPosition);
    };
    let block_id = block_id.clone();
    let Some(Block::Equation(equation)) = session
        .document
        .body
        .iter_mut()
        .find(|block| matches!(block, Block::Equation(value) if value.id == block_id))
    else {
        return Err(EditError::InvalidPosition);
    };
    let text_flow =
        first_math_text_flow_mut(&mut equation.math_tree.root).ok_or(EditError::InvalidPosition)?;
    text_flow.runs[0].text = "x".repeat(caret_math::MAX_MATH_SOURCE_BYTES + 1);
    assert!(equation.math_tree.validate().is_ok());

    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_selection = session.selection().clone();
    let before_generation = session.document_generation();
    let effect = session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(!effect.document_changed);
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.selection(), &before_selection);
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn mod_backspace_preserves_text_island_selection_through_source_map() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abcd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;

    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), r"\frac{x}{\text{abcd}}");
    let prefix_len = r"\frac{x}{\text{".len();
    assert_eq!(session.selection().anchor.offset, prefix_len + 1);
    assert_eq!(session.selection().focus.offset, prefix_len + 3);
    Ok(())
}

#[test]
fn mod_backspace_preserves_reverse_text_island_selection() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abcd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 3,
        focus: 1,
    })?;

    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    let prefix_len = r"\frac{x}{\text{".len();
    assert_eq!(session.selection().anchor.offset, prefix_len + 3);
    assert_eq!(session.selection().focus.offset, prefix_len + 1);
    assert!(session.selection().anchor.offset > session.selection().focus.offset);
    Ok(())
}

#[test]
fn mod_backspace_keeps_text_caret_when_canonical_projection_merges_text_flows()
-> Result<(), EditError> {
    let source = r"\frac{x}{\text{a}\text{b}\alpha\text{d}\text{e}}";
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(source.to_owned()))?;
    move_into_math_text(&mut session)
        .unwrap_or_else(|error| panic!("entering a text flow failed: {error:?}"));
    session
        .run(Command::SetMathTextSelection {
            anchor: 1,
            focus: 1,
        })
        .unwrap_or_else(|error| panic!("setting the text cursor failed: {error:?}"));

    let canonical = {
        let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
            return Err(EditError::InvalidPosition);
        };
        let [ParagraphContent::Math(math)] = paragraph.inlines.as_slice() else {
            return Err(EditError::InvalidPosition);
        };
        caret_math::serialize_tree(&math.math_tree)
    };
    session
        .run(Command::EditorKey(EditorKey::ModBackspace))
        .unwrap_or_else(|error| panic!("formula demotion unexpectedly failed: {error:?}"));

    assert_eq!(first_paragraph_text(&session), canonical);
    assert_eq!(
        session.selection().focus.offset,
        canonical.rfind("{de}").ok_or(EditError::InvalidPosition)? + 3
    );
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn mod_backspace_keeps_source_caret_in_place_and_typing_continues_there() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath("abc".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));

    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert_eq!(session.selection().focus.offset, 2);
    session.run(Command::InsertText("!".to_owned()))?;
    assert_eq!(first_paragraph_text(&session), "ab!c");
    Ok(())
}

#[test]
fn mod_backspace_maps_a_noncanonical_source_caret_to_canonical_text() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::InsertText(" ".to_owned()))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));

    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert_eq!(first_paragraph_text(&session), "x");
    assert_eq!(session.selection().focus.offset, 1);
    session.run(Command::InsertText("!".to_owned()))?;
    assert_eq!(first_paragraph_text(&session), "x!");
    Ok(())
}

#[test]
fn deleting_a_section_title_text_seam_merges_math_and_restores_tree_focus() -> Result<(), EditError>
{
    let environment_id = NodeId::generate();
    let title_id = NodeId::generate();
    let mut title = Paragraph::empty(title_id.clone());
    title.inlines = vec![
        ParagraphContent::Math(InlineMath {
            id: NodeId::generate(),
            math_tree: parsed_inline_tree("x"),
            extra: BTreeMap::new(),
        }),
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
            math_tree: parsed_inline_tree("y"),
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: "tail".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }),
    ];
    let mut document = Document::blank();
    document.body = vec![Block::Environment(caret_doc::BlockEnvironment {
        id: environment_id.clone(),
        kind: caret_doc::BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(title),
        children: vec![Block::Paragraph(Paragraph::empty(NodeId::generate()))],
        extra: BTreeMap::new(),
    })];
    let mut session = Session::new(document).map_err(test_error)?;
    let start = Pos {
        story: StoryId::EnvironmentTitle(environment_id.clone()),
        paragraph_id: title_id.clone(),
        offset: OBJECT_REPLACEMENT.len_utf8(),
        affinity: Affinity::Downstream,
    };
    let mut end = start.clone();
    end.offset += 1;
    session.selection = Selection {
        anchor: start.clone(),
        focus: end,
    };

    let effect = session.run(Command::Delete(Direction::Backward))?;

    let Block::Environment(environment) = &session.document().body[0] else {
        return Err(EditError::InvalidPosition);
    };
    let title = environment
        .title
        .as_ref()
        .ok_or(EditError::InvalidPosition)?;
    assert!(matches!(
        title.inlines.as_slice(),
        [ParagraphContent::Math(_), ParagraphContent::Text(_)]
    ));
    assert!(matches!(
        session.math_edit().map(|edit| (&edit.target, &edit.branch, edit.cursor)),
        Some((MathEditTarget::Inline { paragraph_id, .. }, MathEditBranch::Tree { path, .. }, 1))
            if paragraph_id == &title_id && path.steps().is_empty()
    ));
    let mut old_tail = start.clone();
    old_tail.offset = 9;
    let mapped_tail = effect
        .position_map
        .as_ref()
        .ok_or(EditError::InvalidPosition)?
        .map(&old_tail);
    assert_eq!(mapped_tail.offset, 5);

    session.run(Command::Undo)?;
    let Block::Environment(environment) = &session.document().body[0] else {
        return Err(EditError::InvalidPosition);
    };
    assert!(matches!(
        environment
            .title
            .as_ref()
            .map(|title| title.inlines.as_slice()),
        Some([
            ParagraphContent::Math(_),
            ParagraphContent::Text(_),
            ParagraphContent::Math(_),
            ParagraphContent::Text(_)
        ])
    ));
    session.run(Command::Redo)?;
    assert!(matches!(
        session.math_edit().map(|edit| (&edit.branch, edit.cursor)),
        Some((MathEditBranch::Tree { path, .. }, 1)) if path.steps().is_empty()
    ));
    Ok(())
}

#[test]
fn math_text_insertion_at_run_boundary_uses_the_focused_affinity() -> Result<(), EditError> {
    for (affinity, expected_language, reject_opaque) in [
        (MathPositionAffinity::Upstream, "en", false),
        (MathPositionAffinity::Downstream, "zh", false),
        (MathPositionAffinity::Upstream, "en", true),
    ] {
        let mut session = session_with_text("").map_err(test_error)?;
        insert_equation_text(&mut session, r"\text{ab}")?;
        move_into_math_text(&mut session)?;
        let edit = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Block {
            block_id,
            align: false,
        } = edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        let Block::Equation(equation) = session
            .document
            .body
            .iter_mut()
            .find(|block| matches!(block, Block::Equation(value) if value.id == block_id))
            .ok_or(EditError::InvalidPosition)?
        else {
            return Err(EditError::InvalidPosition);
        };
        let flow = equation
            .math_tree
            .root
            .content
            .iter_mut()
            .find_map(first_math_content_text_mut)
            .ok_or(EditError::InvalidPosition)?;
        flow.runs = vec![
            Run {
                text: "a".to_owned(),
                language: Some("en".to_owned()),
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
            Run {
                text: String::new(),
                language: Some("fr".to_owned()),
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
            Run {
                text: "b".to_owned(),
                language: Some("zh".to_owned()),
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
        ];
        if reject_opaque {
            flow.runs[0]
                .extra
                .insert("vendorField".to_owned(), serde_json::Value::Bool(true));
        }
        let MathEditBranch::Text { path, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        let position = MathTextPosition::new(path.clone(), 1, affinity);
        session.set_math_text_selection_positions(position.clone(), position)?;
        if reject_opaque {
            let document_before = session.document().clone();
            let edit_before = session.math_edit().cloned();
            let generation_before = session.document_generation();
            assert_eq!(
                session.run(Command::InsertText("X".to_owned())),
                Err(EditError::UnsupportedOpaqueContent)
            );
            assert_eq!(session.document(), &document_before);
            assert_eq!(session.math_edit(), edit_before.as_ref());
            assert_eq!(session.document_generation(), generation_before);
            continue;
        }
        session.run(Command::InsertText("X".to_owned()))?;

        let flow = active_math_text_flow(&session)?;
        assert_eq!(flow.runs.len(), 3);
        let inserted = flow
            .runs
            .iter()
            .find(|run| run.text.contains('X'))
            .ok_or(EditError::InvalidPosition)?;
        assert_eq!(inserted.language.as_deref(), Some(expected_language));
        assert_eq!(
            flow.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "aXb"
        );
        assert_eq!(flow.runs[1].text, "");
        assert_eq!(flow.runs[1].language.as_deref(), Some("fr"));
    }
    Ok(())
}

#[test]
fn inline_math_run_property_keeps_text_selection_without_reparsing_source() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abcd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    let before_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let before_source = before_edit.source().map(str::to_owned);

    let effect = session.run(Command::ToggleRunUnderline)?;
    assert!(effect.document_changed);
    assert!(!effect.selection_changed);
    let after_edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_edit.cursor, before_edit.cursor);
    assert_eq!(after_edit.branch, before_edit.branch);
    assert_eq!(after_edit.source().map(str::to_owned), before_source);
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));

    session.run(Command::Undo)?;
    assert!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .all(|run| { run.props.get("underline") != Some(&serde_json::Value::Bool(true)) })
    );
    session.run(Command::Redo)?;
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    Ok(())
}

#[test]
fn math_text_font_family_survives_navigation_out_of_the_text_branch()
-> Result<(), Box<dyn std::error::Error>> {
    let mut navigation = inline_math_text_session_with_selection(r"\frac{x}{\text{abcd}}", 1, 3)?;
    navigation.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    navigation.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 0,
    })?;
    for _ in 0..3 {
        navigation.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert!(matches!(
        navigation.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));
    assert!(
        navigation
            .math_edit()
            .is_some_and(|edit| edit.source_projection.is_none())
    );
    navigation.run(Command::InsertText("q".to_owned()))?;
    let navigation_edit = navigation.math_edit().ok_or(EditError::InvalidPosition)?;
    let navigation_tree = navigation.math_tree_for_edit(navigation_edit, None)?;
    assert!(caret_math::serialize_tree(navigation_tree).contains('q'));
    assert!(math_tree_has_font_environment_for(
        &navigation_tree.root,
        "Noto Serif",
        "bc"
    ));
    Ok(())
}

#[test]
fn align_math_text_font_family_survives_undo_redo_and_tree_navigation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut align = align_math_text_session_with_selection(1, 3)?;
    align.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    assert!(math_tree_has_font_environment_for(
        &align
            .math_tree_for_edit(
                align.math_edit().ok_or(EditError::InvalidPosition)?,
                Some((0, 0)),
            )?
            .root,
        "Noto Serif",
        "bc"
    ));
    align.run(Command::Undo)?;
    assert!(!math_tree_has_font_environment(
        &align
            .math_tree_for_edit(
                align.math_edit().ok_or(EditError::InvalidPosition)?,
                Some((0, 0)),
            )?
            .root
    ));
    align.run(Command::Redo)?;
    align.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 0,
    })?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    assert!(align.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    align.run(Command::InsertText("q".to_owned()))?;
    assert!(math_tree_has_font_environment_for(
        &align
            .math_tree_for_edit(
                align.math_edit().ok_or(EditError::InvalidPosition)?,
                Some((0, 0)),
            )?
            .root,
        "Noto Serif",
        "bc"
    ));
    Ok(())
}

#[test]
fn invalid_inline_fence_in_text_island_preserves_document_and_focus() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();

    assert!(matches!(
        session.run(Command::InsertInlineMath("$x$$y$".to_owned())),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn invalid_math_source_from_a_text_island_keeps_document_and_focus() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();

    assert!(matches!(
        session.run(Command::InsertInlineMath("}".to_owned())),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn math_text_selection_replaces_and_undo_restores_the_selection() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;

    session.run(Command::InsertText("XY".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{aXYd}"));
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(edit.cursor, 3);
    assert!(matches!(
        &edit.branch,
        MathEditBranch::Text { anchor, .. } if anchor.offset == 3
    ));

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abcd}"));
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(edit.cursor, 3);
    assert!(matches!(
        &edit.branch,
        MathEditBranch::Text { anchor, .. } if anchor.offset == 1
    ));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{aXYd}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    Ok(())
}

#[test]
fn collapsed_math_text_selection_keeps_one_visual_affinity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    let context = session
        .math_text_input_context()
        .ok_or(EditError::InvalidPosition)?;
    let anchor = MathTextPosition::new(
        context.focus.path.clone(),
        1,
        MathPositionAffinity::Upstream,
    );
    let focus = MathTextPosition::new(
        context.focus.path.clone(),
        3,
        MathPositionAffinity::Downstream,
    );
    session.set_math_text_selection_positions_with_visual_affinities(
        anchor,
        focus,
        crate::VisualAffinity::Upstream,
        crate::VisualAffinity::Downstream,
    )?;
    session.run(Command::InsertText("X".to_owned()))?;
    session.run(Command::Undo)?;
    let restored = session
        .math_text_input_context()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!((restored.anchor_utf16, restored.focus_utf16), (1, 3));
    assert_eq!(
        restored.anchor_visual_affinity,
        crate::VisualAffinity::Upstream
    );
    assert_eq!(restored.visual_affinity, crate::VisualAffinity::Downstream);

    session.set_math_text_selection_positions_with_visual_affinities(
        restored.focus.clone(),
        restored.focus,
        crate::VisualAffinity::Downstream,
        crate::VisualAffinity::Upstream,
    )?;
    let collapsed = session
        .math_text_input_context()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(collapsed.anchor_utf16, collapsed.focus_utf16);
    assert_eq!(
        collapsed.anchor_visual_affinity,
        crate::VisualAffinity::Upstream
    );
    assert_eq!(collapsed.visual_affinity, crate::VisualAffinity::Upstream);
    Ok(())
}

#[test]
fn math_text_selection_delete_collapses_at_start_and_arrows_collapse_by_direction()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 3,
        focus: 1,
    })?;
    session.run(Command::Delete(Direction::Forward))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{ad}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));

    session.run(Command::Undo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(edit.cursor, 1);
    assert!(matches!(
        &edit.branch,
        MathEditBranch::Text { anchor, .. } if anchor.offset == 3
    ));
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { anchor, ..
                }) if anchor.offset == 3
    ));
    Ok(())
}

#[test]
fn math_text_selection_replaces_a_complete_escaped_source_mapping() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{a\textchar{5F}b}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 2,
    })?;
    session.run(Command::InsertText("X".to_owned()))?;

    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{aXb}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    Ok(())
}

#[test]
fn adjacent_text_islands_share_caret_navigation() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    let inserted = insert_equation_text(&mut session, r"\text{\textbf{a}}\text{b}");
    assert!(
        inserted.is_ok(),
        "inline math insertion failed: {inserted:?}"
    );
    let entered = session.run(Command::EnterMath(Direction::Backward));
    assert!(entered.is_ok(), "entering math failed: {entered:?}");
    let entered_text = session.run(Command::MoveCaret(Direction::Backward));
    assert!(
        entered_text.is_ok(),
        "moving into text island failed: {entered_text:?}"
    );
    assert!(
        matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ),
        "formula movement did not select a text island: {:?}",
        session.math_edit()
    );
    let second_path = match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    {
        MathEditBranch::Text { path, .. } => path.steps().to_vec(),
        MathEditBranch::Source => return Err(EditError::InvalidPosition),
        MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition),
    };
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));

    let moved_within = session.run(Command::MoveCaret(Direction::Backward));
    assert!(
        moved_within.is_ok(),
        "moving inside the island failed: {moved_within:?}"
    );
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    let crossed_backward = session.run(Command::MoveCaret(Direction::Backward));
    assert!(
        crossed_backward.is_ok(),
        "crossing to previous island failed: {crossed_backward:?}"
    );
    let first_path = match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    {
        MathEditBranch::Text { path, .. } => path.steps().to_vec(),
        MathEditBranch::Source => return Err(EditError::InvalidPosition),
        MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition),
    };
    assert_ne!(first_path, second_path);
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));

    let crossed_forward = session.run(Command::MoveCaret(Direction::Forward));
    assert!(
        crossed_forward.is_ok(),
        "crossing to next island failed: {crossed_forward:?}"
    );
    let next_path = match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    {
        MathEditBranch::Text { path, .. } => path.steps().to_vec(),
        MathEditBranch::Source => return Err(EditError::InvalidPosition),
        MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition),
    };
    assert_eq!(next_path, second_path);
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    Ok(())
}

#[test]
fn math_atoms_separate_text_islands_but_empty_islands_keep_a_caret_stop() -> Result<(), EditError> {
    let mut separated = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut separated, r"\text{a}x\text{b}")?;
    separated.run(Command::EnterMath(Direction::Backward))?;
    separated.run(Command::MoveCaret(Direction::Backward))?;
    separated.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(separated.math_edit().map(|edit| edit.cursor), Some(0));
    separated.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        separated.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));

    let mut empty = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut empty, r"\text{}\text{b}")?;
    empty.run(Command::EnterMath(Direction::Backward))?;
    empty.run(Command::MoveCaret(Direction::Backward))?;
    empty.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(empty.math_edit().map(|edit| edit.cursor), Some(0));
    empty.run(Command::MoveCaret(Direction::Backward))?;
    let edit = empty.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let empty_path = path.steps().to_vec();
    let tree = empty.math_tree_for_edit(edit, path.align_cell())?;
    assert!(
        path.resolve_structurally(tree)
            .is_some_and(|text| text.runs.iter().all(|run| run.text.is_empty()))
    );
    assert_eq!(edit.cursor, 0);

    empty.run(Command::MoveCaret(Direction::Forward))?;
    let edit = empty.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert_ne!(path.steps(), empty_path);
    assert_eq!(edit.cursor, 0);
    empty.run(Command::MoveCaret(Direction::Backward))?;
    let edit = empty.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(path.steps(), empty_path);
    assert_eq!(edit.cursor, 0);
    Ok(())
}

#[test]
fn root_text_boundary_exits_and_reenters_tree_native_math_editing() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| { edit.source_projection.is_none() && edit.cursor == 2 })
    );
    Ok(())
}

#[test]
fn structural_math_navigation_keeps_projection_ranges_out_of_document() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\frac{a}{b}".to_owned()))?;
    let document_before = session.document().clone();
    let generation_before = session.document_generation();
    let history_before = session.history_state_id();

    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    for _ in 0..4 {
        session.run(Command::MoveCaret(Direction::Backward))?;
    }

    assert_eq!(session.document(), &document_before);
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_before);
    Ok(())
}

#[test]
fn tree_native_navigation_round_trips_nested_text_and_math_nodes() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(
        r"\frac{\textbf{a}}{b}".to_owned(),
    ))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let before = session.document().clone();
    let history = session.history_state_id();

    let mut visited = Vec::new();
    for _ in 0..64 {
        let Some(edit) = session.math_edit() else {
            break;
        };
        assert!(edit.source_projection.is_none());
        visited.push(match &edit.branch {
            MathEditBranch::Tree { path, .. } => format!("tree:{:?}:{}", path.steps(), edit.cursor),
            MathEditBranch::Text { path, .. } => format!("text:{:?}:{}", path.steps(), edit.cursor),
            MathEditBranch::Source => return Err(EditError::InvalidPosition),
        });
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    let backward = visited.clone();
    session.run(Command::EnterMath(Direction::Forward))?;
    let mut round_trip = Vec::new();
    for _ in 0..64 {
        let Some(edit) = session.math_edit() else {
            break;
        };
        assert!(edit.source_projection.is_none());
        round_trip.push(match &edit.branch {
            MathEditBranch::Tree { path, .. } => format!("tree:{:?}:{}", path.steps(), edit.cursor),
            MathEditBranch::Text { path, .. } => format!("text:{:?}:{}", path.steps(), edit.cursor),
            MathEditBranch::Source => return Err(EditError::InvalidPosition),
        });
        session.run(Command::MoveCaret(Direction::Forward))?;
    }

    assert!(
        backward
            .iter()
            .any(|position| position.starts_with("text:"))
    );
    assert!(
        backward
            .iter()
            .any(|position| position.starts_with("tree:"))
    );
    assert_eq!(
        round_trip,
        backward.iter().rev().cloned().collect::<Vec<_>>()
    );
    assert_eq!(session.document(), &before);
    assert_eq!(session.history_state_id(), history);
    Ok(())
}

#[test]
fn escaped_math_text_demotion_preserves_the_display_caret() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{a\%b}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), r"\frac{1}{\text{a\%b}}");
    assert_eq!(session.selection().focus.offset, 16);
    Ok(())
}

#[test]
fn tree_branch_edit_restores_focus_through_history_without_a_source_projection()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{a}+x")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::MoveCaret(Direction::Forward))?;

    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    let before = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    session.run(Command::InsertText("q".to_owned()))?;
    let after = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    assert_ne!(after, before);
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(before.as_str()));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(after.as_str()));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));

    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(inline_source(&session).as_deref(), Some(before.as_str()));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(after.as_str()));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(before.as_str()));
    Ok(())
}

#[test]
fn empty_inline_math_starts_tree_native_and_undo_keeps_its_focus() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));

    session.run(Command::InsertText("x".to_owned()))?;
    let inserted = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    assert_eq!(inserted, "x");
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(""));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(inserted.as_str()));
    Ok(())
}

#[test]
fn tree_native_mod_backspace_downgrades_and_undo_restores_editable_focus() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("xy".to_owned()))?;
    let before_demotion = session.document().clone();
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));

    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), "xy");

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &before_demotion);
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::InsertText("z".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("xyz"));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    Ok(())
}

#[test]
fn tree_native_template_enters_the_first_operand_and_restores_focus_in_history()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.math_insert_template(r"\frac{}{}")?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(edit.source_projection.is_none());
    let MathEditBranch::Tree { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert!(matches!(
        path.steps(),
        [MathSequenceStep::Operand {
            operand_index: 0,
            ..
        }]
    ));

    session.run(Command::InsertText("x".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\frac{x}{}"));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\frac{}{}"));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\frac{x}{}"));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    Ok(())
}

#[test]
fn radical_template_focuses_only_its_visible_required_operand() -> Result<(), EditError> {
    let mut tree_branch = session_with_text("").map_err(test_error)?;
    tree_branch.begin_inline_math()?;
    tree_branch.math_insert_template(r"\sqrt{}")?;
    let (_, position) = tree_branch
        .math_tree_input_context()
        .ok_or(EditError::InvalidPosition)?;
    assert!(matches!(
        position.path.steps(),
        [MathSequenceStep::Operand {
            operand_index: 1,
            ..
        }]
    ));

    let mut text_branch = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut text_branch, r"\text{}")?;
    move_into_math_text(&mut text_branch)?;
    text_branch.run(Command::InsertInlineMath(r"\sqrt{}".to_owned()))?;
    let (_, position) = text_branch
        .math_tree_input_context()
        .ok_or(EditError::InvalidPosition)?;
    assert!(matches!(
        position.path.steps(),
        [MathSequenceStep::Operand {
            operand_index: 1,
            ..
        }]
    ));
    Ok(())
}

#[test]
fn tree_native_align_text_navigation_crosses_cells_in_both_directions() -> Result<(), EditError> {
    let (parsed_align, projection) =
        display_math_block_with_projection(true, r"\text{a}&\text{b}", NodeId::generate())?;
    let Block::Align(align_block) = parsed_align else {
        return Err(EditError::InvalidPosition);
    };
    let block_id = align_block.id.clone();
    let row = align_block.rows.first().ok_or(EditError::InvalidPosition)?;
    let first_cell = row.cells.first().ok_or(EditError::InvalidPosition)?;
    let row_id = row.id.clone();
    let first_cell_id = first_cell.root.id.clone();
    let source_map = projection
        .align()
        .and_then(|map| map.cell_tree(0, 0))
        .ok_or(EditError::InvalidPosition)?;
    let range = first_text_source_range(&first_cell.root, source_map.root())
        .ok_or(EditError::InvalidPosition)?;
    let text_path = path::path_at_source_boundary(source_map, range.start, true, Some((0, 0)))
        .ok_or(EditError::InvalidPosition)?;
    let mut document = Document::blank();
    document.body.push(Block::Align(align_block));
    let mut session = Session::new(document)?;
    session.math_edit = Some(math_edit_for_test(
        MathEditTarget::Block {
            block_id: block_id.clone(),
            align: true,
        },
        1,
        MathEditBranch::Text {
            path: text_path.clone(),
            anchor: MathTextPosition::new(text_path, 1, MathPositionAffinity::Downstream),
            focus_affinity: MathPositionAffinity::Downstream,
            anchor_visual_affinity: crate::VisualAffinity::Downstream,
            visual_affinity: crate::VisualAffinity::Downstream,
        },
        session.current_math_edit_revision(),
        None,
    ));

    session.run(Command::MoveCaret(Direction::Forward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        match &edit.branch {
            MathEditBranch::Text { path, .. } => path.align_cell(),
            MathEditBranch::Tree { anchor, .. } => {
                session.align_cell_for_position(edit, anchor)?
            }
            MathEditBranch::Source => None,
        },
        Some((0, 1))
    );
    assert_eq!(active_math_text_flow(&session)?.runs[0].text, "b");
    session.run(Command::MoveCaret(Direction::Backward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        match &edit.branch {
            MathEditBranch::Text { path, .. } => path.align_cell(),
            MathEditBranch::Tree { anchor, .. } => {
                session.align_cell_for_position(edit, anchor)?
            }
            MathEditBranch::Source => None,
        },
        Some((0, 0))
    );
    let Some(MathEditBranch::Text { path, .. }) = session.math_edit().map(|edit| &edit.branch)
    else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(path.align_cell(), Some((0, 0)));
    assert_eq!(active_math_text_flow(&session)?.runs[0].text, "a");
    assert_eq!(row_id, first_align(&session)?.rows[0].id);
    assert_eq!(
        first_cell_id,
        first_align(&session)?.rows[0].cells[0].root.id
    );
    Ok(())
}

#[test]
fn tree_native_align_navigation_crosses_cells_and_exits_the_last_boundary() -> Result<(), EditError>
{
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(Paragraph::empty(NodeId::generate())),
        display_math_block(true, r"\frac{a}{b}&x")?,
        Block::Paragraph(Paragraph::empty(NodeId::generate())),
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    let align = first_align(&session)?;
    let first_row_id = align.rows[0].id.clone();
    let first_cell_id = align.rows[0].cells[0].root.id.clone();
    let second_row_id = align.rows[0].id.clone();
    let second_cell_id = align.rows[0].cells[1].root.id.clone();

    let first_cell_is_tree = session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none()
            && matches!(
                &edit.branch,
                MathEditBranch::Tree { anchor, .. }
                    if matches!(&anchor.host, MathPositionHost::AlignCell { row_id, cell_id }
                        if row_id == &first_row_id && cell_id == &first_cell_id)
            )
    });
    assert!(first_cell_is_tree);

    for _ in 0..16 {
        if session.math_edit().is_some_and(|edit| {
            matches!(
                &edit.branch,
                MathEditBranch::Text { path, .. } if path.align_cell() == Some((0, 1))
            ) || matches!(
                &edit.branch,
                MathEditBranch::Tree { anchor, .. }
                    if matches!(&anchor.host, MathPositionHost::AlignCell { row_id, cell_id }
                        if row_id == &second_row_id && cell_id == &second_cell_id)
            )
        }) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none()
            && (matches!(
                &edit.branch,
                MathEditBranch::Text { path, .. } if path.align_cell() == Some((0, 1))
            ) || matches!(
                &edit.branch,
                MathEditBranch::Tree { anchor, .. }
                    if matches!(&anchor.host, MathPositionHost::AlignCell { row_id, cell_id }
                        if row_id == &second_row_id && cell_id == &second_cell_id)
            ))
    }));

    for _ in 0..16 {
        if session.math_edit().is_some_and(|edit| {
            matches!(
                &edit.branch,
                MathEditBranch::Tree { anchor, .. }
                    if matches!(&anchor.host, MathPositionHost::AlignCell { row_id, cell_id }
                        if row_id == &first_row_id && cell_id == &first_cell_id)
            )
        }) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none()
            && matches!(
                &edit.branch,
                MathEditBranch::Tree { anchor, .. }
                    if matches!(&anchor.host, MathPositionHost::AlignCell { row_id, cell_id }
                        if row_id == &first_row_id && cell_id == &first_cell_id)
            )
    }));

    for _ in 0..16 {
        if session.math_edit().is_none() {
            break;
        }
        session.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn tree_native_align_navigation_round_trips_through_empty_cells_and_rows() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    session.insert_display_math(true)?;
    session.run(Command::InsertText("a".to_owned()))?;
    session.run(Command::InsertText("&".to_owned()))?;
    session.run(Command::InsertText("b".to_owned()))?;
    session.run(Command::EditorKey(EditorKey::Enter))?;
    let initial = first_align(&session)?.clone();
    assert_eq!(initial.rows.len(), 2);
    assert_eq!(initial.rows[0].cells.len(), 2);
    assert!(!initial.rows[0].cells[1].root.content.is_empty());
    assert!(
        initial.rows[1]
            .cells
            .iter()
            .all(|cell| cell.root.content.is_empty())
    );
    let generation = session.document_generation();
    let history = session.history_state_id();

    let mut backward = Vec::new();
    let mut previous = None;
    for _ in 0..32 {
        let Some(edit) = session.math_edit() else {
            break;
        };
        assert!(edit.source_projection.is_none());
        let MathEditBranch::Tree { anchor, path } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        let MathPositionHost::AlignCell { row_id, cell_id } = &anchor.host else {
            return Err(EditError::InvalidPosition);
        };
        let position = (row_id.clone(), cell_id.clone(), path.clone(), edit.cursor);
        if previous.as_ref() == Some(&position) {
            break;
        }
        backward.push(position.clone());
        previous = Some(position);
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert!(backward.iter().any(|(row, cell, _, _)| {
        row == &initial.rows[0].id && cell == &initial.rows[0].cells[1].root.id
    }));
    assert!(backward.iter().any(|(row, cell, _, _)| {
        row == &initial.rows[0].id && cell == &initial.rows[0].cells[0].root.id
    }));
    assert!(backward.iter().any(|(row, cell, _, _)| {
        row == &initial.rows[1].id && cell == &initial.rows[1].cells[0].root.id
    }));

    session.run(Command::EnterMath(Direction::Forward))?;
    let mut forward = Vec::new();
    let mut previous = None;
    for _ in 0..32 {
        let Some(edit) = session.math_edit() else {
            break;
        };
        let MathEditBranch::Tree { anchor, path } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        let MathPositionHost::AlignCell { row_id, cell_id } = &anchor.host else {
            return Err(EditError::InvalidPosition);
        };
        let position = (row_id.clone(), cell_id.clone(), path.clone(), edit.cursor);
        if previous.as_ref() == Some(&position) {
            break;
        }
        forward.push(position.clone());
        previous = Some(position);
        session.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert_eq!(forward, backward.iter().rev().cloned().collect::<Vec<_>>());
    assert_eq!(first_align(&session)?, &initial);
    assert_eq!(session.document_generation(), generation);
    assert_eq!(session.history_state_id(), history);
    Ok(())
}

#[test]
fn empty_text_island_is_an_editable_caret_target() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::InsertText("x".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{x}"));
    Ok(())
}

#[test]
fn empty_math_text_horizontal_navigation_uses_the_pressed_arrow() -> Result<(), EditError> {
    for (key, direction) in [
        (
            EditorKey::LeftArrow {
                candidate: None,
                extend: false,
            },
            Direction::Backward,
        ),
        (
            EditorKey::RightArrow {
                candidate: None,
                extend: false,
            },
            Direction::Forward,
        ),
    ] {
        let make_session = || -> Result<Session, EditError> {
            let mut session = session_with_text("").map_err(test_error)?;
            insert_equation_text(&mut session, r"\text{}")?;
            session.run(Command::EnterMath(Direction::Backward))?;
            session.run(Command::MoveCaret(Direction::Backward))?;
            Ok(session)
        };
        let mut routed = make_session()?;
        let mut expected = make_session()?;
        let before = routed
            .math_text_input_context()
            .ok_or(EditError::InvalidPosition)?;
        assert!(before.surrounding_text.is_empty());

        routed.run(Command::EditorKey(key))?;
        expected.run(Command::MoveCaret(direction))?;

        let edit_signature = |session: &Session| {
            session
                .math_edit()
                .map(|edit| (edit.branch.clone(), edit.cursor))
        };
        assert_eq!(edit_signature(&routed), edit_signature(&expected));
        assert_eq!(
            routed.selection().focus.offset,
            expected.selection().focus.offset
        );
        assert_eq!(
            routed.selection().focus.affinity,
            expected.selection().focus.affinity
        );
    }
    Ok(())
}

#[test]
fn nested_empty_text_island_has_symmetric_one_delimiter_navigation() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{\textbf{}}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let text_path =
        path::root_text_flow_path(tree, true, None).ok_or(EditError::InvalidPosition)?;
    let range =
        math_content_range_at(&edit, text_path.steps()).ok_or(EditError::InvalidPosition)?;
    let source_base = source_projection(&edit).edit_range().start;
    let path = path::path_at_source_boundary(
        source_projection(&edit)
            .tree()
            .ok_or(EditError::InvalidPosition)?,
        source_base + range.end,
        false,
        None,
    )
    .ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 0);

    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());
    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| { edit.source_projection.is_none() && edit.cursor == 0 })
    );
    Ok(())
}

#[test]
fn math_text_formatting_keeps_selection_inside_nested_text_environment() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{\textbf{abc}}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 2,
    })?;

    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;

    let MathEditBranch::Text { path, anchor, .. } = &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(anchor.offset, 0);
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    assert_eq!(
        path.resolve(
            session
                .math_tree_for_edit(session.math_edit().ok_or(EditError::InvalidPosition)?, None,)?
        )
        .map(|text| text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()),
        Some("b".to_owned())
    );
    assert!(inline_source(&session).is_some_and(|source| source.contains(r"\textmd{b}")));

    session.run(Command::Undo)?;
    assert!(inline_source(&session).is_some_and(|source| source.contains(r"\textbf{abc}")));
    session.run(Command::Redo)?;
    assert!(inline_source(&session).is_some_and(|source| source.contains(r"\textmd{b}")));
    Ok(())
}

#[test]
fn math_text_entry_at_collapsed_interior_caret_keeps_both_text_sides_editable()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a}x\text{bc}")
    );

    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::InsertText("R".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a}x\text{Rbc}")
    );

    session.run(Command::Undo)?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a}x\text{bc}")
    );
    for _ in 0..8 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { path, ..
                })
                if path.steps() == [MathTextFlowPathStep::Content(0)]
        ) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                })
            if path.steps() == [MathTextFlowPathStep::Content(0)]
    ));
    session.run(Command::InsertText("L".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{aL}x\text{bc}")
    );
    Ok(())
}

#[test]
fn pending_math_text_formats_keep_local_cursor_after_wrapping_a_middle_range()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))?;
    session.run(Command::InsertText("X".to_owned()))?;
    session.run(Command::InsertText("Y".to_owned()))?;

    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab\textbf{\textit{XY}}cd}")
    );
    let flow = active_math_text_flow(&session)?;
    let text = flow
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    assert_eq!(text, "XY");
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    assert!(is_grapheme_boundary(&text, 2));
    Ok(())
}

#[test]
fn math_text_selection_replaces_across_inline_environment_textflows_and_restores_history()
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
    assert_eq!(island.display, "abcd");
    assert_eq!(island.paths.len(), 2);
    let anchor = MathTextPosition::new(island.paths[0].clone(), 1, MathPositionAffinity::Upstream);
    let focus = MathTextPosition::new(island.paths[1].clone(), 1, MathPositionAffinity::Upstream);
    session.set_math_text_selection_positions(anchor.clone(), focus.clone())?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text {
            focus_affinity: MathPositionAffinity::Upstream,
            ..
        })
    ));
    let generation_before = session.document_generation();
    session.run(Command::InsertText("X".to_owned()))?;
    assert_eq!(session.document_generation(), generation_before + 1);
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    assert_eq!(
        path::contiguous_text_flow_text(tree, path)
            .ok_or(EditError::InvalidPosition)?
            .display,
        "aXd"
    );

    session.run(Command::Undo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text {
        path,
        anchor: restored_anchor,
        focus_affinity: restored_focus_affinity,
        ..
    } = &edit.branch
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "abcd");
    assert_eq!(island.global_offset(restored_anchor), Some(1));
    assert_eq!(
        island.global_offset(&MathTextPosition::new(
            path.clone(),
            edit.cursor,
            *restored_focus_affinity
        )),
        Some(3)
    );

    session.run(Command::Redo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    assert_eq!(
        path::contiguous_text_flow_text(tree, path)
            .ok_or(EditError::InvalidPosition)?
            .display,
        "aXd"
    );
    Ok(())
}

#[test]
fn math_text_format_shifts_later_flows_after_a_preserved_leading_empty_run() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{a}\textbf{bc}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "abc");
    assert_eq!(island.paths.len(), 2);

    let Some(MathEditTarget::Inline { math_id, .. }) = session.math_edit().map(|edit| &edit.target)
    else {
        return Err(EditError::InvalidPosition);
    };
    let math_id = math_id.clone();
    let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
        return Err(EditError::InvalidPosition);
    };
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let first_flow = island.paths[0]
        .resolve_structurally_mut(&mut math.math_tree)
        .ok_or(EditError::InvalidPosition)?;
    first_flow.runs.insert(
        0,
        Run {
            text: String::new(),
            language: Some("en".to_owned()),
            props: Default::default(),
            extra: Default::default(),
        },
    );

    session.set_math_text_selection_positions(
        MathTextPosition::new(island.paths[0].clone(), 0, MathPositionAffinity::Downstream),
        MathTextPosition::new(island.paths[1].clone(), 1, MathPositionAffinity::Upstream),
    )?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;

    let formatted = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, anchor, .. } = &formatted.branch else {
        return Err(EditError::InvalidPosition);
    };
    let tree = session.math_tree_for_edit(formatted, None)?;
    let (island, _) = math_text_selection_range(tree, path, anchor, formatted.cursor)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "abc");
    assert!(island.paths.iter().any(|candidate| {
        candidate.resolve_structurally(tree).is_some_and(|flow| {
            flow.runs
                .iter()
                .any(|run| run.text.is_empty() && run.language.as_deref() == Some("en"))
        })
    }));
    let formatted_flows = island
        .paths
        .iter()
        .map(|candidate| {
            let flow = candidate
                .resolve_structurally(tree)
                .ok_or(EditError::InvalidPosition)?;
            Ok((
                flow.runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
                super::text_inline_environment::inherited_environment_value(
                    tree,
                    candidate,
                    InlineEnvironmentKind::Bold,
                ),
            ))
        })
        .collect::<Result<Vec<_>, EditError>>()?;
    for text in ["a", "b", "c"] {
        assert!(
            formatted_flows
                .iter()
                .any(|(flow_text, bold)| flow_text.contains(text) && *bold == Some(true)),
            "selected fragment {text:?} should be bold: {formatted_flows:?}"
        );
    }
    Ok(())
}

#[test]
fn math_text_inline_format_crosses_textflows_and_restores_selection_state()
-> Result<(), Box<dyn std::error::Error>> {
    for kind in [InlineEnvironmentKind::Bold, InlineEnvironmentKind::Italic] {
        for reversed in [false, true] {
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
            let island =
                path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
            let (anchor, focus) = if reversed {
                (
                    MathTextPosition::new(
                        island.paths[1].clone(),
                        1,
                        MathPositionAffinity::Downstream,
                    ),
                    MathTextPosition::new(
                        island.paths[0].clone(),
                        1,
                        MathPositionAffinity::Upstream,
                    ),
                )
            } else {
                (
                    MathTextPosition::new(
                        island.paths[0].clone(),
                        1,
                        MathPositionAffinity::Upstream,
                    ),
                    MathTextPosition::new(
                        island.paths[1].clone(),
                        1,
                        MathPositionAffinity::Downstream,
                    ),
                )
            };
            let (anchor_visual, focus_visual) = if reversed {
                (
                    crate::VisualAffinity::Upstream,
                    crate::VisualAffinity::Downstream,
                )
            } else {
                (
                    crate::VisualAffinity::Downstream,
                    crate::VisualAffinity::Upstream,
                )
            };
            session.set_math_text_selection_positions_with_visual_affinities(
                anchor.clone(),
                focus.clone(),
                anchor_visual,
                focus_visual,
            )?;
            let document_before = session.document().clone();
            let effect = session.run(Command::ToggleInlineFormat(kind))?;
            assert!(effect.document_changed);
            assert!(effect.selection_changed);

            let formatted = session.math_edit().ok_or(EditError::InvalidPosition)?;
            let MathEditBranch::Text {
                path,
                anchor: formatted_anchor,
                focus_affinity,
                anchor_visual_affinity,
                visual_affinity,
            } = &formatted.branch
            else {
                return Err(EditError::InvalidPosition.into());
            };
            assert_eq!(formatted_anchor.affinity, anchor.affinity);
            assert_eq!(*focus_affinity, focus.affinity);
            assert_eq!(*anchor_visual_affinity, anchor_visual);
            assert_eq!(*visual_affinity, focus_visual);
            let tree = session.math_tree_for_edit(formatted, path.align_cell())?;
            let (island, range) =
                math_text_selection_range(tree, path, formatted_anchor, formatted.cursor)
                    .ok_or(EditError::InvalidPosition)?;
            assert_eq!(island.display, "abcd");
            assert_eq!(range, 1..3);
            for (candidate, flow_range) in island.paths.iter().zip(&island.ranges) {
                if range.start.max(flow_range.start) < range.end.min(flow_range.end) {
                    assert_eq!(
                        super::text_inline_environment::inherited_environment_value(
                            tree, candidate, kind,
                        ),
                        Some(true)
                    );
                }
            }

            session.run(Command::Undo)?;
            assert_eq!(session.document(), &document_before);
            let restored = session.math_edit().ok_or(EditError::InvalidPosition)?;
            assert!(matches!(
                &restored.branch,
                MathEditBranch::Text {
                    anchor: restored_anchor,
                    focus_affinity: restored_focus,
                    anchor_visual_affinity: restored_anchor_visual,
                    visual_affinity: restored_visual,
                    ..
                } if restored_anchor.affinity == anchor.affinity
                    && *restored_focus == focus.affinity
                    && *restored_anchor_visual == anchor_visual
                    && *restored_visual == focus_visual
            ));
            session.run(Command::Redo)?;
            let redone = session.math_edit().ok_or(EditError::InvalidPosition)?;
            assert!(matches!(
                &redone.branch,
                MathEditBranch::Text {
                    anchor: redone_anchor,
                    focus_affinity: redone_focus,
                    anchor_visual_affinity: redone_anchor_visual,
                    visual_affinity: redone_visual,
                    ..
                } if redone_anchor.affinity == anchor.affinity
                    && *redone_focus == focus.affinity
                    && *redone_anchor_visual == anchor_visual
                    && *redone_visual == focus_visual
            ));
        }
    }
    Ok(())
}

#[test]
fn math_text_font_family_crosses_textflows_and_keeps_the_selection()
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
    let focus = MathTextPosition::new(island.paths[1].clone(), 1, MathPositionAffinity::Downstream);
    session.set_math_text_selection_positions_with_visual_affinities(
        anchor.clone(),
        focus.clone(),
        crate::VisualAffinity::Upstream,
        crate::VisualAffinity::Downstream,
    )?;
    let document_before = session.document().clone();

    let effect = session.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    assert!(effect.document_changed);
    assert!(effect.selection_changed);
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text {
        path,
        anchor: selected_anchor,
        focus_affinity,
        anchor_visual_affinity,
        visual_affinity,
    } = &edit.branch
    else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(selected_anchor.affinity, anchor.affinity);
    assert_eq!(*focus_affinity, focus.affinity);
    assert_eq!(*anchor_visual_affinity, crate::VisualAffinity::Upstream);
    assert_eq!(*visual_affinity, crate::VisualAffinity::Downstream);
    let tree = session.math_tree_for_edit(edit, path.align_cell())?;
    assert!(math_tree_has_font_environment_for(
        &tree.root,
        "Noto Serif",
        "b"
    ));
    assert!(math_tree_has_font_environment_for(
        &tree.root,
        "Noto Serif",
        "c"
    ));

    let document_after_font = session.document().clone();
    let history_after_font = session.history_state_id();
    let no_op = session.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    assert!(!no_op.document_changed);
    assert!(!no_op.selection_changed);
    assert_eq!(session.document(), &document_after_font);
    assert_eq!(session.history_state_id(), history_after_font);

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &document_before);
    session.run(Command::Redo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    assert!(math_tree_has_font_environment_for(
        &tree.root,
        "Noto Serif",
        "b"
    ));
    assert!(math_tree_has_font_environment_for(
        &tree.root,
        "Noto Serif",
        "c"
    ));
    Ok(())
}

#[test]
fn math_text_font_family_keeps_partial_selection_endpoints_when_one_flow_is_inherited()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{abc}\textbf{xy}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    let first = island
        .paths
        .first()
        .ok_or(EditError::InvalidPosition)?
        .clone();
    session.set_math_text_selection_positions(
        MathTextPosition::new(first.clone(), 0, MathPositionAffinity::Upstream),
        MathTextPosition::new(first, 3, MathPositionAffinity::Downstream),
    )?;
    session.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    let first = island
        .paths
        .first()
        .ok_or(EditError::InvalidPosition)?
        .clone();
    let second = island
        .paths
        .get(1)
        .ok_or(EditError::InvalidPosition)?
        .clone();
    let first_selected = first.clone();
    session.set_math_text_selection_positions(
        MathTextPosition::new(first, 1, MathPositionAffinity::Upstream),
        MathTextPosition::new(second, 1, MathPositionAffinity::Downstream),
    )?;
    session.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, anchor, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(anchor.offset, 1);
    assert_eq!(anchor.path, first_selected);
    assert_eq!(edit.cursor, 1);
    let tree = session.math_tree_for_edit(edit, path.align_cell())?;
    assert_eq!(
        anchor
            .path
            .resolve_structurally(tree)
            .ok_or(EditError::InvalidPosition)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abc"
    );
    assert_eq!(
        path.resolve_structurally(tree)
            .ok_or(EditError::InvalidPosition)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "x"
    );

    let document_before_no_op = session.document().clone();
    let history_before_no_op = session.history_state_id();
    let effect = session.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    assert!(!effect.document_changed);
    assert!(!effect.selection_changed);
    assert_eq!(session.document(), &document_before_no_op);
    assert_eq!(session.history_state_id(), history_before_no_op);
    Ok(())
}

#[test]
fn math_text_selection_replaces_in_reverse_across_inline_environment_textflows()
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
    let anchor =
        MathTextPosition::new(island.paths[1].clone(), 1, MathPositionAffinity::Downstream);
    let focus = MathTextPosition::new(island.paths[0].clone(), 1, MathPositionAffinity::Upstream);
    session.set_math_text_selection_positions(anchor.clone(), focus.clone())?;
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
        "aXd"
    );
    session.run(Command::Undo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text {
        path,
        anchor: restored_anchor,
        ..
    } = &edit.branch
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.global_offset(restored_anchor), Some(3));
    assert_eq!(
        island.global_offset(&MathTextPosition::new(
            path.clone(),
            edit.cursor,
            MathPositionAffinity::Downstream,
        )),
        Some(1)
    );
    Ok(())
}

#[test]
fn math_text_selection_rejects_cross_operand_range_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{\text{ab}}{\text{cd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    let numerator = MathTextFlowPath::new(
        vec![
            MathTextFlowPathStep::Content(0),
            MathTextFlowPathStep::Operand(0),
            MathTextFlowPathStep::Content(0),
        ],
        None,
    );
    let denominator = MathTextFlowPath::new(
        vec![
            MathTextFlowPathStep::Content(0),
            MathTextFlowPathStep::Operand(1),
            MathTextFlowPathStep::Content(0),
        ],
        None,
    );
    assert_eq!(
        numerator.resolve_structurally(tree).map(|flow| flow
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()),
        Some("ab".to_owned())
    );
    assert_eq!(
        denominator.resolve_structurally(tree).map(|flow| flow
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()),
        Some("cd".to_owned())
    );
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    let before_history = session.history_state_id();
    assert_eq!(
        session.set_math_text_selection_positions(
            MathTextPosition::new(numerator, 1, MathPositionAffinity::Upstream,),
            MathTextPosition::new(denominator, 1, MathPositionAffinity::Downstream,),
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
fn math_text_selection_rejects_cross_math_atom_range_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{ab}+\text{cd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    let MathContent::Math(fraction) = tree
        .root
        .content
        .first()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let denominator = fraction.children.get(1).ok_or(EditError::InvalidPosition)?;
    let text_paths = denominator
        .content
        .iter()
        .enumerate()
        .filter(|(_, content)| matches!(content, MathContent::Text(_)))
        .map(|(index, _)| {
            MathTextFlowPath::new(
                vec![
                    MathTextFlowPathStep::Content(0),
                    MathTextFlowPathStep::Operand(1),
                    MathTextFlowPathStep::Content(index),
                ],
                None,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(text_paths.len(), 2);
    let first = text_paths[0]
        .resolve_structurally(tree)
        .ok_or(EditError::InvalidPosition)?;
    let second = text_paths[1]
        .resolve_structurally(tree)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        first
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "ab"
    );
    assert_eq!(
        second
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "cd"
    );
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    let before_history = session.history_state_id();
    assert_eq!(
        session.set_math_text_selection_positions(
            MathTextPosition::new(text_paths[0].clone(), 1, MathPositionAffinity::Upstream,),
            MathTextPosition::new(text_paths[1].clone(), 1, MathPositionAffinity::Downstream,),
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
fn math_text_selection_rejects_cross_align_cell_range_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let (align_block, _) =
        display_math_block_with_projection(true, r"\text{ab} & \text{cd}", NodeId::generate())?;
    let Block::Align(align) = &align_block else {
        return Err(EditError::InvalidPosition.into());
    };
    let cell_zero = align
        .rows
        .first()
        .and_then(|row| row.cells.first())
        .ok_or(EditError::InvalidPosition)?;
    let cell_one = align
        .rows
        .first()
        .and_then(|row| row.cells.get(1))
        .ok_or(EditError::InvalidPosition)?;
    let mut tree_zero = empty_math_tree();
    tree_zero.root = cell_zero.root.clone();
    let mut tree_one = empty_math_tree();
    tree_one.root = cell_one.root.clone();
    let path_zero = path::root_text_flow_path(&tree_zero, true, Some((0, 0)))
        .ok_or(EditError::InvalidPosition)?;
    let path_one = path::root_text_flow_path(&tree_one, true, Some((0, 1)))
        .ok_or(EditError::InvalidPosition)?;
    let block_id = align.id.clone();
    let mut document = Document::blank();
    document.body.push(align_block);
    let mut session = Session::new(document)?;
    session.math_edit = Some(math_edit_for_test(
        MathEditTarget::Block {
            block_id,
            align: true,
        },
        0,
        MathEditBranch::Text {
            path: path_zero.clone(),
            anchor: MathTextPosition::new(path_zero.clone(), 0, MathPositionAffinity::Downstream),
            focus_affinity: MathPositionAffinity::Downstream,
            anchor_visual_affinity: crate::VisualAffinity::Downstream,
            visual_affinity: crate::VisualAffinity::Downstream,
        },
        session.current_math_edit_revision(),
        None,
    ));
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    let before_history = session.history_state_id();
    assert_eq!(
        session.set_math_text_selection_positions(
            MathTextPosition::new(path_zero, 1, MathPositionAffinity::Upstream),
            MathTextPosition::new(path_one, 1, MathPositionAffinity::Downstream),
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
fn failed_cross_textflow_replacement_preserves_document_selection_and_history()
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
    assert_eq!(island.paths.len(), 2);
    session.set_math_text_selection_positions(
        MathTextPosition::new(island.paths[0].clone(), 1, MathPositionAffinity::Upstream),
        MathTextPosition::new(island.paths[1].clone(), 1, MathPositionAffinity::Downstream),
    )?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditTarget::Inline { math_id, .. } = edit.target else {
        return Err(EditError::InvalidPosition.into());
    };
    let Block::Paragraph(paragraph) = session
        .document
        .body
        .iter_mut()
        .find(|block| matches!(block, Block::Paragraph(paragraph) if find_inline_math(&paragraph.inlines, &math_id).is_some()))
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let MathContent::Math(fraction) = math
        .math_tree
        .root
        .content
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(MathContent::Environment(environment)) = fraction
        .children
        .get_mut(1)
        .and_then(|flow| flow.content.get_mut(1))
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(MathContent::Text(text)) = environment.children.first_mut() else {
        return Err(EditError::InvalidPosition.into());
    };
    text.runs
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
        .extra
        .insert("futureRunField".to_owned(), serde_json::json!("opaque"));
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    let before_history = session.history_state_id();
    assert_eq!(
        session.run(Command::InsertText("X".to_owned())),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    assert_eq!(session.history_state_id(), before_history);
    Ok(())
}

#[test]
fn align_row_break_navigation_keeps_nonempty_row_start_backspace_as_noop()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText(r"a\\abc".to_owned()))?;

    for _ in 0..4 {
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    let source_before = session
        .math_edit()
        .map(source_text)
        .unwrap_or_default()
        .to_owned();
    let align_before = first_align(&session)?.clone();
    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(
        session.math_edit().map(source_text),
        Some(source_before.as_str())
    );
    assert_eq!(first_align(&session)?, &align_before);
    Ok(())
}

#[test]
fn deleting_a_node_with_unmappable_metadata_keeps_document_and_math_focus() -> Result<(), EditError>
{
    let mut math_tree = parsed_inline_tree("a+b");
    math_node_mut(&mut math_tree.root, 0)
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("keep"));
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(ParagraphContent::Math(InlineMath {
        id: NodeId::generate(),
        math_tree,
        extra: std::collections::BTreeMap::new(),
    }));
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session = Session::new(document).map_err(test_error)?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    enter_source_edit(&mut session, Direction::Backward)?;

    let mut committed_document = session.document().clone();
    for _ in 0..8 {
        let generation_before = session.document_generation();
        session.run(Command::Delete(Direction::Backward))?;
        if session.has_uncommitted_math_source_draft() {
            assert_eq!(session.document(), &committed_document);
            assert_eq!(session.document_generation(), generation_before);
            break;
        }
        committed_document = session.document().clone();
    }
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some(""));
    let document_before = session.document().clone();
    assert!(matches!(
        session.run(Command::Delete(Direction::Backward)),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &document_before);
    assert!(session.has_uncommitted_math_source_draft());
    Ok(())
}

#[test]
fn align_projection_is_regenerated_and_keeps_focus_through_edit_undo_and_redo()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "% before\n\\begin {aligned}a&b\\end{aligned} % after\n";
    let caret_math::ProjectedMath::Align(mut align) =
        caret_math::parse_projection(source, caret_math::MathInputKind::Align)?
            .into_parts()
            .0
    else {
        panic!("Align input should parse to an Align tree");
    };
    let untouched_cell_id = align.rows[0].cells[1].root.id.clone();
    align.rows[0].cells[1]
        .extra
        .insert("futureCellValue".to_owned(), serde_json::json!("preserve"));
    let canonical_source = caret_math::serialize_align(&align);

    let mut document = Document::blank();
    document.body.push(Block::Align(align));
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    enter_source_edit(&mut session, Direction::Forward)?;

    let initial_edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(source_projection(initial_edit).source(), canonical_source);
    session.run(Command::InsertText("x".to_owned()))?;
    let edited_source = session
        .math_edit()
        .map(source_text)
        .ok_or(EditError::InvalidPosition)?
        .to_owned();
    let Block::Align(edited_align) = session
        .document()
        .body
        .get(1)
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(
        session.math_edit().map(source_text),
        Some(edited_source.as_str())
    );
    assert_eq!(edited_align.rows[0].cells[1].root.id, untouched_cell_id);
    assert_eq!(
        edited_align.rows[0].cells[1].extra["futureCellValue"],
        "preserve"
    );

    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().map(source_text), Some("a & b"));
    assert_eq!(session.math_edit().map(source_text), Some("a & b"));
    session.run(Command::Redo)?;
    assert_eq!(session.math_edit().map(source_text), Some("xa & b"));
    assert_eq!(session.math_edit().map(source_text), Some("xa & b"));
    Ok(())
}

#[test]
fn undo_and_redo_restore_formula_target_and_source_cursor() -> Result<(), Box<dyn std::error::Error>>
{
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("xy".to_owned()))?;
    let after_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(source_text(&after_edit), "xy");
    assert_eq!(after_edit.cursor, 2);
    assert_eq!(after_edit.revision, session.current_math_edit_revision());

    session.run(Command::Undo)?;
    let after_undo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_undo.target, after_edit.target);
    assert_eq!(source_text(after_undo), "");
    assert_eq!(after_undo.cursor, 0);
    assert!(after_undo.source_projection.is_some());
    assert_eq!(inline_source(&session).as_deref(), Some(""));

    session.run(Command::Redo)?;
    let after_redo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_redo.target, after_edit.target);
    assert_eq!(source_text(after_redo), "xy");
    assert_eq!(after_redo.cursor, 2);
    assert_eq!(after_redo.revision, session.current_math_edit_revision());
    assert!(after_redo.source_projection.is_some());
    assert_eq!(inline_source(&session).as_deref(), Some("xy"));
    Ok(())
}

#[test]
fn caret_comparison_rejects_an_invalid_source_position_on_either_side() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let inside = session.current_caret_snapshot()?;
    let mut invalid_inside = inside.clone();
    let DocumentCaretPosition::Host(HostCaretPosition::Inside {
        local: HostCaretLocalPosition::Math(local),
        ..
    }) = &mut invalid_inside.position
    else {
        return Err(EditError::InvalidPosition);
    };
    let MathCaretCoordinate::SourceUtf8 { offset, .. } = &mut local.coordinate else {
        return Err(EditError::InvalidPosition);
    };
    *offset = usize::MAX;
    let MathEditTarget::Inline { math_id, .. } = &session
        .math_edit
        .as_ref()
        .ok_or(EditError::InvalidPosition)?
        .target
    else {
        return Err(EditError::InvalidPosition);
    };
    let outside = SessionCaretSnapshot {
        session_id: session.session_id.clone(),
        document_generation: session.document_generation,
        position: DocumentCaretPosition::Host(HostCaretPosition::Before {
            host_id: math_id.clone(),
            kind: HostCaretKind::Inline,
        }),
    };
    assert_eq!(
        session.compare_caret_snapshots(&outside, &invalid_inside),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(
        session.compare_caret_snapshots(&invalid_inside, &outside),
        Err(EditError::InvalidPosition)
    );
    Ok(())
}

#[test]
fn caret_comparison_uses_align_edit_source_and_valid_utf8_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let parsed = caret_math::parse_projection(
        r"\begin{aligned}é&x\end{aligned}",
        caret_math::MathInputKind::Align,
    )?;
    let (caret_math::ProjectedMath::Align(align), source_projection) = parsed.into_parts() else {
        return Err("aligned source should project to an Align".into());
    };
    let full_source_len = source_projection.source().len();
    let align_id = align.id.clone();
    let mut session = Session::new(caret_doc::Document {
        body: vec![
            Block::Align(align),
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
        ],
        ..caret_doc::Document::blank()
    })?;
    session.math_edit = Some(MathEdit::editing_source(
        MathEditTarget::Block {
            block_id: align_id,
            align: true,
        },
        Some(std::sync::Arc::new(source_projection)),
        0,
        session.current_math_edit_revision(),
    ));
    let start = session.current_caret_snapshot()?;
    if let Some(edit) = session.math_edit.as_mut() {
        edit.cursor = "é".len();
    } else {
        return Err(EditError::InvalidPosition.into());
    }
    let after_multibyte = session.current_caret_snapshot()?;
    assert_eq!(
        session.compare_caret_snapshots(&start, &after_multibyte),
        Ok(std::cmp::Ordering::Less)
    );

    let mut outside_edit_range = start.clone();
    let DocumentCaretPosition::Host(HostCaretPosition::Inside {
        local: HostCaretLocalPosition::Math(local),
        ..
    }) = &mut outside_edit_range.position
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let MathCaretCoordinate::SourceUtf8 { offset, .. } = &mut local.coordinate else {
        return Err(EditError::InvalidPosition.into());
    };
    *offset = full_source_len - 1;
    assert_eq!(
        session.compare_caret_snapshots(&outside_edit_range, &after_multibyte),
        Err(EditError::InvalidPosition)
    );
    Ok(())
}

#[test]
fn caret_comparison_rejects_source_delimiter_positions_but_accepts_body_edges()
-> Result<(), Box<dyn std::error::Error>> {
    let parsed = caret_math::parse_projection("\\(\u{301}x\\)", caret_math::MathInputKind::Inline)?;
    let (caret_math::ProjectedMath::Tree(tree), source_projection) = parsed.into_parts() else {
        return Err("inline source should project to a MathTree".into());
    };
    let paragraph_id = NodeId::generate();
    let math_id = NodeId::generate();
    let mut session = Session::new(caret_doc::Document {
        body: vec![Block::Paragraph(Paragraph {
            id: paragraph_id.clone(),
            inlines: vec![ParagraphContent::Math(caret_doc::InlineMath {
                id: math_id.clone(),
                math_tree: tree,
                extra: Default::default(),
            })],
            language: None,
            layout: None,
            mark_style: Default::default(),
            props: Default::default(),
            extra: Default::default(),
        })],
        ..caret_doc::Document::blank()
    })?;
    session.math_edit = Some(MathEdit::editing_source(
        MathEditTarget::Inline {
            paragraph_id,
            math_id,
        },
        Some(std::sync::Arc::new(source_projection)),
        0,
        session.current_math_edit_revision(),
    ));
    let range = super::navigation::math_input_body_range(
        session
            .math_edit
            .as_ref()
            .ok_or(EditError::InvalidPosition)?,
    )
    .ok_or(EditError::InvalidPosition)?;
    assert!(range.start > 0);
    if let Some(edit) = session.math_edit.as_mut() {
        edit.cursor = range.start;
    } else {
        return Err(EditError::InvalidPosition.into());
    }
    let body_start = session.current_caret_snapshot()?;
    if let Some(edit) = session.math_edit.as_mut() {
        edit.cursor = range.end;
    } else {
        return Err(EditError::InvalidPosition.into());
    }
    let body_end = session.current_caret_snapshot()?;
    assert_eq!(
        session.compare_caret_snapshots(&body_start, &body_end),
        Ok(std::cmp::Ordering::Less)
    );

    let mut before_body = body_start.clone();
    let DocumentCaretPosition::Host(HostCaretPosition::Inside {
        local: HostCaretLocalPosition::Math(local),
        ..
    }) = &mut before_body.position
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let MathCaretCoordinate::SourceUtf8 { offset, .. } = &mut local.coordinate else {
        return Err(EditError::InvalidPosition.into());
    };
    *offset = range.start - 1;
    assert_eq!(
        session.compare_caret_snapshots(&before_body, &body_start),
        Err(EditError::InvalidPosition)
    );

    let mut after_body = body_end.clone();
    let DocumentCaretPosition::Host(HostCaretPosition::Inside {
        local: HostCaretLocalPosition::Math(local),
        ..
    }) = &mut after_body.position
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let MathCaretCoordinate::SourceUtf8 { offset, .. } = &mut local.coordinate else {
        return Err(EditError::InvalidPosition.into());
    };
    *offset = range.end + 1;
    assert_eq!(
        session.compare_caret_snapshots(&after_body, &body_end),
        Err(EditError::InvalidPosition)
    );
    Ok(())
}

#[test]
fn stale_math_edit_rejects_public_block_selection_without_losing_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut paragraph = caret_doc::Paragraph::empty(NodeId::generate());
    let paragraph_id = paragraph.id.clone();
    paragraph
        .inlines
        .push(ParagraphContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: "content".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }],
            extra: Default::default(),
        }));
    let environment_id = NodeId::generate();
    let mut document = caret_doc::Document::blank();
    document
        .body
        .push(Block::Environment(caret_doc::BlockEnvironment {
            id: environment_id.clone(),
            kind: caret_doc::BlockEnvironmentKind::Section,
            definition_id: None,
            props: Default::default(),
            title: Some(caret_doc::Paragraph::empty(NodeId::generate())),
            children: vec![Block::Paragraph(paragraph)],
            extra: Default::default(),
        }));
    let mut session = Session::new(document)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let document_before = session.document().clone();
    let selection_before = session.selection().clone();
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
        session.select_block_environment(environment_id),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.math_edit(), Some(&stale_edit));

    let anchor = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let focus = Pos {
        offset: "content".len(),
        affinity: Affinity::Upstream,
        ..anchor.clone()
    };
    assert!(matches!(
        session.select_block_environments_from_range(
            anchor.clone(),
            focus.clone(),
            caret_layout::VisualAffinity::Downstream,
        ),
        Err(EditError::InvalidPosition)
    ));
    assert!(matches!(
        session.set_navigation_selection(
            Selection { anchor, focus },
            caret_layout::VisualAffinity::Downstream,
            crate::editing::NavigationIntent::Horizontal,
        ),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.math_edit(), Some(&stale_edit));
    Ok(())
}

#[test]
fn undoing_document_settings_restores_source_branch_ranges_for_text_navigation()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abcd}}".to_owned(),
    ))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::InsertText(" ".to_owned()))?;
    let historical_source = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    assert!(historical_source.ends_with(' '));

    session.run(Command::SetPageGeometry(caret_doc::PageGeometry {
        paper_size: caret_doc::PaperSize::A3,
        orientation: caret_doc::Orientation::Landscape,
        margins: caret_doc::PageMargins {
            top: 36.0,
            right: 54.0,
            bottom: 72.0,
            left: 90.0,
        },
    }))?;
    session.exit_math_edit(Direction::Forward, true)?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert_ne!(
        inline_source(&session).as_deref(),
        Some(historical_source.as_str())
    );

    session.run(Command::Undo)?;
    let restored_source = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    assert!(restored_source.contains(r"\text{abcd}"));
    for _ in 0..64 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::InsertText("z".to_owned()))?;
    assert!(
        inline_source(&session)
            .as_deref()
            .is_some_and(|source| source.contains('z'))
    );
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\frac{x}{\text{abcd}}")
    );
    Ok(())
}

#[test]
fn undo_and_redo_restore_align_cursor_in_unicode_content_coordinates()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    let source = "αabcdefghijklmn";
    session.run(Command::InsertText(source.to_owned()))?;
    assert_eq!(session.math_edit().map(source_text), Some(source));
    assert_eq!(
        session.math_edit().map(|edit| edit.cursor),
        Some(source.len())
    );

    session.run(Command::Undo)?;
    let after_undo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(source_text(after_undo), "");
    assert_eq!(after_undo.cursor, 0);

    session.run(Command::Redo)?;
    let after_redo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(source_text(after_redo), source);
    assert_eq!(after_redo.cursor, source.len());
    assert!(source_projection(after_redo).align().is_some());
    Ok(())
}

#[test]
fn inline_math_navigation_ignores_text_flow_extension_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let caret_math::ParsedMath::Tree(math_tree) =
        caret_math::parse("x", caret_math::MathInputKind::Inline)?
    else {
        panic!("inline math should parse into a tree");
    };
    let mut paragraph = Paragraph::empty(NodeId::generate());
    let text = |value: &str| {
        let mut extra = std::collections::BTreeMap::new();
        extra.insert("futureTextFlowField".to_owned(), serde_json::json!(true));
        ParagraphContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: value.to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }],
            extra,
        })
    };
    paragraph.inlines = vec![
        text("before"),
        ParagraphContent::Math(InlineMath {
            id: NodeId::generate(),
            math_tree,
            extra: Default::default(),
        }),
        text("after"),
    ];
    let paragraph_id = paragraph.id.clone();
    let mut document = caret_doc::Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session = Session::new(document)?;
    let before_math = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: "before".len(),
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: before_math.clone(),
        focus: before_math,
    })))?;

    session.run(Command::EnterMath(Direction::Forward))?;
    assert!(session.math_edit().is_some());
    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.offset, "before".len() + 3);
    Ok(())
}

#[test]
fn undo_and_redo_restore_focus_when_deleting_an_empty_display_formula()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "equation")?;
    assert!(session.math_edit().is_some());
    let before_delete_affinity = session.selection_visual_affinity();
    assert_eq!(before_delete_affinity, crate::VisualAffinity::Downstream);
    session.run(Command::Delete(Direction::Backward))?;
    let after_delete_affinity = session.selection_visual_affinity();
    assert_eq!(after_delete_affinity, crate::VisualAffinity::Upstream);
    assert!(session.math_edit().is_none());
    assert!(
        !session
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Equation(_)))
    );

    session.run(Command::Undo)?;
    assert!(session.math_edit().is_some());
    assert_eq!(session.selection_visual_affinity(), before_delete_affinity);
    assert!(
        session
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Equation(_)))
    );
    session.run(Command::Redo)?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection_visual_affinity(), after_delete_affinity);
    assert!(
        !session
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Equation(_)))
    );
    Ok(())
}

#[test]
fn rejecting_missing_block_selection_keeps_active_math_edit() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    accept(&mut session, "math")?;
    let before = session.math_edit().cloned();
    let generation = session.document_generation();

    assert_eq!(
        session.select_block_environment(NodeId::generate()),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.math_edit(), before.as_ref());
    assert_eq!(session.document_generation(), generation);
    Ok(())
}

#[test]
fn align_backspace_removes_a_blank_row_and_history_restores_its_focus()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));
    session.run(Command::InsertText("a".to_owned()))?;
    session.run(Command::EditorKey(EditorKey::Enter))?;

    let align_before = first_align(&session)?.clone();
    assert_eq!(align_before.rows.len(), 2);
    let blank_row_id = align_before.rows[1].id.clone();
    let deleted = session.run(Command::EditorKey(EditorKey::Backspace))?;
    assert!(deleted.document_changed);
    let align_after = first_align(&session)?;
    assert_eq!(align_after.rows.len(), 1);
    assert_eq!(
        caret_math::serialize_tree(&align_after.rows[0].cells[0]),
        "a"
    );
    let edit_after = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(matches!(
        &edit_after.branch,
        MathEditBranch::Tree { anchor, .. }
            if matches!(anchor.host, MathPositionHost::AlignCell { ref row_id, .. }
                if row_id == &align_after.rows[0].id)
    ));

    session.run(Command::Undo)?;
    let restored_align = first_align(&session)?;
    assert_eq!(restored_align.rows.len(), 2);
    assert_eq!(restored_align.rows[1].id, blank_row_id);
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { anchor, .. })
            if matches!(anchor.host, MathPositionHost::AlignCell { ref row_id, .. }
                if row_id == &blank_row_id)
    ));
    session.run(Command::Redo)?;
    assert_eq!(first_align(&session)?.rows.len(), 1);
    let bytes = caret_doc::CdocFile::new(session.document().clone())?.to_bytes()?;
    let reopened = caret_doc::open(&bytes)?;
    let reopened_session = Session::new(reopened.document().clone())?;
    assert_eq!(first_align(&reopened_session)?.rows.len(), 1);
    assert_eq!(
        caret_math::serialize_tree(&first_align(&reopened_session)?.rows[0].cells[0]),
        "a"
    );
    Ok(())
}

#[test]
fn empty_display_math_hosts_enter_and_edit_with_their_existing_root_identity()
-> Result<(), EditError> {
    for align in [false, true] {
        for explicit_placeholder in [false, true] {
            let block_id = NodeId::generate();
            let root_id = NodeId::generate();
            let content = explicit_placeholder
                .then(|| {
                    caret_doc::MathContent::Math(caret_doc::MathNode {
                        id: NodeId::generate(),
                        kind: caret_doc::MathNodeKind::Placeholder,
                        text: None,
                        attributes: BTreeMap::new(),
                        children: Vec::new(),
                        extra: BTreeMap::new(),
                    })
                })
                .into_iter()
                .collect();
            let tree = caret_doc::MathTree {
                schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                root: caret_doc::MathFlow {
                    id: root_id.clone(),
                    content,
                    extra: BTreeMap::new(),
                },
                extra: BTreeMap::new(),
            };
            let block = if align {
                Block::Align(caret_doc::Align {
                    id: block_id.clone(),
                    rows: vec![caret_doc::MathAlignRow {
                        id: NodeId::generate(),
                        cells: vec![tree],
                        extra: BTreeMap::new(),
                    }],
                    extra: BTreeMap::new(),
                })
            } else {
                Block::Equation(caret_doc::Equation {
                    id: block_id.clone(),
                    math_tree: tree,
                    extra: BTreeMap::new(),
                })
            };
            let mut document = caret_doc::Document::blank();
            document.body.push(block);
            let mut session = Session::new(document.clone())?;
            session.run(Command::EnterMath(Direction::Forward))?;
            assert!(matches!(session.math_edit().map(|edit| &edit.target),
                Some(MathEditTarget::Block { block_id: id, align: target_align })
                    if *id == block_id && *target_align == align));
            session.run(Command::InsertText("x".to_owned()))?;
            let inserted = session.document().clone();
            let root = match &inserted.body[1] {
                Block::Equation(equation) => &equation.math_tree.root,
                Block::Align(align) => &align.rows[0].cells[0].root,
                _ => return Err(EditError::InvalidPosition),
            };
            assert_eq!(root.id, root_id);
            assert!(matches!(root.content.as_slice(),
                [caret_doc::MathContent::Math(node)] if node.text.as_deref() == Some("x")));
            session.run(Command::Undo)?;
            assert_eq!(session.document(), &document);
            session.run(Command::Redo)?;
            assert_eq!(session.document(), &inserted);
        }
    }
    Ok(())
}

#[test]
fn caret_moves_between_adjacent_display_math_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        display_math_block(false, "a")?,
        display_math_block(true, "b&c")?,
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: false, .. })
    ));
    assert!(matches!(
        session.active_host_caret_position(),
        Some(super::super::super::HostCaretPosition::Inside {
            local: super::super::super::HostCaretLocalPosition::Math(_),
            ..
        })
    ));

    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());
    assert!(session.block_boundary_exit().is_some());
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: true, .. })
    ));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));

    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(session.math_edit().is_none());
    assert!(session.block_boundary_exit().is_some());
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: false, .. })
    ));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    Ok(())
}

#[test]
fn leaving_empty_inline_at_formula_boundary_stops_before_adjacent_block() -> Result<(), EditError> {
    let inline_id = NodeId::generate();
    let mut paragraph = caret_doc::Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![caret_doc::ParagraphContent::Environment(
        caret_doc::InlineEnvironment {
            id: inline_id,
            kind: caret_doc::InlineEnvironmentKind::Bold,
            definition_id: None,
            props: BTreeMap::new(),
            children: Vec::new(),
            extra: BTreeMap::new(),
        },
    )];
    let Block::Equation(equation) = display_math_block(false, "x")? else {
        return Err(EditError::InvalidPosition);
    };
    let equation_id = equation.id.clone();
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph), Block::Equation(equation)];
    let mut session = Session::new(document)?;

    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: false,
    }))?;
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: false,
    }))?;
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(HostCaretPosition::After {
            kind: HostCaretKind::Inline,
            ..
        })
    ));
    let candidate = crate::editing::NavigationCandidate::Document {
        focus: session.selection().focus.clone(),
        visual_affinity: session.selection_visual_affinity(),
        generation: crate::editing::NavigationGeneration {
            session_id: session.session_id().clone(),
            document_generation: session.document_generation(),
            font_generation: 0,
        },
    };
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: Some(candidate),
        extend: false,
    }))?;
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(HostCaretPosition::Before {
            host_id,
            kind: HostCaretKind::Block,
        }) if host_id == &equation_id
    ));
    assert!(session.math_edit().is_none());
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: false,
    }))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { block_id, align: false }) if block_id == &equation_id
    ));
    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: false,
    }))?;
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(HostCaretPosition::Before {
            host_id,
            kind: HostCaretKind::Block,
        }) if host_id == &equation_id
    ));
    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: false,
    }))?;
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(HostCaretPosition::After {
            kind: HostCaretKind::Inline,
            ..
        })
    ));
    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: false,
    }))?;
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(HostCaretPosition::Inside {
            kind: HostCaretKind::Inline,
            ..
        })
    ));
    Ok(())
}

#[test]
fn empty_inline_boundary_can_enter_adjacent_inline_math() -> Result<(), EditError> {
    let caret_math::ParsedMath::Tree(math_tree) =
        caret_math::parse("x", caret_math::MathInputKind::Inline).map_err(test_error)?
    else {
        return Err(EditError::InvalidMath("expected inline math".to_owned()));
    };
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![
        ParagraphContent::Environment(caret_doc::InlineEnvironment {
            id: NodeId::generate(),
            kind: caret_doc::InlineEnvironmentKind::Bold,
            definition_id: None,
            props: BTreeMap::new(),
            children: Vec::new(),
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Math(InlineMath {
            id: NodeId::generate(),
            math_tree,
            extra: BTreeMap::new(),
        }),
    ];
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session = Session::new(document)?;

    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: false,
    }))?;
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: false,
    }))?;
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(HostCaretPosition::After {
            kind: HostCaretKind::Inline,
            ..
        })
    ));
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: false,
    }))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Inline { .. })
    ));
    assert!(session.host_caret_position.is_none());
    Ok(())
}

#[test]
fn leaving_last_display_math_without_a_paragraph_keeps_an_outer_boundary_stop()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        display_math_block(false, "a")?,
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;

    assert!(session.math_edit().is_none());
    assert!(session.block_boundary_exit().is_some());
    Ok(())
}

#[test]
fn escape_leaves_the_current_display_math_at_its_outer_after_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    for (align, source) in [(false, "a"), (true, "a&b")] {
        let mut document = caret_doc::Document::blank();
        let current_math = display_math_block(align, source)?;
        let current_id = match &current_math {
            Block::Equation(equation) => equation.id.clone(),
            Block::Align(align) => align.id.clone(),
            _ => unreachable!("display_math_block creates a math block"),
        };
        document.body = vec![
            Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
            current_math,
        ];
        let mut session = Session::new(document)?;
        session.run(Command::EnterMath(Direction::Forward))?;

        session.run(Command::EditorKey(EditorKey::Escape))?;

        assert!(session.math_edit().is_none());
        assert_eq!(
            session.block_boundary_exit(),
            Some((current_id.clone(), Direction::Forward))
        );
    }
    Ok(())
}

#[test]
fn repeated_escape_stays_on_the_current_block_and_arrows_continue_to_adjacent_math()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = caret_doc::Document::blank();
    let current_math = display_math_block(false, "a")?;
    let current_id = match &current_math {
        Block::Equation(equation) => equation.id.clone(),
        _ => unreachable!("expected an Equation"),
    };
    let adjacent_math = display_math_block(true, "b&c")?;
    let adjacent_id = match &adjacent_math {
        Block::Align(align) => align.id.clone(),
        _ => unreachable!("expected an Align"),
    };
    document.body = vec![
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        current_math,
        adjacent_math,
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    session.run(Command::EditorKey(EditorKey::Escape))?;

    assert!(session.math_edit().is_none());
    assert_eq!(
        session.block_boundary_exit(),
        Some((current_id.clone(), Direction::Forward))
    );
    let parked_selection = session.selection().clone();

    session.run(Command::EditorKey(EditorKey::Escape))?;

    assert!(session.math_edit().is_none());
    assert_eq!(
        session.block_boundary_exit(),
        Some((current_id, Direction::Forward))
    );
    assert_eq!(session.selection(), &parked_selection);

    session.run(Command::MoveCaret(Direction::Forward))?;

    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { block_id, align: true }) if block_id == &adjacent_id
    ));
    Ok(())
}

#[test]
fn invalid_structure_selection_keeps_the_active_math_edit_target()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    let target_before = session.math_edit().cloned();
    let position = session.selection().focus.clone();
    let result = session.select_block_environments_from_range(
        position.clone(),
        position,
        crate::VisualAffinity::Downstream,
    );
    assert!(matches!(result, Err(EditError::InvalidPosition)));
    assert_eq!(session.math_edit(), target_before.as_ref());
    Ok(())
}

#[test]
fn structure_selection_blocks_entering_adjacent_inline_math()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = caret_doc::Document::blank();
    let Some(Block::Paragraph(paragraph)) = document.body.first_mut() else {
        panic!("blank document should start with a paragraph");
    };
    let tree = match caret_math::parse("x", caret_math::MathInputKind::Inline)? {
        caret_math::ParsedMath::Tree(tree) => tree,
        caret_math::ParsedMath::Align(_) => panic!("expected inline formula"),
    };
    paragraph.inlines = vec![caret_doc::ParagraphContent::Math(caret_doc::InlineMath {
        id: NodeId::generate(),
        math_tree: tree,
        extra: Default::default(),
    })];
    let selected_environment_id = NodeId::generate();
    document
        .body
        .push(Block::Environment(caret_doc::BlockEnvironment {
            id: selected_environment_id.clone(),
            kind: caret_doc::BlockEnvironmentKind::Section,
            definition_id: None,
            props: Default::default(),
            title: None,
            children: vec![Block::Paragraph(caret_doc::Paragraph::empty(
                NodeId::generate(),
            ))],
            extra: Default::default(),
        }));
    let mut session = Session::new(document)?;
    session.select_block_environment(selected_environment_id.clone())?;
    let structure_selection_before = session
        .structure_selection()
        .map(|(a, f)| (a.clone(), f.clone()));
    let document_before = session.document().clone();

    session.run(Command::EnterMath(Direction::Forward))?;

    assert!(session.math_edit().is_none());
    assert_eq!(
        session.selected_block_environment(),
        Some(&selected_environment_id)
    );
    assert_eq!(
        session
            .structure_selection()
            .map(|(a, f)| (a.clone(), f.clone())),
        structure_selection_before
    );
    assert_eq!(session.document(), &document_before);
    Ok(())
}

#[test]
fn math_tree_shift_right_extension_steps_over_nested_structures_atomically() -> Result<(), EditError>
{
    let (equation, equation_id) = equation_with_inline_math_tree(r"a\frac{b}{c}d");
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph_with_run_text("p")), equation];
    let mut session = Session::new(document)?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::EnterMath(Direction::Forward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Tree { path, anchor } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert!(path.steps().is_empty() && anchor.path.steps().is_empty());
    assert_eq!(edit.cursor, 0);

    // 前三次 Shift+Right 依次跨过 a、分式、d，每个嵌套结构整体作为一个原子。
    for expected_cursor in [1, 2, 3] {
        let effect = shift_right(&mut session)?;
        assert!(effect.selection_changed);
        assert_tree_extension_stays_on_anchor_sequence(&session, 0, expected_cursor);
        assert!(!session.has_structure_selection());
    }

    // 到达 root 边界后转换为整公式结构范围，而不是返回 InvalidPosition。
    shift_right(&mut session)?;
    assert_whole_equation_structure_range(&session, &equation_id, Direction::Forward);
    Ok(())
}

#[test]
fn math_tree_shift_left_extension_steps_over_nested_structures_atomically() -> Result<(), EditError>
{
    let (equation, equation_id) = equation_with_inline_math_tree(r"a\frac{b}{c}d");
    let after = paragraph_with_run_text("q");
    let paragraph_id = after.id.clone();
    let mut document = Document::blank();
    document.body = vec![equation, Block::Paragraph(after)];
    let mut session = Session::new(document)?;
    let position = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: position.clone(),
        focus: position,
    })))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));

    // 反向扩选同样按原子步进，不下钻到分式内部。
    for expected_cursor in [2, 1, 0] {
        shift_left(&mut session)?;
        assert_tree_extension_stays_on_anchor_sequence(&session, 3, expected_cursor);
        assert!(!session.has_structure_selection());
    }

    shift_left(&mut session)?;
    assert_whole_equation_structure_range(&session, &equation_id, Direction::Backward);
    Ok(())
}

#[test]
fn math_tree_selection_context_exposes_anchor_and_focus_only_while_current() -> Result<(), EditError>
{
    let (equation, _) = equation_with_inline_math_tree(r"a\frac{b}{c}d");
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph_with_run_text("p")), equation];
    let mut session = Session::new(document)?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::EnterMath(Direction::Forward))?;

    // 坍缩状态：锚点与焦点重合，上下文仍暴露同一序列两端。
    let (target, anchor, focus) = session
        .math_tree_selection_context()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(anchor.offset, focus.offset);
    assert_eq!(anchor.path, focus.path);

    // 扩选后锚点固定在起点、焦点随光标前进；旧输入上下文只暴露焦点。
    shift_right(&mut session)?;
    shift_right(&mut session)?;
    let (extended_target, extended_anchor, extended_focus) = session
        .math_tree_selection_context()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(extended_target, target);
    assert_eq!(extended_anchor.offset, 0);
    assert_eq!(extended_focus.offset, 2);
    assert_eq!(extended_anchor.path, extended_focus.path);
    assert_eq!(extended_anchor.host, extended_focus.host);
    let (_, input_focus) = session
        .math_tree_input_context()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(input_focus, extended_focus);

    // 过期 revision 返回 None，但不修改已提交的公式编辑状态。
    let revision = session.current_math_edit_revision();
    let edit = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    edit.revision.document_generation += 1;
    assert!(session.math_tree_selection_context().is_none());
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision = revision;

    // TextFlow 分支没有结构序列范围。
    let mut text_session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut text_session, r"\text{}")?;
    move_into_math_text(&mut text_session)?;
    assert!(text_session.math_tree_selection_context().is_none());
    Ok(())
}

#[test]
fn math_tree_shift_extension_from_inside_nested_sequence_converts_the_host_range()
-> Result<(), EditError> {
    let (equation, equation_id) = equation_with_inline_math_tree(r"a\frac{b}{c}d");
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph_with_run_text("p")), equation];
    let mut session = Session::new(document)?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::EnterMath(Direction::Forward))?;
    // 非扩选导航仍可下钻：越过 a 后进入分式分子。
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Tree { path, anchor } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(
        path.steps(),
        &[MathSequenceStep::Operand {
            content_index: 1,
            operand_index: 0,
        }]
    );
    assert_eq!(anchor.path, *path);
    assert_eq!(edit.cursor, 0);

    // 分子内扩选先原子步进到分子末尾，再越出序列转换为宿主结构范围，全程不报错。
    shift_right(&mut session)?;
    assert_tree_extension_stays_on_anchor_sequence(&session, 0, 1);
    assert!(session.math_edit().is_some());
    shift_right(&mut session)?;
    assert_whole_equation_structure_range(&session, &equation_id, Direction::Forward);
    Ok(())
}

#[test]
fn math_tree_non_extending_arrows_still_descend_into_nested_structures() -> Result<(), EditError> {
    let (equation, _) = equation_with_inline_math_tree(r"a\frac{b}{c}d");
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph_with_run_text("p")), equation];
    let mut session = Session::new(document)?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::EnterMath(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Tree { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(
        path.steps(),
        &[MathSequenceStep::Operand {
            content_index: 1,
            operand_index: 0,
        }],
        "非扩选 Right 应继续下钻到分式分子"
    );
    assert_eq!(edit.cursor, 0);

    session.run(Command::MoveCaret(Direction::Backward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Tree { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert!(path.steps().is_empty(), "非扩选 Left 应爬回根序列");
    Ok(())
}

#[test]
fn inline_math_shift_right_past_tree_end_uses_a_plain_object_text_selection()
-> Result<(), EditError> {
    let (document, paragraph_id) =
        inline_math_paragraph(vec![inline_math_node(r"\frac{a}{b}"), text_content("cd")]);
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));

    // 第一次 Shift+Right 仍按原子步进停留在公式树内。
    shift_right(&mut session)?;
    assert_tree_extension_stays_on_anchor_sequence(&session, 0, 1);
    assert!(!session.has_structure_selection());

    // 越过行内公式右边界：普通文本选区覆盖 U+FFFC，而不是空覆盖的伪结构选区。
    shift_right(&mut session)?;
    assert!(session.math_edit().is_none());
    assert!(!session.has_structure_selection());
    assert!(session.structure_selection_range().is_none());
    assert_eq!(
        session.selection().anchor,
        Pos {
            story: StoryId::Body,
            paragraph_id: paragraph_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        }
    );
    assert_eq!(
        session.selection().focus,
        Pos {
            story: StoryId::Body,
            paragraph_id: paragraph_id.clone(),
            offset: 3,
            affinity: Affinity::Upstream,
        }
    );
    assert_eq!(
        session.selection_visual_affinity(),
        VisualAffinity::Upstream
    );
    assert_eq!(
        session.selection_anchor_visual_affinity(),
        VisualAffinity::Downstream
    );

    // 下一次 Shift+Right 继续按普通文档文本扩选，不报 InvalidPosition。
    let effect = shift_right(&mut session)?;
    assert!(effect.selection_changed);
    assert!(session.math_edit().is_none());
    assert!(!session.has_structure_selection());
    assert_eq!(
        session.selection().anchor,
        Pos {
            story: StoryId::Body,
            paragraph_id,
            offset: 0,
            affinity: Affinity::Downstream,
        }
    );
    assert_eq!(session.selection().focus.offset, 4);
    Ok(())
}

#[test]
fn inline_math_shift_left_past_tree_start_uses_a_plain_object_text_selection()
-> Result<(), EditError> {
    let (document, paragraph_id) =
        inline_math_paragraph(vec![text_content("ab"), inline_math_node(r"\frac{a}{b}")]);
    let mut session = Session::new(document)?;
    let after_object = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 5,
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: after_object.clone(),
        focus: after_object,
    })))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));

    shift_left(&mut session)?;
    assert_tree_extension_stays_on_anchor_sequence(&session, 1, 0);
    assert!(!session.has_structure_selection());

    shift_left(&mut session)?;
    assert!(session.math_edit().is_none());
    assert!(!session.has_structure_selection());
    assert!(session.structure_selection_range().is_none());
    assert_eq!(
        session.selection().anchor,
        Pos {
            story: StoryId::Body,
            paragraph_id: paragraph_id.clone(),
            offset: 5,
            affinity: Affinity::Upstream,
        }
    );
    assert_eq!(
        session.selection().focus,
        Pos {
            story: StoryId::Body,
            paragraph_id: paragraph_id.clone(),
            offset: 2,
            affinity: Affinity::Downstream,
        }
    );

    // 下一次 Shift+Left 继续按普通文档文本扩选，不报 InvalidPosition。
    shift_left(&mut session)?;
    assert!(session.math_edit().is_none());
    assert!(!session.has_structure_selection());
    assert_eq!(
        session.selection().anchor,
        Pos {
            story: StoryId::Body,
            paragraph_id,
            offset: 5,
            affinity: Affinity::Upstream,
        }
    );
    assert_eq!(session.selection().focus.offset, 1);
    Ok(())
}

#[test]
fn shift_right_at_the_last_equation_text_island_converts_to_the_whole_equation()
-> Result<(), EditError> {
    let (equation, equation_id) = equation_with_inline_math_tree(r"x\text{ab}y");
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph_with_run_text("p")), equation];
    let mut session = Session::new(document)?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::EnterMath(Direction::Forward))?;
    for _ in 0..2 {
        session.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;

    // 同方向没有其它文本岛时，即使 `ab` 之后仍有符号 `y`，结构范围也以整个公式
    // 对象为目标：转换为整公式 BlockEdge 范围而不是返回错误或停留本地。
    shift_right(&mut session)?;

    assert!(session.math_edit().is_none());
    assert_eq!(
        session.selected_atomic_blocks(),
        std::slice::from_ref(&equation_id)
    );
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("缺少整公式结构范围"));
    assert!(matches!(
        anchor,
        crate::editing::DocumentRangeEndpoint::BlockEdge { block_id, .. }
            if block_id == &equation_id
    ));
    assert!(matches!(
        focus,
        crate::editing::DocumentRangeEndpoint::BlockEdge { block_id, .. }
            if block_id == &equation_id
    ));
    Ok(())
}

#[test]
fn align_first_cell_text_edge_shift_converts_the_whole_block()
-> Result<(), Box<dyn std::error::Error>> {
    let align = display_math_block(true, r"\text{ab} & \text{cd}")?;
    let align_id = match &align {
        caret_doc::Block::Align(align) => align.id.clone(),
        _ => return Err("expected an Align block".into()),
    };
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        caret_doc::Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        align,
        caret_doc::Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
    ];
    let mut session = Session::new(document)?;
    enter_text_branch_at_align_cell(&mut session, 0, 0)?;

    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: true,
    }))?;

    // 首 cell 不是 Align 宿主边界；没有本地后续时与 Tree 分支一致，转为整个公式选区。
    assert!(session.math_edit().is_none());
    assert!(session.has_structure_selection());
    assert!(session.structure_selection_range().is_some());
    assert_eq!(
        session.selected_atomic_blocks(),
        std::slice::from_ref(&align_id)
    );
    Ok(())
}

#[test]
fn align_last_cell_text_edge_shift_left_converts_the_whole_block()
-> Result<(), Box<dyn std::error::Error>> {
    let align = display_math_block(true, r"\text{ab} & \text{cd}")?;
    let align_id = match &align {
        caret_doc::Block::Align(align) => align.id.clone(),
        _ => return Err("expected an Align block".into()),
    };
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        caret_doc::Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        align,
        caret_doc::Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
    ];
    let mut session = Session::new(document)?;
    enter_text_branch_at_align_cell(&mut session, 0, 1)?;

    session.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 0,
    })?;
    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: true,
    }))?;

    assert!(session.math_edit().is_none());
    assert!(session.has_structure_selection());
    assert_eq!(
        session.selected_atomic_blocks(),
        std::slice::from_ref(&align_id)
    );
    Ok(())
}

#[test]
fn middle_text_island_shift_right_converts_the_whole_equation()
-> Result<(), Box<dyn std::error::Error>> {
    let equation = display_math_block(false, r"\text{a}+\text{b}")?;
    let equation_id = match &equation {
        caret_doc::Block::Equation(equation) => equation.id.clone(),
        _ => return Err("expected an Equation block".into()),
    };
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        caret_doc::Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        equation,
        caret_doc::Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    for _ in 0..16 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));

    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: true,
    }))?;

    // 第一个文本岛之后还有 `+\text{b}`：离开当前层即整个公式对象，不在中间层停靠。
    assert!(session.math_edit().is_none());
    assert!(session.has_structure_selection());
    assert_eq!(
        session.selected_atomic_blocks(),
        std::slice::from_ref(&equation_id)
    );
    Ok(())
}
