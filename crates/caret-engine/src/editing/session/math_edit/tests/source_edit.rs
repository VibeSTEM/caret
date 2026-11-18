//! 公式编辑测试：source edit。

use super::*;

#[test]
fn math_source_draft_revision_tracks_content_and_history_changes() -> Result<(), EditError> {
    let mut draft = source_draft::MathSourceDraft::new("x".to_owned(), 1);
    let draft_id = draft.id().clone();
    assert_eq!(draft.revision(), 0);
    draft.record("xy".to_owned(), 1, 2)?;
    assert_eq!(draft.revision(), 1);
    let snapshot = draft.command_snapshot();
    assert_eq!(snapshot.id(), &draft_id);
    assert_eq!(snapshot.revision(), 1);
    assert_eq!(draft.undo()?, Some(1));
    assert_eq!(draft.revision(), 2);
    assert_eq!(draft.redo()?, Some(2));
    assert_eq!(draft.revision(), 3);
    draft.record("xy".to_owned(), 2, 2)?;
    assert_eq!(draft.revision(), 3);
    let replacement = source_draft::MathSourceDraft::new("xy".to_owned(), 2);
    assert_ne!(replacement.id(), &draft_id);
    Ok(())
}

#[test]
fn block_path_lookup_rejects_inline_host_ids() -> Result<(), EditError> {
    let caret_math::ParsedMath::Tree(math_tree) =
        caret_math::parse("", caret_math::MathInputKind::Inline).map_err(test_error)?
    else {
        return Err(EditError::InvalidMath("expected inline math".to_owned()));
    };
    let inline_id = NodeId::generate();
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(ParagraphContent::Math(InlineMath {
        id: inline_id.clone(),
        math_tree,
        extra: Default::default(),
    }));
    let blocks = vec![Block::Paragraph(paragraph)];

    assert_eq!(
        caret_structure::find_block_node(&blocks, &inline_id).map(|location| location.kind),
        Some(caret_structure::DocumentNodeKind::InlineMath)
    );
    assert!(find_block_path(&blocks, &inline_id).is_none());
    Ok(())
}

#[test]
fn sidecar_text_mapping_covers_escaped_utf8_and_empty_text() {
    let projection =
        caret_math::parse_projection(r"\text{a\%b}", caret_math::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("escaped text should project: {error}"));
    let caret_math::ProjectedMath::Tree(tree) = projection.content() else {
        panic!("inline text should project to a math tree");
    };
    let Some(MathContent::Text(text)) = tree.root.content.first() else {
        panic!("escaped source should project to a TextFlow");
    };
    let text_map = projection
        .source_map()
        .tree()
        .and_then(|map| map.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(caret_math::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("escaped TextFlow should have a sidecar mapping"));
    assert_eq!(text_source_range_from_map(text_map, 0, 1..2), Some(7..9));
    assert_eq!(
        text_source_boundary_from_map(text, text_map, 0, 3, Affinity::Upstream),
        Some(10)
    );

    let projection = caret_math::parse_projection(r"\text{}", caret_math::MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("empty text should project: {error}"));
    let caret_math::ProjectedMath::Tree(tree) = projection.content() else {
        panic!("inline empty text should project to a math tree");
    };
    let Some(MathContent::Text(text)) = tree.root.content.first() else {
        panic!("empty source should retain a TextFlow");
    };
    let text_map = projection
        .source_map()
        .tree()
        .and_then(|map| map.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(caret_math::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("empty TextFlow should have a sidecar mapping"));
    assert_eq!(
        text_source_boundary_from_map(text, text_map, 0, 0, Affinity::Upstream),
        Some(6)
    );
}

#[test]
fn empty_text_flow_without_mapping_segments_uses_its_projection_range() {
    let (content, projection) =
        caret_math::parse_projection(r"\text{}", caret_math::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("empty text should project: {error}"))
            .into_parts();
    let caret_math::ProjectedMath::Tree(tree) = content else {
        panic!("inline source should produce a tree");
    };
    let Some(MathContent::Text(_)) = tree.root.content.first() else {
        panic!("empty text should remain a TextFlow");
    };
    let text_map = projection
        .tree()
        .and_then(|map| map.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(caret_math::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("empty TextFlow should have a sidecar mapping"));
    let range = text_map
        .range()
        .unwrap_or_else(|| panic!("empty TextFlow should have a zero-width anchor"));
    let source_range = (
        u32::try_from(range.start).unwrap_or_else(|_| panic!("source offset fits u32")),
        u32::try_from(range.end).unwrap_or_else(|_| panic!("source offset fits u32")),
    );
    let path = MathTextFlowPath::new(vec![caret_doc::MathPathStep::Content(0)], None);
    let text_map = path
        .resolve_in_projection(&tree, &projection)
        .map(|(_, source_map)| source_map)
        .unwrap_or_else(|| panic!("zero-width range should bind without map segments"));
    let text = path
        .resolve_structurally(&tree)
        .unwrap_or_else(|| panic!("TextFlow path should remain structural"));

    assert_eq!(text_map.segments().len(), 1);
    assert!(text_map.segments()[0].display().is_empty());
    assert_eq!(text_map.segments()[0].source(), Some(range.clone()));
    assert_eq!(
        text_source_boundary_from_map(text, text_map, 0, 0, Affinity::Upstream),
        Some(source_range.0 as usize)
    );
}

#[test]
fn inline_math_root_text_is_promoted_to_the_paragraph_flow() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"a\text{x}b".to_owned()))?;

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
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x{OBJECT_REPLACEMENT}")
    );
    for content in &paragraph.inlines {
        if let ParagraphContent::Math(math) = content {
            assert!(
                !math
                    .math_tree
                    .root
                    .content
                    .iter()
                    .any(|content| { matches!(content, MathContent::Text(_)) })
            );
        }
    }

    session.run(Command::InsertText("!".to_owned()))?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x{OBJECT_REPLACEMENT}!")
    );
    session.run(Command::Undo)?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x{OBJECT_REPLACEMENT}")
    );
    session.run(Command::Redo)?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x{OBJECT_REPLACEMENT}!")
    );
    Ok(())
}

#[test]
fn inline_math_root_empty_text_is_a_paragraph_text_flow() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\text{}".to_owned()))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(
        matches!(paragraph.inlines.as_slice(), [ParagraphContent::Text(text)] if text.runs.iter().all(|run| run.text.is_empty()))
    );
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.offset, 0);
    session.run(Command::InsertText("x".to_owned()))?;
    assert_eq!(first_paragraph_text(&session), "x");
    Ok(())
}

#[test]
fn inline_math_root_text_keeps_one_format_environment_wrapper() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\text{\textbf{x}}".to_owned()))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let [ParagraphContent::Environment(environment)] = paragraph.inlines.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(environment.kind, InlineEnvironmentKind::Bold);
    assert!(matches!(
        environment.children.as_slice(),
        [ParagraphContent::Text(_)]
    ));
    assert_eq!(first_paragraph_text(&session), "x");
    Ok(())
}

