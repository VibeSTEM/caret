//! 公式编辑测试：tree ops。

use super::*;

#[test]
fn discontinuous_text_source_map_never_deletes_the_gap_between_entries() {
    let source = "\\text{ab% skipped by TeX\ncd}";
    let projection = caret_math::parse_projection(source, caret_math::MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("text source should project: {error}"));
    let caret_math::ProjectedMath::Tree(tree) = projection.content() else {
        panic!("inline text should project to a math tree");
    };
    let Some(MathContent::Text(text)) = tree.root.content.first() else {
        panic!("source should project to a TextFlow");
    };
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abcd"
    );
    let text_map = projection
        .source_map()
        .tree()
        .and_then(|map| map.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(caret_math::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("TextFlow should have a sidecar mapping"));
    assert!(text_map.segments().windows(2).any(|segments| {
        segments[0]
            .source()
            .zip(segments[1].source())
            .is_some_and(|(left, right)| left.end < right.start)
    }));

    let boundary = text_map
        .segments()
        .iter()
        .find(|segment| segment.display().start == 2)
        .and_then(|segment| segment.source())
        .map(|range| range.start)
        .unwrap_or_else(|| panic!("second text island segment should have a source range"));
    let preceding_end = text_map
        .segments()
        .first()
        .and_then(|segment| segment.source())
        .map(|range| range.end)
        .unwrap_or_else(|| panic!("first text island segment should have a source range"));
    assert_eq!(
        text_source_boundary_from_map(text, text_map, 0, 2, Affinity::Upstream),
        Some(preceding_end)
    );
    assert_eq!(
        text_source_boundary_from_map(text, text_map, 0, 2, Affinity::Downstream),
        Some(boundary)
    );
    assert_eq!(
        text_source_range_from_map(text_map, 0, 2..4),
        Some(boundary..boundary + 2)
    );
    assert_eq!(text_source_range_from_map(text_map, 0, 1..3), None);
}

#[test]
fn text_path_resolves_against_sidecar_when_tree_has_no_projection_metadata() {
    let (content, source_projection) =
        caret_math::parse_projection(r"\text{a\%b}", caret_math::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("escaped text should project: {error}"))
            .into_parts();
    let caret_math::ProjectedMath::Tree(tree) = content else {
        panic!("inline source should project to a math tree");
    };
    let Some(MathContent::Text(_text)) = tree.root.content.first() else {
        panic!("source should project to a TextFlow");
    };
    let path = MathTextFlowPath::new(vec![caret_doc::MathPathStep::Content(0)], None);

    assert!(path.resolve(&tree).is_some());
    assert!(
        path.resolve_in_projection(&tree, &source_projection)
            .is_some()
    );
}

#[test]
fn text_path_uses_sidecar_ranges_independent_of_the_document_tree() {
    let (content, source_projection) =
        caret_math::parse_projection(r"\text{ab}", caret_math::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("text should project: {error}"))
            .into_parts();
    let caret_math::ProjectedMath::Tree(mut tree) = content else {
        panic!("inline source should project to a math tree");
    };
    let Some(MathContent::Text(_text)) = tree.root.content.first() else {
        panic!("source should project to a TextFlow");
    };
    let path = MathTextFlowPath::new(vec![caret_doc::MathPathStep::Content(0)], None);
    assert!(
        path.resolve_in_projection(&tree, &source_projection)
            .is_some()
    );
    let Some(MathContent::Text(text)) = tree.root.content.first_mut() else {
        panic!("source should retain a TextFlow");
    };
    text.runs[0].text = "a".to_owned();
    assert!(
        path.resolve_in_projection(&tree, &source_projection)
            .is_none()
    );
}

#[test]
fn inline_math_root_split_preserves_an_empty_math_anchor_inside_text() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\text{a$$b}".to_owned()))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(matches!(
        paragraph.inlines.as_slice(),
        [
            ParagraphContent::Text(_),
            ParagraphContent::Math(math),
            ParagraphContent::Text(_)
        ] if math.math_tree.root.content.iter().any(|item| matches!(
            item,
            MathContent::Math(node) if node.kind == caret_doc::MathNodeKind::Placeholder
        ))
    ));
    Ok(())
}

#[test]
fn inline_math_text_inside_fixed_operand_stays_in_math_tree() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(r"\frac{a}{\text{x}}".to_owned()))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let [ParagraphContent::Math(math)] = paragraph.inlines.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(
        first_paragraph_text(&session),
        OBJECT_REPLACEMENT.to_string()
    );
    let Some(MathContent::Math(fraction)) = math.math_tree.root.content.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(fraction.children.iter().any(|flow| {
        flow.content
            .iter()
            .any(|content| matches!(content, MathContent::Text(_)))
    }));
    Ok(())
}

#[test]
fn mod_backspace_does_not_touch_a_paragraph_adjacent_to_display_math() -> Result<(), EditError> {
    let mut session = session_with_text("outside").map_err(test_error)?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::InsertText(" bold".to_owned()))?;
    session.insert_display_math(false)?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: false, .. })
    ));
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();

    let effect = session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(!effect.document_changed);
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn mod_backspace_rejects_unpreservable_math_metadata_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath("x^2".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
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
    math.math_tree
        .extra
        .insert("vendorField".to_owned(), serde_json::Value::Bool(true));

    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    assert_eq!(
        session.run(Command::EditorKey(EditorKey::ModBackspace)),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn align_candidate_is_disabled_when_insertion_would_exceed_document_depth() -> Result<(), EditError>
{
    let paragraph = Paragraph::empty(NodeId::generate());
    let paragraph_id = paragraph.id.clone();
    let mut block = Block::Paragraph(paragraph);
    for _ in 0..caret_doc::MAX_DOCUMENT_NESTING_DEPTH {
        block = Block::Environment(caret_doc::BlockEnvironment {
            id: NodeId::generate(),
            kind: caret_doc::BlockEnvironmentKind::Section,
            definition_id: None,
            props: Default::default(),
            title: None,
            children: vec![block],
            extra: Default::default(),
        });
    }
    let mut document = caret_doc::Document::blank();
    document.body = vec![block];
    assert!(document.validate().is_ok());

    let mut session = Session::new(document)?;
    assert_eq!(session.selection().focus.paragraph_id, paragraph_id);
    assert_eq!(
        session
            .paragraph_index
            .get(&paragraph_id)
            .map(|location| location.block_path.len()),
        Some(caret_doc::MAX_DOCUMENT_NESTING_DEPTH + 1)
    );
    assert!(!session.can_insert_display_math(true));
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("align".to_owned()))?;
    let candidate = session
        .command_line_candidates()
        .into_iter()
        .find(|candidate| candidate.alias == r"\align")
        .ok_or(EditError::InvalidPosition)?;
    assert!(!candidate.available);

    let document_before = session.document().clone();
    let selection_before = session.selection().clone();
    let generation_before = session.document_generation();
    let history_before = session.history_state_id();
    session.run(Command::AcceptCommandLine)?;
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert!(session.command_line().is_some());
    assert!(session.math_edit().is_none());
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_before);
    assert!(caret_doc::CdocFile::new(session.document().clone()).is_ok());

    session.run(Command::CancelCommandLine)?;
    assert!(session.insert_display_math(true).is_err());
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_before);
    assert!(session.math_edit().is_none());

    let mut equation_session = Session::new(session.document().clone())?;
    equation_session.run(Command::BeginCommandLine)?;
    equation_session.run(Command::TypeCommandLine("equation".to_owned()))?;
    assert!(
        equation_session
            .command_line_candidates()
            .iter()
            .any(|candidate| { candidate.alias == r"\equation" && candidate.available })
    );
    equation_session.run(Command::AcceptCommandLine)?;
    assert!(caret_doc::CdocFile::new(equation_session.document().clone()).is_ok());
    Ok(())
}

#[test]
fn math_insert_and_split_candidates_are_disabled_for_non_writable_documents()
-> Result<(), EditError> {
    let mut document = caret_doc::Document::blank();
    document.body.push(Block::Opaque(caret_doc::OpaqueNode {
        raw: serde_json::json!({"type": "future"}),
    }));
    assert!(document.validate().is_ok());
    assert!(caret_doc::CdocFile::new(document.clone()).is_err());

    let mut insert_session = Session::new(document)?;
    insert_session.run(Command::BeginCommandLine)?;
    insert_session.run(Command::TypeCommandLine("math".to_owned()))?;
    let candidate = insert_session
        .command_line_candidates()
        .into_iter()
        .find(|candidate| candidate.alias == r"\math")
        .ok_or(EditError::InvalidPosition)?;
    assert!(!candidate.available);
    let document_before = insert_session.document().clone();
    let selection_before = insert_session.selection().clone();
    let generation_before = insert_session.document_generation();
    let history_before = insert_session.history_state_id();
    insert_session.run(Command::AcceptCommandLine)?;
    assert_eq!(insert_session.document(), &document_before);
    assert_eq!(insert_session.selection(), &selection_before);
    assert_eq!(insert_session.document_generation(), generation_before);
    assert_eq!(insert_session.history_state_id(), history_before);
    assert!(insert_session.command_line().is_some());
    insert_session.run(Command::CancelCommandLine)?;
    assert!(matches!(
        insert_session.run(Command::InsertInlineMath("x".to_owned())),
        Err(EditError::InvalidDocument(_))
    ));
    assert_eq!(insert_session.document(), &document_before);
    assert_eq!(insert_session.selection(), &selection_before);
    assert_eq!(insert_session.document_generation(), generation_before);
    assert_eq!(insert_session.history_state_id(), history_before);

    let mut split_session = session_with_text("").map_err(test_error)?;
    split_session.run(Command::InsertInlineMath(r"\text{abc}".to_owned()))?;
    split_session.run(Command::EnterMath(Direction::Backward))?;
    split_session
        .document
        .body
        .push(Block::Opaque(caret_doc::OpaqueNode {
            raw: serde_json::json!({"type": "future"}),
        }));
    split_session.run(Command::BeginCommandLine)?;
    split_session.run(Command::TypeCommandLine("math".to_owned()))?;
    let candidate = split_session
        .command_line_candidates()
        .into_iter()
        .find(|candidate| candidate.alias == r"\math")
        .ok_or(EditError::InvalidPosition)?;
    assert!(!candidate.available);
    let document_before = split_session.document().clone();
    let selection_before = split_session.selection().clone();
    let edit_before = split_session.math_edit().cloned();
    let generation_before = split_session.document_generation();
    let history_before = split_session.history_state_id();
    split_session.run(Command::AcceptCommandLine)?;
    assert_eq!(split_session.document(), &document_before);
    assert_eq!(split_session.selection(), &selection_before);
    assert_eq!(split_session.math_edit(), edit_before.as_ref());
    assert_eq!(split_session.document_generation(), generation_before);
    assert_eq!(split_session.history_state_id(), history_before);
    assert!(split_session.command_line().is_some());
    Ok(())
}

#[test]
fn mod_backspace_rejects_math_trees_whose_tex_projection_exceeds_the_source_budget_atomically()
-> Result<(), EditError> {
    let max_source_bytes = caret_math::MAX_MATH_SOURCE_BYTES;
    let cases = [
        "tree source exceeds budget".to_owned(),
        "escaped TeX projection exceeds budget".to_owned(),
    ];
    for (case, text) in cases.into_iter().zip([
        "x".repeat(max_source_bytes + 1),
        "#".repeat(max_source_bytes / 2 + 8),
    ]) {
        let mut session = session_with_text("").map_err(test_error)?;
        session.run(Command::InsertInlineMath(r"\frac{x}{\text{x}}".to_owned()))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        let Some(MathEditTarget::Inline { math_id, .. }) =
            session.math_edit().map(|edit| &edit.target)
        else {
            return Err(EditError::InvalidPosition);
        };
        let math_id = math_id.clone();
        let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
            return Err(EditError::InvalidPosition);
        };
        let math = find_inline_math_mut(&mut paragraph.inlines, &math_id)
            .ok_or(EditError::InvalidPosition)?;
        let text_flow =
            first_math_text_flow_mut(&mut math.math_tree.root).ok_or(EditError::InvalidPosition)?;
        text_flow.runs[0].text = text;
        assert!(math.math_tree.validate().is_ok(), "{case}");
        assert!(
            caret_math::serialize_tree(&math.math_tree).len() > max_source_bytes,
            "{case}"
        );

        let before_document = session.document().clone();
        let before_edit = session.math_edit().cloned();
        let before_selection = session.selection().clone();
        let before_generation = session.document_generation();
        assert_eq!(
            session.run(Command::EditorKey(EditorKey::ModBackspace)),
            Err(EditError::UnsupportedOpaqueContent),
            "{case}"
        );
        assert_eq!(session.document(), &before_document, "{case}");
        assert_eq!(session.math_edit(), before_edit.as_ref(), "{case}");
        assert_eq!(session.selection(), &before_selection, "{case}");
        assert_eq!(session.document_generation(), before_generation, "{case}");
    }
    Ok(())
}

#[test]
fn mod_backspace_rejects_text_and_environment_metadata_atomically() -> Result<(), EditError> {
    for (source, metadata_kind) in [
        (r"\frac{x}{\text{abc}}", 0),
        (r"\frac{x}{\text{abc}}", 1),
        (r"\frac{x}{\text{\textsf{abc}}}", 2),
    ] {
        let mut session = session_with_text("").map_err(test_error)?;
        session.run(Command::InsertInlineMath(source.to_owned()))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        let Some(MathEditTarget::Inline { math_id, .. }) =
            session.math_edit().map(|edit| &edit.target)
        else {
            return Err(EditError::InvalidPosition);
        };
        let math_id = math_id.clone();
        let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
            return Err(EditError::InvalidPosition);
        };
        let math = find_inline_math_mut(&mut paragraph.inlines, &math_id)
            .ok_or(EditError::InvalidPosition)?;
        if metadata_kind < 2 {
            let text = first_math_text_flow_mut(&mut math.math_tree.root)
                .ok_or(EditError::InvalidPosition)?;
            if metadata_kind == 0 {
                text.runs[0]
                    .props
                    .insert("underline".to_owned(), serde_json::Value::Bool(true));
            } else {
                text.extra
                    .insert("vendorField".to_owned(), serde_json::Value::Bool(true));
            }
        } else {
            let environment = first_math_environment_mut(&mut math.math_tree.root)
                .ok_or(EditError::InvalidPosition)?;
            environment
                .extra
                .insert("vendorField".to_owned(), serde_json::Value::Bool(true));
        }

        let before_document = session.document().clone();
        let before_edit = session.math_edit().cloned();
        let before_generation = session.document_generation();
        assert_eq!(
            session.run(Command::EditorKey(EditorKey::ModBackspace)),
            Err(EditError::UnsupportedOpaqueContent)
        );
        assert_eq!(session.document(), &before_document);
        assert_eq!(session.math_edit(), before_edit.as_ref());
        assert_eq!(session.document_generation(), before_generation);
    }
    Ok(())
}

