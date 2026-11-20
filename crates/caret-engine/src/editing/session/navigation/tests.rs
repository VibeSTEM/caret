use super::super::super::{
    Affinity, ClipboardAction, Command, Direction, EditorKey, EnvironmentKind, MathEditBranch,
    NavigationCandidate, NavigationGeneration, Pos, Selection, Session, StoryId,
};
use caret_doc::{
    Block, BlockEnvironment, Document, InlineEnvironment, InlineEnvironmentKind, NodeId,
    ParagraphContent, Run, TextFlow,
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

fn equation_block_for_unit_navigation() -> (Block, NodeId) {
    let caret_math::ParsedMath::Tree(math_tree) =
        caret_math::parse("x", caret_math::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("equation fixture failed: {error}"))
    else {
        panic!("equation fixture should produce a MathTree");
    };
    let id = NodeId::generate();
    (
        Block::Equation(caret_doc::Equation {
            id: id.clone(),
            math_tree,
            extra: Default::default(),
        }),
        id,
    )
}

fn section_block_for_unit_navigation(title: &str, body: &str) -> (Block, NodeId, NodeId, NodeId) {
    let title_paragraph = paragraph_with_run_text(title);
    let title_id = title_paragraph.id.clone();
    let body_paragraph = paragraph_with_run_text(body);
    let body_id = body_paragraph.id.clone();
    let id = NodeId::generate();
    (
        Block::Environment(BlockEnvironment {
            id: id.clone(),
            kind: caret_doc::BlockEnvironmentKind::Section,
            definition_id: None,
            props: std::collections::BTreeMap::new(),
            title: Some(title_paragraph),
            children: vec![Block::Paragraph(body_paragraph)],
            extra: std::collections::BTreeMap::new(),
        }),
        id,
        title_id,
        body_id,
    )
}

fn document_candidate_for(
    session: &Session,
    focus: Pos,
    visual_affinity: caret_layout::VisualAffinity,
) -> NavigationCandidate {
    NavigationCandidate::Document {
        focus,
        visual_affinity,
        generation: NavigationGeneration {
            session_id: session.session_id().clone(),
            document_generation: session.document_generation(),
            font_generation: 0,
        },
    }
}

fn title_start_position(environment_id: &NodeId, title_id: &NodeId) -> Pos {
    Pos {
        story: StoryId::EnvironmentTitle(environment_id.clone()),
        paragraph_id: title_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    }
}

#[test]
fn horizontal_candidate_stops_at_equation_before_entering_environment() {
    let before = paragraph_with_run_text("p");
    let before_id = before.id.clone();
    let (equation, equation_id) = equation_block_for_unit_navigation();
    let (environment, environment_id, title_id, _body_id) =
        section_block_for_unit_navigation("s", "body");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        equation,
        environment,
        Block::Paragraph(paragraph_with_run_text("q")),
    ];
    let mut session = Session::new(document).unwrap_or_else(|error| {
        panic!("session initialization failed: {error}");
    });
    let start = Pos {
        story: StoryId::Body,
        paragraph_id: before_id,
        offset: 1,
        affinity: Affinity::Upstream,
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: start.clone(),
            focus: start.clone(),
        })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));

    let candidate = document_candidate_for(
        &session,
        title_start_position(&environment_id, &title_id),
        caret_layout::VisualAffinity::Downstream,
    );
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate.clone()),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("right candidate failed: {error}"));
    assert!(session.math_edit().is_none());
    assert_eq!(
        session.block_boundary_exit(),
        Some((equation_id.clone(), Direction::Backward))
    );
    assert_eq!(session.selection().focus, start);

    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("second right candidate failed: {error}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(crate::MathEditTarget::Block { block_id, .. }) if block_id == &equation_id
    ));
}

#[test]
fn shift_candidate_adds_equation_then_environment_one_unit_at_a_time() {
    let before = paragraph_with_run_text("p");
    let before_id = before.id.clone();
    let (equation, equation_id) = equation_block_for_unit_navigation();
    let (environment, environment_id, title_id, _body_id) =
        section_block_for_unit_navigation("s", "body");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        equation,
        environment,
        Block::Paragraph(paragraph_with_run_text("q")),
    ];
    let mut session = Session::new(document).unwrap_or_else(|error| {
        panic!("session initialization failed: {error}");
    });
    let start = Pos {
        story: StoryId::Body,
        paragraph_id: before_id,
        offset: 1,
        affinity: Affinity::Upstream,
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: start.clone(),
            focus: start,
        })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));
    let candidate = document_candidate_for(
        &session,
        title_start_position(&environment_id, &title_id),
        caret_layout::VisualAffinity::Downstream,
    );

    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate.clone()),
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("first shift candidate failed: {error}"));
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    assert!(session.selected_block_environments().is_empty());

    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate),
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("second shift candidate failed: {error}"));
    assert_eq!(
        session.selected_block_environments(),
        std::slice::from_ref(&environment_id)
    );
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
}

fn body_position(paragraph_id: &NodeId, offset: usize, affinity: Affinity) -> Pos {
    Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset,
        affinity,
    }
}