#[test]
fn deleting_unrelated_paragraph_text_does_not_merge_an_untouched_math_seam() -> Result<(), EditError>
{
    let mut paragraph = Paragraph::empty(NodeId::generate());
    let empty_text = || caret_doc::TextFlow {
        runs: vec![Run {
            text: String::new(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    };
    paragraph.inlines = vec![
        ParagraphContent::Math(InlineMath {
            id: NodeId::generate(),
            math_tree: parsed_inline_tree("x"),
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Text(empty_text()),
        ParagraphContent::Math(InlineMath {
            id: NodeId::generate(),
            math_tree: parsed_inline_tree("y"),
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: "abc".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }),
    ];
    let map = TextMap::new(&paragraph)?;
    assert_eq!(
        map.text,
        format!("{OBJECT_REPLACEMENT}{OBJECT_REPLACEMENT}abc")
    );

    let merged = map.delete_range_and_merge_inline_math(&mut paragraph, 8, 9)?;

    assert!(merged.is_none());
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
fn math_text_font_family_rejects_invalid_family_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut selected = inline_math_text_session_with_selection(r"\frac{x}{\text{abcd}}", 1, 3)?;
    let document_before = selected.document().clone();
    let edit_before = selected.math_edit().cloned();
    let generation_before = selected.document_generation();
    let history_before = selected.history_state_id();
    assert_eq!(
        selected.run(Command::SetInlineFontFamily("  ".to_owned())),
        Err(EditError::InvalidFontFamily)
    );
    assert_eq!(selected.document(), &document_before);
    assert_eq!(selected.math_edit(), edit_before.as_ref());
    assert_eq!(selected.document_generation(), generation_before);
    assert_eq!(selected.history_state_id(), history_before);
    Ok(())
}

#[test]
fn math_text_run_property_rejects_partial_unknown_run_metadata_atomically() -> Result<(), EditError>
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
    first_math_text_flow_mut(&mut math.math_tree.root)
        .and_then(|text| text.runs.first_mut())
        .ok_or(EditError::InvalidPosition)?
        .extra
        .insert("futureRunField".to_owned(), serde_json::json!("opaque"));
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();

    assert_eq!(
        session.run(Command::ToggleRunUnderline),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);

    session.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 4,
    })?;
    session.run(Command::ToggleRunUnderline)?;
    let run = active_math_text_flow(&session)?
        .runs
        .first()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(run.text, "abcd");
    assert_eq!(
        run.extra.get("futureRunField"),
        Some(&serde_json::json!("opaque"))
    );
    assert_eq!(
        run.props.get("underline"),
        Some(&serde_json::Value::Bool(true))
    );
    Ok(())
}

#[test]
fn starting_math_inside_nested_text_style_keeps_the_wrapper() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{\textbf{abcd}}")
        .unwrap_or_else(|error| panic!("insert formula failed: {error}"));
    session
        .run(Command::EnterMath(Direction::Backward))
        .unwrap_or_else(|error| panic!("enter formula failed: {error}"));
    for _ in 0..16 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            break;
        }
        session
            .run(Command::MoveCaret(Direction::Backward))
            .unwrap_or_else(|error| panic!("move into text failed: {error}"));
    }
    session
        .run(Command::SetMathTextSelection {
            anchor: 2,
            focus: 2,
        })
        .unwrap_or_else(|error| panic!("set text position failed: {error}"));

    session
        .begin_inline_math()
        .unwrap_or_else(|error| panic!("start math failed: {error}"));
    session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("insert math source failed: {error}"));
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{\textbf{ab$x$cd}}")
    );
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let Some(MathContent::Environment(bold)) = equation.math_tree.root.content.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(bold.kind, caret_doc::InlineEnvironmentKind::Bold);
    assert!(matches!(bold.children.first(), Some(MathContent::Text(_))));
    assert!(
        bold.children
            .iter()
            .any(|child| matches!(child, MathContent::Math(_)))
    );
    assert!(matches!(bold.children.last(), Some(MathContent::Text(_))));
    Ok(())
}

#[test]
fn starting_math_rejects_unmappable_run_metadata_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;
    let Some(Block::Equation(equation)) = session.document.body.first_mut() else {
        return Err(EditError::InvalidPosition);
    };
    first_math_text_flow_mut(&mut equation.math_tree.root)
        .and_then(|text| text.runs.first_mut())
        .ok_or(EditError::InvalidPosition)?
        .extra
        .insert("futureRunField".to_owned(), serde_json::json!("opaque"));

    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    assert_eq!(
        session.begin_inline_math(),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn starting_math_normalizes_inline_fences_and_preserves_escaped_text() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{a\%bc}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 4,
    })?;
    session.run(Command::InsertInlineMath("$x$".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a\%bc}x\text{}")
    );
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let display = first_math_text_flow(&equation.math_tree.root)
        .ok_or(EditError::InvalidPosition)?
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    assert_eq!(display, "a%bc");
    assert!(equation.math_tree.root.content.iter().any(|content| {
        matches!(content, MathContent::Math(node) if node.kind == MathNodeKind::Symbol && node.text.as_deref() == Some("x"))
    }));
    Ok(())
}

#[test]
fn starting_math_from_nonfirst_align_cell_keeps_unicode_host_offset()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("α & \\text{abc}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 1))
    ));
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;

    session.run(Command::InsertInlineMath("\\(y\\)".to_owned()))?;

    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { path, anchor })
            if path.steps().is_empty() && matches!(anchor.host, MathPositionHost::AlignCell { .. })
    ));
    let align = first_align(&session)?;
    let cell = &align.rows[0].cells[1];
    assert!(cell.root.content.iter().any(|content| {
        matches!(content, MathContent::Math(node) if node.kind == MathNodeKind::Symbol && node.text.as_deref() == Some("y"))
    }));
    session.run(Command::Undo)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abc"
    );
    session.run(Command::Redo)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));
    Ok(())
}

#[test]
fn deleting_at_adjacent_text_island_boundaries_edits_the_neighbor_flow() -> Result<(), EditError> {
    let mut backward = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut backward, r"\text{a}\text{b}")?;
    backward.run(Command::EnterMath(Direction::Backward))?;
    backward.run(Command::MoveCaret(Direction::Backward))?;
    backward.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(backward.math_edit().map(|edit| edit.cursor), Some(0));
    backward.run(Command::Delete(Direction::Backward))?;
    let edit = backward.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let tree = backward.math_tree_for_edit(edit, path.align_cell())?;
    assert_eq!(
        path.resolve(tree).map(|text| text.runs[0].text.as_str()),
        Some("b")
    );
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
        ["", "b"]
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
        ["a", "b"]
    );

    let mut forward = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut forward, r"\text{a}\text{b}")?;
    forward.run(Command::EnterMath(Direction::Backward))?;
    forward.run(Command::MoveCaret(Direction::Backward))?;
    forward.run(Command::MoveCaret(Direction::Backward))?;
    forward.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(forward.math_edit().map(|edit| edit.cursor), Some(1));
    forward.run(Command::Delete(Direction::Forward))?;
    let edit = forward.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let tree = forward.math_tree_for_edit(edit, path.align_cell())?;
    assert_eq!(
        path.resolve(tree).map(|text| text.runs[0].text.as_str()),
        Some("a")
    );
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
        ["a", ""]
    );
    Ok(())
}

#[test]
fn typing_a_batch_at_script_base_start_keeps_new_script_outside() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.math_insert_template("x^a")?;
    let edit = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Tree { anchor, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let base_path = MathSequencePath::new(vec![MathSequenceStep::Operand {
        content_index: 0,
        operand_index: 0,
    }]);
    let base_start = MathTreePosition {
        host: anchor.host.clone(),
        path: base_path.clone(),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    edit.cursor = 0;
    edit.branch = MathEditBranch::Tree {
        path: base_path,
        anchor: base_start,
    };

    session.run(Command::InsertText("v^b".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("v^{b}x^{a}"));
    assert!(
        session
            .math_tree_input_context()
            .is_some_and(|(_, position)| {
                position.path.steps()
                    == [MathSequenceStep::Operand {
                        content_index: 0,
                        operand_index: 1,
                    }]
                    && position.offset == 1
            })
    );
    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), "v^{b}x^{a}");
    Ok(())
}

#[test]
fn empty_text_island_at_formula_edge_exits_without_reentry_trap() -> Result<(), EditError> {
    let mut forward = session_with_text("").map_err(test_error)?;
    forward.run(Command::InsertInlineMath(r"\text{}".to_owned()))?;
    forward.run(Command::EnterMath(Direction::Backward))?;
    forward.run(Command::MoveCaret(Direction::Backward))?;
    forward.run(Command::MoveCaret(Direction::Forward))?;
    if forward.math_edit().is_some() {
        assert!(matches!(
            forward.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Source)
        ));
        forward.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert!(forward.math_edit().is_none());

    let mut backward = session_with_text("").map_err(test_error)?;
    backward.run(Command::InsertInlineMath(r"\text{}".to_owned()))?;
    backward.run(Command::EnterMath(Direction::Backward))?;
    // 空文本岛在公式边界仍是光标停靠点；连续后退先进入再离开，后续断言防止重新陷入。
    backward.run(Command::MoveCaret(Direction::Backward))?;
    backward.run(Command::MoveCaret(Direction::Backward))?;
    if backward.math_edit().is_some() {
        assert!(matches!(
            backward.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Source)
        ));
        for _ in 0..8 {
            backward.run(Command::MoveCaret(Direction::Backward))?;
            assert!(!matches!(
                backward.math_edit().map(|edit| &edit.branch),
                Some(MathEditBranch::Text { .. })
            ));
            if backward.math_edit().is_none() {
                break;
            }
        }
    }
    assert!(backward.math_edit().is_none());
    Ok(())
}

