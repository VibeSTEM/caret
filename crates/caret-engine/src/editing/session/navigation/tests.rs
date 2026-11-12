use super::super::super::{
    Affinity, ClipboardAction, Command, Direction, EditorKey, EnvironmentKind, MathEditBranch,
    NavigationCandidate, NavigationGeneration, Pos, Selection, Session, StoryId,
};
use caret_doc::{
    Block, Document, InlineEnvironment, InlineEnvironmentKind, NodeId, ParagraphContent, Run,
    TextFlow,
};
use std::collections::BTreeMap;

#[test]
fn editor_key_prioritizes_command_line_over_coexisting_math_edit() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session initialization failed: {error}"));
    session
        .run(Command::InsertInlineMath("x".to_owned()))
        .unwrap_or_else(|error| panic!("inline formula insertion failed: {error}"));
    session
        .run(Command::EnterMath(Direction::Backward))
        .unwrap_or_else(|error| panic!("formula focus failed: {error}"));
    assert!(session.math_edit().is_some());
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    assert!(session.math_edit().is_some());
    assert!(session.command_line().is_some());
    session
        .run(Command::TypeCommandLine("abc".to_owned()))
        .unwrap_or_else(|error| panic!("command line query failed: {error}"));
    let document_selection = session.selection().clone();
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(NavigationCandidate::Document {
                focus: document_selection.focus.clone(),
                visual_affinity: caret_layout::VisualAffinity::Downstream,
                generation: NavigationGeneration {
                    session_id: session.session_id().clone(),
                    document_generation: session.document_generation(),
                    font_generation: 0,
                },
            }),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("left arrow routing failed: {error}"));
    assert_eq!(session.command_line().map(|line| line.cursor), Some(2));
    assert_eq!(session.selection(), &document_selection);

    let document_before_undo_shortcut = session.document().clone();
    let command_line_before_undo_shortcut = session.command_line().cloned();
    session
        .run(Command::EditorKey(EditorKey::ModZ { redo: false }))
        .unwrap_or_else(|error| panic!("command-line undo shortcut failed: {error}"));
    assert_eq!(session.document(), &document_before_undo_shortcut);
    assert_eq!(
        session.command_line().cloned(),
        command_line_before_undo_shortcut
    );

    let stale_candidate = Some(NavigationCandidate::Document {
        focus: document_selection.focus.clone(),
        visual_affinity: caret_layout::VisualAffinity::Downstream,
        generation: NavigationGeneration {
            session_id: session.session_id().clone(),
            document_generation: session.document_generation().saturating_add(1),
            font_generation: 0,
        },
    });
    for key in [
        EditorKey::RightArrow {
            candidate: stale_candidate.clone(),
            extend: false,
        },
        EditorKey::Home {
            candidate: stale_candidate.clone(),
            extend: false,
        },
        EditorKey::End {
            candidate: stale_candidate,
            extend: false,
        },
    ] {
        session
            .run(Command::EditorKey(key))
            .unwrap_or_else(|error| panic!("command line must own the navigation key: {error}"));
        assert_eq!(session.selection(), &document_selection);
    }
    assert_eq!(session.command_line().map(|line| line.cursor), Some(3));

    let document_before_backspace = session.document().clone();
    session
        .run(Command::EditorKey(EditorKey::Backspace))
        .unwrap_or_else(|error| panic!("command-line Backspace failed: {error}"));
    assert_eq!(session.document(), &document_before_backspace);
    assert!(session.math_edit().is_some());
    assert_eq!(
        session.command_line().map(|line| line.query.as_str()),
        Some("ab")
    );
    let command_line_after_backspace = session.command_line().cloned();
    session
        .run(Command::EditorKey(EditorKey::Delete))
        .unwrap_or_else(|error| panic!("command-line Delete failed: {error}"));
    assert_eq!(session.document(), &document_before_backspace);
    assert_eq!(
        session.command_line().cloned(),
        command_line_after_backspace
    );
}

/// 在行内公式的 MathText 文本岛内部创建可扩展的公式会话。
fn session_in_inline_math_text_island() -> Session {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session initialization failed: {error}"));
    session
        .run(Command::InsertInlineMath(r"\frac{1}{\text{ab}}".to_owned()))
        .unwrap_or_else(|error| panic!("inline formula insertion failed: {error}"));
    session
        .run(Command::EnterMath(Direction::Backward))
        .unwrap_or_else(|error| panic!("formula focus failed: {error}"));
    for _ in 0..16 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            break;
        }
        session
            .run(Command::MoveCaret(Direction::Backward))
            .unwrap_or_else(|error| panic!("math caret movement failed: {error}"));
    }
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session
}