#[test]
fn horizontal_candidate_from_environment_outer_stop_stops_at_adjacent_equation() {
    let before = paragraph_with_run_text("p");
    let (environment, environment_id, _title_id, body_id) =
        section_block_for_unit_navigation("s", "body");
    let (equation, equation_id) = equation_block_for_unit_navigation();
    let after = paragraph_with_run_text("q");
    let after_id = after.id.clone();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        environment,
        equation,
        Block::Paragraph(after),
    ];
    let mut session = Session::new(document).unwrap_or_else(|error| {
        panic!("session initialization failed: {error}");
    });
    let start = body_position(&body_id, "body".len(), Affinity::Upstream);
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: start.clone(),
            focus: start,
        })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));
    let candidate = document_candidate_for(
        &session,
        body_position(&after_id, 0, Affinity::Downstream),
        caret_layout::VisualAffinity::Downstream,
    );

    // 先按环境外侧边界契约停靠；下一次按键必须停在相邻公式外前，不能直接到后段。
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate.clone()),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("environment exit failed: {error}"));
    assert_eq!(
        session.block_boundary_exit(),
        Some((environment_id, Direction::Forward))
    );
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate.clone()),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("equation stop failed: {error}"));
    assert!(session.math_edit().is_none());
    assert_eq!(
        session.block_boundary_exit(),
        Some((equation_id.clone(), Direction::Backward))
    );
    // 停靠必须同步折叠选区与可视亲和性，而不是只写共享宿主。
    assert_eq!(session.selection().anchor, session.selection().focus);
    assert_eq!(
        session.selection().focus,
        body_position(&body_id, "body".len(), Affinity::Upstream)
    );
    assert_eq!(
        session.selection_visual_affinity(),
        caret_layout::VisualAffinity::Upstream
    );
    assert_eq!(
        session.selection_anchor_visual_affinity(),
        caret_layout::VisualAffinity::Upstream
    );
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("equation entry failed: {error}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(crate::MathEditTarget::Block { block_id, .. }) if block_id == &equation_id
    ));
}

#[test]
fn horizontal_candidate_backward_from_environment_outer_stop_stops_at_adjacent_equation() {
    let before = paragraph_with_run_text("p");
    let before_id = before.id.clone();
    let (equation, equation_id) = equation_block_for_unit_navigation();
    let (environment, environment_id, title_id, _body_id) =
        section_block_for_unit_navigation("s", "body");
    let after = paragraph_with_run_text("q");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        equation,
        environment,
        Block::Paragraph(after),
    ];
    let mut session = Session::new(document).unwrap_or_else(|error| {
        panic!("session initialization failed: {error}");
    });
    let start = title_start_position(&environment_id, &title_id);
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: start.clone(),
            focus: start,
        })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));
    let candidate = document_candidate_for(
        &session,
        body_position(&before_id, 1, Affinity::Upstream),
        caret_layout::VisualAffinity::Upstream,
    );

    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(candidate.clone()),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("environment exit failed: {error}"));
    assert_eq!(
        session.block_boundary_exit(),
        Some((environment_id.clone(), Direction::Backward))
    );
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(candidate.clone()),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("equation stop failed: {error}"));
    assert!(session.math_edit().is_none());
    assert_eq!(
        session.block_boundary_exit(),
        Some((equation_id.clone(), Direction::Forward))
    );
    assert_eq!(session.selection().anchor, session.selection().focus);
    assert_eq!(
        session.selection().focus,
        title_start_position(&environment_id, &title_id)
    );
    assert_eq!(
        session.selection_visual_affinity(),
        caret_layout::VisualAffinity::Downstream
    );
    assert_eq!(
        session.selection_anchor_visual_affinity(),
        caret_layout::VisualAffinity::Downstream
    );
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(candidate),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("equation entry failed: {error}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(crate::MathEditTarget::Block { block_id, .. }) if block_id == &equation_id
    ));
}

#[test]
fn shift_candidate_backward_from_environment_extends_by_document_units() {
    let before = paragraph_with_run_text("p");
    let before_id = before.id.clone();
    let (first, first_id, _first_title, _first_body) =
        section_block_for_unit_navigation("s1", "b1");
    let (equation, equation_id) = equation_block_for_unit_navigation();
    let (second, second_id, _second_title, second_body) =
        section_block_for_unit_navigation("s2", "b2");
    let after = paragraph_with_run_text("q");
    let after_id = after.id.clone();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        first,
        equation,
        second,
        Block::Paragraph(after),
    ];
    let mut session = Session::new(document).unwrap_or_else(|error| {
        panic!("session initialization failed: {error}");
    });
    let start = body_position(&after_id, 0, Affinity::Downstream);
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: start.clone(),
            focus: start,
        })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));

    // 第一次选中结尾环境；第二次必须加入前一个公式，而不是在环境内逐字后退。
    let candidate = document_candidate_for(
        &session,
        body_position(&second_body, "b2".len(), Affinity::Upstream),
        caret_layout::VisualAffinity::Upstream,
    );
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(candidate),
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("first shift left failed: {error}"));
    assert_eq!(
        session.selected_block_environments(),
        std::slice::from_ref(&second_id)
    );

    let candidate = document_candidate_for(
        &session,
        body_position(&before_id, 1, Affinity::Upstream),
        caret_layout::VisualAffinity::Upstream,
    );
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(candidate),
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("second shift left failed: {error}"));
    assert_eq!(
        session.selected_math_blocks(),
        std::slice::from_ref(&equation_id)
    );
    assert_eq!(
        session.selected_block_environments(),
        std::slice::from_ref(&second_id)
    );

    // 第三次继续加入前一个环境；第四次才扩展到前段文本终点。
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("third shift left failed: {error}"));
    assert_eq!(
        session.selected_block_environments(),
        &[first_id.clone(), second_id.clone()]
    );
    assert!(matches!(
        session.structure_selection_range().map(|(_, focus)| focus),
        Some(crate::editing::DocumentRangeEndpoint::BlockEdge {
            block_id,
            side: Direction::Backward,
        }) if block_id == &first_id
    ));
    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("fourth shift left failed: {error}"));
    assert_eq!(
        session.structure_selection_range().map(|(_, focus)| focus),
        Some(&crate::editing::DocumentRangeEndpoint::Text(body_position(
            &before_id,
            1,
            Affinity::Upstream
        )))
    );
}