#[test]
fn empty_text_island_between_source_content_can_be_crossed_and_reentered() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"a\text{}b")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let index = tree
        .root
        .content
        .iter()
        .enumerate()
        .find_map(|(index, content)| match content {
            MathContent::Text(text) if text.runs.iter().all(|run| run.text.is_empty()) => {
                Some(index)
            }
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(index)], None);
    session.enter_math_text_branch(path, 0);

    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));
    for _ in 0..3 {
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
    Ok(())
}

#[test]
fn deleting_last_character_of_a_grouped_textflow_keeps_the_empty_textflow() -> Result<(), EditError>
{
    let source = r"\text{a{b}}";
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, source)?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let first_text_end = tree
        .root
        .content
        .iter()
        .enumerate()
        .find_map(|(index, content)| match content {
            MathContent::Text(_) => {
                math_content_range_at(&edit, &[MathTextFlowPathStep::Content(index)])
                    .map(|range| range.end)
            }
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let source_base = source_projection(&edit).edit_range().start;
    let path = path::path_at_source_boundary(
        source_projection(&edit)
            .tree()
            .ok_or(EditError::InvalidPosition)?,
        source_base + first_text_end,
        false,
        None,
    )
    .ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 1);

    session.run(Command::Delete(Direction::Backward))?;

    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    assert!(inline_source(&session).is_some_and(|source| !source.contains('a')));
    session.run(Command::InsertText("z".to_owned()))?;
    assert!(inline_source(&session).is_some_and(|source| source.contains('z')));
    Ok(())
}

#[test]
fn math_text_entry_splices_math_nodes_between_text_flows_and_round_trips_history()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 2,
    })?;

    session.run(Command::InsertInlineMath("x^2".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a}x^{2}\text{c}")
    );
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { path, .. }) if path.steps().is_empty()
    ));
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(
        matches!(equation.math_tree.root.content.first(), Some(MathContent::Text(text)) if text.runs[0].text == "a")
    );
    assert!(
        equation
            .math_tree
            .root
            .content
            .iter()
            .any(|content| matches!(content, MathContent::Math(_)))
    );
    assert!(
        matches!(equation.math_tree.root.content.last(), Some(MathContent::Text(text)) if text.runs[0].text == "c")
    );

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abc}"));
    assert!(matches!(
        session.math_edit().map(|edit| (&edit.branch, edit.cursor)),
        Some((MathEditBranch::Text { anchor, ..
                }, 2)) if anchor.offset == 1
    ));
    session.run(Command::Redo)?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a}x^{2}\text{c}")
    );
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    session.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a}x^{2}y\text{c}")
    );
    Ok(())
}

#[test]
fn math_text_entry_at_empty_flow_end_keeps_an_editable_text_anchor() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;

    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{abc}x\text{}")
    );
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(
        matches!(equation.math_tree.root.content.last(), Some(MathContent::Text(text)) if text.runs.is_empty() || text.runs.iter().all(|run| run.text.is_empty()))
    );
    Ok(())
}

#[test]
fn multi_flow_root_text_exits_at_both_outer_boundaries() -> Result<(), EditError> {
    let mut forward = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut forward, r"\text{a}\textbf{b}")?;
    move_into_math_text(&mut forward)?;
    forward.run(Command::MoveCaret(Direction::Forward))?;
    assert!(forward.math_edit().is_none());

    let mut backward = session_with_text("x").map_err(test_error)?;
    insert_equation_text(&mut backward, r"\text{a}\textbf{b}")?;
    move_into_math_text(&mut backward)?;
    let edit = backward
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = backward.math_tree_for_edit(&edit, None)?;
    let first = path::root_text_flow_path(tree, true, None).ok_or(EditError::InvalidPosition)?;
    assert!(matches!(
        math_text_caret_step(tree, &first, 0, Direction::Backward)?,
        MathTextCaretStep::Boundary
    ));
    assert_eq!(
        path::root_text_flow_path(tree, true, None)
            .ok_or(EditError::InvalidPosition)?
            .steps(),
        first.steps()
    );
    backward.enter_math_text_branch(first, 0);
    backward.run(Command::MoveCaret(Direction::Backward))?;
    assert!(backward.math_edit().is_none(), "{:?}", backward.math_edit());
    Ok(())
}

#[test]
fn equation_and_align_text_islands_share_the_same_edit_branch()
-> Result<(), Box<dyn std::error::Error>> {
    let mut equation = session_with_text("")?;
    accept(&mut equation, "equation")?;
    enter_source_edit(&mut equation, Direction::Forward)?;
    equation.run(Command::InsertText(r"\text{abc}".to_owned()))?;
    equation.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        equation.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    equation.run(Command::InsertText("Xé%".to_owned()))?;
    assert_eq!(inline_source(&equation).as_deref(), Some(r"\text{abcXé\%}"));
    equation.run(Command::InsertText("Z".to_owned()))?;
    assert_eq!(
        inline_source(&equation).as_deref(),
        Some(r"\text{abcXé\%Z}")
    );
    equation.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    let effect = equation.run(Command::ToggleRunUnderline)?;
    assert!(!effect.selection_changed);
    assert!(active_math_text_flow(&equation)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    equation.run(Command::Undo)?;
    assert!(
        active_math_text_flow(&equation)?
            .runs
            .iter()
            .all(|run| { run.props.get("underline") != Some(&serde_json::Value::Bool(true)) })
    );
    equation.run(Command::Redo)?;
    assert!(active_math_text_flow(&equation)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    equation.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    assert!(inline_source(&equation).is_some_and(|source| source.contains(r"\textbf{bc}")));
    assert!(matches!(
        equation.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    equation.run(Command::Undo)?;
    assert!(inline_source(&equation).is_some_and(|source| !source.contains(r"\textbf{bc}")));
    equation.run(Command::Redo)?;
    assert!(inline_source(&equation).is_some_and(|source| source.contains(r"\textbf{bc}")));

    let mut align = session_with_text("")?;
    accept(&mut align, "align")?;
    enter_source_edit(&mut align, Direction::Forward)?;
    align.run(Command::InsertText(r"\text{abc}".to_owned()))?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        align.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 0))
    ));
    align.run(Command::InsertText("Xé%".to_owned()))?;
    assert!(inline_source(&align).is_some_and(|source| source.contains(r"\text{abcXé\%}")));
    align.run(Command::InsertText("Z".to_owned()))?;
    assert!(inline_source(&align).is_some_and(|source| source.contains(r"\text{abcXé\%Z}")));
    align.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    let effect = align.run(Command::ToggleRunUnderline)?;
    assert!(!effect.selection_changed);
    assert!(active_math_text_flow(&align)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    align.run(Command::Undo)?;
    assert!(
        active_math_text_flow(&align)?
            .runs
            .iter()
            .all(|run| { run.props.get("underline") != Some(&serde_json::Value::Bool(true)) })
    );
    align.run(Command::Redo)?;
    assert!(active_math_text_flow(&align)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    align.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    assert!(inline_source(&align).is_some_and(|source| source.contains(r"\textbf{bc}")));
    assert!(matches!(
        align.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 0))
    ));
    Ok(())
}