#[test]
fn mod_backspace_rejects_tree_only_content_atomically() -> Result<(), EditError> {
    for metadata_kind in 0..3 {
        let mut session = session_with_text("").map_err(test_error)?;
        session.run(Command::InsertInlineMath("x".to_owned()))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        let Some(MathEditTarget::Inline { math_id, .. }) =
            session.math_edit().map(|edit| &edit.target)
        else {
            return Err(EditError::InvalidPosition);
        };
        let math_id = math_id.clone();
        let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
            return Err(EditError::InvalidPosition);
        };
        let math = find_inline_math_mut(&mut paragraph.inlines, &math_id)
            .ok_or(EditError::InvalidPosition)?;
        match metadata_kind {
            0 => {
                let children = std::mem::take(&mut math.math_tree.root.content);
                math.math_tree.root.content =
                    vec![MathContent::Environment(caret_doc::InlineEnvironment {
                        id: NodeId::generate(),
                        kind: caret_doc::InlineEnvironmentKind::Bold,
                        definition_id: None,
                        props: BTreeMap::new(),
                        children,
                        extra: BTreeMap::from([(
                            "unrepresentable".to_owned(),
                            serde_json::json!(true),
                        )]),
                    })];
            }
            1 => math
                .math_tree
                .root
                .content
                .push(MathContent::Text(caret_doc::TextFlow {
                    runs: vec![Run {
                        text: "secret".to_owned(),
                        language: Some("en".to_owned()),
                        props: BTreeMap::new(),
                        extra: BTreeMap::new(),
                    }],
                    extra: BTreeMap::new(),
                })),
            _ => {
                let Some(MathContent::Math(mut node)) =
                    math.math_tree.root.content.first().cloned()
                else {
                    return Err(EditError::InvalidPosition);
                };
                node.text = Some("y".to_owned());
                node.extra
                    .insert("unrepresentable".to_owned(), serde_json::json!(true));
                math.math_tree.root.content.push(MathContent::Math(node));
            }
        }

        let before_document = session.document().clone();
        let before_edit = session.math_edit().cloned();
        let before_generation = session.document_generation();
        assert_eq!(
            session.run(Command::EditorKey(EditorKey::ModBackspace)),
            Err(EditError::UnsupportedOpaqueContent)
        );
        assert_eq!(session.document(), &before_document);
        assert_eq!(session.math_edit(), before_edit.as_ref());
        assert_eq!(session.document_generation(), before_generation);
    }
    Ok(())
}

#[test]
fn mod_backspace_materializes_empty_math_as_empty_text_and_is_undoable() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath(String::new()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let before = session.document().clone();

    let effect = session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(effect.document_changed);
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), "");
    assert!(matches!(
        session.document().body.first(),
        Some(Block::Paragraph(Paragraph { inlines, .. }))
            if matches!(inlines.as_slice(), [ParagraphContent::Text(flow)] if flow.runs.len() == 1 && flow.runs[0].text.is_empty())
    ));

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &before);
    assert!(session.math_edit().is_some());
    session.run(Command::Redo)?;
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn mod_backspace_accepts_font_properties_reconstructed_from_math_source() -> Result<(), EditError> {
    for source in [
        r"\frac{x}{\text{\textsf{sans}}}",
        r"\frac{x}{\text{\texttt {mono}}}",
    ] {
        let mut session = session_with_text("").map_err(test_error)?;
        session.run(Command::InsertInlineMath(source.to_owned()))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        let Some(MathEditTarget::Inline { math_id, .. }) =
            session.math_edit().map(|edit| &edit.target)
        else {
            return Err(EditError::InvalidPosition);
        };
        let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
            return Err(EditError::InvalidPosition);
        };
        let math =
            find_inline_math(&paragraph.inlines, math_id).ok_or(EditError::InvalidPosition)?;
        assert!(math_tree_has_font_environment(&math.math_tree.root));
        let canonical = caret_math::serialize_tree(&math.math_tree);
        session.run(Command::EditorKey(EditorKey::ModBackspace))?;
        assert_eq!(first_paragraph_text(&session), canonical);
        assert!(session.math_edit().is_none());
    }
    Ok(())
}

#[test]
fn mod_backspace_downgrades_multiline_math_to_canonical_single_line_text() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath("x\ny".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;

    let canonical = {
        let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
            return Err(EditError::InvalidPosition);
        };
        let [ParagraphContent::Math(math)] = paragraph.inlines.as_slice() else {
            return Err(EditError::InvalidPosition);
        };
        caret_math::serialize_tree(&math.math_tree)
    };
    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert_eq!(first_paragraph_text(&session), canonical);
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn math_text_edge_insertion_does_not_inherit_from_the_opposite_run_side() -> Result<(), EditError> {
    for (offset, affinity, expected_text) in [
        (0, MathPositionAffinity::Upstream, "Xab"),
        (2, MathPositionAffinity::Downstream, "abX"),
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
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        let MathEditBranch::Text { path, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        let Block::Equation(equation) = session
            .document
            .body
            .iter_mut()
            .find(|block| matches!(block, Block::Equation(value) if &value.id == block_id))
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
        let run = flow.runs.first_mut().ok_or(EditError::InvalidPosition)?;
        run.language = Some("zh-Hans".to_owned());
        run.props
            .insert("underline".to_owned(), serde_json::Value::Bool(true));
        let position = MathTextPosition::new(path.clone(), offset, affinity);
        session.set_math_text_selection_positions(position.clone(), position)?;

        session.run(Command::InsertText("X".to_owned()))?;

        let flow = active_math_text_flow(&session)?;
        let inserted = flow
            .runs
            .iter()
            .find(|run| run.text.contains('X'))
            .ok_or(EditError::InvalidPosition)?;
        assert_eq!(inserted.language, None);
        assert!(!inserted.props.contains_key("underline"));
        assert_eq!(
            flow.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            expected_text
        );
    }
    Ok(())
}

#[test]
fn text_island_edit_is_tree_native_above_the_latex_source_limit() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{a}")?;
    move_into_math_text(&mut session)?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| edit.source_projection.is_none())
    );

    let inserted = "x".repeat(caret_math::MAX_MATH_SOURCE_BYTES + 1);
    session
        .run(Command::InsertText(inserted.clone()))
        .unwrap_or_else(|error| panic!("large TextFlow insertion failed: {error:?}"));
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| edit.source_projection.is_none())
    );
    assert!(
        inline_source(&session)
            .ok_or(EditError::InvalidPosition)?
            .len()
            > caret_math::MAX_MATH_SOURCE_BYTES
    );
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("large TextFlow undo failed: {error:?}"));
    assert_eq!(active_math_text_flow(&session)?.runs[0].text, "a");
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("large TextFlow redo failed: {error:?}"));
    assert_eq!(
        active_math_text_flow(&session)?.runs[0].text,
        format!("a{inserted}")
    );
    Ok(())
}

#[test]
fn math_text_run_property_splits_range_and_undoes_without_reparsing_source() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    let source = session
        .math_edit()
        .and_then(|edit| edit.source().map(str::to_owned));

    let effect = session.run(Command::ToggleRunUnderline)?;
    assert!(effect.document_changed);
    assert!(!effect.selection_changed);
    assert_eq!(
        session
            .math_edit()
            .and_then(|edit| edit.source().map(str::to_owned)),
        source
    );
    assert!(matches!(
        active_math_text_flow(&session)?.runs.as_slice(),
        [before, selected, after]
            if before.text == "a"
                && selected.text == "bc"
                && selected.props.get("underline") == Some(&serde_json::Value::Bool(true))
                && after.text == "d"
    ));

    session.run(Command::Undo)?;
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abcd"
    );
    assert_eq!(
        session.math_edit().and_then(|edit| edit.source()),
        source.as_deref()
    );
    session.run(Command::Redo)?;
    let redone = active_math_text_flow(&session)?;
    assert!(redone.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    Ok(())
}

#[test]
fn math_text_run_property_preserves_text_flow_extensions_and_undoes() -> Result<(), EditError> {
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
    let text =
        first_math_text_flow_mut(&mut math.math_tree.root).ok_or(EditError::InvalidPosition)?;
    text.extra.insert(
        "futureTextFlowField".to_owned(),
        serde_json::json!("preserved"),
    );

    session.run(Command::ToggleRunUnderline)?;
    let formatted = active_math_text_flow(&session)?;
    assert_eq!(
        formatted.extra.get("futureTextFlowField"),
        Some(&serde_json::json!("preserved"))
    );
    assert!(formatted.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));

    session.run(Command::Undo)?;
    let undone = active_math_text_flow(&session)?;
    assert_eq!(
        undone
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abcd"
    );
    assert_eq!(
        undone.extra.get("futureTextFlowField"),
        Some(&serde_json::json!("preserved"))
    );
    session.run(Command::Redo)?;
    let redone = active_math_text_flow(&session)?;
    assert!(redone.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    assert_eq!(
        redone.extra.get("futureTextFlowField"),
        Some(&serde_json::json!("preserved"))
    );
    Ok(())
}

#[test]
fn math_text_font_family_writes_environment_that_undo_redo_and_serialization_preserve()
-> Result<(), Box<dyn std::error::Error>> {
    let mut selected = inline_math_text_session_with_selection(r"\frac{x}{\text{abcd}}", 1, 3)?;
    let effect = selected.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    assert!(effect.document_changed);
    assert!(effect.selection_changed);
    let active_text = active_math_text_flow(&selected)?;
    assert_eq!(
        active_text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "bc"
    );
    assert!(math_tree_has_font_environment_for(
        &selected
            .math_tree_for_edit(
                selected.math_edit().ok_or(EditError::InvalidPosition)?,
                None,
            )?
            .root,
        "Noto Serif",
        "bc"
    ));
    selected.run(Command::Undo)?;
    assert!(!math_tree_has_font_environment(
        &selected
            .math_tree_for_edit(
                selected.math_edit().ok_or(EditError::InvalidPosition)?,
                None,
            )?
            .root
    ));
    selected.run(Command::Redo)?;
    assert!(math_tree_has_font_environment_for(
        &selected
            .math_tree_for_edit(
                selected.math_edit().ok_or(EditError::InvalidPosition)?,
                None,
            )?
            .root,
        "Noto Serif",
        "bc"
    ));
    selected.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 0,
    })?;
    selected.run(Command::SetInlineFontFamily("sans-serif".to_owned()))?;
    let reverse_edit = selected.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(reverse_edit.cursor, 0);
    assert!(matches!(
        &reverse_edit.branch,
        MathEditBranch::Text { anchor, .. } if anchor.offset == 2
    ));
    assert!(math_tree_has_font_environment_for(
        &selected.math_tree_for_edit(reverse_edit, None)?.root,
        "sans-serif",
        "bc"
    ));
    let serialized = caret_math::serialize_tree(selected.math_tree_for_edit(reverse_edit, None)?);
    let caret_math::ParsedMath::Tree(round_trip) =
        caret_math::parse(&serialized, caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    assert!(math_tree_has_font_environment_for(
        &round_trip.root,
        "sans-serif",
        "bc"
    ));
    Ok(())
}

#[test]
fn math_text_pending_font_family_tracks_history_and_styles_inserted_text()
-> Result<(), Box<dyn std::error::Error>> {
    let mut pending = inline_math_text_session_with_selection(r"\frac{x}{\textsf{ab}}", 1, 1)?;
    pending.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    assert_eq!(
        pending.pending_inline_style.active_font_family(),
        Some("Noto Serif")
    );
    pending.run(Command::SetInlineFontFamily("sans-serif".to_owned()))?;
    assert_eq!(
        pending.pending_inline_style.active_font_family(),
        Some("sans-serif")
    );
    pending.run(Command::Undo)?;
    assert_eq!(
        pending.pending_inline_style.active_font_family(),
        Some("Noto Serif")
    );
    pending.run(Command::Redo)?;
    pending.run(Command::InsertText("X".to_owned()))?;
    assert!(math_tree_has_font_environment_for(
        &pending
            .math_tree_for_edit(pending.math_edit().ok_or(EditError::InvalidPosition)?, None)?
            .root,
        "sans-serif",
        "X"
    ));
    Ok(())
}

#[test]
fn math_text_pending_run_property_styles_inserted_text_and_moves_clear_it()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    let original_edit = session.math_edit().cloned();
    session.run(Command::ToggleRunUnderline)?;
    assert!(session.pending_inline_style.is_active());
    session.run(Command::Undo)?;
    assert!(!session.pending_inline_style.is_active());
    assert_eq!(session.math_edit(), original_edit.as_ref());
    session.run(Command::Redo)?;
    assert!(session.pending_inline_style.is_active());
    session.run(Command::InsertText("X".to_owned()))?;
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "X" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(!session.pending_inline_style.is_active());

    let mut disable = session_with_text("")?;
    insert_equation_text(&mut disable, r"\text{a}")?;
    move_into_math_text(&mut disable)?;
    disable.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 1,
    })?;
    disable.run(Command::ToggleRunUnderline)?;
    disable.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 1,
    })?;
    disable.run(Command::ToggleRunUnderline)?;
    let flow = active_math_text_flow(&disable)?;
    assert!(
        flow.runs
            .iter()
            .all(|run| run.props.get("underline") != Some(&serde_json::Value::Bool(true)))
    );
    Ok(())
}

#[test]
fn starting_math_from_a_text_island_splits_the_text_source_and_is_undoable() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;

    session.begin_inline_math()?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}{}\text{cd}")
    );
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(math_flow_has_placeholder(
        &session.math_tree_for_edit(edit, None)?.root
    ));
    assert_eq!(edit.cursor, 1);
    assert!(matches!(
        &edit.branch,
        MathEditBranch::Tree { path, anchor }
            if path.steps().is_empty() && anchor.offset == 1
    ));

    session.run(Command::InsertText("x".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}x\text{cd}")
    );
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo of inserted math text failed: {error:?}"));
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}{}\text{cd}")
    );
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(math_flow_has_placeholder(
        &session.math_tree_for_edit(edit, None)?.root
    ));
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo of inserted math text failed: {error:?}"));
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}x\text{cd}")
    );
    for _ in 0..8 {
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
    session.run(Command::InsertText("z".to_owned()))?;
    assert_ne!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}x\text{cd}")
    );
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}x\text{cd}")
    );
    session.run(Command::Undo)?;
    session.run(Command::Undo)?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}x\text{cd}")
    );
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("second undo failed: {error:?}"));
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}{}\text{cd}")
    );
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo of math entry failed: {error:?}"));
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abcd}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    Ok(())
}