#[test]
fn horizontal_candidate_direction_mismatch_does_not_park_at_equation() {
    let before = paragraph_with_run_text("p");
    let before_id = before.id.clone();
    let (environment, environment_id, _title_id, body_id) =
        section_block_for_unit_navigation("s", "body");
    let (equation, equation_id) = equation_block_for_unit_navigation();
    let after = paragraph_with_run_text("q");
    let after_id = after.id.clone();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        environment,
        equation,
        Block::Paragraph(after),
    ];
    let mut session = Session::new(document).unwrap_or_else(|error| {
        panic!("session initialization failed: {error}");
    });
    let start = body_position(&body_id, "body".len(), Affinity::Upstream);
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: start.clone(),
            focus: start,
        })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));

    // 先按右手键离开环境外侧停靠。
    let forward_candidate = document_candidate_for(
        &session,
        body_position(&after_id, 0, Affinity::Downstream),
        caret_layout::VisualAffinity::Downstream,
    );
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(forward_candidate),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("environment exit failed: {error}"));
    assert_eq!(
        session.block_boundary_exit(),
        Some((environment_id, Direction::Forward))
    );

    // RightArrow 却带反向候选：方向冲突时不能把光标反向停到公式外前。
    let reverse_candidate = document_candidate_for(
        &session,
        body_position(&before_id, 1, Affinity::Upstream),
        caret_layout::VisualAffinity::Upstream,
    );
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(reverse_candidate),
            extend: false,
        }))
        .unwrap_or_else(|error| panic!("mismatched candidate failed: {error}"));
    assert_ne!(
        session.block_boundary_exit(),
        Some((equation_id, Direction::Backward))
    );
}

#[test]
fn shift_between_paragraph_and_inline_math_root_edits_only_the_selected_items()
-> Result<(), Box<dyn std::error::Error>> {
    for (forward, start, copied, remaining) in [(true, 1, "bx", "+y"), (false, 6, "yc", "x+")] {
        let mut session = Session::new(Document::blank())?;
        session.run(Command::InsertText("ab".into()))?;
        session.run(Command::InsertInlineMath("x+y".into()))?;
        session.run(Command::InsertText("cd".into()))?;
        let original = session.document().clone();
        let mut anchor = session.selection().focus.clone();
        anchor.offset = start;
        let mut edge = anchor.clone();
        edge.offset = if forward { 2 } else { 5 };
        session.run(Command::SetSelection(Some(Selection {
            anchor: anchor.clone(),
            focus: edge.clone(),
        })))?;
        let key = |right: bool| {
            Command::EditorKey(if right {
                EditorKey::RightArrow {
                    candidate: None,
                    extend: true,
                }
            } else {
                EditorKey::LeftArrow {
                    candidate: None,
                    extend: true,
                }
            })
        };
        session.run(key(forward))?;
        assert_eq!(session.selected_text()?, copied);
        let mut collapse = session.clone();
        collapse.run(Command::EditorKey(if forward {
            EditorKey::RightArrow {
                candidate: None,
                extend: false,
            }
        } else {
            EditorKey::LeftArrow {
                candidate: None,
                extend: false,
            }
        }))?;
        assert!(collapse.structure_selection_range().is_none());
        assert_eq!(
            collapse
                .math_tree_input_context()
                .ok_or("expected math focus after collapse")?
                .1
                .affinity,
            if forward {
                crate::editing::MathPositionAffinity::Upstream
            } else {
                crate::editing::MathPositionAffinity::Downstream
            }
        );
        session.run(key(!forward))?;
        assert_eq!(session.selection().anchor, anchor);
        assert_eq!(session.selection().focus.offset, edge.offset);
        session.run(key(forward))?;
        let parts = session
            .inline_math_range_parts()
            .ok_or("expected mixed selection")?;
        let stale = NavigationCandidate::Document {
            focus: edge.clone(),
            visual_affinity: caret_layout::VisualAffinity::Downstream,
            generation: NavigationGeneration {
                session_id: session.session_id().clone(),
                document_generation: session.document_generation() + 1,
                font_generation: 0,
            },
        };
        let history = session.history_state_id();
        assert!(
            session
                .run(Command::EditorKey(EditorKey::RightArrow {
                    candidate: Some(stale),
                    extend: true
                }))
                .is_err()
        );
        assert_eq!(session.history_state_id(), history);
        assert_eq!(session.inline_math_range_parts().as_ref(), Some(&parts));
        for replacement in [false, true] {
            let mut edit = session.clone();
            if replacement {
                edit.run(Command::InsertText("Z".into()))?;
            } else {
                edit.run(Command::EditorKey(EditorKey::Delete))?;
            }
            let Block::Paragraph(p) = &edit.document().body[0] else {
                return Err("expected paragraph".into());
            };
            let text = p
                .inlines
                .iter()
                .map(|node| match node {
                    ParagraphContent::Math(math) => {
                        format!("<{}>", caret_math::serialize_tree(&math.math_tree))
                    }
                    ParagraphContent::Text(flow) => {
                        flow.runs.iter().map(|run| run.text.as_str()).collect()
                    }
                    _ => String::new(),
                })
                .collect::<String>();
            let insert = if replacement { "Z" } else { "" };
            let expected = if forward {
                format!("a{insert}<{remaining}>cd")
            } else {
                format!("ab<{remaining}>{insert}d")
            };
            assert_eq!(text, expected);
            let result = edit.document().clone();
            edit.run(Command::Undo)?;
            assert_eq!(edit.document(), &original);
            assert_eq!(edit.inline_math_range_parts().as_ref(), Some(&parts));
            edit.run(Command::Redo)?;
            assert_eq!(edit.document(), &result);
            assert!(edit.structure_selection_range().is_none());
        }
    }
    Ok(())
}

