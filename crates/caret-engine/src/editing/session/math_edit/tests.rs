//! 公式编辑状态的行为测试。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

fn aligned_body_offset(align: &Align) -> usize {
    align
        .source
        .find("\\begin{aligned}")
        .unwrap_or_else(|| panic!("aligned test source must have an opening environment"))
        + "\\begin{aligned}".len()
}

fn assert_math_document_is_not_yet_saveable(document: &caret_doc::Document) {
    assert!(matches!(
        caret_doc::CdocFile::new(document.clone()),
        Err(caret_doc::DocumentError::ReadOnlyPreview)
    ));
}

fn first_paragraph_text(session: &Session) -> String {
    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        panic!("first block should be a paragraph");
    };
    TextMap::new(paragraph)
        .unwrap_or_else(|error| panic!("paragraph mapping failed: {error}"))
        .text
}

fn math_tree_has_font_environment(flow: &MathFlow) -> bool {
    flow.content.iter().any(math_content_has_font_environment)
}

fn math_content_has_font_environment(content: &MathContent) -> bool {
    match content {
        MathContent::Environment(environment) => {
            environment.kind == caret_doc::InlineEnvironmentKind::Font
                || environment
                    .children
                    .iter()
                    .any(math_content_has_font_environment)
        }
        MathContent::Math(node) => node.children.iter().any(math_tree_has_font_environment),
        MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
    }
}

fn first_math_text_flow_mut(flow: &mut MathFlow) -> Option<&mut caret_doc::TextFlow> {
    flow.content
        .iter_mut()
        .find_map(first_math_content_text_mut)
}

fn first_math_text_flow(flow: &MathFlow) -> Option<&caret_doc::TextFlow> {
    flow.content.iter().find_map(first_math_content_text)
}

fn first_math_content_text(content: &MathContent) -> Option<&caret_doc::TextFlow> {
    match content {
        MathContent::Text(text) => Some(text),
        MathContent::Math(node) => node
            .children
            .iter()
            .find_map(|child| child.content.iter().find_map(first_math_content_text)),
        MathContent::Environment(environment) => environment
            .children
            .iter()
            .find_map(first_math_content_text),
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => None,
    }
}

fn active_math_text_flow(session: &Session) -> Result<&caret_doc::TextFlow, EditError> {
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    path.resolve(session.math_tree_for_edit(edit, path.align_cell())?)
        .ok_or(EditError::InvalidPosition)
}

fn first_math_content_text_mut(content: &mut MathContent) -> Option<&mut caret_doc::TextFlow> {
    match content {
        MathContent::Text(text) => Some(text),
        MathContent::Math(node) => node.children.iter_mut().find_map(first_math_text_flow_mut),
        MathContent::Environment(environment) => environment
            .children
            .iter_mut()
            .find_map(first_math_content_text_mut),
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => None,
    }
}

fn first_math_environment_mut(
    flow: &mut MathFlow,
) -> Option<&mut caret_doc::InlineEnvironment<MathContent>> {
    flow.content
        .iter_mut()
        .find_map(first_math_content_environment_mut)
}

fn first_math_content_environment_mut(
    content: &mut MathContent,
) -> Option<&mut caret_doc::InlineEnvironment<MathContent>> {
    match content {
        MathContent::Environment(environment) => Some(environment),
        MathContent::Math(node) => node
            .children
            .iter_mut()
            .find_map(first_math_environment_mut),
        MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => None,
    }
}

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
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\alpha".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(6));

    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    Ok(())
}