#[test]
fn starting_math_splits_known_run_properties_across_text_flows() -> Result<(), EditError> {
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
    let text =
        first_math_text_flow_mut(&mut equation.math_tree.root).ok_or(EditError::InvalidPosition)?;
    text.runs
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
        .props
        .insert("underline".to_owned(), serde_json::Value::Bool(true));

    session.begin_inline_math()?;
    session.run(Command::InsertText("x".to_owned()))?;
    let Some(Block::Equation(equation)) = session.document.body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let text_flows = equation
        .math_tree
        .root
        .content
        .iter()
        .filter_map(|content| match content {
            MathContent::Text(text) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(text_flows.len(), 2);
    assert_eq!(text_flows[0].runs[0].text, "ab");
    assert_eq!(text_flows[1].runs[0].text, "cd");
    for text in text_flows {
        assert_eq!(
            text.runs[0].props.get("underline"),
            Some(&serde_json::Value::Bool(true))
        );
    }
    Ok(())
}

#[test]
fn starting_math_inside_equation_text_flow_is_undoable() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "equation")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText(r"\text{abc}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;

    session.run(Command::InsertInlineMath(r"\(y\)".to_owned()))?;
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(
        caret_math::serialize_tree(&equation.math_tree),
        r"\text{a}y\text{bc}"
    );
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { path, .. }) if path.steps().is_empty()
    ));

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abc}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::Redo)?;
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(
        caret_math::serialize_tree(&equation.math_tree),
        r"\text{a}y\text{bc}"
    );
    Ok(())
}

#[test]
fn inserting_empty_text_deletes_a_selected_math_text_range() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 2,
    })?;
    session.run(Command::InsertText(String::new()))?;

    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{ac}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    Ok(())
}

#[test]
fn empty_fixed_math_operand_backspace_exits_or_removes_structure() -> Result<(), EditError> {
    for (source, expected_source) in [(r"\frac{x}{}", r"\frac{x}{}"), (r"\frac{}{}", "")] {
        let Block::Equation(equation) = display_math_block(false, source)? else {
            return Err(EditError::InvalidPosition);
        };
        let id = equation.id.clone();
        let mut document = Document::blank();
        document.body.push(Block::Equation(equation));
        let mut session = Session::new(document)?;
        let position = MathTreePosition {
            host: MathPositionHost::Equation {
                owner_id: id.clone(),
            },
            path: MathSequencePath::new(vec![MathSequenceStep::Operand {
                content_index: 0,
                operand_index: 1,
            }]),
            offset: 0,
            affinity: MathPositionAffinity::Downstream,
        };
        session.math_edit = Some(MathEdit {
            target: MathEditTarget::Block {
                block_id: id,
                align: false,
            },
            source_projection: None,
            source_draft: None,
            revision: session.current_math_edit_revision(),
            cursor: 0,
            branch: MathEditBranch::Tree {
                path: position.path.clone(),
                anchor: position,
            },
        });
        session.run(Command::Delete(Direction::Backward))?;
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree { path, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        assert!(path.steps().is_empty());
        assert_eq!(inline_source(&session).as_deref(), Some(expected_source));
    }
    Ok(())
}

#[test]
fn empty_math_operand_backspace_preserves_opaque_metadata_atomically() -> Result<(), EditError> {
    for annotation in 0..3 {
        let Block::Equation(mut equation) = display_math_block(false, r"\frac{}{}")? else {
            return Err(EditError::InvalidPosition);
        };
        let id = equation.id.clone();
        let Some(MathContent::Math(node)) = equation.math_tree.root.content.first_mut() else {
            return Err(EditError::InvalidPosition);
        };
        if annotation == 1 {
            node.children
                .get_mut(1)
                .ok_or(EditError::InvalidPosition)?
                .extra
                .insert("vendorOperand".to_owned(), serde_json::json!("preserve"));
        } else if annotation == 0 {
            node.extra
                .insert("vendorNode".to_owned(), serde_json::json!("preserve"));
        } else {
            let denominator = node.children.get_mut(1).ok_or(EditError::InvalidPosition)?;
            let Some(MathContent::Math(placeholder)) = denominator.content.first_mut() else {
                return Err(EditError::InvalidPosition);
            };
            placeholder.extra.insert(
                "vendorPlaceholder".to_owned(),
                serde_json::json!("preserve"),
            );
        }

        let mut document = Document::blank();
        document.body.push(Block::Equation(equation));
        let mut session = Session::new(document)?;
        let position = MathTreePosition {
            host: MathPositionHost::Equation {
                owner_id: id.clone(),
            },
            path: MathSequencePath::new(vec![MathSequenceStep::Operand {
                content_index: 0,
                operand_index: 1,
            }]),
            offset: 0,
            affinity: MathPositionAffinity::Downstream,
        };
        session.math_edit = Some(MathEdit {
            target: MathEditTarget::Block {
                block_id: id,
                align: false,
            },
            source_projection: None,
            source_draft: None,
            revision: session.current_math_edit_revision(),
            cursor: 0,
            branch: MathEditBranch::Tree {
                path: position.path.clone(),
                anchor: position,
            },
        });

        let document_before = session.document().clone();
        let selection_before = session.selection().clone();
        let edit_before = session.math_edit().cloned();
        let revision_before = session.current_math_edit_revision();
        let generation_before = session.document_generation();
        let history_before = session.history_state_id();
        let undo_before = session.can_undo();
        let redo_before = session.can_redo();

        assert!(matches!(
            session.run(Command::Delete(Direction::Backward)),
            Err(EditError::UnsupportedOpaqueContent)
        ));
        assert_eq!(session.document(), &document_before);
        assert_eq!(session.selection(), &selection_before);
        assert_eq!(session.math_edit(), edit_before.as_ref());
        assert_eq!(session.current_math_edit_revision(), revision_before);
        assert_eq!(session.document_generation(), generation_before);
        assert_eq!(session.history_state_id(), history_before);
        assert_eq!(session.can_undo(), undo_before);
        assert_eq!(session.can_redo(), redo_before);
    }
    Ok(())
}

#[test]
fn empty_align_cell_backspace_keeps_other_cells_and_moves_out() -> Result<(), EditError> {
    let Block::Align(align) = display_math_block(true, "x&")? else {
        return Err(EditError::InvalidPosition);
    };
    let id = align.id.clone();
    let row_id = align.rows[0].id.clone();
    let first_cell_id = align.rows[0].cells[0].root.id.clone();
    let cell_id = align.rows[0].cells[1].root.id.clone();
    let mut document = Document::blank();
    document.body.push(Block::Align(align));
    let mut session = Session::new(document)?;
    let position = MathTreePosition {
        host: MathPositionHost::AlignCell {
            row_id: row_id.clone(),
            cell_id: cell_id.clone(),
        },
        path: MathSequencePath::root(),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    session.math_edit = Some(MathEdit {
        target: MathEditTarget::Block {
            block_id: id.clone(),
            align: true,
        },
        source_projection: None,
        source_draft: None,
        revision: session.current_math_edit_revision(),
        cursor: 0,
        branch: MathEditBranch::Tree {
            path: position.path.clone(),
            anchor: position,
        },
    });

    let second_cell_position = session
        .active_host_caret_position()
        .ok_or(EditError::InvalidPosition)?;
    assert!(matches!(
        &second_cell_position,
        super::super::super::HostCaretPosition::Inside {
            local: super::super::super::HostCaretLocalPosition::Math(
                super::super::super::MathCaretLocalPosition {
                    coordinate: super::super::super::MathCaretCoordinate::TreeBoundary {
                        host: MathPositionHost::AlignCell { cell_id: active_cell_id, .. },
                        affinity: MathPositionAffinity::Downstream,
                        ..
                    },
                    ..
                }
            ),
            ..
        } if active_cell_id == &cell_id
    ));
    let mut first_cell_session = Session::new(session.document().clone())?;
    let first_cell_position = MathTreePosition {
        host: MathPositionHost::AlignCell {
            row_id: row_id.clone(),
            cell_id: first_cell_id.clone(),
        },
        path: MathSequencePath::root(),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    first_cell_session.math_edit = Some(MathEdit {
        target: MathEditTarget::Block {
            block_id: id.clone(),
            align: true,
        },
        source_projection: None,
        source_draft: None,
        revision: first_cell_session.current_math_edit_revision(),
        cursor: 0,
        branch: MathEditBranch::Tree {
            path: first_cell_position.path.clone(),
            anchor: first_cell_position,
        },
    });
    assert_ne!(
        second_cell_position,
        first_cell_session
            .active_host_caret_position()
            .ok_or(EditError::InvalidPosition)?
    );

    session.run(Command::Delete(Direction::Backward))?;
    let align = first_align(&session)?;
    assert_eq!(align.rows[0].cells.len(), 2);
    assert!(!align.rows[0].cells[0].root.content.is_empty());
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { anchor, .. })
            if matches!(anchor.host, MathPositionHost::AlignCell { .. })
    ));
    Ok(())
}

#[test]
fn tree_native_mod_backspace_accepts_empty_script_slots_and_restores_them() -> Result<(), EditError>
{
    for (template, expected_source, expected_cursor) in [
        (r"\frac{}{}", r"\frac{}{}", 6),
        (r"\sqrt{}", r"\sqrt{}", 6),
        ("d_{}^{}", "d_{}^{{}}", 3),
    ] {
        let mut session = session_with_text("").map_err(test_error)?;
        session.begin_inline_math()?;
        session.math_insert_template(template)?;
        let before_demotion = session.document().clone();
        let before_math_edit = session.math_edit().cloned();
        assert!(
            session
                .math_edit()
                .is_some_and(|edit| { matches!(edit.branch, MathEditBranch::Tree { .. }) })
        );

        session.run(Command::EditorKey(EditorKey::ModBackspace))?;
        assert!(session.math_edit().is_none());
        assert_eq!(first_paragraph_text(&session), expected_source);
        assert_eq!(session.selection().focus.offset, expected_cursor);

        session.run(Command::Undo)?;
        assert_eq!(session.document(), &before_demotion);
        assert_eq!(session.math_edit(), before_math_edit.as_ref());
        assert!(
            session
                .math_edit()
                .is_some_and(|edit| { matches!(edit.branch, MathEditBranch::Tree { .. }) })
        );
        session.run(Command::Redo)?;
        assert_eq!(first_paragraph_text(&session), expected_source);
        assert_eq!(session.selection().focus.offset, expected_cursor);
    }
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.math_insert_template(r"\sqrt{q_1}d_{}")?;
    let expected_source = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), expected_source);
    Ok(())
}

#[test]
fn tree_native_mod_backspace_materializes_empty_root_math() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    let before = session.document().clone();
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Tree { .. }))
    );

    let effect = session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(effect.document_changed);
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), "");

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &before);
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Tree { .. }))
    );
    session.run(Command::Redo)?;
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn tree_native_mod_backspace_rejects_opaque_placeholder_metadata_atomically()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.math_insert_template(r"\sqrt{}")?;
    let Block::Paragraph(paragraph) = session
        .document
        .body
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition);
    };
    let math = paragraph
        .inlines
        .iter_mut()
        .find_map(|content| match content {
            ParagraphContent::Math(math) => Some(math),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let MathContent::Math(radical) = math
        .math_tree
        .root
        .content
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition);
    };
    let placeholder_flow = radical
        .children
        .first_mut()
        .ok_or(EditError::InvalidPosition)?;
    let Some(MathContent::Math(placeholder)) = placeholder_flow.content.first_mut() else {
        return Err(EditError::InvalidPosition);
    };
    placeholder.extra.insert(
        "vendorPlaceholder".to_owned(),
        serde_json::json!("preserve"),
    );

    let document_before = session.document().clone();
    let selection_before = session.selection().clone();
    let edit_before = session.math_edit().cloned();
    let revision_before = session.current_math_edit_revision();
    let generation_before = session.document_generation();
    let history_before = session.history_state_id();
    let undo_before = session.can_undo();
    let redo_before = session.can_redo();
    assert!(matches!(
        session.run(Command::EditorKey(EditorKey::ModBackspace)),
        Err(EditError::UnsupportedOpaqueContent)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.current_math_edit_revision(), revision_before);
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_before);
    assert_eq!(session.can_undo(), undo_before);
    assert_eq!(session.can_redo(), redo_before);
    Ok(())
}

#[test]
fn tree_native_mod_backspace_maps_subsup_with_empty_sibling_slot() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.math_insert_template(r"sada_{a}^{}a")?;
    let expected_source = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    let edit = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Tree { anchor, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let path = MathSequencePath::new(vec![MathSequenceStep::Operand {
        content_index: 3,
        operand_index: 2,
    }]);
    let position = MathTreePosition {
        host: anchor.host.clone(),
        path: path.clone(),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    edit.cursor = 0;
    edit.branch = MathEditBranch::Tree {
        path,
        anchor: position,
    };
    let before_math_edit = session.math_edit().cloned();
    let sup_cursor = expected_source
        .find("^{}")
        .ok_or(EditError::InvalidPosition)?
        + 2;

    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), expected_source);
    assert_eq!(session.selection().focus.offset, sup_cursor);
    session.run(Command::Undo)?;
    assert_eq!(session.math_edit(), before_math_edit.as_ref());
    session.run(Command::Redo)?;
    assert_eq!(session.selection().focus.offset, sup_cursor);

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
    let expected_source = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    let end_cursor = expected_source.len();
    let before_math_edit = session.math_edit().cloned();

    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), expected_source);
    assert_eq!(session.selection().focus.offset, end_cursor);
    session.run(Command::Undo)?;
    assert_eq!(session.math_edit(), before_math_edit.as_ref());
    session.run(Command::Redo)?;
    assert_eq!(session.selection().focus.offset, end_cursor);
    Ok(())
}

#[test]
fn script_base_start_insertion_lifts_for_script_kinds_and_grouped_base() -> Result<(), EditError> {
    for (source, expected, group_depth) in [
        ("x^a", "vx^{a}", 0),
        ("x_a", "vx_{a}", 0),
        ("x_a^b", "vx_{a}^{b}", 0),
        ("{x}^a", "v{x}^{a}", 1),
        ("{{x}}^a", "v{{x}}^{a}", 2),
    ] {
        let mut session = session_with_text("").map_err(test_error)?;
        session.begin_inline_math()?;
        session.math_insert_template(source)?;
        let edit = session
            .math_edit
            .as_mut()
            .ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree { anchor, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        let mut steps = vec![MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }];
        for _ in 0..group_depth {
            steps.push(MathSequenceStep::Operand {
                content_index: 0,
                operand_index: 0,
            });
        }
        let path = MathSequencePath::new(steps);
        let position = MathTreePosition {
            host: anchor.host.clone(),
            path,
            offset: 0,
            affinity: MathPositionAffinity::Downstream,
        };
        edit.cursor = 0;
        edit.branch = MathEditBranch::Tree {
            path: position.path.clone(),
            anchor: position,
        };

        session.run(Command::InsertText("v".to_owned()))?;
        assert_eq!(
            inline_source(&session).as_deref(),
            Some(expected),
            "{source}"
        );
        assert!(
            session
                .math_tree_input_context()
                .is_some_and(|(_, position)| {
                    position.path.steps().is_empty() && position.offset == 1
                })
        );
        session.run(Command::EditorKey(EditorKey::ModBackspace))?;
        assert!(session.math_edit().is_none());
        assert_eq!(first_paragraph_text(&session), expected, "{source}");
    }
    Ok(())
}