fn mixed_tree_focus_offset(session: &Session) -> Result<usize, &'static str> {
    match session.structure_selection_range() {
        Some((_, crate::DocumentRangeEndpoint::InlineMathTree { position, .. })) => {
            Ok(position.offset)
        }
        _ => Err("expected mixed selection with tree focus"),
    }
}

#[test]
fn visual_candidate_crossing_rtl_inline_math_steps_from_the_near_root_edge()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut session, paragraph_id) = session_with_inline_math_in_the_middle("x+y");
    let after = body_position(&paragraph_id, 5, Affinity::Upstream);
    let anchor = body_position(&paragraph_id, 6, Affinity::Downstream);
    let before = body_position(&paragraph_id, 2, Affinity::Downstream);
    session.run(Command::SetSelection(Some(Selection {
        anchor: anchor.clone(),
        focus: after.clone(),
    })))?;

    // RTL 视觉右移可从 InlineMath 之后跨到逻辑上较早的正文偏移。
    for expected_offset in [2, 1] {
        let candidate = document_candidate_for(
            &session,
            before.clone(),
            caret_layout::VisualAffinity::Downstream,
        );
        session.run(Command::EditorKey(EditorKey::RightArrow {
            candidate: Some(candidate),
            extend: true,
        }))?;
        let parts = session
            .inline_math_range_parts()
            .ok_or("visual crossing should retain an InlineMath range")?;
        assert_eq!(mixed_tree_focus_offset(&session)?, expected_offset);
        assert_eq!(parts.body.anchor.offset, 6);
    }
    assert_eq!(session.selected_text()?, "+yc");

    // 视觉左移回到正文锚点时，候选不再跨公式，交给共享范围入口收缩。
    let outside = document_candidate_for(
        &session,
        body_position(&paragraph_id, 6, Affinity::Downstream),
        caret_layout::VisualAffinity::Downstream,
    );
    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: Some(outside),
        extend: true,
    }))?;
    assert_eq!(mixed_tree_focus_offset(&session)?, 2);
    assert_eq!(session.selected_text()?, "yc");
    Ok(())
}

#[test]
fn inline_math_entry_direction_is_independent_of_the_body_selection_anchor()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut session, paragraph_id) = session_with_inline_math_in_the_middle("x+y");
    let anchor = body_position(&paragraph_id, 1, Affinity::Downstream);
    let focus = body_position(&paragraph_id, 5, Affinity::Upstream);
    let entered_edge = body_position(&paragraph_id, 2, Affinity::Downstream);
    session.run(Command::SetSelection(Some(Selection {
        anchor: anchor.clone(),
        focus,
    })))?;

    for expected_offset in [2, 1] {
        let candidate = document_candidate_for(
            &session,
            entered_edge.clone(),
            caret_layout::VisualAffinity::Downstream,
        );
        session.run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: Some(candidate),
            extend: true,
        }))?;
        assert_eq!(mixed_tree_focus_offset(&session)?, expected_offset);
    }
    assert_eq!(session.selected_text()?, "bx");

    let candidate = document_candidate_for(
        &session,
        body_position(&paragraph_id, 5, Affinity::Upstream),
        caret_layout::VisualAffinity::Upstream,
    );
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: Some(candidate),
        extend: true,
    }))?;
    assert_eq!(mixed_tree_focus_offset(&session)?, 2);
    assert_eq!(session.selected_text()?, "bx+");
    let range = session
        .structure_selection_range()
        .ok_or("expected document-level InlineMath range")?;
    assert_eq!(
        range.0,
        &crate::editing::DocumentRangeEndpoint::Text(anchor)
    );
    Ok(())
}