#[test]
fn math_text_control_word_template_keeps_following_letters_separate()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("alpha".to_owned()))?;
    session.run(Command::AcceptCommandLine)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
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
            .any(|content| { math_content_has_symbol_text(content, "alpha") })
    );
    session.run(Command::InsertText("x".to_owned()))?;
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    let root = &equation.math_tree.root.content;
    assert!(matches!(
        root.as_slice(),
        [
            MathContent::Text(left),
            MathContent::Math(alpha),
            MathContent::Math(x),
            MathContent::Text(right),
        ] if text_flow_display(left) == "a"
            && alpha.text.as_deref() == Some("alpha")
            && x.text.as_deref() == Some("x")
            && text_flow_display(right) == "b"
    ));
    session.run(Command::Undo)?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Tree { .. }))
    );
    session.run(Command::Redo)?;
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
fn repeated_math_text_format_toggle_preserves_the_previous_wrapper() -> Result<(), EditError> {
    let caret_math::ParsedMath::Tree(mut tree) =
        caret_math::parse(r"\text{abc}", caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
    else {
        return Err(EditError::InvalidPosition);
    };
    let path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(0)], None);
    let path = text_branch::apply_math_text_environment_tree_range(
        &mut tree,
        &path,
        0..3,
        InlineEnvironmentKind::Bold,
        true,
    )?;
    let Some(MathContent::Environment(original)) = tree.root.content.first() else {
        return Err(EditError::InvalidPosition);
    };
    let original_id = original.id.clone();
    let original_props = original.props.clone();
    let path = text_branch::apply_math_text_environment_tree_range(
        &mut tree,
        &path,
        0..3,
        InlineEnvironmentKind::Bold,
        false,
    )?;
    let [MathContent::Environment(environment)] = tree.root.content.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(environment.kind, InlineEnvironmentKind::Bold);
    assert_eq!(environment.id, original_id);
    assert_eq!(environment.props, original_props);
    let [MathContent::Environment(inner)] = environment.children.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(
        inner.props.get("enabled"),
        Some(&serde_json::Value::Bool(false))
    );
    assert!(matches!(
        inner.children.as_slice(),
        [MathContent::Text(text)] if text.runs.iter().map(|run| run.text.as_str()).collect::<String>() == "abc"
    ));
    assert!(matches!(
        path.steps(),
        [
            MathTextFlowPathStep::Content(0),
            MathTextFlowPathStep::EnvironmentChild(0),
            MathTextFlowPathStep::EnvironmentChild(0)
        ]
    ));
    Ok(())
}

#[test]
fn shared_math_format_respects_operand_depth_and_keeps_no_op_atomic() -> Result<(), EditError> {
    let mut content = MathContent::Text(caret_doc::TextFlow {
        runs: vec![Run {
            text: "abc".to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        }],
        extra: Default::default(),
    });
    let environment_count = caret_doc::MAX_MATH_TREE_DEPTH - 2;
    for _ in 0..environment_count {
        content = MathContent::Environment(caret_doc::InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Font,
            definition_id: None,
            props: [(
                "fontFamily".to_owned(),
                serde_json::Value::String("Same".to_owned()),
            )]
            .into(),
            children: vec![content],
            extra: Default::default(),
        });
    }
    let mut tree = MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: MathFlow {
            id: NodeId::generate(),
            content: vec![MathContent::Math(caret_doc::MathNode {
                id: NodeId::generate(),
                kind: MathNodeKind::Row,
                text: None,
                attributes: Default::default(),
                children: vec![MathFlow {
                    id: NodeId::generate(),
                    content: vec![content],
                    extra: Default::default(),
                }],
                extra: Default::default(),
            })],
            extra: Default::default(),
        },
        extra: Default::default(),
    };
    tree.validate()
        .map_err(|error| EditError::InvalidMath(error.to_string()))?;
    let mut steps = vec![
        MathTextFlowPathStep::Content(0),
        MathTextFlowPathStep::Operand(0),
        MathTextFlowPathStep::Content(0),
    ];
    steps.extend((0..environment_count).map(|_| MathTextFlowPathStep::EnvironmentChild(0)));
    let path = MathTextFlowPath::new(steps, None);
    let original = tree.clone();
    let no_op_path =
        text_branch::apply_math_text_font_family_tree_range(&mut tree, &path, 1..2, "Same")?;
    assert_eq!(tree, original);
    assert_eq!(no_op_path, path);
    assert_eq!(
        text_branch::apply_math_text_environment_tree_range(
            &mut tree,
            &path,
            0..3,
            InlineEnvironmentKind::Bold,
            true
        ),
        Err(EditError::NestingLimit)
    );
    assert_eq!(tree, original);
    Ok(())
}

#[test]
fn math_text_format_range_keeps_empty_runs_at_the_same_boundaries_as_body_text()
-> Result<(), EditError> {
    let empty_leading = Run {
        text: String::new(),
        language: Some("en".to_owned()),
        props: Default::default(),
        extra: Default::default(),
    };
    let text = Run {
        text: "abc".to_owned(),
        language: None,
        props: Default::default(),
        extra: Default::default(),
    };
    let empty_trailing = Run {
        text: String::new(),
        language: Some("zh".to_owned()),
        props: Default::default(),
        extra: Default::default(),
    };
    let flow = caret_doc::TextFlow {
        runs: vec![empty_leading.clone(), text.clone(), empty_trailing.clone()],
        ..Default::default()
    };

    let formatted = crate::editing::flow_format::format_flow_range(
        &[MathContent::Text(flow)],
        0..3,
        crate::editing::flow_format::InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
        crate::editing::flow_format::InlineFormatContext {
            inherited: Default::default(),
            content_depth: 1,
            max_depth: caret_doc::MAX_MATH_TREE_DEPTH,
        },
    )?;
    let [MathContent::Text(left), MathContent::Environment(selected)] = formatted.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(left.runs, vec![empty_leading]);
    let [MathContent::Text(selected)] = selected.children.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(selected.runs, vec![text, empty_trailing]);
    Ok(())
}

#[test]
fn math_text_inline_format_failure_across_flows_is_atomic() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{ab}\textbf{cd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let tree = session.math_tree_for_edit(&edit, None)?.clone();
    let island = path::contiguous_text_flow_text(&tree, path).ok_or(EditError::InvalidPosition)?;
    session.set_math_text_selection_positions(
        MathTextPosition::new(island.paths[0].clone(), 1, MathPositionAffinity::Upstream),
        MathTextPosition::new(island.paths[1].clone(), 1, MathPositionAffinity::Downstream),
    )?;
    let mut updated_tree = tree;
    island.paths[1]
        .resolve_structurally_mut(&mut updated_tree)
        .ok_or(EditError::InvalidPosition)?
        .extra
        .insert("vendorField".to_owned(), serde_json::Value::Bool(true));
    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &edit.target
    else {
        return Err(EditError::InvalidPosition);
    };
    let Block::Paragraph(paragraph) = session
        .document
        .body
        .iter_mut()
        .find(|block| matches!(block, Block::Paragraph(value) if &value.id == paragraph_id))
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition);
    };
    find_inline_math_mut(&mut paragraph.inlines, math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree = updated_tree;
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();
    let history_before = session.history_state_id();
    assert_eq!(
        session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic)),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_before);
    Ok(())
}

#[test]
fn adjacent_align_text_islands_keep_cell_host_offsets_through_edit_and_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText(r"\text{a}&\text{b}\text{c}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 1))
    ));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));

    session.run(Command::InsertText("X".to_owned()))?;
    assert!(inline_source(&session).is_some_and(|source| source.contains("bXc")));
    session.run(Command::Undo)?;
    assert!(inline_source(&session).is_some_and(|source| source.contains("bc")));
    session.run(Command::Redo)?;
    assert!(inline_source(&session).is_some_and(|source| source.contains("bXc")));
    Ok(())
}

#[test]
fn align_text_island_uses_parser_cell_ranges_after_comments_and_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let mut align = session_with_text("")?;
    accept(&mut align, "align")?;
    enter_source_edit(&mut align, Direction::Forward)?;
    align.run(Command::PasteText(
        "x & y \\\\ % row note\n \\text{abc}".to_owned(),
    ))?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        align.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((1, 0))
    ));
    align.run(Command::InsertText("X".to_owned()))?;
    assert!(inline_source(&align).is_some_and(|source| source.contains(r"\text{abcX}")));
    assert!(align.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none()
            && matches!(&edit.branch, MathEditBranch::Text { path, .. } if path.align_cell() == Some((1, 0)))
    }));
    Ok(())
}