#[test]
fn tree_native_backspace_in_empty_superscript_leaves_sibling_contents_intact()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.math_insert_template(r"sada_{a}^{}a")?;
    let source_before = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    let edit = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Tree { anchor, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let path = MathSequencePath::new(vec![MathSequenceStep::Operand {
        content_index: 3,
        operand_index: 2,
    }]);
    let position = MathTreePosition {
        host: anchor.host.clone(),
        path: path.clone(),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    edit.cursor = 0;
    edit.branch = MathEditBranch::Tree {
        path,
        anchor: position,
    };

    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(source_before.as_str())
    );
    assert!(session.math_edit().is_some_and(|edit| {
        matches!(
            &edit.branch,
            MathEditBranch::Tree { path, anchor }
                if path.steps().is_empty() && anchor.path.steps().is_empty()
                    && anchor.offset == 3 && edit.cursor == 3
        )
    }));
    Ok(())
}

#[test]
fn tree_native_template_replaces_placeholder_in_a_later_operand() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.math_insert_template(r"\frac{}{}")?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::InsertText("y".to_owned()))?;

    assert_eq!(inline_source(&session).as_deref(), Some(r"\frac{x}{y}"));
    Ok(())
}

#[test]
fn tree_native_script_closing_brace_exits_slot_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("x^".to_owned()))?;
    session.run(Command::InsertText("}".to_owned()))?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(
        matches!(edit.branch, MathEditBranch::Tree { ref path, .. } if path.steps().is_empty())
    );
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();
    let history_before = session.history_state_id();
    assert!(matches!(
        session.run(Command::InsertText("}".to_owned())),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_before);
    Ok(())
}

#[test]
fn tree_native_align_separators_create_stable_rows_cells_and_history() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.insert_display_math(true)?;
    session.run(Command::InsertText("a".to_owned()))?;
    let align_id = first_align(&session)?.id.clone();
    let original_row_id = first_align(&session)?.rows[0].id.clone();
    let original_cell_id = first_align(&session)?.rows[0].cells[0].root.id.clone();
    session.run(Command::InsertText("&".to_owned()))?;
    let after_column = first_align(&session)?;
    assert_eq!(after_column.rows[0].id, original_row_id);
    assert_eq!(after_column.rows[0].cells[0].root.id, original_cell_id);
    assert_eq!(
        caret_math::serialize_tree(&after_column.rows[0].cells[0]),
        "a"
    );
    let second_cell_id = after_column.rows[0].cells[1].root.id.clone();
    session.run(Command::InsertText("b".to_owned()))?;
    session.run(Command::EditorKey(EditorKey::Enter))?;
    let after_row = first_align(&session)?;
    assert_eq!(after_row.rows.len(), 2);
    assert_eq!(after_row.rows[0].id, original_row_id);
    let new_row_id = after_row.rows[1].id.clone();
    assert_eq!(after_row.rows[0].cells[1].root.id, second_cell_id);
    let new_cell_id = after_row.rows[1].cells[1].root.id.clone();
    session.run(Command::InsertText("c".to_owned()))?;
    assert_eq!(
        caret_math::serialize_tree(&first_align(&session)?.rows[1].cells[1]),
        "c"
    );

    session.run(Command::Undo)?;
    assert_eq!(first_align(&session)?.rows[1].cells[1].root.id, new_cell_id);
    session.run(Command::Undo)?;
    assert_eq!(first_align(&session)?.rows.len(), 1);
    assert_eq!(
        first_align(&session)?.rows[0].cells[1].root.id,
        second_cell_id
    );
    session.run(Command::Redo)?;
    assert_eq!(first_align(&session)?.rows[1].id, new_row_id);
    session.run(Command::Redo)?;
    assert_eq!(first_align(&session)?.rows[1].cells[1].root.id, new_cell_id);
    assert_eq!(first_align(&session)?.id, align_id);
    Ok(())
}

#[test]
fn tree_native_scripts_and_groups_build_structure_during_typing() -> Result<(), EditError> {
    for (inputs, expected_kind, expected_source) in [
        (
            ["x", "^", "2"],
            caret_doc::MathNodeKind::Superscript,
            "x^{2}",
        ),
        (["y", "_", "1"], caret_doc::MathNodeKind::Subscript, "y_{1}"),
    ] {
        let mut session = session_with_text("").map_err(test_error)?;
        session.begin_inline_math()?;
        for input in inputs {
            session.run(Command::InsertText(input.to_owned()))?;
        }
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert!(edit.source_projection.is_none());
        let MathEditBranch::Tree { path, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        assert!(path.steps().iter().any(|step| matches!(
            step,
            MathSequenceStep::Operand {
                operand_index: 1,
                ..
            }
        )));
        assert_eq!(inline_source(&session).as_deref(), Some(expected_source));
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        let tree = session
            .document()
            .body
            .iter()
            .find_map(|block| match block {
                Block::Paragraph(paragraph) if &paragraph.id == paragraph_id => {
                    paragraph.inlines.iter().find_map(|content| match content {
                        ParagraphContent::Math(math) if &math.id == math_id => {
                            Some(&math.math_tree)
                        }
                        _ => None,
                    })
                }
                _ => None,
            })
            .ok_or(EditError::InvalidPosition)?;
        assert!(tree.root.content.iter().any(
            |content| matches!(content, MathContent::Math(node) if node.kind == expected_kind)
        ));
    }

    let mut grouped = session_with_text("").map_err(test_error)?;
    grouped.begin_inline_math()?;
    grouped.run(Command::InsertText("{".to_owned()))?;
    grouped.run(Command::InsertText("ab".to_owned()))?;
    grouped.run(Command::InsertText("}".to_owned()))?;
    assert_eq!(inline_source(&grouped).as_deref(), Some("{ab}"));
    assert!(grouped.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    Ok(())
}

#[test]
fn invalid_tree_native_host_or_path_fails_without_changing_editor_state() -> Result<(), EditError> {
    for invalid_host in [true, false] {
        let mut session = session_with_text("").map_err(test_error)?;
        session.begin_inline_math()?;
        session.run(Command::InsertText("x".to_owned()))?;
        assert!(session.math_edit().is_some_and(|edit| {
            edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
        }));
        let edit = session
            .math_edit
            .as_mut()
            .ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree { anchor, path } = &mut edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        if invalid_host {
            anchor.host = MathPositionHost::InlineMath {
                owner_id: NodeId::generate(),
            };
        } else {
            let invalid_path = MathSequencePath::new(vec![MathSequenceStep::Operand {
                content_index: usize::MAX,
                operand_index: 0,
            }]);
            anchor.path = invalid_path.clone();
            *path = invalid_path;
        }
        let document_before = session.document().clone();
        let edit_before = session.math_edit().cloned();
        let generation_before = session.document_generation();
        let history_before = session.history_state_id();

        assert_eq!(
            session.run(Command::MoveCaret(Direction::Backward)),
            Err(EditError::InvalidPosition)
        );
        assert_eq!(session.document(), &document_before);
        assert_eq!(session.math_edit(), edit_before.as_ref());
        assert_eq!(session.document_generation(), generation_before);
        assert_eq!(session.history_state_id(), history_before);
    }
    Ok(())
}

#[test]
fn text_edit_preserves_nested_text_structure() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{\textbf{abc}}")?;
    move_into_math_text(&mut session)?;
    match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    {
        MathEditBranch::Text { path, .. } => {
            let tree = session.math_tree_for_edit(
                session.math_edit().ok_or(EditError::InvalidPosition)?,
                path.align_cell(),
            )?;
            let Some(MathContent::Environment(_environment)) = tree.root.content.first() else {
                return Err(EditError::InvalidPosition);
            };
        }
        MathEditBranch::Source => return Err(EditError::InvalidPosition),
        MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition),
    }

    session.run(Command::InsertText("Xé".to_owned()))?;

    let MathEditBranch::Text { path, .. } = &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    else {
        return Err(EditError::InvalidPosition);
    };
    let tree = session.math_tree_for_edit(
        session.math_edit().ok_or(EditError::InvalidPosition)?,
        path.align_cell(),
    )?;
    let Some(MathContent::Environment(_environment)) = tree.root.content.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(
        path.resolve(tree).map(|text| text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()),
        Some("abcXé".to_owned())
    );
    Ok(())
}

#[test]
fn math_text_formatting_rejects_unsafe_model_metadata_splits_atomically() -> Result<(), EditError> {
    for metadata_kind in ["flow", "run"] {
        let mut session = session_with_text("").map_err(test_error)?;
        insert_equation_text(&mut session, r"\text{abc}")?;
        move_into_math_text(&mut session)?;
        mutate_active_math_text_flow(&mut session, |text| {
            match metadata_kind {
                "flow" => {
                    text.extra.insert(
                        "futureFlowValue".to_owned(),
                        serde_json::json!(metadata_kind),
                    );
                }
                "run" => {
                    text.runs[0].extra.insert(
                        "futureRunValue".to_owned(),
                        serde_json::json!(metadata_kind),
                    );
                }
                _ => unreachable!(),
            }
            Ok(())
        })?;
        session.run(Command::SetMathTextSelection {
            anchor: 1,
            focus: 2,
        })?;
        session.run(Command::BeginCommandLine)?;
        session.run(Command::TypeCommandLine("math".to_owned()))?;
        let candidate = session
            .command_line_candidates()
            .into_iter()
            .find(|candidate| candidate.alias == "\\math")
            .ok_or(EditError::InvalidPosition)?;
        assert!(!candidate.available);
        let document_before = session.document().clone();
        let edit_before = session.math_edit().cloned();
        let generation_before = session.document_generation();
        let selection_before = session.selection().clone();

        session.run(Command::AcceptCommandLine)?;
        assert!(session.command_line().is_some());
        assert_eq!(session.document(), &document_before);
        assert_eq!(session.math_edit(), edit_before.as_ref());
        assert_eq!(session.document_generation(), generation_before);
        assert_eq!(session.selection(), &selection_before);
        session.run(Command::CancelCommandLine)?;

        assert!(matches!(
            session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold)),
            Err(EditError::UnsupportedOpaqueContent)
        ));
        assert_eq!(session.document(), &document_before);
        assert_eq!(session.math_edit(), edit_before.as_ref());
        assert_eq!(session.document_generation(), generation_before);
        assert_eq!(session.selection(), &selection_before);
    }

    let mut full_selection = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut full_selection, r"\text{abc}")?;
    move_into_math_text(&mut full_selection)?;
    mutate_active_math_text_flow(&mut full_selection, |text| {
        text.extra
            .insert("futureFlowValue".to_owned(), serde_json::json!(1));
        text.runs[0]
            .extra
            .insert("futureRunValue".to_owned(), serde_json::json!(2));
        Ok(())
    })?;
    full_selection.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 3,
    })?;
    full_selection.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    let selected = active_math_text_flow(&full_selection)?;
    assert_eq!(selected.extra["futureFlowValue"], 1);
    assert_eq!(selected.runs[0].extra["futureRunValue"], 2);
    Ok(())
}

#[test]
fn inline_math_nested_text_split_preserves_inserted_node_ranges_and_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{\text{abc}}{z}".to_owned(),
    ))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    for _ in 0..24 {
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
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;

    session.run(Command::InsertInlineMath(r"\frac{p}{q}".to_owned()))?;
    let source = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    assert!(
        source.contains(r"\text{a}\frac{p}{q}\text{bc}"),
        "source: {source}"
    );
    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(ParagraphContent::Math(inline_math)) = paragraph
        .inlines
        .iter()
        .find(|content| matches!(content, ParagraphContent::Math(_)))
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(MathContent::Math(outer_fraction)) = inline_math.math_tree.root.content.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(numerator) = outer_fraction.children.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert!(matches!(
        numerator.content.as_slice(),
        [
            MathContent::Text(left),
            MathContent::Math(inner),
            MathContent::Text(right),
        ] if text_flow_display(left) == "a"
            && inner.kind == MathNodeKind::Fraction
            && text_flow_display(right) == "bc"
    ));

    session.run(Command::Undo)?;
    assert!(
        inline_source(&session)
            .as_deref()
            .is_some_and(|source| source.contains(r"\text{abc}"))
    );
    session.run(Command::Redo)?;
    assert!(
        inline_source(&session)
            .as_deref()
            .is_some_and(|source| source.contains(r"\text{a}\frac{p}{q}\text{bc}"))
    );
    Ok(())
}

#[test]
fn align_nonfirst_cell_math_text_split_keeps_cell_mapping_through_history()
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
        focus: 2,
    })?;
    session.run(Command::InsertInlineMath(r"\frac{p}{q}".to_owned()))?;
    let Some(Block::Align(align)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { path, anchor })
            if path.steps().is_empty() && matches!(anchor.host, MathPositionHost::AlignCell { .. })
    ));
    assert!(matches!(
        align.rows[0].cells[1].root.content.as_slice(),
        [
            MathContent::Text(left),
            MathContent::Math(fraction),
            MathContent::Text(right),
        ] if text_flow_display(left) == "a"
            && fraction.kind == MathNodeKind::Fraction
            && text_flow_display(right) == "c"
    ));

    session.run(Command::Undo)?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\begin{aligned}α & \text{abc}\end{aligned}")
    );
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 1))
    ));
    session.run(Command::Redo)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { anchor, .. })
            if matches!(anchor.host, MathPositionHost::AlignCell { .. })
    ));
    session.run(Command::Undo)?;
    session.run(Command::InsertText("z".to_owned()))?;
    assert!(inline_source(&session).is_some_and(|source| source.contains(r"\text{azc}")));
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\begin{aligned}α & \text{ac}\end{aligned}")
    );
    Ok(())
}

#[test]
fn math_text_entry_rejects_opaque_split_metadata_atomically() -> Result<(), EditError> {
    for metadata_kind in ["flow", "run"] {
        let mut session = session_with_text("").map_err(test_error)?;
        insert_equation_text(&mut session, r"\text{abc}")?;
        move_into_math_text(&mut session)?;
        mutate_active_math_text_flow(&mut session, |text| {
            match metadata_kind {
                "flow" => {
                    text.extra
                        .insert("futureFlowValue".to_owned(), serde_json::json!(1));
                }
                "run" => {
                    text.runs[0]
                        .extra
                        .insert("futureRunValue".to_owned(), serde_json::json!(1));
                }
                _ => unreachable!(),
            }
            Ok(())
        })?;
        session.run(Command::SetMathTextSelection {
            anchor: 1,
            focus: 2,
        })?;
        let document_before = session.document().clone();
        let edit_before = session.math_edit().cloned();
        let generation_before = session.document_generation();
        let selection_before = session.selection().clone();

        assert!(matches!(
            session.run(Command::InsertInlineMath("x".to_owned())),
            Err(EditError::UnsupportedOpaqueContent)
        ));
        assert_eq!(session.document(), &document_before);
        assert_eq!(session.math_edit(), edit_before.as_ref());
        assert_eq!(session.document_generation(), generation_before);
        assert_eq!(session.selection(), &selection_before);
    }
    Ok(())
}