#[test]
fn mixed_pointer_text_grapheme_preserves_nested_operands_and_history()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        DocumentRangeEndpoint, MathPositionAffinity, MathTextFlowPath,
        MathTextFlowPathStep as Step, MathTextPosition, StructureSelection, VisualAffinity,
    };
    let mut paragraph = paragraph_with_run_text("前");
    let ParagraphContent::Math(mut formula) = inline_math_content(r"x+\frac{a}{b}+z") else {
        return Err("expected formula".into());
    };
    let caret_doc::MathContent::Math(fraction) = &mut formula.math_tree.root.content[2] else {
        return Err("expected fraction".into());
    };
    let ParagraphContent::Text(flow) = paragraph_with_run_text("e\u{301}猫").inlines.remove(0)
    else {
        return Err("expected text".into());
    };
    fraction.children[0].content = vec![caret_doc::MathContent::Text(flow)];
    let math_id = formula.id.clone();
    let paragraph_id = paragraph.id.clone();
    paragraph.inlines.push(ParagraphContent::Math(formula));
    paragraph
        .inlines
        .extend(paragraph_with_run_text("后").inlines);
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let path = MathTextFlowPath::from_interaction_path(
        vec![Step::Content(2), Step::Operand(0), Step::Content(0)],
        None,
    );
    let text = |offset| DocumentRangeEndpoint::InlineMathText {
        paragraph_id: paragraph_id.clone(),
        math_id: math_id.clone(),
        position: MathTextPosition::new(path.clone(), offset, MathPositionAffinity::Downstream),
        entry_direction: Direction::Forward,
        entry_math_forward: true,
    };
    let end = "e\u{301}".len();
    let body = DocumentRangeEndpoint::Text(body_position(&paragraph_id, 0, Affinity::Downstream));
    for reversed in [false, true] {
        let (anchor, focus) = if reversed {
            (text(end), body.clone())
        } else {
            (body.clone(), text(end))
        };
        let range = StructureSelection { anchor, focus };
        let mut session = Session::new(document.clone())?;
        let visual = VisualAffinity::Downstream;
        session.set_pointer_inline_math_selection(range, visual, visual)?;
        let range = session
            .structure_selection
            .clone()
            .ok_or("missing mixed selection")?;
        assert_eq!(session.selected_text()?, "前x+e\u{301}");
        let invalid = StructureSelection {
            anchor: body.clone(),
            focus: text(1),
        };
        assert!(
            session
                .set_pointer_inline_math_selection(invalid, visual, visual)
                .is_err()
        );
        assert_eq!(
            session.structure_selection_range(),
            Some((&range.anchor, &range.focus))
        );
        for replacement in [false, true] {
            let mut edit = session.clone();
            edit.run(if replacement {
                Command::InsertText("替".into())
            } else {
                Command::EditorKey(EditorKey::Delete)
            })?;
            let Block::Paragraph(p) = &edit.document().body[0] else {
                return Err("expected paragraph".into());
            };
            let math = p
                .inlines
                .iter()
                .find_map(|inline| {
                    if let ParagraphContent::Math(math) = inline {
                        Some(math)
                    } else {
                        None
                    }
                })
                .ok_or("formula disappeared")?;
            assert_eq!(math.id, math_id);
            let caret_doc::MathContent::Math(fraction) = &math.math_tree.root.content[0] else {
                return Err("fraction disappeared".into());
            };
            let [caret_doc::MathContent::Text(numerator)] = fraction.children[0].content.as_slice()
            else {
                return Err("missing numerator text".into());
            };
            assert_eq!(
                numerator
                    .runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
                "猫"
            );
            assert_eq!(caret_math::serialize_node(&fraction.children[1]), "b");
            assert_eq!(math.math_tree.root.content.len(), 3);
            assert_eq!(
                p.inlines
                    .iter()
                    .filter_map(|inline| if let ParagraphContent::Text(t) = inline {
                        Some(t.runs.iter().map(|r| r.text.as_str()).collect::<String>())
                    } else {
                        None
                    })
                    .collect::<String>(),
                if replacement { "替后" } else { "后" }
            );
            let changed = edit.document().clone();
            edit.run(Command::Undo)?;
            assert_eq!(edit.document(), &document);
            assert_eq!(
                edit.structure_selection_range(),
                Some((&range.anchor, &range.focus))
            );
            edit.run(Command::Redo)?;
            assert_eq!(edit.document(), &changed);
        }
    }
    let mut session = Session::new(document.clone())?;
    session.set_pointer_inline_math_selection(
        StructureSelection {
            anchor: text(0),
            focus: text(end),
        },
        VisualAffinity::Downstream,
        VisualAffinity::Upstream,
    )?;
    assert_eq!(session.selected_text()?, "e\u{301}");
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: true,
    }))?;
    assert_eq!(session.selected_text()?, "e\u{301}猫");
    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: true,
    }))?;
    assert_eq!(session.selected_text()?, "e\u{301}");
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: false,
    }))?;
    assert_eq!(
        session
            .math_text_input_context()
            .ok_or("missing math caret")?
            .focus
            .offset,
        end
    );
    Ok(())
}