#[test]
fn enter_in_align_textflow_rejects_without_using_source_offsets()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(Paragraph::empty(NodeId::generate())),
        display_math_block(true, r"\text{abc}")?,
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Text { .. })
    }));
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    assert_eq!(
        session.run(Command::EditorKey(EditorKey::Enter)),
        Err(EditError::UnsupportedNewline)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    Ok(())
}

#[test]
fn text_island_edits_preserve_run_properties_and_keep_opaque_run_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abc}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let paragraph = match session.document.body.first_mut() {
        Some(Block::Paragraph(paragraph)) => paragraph,
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let math_id = match paragraph.inlines.first() {
        Some(caret_doc::FlowContent::Math(math)) => math.id.clone(),
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let text =
        first_math_text_flow_mut(&mut math.math_tree.root).ok_or(EditError::InvalidPosition)?;
    text.runs[0]
        .props
        .insert("underline".to_owned(), serde_json::Value::Bool(true));
    session.run(Command::InsertText("X".to_owned()))?;
    let paragraph = match session.document.body.first() {
        Some(Block::Paragraph(paragraph)) => paragraph,
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let math = find_inline_math(&paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let text = first_math_text_flow(&math.math_tree.root).ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abcX"
    );
    assert_eq!(
        text.runs.last().and_then(|run| run.props.get("underline")),
        None
    );

    session.run(Command::Delete(Direction::Backward))?;
    session.run(Command::InsertText("Y".to_owned()))?;
    let paragraph = match session.document.body.first() {
        Some(Block::Paragraph(paragraph)) => paragraph,
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let math = find_inline_math(&paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let text = first_math_text_flow(&math.math_tree.root).ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abcY"
    );
    assert_eq!(
        text.runs.last().and_then(|run| run.props.get("underline")),
        None
    );

    let paragraph = match session.document.body.first_mut() {
        Some(Block::Paragraph(paragraph)) => paragraph,
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let text =
        first_math_text_flow_mut(&mut math.math_tree.root).ok_or(EditError::InvalidPosition)?;
    let opaque = text.runs.last_mut().ok_or(EditError::InvalidPosition)?;
    opaque
        .extra
        .insert("vendorRun".to_owned(), serde_json::Value::Bool(true));
    session.run(Command::InsertText("Y".to_owned()))?;
    let text = active_math_text_flow(&session)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abcYY"
    );
    assert!(text.runs.iter().any(|run| {
        run.text == "Y" && run.extra.get("vendorRun") == Some(&serde_json::Value::Bool(true))
    }));
    Ok(())
}

#[test]
fn paste_preserves_latex_group_syntax() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::PasteText("x^{2}".to_owned()))?;

    assert_eq!(inline_source(&session).as_deref(), Some("x^{2}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(5));
    Ok(())
}

#[test]
fn align_cell_mapping_counts_rows_and_cells() {
    assert_eq!(
        projected_align_cell_at("a&=b \\\\ c& =d", 12),
        Some((1, 1, 1))
    );
    assert_eq!(projected_align_cell_at("a&=b", 1), Some((0, 0, 1)));
    assert_eq!(projected_align_cell_at("a  &b", 2), Some((0, 0, 2)));
    assert_eq!(projected_align_cell_at("a  &b", 3), Some((0, 0, 3)));
    assert_eq!(projected_align_cell_at("a  ", 3), Some((0, 0, 3)));
    assert_eq!(projected_align_cell_at("a  \\\\  b", 3), Some((0, 0, 3)));
    assert_eq!(projected_align_cell_at("   ", 0), Some((0, 0, 0)));
    assert_eq!(projected_align_cell_at("   ", 1), Some((0, 0, 0)));
    assert_eq!(
        projected_align_cell_at("% empty align\n", 4),
        Some((0, 0, 0))
    );
}

#[test]
fn align_cell_mapping_ignores_nested_environment_separators() {
    let source = r"\begin{matrix}a&b\\c&d\end{matrix}&=z";
    let nested_cell = source
        .find("c&d")
        .unwrap_or_else(|| panic!("missing nested cell"))
        + 2;
    assert_eq!(
        projected_align_cell_at(source, nested_cell),
        Some((0, 0, nested_cell))
    );

    let outer_cell = source
        .rfind('&')
        .unwrap_or_else(|| panic!("missing outer separator"))
        + 1;
    assert_eq!(projected_align_cell_at(source, outer_cell), Some((0, 1, 0)));

    let commented_begin =
        "\\begin % environment name follows the comment\n{matrix}a&b\\end{matrix}&=z";
    let nested_separator = commented_begin
        .find("a&b")
        .unwrap_or_else(|| panic!("missing commented nested matrix"))
        + 2;
    assert_eq!(
        projected_align_cell_at(commented_begin, nested_separator),
        Some((0, 0, nested_separator))
    );
}

#[test]
fn align_cell_mapping_ignores_separators_in_comments() {
    let source = "a % & ignored \\\\ ignored\n+b&c";
    let in_comment = source
        .find("ignored")
        .unwrap_or_else(|| panic!("missing comment"))
        + 3;
    assert_eq!(
        projected_align_cell_at(source, in_comment),
        Some((0, 0, in_comment))
    );
    let after_outer_separator = source
        .rfind('&')
        .unwrap_or_else(|| panic!("missing separator"))
        + 2;
    assert_eq!(
        projected_align_cell_at(source, after_outer_separator),
        Some((0, 1, 1))
    );

    assert_eq!(
        projected_align_cell_at("% leading cell comment\nx", 24),
        Some((0, 0, 1))
    );
    assert_eq!(
        projected_align_cell_at("% leading cell comment\nx", 4),
        Some((0, 0, 0))
    );
    let later_cell_comment = "a&% comment\nb";
    let comment_cursor = later_cell_comment
        .find("comment")
        .unwrap_or_else(|| panic!("missing cell comment"))
        + 3;
    assert_eq!(
        projected_align_cell_at(later_cell_comment, comment_cursor),
        Some((0, 1, 0))
    );
    let later_row_comment = "a \\\\% comment\nb";
    let row_comment_cursor = later_row_comment
        .find("comment")
        .unwrap_or_else(|| panic!("missing row comment"))
        + 3;
    assert_eq!(
        projected_align_cell_at(later_row_comment, row_comment_cursor),
        Some((1, 0, 0))
    );
}

#[test]
fn metadata_mapping_preserves_math_text_runs_and_environment_identity() {
    fn first_environment(
        flow: &mut MathFlow,
    ) -> Option<&mut caret_doc::InlineEnvironment<MathContent>> {
        for content in &mut flow.content {
            match content {
                MathContent::Environment(environment) => return Some(environment),
                MathContent::Math(node) => {
                    for child in &mut node.children {
                        if let Some(environment) = first_environment(child) {
                            return Some(environment);
                        }
                    }
                }
                MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
            }
        }
        None
    }
    fn first_text(contents: &mut [MathContent]) -> Option<&mut caret_doc::TextFlow> {
        for content in contents {
            match content {
                MathContent::Text(text) => return Some(text),
                MathContent::Environment(environment) => {
                    if let Some(text) = first_text(&mut environment.children) {
                        return Some(text);
                    }
                }
                MathContent::Math(node) => {
                    for child in &mut node.children {
                        if let Some(text) = first_text(&mut child.content) {
                            return Some(text);
                        }
                    }
                }
                MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
            }
        }
        None
    }

    let source = r"\text{\textbf{a}}+x";
    let mut previous = parsed_inline_tree(source);
    let environment = first_environment(&mut previous.root)
        .unwrap_or_else(|| panic!("text formatting environment should be parsed"));
    let environment_id = environment.id.clone();
    environment.definition_id = Some("template.bold".to_owned());
    environment
        .props
        .insert("vendorProperty".to_owned(), serde_json::json!("kept"));
    environment
        .extra
        .insert("vendorEnvironment".to_owned(), serde_json::json!(3));

    let text = first_text(&mut previous.root.content)
        .unwrap_or_else(|| panic!("text mode should produce a TextFlow"));
    text.runs[0].language = Some("en".to_owned());
    text.runs[0]
        .props
        .insert("underline".to_owned(), serde_json::json!(true));
    text.runs[0]
        .extra
        .insert("vendorRun".to_owned(), serde_json::json!(4));
    let updated_source = source.replace("+x", "+y");
    let mut updated = parsed_inline_tree(&updated_source);
    let edit = (source.len() - 1)..source.len();
    assert!(preserve_math_tree_metadata_for_test(
        &previous,
        &mut updated,
        &edit,
        &edit,
    ));
    let environment = first_environment(&mut updated.root)
        .unwrap_or_else(|| panic!("mapped environment should remain"));
    assert_eq!(environment.id, environment_id);
    assert_eq!(environment.definition_id.as_deref(), Some("template.bold"));
    assert_eq!(environment.props["vendorProperty"], "kept");
    assert_eq!(environment.extra["vendorEnvironment"], 3);
    let text = first_text(&mut updated.root.content)
        .unwrap_or_else(|| panic!("mapped TextFlow should remain"));
    assert_eq!(text.runs[0].language.as_deref(), Some("en"));
    assert_eq!(text.runs[0].props["underline"], true);
    assert_eq!(text.runs[0].extra["vendorRun"], 4);
}

#[test]
fn math_edit_refuses_to_drop_unknown_fields_from_an_unmapped_node() {
    let mut previous = parsed_inline_tree("a+b");
    math_node_mut(&mut previous.root, 0)
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("keep"));
    let mut updated = parsed_inline_tree(r"\frac{a}{b}");

    assert!(!preserve_math_tree_metadata(&previous, &mut updated));
}

#[test]
fn appending_duplicate_align_cell_keeps_old_metadata_at_source_positions() -> Result<(), EditError>
{
    let Block::Align(mut previous) = display_math_block(true, "a&a")? else {
        panic!("expected Align block");
    };
    let first_id = previous.rows[0].cells[0].root.id.clone();
    let last_id = previous.rows[0].cells[1].root.id.clone();
    previous.rows[0].cells[0]
        .extra
        .insert("origin".to_owned(), serde_json::json!("first"));
    previous.rows[0].cells[1]
        .extra
        .insert("origin".to_owned(), serde_json::json!("last"));

    let Block::Align(mut updated) = display_math_block_with_id(true, "a&a&a", previous.id.clone())?
    else {
        panic!("expected parsed Align block");
    };
    let old_offset = aligned_body_offset(&previous);
    let new_offset = aligned_body_offset(&updated);
    let old_body_len = aligned_body_len(&previous);
    let new_body_len = aligned_body_len(&updated);
    preserve_align_metadata_for_test(
        &previous,
        &mut updated,
        &(old_offset + old_body_len..old_offset + old_body_len),
        &(new_offset + old_body_len..new_offset + new_body_len),
    );

    assert_eq!(updated.rows[0].cells[0].root.id, first_id);
    assert_eq!(updated.rows[0].cells[1].root.id, last_id);
    assert_eq!(updated.rows[0].cells[0].extra["origin"], "first");
    assert_eq!(updated.rows[0].cells[1].extra["origin"], "last");
    Ok(())
}

#[test]
fn filling_an_empty_align_cell_preserves_its_identity_and_unknown_fields() -> Result<(), EditError>
{
    let Block::Align(mut previous) = display_math_block(true, "a&&b")? else {
        panic!("expected Align block");
    };
    let empty_cell = &mut previous.rows[0].cells[1];
    let cell_id = empty_cell.root.id.clone();
    empty_cell
        .extra
        .insert("futureCellValue".to_owned(), serde_json::json!("keep"));
    let Block::Align(mut updated) = display_math_block_with_id(true, "a&x&b", previous.id.clone())?
    else {
        panic!("expected parsed Align block");
    };
    let old_range = aligned_cell_range(&previous, 0, 1);
    let new_range = aligned_cell_range(&updated, 0, 1);
    preserve_align_metadata_for_test(&previous, &mut updated, &old_range, &new_range);

    assert_eq!(updated.rows[0].cells[1].root.id, cell_id);
    assert_eq!(updated.rows[0].cells[1].extra["futureCellValue"], "keep");
    Ok(())
}

#[test]
fn align_metadata_matches_cells_one_to_one_across_trivia_and_merges() -> Result<(), EditError> {
    let previous_source = "\\begin{aligned}a& % before\n \\end{aligned}";
    let updated_source = "\\begin{aligned}a&x % before\n \\end{aligned}";
    let previous_projection =
        caret_math::parse_projection(previous_source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
    let updated_projection =
        caret_math::parse_projection(updated_source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
    let (caret_math::ProjectedMath::Align(mut previous), previous_map) =
        previous_projection.into_parts()
    else {
        return Err(EditError::InvalidPosition);
    };
    let (caret_math::ProjectedMath::Align(mut updated), updated_map) =
        updated_projection.into_parts()
    else {
        return Err(EditError::InvalidPosition);
    };
    let empty_cell_id = previous.rows[0].cells[1].root.id.clone();
    previous.rows[0].cells[1]
        .extra
        .insert("vendorCell".to_owned(), serde_json::json!("kept"));
    let old_cursor = previous_map
        .align()
        .ok_or(EditError::InvalidPosition)?
        .cell_navigation_ranges()[0][1]
        .start;
    let new_range = updated_map
        .align()
        .and_then(|map| map.cell_range(0, 1))
        .ok_or(EditError::InvalidPosition)?;
    assert!(preserve_align_metadata(
        &previous,
        &mut updated,
        &(old_cursor..old_cursor),
        &new_range,
        &previous_map,
        &updated_map,
    ));
    assert_eq!(updated.rows[0].cells[1].root.id, empty_cell_id);
    assert_eq!(updated.rows[0].cells[1].extra["vendorCell"], "kept");

    let previous_source = "\\begin{aligned}a&b\\end{aligned}";
    let updated_source = "\\begin{aligned}b\\end{aligned}";
    let previous_projection =
        caret_math::parse_projection(previous_source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
    let updated_projection =
        caret_math::parse_projection(updated_source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
    let (caret_math::ProjectedMath::Align(mut previous), previous_map) =
        previous_projection.into_parts()
    else {
        return Err(EditError::InvalidPosition);
    };
    let (caret_math::ProjectedMath::Align(mut updated), updated_map) =
        updated_projection.into_parts()
    else {
        return Err(EditError::InvalidPosition);
    };
    let surviving_cell_id = previous.rows[0].cells[1].root.id.clone();
    previous.rows[0].cells[1]
        .extra
        .insert("vendorCell".to_owned(), serde_json::json!("survives"));
    let old_map = previous_map.align().ok_or(EditError::InvalidPosition)?;
    let new_range = updated_map
        .align()
        .and_then(|map| map.cell_range(0, 0))
        .ok_or(EditError::InvalidPosition)?;
    let old_second_cell = old_map.cell_range(0, 1).ok_or(EditError::InvalidPosition)?;
    let old_edit = old_map.cell_navigation_ranges()[0][0].start..old_second_cell.start;
    assert!(preserve_align_metadata(
        &previous,
        &mut updated,
        &old_edit,
        &(new_range.start..new_range.start),
        &previous_map,
        &updated_map,
    ));
    assert_eq!(updated.rows[0].cells[0].root.id, surviving_cell_id);
    assert_eq!(updated.rows[0].cells[0].extra["vendorCell"], "survives");
    Ok(())
}

#[test]
fn align_metadata_matches_rows_with_trivia_around_an_edited_cell() -> Result<(), EditError> {
    let previous_source = "\\begin{aligned}a\\\\ % before\n \\sqrt{} % after\n \\\\b\\end{aligned}";
    let updated_source = "\\begin{aligned}a\\\\ % before\n % after\n \\\\b\\end{aligned}";
    let previous_projection =
        caret_math::parse_projection(previous_source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
    let updated_projection =
        caret_math::parse_projection(updated_source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
    let (caret_math::ProjectedMath::Align(mut previous), previous_map) =
        previous_projection.into_parts()
    else {
        return Err(EditError::InvalidPosition);
    };
    let (caret_math::ProjectedMath::Align(mut updated), updated_map) =
        updated_projection.into_parts()
    else {
        return Err(EditError::InvalidPosition);
    };
    let row_id = previous.rows[1].id.clone();
    let cell_id = previous.rows[1].cells[0].root.id.clone();
    previous.rows[1]
        .extra
        .insert("vendorRow".to_owned(), serde_json::json!("kept"));
    previous.rows[1].cells[0]
        .extra
        .insert("vendorCell".to_owned(), serde_json::json!("kept"));
    let old_edit_start = previous_source
        .find(r"\sqrt{}")
        .ok_or(EditError::InvalidPosition)?;
    let old_edit = old_edit_start..old_edit_start + r"\sqrt{}".len();
    let new_edit = old_edit_start..old_edit_start;

    assert_eq!(previous.rows.len(), 3);
    assert_eq!(updated.rows.len(), 3);
    assert!(preserve_align_metadata(
        &previous,
        &mut updated,
        &old_edit,
        &new_edit,
        &previous_map,
        &updated_map,
    ));
    assert_eq!(updated.rows[1].id, row_id);
    assert_eq!(updated.rows[1].extra["vendorRow"], "kept");
    assert_eq!(updated.rows[1].cells[0].root.id, cell_id);
    assert_eq!(updated.rows[1].cells[0].extra["vendorCell"], "kept");
    Ok(())
}

#[test]
fn reparsing_unranged_math_nodes_never_reuses_an_identity_twice() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\sum".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::InsertText(" ".to_owned()))?;

    session.document().validate().map_err(|error| {
        EditError::InvalidDocument(format!("math node IDs should remain unique: {error}"))
    })?;
    session
        .document()
        .validate()
        .map_err(|error| EditError::InvalidDocument(error.to_string()))
}

#[test]
fn stale_math_revision_keeps_pending_draft_until_explicit_escape()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::InsertText("\\".to_owned()))?;
    assert!(session.has_uncommitted_math_source_draft());

    let committed_document = session.document().clone();
    let committed_source = committed_inline_source(&session);
    let selection = session.selection().clone();
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let generation = session.document_generation();
    let can_undo = session.can_undo();
    let can_redo = session.can_redo();
    session.run(Command::BeginCommandLine)?;
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
        session.run(Command::InsertText("y".to_owned())),
        Err(EditError::InvalidPosition)
    ));
    assert!(matches!(
        session.run(Command::SetMathTextSelection {
            anchor: 0,
            focus: 0
        }),
        Err(EditError::InvalidPosition)
    ));
    assert!(matches!(
        session.run(Command::Undo),
        Err(EditError::InvalidPosition)
    ));
    assert!(session.begin_save_checkpoint().is_err());
    assert_eq!(session.document(), &committed_document);
    assert_eq!(committed_inline_source(&session), committed_source);
    assert_eq!(session.selection(), &selection);
    assert_eq!(session.document_generation(), generation);
    assert!(session.has_uncommitted_math_source_draft());
    assert_eq!(session.math_edit(), Some(&stale_edit));
    assert_eq!(session.can_undo(), can_undo);
    assert_eq!(session.can_redo(), can_redo);

    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.command_line().is_none());
    assert_eq!(session.math_edit(), Some(&stale_edit));
    assert!(session.has_uncommitted_math_source_draft());
    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.document(), &committed_document);
    assert_eq!(committed_inline_source(&session), committed_source);
    assert_eq!(session.selection(), &selection);
    assert_eq!(session.document_generation(), generation);
    assert!(!session.has_uncommitted_math_source_draft());
    assert_eq!(session.can_undo(), can_undo);
    assert_eq!(session.can_redo(), can_redo);
    assert!(edit.source_draft_is_pending());
    Ok(())
}

#[test]
fn restore_math_edit_reuses_matching_projection_and_rejects_mismatched_kind()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let position = history_position(&edit);

    let revision = session.current_math_edit_revision();
    let restored = restore_math_edit(&session.document, position.clone(), revision.clone())
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(restored.revision, revision);
    assert!(Arc::ptr_eq(
        restored
            .source_projection
            .as_ref()
            .ok_or(EditError::InvalidPosition)?,
        edit.source_projection
            .as_ref()
            .ok_or(EditError::InvalidPosition)?
    ));

    let mut stale_position = position;
    stale_position.source_projection = Some(Arc::new(
        caret_math::parse_projection("other", caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
            .into_parts()
            .1,
    ));
    let stale_revision = session.current_math_edit_revision();
    assert!(restore_math_edit(&session.document, stale_position, stale_revision).is_none());
    Ok(())
}

#[test]
fn failed_math_history_restore_rolls_back_document_patch_and_keeps_history_position()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some(""));
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .cursor = usize::MAX;
    session.run(Command::Delete(Direction::Backward))?;
    assert!(session.math_edit().is_none());
    let document_before_failed_undo = session.document().clone();
    let selection_before_failed_undo = session.selection().clone();

    assert!(matches!(
        session.run(Command::Undo),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before_failed_undo);
    assert_eq!(session.selection(), &selection_before_failed_undo);
    assert!(session.math_edit().is_none());
    assert!(session.edit_history.can_undo());
    Ok(())
}

