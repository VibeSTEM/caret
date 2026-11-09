use super::super::super::{
    ClipboardAction, Command, Direction, EditorKey, MathEditBranch, NavigationCandidate,
    NavigationGeneration, Session,
};
use caret_doc::Document;

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
            document_generation: session.document_generation().saturating_sub(1),
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

#[test]
fn shift_horizontal_navigation_does_not_exit_math_text_at_text_island_edges() {
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
        .run(Command::SetMathTextSelection {
            anchor: 0,
            focus: 0,
        })
        .unwrap_or_else(|error| panic!("math text selection failed: {error}"));
    let initial_position = session
        .math_text_input_context()
        .unwrap_or_else(|| panic!("math text context missing"))
        .focus;

    session
        .run(Command::EditorKey(EditorKey::LeftArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-left at text edge failed: {error}"));
    assert!(session.math_edit().is_some());
    assert_eq!(
        session
            .math_text_input_context()
            .map(|context| context.focus),
        Some(initial_position.clone())
    );

    session
        .run(Command::SetMathTextSelection {
            anchor: 2,
            focus: 2,
        })
        .unwrap_or_else(|error| panic!("math text selection failed: {error}"));
    let end_position = session
        .math_text_input_context()
        .unwrap_or_else(|| panic!("math text context missing"))
        .focus;
    session
        .run(Command::EditorKey(EditorKey::RightArrow {
            candidate: None,
            extend: true,
        }))
        .unwrap_or_else(|error| panic!("shift-right at text edge failed: {error}"));
    assert!(session.math_edit().is_some());
    assert_eq!(
        session
            .math_text_input_context()
            .map(|context| context.focus),
        Some(end_position)
    );
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