#[test]
fn inline_math_text_copy_is_plain_across_text_and_tree_endpoints()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        DocumentRangeEndpoint, MathPositionAffinity, MathPositionHost, MathSequencePath,
        MathSequenceStep, MathTextFlowPath, MathTextFlowPathStep as Step, MathTextPosition,
        StructureSelection, VisualAffinity,
    };
    let mut paragraph = paragraph_with_run_text("");
    let ParagraphContent::Math(mut formula) = inline_math_content(r"\frac{a}{b}") else {
        return Err("expected formula".into());
    };
    let caret_doc::MathContent::Math(fraction) = &mut formula.math_tree.root.content[0] else {
        return Err("expected fraction".into());
    };
    let ParagraphContent::Text(text_flow) = paragraph_with_run_text("ab").inlines.remove(0) else {
        return Err("expected text flow".into());
    };
    fraction.children[0].content = vec![caret_doc::MathContent::Text(text_flow)];
    let math_id = formula.id.clone();
    let paragraph_id = paragraph.id.clone();
    paragraph.inlines.push(ParagraphContent::Math(formula));
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let text_path = MathTextFlowPath::from_interaction_path(
        vec![Step::Content(0), Step::Operand(0), Step::Content(0)],
        None,
    );
    let text_range = StructureSelection {
        anchor: DocumentRangeEndpoint::InlineMathText {
            paragraph_id: paragraph_id.clone(),
            math_id: math_id.clone(),
            position: MathTextPosition::new(text_path.clone(), 0, MathPositionAffinity::Downstream),
            entry_direction: Direction::Forward,
            entry_math_forward: true,
        },
        focus: DocumentRangeEndpoint::InlineMathText {
            paragraph_id: paragraph_id.clone(),
            math_id: math_id.clone(),
            position: MathTextPosition::new(text_path, 2, MathPositionAffinity::Upstream),
            entry_direction: Direction::Forward,
            entry_math_forward: true,
        },
    };
    let sequence_path = MathSequencePath::new(vec![MathSequenceStep::Operand {
        content_index: 0,
        operand_index: 0,
    }]);
    let tree_position = |offset| crate::MathTreePosition {
        host: MathPositionHost::InlineMath {
            owner_id: math_id.clone(),
        },
        path: sequence_path.clone(),
        offset,
        affinity: MathPositionAffinity::Downstream,
    };
    let tree_range = StructureSelection {
        anchor: DocumentRangeEndpoint::InlineMathTree {
            paragraph_id: paragraph_id.clone(),
            position: tree_position(0),
            entry_direction: Direction::Forward,
            entry_math_forward: true,
        },
        focus: DocumentRangeEndpoint::InlineMathTree {
            paragraph_id,
            position: tree_position(1),
            entry_direction: Direction::Forward,
            entry_math_forward: true,
        },
    };
    for range in [text_range, tree_range] {
        let mut session = Session::new(document.clone())?;
        session.set_pointer_inline_math_selection(
            range,
            VisualAffinity::Downstream,
            VisualAffinity::Upstream,
        )?;
        assert_eq!(session.selected_text()?, "ab");
    }
    Ok(())
}

#[test]
fn mixed_pointer_between_formulae_in_two_paragraphs_preserves_both_cuts()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        DocumentRangeEndpoint, MathPositionAffinity, MathPositionHost, MathSequencePath,
        MathTreePosition, StructureSelection, VisualAffinity,
    };
    let mut document = Document::blank();
    document.body.clear();
    let mut endpoints = Vec::new();
    for (before, formula, after, offset) in [("前", "a+b", "甲", 1), ("乙", "c+d", "后", 2)] {
        let mut paragraph = paragraph_with_run_text(before);
        let ParagraphContent::Math(math) = inline_math_content(formula) else {
            return Err("expected formula".into());
        };
        endpoints.push(DocumentRangeEndpoint::InlineMathTree {
            paragraph_id: paragraph.id.clone(),
            position: MathTreePosition {
                host: MathPositionHost::InlineMath {
                    owner_id: math.id.clone(),
                },
                path: MathSequencePath::root(),
                offset,
                affinity: MathPositionAffinity::Downstream,
            },
            entry_direction: Direction::Forward,
            entry_math_forward: true,
        });
        paragraph.inlines.push(ParagraphContent::Math(math));
        paragraph
            .inlines
            .extend(paragraph_with_run_text(after).inlines);
        document.body.push(Block::Paragraph(paragraph));
    }
    for (anchor, focus) in [
        (&endpoints[0], &endpoints[1]),
        (&endpoints[1], &endpoints[0]),
    ] {
        let mut session = Session::new(document.clone())?;
        session.set_pointer_inline_math_selection(
            StructureSelection {
                anchor: anchor.clone(),
                focus: focus.clone(),
            },
            VisualAffinity::Downstream,
            VisualAffinity::Upstream,
        )?;
        let installed = session
            .structure_selection
            .clone()
            .ok_or("missing mixed selection")?;
        assert_eq!(session.selected_text()?, "+b甲\n乙c+");
        session.run(Command::InsertText("替".into()))?;
        let [Block::Paragraph(remaining)] = session.document().body.as_slice() else {
            return Err("paragraphs should merge".into());
        };
        // 跨宿主替换使用合并后的正文切口，保留左右两个公式及其未选中的内容。
        let rendered: String = remaining
            .inlines
            .iter()
            .map(|item| match item {
                ParagraphContent::Text(t) => t.runs.iter().map(|r| r.text.as_str()).collect(),
                ParagraphContent::Math(m) => {
                    format!("<{}>", caret_math::serialize_tree(&m.math_tree))
                }
                _ => String::new(),
            })
            .collect();
        assert_eq!(rendered, "前<a>替<d>后");
        let after = session.document().clone();
        session.run(Command::Undo)?;
        assert_eq!(session.document(), &document);
        assert_eq!(
            session.structure_selection_range(),
            Some((&installed.anchor, &installed.focus))
        );
        session.run(Command::Redo)?;
        assert_eq!(session.document(), &after);
    }
    Ok(())
}