/// 在行内公式的根级 MathTree 内部创建可扩展的公式会话（无根级文本岛）。
fn session_in_inline_math_tree(source: &str) -> Session {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session initialization failed: {error}"));
    session
        .run(Command::InsertInlineMath(source.to_owned()))
        .unwrap_or_else(|error| panic!("inline formula insertion failed: {error}"));
    session
        .run(Command::EnterMath(Direction::Backward))
        .unwrap_or_else(|error| panic!("formula focus failed: {error}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));
    session
}

#[test]
fn shift_right_at_inline_math_tree_end_uses_a_plain_object_text_selection() {
    let mut session = session_in_inline_math_tree("x+y");

    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-right at tree end failed: {error}"));

    // InlineMath 只是段落文本里的一个 U+FFFC 对象字符：越界后写普通文本选区，
    // 不安装覆盖不到任何块/公式块的空结构范围。
    assert!(session.math_edit().is_none());
    assert!(!session.has_structure_selection());
    assert!(session.structure_selection_range().is_none());
    let selection = session.selection().clone();
    // U+FFFC 占三个 UTF-8 字节；正向范围从对象前侧到对象后侧。
    assert_eq!(selection.anchor.offset, 0);
    assert_eq!(selection.anchor.affinity, Affinity::Downstream);
    assert_eq!(selection.focus.offset, 3);
    assert_eq!(selection.focus.affinity, Affinity::Upstream);
    let text = session
        .selected_text()
        .unwrap_or_else(|error| panic!("selected text failed: {error}"));
    assert!(
        text.contains('\u{FFFC}'),
        "inline math conversion should keep the placeholder: {text:?}"
    );

    // 下一次 Shift 继续按普通文档文本扩选，不报 InvalidPosition。
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-right after conversion failed: {error}"));
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().anchor.offset, 0);
    assert_eq!(session.selection().focus.offset, 3);
}

#[test]
fn shift_left_at_inline_math_tree_start_uses_a_plain_object_text_selection() {
    let mut session = session_in_inline_math_tree("x+y");
    for _ in 0..3 {
        session
            .run(Command::MoveCaret(Direction::Backward))
            .unwrap_or_else(|error| panic!("math caret movement failed: {error}"));
    }
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));

    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-left at tree start failed: {error}"));

    assert!(session.math_edit().is_none());
    assert!(!session.has_structure_selection());
    assert!(session.structure_selection_range().is_none());
    let selection = session.selection().clone();
    assert_eq!(selection.anchor.offset, 3);
    assert_eq!(selection.anchor.affinity, Affinity::Upstream);
    assert_eq!(selection.focus.offset, 0);
    assert_eq!(selection.focus.affinity, Affinity::Downstream);
    let text = session
        .selected_text()
        .unwrap_or_else(|error| panic!("selected text failed: {error}"));
    assert!(
        text.contains('\u{FFFC}'),
        "inline math conversion should keep the placeholder: {text:?}"
    );

    // 下一次 Shift+Left 继续按普通文档文本扩选，不报 InvalidPosition。
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-left after conversion failed: {error}"));
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().anchor.offset, 3);
    assert_eq!(session.selection().focus.offset, 0);
}

#[test]
fn shift_left_at_nested_math_text_island_converts_the_inline_host_range() {
    let mut session = session_in_inline_math_text_island();
    session
        .run(Command::SetMathTextSelection {
            anchor: 0,
            focus: 0,
        })
        .unwrap_or_else(|error| panic!("math text selection failed: {error}"));

    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-left at text edge failed: {error}"));

    // 文本岛内没有同方向后续，离开当前层即转为整个 InlineMath 的普通文本范围；
    // 光标状态不保留已累积的锚点，也不暴露空覆盖结构选区。
    assert!(session.math_edit().is_none());
    assert!(!session.has_structure_selection());
    assert!(session.structure_selection_range().is_none());
    assert_eq!(session.selection().anchor.offset, 3);
    assert_eq!(session.selection().focus.offset, 0);
}

#[test]
fn shift_right_at_nested_math_text_island_converts_the_inline_host_range() {
    let mut session = session_in_inline_math_text_island();
    session
        .run(Command::SetMathTextSelection {
            anchor: 2,
            focus: 2,
        })
        .unwrap_or_else(|error| panic!("math text selection failed: {error}"));

    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-right at text edge failed: {error}"));

    assert!(session.math_edit().is_none());
    assert!(!session.has_structure_selection());
    assert!(session.structure_selection_range().is_none());
    assert_eq!(session.selection().anchor.offset, 0);
    assert_eq!(session.selection().focus.offset, 3);
}

#[test]
fn horizontal_navigation_enters_and_leaves_both_inline_environment_edges() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session initialization failed: {error}"));
    session
        .run(Command::InsertText("ab".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    let paragraph_id = session.selection().focus.paragraph_id.clone();
    let story = session.selection().focus.story.clone();
    let position = |offset, affinity| Pos {
        story: story.clone(),
        paragraph_id: paragraph_id.clone(),
        offset,
        affinity,
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: position(0, Affinity::Downstream),
            focus: position(1, Affinity::Upstream),
        })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));
    session
        .run(Command::ToggleInlineFormat(
            caret_doc::InlineEnvironmentKind::Bold,
        ))
        .unwrap_or_else(|error| panic!("inline format failed: {error}"));

    session
        .run(Command::SetSelection(Some(Selection {
            anchor: position(0, Affinity::Upstream),
            focus: position(0, Affinity::Upstream),
        })))
        .unwrap_or_else(|error| panic!("start boundary setup failed: {error}"));
    session
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| panic!("entering the inline start failed: {error}"));
    assert_eq!(session.selection().focus, position(0, Affinity::Downstream));
    assert!(
        session
            .current_environment_view()
            .unwrap_or_default()
            .inline()
            .iter()
            .any(|entry| entry.kind == EnvironmentKind::Bold)
    );

    session
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| panic!("moving inside the inline environment failed: {error}"));
    assert_eq!(session.selection().focus, position(1, Affinity::Upstream));
    session
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| panic!("leaving the inline end failed: {error}"));
    assert_eq!(session.selection().focus, position(1, Affinity::Downstream));

    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("entering the inline end failed: {error}"));
    assert_eq!(session.selection().focus, position(1, Affinity::Upstream));
    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| {
            panic!("moving backward inside the inline environment failed: {error}")
        });
    assert_eq!(session.selection().focus, position(0, Affinity::Downstream));
    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("leaving the inline start failed: {error}"));
    assert_eq!(session.selection().focus, position(0, Affinity::Upstream));
}