#[test]
fn reentering_inline_math_uses_canonical_source_and_keeps_host_side()
-> Result<(), Box<dyn std::error::Error>> {
    let mut from_left = session_with_text("")?;
    from_left.run(Command::InsertInlineMath("$x$".to_owned()))?;
    let paragraph_id = from_left.selection().focus.paragraph_id.clone();
    let left = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    from_left.run(Command::SetSelection(Some(Selection {
        anchor: left.clone(),
        focus: left,
    })))?;
    from_left.run(Command::EnterMath(Direction::Forward))?;
    assert_eq!(from_left.math_edit().map(|edit| edit.cursor), Some(0));
    from_left.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(inline_source(&from_left).as_deref(), Some("yx"));

    let mut from_right = session_with_text("")?;
    from_right.run(Command::InsertInlineMath(r"\(x\)".to_owned()))?;
    from_right.run(Command::EnterMath(Direction::Backward))?;
    assert_eq!(from_right.math_edit().map(|edit| edit.cursor), Some(1));
    from_right.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(inline_source(&from_right).as_deref(), Some("xy"));
    from_right.run(Command::MoveCaret(Direction::Forward))?;
    assert!(from_right.math_edit().is_none());

    let mut from_left = session_with_text("")?;
    from_left.run(Command::InsertInlineMath(r"\(x\)".to_owned()))?;
    let paragraph_id = from_left.selection().focus.paragraph_id.clone();
    let left = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    from_left.run(Command::SetSelection(Some(Selection {
        anchor: left.clone(),
        focus: left,
    })))?;
    from_left.run(Command::EnterMath(Direction::Forward))?;
    assert_eq!(from_left.math_edit().map(|edit| edit.cursor), Some(0));
    from_left.run(Command::MoveCaret(Direction::Backward))?;
    assert!(from_left.math_edit().is_none());
    Ok(())
}