#[test]
fn extending_a_math_anchor_over_the_next_block_preserves_the_formula_suffix()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        DocumentRangeEndpoint, MathPositionAffinity, MathPositionHost, MathSequencePath,
        MathTreePosition, StructureSelection, VisualAffinity,
    };
    let mut paragraph = paragraph_with_run_text("甲");
    let ParagraphContent::Math(math) = inline_math_content("a+b") else {
        return Err("expected formula".into());
    };
    let anchor = DocumentRangeEndpoint::InlineMathTree {
        paragraph_id: paragraph.id.clone(),
        position: MathTreePosition {
            host: MathPositionHost::InlineMath {
                owner_id: math.id.clone(),
            },
            path: MathSequencePath::root(),
            offset: 1,
            affinity: MathPositionAffinity::Downstream,
        },
        entry_direction: Direction::Forward,
        entry_math_forward: true,
    };
    paragraph.inlines.push(ParagraphContent::Math(math));
    let focus = DocumentRangeEndpoint::Text(body_position(
        &paragraph.id,
        "甲\u{fffc}".len(),
        Affinity::Upstream,
    ));
    let (equation, id) = equation_block_for_unit_navigation();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph),
        equation,
        Block::Paragraph(paragraph_with_run_text("末")),
    ];
    let mut session = Session::new(document.clone())?;
    session.set_pointer_inline_math_selection(
        StructureSelection {
            anchor: anchor.clone(),
            focus,
        },
        VisualAffinity::Downstream,
        VisualAffinity::Upstream,
    )?;
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: true,
    }))?;
    assert_eq!(session.selected_text()?, "+b\n\u{fffc}");
    let focus = DocumentRangeEndpoint::BlockEdge {
        block_id: id,
        side: Direction::Forward,
    };
    assert_eq!(session.structure_selection_range(), Some((&anchor, &focus)));
    session.run(Command::EditorKey(EditorKey::Delete))?;
    assert_eq!(session.document().body.len(), 2);
    session.run(Command::Undo)?;
    assert_eq!(session.document(), &document);
    assert_eq!(session.structure_selection_range(), Some((&anchor, &focus)));
    assert_eq!(session.selected_text()?, "+b\n\u{fffc}");
    Ok(())
}