#[test]
fn empty_inline_environment_has_one_editable_internal_caret_stop() {
    let mut document = Document::blank();
    let environment_id = NodeId::generate();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines = vec![ParagraphContent::Environment(InlineEnvironment {
        id: environment_id.clone(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::from([("enabled".to_owned(), serde_json::Value::Bool(true))]),
        children: vec![ParagraphContent::Text(TextFlow {
            runs: vec![Run {
                text: String::new(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    })];
    let mut session = Session::new(document.clone())
        .unwrap_or_else(|error| panic!("empty environment session failed: {error}"));
    let mut outside = session.selection().focus.clone();
    outside.affinity = Affinity::Upstream;
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: outside.clone(),
            focus: outside,
        })))
        .unwrap_or_else(|error| panic!("outside caret setup failed: {error}"));

    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("entering empty environment failed: {error}"));
    assert_eq!(session.selection().focus.offset, 0);
    let visual_affinity = match session.selection_visual_affinity() {
        caret_layout::VisualAffinity::Upstream => caret_layout::VisualAffinity::Downstream,
        caret_layout::VisualAffinity::Downstream => caret_layout::VisualAffinity::Upstream,
    };
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(NavigationCandidate::Document {
                focus: session.selection().focus.clone(),
                visual_affinity,
                generation: NavigationGeneration {
                    session_id: session.session_id().clone(),
                    document_generation: session.document_generation(),
                    font_generation: 0,
                },
            }),
            extend: false,
        }))
        .unwrap_or_else(|error| {
            panic!("visual caret switch inside empty environment failed: {error}")
        });
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(crate::editing::HostCaretPosition::Inside {
            host_id,
            kind: crate::editing::HostCaretKind::Inline,
            ..
        }) if host_id == &environment_id
    ));
    assert_eq!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("empty inline environment lookup failed: {error}"))
            .unwrap_or_else(|| panic!("empty inline environment was not active"))
            .environment_id,
        environment_id
    );
    assert!(matches!(
        session.active_host_caret_position(),
        Some(crate::editing::HostCaretPosition::Inside {
            local: crate::editing::HostCaretLocalPosition::Paragraph(_),
            ..
        })
    ));
    assert_eq!(
        session
            .current_environment_view()
            .unwrap_or_else(|error| panic!("environment view lookup failed: {error}"))
            .inline()
            .len(),
        1
    );
    let document_before_preedit = session.document().clone();
    session
        .run(Command::UpdateComposition {
            id: 94,
            preedit: "ni".to_owned(),
        })
        .unwrap_or_else(|error| panic!("empty inline preedit failed: {error}"));
    session
        .run(Command::CancelComposition { id: 94 })
        .unwrap_or_else(|error| panic!("empty inline preedit cancellation failed: {error}"));
    assert_eq!(session.document(), &document_before_preedit);
    assert_eq!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!(
                "inline environment after preedit cancel failed: {error}"
            ))
            .unwrap_or_else(|| panic!("preedit cancellation lost empty inline focus"))
            .environment_id,
        environment_id
    );

    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| {
            panic!("leaving toward the empty environment's before side failed: {error}")
        });
    assert_eq!(session.selection().focus.affinity, Affinity::Upstream);
    assert!(
        session
            .current_environment_view()
            .unwrap_or_else(|error| panic!("environment view lookup failed: {error}"))
            .inline()
            .is_empty()
    );
    let outside_visual_affinity = match session.selection_visual_affinity() {
        caret_layout::VisualAffinity::Upstream => caret_layout::VisualAffinity::Downstream,
        caret_layout::VisualAffinity::Downstream => caret_layout::VisualAffinity::Upstream,
    };
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(NavigationCandidate::Document {
                focus: session.selection().focus.clone(),
                visual_affinity: outside_visual_affinity,
                generation: NavigationGeneration {
                    session_id: session.session_id().clone(),
                    document_generation: session.document_generation(),
                    font_generation: 0,
                },
            }),
            extend: false,
        }))
        .unwrap_or_else(|error| {
            panic!("visual caret switch outside empty environment failed: {error}")
        });
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(crate::editing::HostCaretPosition::Before {
            kind: crate::editing::HostCaretKind::Inline,
            ..
        })
    ));
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("leaving the empty host at document start failed: {error}"));
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(crate::editing::HostCaretPosition::Before {
            kind: crate::editing::HostCaretKind::Inline,
            ..
        })
    ));
    let before_edge_position = session.selection().clone();
    let before_edge_host = session.host_caret_position.clone();
    let before_edge_history = session.history_state_id();
    let before_edge_merge_epoch = session.history_merge_epoch;
    let before_edge_effect = session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| {
            panic!("moving before the inline host at document start failed: {error}")
        });
    assert!(!before_edge_effect.selection_changed && !before_edge_effect.document_changed);
    assert_eq!(session.selection(), &before_edge_position);
    assert_eq!(session.host_caret_position, before_edge_host);
    assert_eq!(session.history_state_id(), before_edge_history);
    assert_eq!(session.history_merge_epoch, before_edge_merge_epoch);
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("re-entering the empty environment failed: {error}"));
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| {
            panic!("leaving toward the empty environment's after side failed: {error}")
        });
    assert_eq!(session.selection().focus.affinity, Affinity::Downstream);
    assert!(
        session
            .current_environment_view()
            .unwrap_or_else(|error| panic!("environment view lookup failed: {error}"))
            .inline()
            .is_empty()
    );
    let after_edge_position = session.selection().clone();
    let after_edge_host = session.host_caret_position.clone();
    let after_edge_history = session.history_state_id();
    let after_edge_merge_epoch = session.history_merge_epoch;
    let after_edge_effect = session
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| {
            panic!("moving after the inline host at document end failed: {error}")
        });
    assert!(!after_edge_effect.selection_changed && !after_edge_effect.document_changed);
    assert_eq!(session.selection(), &after_edge_position);
    assert_eq!(session.host_caret_position, after_edge_host);
    assert_eq!(session.history_state_id(), after_edge_history);
    assert_eq!(session.history_merge_epoch, after_edge_merge_epoch);
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| {
            panic!("entering from the empty environment's after side failed: {error}")
        });
    assert_eq!(
        session
            .current_environment_view()
            .unwrap_or_else(|error| panic!("environment view lookup failed: {error}"))
            .inline()
            .len(),
        1
    );

    session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("typing into empty environment failed: {error}"));
    assert!(
        session
            .current_environment_view()
            .unwrap_or_else(|error| panic!("environment view lookup failed: {error}"))
            .inline()
            .iter()
            .any(|entry| {
                entry.source == crate::editing::EnvironmentSource::InlineEnvironment
                    && entry.id.as_ref() == Some(&environment_id)
            })
    );
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo after typing into empty inline failed: {error}"));
    assert!(
        session
            .current_environment_view()
            .unwrap_or_else(|error| panic!("environment view lookup failed: {error}"))
            .inline()
            .iter()
            .any(|entry| {
                entry.source == crate::editing::EnvironmentSource::InlineEnvironment
                    && entry.id.as_ref() == Some(&environment_id)
            })
    );
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo after typing into empty inline failed: {error}"));
}