#[test]
fn incomplete_math_source_blocks_exit_until_repaired_or_cancelled() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::InsertText("\\".to_owned()))?;
    let document = session.document().clone();
    let edit = session.math_edit().cloned();

    assert!(matches!(
        session.run(Command::EditorKey(EditorKey::Enter)),
        Err(EditError::InvalidMath(_))
    ));
    assert!(matches!(
        session.run(Command::EditorKey(EditorKey::ModBackspace)),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &document);
    assert_eq!(session.math_edit(), edit.as_ref());

    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.document(), &document);
    Ok(())
}

#[test]
fn incomplete_math_draft_blocks_document_settings_without_invalidating_its_revision()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::InsertText("\\".to_owned()))?;
    let document = session.document().clone();
    let generation = session.document_generation();
    let source = session
        .math_edit()
        .and_then(MathEdit::source)
        .map(str::to_owned);

    for command in [
        Command::SetPageGeometry(caret_doc::PageGeometry {
            paper_size: caret_doc::PaperSize::A3,
            orientation: caret_doc::Orientation::Landscape,
            margins: caret_doc::PageMargins {
                top: 36.0,
                right: 54.0,
                bottom: 72.0,
                left: 90.0,
            },
        }),
        Command::SetDefaultFontFamily(Some("Noto Sans".to_owned())),
        Command::SetDocumentFontFamily {
            target: caret_doc::DocumentFontTarget::Script("Latn".to_owned()),
            family: Some("Noto Sans".to_owned()),
        },
    ] {
        assert!(matches!(
            session.run(command),
            Err(EditError::InvalidMath(_))
        ));
        assert_eq!(session.document(), &document);
        assert_eq!(session.document_generation(), generation);
        assert_eq!(
            session.math_edit().and_then(MathEdit::source),
            source.as_deref()
        );
    }

    session.run(Command::InsertText("alpha".to_owned()))?;
    assert_eq!(
        session.math_edit().and_then(MathEdit::source),
        Some(r"x\alpha")
    );
    Ok(())
}