#[test]
fn parsed_equation_text_reenters_through_tree_native_editing() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());

    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Text { .. })
    }));
    session.run(Command::InsertText("d".to_owned()))?;
    assert_eq!(active_math_text_flow(&session)?.runs[0].text, "abcd");
    Ok(())
}

#[test]
fn pending_math_text_format_applies_to_inserted_text_and_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{a}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))?;
    session.run(Command::ToggleRunUnderline)?;
    session.run(Command::InsertText("b".to_owned()))?;

    assert!(
        inline_source(&session)
            .is_some_and(|source| { source.contains(r"\text{a\textbf{\textit{b}}}") })
    );
    assert!(math_tree_has_environment(
        session.document(),
        InlineEnvironmentKind::Bold
    ));
    assert!(math_tree_has_environment(
        session.document(),
        InlineEnvironmentKind::Italic
    ));
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "b" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{a}"));
    assert!(!math_tree_has_environment(
        session.document(),
        InlineEnvironmentKind::Bold
    ));
    session.run(Command::Redo)?;
    assert!(
        inline_source(&session)
            .is_some_and(|source| { source.contains(r"\text{a\textbf{\textit{b}}}") })
    );
    assert!(math_tree_has_environment(
        session.document(),
        InlineEnvironmentKind::Bold
    ));
    assert!(math_tree_has_environment(
        session.document(),
        InlineEnvironmentKind::Italic
    ));

    let mut combining = session_with_text("")?;
    insert_equation_text(&mut combining, r"\text{a}")?;
    move_into_math_text(&mut combining)?;
    combining.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    combining.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    combining.run(Command::ToggleRunUnderline)?;
    combining.run(Command::InsertText("\u{301}".to_owned()))?;
    assert!(
        inline_source(&combining)
            .is_some_and(|source| { source.contains("\\text{\\textbf{a\u{301}}}") })
    );
    assert!(math_tree_has_environment(
        combining.document(),
        InlineEnvironmentKind::Bold
    ));
    assert!(active_math_text_flow(&combining)?.runs.iter().any(|run| {
        run.text == "a\u{301}" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));

    let mut disable = session_with_text("")?;
    insert_equation_text(&mut disable, r"\text{\textbf{a}}")?;
    move_into_math_text(&mut disable)?;
    disable.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    disable.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    disable.run(Command::InsertText("b".to_owned()))?;
    assert_eq!(
        active_math_text_bold_chars(&disable)?,
        vec![('a', true), ('b', false)]
    );
    assert!(math_tree_has_environment(
        disable.document(),
        InlineEnvironmentKind::Bold
    ));

    let mut align = session_with_text("")?;
    align.insert_display_math(true)?;
    enter_source_edit(&mut align, Direction::Forward)?;
    align.run(Command::InsertText(r"\text{a}".to_owned()))?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        align.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 0))
    ));
    align.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    align.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))?;
    align.run(Command::InsertText("b".to_owned()))?;
    assert!(inline_source(&align).is_some_and(|source| { source.contains(r"\text{a\textit{b}}") }));
    assert!(math_tree_has_environment(
        align.document(),
        InlineEnvironmentKind::Italic
    ));
    align.run(Command::Undo)?;
    align.run(Command::Redo)?;
    assert!(inline_source(&align).is_some_and(|source| { source.contains(r"\text{a\textit{b}}") }));
    Ok(())
}

#[test]
fn theme_template_variant_controls_math_text_toggle_and_inserted_text()
-> Result<(), Box<dyn std::error::Error>> {
    let math_tree = MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: MathFlow {
            id: NodeId::generate(),
            content: vec![MathContent::Math(MathNode {
                id: NodeId::generate(),
                kind: MathNodeKind::Fraction,
                text: None,
                attributes: BTreeMap::from([("style".to_owned(), "frac".to_owned())]),
                children: vec![
                    MathFlow {
                        id: NodeId::generate(),
                        content: vec![MathContent::Environment(caret_doc::InlineEnvironment {
                            id: NodeId::generate(),
                            kind: InlineEnvironmentKind::Bold,
                            definition_id: Some("math-bold/quiet".to_owned()),
                            props: BTreeMap::new(),
                            children: vec![MathContent::Text(TextFlow {
                                runs: vec![Run {
                                    text: "abc".to_owned(),
                                    language: None,
                                    props: BTreeMap::new(),
                                    extra: BTreeMap::new(),
                                }],
                                extra: BTreeMap::new(),
                            })],
                            extra: BTreeMap::new(),
                        })],
                        extra: BTreeMap::new(),
                    },
                    MathFlow {
                        id: NodeId::generate(),
                        content: vec![MathContent::Math(MathNode {
                            id: NodeId::generate(),
                            kind: MathNodeKind::Symbol,
                            text: Some("1".to_owned()),
                            attributes: BTreeMap::new(),
                            children: Vec::new(),
                            extra: BTreeMap::new(),
                        })],
                        extra: BTreeMap::new(),
                    },
                ],
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    let mut paragraph = Paragraph::empty(NodeId::generate());
    let paragraph_id = paragraph.id.clone();
    paragraph.inlines = vec![ParagraphContent::Math(InlineMath {
        id: NodeId::generate(),
        math_tree,
        extra: BTreeMap::new(),
    })];
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    document.global_settings.theme = Some(caret_doc::ThemeSnapshot {
        format_version: 1,
        source_id: None,
        named_styles: BTreeMap::new(),
        environment_templates: BTreeMap::from([(
            "math-bold".to_owned(),
            serde_json::json!({
                "baseKind": "bold",
                "props": {"enabled": true},
                "variants": {"math-bold/quiet": {"enabled": false}}
            }),
        )]),
        default_inline_environment_templates: BTreeMap::from([(
            "bold".to_owned(),
            serde_json::json!("math-bold"),
        )]),
        extra: BTreeMap::new(),
    });
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("valid math template session should be accepted: {error}"));
    let math_end = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: '\u{fffc}'.len_utf8(),
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: math_end.clone(),
        focus: math_end,
    })))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;

    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    assert_eq!(
        session
            .pending_inline_style
            .get(InlineEnvironmentKind::Bold),
        Some(true)
    );
    session.run(Command::InsertText("x".to_owned()))?;

    fn has_bold_environment_with_x(nodes: &[MathContent]) -> bool {
        nodes.iter().any(|content| match content {
            MathContent::Environment(environment) => {
                let is_enabled_bold = environment.kind == InlineEnvironmentKind::Bold
                    && environment.props.get("enabled") == Some(&serde_json::json!(true));
                (is_enabled_bold
                    && environment.children.iter().any(|child| match child {
                        MathContent::Text(flow) => flow.runs.iter().any(|run| run.text == "x"),
                        _ => false,
                    }))
                    || has_bold_environment_with_x(&environment.children)
            }
            MathContent::Math(node) => node
                .children
                .iter()
                .any(|flow| has_bold_environment_with_x(&flow.content)),
            MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
        })
    }
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(has_bold_environment_with_x(
        &session.math_tree_for_edit(edit, None)?.root.content
    ));
    assert!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .any(|run| run.text == "x")
    );
    Ok(())
}

#[test]
fn math_text_pending_bold_and_font_keep_affinity_through_undo_redo()
-> Result<(), Box<dyn std::error::Error>> {
    for font in [false, true] {
        let mut session = session_with_text("")?;
        insert_equation_text(&mut session, r"\text{abc}")?;
        move_into_math_text(&mut session)?;
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text { path, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition.into());
        };
        let position = MathTextPosition::new(path.clone(), 1, MathPositionAffinity::Upstream);
        session.set_math_text_selection_positions_with_visual_affinities(
            position.clone(),
            position,
            crate::VisualAffinity::Upstream,
            crate::VisualAffinity::Upstream,
        )?;
        let before = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        if font {
            session.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
        } else {
            session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
        }
        assert_eq!(session.math_edit(), Some(&before));
        session.run(Command::Undo)?;
        assert_eq!(session.math_edit(), Some(&before));
        session.run(Command::Redo)?;
        assert_eq!(session.math_edit(), Some(&before));
    }
    Ok(())
}

#[test]
fn tab_does_not_exit_math_edit_or_insert_spaces_in_the_host_paragraph() -> Result<(), EditError> {
    let mut session = session_with_text("beforeafter").map_err(test_error)?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.begin_inline_math()?;
    let document_before_tab = session.document().clone();
    let math_edit_before_tab = session.math_edit().cloned();

    session.run(Command::EditorKey(EditorKey::Tab))?;

    assert_eq!(session.document(), &document_before_tab);
    assert_eq!(session.math_edit(), math_edit_before_tab.as_ref());
    Ok(())
}

#[test]
fn preserving_math_metadata_keeps_tree_and_unchanged_node_identity() {
    let mut previous = parsed_inline_tree("a+b");
    previous
        .extra
        .insert("futureTreeValue".to_owned(), serde_json::json!(1));
    let unchanged_id = math_node(&previous.root, 0).id.clone();
    math_node_mut(&mut previous.root, 0)
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("kept"));

    let mut updated = parsed_inline_tree("a+bc");
    preserve_math_tree_metadata(&previous, &mut updated);

    assert_eq!(updated.extra["futureTreeValue"], serde_json::json!(1));
    assert_eq!(math_node(&updated.root, 0).id, unchanged_id);
    assert_eq!(
        math_node(&updated.root, 0).extra["futureNodeValue"],
        serde_json::json!("kept")
    );
}

#[test]
fn repeated_structure_insertions_keep_metadata_on_the_original_node() {
    let mut previous = parsed_inline_tree("a+a");
    math_node_mut(&mut previous.root, 0)
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("original"));
    let original_id = math_node(&previous.root, 0).id.clone();
    let mut updated = parsed_inline_tree("a+a+a");

    assert!(preserve_math_tree_metadata_for_test(
        &previous,
        &mut updated,
        &(0..0),
        &(0..2),
    ));
    assert!(math_node(&updated.root, 0).extra.is_empty());
    assert_eq!(math_node(&updated.root, 2).id, original_id);
    assert_eq!(
        math_node(&updated.root, 2).extra["futureNodeValue"],
        "original"
    );
}

#[test]
fn splitting_a_metadata_bearing_align_row_is_rejected() -> Result<(), EditError> {
    let Block::Align(mut previous) = display_math_block(true, "a+b&b")? else {
        panic!("expected Align block");
    };
    let old_row_id = previous.rows[0].id.clone();
    previous.rows[0].extra.insert(
        "futureRowValue".to_owned(),
        serde_json::json!("preserve once"),
    );
    let first_cell_id = previous.rows[0].cells[0].root.id.clone();
    let second_cell_id = previous.rows[0].cells[1].root.id.clone();
    previous.rows[0].cells[0]
        .extra
        .insert("futureCellValue".to_owned(), serde_json::json!("left"));
    previous.rows[0].cells[1]
        .extra
        .insert("futureCellValue".to_owned(), serde_json::json!("right"));

    let Block::Align(mut updated) =
        display_math_block_with_id(true, r"a+b \\ b", previous.id.clone())?
    else {
        panic!("expected parsed Align block");
    };
    let old_offset = aligned_body_offset(&previous);
    let new_offset = aligned_body_offset(&updated);
    let metadata_preserved = preserve_align_metadata_for_test(
        &previous,
        &mut updated,
        &(old_offset + 3..old_offset + 4),
        &(new_offset + 3..new_offset + 7),
    );

    assert!(!metadata_preserved);
    assert_eq!(previous.rows[0].id, old_row_id);
    assert_eq!(previous.rows[0].cells[0].root.id, first_cell_id);
    assert_eq!(previous.rows[0].cells[1].root.id, second_cell_id);
    Ok(())
}

#[test]
fn editing_align_cell_preserves_tree_and_node_metadata() -> Result<(), EditError> {
    let Block::Align(mut previous) = display_math_block(true, "a&b+c")? else {
        panic!("expected Align block");
    };
    let row_id = previous.rows[0].id.clone();
    let first_cell_id = previous.rows[0].cells[0].root.id.clone();
    let edited_cell_id = previous.rows[0].cells[1].root.id.clone();
    previous.rows[0].cells[0]
        .extra
        .insert("futureTreeValue".to_owned(), serde_json::json!("unchanged"));
    previous.rows[0].cells[0]
        .root
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("unchanged"));
    previous.rows[0].cells[1]
        .root
        .extra
        .insert("editedCellNodeValue".to_owned(), serde_json::json!("kept"));
    let Block::Align(mut updated) = display_math_block_with_id(true, "a&b+x", previous.id.clone())?
    else {
        panic!("expected parsed Align block");
    };
    let old_offset = aligned_body_offset(&previous);
    let new_offset = aligned_body_offset(&updated);
    preserve_align_metadata_for_test(
        &previous,
        &mut updated,
        &(old_offset + 4..old_offset + 5),
        &(new_offset + 4..new_offset + 5),
    );

    assert_eq!(updated.rows[0].id, row_id);
    assert_eq!(
        updated.rows[0].cells[0].extra["futureTreeValue"],
        "unchanged"
    );
    assert_eq!(updated.rows[0].cells[0].root.id, first_cell_id);
    assert_eq!(
        updated.rows[0].cells[0].root.extra["futureNodeValue"],
        "unchanged"
    );
    assert_eq!(updated.rows[0].cells[1].root.id, edited_cell_id);
    assert_eq!(
        updated.rows[0].cells[1].root.extra["editedCellNodeValue"],
        "kept"
    );
    Ok(())
}

#[test]
fn inserting_duplicate_align_cell_before_existing_cells_keeps_their_identity()
-> Result<(), EditError> {
    let Block::Align(previous) = display_math_block(true, "a&a")? else {
        panic!("expected Align block");
    };
    let first_id = previous.rows[0].cells[0].root.id.clone();
    let last_id = previous.rows[0].cells[1].root.id.clone();
    let Block::Align(mut updated) = display_math_block_with_id(true, "a&a&a", previous.id.clone())?
    else {
        panic!("expected parsed Align block");
    };
    let old_offset = aligned_body_offset(&previous);
    let new_offset = aligned_body_offset(&updated);
    let inserted_len = aligned_body_len(&updated) - aligned_body_len(&previous);
    preserve_align_metadata_for_test(
        &previous,
        &mut updated,
        &(old_offset..old_offset),
        &(new_offset..new_offset + inserted_len),
    );

    assert_eq!(updated.rows[0].cells[1].root.id, first_id);
    assert_eq!(updated.rows[0].cells[2].root.id, last_id);
    Ok(())
}