#[test]
fn backspace_deletes_one_grapheme_from_formula_source() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("abc".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(inline_source(&session).as_deref(), Some("ab"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    Ok(())
}

#[test]
fn inline_math_root_text_is_promoted_to_the_paragraph_flow() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn inline_math_root_split_preserves_an_empty_math_anchor_inside_text() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn editing_inline_math_source_promotes_new_root_text_and_moves_focus_to_it() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("ab".to_owned()))?;
    let original_math_id = match session.document().body.first() {
        Some(Block::Paragraph(paragraph)) => match paragraph.inlines.first() {
            Some(ParagraphContent::Math(math)) => math.id.clone(),
            _ => return Err(EditError::InvalidPosition),
        },
        _ => return Err(EditError::InvalidPosition),
    };
    session.run(Command::EnterMath(Direction::Backward))?;
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
fn inline_math_root_text_keeps_one_format_environment_wrapper() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn inline_math_text_inside_fixed_operand_stays_in_math_tree() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn inline_math_root_split_rejects_unowned_source_map_extensions_atomically() -> Result<(), EditError>
{
    let caret_math::ParsedMath::Tree(mut math_tree) =
        caret_math::parse(r"\text{x}", caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
    else {
        return Err(EditError::InvalidPosition);
    };
    let Some(MathContent::Text(text)) = math_tree.root.content.first_mut() else {
        return Err(EditError::InvalidPosition);
    };
    text.source_map[0]
        .extra
        .insert("vendorMapField".to_owned(), serde_json::json!(true));
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(ParagraphContent::Math(InlineMath {
        id: NodeId::generate(),
        math_tree,
        extra: BTreeMap::new(),
    }));
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session = Session::new(document)?;
    let mut after_math = session.selection().focus.clone();
    after_math.offset = OBJECT_REPLACEMENT.len_utf8();
    session.run(Command::SetSelection(Some(Selection {
        anchor: after_math.clone(),
        focus: after_math,
    })))?;
    session.run(Command::EnterMath(Direction::Backward))?;

    let before_document = session.document().clone();
    let before_selection = session.selection().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    assert!(session.run(Command::InsertText("y".to_owned())).is_err());
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.selection(), &before_selection);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn unsupported_host_commands_do_not_escape_formula_editing() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::InsertText("host".to_owned()))?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
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
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn formula_focus_allows_global_settings_and_mouse_selection_exit() -> Result<(), EditError> {
    let mut session = session_with_text("host").map_err(|_| EditError::InvalidPosition)?;
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
    let mut source_session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    source_session.run(Command::InsertInlineMath("x".to_owned()))?;
    source_session.run(Command::EnterMath(Direction::Backward))?;
    let source_edit = source_session.math_edit().cloned();
    source_session.run(Command::SetSelection(None))?;
    assert_eq!(source_session.math_edit(), source_edit.as_ref());

    let mut text_session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
    assert!(matches!(edit.branch, MathEditBranch::Text { anchor, .. } if anchor == edit.cursor));
    assert_eq!(
        inline_source(&text_session).as_deref(),
        Some(r"\text{abcd}")
    );
    Ok(())
}

#[test]
fn mod_backspace_does_not_touch_a_paragraph_adjacent_to_display_math() -> Result<(), EditError> {
    let mut session = session_with_text("outside").map_err(|_| EditError::InvalidPosition)?;
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
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn mod_backspace_rejects_text_and_environment_metadata_atomically() -> Result<(), EditError> {
    for (source, metadata_kind) in [
        (r"\frac{x}{\text{abc}}", 0),
        (r"\frac{x}{\text{abc}}", 1),
        (r"\frac{x}{\text{\textsf{abc}}}", 2),
    ] {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
                text.source_map[0]
                    .extra
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
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
                        source_range: None,
                        props: BTreeMap::new(),
                        children,
                        extra: BTreeMap::new(),
                    })];
            }
            1 => math
                .math_tree
                .root
                .content
                .push(MathContent::Text(caret_doc::TextFlow {
                    runs: vec![Run {
                        text: "secret".to_owned(),
                        language: None,
                        props: BTreeMap::new(),
                        extra: BTreeMap::new(),
                    }],
                    source_range: None,
                    source_map: Vec::new(),
                    extra: BTreeMap::new(),
                })),
            _ => {
                let Some(MathContent::Math(mut node)) =
                    math.math_tree.root.content.first().cloned()
                else {
                    return Err(EditError::InvalidPosition);
                };
                node.source_range = None;
                node.text = Some("y".to_owned());
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
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(String::new()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
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
fn mod_backspace_preserves_text_island_selection_through_source_map() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn mod_backspace_rejects_selection_that_becomes_mid_grapheme_in_source_text()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn mod_backspace_accepts_font_properties_reconstructed_from_math_source() -> Result<(), EditError> {
    for source in [
        r"\frac{x}{\text{\textsf{sans}}}",
        r"\frac{x}{\text{\texttt {mono}}}",
    ] {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("x\ny".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;

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
fn mod_backspace_keeps_source_caret_in_place_and_typing_continues_there() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("abc".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
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
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
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
fn mod_backspace_snaps_collapsed_caret_downstream_when_text_graphemes_join() -> Result<(), EditError>
{
    let mut session = session_with_text("\u{301}").map_err(|_| EditError::InvalidPosition)?;
    let mut start = session.selection().focus.clone();
    start.offset = 0;
    start.affinity = Affinity::Downstream;
    session.run(Command::SetSelection(Some(Selection {
        anchor: start.clone(),
        focus: start,
    })))?;
    session.run(Command::InsertInlineMath("a".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;

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
        caret_math::parse("", caret_math::MathInputKind::Inline)
            .map_err(|_| EditError::InvalidMath("empty inline fixture failed".to_owned()))?
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
            source_range: None,
            source_map: Vec::new(),
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
            source_range: None,
            source_map: Vec::new(),
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
    let mut session = session_with_text("a").map_err(|_| EditError::InvalidPosition)?;
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
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.selection().focus.offset, atom_end + "\u{301}".len());
    Ok(())
}

#[test]
fn text_island_accepts_plain_input_and_rebases_its_path() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    let before_command = session.document().clone();
    let before_focus = session.math_edit().cloned();
    assert_eq!(
        session.run(Command::BeginCommandLine),
        Err(EditError::UnknownCommand)
    );
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
fn math_text_run_property_splits_range_and_undoes_without_reparsing_source() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    let source = session.math_edit().map(|edit| edit.source().to_owned());

    let effect = session.run(Command::ToggleRunUnderline)?;
    assert!(effect.document_changed);
    assert_eq!(
        session.math_edit().map(|edit| edit.source().to_owned()),
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
        session.math_edit().map(|edit| edit.source()),
        source.as_deref()
    );
    session.run(Command::Redo)?;
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    Ok(())
}
#[test]
fn math_text_pending_run_property_styles_inserted_text_and_moves_clear_it() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
    Ok(())
}
#[test]
fn math_text_run_property_rejects_partial_unknown_run_metadata_atomically() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn starting_math_from_a_text_island_splits_the_text_source_and_is_undoable() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;

    session.begin_inline_math()?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{ab$$cd}"));
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(edit.cursor, r"\text{ab$".len());
    assert!(matches!(edit.branch, MathEditBranch::Source));

    session.run(Command::InsertText("x".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{ab$x$cd}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Source)
    ));

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{ab$$cd}"));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abcd}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    Ok(())
}
#[test]
fn starting_math_inside_nested_text_style_keeps_the_wrapper() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn starting_math_splits_known_run_properties_across_text_flows() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn starting_math_rejects_unmappable_run_metadata_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
    assert!(matches!(
        session.begin_inline_math(),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}
#[test]
fn starting_math_normalizes_inline_fences_and_preserves_multirange_mapping_metadata()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{a\%bc}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 4,
    })?;
    let Some(Block::Equation(equation)) = session.document.body.first_mut() else {
        return Err(EditError::InvalidPosition);
    };
    let text =
        first_math_text_flow_mut(&mut equation.math_tree.root).ok_or(EditError::InvalidPosition)?;
    let escaped = text
        .source_map
        .iter_mut()
        .find(|entry| {
            entry.source_range.end - entry.source_range.start > entry.text_end - entry.text_start
        })
        .ok_or(EditError::InvalidPosition)?;
    escaped
        .extra
        .insert("futureMapField".to_owned(), serde_json::json!("preserve"));

    session.run(Command::InsertInlineMath("$x$".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{a\%bc$x$}"));
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let mut root = equation.math_tree.root.clone();
    let text = first_math_text_flow_mut(&mut root).ok_or(EditError::InvalidPosition)?;
    let preserved = text
        .source_map
        .iter()
        .any(|entry| entry.extra.get("futureMapField") == Some(&serde_json::json!("preserve")));
    assert!(preserved);
    Ok(())
}
#[test]
fn starting_math_from_nonfirst_align_cell_keeps_unicode_host_offset()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    session.run(Command::InsertText("α & \\text{abc}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, .. }) if path.align_cell() == Some((0, 1))
    ));
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;

    session.run(Command::InsertInlineMath("\\(y\\)".to_owned()))?;

    assert!(matches!(
        session.document().body.first(),
        Some(Block::Align(value))
            if value.rows[0].cells[0].source == "α"
                && value.rows[0].cells[1].source.contains(r"\text{a$y$bc}")
    ));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Source)
    ));
    Ok(())
}