#[test]
fn deleting_the_last_align_cell_content_preserves_the_trailing_row_through_history()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "\\begin{aligned}a\\\\ % before\n \\sqrt{} % after\n\\end{aligned}";
    let parsed = caret_math::parse_projection(source, caret_math::MathInputKind::Align)?;
    let (caret_math::ProjectedMath::Align(mut align), projection) = parsed.into_parts() else {
        return Err("Align source should produce an Align tree".into());
    };
    assert_eq!(align.rows.len(), 2);
    let block_id = align.id.clone();
    let row_id = align.rows[1].id.clone();
    let cell_id = align.rows[1].cells[0].root.id.clone();
    align.rows[1]
        .extra
        .insert("vendorRow".to_owned(), serde_json::json!("keep row"));
    align.rows[1].cells[0]
        .extra
        .insert("vendorCell".to_owned(), serde_json::json!("keep cell"));
    let edit_range = projection.edit_range();
    let cell_range = projection
        .align()
        .and_then(|map| map.cell_range(1, 0))
        .ok_or(EditError::InvalidPosition)?;
    let source_base = cell_range
        .start
        .checked_sub(edit_range.start)
        .ok_or(EditError::InvalidPosition)?;
    let cursor = source_base + r"\sqrt{".len();
    let mut document = Document::blank();
    document.body.push(Block::Align(align));
    let mut session = Session::new(document)?;
    session.math_edit = Some(MathEdit {
        target: MathEditTarget::Block {
            block_id: block_id.clone(),
            align: true,
        },
        source_projection: Some(Arc::new(projection)),
        source_draft: None,
        revision: MathEditRevision {
            session_id: session.session_id.clone(),
            document_generation: session.document_generation,
            projection_id: NodeId::generate(),
        },
        cursor,
        branch: MathEditBranch::Source,
    });

    let deletion = session.run(Command::Delete(Direction::Backward))?;
    assert!(deletion.document_changed);
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 1, 0)?;
    let deleted = first_align(&session)?;
    assert_eq!(deleted.rows.len(), 2);
    assert_eq!(deleted.rows[1].id, row_id);
    assert_eq!(deleted.rows[1].extra["vendorRow"], "keep row");
    assert_eq!(deleted.rows[1].cells[0].root.id, cell_id);
    assert_eq!(deleted.rows[1].cells[0].extra["vendorCell"], "keep cell");
    assert!(!caret_math::serialize_tree(&deleted.rows[1].cells[0]).contains(r"\sqrt"));

    session.run(Command::Undo)?;
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 1, 0)?;
    let restored = first_align(&session)?;
    assert_eq!(restored.rows.len(), 2);
    assert_eq!(restored.rows[1].id, row_id);
    assert_eq!(restored.rows[1].cells[0].root.id, cell_id);
    assert!(caret_math::serialize_tree(&restored.rows[1].cells[0]).contains(r"\sqrt"));

    session.run(Command::Redo)?;
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 1, 0)?;
    assert_eq!(first_align(&session)?.rows.len(), 2);
    Ok(())
}

#[test]
fn softbreak_is_content_not_an_empty_align_row() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    let row = &first_align(&session)?.rows[0];
    let path = MathSequencePath::root();
    let anchor = MathTreePosition {
        host: MathPositionHost::AlignCell {
            row_id: row.id.clone(),
            cell_id: row.cells[0].root.id.clone(),
        },
        path: path.clone(),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    let Block::Align(align) = session
        .document
        .body
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    align.rows[0].cells[0]
        .root
        .content
        .push(MathContent::SoftBreak(caret_doc::SoftBreak::default()));
    let edit = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    edit.cursor = 0;
    edit.branch = MathEditBranch::Tree { path, anchor };

    let effect = session.run(Command::EditorKey(EditorKey::Backspace))?;

    assert!(!effect.document_changed);
    assert_eq!(first_align(&session)?.rows.len(), 1);
    assert_eq!(
        first_align(&session)?.rows[0].cells[0].root.content,
        vec![MathContent::SoftBreak(caret_doc::SoftBreak::default())]
    );
    Ok(())
}

#[test]
fn unmatched_closing_brace_is_not_silently_discarded() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    let generation = session.document_generation();
    let result = session.run(Command::PasteText("x}y".to_owned()));

    assert!(matches!(result, Err(EditError::InvalidMath(_))));
    assert_eq!(inline_source(&session).as_deref(), Some(""));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    assert_eq!(session.document_generation(), generation);
    Ok(())
}

#[test]
fn adjacent_block_after_stop_returns_to_the_following_paragraph() -> Result<(), EditError> {
    let mut document = Document::blank();
    let Block::Equation(equation) = display_math_block(false, "x")? else {
        return Err(EditError::InvalidPosition);
    };
    let equation_id = equation.id.clone();
    let mut paragraph = Paragraph::empty(NodeId::generate());
    let inline_id = NodeId::generate();
    paragraph.inlines = vec![ParagraphContent::Environment(
        caret_doc::InlineEnvironment {
            id: inline_id.clone(),
            kind: caret_doc::InlineEnvironmentKind::Bold,
            definition_id: None,
            props: BTreeMap::new(),
            children: Vec::new(),
            extra: BTreeMap::new(),
        },
    )];
    let paragraph_id = paragraph.id.clone();
    document.body = vec![Block::Equation(equation), Block::Paragraph(paragraph)];
    let mut session = Session::new(document)?;
    let paragraph_start = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 0,
        affinity: Affinity::Upstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: paragraph_start.clone(),
        focus: paragraph_start.clone(),
    })))?;

    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: false,
    }))?;
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(HostCaretPosition::After {
            host_id,
            kind: HostCaretKind::Block,
        }) if host_id == &equation_id
    ));
    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: false,
    }))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { block_id, align: false }) if block_id == &equation_id
    ));
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: false,
    }))?;
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(HostCaretPosition::After {
            host_id,
            kind: HostCaretKind::Block,
        }) if host_id == &equation_id
    ));
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: false,
    }))?;
    assert_eq!(session.selection().focus, paragraph_start);
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(HostCaretPosition::Before {
            host_id,
            kind: HostCaretKind::Inline,
        }) if host_id == &inline_id
    ));
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: false,
    }))?;
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(HostCaretPosition::Inside {
            host_id,
            kind: HostCaretKind::Inline,
            ..
        }) if host_id == &inline_id
    ));
    Ok(())
}

#[test]
fn math_pending_explicit_off_is_written_even_when_the_text_flow_is_unset() -> Result<(), EditError>
{
    for (kind, expected) in [
        (InlineEnvironmentKind::Bold, r"\textmd{x}"),
        (InlineEnvironmentKind::Italic, r"\textup{x}"),
    ] {
        let mut session = session_with_text("").map_err(test_error)?;
        insert_equation_text(&mut session, r"\text{}")?;
        move_into_math_text(&mut session)?;
        session.run(Command::ToggleInlineFormat(kind))?;
        session.run(Command::ToggleInlineFormat(kind))?;
        session.run(Command::InsertText("x".to_owned()))?;
        assert!(inline_source(&session).is_some_and(|source| source.contains(expected)));
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text { path, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        assert_eq!(
            super::text_inline_environment::inherited_environment_value(
                session.math_tree_for_edit(edit, path.align_cell())?,
                path,
                kind
            ),
            Some(false)
        );
    }
    Ok(())
}