#[test]
fn empty_inline_host_does_not_swallow_invalid_or_visual_only_candidates() {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines = vec![ParagraphContent::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::new(),
        children: vec![ParagraphContent::Text(TextFlow {
            runs: vec![Run {
                text: String::new(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    })];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("empty environment session failed: {error}"));
    let before = session.selection().clone();
    let generation = NavigationGeneration {
        session_id: session.session_id().clone(),
        document_generation: session.document_generation(),
        font_generation: 0,
    };
    let invalid = NavigationCandidate::Document {
        focus: Pos {
            offset: 1,
            ..before.focus.clone()
        },
        visual_affinity: session.selection_visual_affinity(),
        generation: generation.clone(),
    };
    assert_eq!(
        session.run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(invalid),
            extend: false,
        })),
        Err(crate::editing::EditError::InvalidPosition)
    );
    assert_eq!(session.selection(), &before);
    assert!(session.host_caret_position.is_none());

    let alternate_visual_affinity = match session.selection_visual_affinity() {
        caret_layout::VisualAffinity::Upstream => caret_layout::VisualAffinity::Downstream,
        caret_layout::VisualAffinity::Downstream => caret_layout::VisualAffinity::Upstream,
    };
    let alternate = NavigationCandidate::Document {
        focus: before.focus,
        visual_affinity: alternate_visual_affinity,
        generation,
    };
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(alternate),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("visual-only candidate was rejected: {error}"));
    assert_eq!(
        session.selection_visual_affinity(),
        alternate_visual_affinity
    );
    assert!(session.host_caret_position.is_none());
}

#[test]
fn typing_at_empty_inline_environment_between_text_stays_inside_its_host() {
    let mut document = Document::blank();
    let environment_id = NodeId::generate();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    let run = |text: &str| {
        ParagraphContent::Text(TextFlow {
            runs: vec![Run {
                text: text.to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        })
    };
    paragraph.inlines = vec![
        run("a"),
        ParagraphContent::Environment(InlineEnvironment {
            id: environment_id.clone(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: BTreeMap::from([("enabled".to_owned(), serde_json::Value::Bool(true))]),
            children: Vec::new(),
            extra: BTreeMap::new(),
        }),
        run("b"),
    ];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("adjacent empty environment session failed: {error}"));
    let mut before_environment = session.selection().focus.clone();
    before_environment.offset = 1;
    before_environment.affinity = Affinity::Upstream;
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: before_environment.clone(),
            focus: before_environment,
        })))
        .unwrap_or_else(|error| panic!("before-environment caret setup failed: {error}"));
    let focus_before_stale_candidate = session.selection().clone();
    let stale_candidate = NavigationCandidate::Document {
        focus: focus_before_stale_candidate.focus.clone(),
        visual_affinity: caret_layout::VisualAffinity::Downstream,
        generation: NavigationGeneration {
            session_id: session.session_id().clone(),
            document_generation: session.document_generation().saturating_add(1),
            font_generation: 0,
        },
    };
    assert!(
        session
            .run(Command::EditorKey(EditorKey::RightArrow {
                candidate: Some(stale_candidate),
                extend: false,
            }))
            .is_err()
    );
    assert_eq!(session.selection(), &focus_before_stale_candidate);
    let mut visual_target = focus_before_stale_candidate.focus.clone();
    visual_target.offset = 2;
    visual_target.affinity = Affinity::Downstream;
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(NavigationCandidate::Document {
                focus: visual_target,
                visual_affinity: caret_layout::VisualAffinity::Downstream,
                generation: NavigationGeneration {
                    session_id: session.session_id().clone(),
                    document_generation: session.document_generation(),
                    font_generation: 0,
                },
            }),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("visual candidate skipped the empty environment: {error}"));
    assert_eq!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("empty inline environment lookup failed: {error}"))
            .unwrap_or_else(|| panic!("visual navigation did not enter the inline environment"))
            .environment_id,
        environment_id
    );
    session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("typing inside empty environment failed: {error}"));

    let Block::Paragraph(paragraph) = &session.document().body[0] else {
        panic!("document paragraph unexpectedly changed type");
    };
    let Some(ParagraphContent::Environment(environment)) = paragraph.inlines.get(1) else {
        panic!("empty inline environment was not preserved");
    };
    assert_eq!(environment.id, environment_id);
    assert!(
        matches!(environment.children.first(), Some(ParagraphContent::Text(flow)) if flow.runs.iter().map(|run| run.text.as_str()).collect::<String>() == "x")
    );
    assert!(
        matches!(paragraph.inlines.first(), Some(ParagraphContent::Text(flow)) if flow.runs.iter().map(|run| run.text.as_str()).collect::<String>() == "a")
    );
    assert!(
        matches!(paragraph.inlines.get(2), Some(ParagraphContent::Text(flow)) if flow.runs.iter().map(|run| run.text.as_str()).collect::<String>() == "b")
    );
}

#[test]
fn empty_sibling_inline_hosts_are_reached_from_both_directions() {
    let first_id = NodeId::generate();
    let second_id = NodeId::generate();
    let empty_environment = |id: NodeId, kind| {
        ParagraphContent::Environment(InlineEnvironment {
            id,
            kind,
            definition_id: None,
            props: BTreeMap::from([("enabled".to_owned(), serde_json::Value::Bool(true))]),
            children: Vec::new(),
            extra: BTreeMap::new(),
        })
    };
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines = vec![
        empty_environment(first_id.clone(), InlineEnvironmentKind::Bold),
        empty_environment(second_id.clone(), InlineEnvironmentKind::Italic),
    ];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("sibling empty environment session failed: {error}"));
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("entering right sibling from the right failed: {error}"));
    assert_eq!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("right sibling lookup failed: {error}"))
            .unwrap_or_else(|| panic!("right sibling did not become active"))
            .environment_id,
        second_id
    );
    session
        .run(Command::InsertText("r".to_owned()))
        .unwrap_or_else(|error| panic!("typing into right sibling failed: {error}"));

    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines = vec![
        empty_environment(first_id.clone(), InlineEnvironmentKind::Bold),
        empty_environment(second_id.clone(), InlineEnvironmentKind::Italic),
    ];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("sibling empty environment session failed: {error}"));
    let mut before = session.selection().focus.clone();
    before.affinity = Affinity::Upstream;
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: before.clone(),
            focus: before,
        })))
        .unwrap_or_else(|error| panic!("left sibling boundary setup failed: {error}"));
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("entering left sibling from the left failed: {error}"));
    assert_eq!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("left sibling lookup failed: {error}"))
            .unwrap_or_else(|| panic!("left sibling did not become active"))
            .environment_id,
        first_id
    );
    session
        .run(Command::InsertText("l".to_owned()))
        .unwrap_or_else(|error| panic!("typing into left sibling failed: {error}"));
}