#[test]
fn starting_math_inside_equation_text_flow_is_undoable() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "equation")?;
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
    assert!(matches!(
        session.document().body.first(),
        Some(Block::Equation(value)) if value.math_tree.source == r"\text{a$y$bc}"
    ));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Source)
    ));

    session.run(Command::Undo)?;
    assert!(matches!(
        session.document().body.first(),
        Some(Block::Equation(value)) if value.math_tree.source == r"\text{abc}"
    ));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    Ok(())
}

#[test]
fn invalid_inline_fence_in_text_island_preserves_document_and_focus() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
        edit.branch,
        MathEditBranch::Text { anchor: 3, .. }
    ));

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abcd}"));
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(edit.cursor, 3);
    assert!(matches!(
        edit.branch,
        MathEditBranch::Text { anchor: 1, .. }
    ));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{aXYd}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    Ok(())
}
#[test]
fn math_text_selection_delete_collapses_at_start_and_arrows_collapse_by_direction()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
        edit.branch,
        MathEditBranch::Text { anchor: 3, .. }
    ));
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { anchor: 3, .. })
    ));
    Ok(())
}
#[test]
fn math_text_selection_rejects_non_grapheme_positions_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn math_text_selection_replaces_a_complete_escaped_source_mapping() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn inserting_empty_text_deletes_a_selected_math_text_range() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn adjacent_text_islands_share_caret_navigation() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
    let second_range = match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    {
        MathEditBranch::Text { path, .. } => path.source_range(),
        MathEditBranch::Source => return Err(EditError::InvalidPosition),
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
    let first_range = match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    {
        MathEditBranch::Text { path, .. } => path.source_range(),
        MathEditBranch::Source => return Err(EditError::InvalidPosition),
    };
    assert_ne!(first_range, second_range);
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));

    let crossed_forward = session.run(Command::MoveCaret(Direction::Forward));
    assert!(
        crossed_forward.is_ok(),
        "crossing to next island failed: {crossed_forward:?}"
    );
    let next_range = match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    {
        MathEditBranch::Text { path, .. } => path.source_range(),
        MathEditBranch::Source => return Err(EditError::InvalidPosition),
    };
    assert_eq!(next_range, second_range);
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    Ok(())
}
#[test]
fn math_atoms_separate_text_islands_but_empty_islands_keep_a_caret_stop() -> Result<(), EditError> {
    let mut separated = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut separated, r"\text{a}x\text{b}")?;
    separated.run(Command::EnterMath(Direction::Backward))?;
    separated.run(Command::MoveCaret(Direction::Backward))?;
    separated.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(separated.math_edit().map(|edit| edit.cursor), Some(0));
    separated.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        separated.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Source)
    ));

    let mut empty = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
    assert_eq!(path.source_range().0, path.source_range().1);
    assert_eq!(edit.cursor, 0);

    empty.run(Command::MoveCaret(Direction::Forward))?;
    let edit = empty.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert_ne!(path.source_range().0, path.source_range().1);
    assert_eq!(edit.cursor, 0);
    empty.run(Command::MoveCaret(Direction::Backward))?;
    let edit = empty.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(path.source_range().0, path.source_range().1);
    assert_eq!(edit.cursor, 0);
    Ok(())
}
#[test]
fn text_island_boundary_exit_and_reverse_motion_reenter_text_mode() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));

    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Source)
    ));
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));

    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Source)
    ));
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    Ok(())
}
#[test]
fn empty_text_island_is_an_editable_caret_target() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn empty_text_island_at_formula_edge_exits_without_reentry_trap() -> Result<(), EditError> {
    let mut forward = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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

    let mut backward = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    backward.run(Command::InsertInlineMath(r"\text{}".to_owned()))?;
    backward.run(Command::EnterMath(Direction::Backward))?;
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
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"a\text{}b")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let (index, range) = tree
        .root
        .content
        .iter()
        .enumerate()
        .find_map(|(index, content)| match content {
            MathContent::Text(text) if text.runs.iter().all(|run| run.text.is_empty()) => text
                .source_range
                .as_ref()
                .map(|range| (index, (range.start, range.end))),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(index)], range, None);
    session.enter_math_text_branch(path, 0);

    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Source)
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
fn nested_empty_text_island_has_symmetric_one_delimiter_navigation() -> Result<(), EditError> {
    fn empty_text_range(contents: &[MathContent]) -> Option<(u32, u32)> {
        for content in contents {
            match content {
                MathContent::Text(text) if text.runs.iter().all(|run| run.text.is_empty()) => {
                    return text
                        .source_range
                        .as_ref()
                        .map(|range| (range.start, range.end));
                }
                MathContent::Environment(environment) => {
                    if let Some(range) = empty_text_range(&environment.children) {
                        return Some(range);
                    }
                }
                MathContent::Math(node) => {
                    for child in &node.children {
                        if let Some(range) = empty_text_range(&child.content) {
                            return Some(range);
                        }
                    }
                }
                MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
            }
        }
        None
    }

    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{\textbf{}}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let (start, end) = empty_text_range(&tree.root.content).ok_or(EditError::InvalidPosition)?;
    let path = path::path_at_source_boundary(tree, end as usize, false, None)
        .ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 0);

    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Source)
    ));
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| edit.cursor > start as usize)
    );
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    Ok(())
}
#[test]
fn deleting_last_character_of_a_grouped_textflow_falls_back_to_source_caret()
-> Result<(), EditError> {
    let source = r"\text{a{b}}";
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
        .find_map(|content| match content {
            MathContent::Text(text) => text.source_range.as_ref().map(|range| range.end as usize),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let path = path::path_at_source_boundary(tree, first_text_end, false, None)
        .ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 1);

    session.run(Command::Delete(Direction::Backward))?;

    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Source)
    ));
    assert!(inline_source(&session).is_some_and(|source| !source.contains('a')));
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
        Some(r"\text{abc\%\textchar{20}\textchar{20}\textchar{9}\textchar{A0}}")
    );
    let MathEditBranch::Text { path, .. } = &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    let text = path
        .resolve(&equation.math_tree)
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
        Some(r"\text{abc\%\textchar{20}\textchar{20}\textchar{9}\textchar{A0}}")
    );
    Ok(())
}
#[test]
fn equation_and_align_text_islands_share_the_same_edit_branch()
-> Result<(), Box<dyn std::error::Error>> {
    let mut equation = session_with_text("")?;
    accept(&mut equation, "equation")?;
    equation.run(Command::InsertText(r"\text{abc}".to_owned()))?;
    equation.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        equation.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    equation.run(Command::InsertText("X".to_owned()))?;
    assert!(matches!(
        equation.document().body.first(),
        Some(Block::Equation(value)) if value.math_tree.source == r"\text{abcX}"
    ));
    equation.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    equation.run(Command::ToggleRunUnderline)?;
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

    let mut align = session_with_text("")?;
    accept(&mut align, "align")?;
    align.run(Command::InsertText(r"\text{abc}".to_owned()))?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        align.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, .. }) if path.align_cell() == Some((0, 0))
    ));
    align.run(Command::InsertText("X".to_owned()))?;
    assert!(matches!(
        align.document().body.first(),
        Some(Block::Align(value)) if value.rows[0].cells[0].source.contains(r"\text{abcX}")
    ));
    align.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    align.run(Command::ToggleRunUnderline)?;
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
    Ok(())
}