#[test]
fn preserving_math_metadata_keeps_the_parent_of_an_edited_subtree() {
    let old_source = "\\frac{ab}{c}";
    let new_source = "\\frac{axb}{c}";
    let mut previous = parsed_inline_tree(old_source);
    let (prefix_end, previous_tail, _) = unchanged_source_boundaries(old_source, new_source);
    let projection = caret_math::parse_projection(old_source, caret_math::MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("old source should project: {error}"));
    let operands = projection
        .source_map()
        .tree()
        .and_then(|map| map.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(caret_math::MathContentSourceMap::operands)
        .unwrap_or_else(|| panic!("fraction should expose operand maps"));
    let Some(changed_index) = operands.iter().position(|flow| {
        flow.range()
            .is_some_and(|range| range.start <= prefix_end && range.end >= previous_tail)
    }) else {
        panic!("fraction operand should contain the edit");
    };
    let changed_parent = &mut math_node_mut(&mut previous.root, 0).children[changed_index];
    let changed_parent_id = changed_parent.id.clone();
    changed_parent
        .extra
        .insert("futureOperandValue".to_owned(), serde_json::json!(true));

    let mut updated = parsed_inline_tree(new_source);
    preserve_math_tree_metadata(&previous, &mut updated);

    let preserved = math_node(&updated.root, 0)
        .children
        .iter()
        .find(|flow| flow.extra.contains_key("futureOperandValue"));
    assert!(preserved.is_some_and(|node| {
        node.id == changed_parent_id && node.extra["futureOperandValue"] == true
    }));
}

#[test]
fn stale_math_edit_can_be_abandoned_with_escape_without_changing_the_tree()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let document_before = session.document().clone();
    let selection_before = session.selection().clone();
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision
        .document_generation += 1;

    let effect = session.run(Command::EditorKey(EditorKey::Escape))?;

    assert!(effect.selection_changed);
    assert!(!effect.document_changed);
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn restore_math_edit_rejects_mismatched_tree_and_align_locations_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    for (query, source, kind) in [
        ("math", "x+y", caret_math::MathInputKind::Inline),
        (
            "align",
            r"\begin{aligned}x+y\end{aligned}",
            caret_math::MathInputKind::Align,
        ),
    ] {
        let mut session = session_with_text("")?;
        accept(&mut session, query)?;
        enter_source_edit(&mut session, Direction::Forward)?;
        session.run(Command::InsertText("x+y".to_owned()))?;
        let edit = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        assert_eq!(source_projection(&edit).source(), source);
        let mut position = history_position(&edit);
        let document_before = session.document.clone();
        let revision = session.current_math_edit_revision();
        assert!(restore_math_edit(&session.document, position.clone(), revision.clone()).is_some());
        assert_eq!(session.document, document_before);
        let mismatched_source = source.replace("x+y", "x+z");
        let projection = caret_math::parse_projection(&mismatched_source, kind)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
            .into_parts()
            .1;

        position.source_projection = Some(Arc::new(projection));
        position.projection_identity =
            Some(Arc::as_ptr(source_position_projection(&position)) as usize);
        assert!(restore_math_edit(&session.document, position, revision).is_none());
        assert_eq!(session.document, document_before);
        assert_eq!(session.math_edit(), Some(&edit));
    }
    Ok(())
}

#[test]
fn history_restores_source_free_textflow_path_for_tree_editing() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 2,
    })?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| edit.source_projection.is_none())
    );

    session.run(Command::InsertText("X".to_owned()))?;
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "aXc"
    );
    session.run(Command::Undo)?;
    let restored = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(restored.source_projection.is_none());
    assert!(matches!(restored.branch, MathEditBranch::Text { .. }));
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abc"
    );

    session.run(Command::Redo)?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| edit.source_projection.is_none())
    );
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "aXc"
    );
    Ok(())
}

#[test]
fn reentering_formula_rebases_text_path_for_pending_style_undo()
-> Result<(), Box<dyn std::error::Error>> {
    let source = r"\frac {x}{\text{t}}";
    let canonical_tree =
        match caret_math::parse_projection(source, caret_math::MathInputKind::Inline)?
            .into_parts()
            .0
        {
            caret_math::ProjectedMath::Tree(tree) => tree,
            caret_math::ProjectedMath::Align(_) => return Err(EditError::InvalidPosition.into()),
        };
    let canonical_source = caret_math::serialize_tree(&canonical_tree);
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(source.to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    assert_eq!(
        session.math_edit().and_then(MathEdit::source),
        Some(canonical_source.as_str())
    );
    session.run(Command::InsertText(" ".to_owned()))?;
    let historical_source = session
        .math_edit()
        .and_then(MathEdit::source)
        .ok_or(EditError::InvalidPosition)?
        .to_owned();
    assert_ne!(historical_source, canonical_source);

    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let start = first_text_source_range(
        &tree.root,
        source_projection(&edit)
            .tree()
            .ok_or(EditError::InvalidPosition)?
            .root(),
    )
    .ok_or(EditError::InvalidPosition)?
    .start;
    let source_base = source_projection(&edit).edit_range().start;
    let path = path::path_at_source_boundary(
        source_projection(&edit)
            .tree()
            .ok_or(EditError::InvalidPosition)?,
        source_base + start,
        true,
        None,
    )
    .ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 0);
    session.run(Command::ToggleRunUnderline)?;

    session.exit_math_edit(Direction::Forward, true)?;
    enter_source_edit(&mut session, Direction::Backward)?;
    assert_eq!(
        session.math_edit().and_then(MathEdit::source),
        Some(canonical_source.as_str())
    );
    session.run(Command::Undo)?;
    let restored = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(matches!(restored.branch, MathEditBranch::Text { .. }));
    assert!(restored.source_projection.is_none());
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "t"
    );
    assert!(
        session
            .pending_inline_style
            .active_run_properties()
            .next()
            .is_none()
    );
    session.run(Command::InsertText("z".to_owned()))?;
    assert!(inline_source(&session).is_some_and(|source| source.contains('z')));
    session.run(Command::Delete(Direction::Backward))?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| edit.source_projection.is_none())
    );
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "t"
    );
    Ok(())
}

#[test]
fn undo_rejects_a_stale_math_source_map_and_restores_the_history_transaction()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x+y".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let stale_source = source_projection(&edit).source().replace("x+y", "x+z");
    let source_projection =
        caret_math::parse_projection(&stale_source, caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
            .into_parts()
            .1;
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .source_projection = Some(Arc::new(source_projection));

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
    let document_before_failed_undo = session.document.clone();
    let selection_before_failed_undo = session.selection.clone();
    let edit_before_failed_undo = session.math_edit().cloned();
    let generation_before_failed_undo = session.document_generation;
    let history_state_before_failed_undo = session.history_state_id;
    assert!(session.can_undo());

    assert!(matches!(
        session.run(Command::Undo),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document, document_before_failed_undo);
    assert_eq!(session.selection, selection_before_failed_undo);
    assert_eq!(session.math_edit(), edit_before_failed_undo.as_ref());
    assert_eq!(session.document_generation, generation_before_failed_undo);
    assert_eq!(session.history_state_id, history_state_before_failed_undo);
    assert!(session.can_undo());
    assert!(!session.can_redo());
    Ok(())
}

#[test]
fn redo_rejects_a_missing_math_edit_target_without_consuming_history()
-> Result<(), Box<dyn std::error::Error>> {
    let geometry = caret_doc::PageGeometry {
        paper_size: caret_doc::PaperSize::A3,
        orientation: caret_doc::Orientation::Landscape,
        margins: caret_doc::PageMargins {
            top: 36.0,
            right: 54.0,
            bottom: 72.0,
            left: 90.0,
        },
    };
    let mut valid_session = session_with_text("")?;
    valid_session.run(Command::InsertInlineMath("x+y".to_owned()))?;
    valid_session.run(Command::EnterMath(Direction::Backward))?;
    valid_session.run(Command::SetPageGeometry(geometry))?;
    valid_session.run(Command::Undo)?;
    valid_session.run(Command::Redo)?;
    assert!(valid_session.can_undo());
    assert!(!valid_session.can_redo());

    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x+y".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::SetPageGeometry(geometry))?;
    session.run(Command::Undo)?;
    assert!(session.can_redo());

    let MathEditTarget::Inline { math_id, .. } = session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .target
        .clone()
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let Block::Paragraph(paragraph) = session
        .document
        .body
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    // 故障注入已失效的公式身份，检查重放失败不会消费 redo 事务。
    find_inline_math_mut(&mut paragraph.inlines, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .id = NodeId::generate();

    let document_before_failed_redo = session.document.clone();
    let selection_before_failed_redo = session.selection.clone();
    let edit_before_failed_redo = session.math_edit().cloned();
    let revision_before_failed_redo = edit_before_failed_redo
        .as_ref()
        .map(|edit| edit.revision.clone());
    let generation_before_failed_redo = session.document_generation;
    let history_state_before_failed_redo = session.history_state_id;

    assert!(matches!(
        session.run(Command::Redo),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document, document_before_failed_redo);
    assert_eq!(session.selection, selection_before_failed_redo);
    assert_eq!(session.math_edit(), edit_before_failed_redo.as_ref());
    assert_eq!(
        session.math_edit().map(|edit| edit.revision.clone()),
        revision_before_failed_redo
    );
    assert_eq!(session.document_generation, generation_before_failed_redo);
    assert_eq!(session.history_state_id, history_state_before_failed_redo);
    assert!(session.can_undo());
    assert!(session.can_redo());
    Ok(())
}

#[test]
fn reopening_a_canonical_projection_does_not_break_formula_creation_undo()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("a + b".to_owned()))?;
    assert!(session.math_edit().is_none());
    enter_source_edit(&mut session, Direction::Backward)?;
    assert_eq!(session.math_edit().map(source_text), Some("a+b"));

    session.run(Command::Undo)?;
    assert!(session.math_edit().is_none());
    assert!(session.document().body.iter().all(|block| {
        !matches!(block, Block::Paragraph(paragraph) if paragraph.inlines.iter().any(|node| matches!(node, ParagraphContent::Math(_))))
    }));
    session.run(Command::Redo)?;
    assert!(session.document().body.iter().any(|block| {
        matches!(block, Block::Paragraph(paragraph) if paragraph.inlines.iter().any(|node| matches!(node, ParagraphContent::Math(_))))
    }));
    Ok(())
}

#[test]
fn tree_native_text_edit_keeps_empty_textflow_without_source_anchors() -> Result<(), EditError> {
    for source in [r"\text{a}\text{}", r"\text{a}\text{}\text{b}"] {
        let (content, _) = caret_math::parse_projection(source, caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
            .into_parts();
        let caret_math::ProjectedMath::Tree(tree) = content else {
            return Err(EditError::InvalidPosition);
        };
        let empty_index = tree
            .root
            .content
            .iter()
            .position(|content| {
                matches!(content, MathContent::Text(text) if text.runs.iter().all(|run| run.text.is_empty()))
            })
            .ok_or(EditError::InvalidPosition)?;

        let (source_map, refreshed_tree) =
            crate::math_validation::project_tree_for_edit(&tree, caret_math::MathInputKind::Inline)
                .map_err(test_error)?;
        let Some(MathContent::Text(empty)) = refreshed_tree.root.content.get(empty_index) else {
            return Err(EditError::InvalidPosition);
        };
        let text_map = source_map
            .tree()
            .and_then(|map| map.content_at(&[caret_doc::MathPathStep::Content(empty_index)]))
            .and_then(caret_math::MathContentSourceMap::text)
            .ok_or(EditError::InvalidPosition)?;
        let source_range = text_map.range().ok_or(EditError::InvalidPosition)?;
        assert_eq!(source_range.start, source_range.end);
        assert!(
            text_map
                .segments()
                .iter()
                .any(|segment| segment.display().is_empty()
                    && segment.source() == Some(source_range.clone()))
        );
        assert_eq!(
            text_source_boundary_from_map(empty, text_map, 0, 0, Affinity::Downstream,),
            Some(source_range.start)
        );

        let mut document = Document::blank();
        document.body = vec![
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
            Block::Equation(Equation {
                id: NodeId::generate(),
                math_tree: refreshed_tree,
                extra: BTreeMap::new(),
            }),
        ];
        let mut session = Session::new(document)?;
        session.run(Command::EnterMath(Direction::Forward))?;
        let path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(empty_index)], None);
        session.enter_math_text_branch(path, 0);
        session.run(Command::SetMathTextSelection {
            anchor: 0,
            focus: 0,
        })?;
        session.run(Command::InsertText("x".to_owned()))?;
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert!(edit.source_projection.is_none());
        assert!(matches!(edit.branch, MathEditBranch::Text { .. }));
        let text = active_math_text_flow(&session)?;
        assert_eq!(
            text.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "x"
        );
    }
    Ok(())
}

#[test]
fn undo_and_redo_restore_equation_and_align_edit_targets() -> Result<(), Box<dyn std::error::Error>>
{
    for (query, align) in [("equation", false), ("align", true)] {
        let mut session = session_with_text("")?;
        accept(&mut session, query)?;
        let before_edit = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        assert!(before_edit.source_projection.is_none());
        assert!(matches!(before_edit.branch, MathEditBranch::Tree { .. }));
        session
            .run(Command::InsertText("x".to_owned()))
            .map_err(|error| format!("{query} insert failed: {error:?}"))?;
        let after_edit = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        assert!(matches!(
            after_edit.target,
            MathEditTarget::Block { align: value, .. } if value == align
        ));
        assert!(after_edit.source_projection.is_none());
        assert!(matches!(after_edit.branch, MathEditBranch::Tree { .. }));

        session
            .run(Command::Undo)
            .map_err(|error| format!("{query} undo failed: {error:?}"))?;
        let after_undo = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert_eq!(after_undo.target, after_edit.target);
        assert_eq!(after_undo.cursor, 0);
        assert!(after_undo.source_projection.is_none());
        assert!(matches!(after_undo.branch, MathEditBranch::Tree { .. }));

        session.run(Command::Redo)?;
        let after_redo = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert_eq!(after_redo.target, after_edit.target);
        assert!(after_redo.source_projection.is_none());
        assert!(matches!(after_redo.branch, MathEditBranch::Tree { .. }));
        assert!(inline_source(&session).is_some_and(|source| source.contains('x')));
        assert_eq!(after_redo.cursor, 1);
    }
    Ok(())
}

#[test]
fn redo_formula_creation_reopens_the_same_edit_target() -> Result<(), Box<dyn std::error::Error>> {
    for query in ["math", "equation", "align"] {
        let mut session = session_with_text("")?;
        accept(&mut session, query)?;
        let created = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        assert!(created.source_projection.is_none());
        assert!(matches!(created.branch, MathEditBranch::Tree { .. }));

        session.run(Command::Undo)?;
        assert!(session.math_edit().is_none());
        session.run(Command::Redo)?;
        let restored = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert_eq!(restored.target, created.target);
        assert!(restored.source_projection.is_none());
        assert!(matches!(restored.branch, MathEditBranch::Tree { .. }));
        assert_eq!(restored.cursor, created.cursor);
    }
    Ok(())
}