#[test]
fn empty_inline_host_identity_survives_three_sibling_navigation_and_history() {
    let host_ids = [NodeId::generate(), NodeId::generate(), NodeId::generate()];
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines = host_ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            ParagraphContent::Environment(InlineEnvironment {
                id: id.clone(),
                kind: [
                    InlineEnvironmentKind::Bold,
                    InlineEnvironmentKind::Italic,
                    InlineEnvironmentKind::Font,
                ][index],
                definition_id: None,
                props: BTreeMap::from([("enabled".to_owned(), serde_json::Value::Bool(true))]),
                children: Vec::new(),
                extra: BTreeMap::new(),
            })
        })
        .collect();
    let mut projection_session = Session::new(document.clone())
        .unwrap_or_else(|error| panic!("empty sibling projection session failed: {error}"));
    assert!(matches!(
        projection_session.active_host_caret_position(),
        Some(crate::editing::HostCaretPosition::After {
            host_id,
            kind: crate::editing::HostCaretKind::Inline,
        }) if host_id == host_ids[2]
    ));
    let mut before = projection_session.selection().focus.clone();
    before.affinity = Affinity::Upstream;
    projection_session
        .run(Command::SetSelection(Some(Selection {
            anchor: before.clone(),
            focus: before,
        })))
        .unwrap_or_else(|error| panic!("empty sibling leading boundary setup failed: {error}"));
    assert!(matches!(
        projection_session.active_host_caret_position(),
        Some(crate::editing::HostCaretPosition::Before {
            host_id,
            kind: crate::editing::HostCaretKind::Inline,
        }) if host_id == host_ids[0]
    ));
    let mut session = Session::new(document.clone())
        .unwrap_or_else(|error| panic!("empty sibling session failed: {error}"));

    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("entering the last empty sibling failed: {error}"));
    assert_eq!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("last sibling lookup failed: {error}"))
            .unwrap_or_else(|| panic!("last sibling did not become active"))
            .environment_id,
        host_ids[2]
    );
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("leaving the last empty sibling failed: {error}"));
    assert!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("outside sibling lookup failed: {error}"))
            .is_none()
    );
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("entering the middle empty sibling failed: {error}"));
    assert_eq!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("middle sibling lookup failed: {error}"))
            .unwrap_or_else(|| panic!("middle sibling did not become active"))
            .environment_id,
        host_ids[1]
    );
    session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("typing into the middle sibling failed: {error}"));
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo in the middle sibling failed: {error}"));
    assert_eq!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("undo sibling lookup failed: {error}"))
            .unwrap_or_else(|| panic!("undo lost the active sibling"))
            .environment_id,
        host_ids[1]
    );
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo in the middle sibling failed: {error}"));
    assert_eq!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("redo sibling lookup failed: {error}"))
            .unwrap_or_else(|| panic!("redo lost the active sibling"))
            .environment_id,
        host_ids[1]
    );

    let mut forward_session = Session::new(document)
        .unwrap_or_else(|error| panic!("forward sibling session failed: {error}"));
    let mut before = forward_session.selection().focus.clone();
    before.affinity = Affinity::Upstream;
    forward_session
        .run(Command::SetSelection(Some(Selection {
            anchor: before.clone(),
            focus: before,
        })))
        .unwrap_or_else(|error| panic!("left sibling boundary setup failed: {error}"));
    for _ in 0..3 {
        forward_session
            .run(Command::EditorKey(EditorKey::RightArrow {
                candidate: None,
                extend: false,
            }))
            .unwrap_or_else(|error| panic!("moving to the middle sibling failed: {error}"));
    }
    for text in ["x", "y"] {
        forward_session
            .run(Command::InsertText(text.to_owned()))
            .unwrap_or_else(|error| panic!("consecutive middle sibling input failed: {error}"));
        assert_eq!(
            forward_session
                .current_inline_environment()
                .unwrap_or_else(|error| panic!("middle sibling lookup failed: {error}"))
                .unwrap_or_else(|| panic!("typing left the middle sibling"))
                .environment_id,
            host_ids[1]
        );
    }
}

#[test]
fn nonempty_inline_environment_projects_three_host_states() {
    let host_id = NodeId::generate();
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines = vec![ParagraphContent::Environment(InlineEnvironment {
        id: host_id.clone(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::new(),
        children: vec![ParagraphContent::Text(TextFlow {
            runs: vec![Run {
                text: "x".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    })];
    let paragraph_id = paragraph.id.clone();
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("nonempty inline session failed: {error}"));

    for (offset, affinity, expected_inside) in [
        (0, Affinity::Upstream, false),
        (0, Affinity::Downstream, true),
        (1, Affinity::Upstream, true),
        (1, Affinity::Downstream, false),
    ] {
        let position = Pos {
            story: crate::editing::StoryId::Body,
            paragraph_id: paragraph_id.clone(),
            offset,
            affinity,
        };
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: position.clone(),
                focus: position,
            })))
            .unwrap_or_else(|error| panic!("setting inline boundary selection failed: {error}"));
        let projected = session.active_host_caret_position();
        match (affinity, expected_inside, projected) {
            (
                Affinity::Upstream,
                false,
                Some(crate::editing::HostCaretPosition::Before {
                    host_id: projected_id,
                    kind: crate::editing::HostCaretKind::Inline,
                }),
            ) if offset == 0 && projected_id == host_id => {}
            (
                Affinity::Downstream,
                false,
                Some(crate::editing::HostCaretPosition::After {
                    host_id: projected_id,
                    kind: crate::editing::HostCaretKind::Inline,
                }),
            ) if offset == 1 && projected_id == host_id => {}
            (
                _,
                true,
                Some(crate::editing::HostCaretPosition::Inside {
                    host_id: projected_id,
                    kind: crate::editing::HostCaretKind::Inline,
                    ..
                }),
            ) if projected_id == host_id => {}
            (_, _, actual) => panic!("unexpected inline host projection: {actual:?}"),
        }
    }
}

#[test]
fn backslash_and_dollar_keys_enter_their_editor_modes() {
    let mut command_session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("session initialization failed: {error}"));
    command_session
        .run(Command::EditorKey(EditorKey::Backslash))
        .unwrap_or_else(|error| panic!("backslash input failed: {error}"));
    assert!(command_session.command_line().is_some());

    let mut math_session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("session initialization failed: {error}"));
    let effect = math_session
        .run(Command::EditorKey(EditorKey::Dollar))
        .unwrap_or_else(|error| panic!("dollar input failed: {error}"));
    assert!(effect.document_changed);
    assert!(math_session.math_edit().is_some());
}