#[test]
fn adjacent_align_text_islands_keep_cell_host_offsets_through_edit_and_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    session.run(Command::InsertText(r"\text{a}&\text{b}\text{c}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, .. }) if path.align_cell() == Some((0, 1))
    ));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));

    session.run(Command::InsertText("X".to_owned()))?;
    assert!(matches!(
        session.document().body.first(),
        Some(Block::Align(value))
            if value.rows[0].cells[0].source.contains(r"\text{a}")
                && value.rows[0].cells[1].source.contains(r"\text{bX}\text{c}")
    ));
    session.run(Command::Undo)?;
    assert!(matches!(
        session.document().body.first(),
        Some(Block::Align(value)) if value.rows[0].cells[1].source.contains(r"\text{b}\text{c}")
    ));
    session.run(Command::Redo)?;
    assert!(matches!(
        session.document().body.first(),
        Some(Block::Align(value)) if value.rows[0].cells[1].source.contains(r"\text{bX}\text{c}")
    ));
    Ok(())
}

#[test]
fn equation_text_delete_snaps_after_graphemes_join() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "equation")?;
    session.run(Command::InsertText("\\text{🇦x🇧}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(4));
    session.run(Command::Delete(Direction::Forward))?;

    assert!(matches!(
        session.document().body.first(),
        Some(Block::Equation(value)) if value.math_tree.source == "\\text{🇦🇧}"
    ));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    Ok(())
}

#[test]
fn align_text_island_uses_parser_cell_ranges_after_comments_and_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let mut align = session_with_text("")?;
    accept(&mut align, "align")?;
    align.run(Command::PasteText(
        "x & y \\\\ % row note\n \\text{abc}".to_owned(),
    ))?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        align.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, .. }) if path.align_cell() == Some((1, 0))
    ));
    align.run(Command::InsertText("X".to_owned()))?;
    assert!(matches!(
        align.document().body.first(),
        Some(Block::Align(value)) if value.rows[1].cells[0].source.contains(r"\text{abcX}")
    ));
    Ok(())
}