#[test]
fn pointer_math_ranges_extend_in_gesture_direction_after_reverse_or_rtl_drag()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        DocumentRangeEndpoint, MathPositionAffinity, MathPositionHost, MathSequencePath,
        MathSequenceStep, MathTextFlowPath, MathTextFlowPathStep as Step, MathTextPosition,
        MathTreePosition, StructureSelection, VisualAffinity,
    };
    let mut paragraph = paragraph_with_run_text("");
    let ParagraphContent::Math(mut math) = inline_math_content(r"\frac{yzw}{b}") else {
        return Err("missing formula".into());
    };
    let caret_doc::MathContent::Math(fraction) = &mut math.math_tree.root.content[0] else {
        return Err("missing fraction".into());
    };
    let ParagraphContent::Text(flow) = paragraph_with_run_text("abcd").inlines.remove(0) else {
        return Err("missing text flow".into());
    };
    fraction.children[0]
        .content
        .insert(0, caret_doc::MathContent::Text(flow));
    fraction.children[1].content = vec![caret_doc::MathContent::Text(TextFlow {
        runs: vec![Run {
            text: "q".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    })];
    let paragraph_id = paragraph.id.clone();
    let math_id = math.id.clone();
    paragraph.inlines.push(ParagraphContent::Math(math));
    let mut doc = Document::blank();
    doc.body = vec![Block::Paragraph(paragraph)];
    let text = |offset, direction, math_forward| DocumentRangeEndpoint::InlineMathText {
        paragraph_id: paragraph_id.clone(),
        math_id: math_id.clone(),
        position: MathTextPosition::new(
            MathTextFlowPath::from_interaction_path(
                vec![Step::Content(0), Step::Operand(0), Step::Content(0)],
                None,
            ),
            offset,
            MathPositionAffinity::Downstream,
        ),
        entry_direction: direction,
        entry_math_forward: math_forward,
    };
    let tree = |offset, direction, math_forward| DocumentRangeEndpoint::InlineMathTree {
        paragraph_id: paragraph_id.clone(),
        position: MathTreePosition {
            host: MathPositionHost::InlineMath {
                owner_id: math_id.clone(),
            },
            path: MathSequencePath::new(vec![MathSequenceStep::Operand {
                content_index: 0,
                operand_index: 0,
            }]),
            offset,
            affinity: MathPositionAffinity::Downstream,
        },
        entry_direction: direction,
        entry_math_forward: math_forward,
    };
    let right = Direction::Forward;
    let left = Direction::Backward;
    let key = |direction| {
        Command::EditorKey(if direction == right {
            EditorKey::RightArrow {
                candidate: None,
                extend: true,
            }
        } else {
            EditorKey::LeftArrow {
                candidate: None,
                extend: true,
            }
        })
    };
    // 指针端点保存焦点所在数学局部方向；RTL 的正文锚点不决定公式内左右映射。
    for (anchor, focus, direction, initial, extended) in [
        (
            tree(2, right, true),
            text(3, right, false),
            right,
            "dy",
            "cdy",
        ),
        (text(3, left, true), tree(2, left, true), left, "dy", "dyz"),
        (
            tree(3, right, true),
            tree(2, right, false),
            right,
            "z",
            "yz",
        ),
    ] {
        let mut session = Session::new(doc.clone())?;
        session.set_pointer_inline_math_selection(
            StructureSelection { anchor, focus },
            VisualAffinity::Downstream,
            VisualAffinity::Upstream,
        )?;
        let original = session
            .structure_selection
            .clone()
            .ok_or("missing pointer range")?;
        assert_eq!(session.selected_text()?, initial);
        session.run(key(direction))?;
        assert_eq!(session.selected_text()?, extended);
        assert_eq!(
            session.structure_selection_range().map(|(a, _)| a),
            Some(&original.anchor)
        );
        session.run(key(if direction == right { left } else { right }))?;
        assert_eq!(session.selected_text()?, initial);
    }

    let mut session = Session::new(doc.clone())?;
    let range = StructureSelection {
        anchor: tree(2, right, true),
        focus: text(3, right, true),
    };
    session.set_pointer_inline_math_selection(
        range.clone(),
        VisualAffinity::Downstream,
        VisualAffinity::Upstream,
    )?;
    let other_island = MathTextPosition::new(
        MathTextFlowPath::from_interaction_path(
            vec![Step::Content(0), Step::Operand(1), Step::Content(0)],
            None,
        ),
        1,
        MathPositionAffinity::Downstream,
    );
    let generation = NavigationGeneration {
        session_id: session.session_id().clone(),
        document_generation: session.document_generation(),
        font_generation: 0,
    };
    let selection_before = session
        .structure_selection_range()
        .map(|(a, f)| (a.clone(), f.clone()));
    let document_before = session.document().clone();
    let invalid_same_island = MathTextPosition::new(
        MathTextFlowPath::from_interaction_path(
            vec![Step::Content(0), Step::Operand(0), Step::Content(0)],
            None,
        ),
        99,
        MathPositionAffinity::Downstream,
    );
    for candidate_position in [other_island, invalid_same_island] {
        let invalid_candidate = NavigationCandidate::MathText {
            focus: candidate_position,
            visual_affinity: VisualAffinity::Downstream,
            generation: generation.clone(),
        };
        assert!(
            session
                .run(Command::EditorKey(EditorKey::RightArrow {
                    candidate: Some(invalid_candidate),
                    extend: true,
                }))
                .is_err()
        );
        assert_eq!(
            session
                .structure_selection_range()
                .map(|(a, f)| (a.clone(), f.clone())),
            selection_before
        );
    }
    assert_eq!(session.document(), &document_before);
    Ok(())
}

#[test]
fn selected_math_symbols_and_text_keep_control_words_and_color_scope()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        DocumentRangeEndpoint, MathPositionAffinity, MathPositionHost, MathSequencePath,
        MathSequenceStep, MathTextFlowPath, MathTextFlowPathStep as Step, MathTextPosition,
        MathTreePosition, StructureSelection, VisualAffinity,
    };
    for (source, expected) in [
        (r"\frac{\alpha}{b}", r"\alpha "),
        (r"\frac{\color{red}x}{b}", r"\textcolor{red}{x}"),
    ] {
        let mut paragraph = paragraph_with_run_text("");
        let ParagraphContent::Math(mut math) = inline_math_content(source) else {
            return Err("missing formula".into());
        };
        let caret_doc::MathContent::Math(fraction) = &mut math.math_tree.root.content[0] else {
            return Err("missing fraction".into());
        };
        for text in ["a", "bc"] {
            let ParagraphContent::Text(flow) = paragraph_with_run_text(text).inlines.remove(0)
            else {
                return Err("missing text flow".into());
            };
            fraction.children[0]
                .content
                .push(caret_doc::MathContent::Text(flow));
        }
        let math_id = math.id.clone();
        let paragraph_id = paragraph.id.clone();
        paragraph.inlines.push(ParagraphContent::Math(math));
        let mut document = Document::blank();
        document.body = vec![Block::Paragraph(paragraph)];
        let tree = |offset| DocumentRangeEndpoint::InlineMathTree {
            paragraph_id: paragraph_id.clone(),
            position: MathTreePosition {
                host: MathPositionHost::InlineMath {
                    owner_id: math_id.clone(),
                },
                path: MathSequencePath::new(vec![MathSequenceStep::Operand {
                    content_index: 0,
                    operand_index: 0,
                }]),
                offset,
                affinity: MathPositionAffinity::Downstream,
            },
            entry_direction: Direction::Forward,
            entry_math_forward: true,
        };
        let partial = DocumentRangeEndpoint::InlineMathText {
            paragraph_id: paragraph_id.clone(),
            math_id: math_id.clone(),
            position: MathTextPosition::new(
                MathTextFlowPath::from_interaction_path(
                    vec![Step::Content(0), Step::Operand(0), Step::Content(2)],
                    None,
                ),
                1,
                MathPositionAffinity::Downstream,
            ),
            entry_direction: Direction::Forward,
            entry_math_forward: true,
        };
        for (focus, suffix) in [(tree(3), "abc"), (partial, "ab")] {
            let mut session = Session::new(document.clone())?;
            session.set_pointer_inline_math_selection(
                StructureSelection {
                    anchor: tree(0),
                    focus,
                },
                VisualAffinity::Downstream,
                VisualAffinity::Upstream,
            )?;
            assert_eq!(session.selected_text()?, format!("{expected}{suffix}"));
        }
    }
    Ok(())
}