#[test]
fn clipboard_keys_are_decided_by_the_session_focus_owner() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("session initialization failed: {error}"));
    let paste = session
        .run(Command::EditorKey(EditorKey::ModV))
        .unwrap_or_else(|error| panic!("document paste shortcut failed: {error}"));
    assert_eq!(paste.clipboard_action, Some(ClipboardAction::Paste));

    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command input did not open: {error}"));
    let blocked = session
        .run(Command::EditorKey(EditorKey::ModV))
        .unwrap_or_else(|error| panic!("command line clipboard shortcut failed: {error}"));
    assert_eq!(blocked.clipboard_action, None);
}

fn paragraph_with_run_text(text: &str) -> caret_doc::Paragraph {
    let mut paragraph = caret_doc::Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(ParagraphContent::Text(TextFlow {
        runs: vec![Run {
            text: text.to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    }));
    paragraph
}

fn inline_math_content(source: &str) -> ParagraphContent {
    let caret_math::ParsedMath::Tree(math_tree) =
        caret_math::parse(source, caret_math::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("inline formula fixture failed: {error}"))
    else {
        panic!("inline formula fixture should produce a MathTree");
    };
    ParagraphContent::Math(caret_doc::InlineMath {
        id: NodeId::generate(),
        math_tree,
        extra: BTreeMap::new(),
    })
}

/// 创建 middle = `ab{inline math}cd`、前后各有一个段落的文档，并把焦点放到公式前。
fn session_with_inline_math_in_the_middle(source: &str) -> (Session, NodeId) {
    let above = paragraph_with_run_text("above");
    let mut middle = paragraph_with_run_text("ab");
    middle.inlines.push(inline_math_content(source));
    middle.inlines.push(ParagraphContent::Text(TextFlow {
        runs: vec![Run {
            text: "cd".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    }));
    let middle_id = middle.id.clone();
    let below = paragraph_with_run_text("below");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(above),
        Block::Paragraph(middle),
        Block::Paragraph(below),
    ];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("session initialization failed: {error}"));
    let position = Pos {
        story: StoryId::Body,
        paragraph_id: middle_id.clone(),
        offset: 2,
        affinity: Affinity::Downstream,
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: position.clone(),
            focus: position,
        })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));
    session
        .run(Command::EnterMath(Direction::Forward))
        .unwrap_or_else(|error| panic!("inline formula focus failed: {error}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));
    (session, middle_id)
}

fn vertical_key(direction: Direction, extend: bool) -> Command {
    let candidate = None;
    Command::EditorKey(match direction {
        Direction::Backward => EditorKey::UpArrow {
            candidate,
            extend,
            structure: false,
        },
        Direction::Forward => EditorKey::DownArrow {
            candidate,
            extend,
            structure: false,
        },
    })
}

#[test]
fn inline_math_up_at_tree_edge_exits_to_the_object_front() {
    let (mut session, paragraph_id) = session_with_inline_math_in_the_middle("x");

    let effect = session
        .run(vertical_key(Direction::Backward, false))
        .unwrap_or_else(|error| panic!("inline math Up failed: {error}"));

    assert!(effect.selection_changed);
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().anchor, session.selection().focus);
    assert_eq!(session.selection().focus.paragraph_id, paragraph_id);
    assert_eq!(session.selection().focus.offset, 2);
    assert_eq!(session.selection().focus.affinity, Affinity::Downstream);
}

#[test]
fn inline_math_down_at_tree_edge_exits_to_the_object_back() {
    let (mut session, paragraph_id) = session_with_inline_math_in_the_middle("x");

    session
        .run(vertical_key(Direction::Forward, false))
        .unwrap_or_else(|error| panic!("inline math Down failed: {error}"));

    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().anchor, session.selection().focus);
    assert_eq!(session.selection().focus.paragraph_id, paragraph_id);
    assert_eq!(session.selection().focus.offset, 5);
    assert_eq!(session.selection().focus.affinity, Affinity::Upstream);
}

#[test]
fn inline_math_shift_vertical_at_tree_edge_also_exits_the_math_edit() {
    for direction in [Direction::Backward, Direction::Forward] {
        let (mut session, _paragraph_id) = session_with_inline_math_in_the_middle("x");

        let effect = session
            .run(vertical_key(direction, true))
            .unwrap_or_else(|error| panic!("inline math shift vertical failed: {error}"));

        assert!(effect.selection_changed);
        assert!(
            session.math_edit().is_none(),
            "Shift+vertical must not be swallowed inside InlineMath"
        );
        assert_eq!(session.selection().anchor, session.selection().focus);
    }
}

#[test]
fn inline_math_text_island_vertical_edge_exits_to_the_object_sides() {
    let mut session = session_in_inline_math_text_island();
    session
        .run(vertical_key(Direction::Backward, false))
        .unwrap_or_else(|error| panic!("math text Up failed: {error}"));
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.offset, 0);
    assert_eq!(session.selection().focus.affinity, Affinity::Downstream);

    let mut session = session_in_inline_math_text_island();
    session
        .run(vertical_key(Direction::Forward, false))
        .unwrap_or_else(|error| panic!("math text Down failed: {error}"));
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.offset, 3);
    assert_eq!(session.selection().focus.affinity, Affinity::Upstream);
}