#[test]
fn enter_in_align_textflow_rejects_without_using_source_offsets()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    session.run(Command::InsertText(r"\text{abc}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
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
fn text_island_edits_preserve_run_properties_and_reject_opaque_run_fields()
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
    assert!(
        text.runs.last().is_some_and(|run| {
            run.props.get("underline") == Some(&serde_json::Value::Bool(true))
        })
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
    assert!(
        text.runs.last().is_some_and(|run| {
            run.props.get("underline") == Some(&serde_json::Value::Bool(true))
        })
    );

    let paragraph = match session.document.body.first_mut() {
        Some(Block::Paragraph(paragraph)) => paragraph,
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let text =
        first_math_text_flow_mut(&mut math.math_tree.root).ok_or(EditError::InvalidPosition)?;
    text.runs
        .last_mut()
        .ok_or(EditError::InvalidPosition)?
        .extra
        .insert("vendorRun".to_owned(), serde_json::Value::Bool(true));
    let before = session.document().clone();
    let before_edit = session.math_edit().cloned();
    assert_eq!(
        session.run(Command::InsertText("Y".to_owned())),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before);
    assert_eq!(session.math_edit(), before_edit.as_ref());
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
fn tab_does_not_exit_math_edit_or_insert_spaces_in_the_host_paragraph() -> Result<(), EditError> {
    let mut session = session_with_text("beforeafter").map_err(|_| EditError::InvalidPosition)?;
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
fn paste_preserves_latex_group_syntax() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    session.run(Command::PasteText("x^{2}".to_owned()))?;

    assert_eq!(inline_source(&session).as_deref(), Some("x^{2}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(5));
    Ok(())
}

#[test]
fn align_row_break_is_one_navigation_and_deletion_unit() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    session.run(Command::InsertText(r"a\\abc".to_owned()))?;

    for _ in 0..4 {
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    session.run(Command::Delete(Direction::Backward))?;

    let Some(Block::Align(align)) = session.document().body.first() else {
        panic!("expected Align block");
    };
    assert_eq!(align.source, r"\begin{aligned}aabc\end{aligned}");
    Ok(())
}

#[test]
fn math_source_mode_rejects_ime_without_changing_focus_or_document()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("abc".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
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
fn align_cell_mapping_counts_rows_and_cells() {
    assert_eq!(align_cell_at("a&=b \\\\ c& =d", 12), Some((1, 1, 1)));
    assert_eq!(align_cell_at("a&=b", 1), Some((0, 0, 1)));
}

#[test]
fn align_cell_mapping_ignores_nested_environment_separators() {
    let source = r"\begin{matrix}a&b\\c&d\end{matrix}&=z";
    let nested_cell = source
        .find("c&d")
        .unwrap_or_else(|| panic!("missing nested cell"))
        + 2;
    assert_eq!(
        align_cell_at(source, nested_cell),
        Some((0, 0, nested_cell))
    );

    let outer_cell = source
        .rfind('&')
        .unwrap_or_else(|| panic!("missing outer separator"))
        + 1;
    assert_eq!(align_cell_at(source, outer_cell), Some((0, 1, 0)));

    let commented_begin =
        "\\begin % environment name follows the comment\n{matrix}a&b\\end{matrix}&=z";
    let nested_separator = commented_begin
        .find("a&b")
        .unwrap_or_else(|| panic!("missing commented nested matrix"))
        + 2;
    assert_eq!(
        align_cell_at(commented_begin, nested_separator),
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
    assert_eq!(align_cell_at(source, in_comment), Some((0, 0, in_comment)));
    let after_outer_separator = source
        .rfind('&')
        .unwrap_or_else(|| panic!("missing separator"))
        + 2;
    assert_eq!(
        align_cell_at(source, after_outer_separator),
        Some((0, 1, 1))
    );

    assert_eq!(
        align_cell_at("% leading cell comment\nx", 24),
        Some((0, 0, 1))
    );
    assert_eq!(
        align_cell_at("% leading cell comment\nx", 4),
        Some((0, 0, 0))
    );
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
    if let Some(range) = &mut text.source_range {
        range
            .extra
            .insert("vendorRange".to_owned(), serde_json::json!(5));
    }
    text.source_map[0]
        .extra
        .insert("vendorMap".to_owned(), serde_json::json!(6));
    text.source_map[0]
        .source_range
        .extra
        .insert("vendorMapRange".to_owned(), serde_json::json!(7));

    let updated_source = source.replace("+x", "+y");
    let mut updated = parsed_inline_tree(&updated_source);
    let edit = (source.len() - 1)..source.len();
    assert!(preserve_math_tree_metadata_for_edit(
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
    assert_eq!(text.source_map[0].extra["vendorMap"], 6);
    assert_eq!(text.source_map[0].source_range.extra["vendorMapRange"], 7);
    let Some(range) = &text.source_range else {
        panic!("mapped TextFlow should retain its source range");
    };
    assert_eq!(range.extra["vendorRange"], 5);
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
fn duplicate_text_source_map_targets_refuse_ambiguous_metadata_transfer() {
    let mut previous = parsed_inline_tree(r"\text{x}");
    let Some(MathContent::Text(previous_text)) = previous.root.content.first_mut() else {
        panic!("text formula should start with a TextFlow");
    };
    previous_text.source_map[0]
        .extra
        .insert("vendorMapping".to_owned(), serde_json::json!(true));
    let mut updated = parsed_inline_tree(r"\text{x}");
    let Some(MathContent::Text(updated_text)) = updated.root.content.first_mut() else {
        panic!("text formula should start with a TextFlow");
    };
    updated_text
        .source_map
        .push(updated_text.source_map[0].clone());

    assert!(!preserve_math_tree_metadata(&previous, &mut updated));
}

#[test]
fn repeated_structure_insertions_keep_metadata_on_the_original_node() {
    let mut previous = parsed_inline_tree("a+a");
    math_node_mut(&mut previous.root, 0)
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("original"));
    let original_id = math_node(&previous.root, 0).id.clone();
    let mut updated = parsed_inline_tree("a+a+a");

    assert!(preserve_math_tree_metadata_for_edit(
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
    let mut session = Session::new(document).map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::EnterMath(Direction::Backward))?;

    let mut refused = false;
    for _ in 0..5 {
        let generation_before = session.document_generation();
        let document_before = session.document().clone();
        let edit_before = session.math_edit().cloned();
        match session.run(Command::Delete(Direction::Backward)) {
            Ok(_) => {}
            Err(EditError::InvalidMath(_)) => {
                assert_eq!(session.document(), &document_before);
                assert_eq!(session.math_edit(), edit_before.as_ref());
                assert_eq!(session.document_generation(), generation_before);
                refused = true;
                break;
            }
            Err(error) => return Err(error),
        }
    }
    assert!(refused, "metadata-bearing node deletion should be rejected");
    Ok(())
}

#[test]
fn deleting_mapped_math_text_with_mapping_metadata_is_atomic() -> Result<(), EditError> {
    let mut math_tree = parsed_inline_tree(r"\text{x}");
    let Some(MathContent::Text(text)) = math_tree.root.content.first_mut() else {
        panic!("text formula should start with a TextFlow");
    };
    text.source_map[0]
        .extra
        .insert("vendorMapping".to_owned(), serde_json::json!(true));
    text.source_map[0]
        .source_range
        .extra
        .insert("vendorRange".to_owned(), serde_json::json!(true));

    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(ParagraphContent::Math(InlineMath {
        id: NodeId::generate(),
        math_tree,
        extra: std::collections::BTreeMap::new(),
    }));
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session = Session::new(document).map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();

    assert!(matches!(
        session.run(Command::Delete(Direction::Backward)),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.document_generation(), generation_before);
    Ok(())
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
    let metadata_preserved = preserve_align_metadata(
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
    let Block::Align(mut previous) = display_math_block(true, "a+b&b")? else {
        panic!("expected Align block");
    };
    previous.source = r"\begin {aligned}a+b&b\end {aligned}".to_owned();
    let row_id = previous.rows[0].id.clone();
    let tree_id = previous.rows[0].cells[0].root.id.clone();
    previous.rows[0].cells[0]
        .extra
        .insert("futureTreeValue".to_owned(), serde_json::json!("kept"));
    previous.rows[0].cells[0]
        .root
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("kept"));
    let Block::Align(mut updated) = display_math_block_with_id(true, "a+x&b", previous.id.clone())?
    else {
        panic!("expected parsed Align block");
    };
    let old_offset =
        align_source::align_body_range(&previous.source).map_or(0, |range| range.start);
    let new_offset = align_source::align_body_range(&updated.source).map_or(0, |range| range.start);
    preserve_align_metadata(
        &previous,
        &mut updated,
        &(old_offset + 2..old_offset + 3),
        &(new_offset + 2..new_offset + 3),
    );

    assert_eq!(updated.rows[0].id, row_id);
    assert_eq!(updated.rows[0].cells[0].extra["futureTreeValue"], "kept");
    assert_eq!(updated.rows[0].cells[0].root.id, tree_id);
    assert_eq!(
        updated.rows[0].cells[0].root.extra["futureNodeValue"],
        "kept"
    );
    Ok(())
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
    preserve_align_metadata(
        &previous,
        &mut updated,
        &(old_offset + 3..old_offset + 3),
        &(new_offset + 3..new_offset + 5),
    );

    assert_eq!(updated.rows[0].cells[0].root.id, first_id);
    assert_eq!(updated.rows[0].cells[1].root.id, last_id);
    assert_eq!(updated.rows[0].cells[0].extra["origin"], "first");
    assert_eq!(updated.rows[0].cells[1].extra["origin"], "last");
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
    preserve_align_metadata(
        &previous,
        &mut updated,
        &(old_offset..old_offset),
        &(new_offset..new_offset + 2),
    );

    assert_eq!(updated.rows[0].cells[1].root.id, first_id);
    assert_eq!(updated.rows[0].cells[2].root.id, last_id);
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
    let old_offset = aligned_body_offset(&previous);
    let new_offset = aligned_body_offset(&updated);
    preserve_align_metadata(
        &previous,
        &mut updated,
        &(old_offset + 2..old_offset + 2),
        &(new_offset + 2..new_offset + 3),
    );

    assert_eq!(updated.rows[0].cells[1].root.id, cell_id);
    assert_eq!(updated.rows[0].cells[1].extra["futureCellValue"], "keep");
    Ok(())
}

#[test]
fn reparsing_unranged_math_nodes_never_reuses_an_identity_twice() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn preserving_math_metadata_keeps_the_parent_of_an_edited_subtree() {
    let old_source = "\\frac{ab}{c}";
    let new_source = "\\frac{axb}{c}";
    let mut previous = parsed_inline_tree(old_source);
    let (prefix_end, previous_tail, _) = unchanged_source_boundaries(old_source, new_source);
    let Some(changed_parent) = math_node_mut(&mut previous.root, 0)
        .children
        .iter_mut()
        .find(|flow| {
            flow.source_range.as_ref().is_some_and(|range| {
                range.start as usize <= prefix_end && range.end as usize >= previous_tail
            })
        })
    else {
        panic!("fraction operand should contain the edit");
    };
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

use crate::editing::{CommandLineMode, EditorKey};
use caret_doc::{Document, InlineEnvironmentKind, Run};
use std::collections::BTreeMap;

fn parsed_inline_tree(source: &str) -> MathTree {
    match caret_math::parse(source, caret_math::MathInputKind::Inline) {
        Ok(caret_math::ParsedMath::Tree(tree)) => tree,
        Ok(caret_math::ParsedMath::Align(_)) => panic!("expected inline math tree"),
        Err(error) => panic!("test formula should parse: {error}"),
    }
}

fn math_node(flow: &MathFlow, index: usize) -> &MathNode {
    flow.content
        .iter()
        .filter_map(|content| match content {
            MathContent::Math(node) => Some(node),
            _ => None,
        })
        .nth(index)
        .unwrap_or_else(|| panic!("expected math node at index {index}"))
}

fn math_node_mut(flow: &mut MathFlow, index: usize) -> &mut MathNode {
    flow.content
        .iter_mut()
        .filter_map(|content| match content {
            MathContent::Math(node) => Some(node),
            _ => None,
        })
        .nth(index)
        .unwrap_or_else(|| panic!("expected math node at index {index}"))
}

fn session_with_text(text: &str) -> Result<Session, Box<dyn std::error::Error>> {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    if !text.is_empty() {
        paragraph
            .inlines
            .push(ParagraphContent::Text(caret_doc::TextFlow {
                runs: vec![Run {
                    text: text.to_owned(),
                    language: None,
                    props: BTreeMap::new(),
                    extra: BTreeMap::new(),
                }],
                source_range: None,
                source_map: Vec::new(),
                extra: BTreeMap::new(),
            }));
    }
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session = Session::new(document)?;
    if !text.is_empty() {
        for _ in 0..text.chars().count() {
            session.run(Command::MoveCaret(Direction::Forward))?;
        }
    }
    Ok(session)
}

fn insert_equation_text(session: &mut Session, source: &str) -> Result<(), EditError> {
    session.insert_display_math(false)?;
    session.run(Command::InsertText(source.to_owned()))?;
    Ok(())
}

fn move_into_math_text(session: &mut Session) -> Result<(), EditError> {
    session.run(Command::EnterMath(Direction::Backward))?;
    for _ in 0..64 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            return Ok(());
        }
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    Err(EditError::InvalidPosition)
}

fn accept(session: &mut Session, query: &str) -> Result<Effect, EditError> {
    // Exercise internal math editing separately from the disabled production candidates.
    match query {
        "math" => return session.begin_inline_math(),
        "equation" => return session.insert_display_math(false),
        "align" => return session.insert_display_math(true),
        _ => {}
    }
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine(query.to_owned()))?;
    session.run(Command::AcceptCommandLine)
}

fn inline_source(session: &Session) -> Option<String> {
    session
        .document()
        .body
        .iter()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) => paragraph.inlines.iter().find_map(|node| match node {
                ParagraphContent::Math(math) => Some(math.math_tree.source.clone()),
                _ => None,
            }),
            Block::Equation(equation) => Some(equation.math_tree.source.clone()),
            _ => None,
        })
}

#[test]
fn math_command_inserts_inline_math_and_sub_commands_edit_it()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("a")?;
    accept(&mut session, "math")?;
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
fn undo_and_redo_from_math_command_query_restore_formula_editing() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    accept(&mut session, "math").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("frac".to_owned()))?;

    session.run(Command::Undo)?;
    assert!(session.command_line().is_none());
    assert_eq!(session.math_edit().map(|edit| edit.source()), Some(""));
    assert_eq!(inline_source(&session).as_deref(), Some(""));

    session.run(Command::Redo)?;
    assert!(session.command_line().is_none());
    assert_eq!(session.math_edit().map(|edit| edit.source()), Some("x"));
    assert_eq!(inline_source(&session).as_deref(), Some("x"));
    Ok(())
}

#[test]
fn undo_and_redo_restore_formula_target_and_source_cursor() -> Result<(), Box<dyn std::error::Error>>
{
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    let before_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    session.run(Command::InsertText("xy".to_owned()))?;
    let after_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_edit.source(), "xy");
    assert_eq!(after_edit.cursor, 2);

    session.run(Command::Undo)?;
    let after_undo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_undo.target, after_edit.target);
    assert_eq!(after_undo.source(), "");
    assert_eq!(after_undo.cursor, 0);
    assert!(Arc::ptr_eq(
        &after_undo.source_projection,
        &before_edit.source_projection
    ));
    assert_eq!(inline_source(&session).as_deref(), Some(""));

    session.run(Command::Redo)?;
    let after_redo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_redo.target, after_edit.target);
    assert_eq!(after_redo.source(), "xy");
    assert_eq!(after_redo.cursor, 2);
    assert!(Arc::ptr_eq(
        &after_redo.source_projection,
        &after_edit.source_projection
    ));
    assert_eq!(inline_source(&session).as_deref(), Some("xy"));
    Ok(())
}

#[test]
fn restore_math_edit_reuses_matching_projection_and_rejects_stale_source()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let position = history_position(&edit);

    let restored = restore_math_edit(session.document(), position.clone())
        .ok_or(EditError::InvalidPosition)?;
    assert!(Arc::ptr_eq(
        &restored.source_projection,
        &edit.source_projection
    ));

    let mut stale_position = position;
    stale_position.source_projection = Arc::new(
        caret_math::parse_projection("other", caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
            .into_parts()
            .1,
    );
    assert!(restore_math_edit(session.document(), stale_position).is_none());
    Ok(())
}

#[test]
fn reentering_formula_preserves_noncanonical_projection_for_pending_style_undo()
-> Result<(), Box<dyn std::error::Error>> {
    let source = r"\frac {x}{\text{t}}";
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(source.to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert_eq!(session.math_edit().map(MathEdit::source), Some(source));

    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let text = tree
        .root
        .content
        .iter()
        .find_map(first_math_content_text)
        .ok_or(EditError::InvalidPosition)?;
    let start = text
        .source_range
        .as_ref()
        .ok_or(EditError::InvalidPosition)?
        .start as usize;
    let path =
        path::path_at_source_boundary(tree, start, true, None).ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 0);
    session.run(Command::ToggleRunUnderline)?;

    session.exit_math_edit(Direction::Forward, true)?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert_eq!(session.math_edit().map(MathEdit::source), Some(source));

    let before_failed_undo = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    {
        let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
            return Err(EditError::InvalidPosition.into());
        };
        let Some(ParagraphContent::Math(math)) = paragraph.inlines.first_mut() else {
            return Err(EditError::InvalidPosition.into());
        };
        math.math_tree.source = "stale".to_owned();
    }
    assert!(matches!(
        session.run(Command::Undo),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.math_edit(), Some(&before_failed_undo));
    {
        let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
            return Err(EditError::InvalidPosition.into());
        };
        let Some(ParagraphContent::Math(math)) = paragraph.inlines.first_mut() else {
            return Err(EditError::InvalidPosition.into());
        };
        math.math_tree.source = source.to_owned();
    }

    session.run(Command::Undo)?;
    let restored = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(restored.source(), source);
    assert!(matches!(restored.branch, MathEditBranch::Text { .. }));
    assert!(
        session
            .pending_inline_style
            .active_run_properties()
            .next()
            .is_none()
    );
    Ok(())
}

#[test]
fn failed_math_history_restore_rolls_back_document_patch_and_keeps_history_position()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(session.math_edit().map(MathEdit::source), Some(""));
    {
        let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
            return Err(EditError::InvalidPosition.into());
        };
        let Some(ParagraphContent::Math(math)) = paragraph.inlines.first_mut() else {
            return Err(EditError::InvalidPosition.into());
        };
        math.math_tree.source = "stale".to_owned();
    }
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
fn reopening_a_canonical_projection_does_not_break_formula_creation_undo()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("a + b".to_owned()))?;
    assert!(session.math_edit().is_none());
    session.run(Command::EnterMath(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.source()), Some("a + b"));

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
fn undo_and_redo_restore_equation_and_align_edit_targets() -> Result<(), Box<dyn std::error::Error>>
{
    for (query, align) in [("equation", false), ("align", true)] {
        let mut session = session_with_text("")?;
        accept(&mut session, query)?;
        let before_edit = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        session.run(Command::InsertText("x".to_owned()))?;
        let after_edit = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        assert!(matches!(
            after_edit.target,
            MathEditTarget::Block { align: value, .. } if value == align
        ));

        session.run(Command::Undo)?;
        let after_undo = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert_eq!(after_undo.target, after_edit.target);
        assert_eq!(after_undo.cursor, 0);
        assert!(Arc::ptr_eq(
            &after_undo.source_projection,
            &before_edit.source_projection
        ));

        session.run(Command::Redo)?;
        let after_redo = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert_eq!(after_redo.target, after_edit.target);
        assert_eq!(after_redo.source(), "x");
        assert_eq!(after_redo.cursor, 1);
        assert!(Arc::ptr_eq(
            &after_redo.source_projection,
            &after_edit.source_projection
        ));
    }
    Ok(())
}