#[test]
fn backspace_deletes_an_empty_fenced_inline_formula() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    session.run(Command::InsertInlineMath("$ $".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let before_delete_affinity = session.selection_visual_affinity();
    assert_eq!(before_delete_affinity, crate::VisualAffinity::Upstream);
    session.run(Command::Delete(Direction::Backward))?;
    let after_delete_affinity = session.selection_visual_affinity();
    assert_eq!(after_delete_affinity, crate::VisualAffinity::Downstream);

    assert!(inline_source(&session).is_none());
    assert!(session.math_edit().is_none());

    session.run(Command::Undo)?;
    assert!(inline_source(&session).is_some());
    assert!(session.math_edit().is_some());
    assert_eq!(session.selection_visual_affinity(), before_delete_affinity);
    session.run(Command::Redo)?;
    assert!(inline_source(&session).is_none());
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection_visual_affinity(), after_delete_affinity);
    Ok(())
}

#[test]
fn incomplete_math_source_is_retained_and_has_local_undo_redo() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    let committed_document = session.document().clone();
    let committed_generation = session.document_generation();

    session.run(Command::InsertText("\\".to_owned()))?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x\\"));
    assert_eq!(session.document(), &committed_document);
    assert_eq!(session.document_generation(), committed_generation);
    assert!(session.can_undo());

    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x"));
    assert_eq!(session.document(), &committed_document);
    assert_eq!(session.document_generation(), committed_generation);
    assert!(session.can_redo());

    session.run(Command::Redo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x\\"));
    session.run(Command::InsertText("alpha".to_owned()))?;
    assert_eq!(
        session.math_edit().and_then(MathEdit::source),
        Some(r"x\alpha")
    );
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| !edit.source_draft_is_pending())
    );
    assert_ne!(session.document(), &committed_document);

    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x"));
    assert_eq!(session.document(), &committed_document);
    Ok(())
}

#[test]
fn incomplete_math_fence_can_be_removed_with_backspace() -> Result<(), EditError> {
    for fence in ["$", "\\("] {
        let mut session = session_with_text("").map_err(test_error)?;
        accept(&mut session, "math")?;
        enter_source_edit(&mut session, Direction::Forward)?;
        session.run(Command::InsertText(fence.to_owned()))?;
        assert_eq!(session.math_edit().and_then(MathEdit::source), Some(fence));
        assert!(session.has_uncommitted_math_source_draft());

        while session.has_uncommitted_math_source_draft() {
            session.run(Command::Delete(Direction::Backward))?;
        }
        assert_eq!(session.math_edit().and_then(MathEdit::source), Some(""));
    }
    Ok(())
}

#[test]
fn draft_undo_keeps_auto_paired_closing_braces() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("^".to_owned()))?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("^{}"));

    session.run(Command::InsertText("}\\".to_owned()))?;
    assert_eq!(
        session.math_edit().and_then(MathEdit::source),
        Some("^{}\\")
    );
    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("^{}"));
    session.run(Command::Redo)?;
    assert_eq!(
        session.math_edit().and_then(MathEdit::source),
        Some("^{}\\")
    );
    Ok(())
}

#[test]
fn incomplete_equation_and_align_source_is_kept_separate_from_the_committed_tree()
-> Result<(), EditError> {
    for kind in ["equation", "align"] {
        let mut session = session_with_text("").map_err(test_error)?;
        accept(&mut session, kind)?;
        enter_source_edit(&mut session, Direction::Forward)?;
        session.run(Command::InsertText("x".to_owned()))?;
        let committed = session.document().clone();

        session.run(Command::InsertText("\\".to_owned()))?;
        assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x\\"));
        assert_eq!(session.document(), &committed);
        assert!(session.has_uncommitted_math_source_draft());

        session.run(Command::Undo)?;
        assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x"));
        assert_eq!(session.document(), &committed);
        session.run(Command::Redo)?;
        assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x\\"));
    }
    Ok(())
}

#[test]
fn crossing_from_math_draft_history_to_document_history_discards_stale_redo()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(test_error)?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::InsertText("\\".to_owned()))?;

    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x"));
    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some(""));
    session.run(Command::Redo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x"));
    assert!(
        !session
            .math_edit()
            .is_some_and(|edit| edit.source_draft_is_pending())
    );
    Ok(())
}

#[test]
fn backspace_removes_empty_construct_and_then_the_object() -> Result<(), Box<dyn std::error::Error>>
{
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("sqrt".to_owned()))?;
    session.run(Command::AcceptCommandLine)?;
    assert_eq!(inline_source(&session).as_deref(), Some("\\sqrt{}"));
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(inline_source(&session).as_deref(), Some(""));
    session.run(Command::Delete(Direction::Backward))?;
    assert!(session.math_edit().is_none());
    assert_eq!(inline_source(&session), None);
    Ok(())
}

#[test]
fn backspace_removes_empty_construct_in_align_cell_and_history_restores_it()
-> Result<(), Box<dyn std::error::Error>> {
    let parsed = caret_math::parse_projection(
        "\\begin{aligned}x& % before\n \\sqrt{} % after\n\\end{aligned}",
        caret_math::MathInputKind::Align,
    )?;
    let (caret_math::ProjectedMath::Align(mut align), projection) = parsed.into_parts() else {
        return Err("Align source should produce an Align tree".into());
    };
    align
        .extra
        .insert("vendorAlign".to_owned(), serde_json::json!({"kept": true}));
    align.rows[0]
        .extra
        .insert("vendorRow".to_owned(), serde_json::json!("kept"));
    align.rows[0].cells[1]
        .extra
        .insert("vendorCell".to_owned(), serde_json::json!("kept"));
    align.rows[0].cells[1]
        .root
        .extra
        .insert("vendorFlow".to_owned(), serde_json::json!("kept"));
    let block_id = align.id.clone();
    let first_cell_id = align.rows[0].cells[0].root.id.clone();
    let second_cell_id = align.rows[0].cells[1].root.id.clone();
    let edit_range = projection.edit_range();
    let cell_range = projection
        .align()
        .and_then(|map| map.cell_range(0, 1))
        .ok_or(EditError::InvalidPosition)?;
    let source_base = cell_range
        .start
        .checked_sub(edit_range.start)
        .ok_or(EditError::InvalidPosition)?;
    let cursor = source_base + r"\sqrt{".len();

    let mut document = Document::blank();
    document.body.push(Block::Align(align));
    let mut session = Session::new(document)?;
    assert_eq!(
        first_align(&session)?.rows[0].cells[0].root.id,
        first_cell_id
    );
    assert_eq!(
        first_align(&session)?.rows[0].cells[1].root.id,
        second_cell_id
    );
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

    assert_eq!(
        session
            .math_edit()
            .and_then(MathEdit::align_cell_at_cursor)
            .map(|(row, cell, _)| (row, cell)),
        Some((0, 1))
    );
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    let generation_before_delete = session.document_generation();
    let deletion = session.run(Command::Delete(Direction::Backward))?;
    assert!(deletion.document_changed);
    assert!(session.document_generation() > generation_before_delete);
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    let deleted_align = first_align(&session)?;
    assert_eq!(deleted_align.rows[0].cells[0].root.id, first_cell_id);
    assert_eq!(deleted_align.rows[0].cells[1].root.id, second_cell_id);
    assert_eq!(
        deleted_align.extra["vendorAlign"],
        serde_json::json!({"kept": true})
    );
    assert_eq!(deleted_align.rows[0].extra["vendorRow"], "kept");
    assert_eq!(deleted_align.rows[0].cells[1].extra["vendorCell"], "kept");
    assert_eq!(
        deleted_align.rows[0].cells[1].root.extra["vendorFlow"],
        "kept"
    );
    assert!(!caret_math::serialize_tree(&deleted_align.rows[0].cells[1]).contains(r"\sqrt"));
    let after_delete = session
        .math_edit()
        .map(source_text)
        .ok_or(EditError::InvalidPosition)?;
    assert!(!after_delete.contains(r"\sqrt"));
    assert!(after_delete.contains("% before"));
    assert!(after_delete.contains("% after"));
    session.run(Command::Undo)?;
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    let restored_align = first_align(&session)?;
    assert_eq!(restored_align.rows[0].cells[0].root.id, first_cell_id);
    assert_eq!(restored_align.rows[0].cells[1].root.id, second_cell_id);
    assert_eq!(
        restored_align.extra["vendorAlign"],
        serde_json::json!({"kept": true})
    );
    assert_eq!(restored_align.rows[0].extra["vendorRow"], "kept");
    assert_eq!(restored_align.rows[0].cells[1].extra["vendorCell"], "kept");
    assert_eq!(
        restored_align.rows[0].cells[1].root.extra["vendorFlow"],
        "kept"
    );
    assert!(caret_math::serialize_tree(&restored_align.rows[0].cells[1]).contains(r"\sqrt"));
    assert_eq!(
        session.math_edit().map(source_text),
        Some("x& % before\n \\sqrt{} % after\n")
    );
    session.run(Command::Redo)?;
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    let redone_align = first_align(&session)?;
    assert_eq!(redone_align.rows[0].cells[0].root.id, first_cell_id);
    assert_eq!(redone_align.rows[0].cells[1].root.id, second_cell_id);
    assert_eq!(
        redone_align.extra["vendorAlign"],
        serde_json::json!({"kept": true})
    );
    assert_eq!(redone_align.rows[0].extra["vendorRow"], "kept");
    assert_eq!(redone_align.rows[0].cells[1].extra["vendorCell"], "kept");
    assert_eq!(
        redone_align.rows[0].cells[1].root.extra["vendorFlow"],
        "kept"
    );
    assert!(!caret_math::serialize_tree(&redone_align.rows[0].cells[1]).contains(r"\sqrt"));
    let after_redo = session
        .math_edit()
        .map(source_text)
        .ok_or(EditError::InvalidPosition)?;
    assert!(!after_redo.contains(r"\sqrt"));
    Ok(())
}

#[test]
fn backspace_in_empty_fraction_argument_keeps_nonempty_sibling_recoverable()
-> Result<(), Box<dyn std::error::Error>> {
    let parsed = caret_math::parse_projection(r"\frac{a}{}", caret_math::MathInputKind::Inline)?;
    let (caret_math::ProjectedMath::Tree(tree), projection) = parsed.into_parts() else {
        return Err("inline source should produce a MathTree".into());
    };
    let mut document = Document::blank();
    let Some(Block::Paragraph(paragraph)) = document.body.first_mut() else {
        return Err("blank document should contain a paragraph".into());
    };
    let paragraph_id = paragraph.id.clone();
    let math_id = NodeId::generate();
    paragraph.inlines = vec![ParagraphContent::Math(InlineMath {
        id: math_id.clone(),
        math_tree: tree,
        extra: Default::default(),
    })];
    let mut session = Session::new(document)?;
    session.math_edit = Some(MathEdit {
        target: MathEditTarget::Inline {
            paragraph_id,
            math_id,
        },
        source_projection: Some(Arc::new(projection)),
        source_draft: None,
        revision: MathEditRevision {
            session_id: session.session_id.clone(),
            document_generation: session.document_generation,
            projection_id: NodeId::generate(),
        },
        cursor: r"\frac{a}{".len(),
        branch: MathEditBranch::Source,
    });
    let before_document = session.document.clone();
    let before_generation = session.document_generation;

    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(session.math_edit().map(source_text), Some(r"\frac{a}"));
    assert!(session.has_uncommitted_math_source_draft());
    assert_eq!(session.document, before_document);
    assert_eq!(session.document_generation, before_generation);
    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().map(source_text), Some(r"\frac{a}{}"));
    assert!(!session.has_uncommitted_math_source_draft());
    session.run(Command::Redo)?;
    assert_eq!(session.math_edit().map(source_text), Some(r"\frac{a}"));
    assert_eq!(session.document, before_document);
    assert_eq!(session.document_generation, before_generation);
    Ok(())
}

#[test]
fn edited_math_documents_remain_saveable_after_tree_edits() -> Result<(), Box<dyn std::error::Error>>
{
    let mut inline = session_with_text("")?;
    accept(&mut inline, "math")?;
    inline.run(Command::InsertText("x".to_owned()))?;
    assert_math_document_is_saveable(inline.document());

    let mut equation = session_with_text("")?;
    accept(&mut equation, "equation")?;
    equation.run(Command::InsertText("x+y".to_owned()))?;
    assert_math_document_is_saveable(equation.document());

    let mut align = session_with_text("")?;
    accept(&mut align, "align")?;
    align.run(Command::InsertText("x&=y".to_owned()))?;
    assert_math_document_is_saveable(align.document());
    Ok(())
}

#[test]
fn align_enter_inserts_row_break() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    let active_view = session.current_environment_view()?;
    assert_eq!(
        active_view.innermost_block().map(|entry| entry.kind),
        Some(crate::EnvironmentKind::Align)
    );
    session.run(Command::InsertText("a&=b".to_owned()))?;
    session.run(Command::EditorKey(EditorKey::Enter))?;
    session.run(Command::InsertText("c&=d".to_owned()))?;
    let Some(Block::Align(value)) = session.document().body.first() else {
        panic!("expected align block");
    };
    assert_eq!(value.rows.len(), 2);
    assert_eq!(
        session.math_edit().and_then(MathEdit::align_cell_at_cursor),
        Some((1, 1, 2))
    );
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let source_map = source_projection(edit)
        .align()
        .ok_or(EditError::InvalidPosition)?;
    let content_range = source_map.content_range();
    assert_eq!(
        &source_projection(edit).source()[content_range],
        source_text(edit)
    );
    assert_eq!(source_text(edit), "a&=b \\\\ c&=d");
    Ok(())
}