#[test]
fn block_math_vertical_edge_behavior_is_unchanged() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session initialization failed: {error}"));
    session
        .insert_display_math(false)
        .unwrap_or_else(|error| panic!("equation insertion failed: {error}"));

    // Block 的 Shift 垂直仍没有布局候选：保持既有 no-op。
    let before = session.selection().clone();
    let effect = session
        .run(vertical_key(Direction::Backward, true))
        .unwrap_or_else(|error| panic!("equation Shift+Up failed: {error}"));
    assert!(!effect.selection_changed);
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(crate::MathEditTarget::Block { .. })
    ));
    assert_eq!(session.selection(), &before);

    // Block 的非扩展垂直在边缘退出到块外侧停靠。
    session
        .run(vertical_key(Direction::Forward, false))
        .unwrap_or_else(|error| panic!("equation Down failed: {error}"));
    assert!(session.math_edit().is_none());
    assert!(session.block_boundary_exit().is_some());
}

#[test]
fn command_line_vertical_navigation_keeps_formula_focus_and_query() {
    let mut session = session_in_inline_math_tree("x");
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("abc".to_owned()))
        .unwrap_or_else(|error| panic!("command line typing failed: {error}"));

    session
        .run(vertical_key(Direction::Backward, false))
        .unwrap_or_else(|error| panic!("command line Up failed: {error}"));

    assert!(session.math_edit().is_some());
    assert_eq!(
        session.command_line().map(|line| line.query.as_str()),
        Some("abc")
    );
}

#[test]
fn ime_composition_vertical_navigation_does_not_exit_formula() {
    let mut session = session_in_inline_math_text_island();
    session
        .run(Command::UpdateComposition {
            id: 902,
            preedit: "中".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition update failed: {error}"));
    let before = session.document().clone();

    let effect = session
        .run(vertical_key(Direction::Backward, false))
        .unwrap_or_else(|error| panic!("composition Up failed: {error}"));

    assert!(!effect.selection_changed);
    assert_eq!(session.document(), &before);
    assert!(session.math_edit().is_some());
    assert!(session.composition_preview_with_range(0).is_ok());
}

#[test]
fn inline_math_source_branch_vertical_edge_exits_the_formula() {
    let (mut session, paragraph_id) = session_with_inline_math_in_the_middle("x");
    // Source 分支与 MathTree/MathText 同规则：垂直边缘退出到 U+FFFC 前侧。
    let Some(edit) = session.math_edit.as_mut() else {
        panic!("inline math edit should be active");
    };
    edit.branch = MathEditBranch::Source;
    edit.source_draft = None;

    session
        .run(vertical_key(Direction::Backward, false))
        .unwrap_or_else(|error| panic!("inline source Up failed: {error}"));

    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.paragraph_id, paragraph_id);
    assert_eq!(session.selection().focus.offset, 2);
    assert_eq!(session.selection().focus.affinity, Affinity::Downstream);
}

#[test]
fn inline_math_boundary_exit_uses_the_object_outside_affinities() {
    // Shift+Right 越界：anchor 在对象前侧(Downstream)，focus 在对象后侧(Upstream)。
    let mut session = session_in_inline_math_tree("x+y");
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-right boundary conversion failed: {error}"));
    let selection = session.selection().clone();
    assert!(session.math_edit().is_none());
    assert_eq!(
        (selection.anchor.offset, selection.anchor.affinity),
        (0, Affinity::Downstream)
    );
    assert_eq!(
        (selection.focus.offset, selection.focus.affinity),
        (3, Affinity::Upstream)
    );
    assert_eq!(
        session.selection_visual_affinity(),
        caret_layout::VisualAffinity::Upstream
    );
    assert_eq!(
        session.selection_anchor_visual_affinity(),
        caret_layout::VisualAffinity::Downstream
    );

    // Shift+Left 对称：anchor 在对象后侧(Upstream)，focus 在对象前侧(Downstream)。
    let mut session = session_in_inline_math_tree("x+y");
    for _ in 0..3 {
        session
            .run(Command::MoveCaret(Direction::Backward))
            .unwrap_or_else(|error| panic!("math caret movement failed: {error}"));
    }
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-left boundary conversion failed: {error}"));
    let selection = session.selection().clone();
    assert!(session.math_edit().is_none());
    assert_eq!(
        (selection.anchor.offset, selection.anchor.affinity),
        (3, Affinity::Upstream)
    );
    assert_eq!(
        (selection.focus.offset, selection.focus.affinity),
        (0, Affinity::Downstream)
    );

    // 非扩展越界必须落到与 Shift 相同的对象外侧位置，caret 才不会落到对象内部。
    let mut session = session_in_inline_math_tree("x+y");
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("non-shift right boundary exit failed: {error}"));
    let focus = session.selection().focus.clone();
    assert!(session.math_edit().is_none());
    assert_eq!((focus.offset, focus.affinity), (3, Affinity::Upstream));

    let mut session = session_in_inline_math_tree("x+y");
    for _ in 0..3 {
        session
            .run(Command::MoveCaret(Direction::Backward))
            .unwrap_or_else(|error| panic!("math caret movement failed: {error}"));
    }
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("non-shift left boundary exit failed: {error}"));
    let focus = session.selection().focus.clone();
    assert!(session.math_edit().is_none());
    assert_eq!((focus.offset, focus.affinity), (0, Affinity::Downstream));
}