#[test]
fn undo_and_redo_restore_align_cursor_in_unicode_content_coordinates()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    let source = "αabcdefghijklmn";
    session.run(Command::InsertText(source.to_owned()))?;
    assert_eq!(session.math_edit().map(|edit| edit.source()), Some(source));
    assert_eq!(
        session.math_edit().map(|edit| edit.cursor),
        Some(source.len())
    );

    session.run(Command::Undo)?;
    let after_undo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_undo.source(), "");
    assert_eq!(after_undo.cursor, 0);

    session.run(Command::Redo)?;
    let after_redo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_redo.source(), source);
    assert_eq!(after_redo.cursor, source.len());
    assert!(after_redo.source_projection.align().is_some());
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

        session.run(Command::Undo)?;
        assert!(session.math_edit().is_none());
        session.run(Command::Redo)?;
        let restored = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert_eq!(restored.target, created.target);
        assert_eq!(restored.source(), created.source());
        assert_eq!(restored.cursor, created.cursor);
    }
    Ok(())
}

#[test]
fn entering_fenced_inline_math_edits_inside_the_fences_from_both_sides()
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
    assert_eq!(from_left.math_edit().map(|edit| edit.cursor), Some(1));
    from_left.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(inline_source(&from_left).as_deref(), Some("$yx$"));

    let mut from_right = session_with_text("")?;
    from_right.run(Command::InsertInlineMath(r"\(x\)".to_owned()))?;
    from_right.run(Command::EnterMath(Direction::Backward))?;
    assert_eq!(from_right.math_edit().map(|edit| edit.cursor), Some(3));
    from_right.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(inline_source(&from_right).as_deref(), Some(r"\(xy\)"));
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
    assert_eq!(from_left.math_edit().map(|edit| edit.cursor), Some(2));
    from_left.run(Command::MoveCaret(Direction::Backward))?;
    assert!(from_left.math_edit().is_none());
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
            source_range: None,
            source_map: Vec::new(),
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
fn backspace_deletes_an_empty_fenced_inline_formula() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
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
fn backspace_removes_empty_construct_and_then_the_object() -> Result<(), Box<dyn std::error::Error>>
{
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
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
fn greek_sub_commands_are_case_sensitive() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    accept(&mut session, "Gamma")?;
    accept(&mut session, "gamma")?;
    assert_eq!(inline_source(&session).as_deref(), Some("\\Gamma\\gamma"));
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
    assert_eq!(equation.math_tree.source, "a+b");
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
fn edited_math_documents_remain_read_only_until_layout_support_is_enabled()
-> Result<(), Box<dyn std::error::Error>> {
    let mut inline = session_with_text("")?;
    accept(&mut inline, "math")?;
    inline.run(Command::InsertText("x".to_owned()))?;
    assert_math_document_is_not_yet_saveable(inline.document());

    let mut equation = session_with_text("")?;
    accept(&mut equation, "equation")?;
    equation.run(Command::InsertText("x+y".to_owned()))?;
    assert_math_document_is_not_yet_saveable(equation.document());

    let mut align = session_with_text("")?;
    accept(&mut align, "align")?;
    align.run(Command::InsertText("x&=y".to_owned()))?;
    assert_math_document_is_not_yet_saveable(align.document());
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
fn align_enter_inserts_row_break() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
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
    let source_map = edit
        .source_projection
        .align()
        .ok_or(EditError::InvalidPosition)?;
    let content_range = source_map.content_range();
    assert_eq!(
        &edit.source_projection.source()[content_range],
        edit.source()
    );
    assert_eq!(edit.source(), "a&=b \\\\ c&=d");
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
        session.run(Command::InsertText(first.to_owned()))?;
        assert_eq!(session.math_edit().map(|edit| edit.source()), Some(first));
        assert_eq!(
            session.math_edit().map(|edit| edit.cursor),
            Some(first.len())
        );

        session.run(Command::InsertText(next.to_owned()))?;
        assert_eq!(
            session.math_edit().map(|edit| edit.source()),
            Some(expected)
        );
        assert_eq!(
            session.math_edit().map(|edit| edit.cursor),
            Some(expected.len())
        );
    }
    Ok(())
}