#[test]
fn align_source_backspace_removes_empty_rows_and_keeps_nonempty_row_start_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let mut blank_trailing_row = session_with_text("")?;
    accept(&mut blank_trailing_row, "align")?;
    enter_source_edit(&mut blank_trailing_row, Direction::Forward)?;
    blank_trailing_row.run(Command::InsertText("a".to_owned()))?;
    blank_trailing_row.run(Command::EditorKey(EditorKey::Enter))?;
    assert!(
        blank_trailing_row
            .math_edit()
            .is_some_and(|edit| source_text(edit).contains(r"\\"))
    );
    blank_trailing_row.run(Command::EditorKey(EditorKey::Backspace))?;
    assert!(
        !blank_trailing_row
            .math_edit()
            .is_some_and(|edit| source_text(edit).contains(r"\\"))
    );
    assert_eq!(first_align(&blank_trailing_row)?.rows.len(), 1);

    let mut blank_multicell_row = session_with_text("")?;
    accept(&mut blank_multicell_row, "align")?;
    enter_source_edit(&mut blank_multicell_row, Direction::Forward)?;
    blank_multicell_row.run(Command::InsertText("a&b".to_owned()))?;
    blank_multicell_row.run(Command::EditorKey(EditorKey::Enter))?;
    blank_multicell_row.run(Command::InsertText("&".to_owned()))?;
    assert_eq!(
        blank_multicell_row
            .math_edit()
            .and_then(MathEdit::align_cell_at_cursor),
        Some((1, 1, 0))
    );
    blank_multicell_row.run(Command::EditorKey(EditorKey::Backspace))?;
    assert_eq!(first_align(&blank_multicell_row)?.rows.len(), 1);
    assert_eq!(first_align(&blank_multicell_row)?.rows[0].cells.len(), 2);
    assert_eq!(
        blank_multicell_row.math_edit().map(source_text),
        Some("a&b ")
    );

    let mut blank_middle_row = session_with_text("")?;
    accept(&mut blank_middle_row, "align")?;
    enter_source_edit(&mut blank_middle_row, Direction::Forward)?;
    blank_middle_row.run(Command::InsertText(r"a&b \\ & \\ c".to_owned()))?;
    let edit = blank_middle_row
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let projection = edit
        .source_projection
        .as_deref()
        .ok_or(EditError::InvalidPosition)?;
    let align_map = projection.align().ok_or(EditError::InvalidPosition)?;
    let cursor = align_map
        .row_navigation_range(1)
        .ok_or(EditError::InvalidPosition)?
        .start
        .checked_sub(projection.edit_range().start)
        .ok_or(EditError::InvalidPosition)?;
    blank_middle_row
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .cursor = cursor;
    assert_eq!(
        blank_middle_row
            .math_edit()
            .and_then(MathEdit::align_cell_at_cursor),
        Some((1, 0, 0))
    );
    blank_middle_row.run(Command::EditorKey(EditorKey::Backspace))?;
    assert_eq!(first_align(&blank_middle_row)?.rows.len(), 2);
    assert_eq!(first_align(&blank_middle_row)?.rows[0].cells.len(), 2);
    assert_eq!(
        caret_math::serialize_tree(&first_align(&blank_middle_row)?.rows[1].cells[0]),
        "c"
    );

    let mut blank_first_row = session_with_text("")?;
    accept(&mut blank_first_row, "align")?;
    enter_source_edit(&mut blank_first_row, Direction::Forward)?;
    blank_first_row.run(Command::EditorKey(EditorKey::Enter))?;
    blank_first_row.run(Command::InsertText("b".to_owned()))?;
    if let Some(edit) = &mut blank_first_row.math_edit {
        edit.cursor = 0;
    }
    assert_eq!(
        blank_first_row
            .math_edit()
            .and_then(MathEdit::align_cell_at_cursor),
        Some((0, 0, 0))
    );
    blank_first_row.run(Command::EditorKey(EditorKey::Backspace))?;
    assert_eq!(first_align(&blank_first_row)?.rows.len(), 1);
    assert_eq!(
        caret_math::serialize_tree(&first_align(&blank_first_row)?.rows[0].cells[0]),
        "b"
    );

    let mut nonempty_row = session_with_text("")?;
    accept(&mut nonempty_row, "align")?;
    enter_source_edit(&mut nonempty_row, Direction::Forward)?;
    nonempty_row.run(Command::InsertText("a".to_owned()))?;
    nonempty_row.run(Command::EditorKey(EditorKey::Enter))?;
    nonempty_row.run(Command::InsertText("b".to_owned()))?;
    nonempty_row.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(
        nonempty_row
            .math_edit()
            .and_then(MathEdit::align_cell_at_cursor),
        Some((1, 0, 0))
    );
    let source_before =
        source_text(nonempty_row.math_edit().ok_or(EditError::InvalidPosition)?).to_owned();
    let document_before = nonempty_row.document().clone();
    let edit_before = nonempty_row.math_edit().cloned();
    let effect = nonempty_row.run(Command::EditorKey(EditorKey::Backspace))?;
    assert!(!effect.document_changed);
    assert!(!effect.selection_changed);
    assert_eq!(
        source_text(nonempty_row.math_edit().ok_or(EditError::InvalidPosition)?),
        source_before
    );
    assert_eq!(nonempty_row.document(), &document_before);
    assert_eq!(nonempty_row.math_edit(), edit_before.as_ref());
    Ok(())
}

#[test]
fn align_backspace_removes_one_of_two_adjacent_empty_rows() -> Result<(), Box<dyn std::error::Error>>
{
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    session.run(Command::InsertText("a".to_owned()))?;
    session.run(Command::EditorKey(EditorKey::Enter))?;
    session.run(Command::EditorKey(EditorKey::Enter))?;
    assert_eq!(first_align(&session)?.rows.len(), 3);

    session.run(Command::EditorKey(EditorKey::Backspace))?;

    assert_eq!(first_align(&session)?.rows.len(), 2);
    assert_eq!(
        caret_math::serialize_tree(&first_align(&session)?.rows[0].cells[0]),
        "a"
    );
    assert!(
        first_align(&session)?.rows[1].cells[0]
            .root
            .content
            .is_empty()
    );
    Ok(())
}

#[test]
fn align_backspace_removes_a_row_with_an_empty_text_flow() -> Result<(), Box<dyn std::error::Error>>
{
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText(r"a \\ \text{}".to_owned()))?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let align_cell = edit
        .align_cell_at_cursor()
        .map(|(row, cell, _)| (row, cell))
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, Some(align_cell))?;
    let path = path::root_text_flow_path(tree, false, Some(align_cell))
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(align_cell.0, 1);
    session.enter_math_text_branch(path, 0);

    let effect = session.run(Command::EditorKey(EditorKey::Backspace))?;

    assert!(effect.document_changed);
    assert_eq!(first_align(&session)?.rows.len(), 1);
    assert_eq!(
        caret_math::serialize_tree(&first_align(&session)?.rows[0].cells[0]),
        "a"
    );
    Ok(())
}

#[test]
fn backspace_in_a_unique_empty_align_textflow_deletes_the_host_in_both_branches()
-> Result<(), Box<dyn std::error::Error>> {
    let mut source_session = session_with_text("")?;
    accept(&mut source_session, "align")?;
    enter_source_edit(&mut source_session, Direction::Forward)?;
    source_session.run(Command::InsertText(r"\text{}".to_owned()))?;
    source_session.run(Command::EditorKey(EditorKey::Backspace))?;
    assert!(first_align(&source_session).is_err());
    assert!(source_session.math_edit().is_none());

    let mut text_session = session_with_text("")?;
    accept(&mut text_session, "align")?;
    enter_source_edit(&mut text_session, Direction::Forward)?;
    text_session.run(Command::InsertText(r"\text{}".to_owned()))?;
    let edit = text_session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let align_cell = (0, 0);
    let tree = text_session.math_tree_for_edit(&edit, Some(align_cell))?;
    let path = path::root_text_flow_path(tree, false, Some(align_cell))
        .ok_or(EditError::InvalidPosition)?;
    text_session.enter_math_text_branch(path, 0);
    text_session.run(Command::EditorKey(EditorKey::Backspace))?;
    assert!(first_align(&text_session).is_err());
    assert!(text_session.math_edit().is_none());
    Ok(())
}

#[test]
fn align_backspace_at_nonempty_row_start_is_a_complete_no_op()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    session.run(Command::InsertText("a".to_owned()))?;
    session.run(Command::EditorKey(EditorKey::Enter))?;
    session.run(Command::InsertText("b".to_owned()))?;
    session.run(Command::Undo)?;
    assert!(session.can_redo());
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
    let edit = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    edit.cursor = 0;
    edit.branch = MathEditBranch::Tree { path, anchor };
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let selection_before = session.selection().clone();
    let visual_affinity_before = session.selection_visual_affinity;
    let generation_before = session.document_generation();
    let history_state_before = session.history_state_id;
    let merge_epoch_before = session.history_merge_epoch;
    let can_redo_before = session.can_redo();

    let effect = session.run(Command::EditorKey(EditorKey::Backspace))?;

    assert!(!effect.document_changed);
    assert!(!effect.selection_changed);
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.selection_visual_affinity, visual_affinity_before);
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id, history_state_before);
    assert_eq!(session.history_merge_epoch, merge_epoch_before);
    assert_eq!(session.can_redo(), can_redo_before);
    session.run(Command::Redo)?;
    assert_eq!(
        caret_math::serialize_tree(&first_align(&session)?.rows[1].cells[0]),
        "b"
    );
    Ok(())
}

#[test]
fn align_backspace_checks_every_cell_and_refuses_to_drop_row_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    let mut occupied_row = session_with_text("")?;
    accept(&mut occupied_row, "align")?;
    occupied_row.run(Command::InsertText("a".to_owned()))?;
    occupied_row.run(Command::InsertText("&".to_owned()))?;
    occupied_row.run(Command::InsertText("b".to_owned()))?;
    occupied_row.run(Command::EditorKey(EditorKey::Enter))?;
    occupied_row.run(Command::InsertText("c".to_owned()))?;
    let row = &first_align(&occupied_row)?.rows[1];
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
    let edit = occupied_row
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    edit.cursor = 0;
    edit.branch = MathEditBranch::Tree { path, anchor };
    let document_before = occupied_row.document().clone();
    let edit_before = occupied_row.math_edit().cloned();
    occupied_row.run(Command::EditorKey(EditorKey::Backspace))?;
    assert_eq!(occupied_row.document(), &document_before);
    assert_eq!(occupied_row.math_edit(), edit_before.as_ref());
    assert_eq!(first_align(&occupied_row)?.rows.len(), 2);
    assert_eq!(
        caret_math::serialize_tree(&first_align(&occupied_row)?.rows[0].cells[0]),
        "a"
    );
    assert_eq!(
        caret_math::serialize_tree(&first_align(&occupied_row)?.rows[0].cells[1]),
        "b"
    );
    assert_eq!(
        caret_math::serialize_tree(&first_align(&occupied_row)?.rows[1].cells[1]),
        "c"
    );

    let mut metadata_row = session_with_text("")?;
    accept(&mut metadata_row, "align")?;
    metadata_row.run(Command::InsertText("a".to_owned()))?;
    metadata_row.run(Command::EditorKey(EditorKey::Enter))?;
    let Block::Align(align) = metadata_row
        .document
        .body
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    align.rows[1]
        .extra
        .insert("vendorRow".to_owned(), serde_json::Value::Bool(true));
    let document_before = metadata_row.document().clone();
    let edit_before = metadata_row.math_edit().cloned();
    let generation_before = metadata_row.document_generation();

    assert_eq!(
        metadata_row.run(Command::EditorKey(EditorKey::Backspace)),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(metadata_row.document(), &document_before);
    assert_eq!(metadata_row.math_edit(), edit_before.as_ref());
    assert_eq!(metadata_row.document_generation(), generation_before);
    Ok(())
}

#[test]
fn align_source_row_backspace_preserves_or_atomically_rejects_neighbor_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("a".to_owned()))?;
    session.run(Command::EditorKey(EditorKey::Enter))?;
    let Block::Align(align) = session
        .document
        .body
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(MathContent::Math(node)) = align.rows[0].cells[0].root.content.first_mut() else {
        return Err(EditError::InvalidPosition.into());
    };
    node.extra
        .insert("vendorNode".to_owned(), serde_json::Value::Bool(true));
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let selection_before = session.selection().clone();
    let generation_before = session.document_generation();
    let history_state_before = session.history_state_id;
    let merge_epoch_before = session.history_merge_epoch;
    let can_redo_before = session.can_redo();

    match session.run(Command::EditorKey(EditorKey::Backspace)) {
        Ok(effect) => {
            assert!(effect.document_changed);
            assert_eq!(first_align(&session)?.rows.len(), 1);
            let Some(MathContent::Math(node)) =
                first_align(&session)?.rows[0].cells[0].root.content.first()
            else {
                return Err(EditError::InvalidPosition.into());
            };
            assert_eq!(
                node.extra.get("vendorNode"),
                Some(&serde_json::Value::Bool(true))
            );
            assert!(
                !session
                    .math_edit()
                    .is_some_and(MathEdit::source_draft_is_pending)
            );
        }
        Err(EditError::InvalidMath(_)) => {
            assert_eq!(session.document(), &document_before);
            assert_eq!(session.math_edit(), edit_before.as_ref());
            assert_eq!(session.selection(), &selection_before);
            assert_eq!(session.document_generation(), generation_before);
            assert_eq!(session.history_state_id, history_state_before);
            assert_eq!(session.history_merge_epoch, merge_epoch_before);
            assert_eq!(session.can_redo(), can_redo_before);
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[test]
fn incomplete_source_prefix_is_kept_out_of_the_committed_tree()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    let committed = session.document().clone();
    session.run(Command::InsertText("\\".to_owned()))?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x\\"));
    assert_eq!(session.document(), &committed);
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    Ok(())
}

#[test]
fn leaving_and_reentering_inline_math_splits_typing_undo_groups()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.begin_inline_math()?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("xy"));

    session.run(Command::Undo)?;

    assert_eq!(inline_source(&session).as_deref(), Some("x"));
    enter_source_edit(&mut session, Direction::Backward)?;
    assert_eq!(session.math_edit().map(source_text), Some("x"));
    Ok(())
}

#[test]
fn deleting_empty_align_after_a_formula_chain_is_undoable() -> Result<(), Box<dyn std::error::Error>>
{
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        display_math_block(false, "a")?,
        display_math_block(true, "b")?,
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::Delete(Direction::Backward))?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    assert!(
        first_align(&session)?.rows[0].cells[0]
            .root
            .content
            .is_empty()
    );
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: true, .. })
    ));

    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(session.document().body.len(), 2);
    assert!(session.math_edit().is_none());

    session.run(Command::Undo)?;
    assert_eq!(session.document().body.len(), 3);
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: true, .. })
    ));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::Redo)?;
    assert_eq!(session.document().body.len(), 2);
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn superscript_and_subscript_templates_use_previous_tree_content_as_their_base()
-> Result<(), EditError> {
    for (template, empty_script, filled_script) in
        [("^{}", "x^{}", "x^{2}"), ("_{}", "x_{}", "x_{2}")]
    {
        let mut session = session_with_text("").map_err(test_error)?;
        session.begin_inline_math()?;
        session.run(Command::InsertText("x".to_owned()))?;

        session.math_insert_template(template)?;
        assert_eq!(inline_source(&session).as_deref(), Some(empty_script));
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree { path, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        assert!(matches!(
            path.steps(),
            [MathSequenceStep::Operand {
                operand_index: 1,
                ..
            }]
        ));

        session.run(Command::InsertText("2".to_owned()))?;
        assert_eq!(inline_source(&session).as_deref(), Some(filled_script));
        session.run(Command::Undo)?;
        assert_eq!(inline_source(&session).as_deref(), Some(empty_script));
        session.run(Command::Redo)?;
        assert_eq!(inline_source(&session).as_deref(), Some(filled_script));
    }
    Ok(())
}