/// 构造一个显示公式块；Align 单行单 cell，Equation 单行公式。
fn navigation_math_block(align: bool) -> (Block, NodeId) {
    let id = NodeId::generate();
    let caret_math::ParsedMath::Tree(math_tree) =
        caret_math::parse("x", caret_math::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("math fixture failed: {error}"))
    else {
        panic!("inline math fixture should produce a MathTree");
    };
    let block = if align {
        Block::Align(caret_doc::Align {
            id: id.clone(),
            rows: vec![caret_doc::MathAlignRow {
                id: NodeId::generate(),
                cells: vec![math_tree],
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        })
    } else {
        Block::Equation(caret_doc::Equation {
            id: id.clone(),
            math_tree,
            extra: BTreeMap::new(),
        })
    };
    (block, id)
}

fn navigation_document_candidate(session: &Session, focus: Pos) -> NavigationCandidate {
    NavigationCandidate::Document {
        focus,
        visual_affinity: caret_layout::VisualAffinity::Downstream,
        generation: NavigationGeneration {
            session_id: session.session_id().clone(),
            document_generation: session.document_generation(),
            font_generation: 0,
        },
    }
}

fn body_pos(paragraph: &caret_doc::Paragraph, offset: usize, affinity: Affinity) -> Pos {
    Pos {
        story: StoryId::Body,
        paragraph_id: paragraph.id.clone(),
        offset,
        affinity,
    }
}

/// 构造 `p / Equation / Align / q` 文档：连续两个显示公式块夹在段落之间。
fn session_between_consecutive_math_blocks() -> (Session, Pos, Pos, NodeId, NodeId) {
    let before = paragraph_with_run_text("p");
    let before_end = body_pos(&before, 1, Affinity::Upstream);
    let after = paragraph_with_run_text("q");
    let after_start = body_pos(&after, 0, Affinity::Downstream);
    let (first, first_id) = navigation_math_block(false);
    let (second, second_id) = navigation_math_block(true);
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        first,
        second,
        Block::Paragraph(after),
    ];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("consecutive math session failed: {error}"));
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: before_end.clone(),
            focus: before_end.clone(),
        })))
        .unwrap_or_else(|error| panic!("caret setup failed: {error}"));
    (session, before_end, after_start, first_id, second_id)
}

#[test]
fn horizontal_candidate_crossing_consecutive_math_blocks_stops_at_the_first_block() {
    let (mut session, before_end, after_start, first_id, second_id) =
        session_between_consecutive_math_blocks();
    let candidate = navigation_document_candidate(&session, after_start.clone());

    // 第一次 Right：候选跨过两个公式块，但只在前方第一块外侧停靠。
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate.clone()),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("entering first math stop failed: {error}"));
    assert!(session.math_edit().is_none());
    assert_eq!(
        session.block_boundary_exit(),
        Some((first_id.clone(), Direction::Backward))
    );
    assert!(matches!(
        session.host_caret_position.as_ref(),
        Some(crate::editing::HostCaretPosition::Before {
            host_id,
            kind: crate::editing::HostCaretKind::Block,
        }) if host_id == &first_id
    ));
    assert_eq!(session.selection().focus, before_end);

    // 第二次 Right：进入第一块，而不是跳到候选所在段落。
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("entering first math block failed: {error}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(crate::editing::MathEditTarget::Block { block_id, align: false })
            if block_id == &first_id
    ));

    // 反向对称：从后段起点 Left 先停在最后一块外侧。
    let mut session = Session::new(session.document().clone())
        .unwrap_or_else(|error| panic!("reverse session failed: {error}"));
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: after_start.clone(),
            focus: after_start.clone(),
        })))
        .unwrap_or_else(|error| panic!("reverse caret setup failed: {error}"));
    let candidate = navigation_document_candidate(&session, before_end);
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(candidate.clone()),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("entering last math stop failed: {error}"));
    assert!(session.math_edit().is_none());
    assert_eq!(
        session.block_boundary_exit(),
        Some((second_id.clone(), Direction::Forward))
    );
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(candidate),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("entering last math block failed: {error}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(crate::editing::MathEditTarget::Block { block_id, align: true })
            if block_id == &second_id
    ));
}

#[test]
fn shift_candidate_crossing_consecutive_math_blocks_extends_one_block_at_a_time() {
    let (mut session, _, after_start, first_id, second_id) =
        session_between_consecutive_math_blocks();
    let candidate = navigation_document_candidate(&session, after_start.clone());

    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate.clone()),
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("first shift-right failed: {error}"));
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&first_id)
    );
    let (_, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("first structural range missing"));
    assert!(matches!(
        focus,
        crate::editing::DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Forward,
        } if block_id == &first_id
    ));

    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate.clone()),
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("second shift-right failed: {error}"));
    assert_eq!(
        session.selected_math_blocks(),
        &[first_id.clone(), second_id.clone()]
    );
    let (_, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("second structural range missing"));
    assert!(matches!(
        focus,
        crate::editing::DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Forward,
        } if block_id == &second_id
    ));
    assert_eq!(
        session.selection().focus.paragraph_id,
        after_start.paragraph_id
    );
    assert_eq!(session.selection().focus.offset, after_start.offset);

    // 第三次 Shift+Right 仍按文档单元继续，焦点越过公式链进入后段文本起点。
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate),
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("third shift-right failed: {error}"));
    let (_, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("third structural range missing"));
    assert!(matches!(
        focus,
        crate::editing::DocumentRangeEndpoint::Text(position)
            if position.paragraph_id == after_start.paragraph_id && position.offset == 0
    ));
    assert!(session.block_boundary_exit().is_none());

    // 反向对称：从后段起点 Shift+Left 逐个纳入公式块。
    let before_end = body_pos(
        session
            .document()
            .body
            .first()
            .and_then(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph),
                _ => None,
            })
            .unwrap_or_else(|| panic!("leading paragraph missing")),
        1,
        Affinity::Upstream,
    );
    let mut session = Session::new(session.document().clone())
        .unwrap_or_else(|error| panic!("reverse session failed: {error}"));
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: after_start.clone(),
            focus: after_start.clone(),
        })))
        .unwrap_or_else(|error| panic!("reverse caret setup failed: {error}"));
    let candidate = navigation_document_candidate(&session, before_end.clone());
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(candidate.clone()),
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("first shift-left failed: {error}"));
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&second_id)
    );
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(candidate.clone()),
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("second shift-left failed: {error}"));
    assert_eq!(
        session.selected_math_blocks(),
        &[first_id.clone(), second_id.clone()]
    );
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(candidate),
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("third shift-left failed: {error}"));
    let (_, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("reverse structural range missing"));
    assert!(matches!(
        focus,
        crate::editing::DocumentRangeEndpoint::Text(position)
            if position.paragraph_id == before_end.paragraph_id && position.offset == 1
    ));
}