#[test]
fn invalid_source_is_rejected_without_changes() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    session.run(Command::InsertText("x".to_owned()))?;
    let result = session.run(Command::InsertText("\\".to_owned()));
    assert!(matches!(result, Err(EditError::InvalidMath(_))));
    assert_eq!(inline_source(&session).as_deref(), Some("x"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
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

    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: true, .. })
    ));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));

    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: false, .. })
    ));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    Ok(())
}

#[test]
fn leaving_last_display_math_without_a_paragraph_keeps_formula_focus()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        display_math_block(false, "a")?,
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    let target = session.math_edit().cloned();
    let selection = session.selection().clone();

    session.run(Command::MoveCaret(Direction::Forward))?;

    assert_eq!(session.math_edit(), target.as_ref());
    assert_eq!(session.selection(), &selection);
    Ok(())
}

#[test]
fn escape_leaves_last_equation_and_align_at_the_nearest_body_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    for (align, source) in [(false, "a"), (true, "a&b")] {
        let mut document = caret_doc::Document::blank();
        document.body = vec![
            Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
            display_math_block(align, source)?,
        ];
        let mut session = Session::new(document)?;
        let paragraph_id = match session.document().body.first() {
            Some(Block::Paragraph(paragraph)) => paragraph.id.clone(),
            _ => panic!("document should start with a paragraph"),
        };
        session.run(Command::EnterMath(Direction::Forward))?;

        session.run(Command::EditorKey(EditorKey::Escape))?;

        assert!(session.math_edit().is_none());
        assert_eq!(session.selection().focus.paragraph_id, paragraph_id);
        assert_eq!(session.selection().focus.offset, 0);
    }
    Ok(())
}

