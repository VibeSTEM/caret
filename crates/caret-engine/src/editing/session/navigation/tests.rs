use super::super::super::{
    ClipboardAction, Command, Direction, EditorKey, NavigationCandidate, NavigationGeneration,
    Session,
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