#[test]
fn escape_skips_adjacent_formula_blocks_and_updates_paragraph_affinity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        display_math_block(false, "a")?,
        display_math_block(true, "b&c")?,
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    session.run(Command::EditorKey(EditorKey::Escape))?;

    let Some(Block::Paragraph(after)) = session.document().body.last() else {
        panic!("document should end with a paragraph");
    };
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.paragraph_id, after.id);
    assert_eq!(session.selection().focus.affinity, Affinity::Upstream);
    assert_eq!(
        session.selection_visual_affinity(),
        crate::VisualAffinity::Upstream
    );
    Ok(())
}

#[test]
fn escape_uses_the_opposite_side_only_when_the_preferred_side_has_no_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let before = Paragraph::empty(NodeId::generate());
    let after = Paragraph::empty(NodeId::generate());
    let after_id = after.id.clone();
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        display_math_block(false, "x")?,
        Block::Paragraph(after),
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    let Block::Paragraph(before) = &mut session.document.body[0] else {
        panic!("document should start with the paragraph");
    };
    before.inlines = vec![ParagraphContent::Opaque(caret_doc::OpaqueNode {
        raw: serde_json::json!({"type": "future_inline", "payload": "preserve"}),
    })];

    session.run(Command::EditorKey(EditorKey::Escape))?;

    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.paragraph_id, after_id);
    Ok(())
}

#[test]
fn leaving_and_reentering_inline_math_splits_typing_undo_groups()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("xy"));

    session.run(Command::Undo)?;

    assert_eq!(inline_source(&session).as_deref(), Some("x"));
    assert_eq!(session.math_edit().map(|edit| edit.source()), Some("x"));
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
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.source()), Some(""));
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
    session.run(Command::Redo)?;
    assert_eq!(session.document().body.len(), 2);
    assert!(session.math_edit().is_none());
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
