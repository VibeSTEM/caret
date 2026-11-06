//! 验证文档会话打开、编辑和保存状态。

use super::DocumentSessionService;
use caret_doc::{Block, CdocFile, OpenedDocument, ParagraphContent, PreviewReason};
use caret_engine::{Command, Direction};
use serde_json::Value;
use serde_json::json;

fn enter_inline_math(service: &mut DocumentSessionService) -> Result<(), String> {
    let session = service.view_mut().session_mut_for_test();
    session
        .run(Command::InsertInlineMath("x".to_owned()))
        .map_err(|error| error.to_string())?;
    session
        .run(Command::EnterMath(Direction::Backward))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn type_math_text(service: &mut DocumentSessionService) -> Result<(), String> {
    let session = service.view_mut().session_mut_for_test();
    session
        .run(Command::InsertText("a".to_owned()))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn mark_current_history_saved_for_test(service: &mut DocumentSessionService) {
    service.saved_history_state_id = service.view.history_state_id();
    service.has_save_point = true;
}

#[test]
fn document_session_can_be_moved_between_worker_and_gui_threads() {
    fn assert_send<T: Send>() {}
    assert_send::<DocumentSessionService>();
}

#[test]
fn failed_open_preserves_the_active_session_and_save_snapshot() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    let original_snapshot = service.save_snapshot()?;
    let original_status: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;

    let result = service.open_bytes(b"not json", std::path::Path::new("bad.cdoc"), 1, 0);

    assert!(result.is_err());
    assert_eq!(service.save_snapshot()?, original_snapshot);
    let current_status: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
    assert_eq!(current_status, original_status);
    Ok(())
}

#[test]
fn rejected_snapshot_does_not_split_the_next_typing_undo_group() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    service.view_mut().insert_text_json("a".to_owned())?;
    service
        .file
        .required_features
        .push("vendor.future".to_owned());

    assert_eq!(
        service.save_snapshot(),
        Err("cdoc.safe_preview_read_only".to_owned())
    );
    service.view_mut().insert_text_json("b".to_owned())?;
    let paragraph_text = |document: &caret_doc::Document| {
        document
            .body
            .iter()
            .find_map(|block| match block {
                Block::Paragraph(paragraph) => Some(
                    paragraph
                        .inlines
                        .iter()
                        .filter_map(|content| match content {
                            ParagraphContent::Text(text) => Some(
                                text.runs
                                    .iter()
                                    .map(|run| run.text.as_str())
                                    .collect::<String>(),
                            ),
                            ParagraphContent::Math(_)
                            | ParagraphContent::Environment(_)
                            | ParagraphContent::SoftBreak(_)
                            | ParagraphContent::Opaque(_) => None,
                        })
                        .collect::<String>(),
                ),
                Block::Equation(_)
                | Block::Align(_)
                | Block::Environment(_)
                | Block::ExplicitBreak(_)
                | Block::Opaque(_) => None,
            })
            .unwrap_or_default()
    };
    assert_eq!(paragraph_text(service.view.document()), "ab");
    service
        .view_mut()
        .session_mut_for_test()
        .run(Command::Undo)
        .map_err(|error| error.to_string())?;

    assert_eq!(paragraph_text(service.view.document()), "");
    Ok(())
}

#[test]
fn unsupported_required_feature_cannot_replace_the_current_session() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    service.view_mut().insert_text_json("existing".to_owned())?;
    let before = service.save_snapshot()?;
    let mut value: Value =
        serde_json::from_slice(&before.bytes).map_err(|error| error.to_string())?;
    value["requiredFeatures"] = json!(["vendor.future"]);
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;

    let result = DocumentSessionService::prepare_open(
        &bytes,
        std::path::Path::new("future.cdoc"),
        service.session_generation() + 1,
    );
    assert!(matches!(result, Err(ref error) if error == "cdoc.safe_preview_read_only"));
    assert_eq!(service.document_generation(), before.document_generation);
    assert_eq!(service.save_snapshot()?, before);
    Ok(())
}

#[test]
fn math_feature_stays_read_only_until_formula_layout_is_supported() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    service.view_mut().insert_text_json("existing".to_owned())?;
    let before = service.save_snapshot()?;
    let mut value: Value =
        serde_json::from_slice(&before.bytes).map_err(|error| error.to_string())?;
    value["document"]["body"] = json!([{
        "type": "paragraph",
        "id": "paragraph",
        "inlines": [{
        "type": "inlineMath",
        "id": "inline-math",
        "mathTree": {
            "schemaVersion": 3,
            "root": {
                "id": "root",
                "content": [{"type": "math", "id": "x", "kind": "symbol", "text": "x"}]
            }
        }
    }]}]);
    value["requiredFeatures"] = json!([
        "caret.math.content-flow.v2",
        "caret.math.inline",
        "caret.math.syntax.v1",
        "caret.paragraph.content-flow.v1",
        "caret.paragraph.layout-settings",
        "caret.text.default-font-size"
    ]);
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;

    let opened = caret_doc::open(&bytes).map_err(|error| error.to_string())?;
    let OpenedDocument::SafePreview(preview) = opened else {
        return Err("unsupported math layout should remain a safe preview".to_owned());
    };
    let Some(Block::Paragraph(paragraph)) = preview.document().body.first() else {
        return Err("safe preview should retain the paragraph".to_owned());
    };
    if !matches!(paragraph.inlines.first(), Some(ParagraphContent::Math(_)))
        || !preview
            .reasons
            .contains(&PreviewReason::UnsupportedRequiredFeature(
                "caret.math.inline".to_owned(),
            ))
    {
        return Err(
            "safe preview should retain the known MathTree and explain its capability".to_owned(),
        );
    }

    let result = DocumentSessionService::prepare_open(
        &bytes,
        std::path::Path::new("math.cdoc"),
        service.session_generation() + 1,
    );

    assert!(
        matches!(result, Err(ref error) if error == "cdoc.safe_preview_read_only"),
        "unexpected result: {}",
        result.err().unwrap_or_default()
    );
    assert_eq!(service.document_generation(), before.document_generation);
    assert_eq!(service.save_snapshot()?, before);
    Ok(())
}

#[test]
fn prepared_open_rejects_a_document_edited_while_it_was_loading() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    let before = service.save_snapshot()?;
    let (candidate, layout) = DocumentSessionService::prepare_open(
        &before.bytes,
        std::path::Path::new("opened.cdoc"),
        before.session_generation + 1,
    )?;
    service.view_mut().insert_text_json("new edit".to_owned())?;

    let result = service.install_prepared(
        candidate,
        layout,
        before.session_generation,
        before.document_generation,
    );
    assert_eq!(result.err().as_deref(), Some("document.open_stale_session"));
    assert_eq!(service.session_generation(), before.session_generation);
    assert_eq!(
        service.document_generation(),
        before.document_generation + 1
    );
    let snapshot = service.save_snapshot()?;
    let saved: Value =
        serde_json::from_slice(&snapshot.bytes).map_err(|error| error.to_string())?;
    assert_eq!(
        saved["document"]["body"][0]["inlines"][0]["runs"][0]["text"],
        "new edit"
    );
    Ok(())
}

#[test]
fn late_save_completion_cannot_move_the_saved_revision_backwards() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    service.view_mut().insert_text_json("first".to_owned())?;
    let earlier = service.save_snapshot()?;
    service.view_mut().insert_text_json(" second".to_owned())?;
    let later = service.save_snapshot()?;
    let path = std::path::Path::new("document.cdoc");

    assert!(service.mark_saved(
        later.session_generation,
        later.document_generation,
        later.history_state_id,
        path,
        &later.content_digest
    ));
    assert!(service.mark_saved(
        earlier.session_generation,
        earlier.document_generation,
        earlier.history_state_id,
        std::path::Path::new("older.cdoc"),
        &earlier.content_digest
    ));
    let status: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
    assert_eq!(
        status["savedGeneration"],
        later.document_generation.to_string()
    );
    assert_eq!(status["path"], "document.cdoc");
    assert_eq!(status["dirty"], false);
    Ok(())
}

#[test]
fn revision_epoch_resets_saved_generation_and_accepts_zero_revision_save() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    service.view_mut().insert_text_json("dirty".to_owned())?;
    let old_session_generation = service.session_generation();
    let history_state_id = service.view.history_state_id();
    service.saved_generation = u64::MAX - 1;
    service.saved_history_state_id = history_state_id.saturating_sub(1);
    service.path = Some(std::path::PathBuf::from("old.cdoc"));
    service.baseline_digest = Some("old-digest".to_owned());
    service.has_save_point = true;
    assert!(service.is_dirty());
    assert_eq!(service.saved_generation, u64::MAX - 1);

    let Some(new_session_generation) = service.advance_session_generation() else {
        return Err("session generation should advance".to_owned());
    };
    assert_eq!(new_session_generation, old_session_generation + 1);
    assert_eq!(service.saved_generation, 0);
    assert!(!service.mark_saved(
        old_session_generation,
        u64::MAX - 1,
        history_state_id,
        std::path::Path::new("stale.cdoc"),
        "stale-digest",
    ));
    assert!(service.mark_saved(
        new_session_generation,
        0,
        history_state_id,
        std::path::Path::new("new.cdoc"),
        "new-digest",
    ));

    let status: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
    assert_eq!(
        status["sessionGeneration"],
        new_session_generation.to_string()
    );
    assert_eq!(status["savedGeneration"], "0");
    assert_eq!(status["path"], "new.cdoc");
    assert_eq!(status["dirty"], false);
    assert_eq!(service.baseline_digest.as_deref(), Some("new-digest"));
    Ok(())
}

#[test]
fn exhausted_session_epoch_still_resets_saved_generation_baseline() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    service.session_generation = u64::MAX;
    service.saved_generation = u64::MAX - 1;
    service.saved_history_state_id = 7;
    service.path = Some(std::path::PathBuf::from("existing.cdoc"));
    service.baseline_digest = Some("existing-digest".to_owned());

    assert_eq!(service.advance_session_generation(), None);
    assert_eq!(service.session_generation(), u64::MAX);
    assert_eq!(service.saved_generation, 0);
    assert_eq!(service.saved_history_state_id, 7);
    assert_eq!(
        service.path.as_deref(),
        Some(std::path::Path::new("existing.cdoc"))
    );
    assert_eq!(service.baseline_digest.as_deref(), Some("existing-digest"));
    Ok(())
}

#[test]
fn save_completion_from_a_future_revision_epoch_is_rejected() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    assert!(!service.mark_saved(
        service.session_generation(),
        u64::MAX,
        1,
        std::path::Path::new("stale.cdoc"),
        "stale-digest",
    ));
    assert_eq!(service.saved_generation, 0);
    assert!(service.path.is_none());
    assert!(service.baseline_digest.is_none());
    assert!(!service.is_dirty());
    Ok(())
}

#[test]
fn save_completion_only_marks_the_captured_revision_saved() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    service.view_mut().layout_snapshot_json()?;
    service.view_mut().insert_text_json("first".to_owned())?;
    let captured = service.save_snapshot()?;
    service.view_mut().insert_text_json(" later".to_owned())?;

    assert!(service.mark_saved(
        captured.session_generation,
        captured.document_generation,
        captured.history_state_id,
        std::path::Path::new("document.cdoc"),
        &captured.content_digest,
    ));
    let status: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
    assert_eq!(status["dirty"], true);
    assert_eq!(status["fileName"], "document.cdoc");
    assert_eq!(
        status["savedGeneration"],
        captured.document_generation.to_string()
    );
    Ok(())
}

#[test]
fn undo_to_saved_history_state_restores_saved_status_and_redo_marks_edited() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    service.view_mut().insert_text_json("saved".to_owned())?;
    let saved_snapshot = service.save_snapshot()?;
    assert!(service.mark_saved(
        saved_snapshot.session_generation,
        saved_snapshot.document_generation,
        saved_snapshot.history_state_id,
        std::path::Path::new("document.cdoc"),
        &saved_snapshot.content_digest,
    ));

    service.view_mut().insert_text_json(" later".to_owned())?;
    let edited: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
    assert_eq!(edited["dirty"], true);

    service.view_mut().undo_json()?;
    let undone: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
    assert_eq!(undone["dirty"], false);
    assert_eq!(
        undone["historyStateId"],
        saved_snapshot.history_state_id.to_string()
    );

    service.view_mut().redo_json()?;
    let redone: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
    assert_eq!(redone["dirty"], true);
    Ok(())
}

#[test]
fn pending_formatting_does_not_mark_saved_document_dirty() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    let saved_snapshot = service.save_snapshot()?;
    assert!(service.mark_saved(
        saved_snapshot.session_generation,
        saved_snapshot.document_generation,
        saved_snapshot.history_state_id,
        std::path::Path::new("document.cdoc"),
        &saved_snapshot.content_digest,
    ));

    for (action, text) in [("begin", ""), ("type", "bold"), ("accept", "")] {
        service
            .view_mut()
            .command_line_command_json(action, text.to_owned())?;
    }

    let formatted: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
    assert_eq!(formatted["dirty"], false);
    assert_eq!(
        formatted["historyStateId"],
        saved_snapshot.history_state_id.to_string()
    );

    service.view_mut().undo_json()?;
    service.view_mut().redo_json()?;
    let restored: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
    assert_eq!(restored["dirty"], false);
    Ok(())
}

#[test]
fn tree_native_math_text_is_dirty_without_a_source_draft() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    enter_inline_math(&mut service)?;
    mark_current_history_saved_for_test(&mut service);
    let saved_history_state_id = service.view.history_state_id();
    type_math_text(&mut service)?;

    let status: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
    assert_eq!(status["dirty"], true);
    assert_eq!(status["mathSourceDraftPending"], false);
    assert!(!service.view_mut().has_uncommitted_math_source_draft());
    assert_ne!(status["historyStateId"], saved_history_state_id.to_string());
    assert_eq!(
        service.save_snapshot().err().as_deref(),
        Some("cdoc.safe_preview_read_only")
    );
    Ok(())
}

#[test]
fn prepared_open_replaces_tree_native_math_edit_without_a_source_draft() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    let saved = service.save_snapshot()?;
    let (candidate, layout) = DocumentSessionService::prepare_open(
        &saved.bytes,
        std::path::Path::new("opened.cdoc"),
        saved.session_generation + 1,
    )?;
    enter_inline_math(&mut service)?;
    mark_current_history_saved_for_test(&mut service);
    type_math_text(&mut service)?;

    let result = service.install_prepared(
        candidate,
        layout,
        saved.session_generation,
        service.document_generation(),
    );
    assert!(result.is_ok());
    assert!(!service.is_dirty());
    Ok(())
}

#[test]
fn delayed_save_completion_from_replaced_session_is_ignored() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    let stale = service.save_snapshot()?;
    let file = CdocFile::new(caret_doc::Document::blank()).map_err(|error| error.to_string())?;
    let bytes = file.to_bytes().map_err(|error| error.to_string())?;
    service.open_bytes(&bytes, std::path::Path::new("opened.cdoc"), 1, 0)?;

    assert!(!service.mark_saved(
        stale.session_generation,
        stale.document_generation,
        stale.history_state_id,
        std::path::Path::new("stale.cdoc"),
        &stale.content_digest,
    ));
    let status: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
    assert_eq!(status["fileName"], "opened.cdoc");
    assert_eq!(status["dirty"], false);
    Ok(())
}

#[test]
fn editing_and_saving_preserves_unknown_optional_envelope_fields() -> Result<(), String> {
    let mut file =
        CdocFile::new(caret_doc::Document::blank()).map_err(|error| error.to_string())?;
    file.extra
        .insert("futureEnvelopeData".to_owned(), json!({"retained": true}));
    let bytes = file.to_bytes().map_err(|error| error.to_string())?;
    let mut service = DocumentSessionService::new_blank()?;
    service.open_bytes(&bytes, std::path::Path::new("future.cdoc"), 1, 0)?;
    service.view_mut().insert_text_json("edited".to_owned())?;

    let snapshot = service.save_snapshot()?;
    let saved: serde_json::Value =
        serde_json::from_slice(&snapshot.bytes).map_err(|error| error.to_string())?;
    assert_eq!(saved["futureEnvelopeData"]["retained"], true);
    assert_eq!(
        saved["document"]["body"][0]["inlines"][0]["runs"][0]["text"],
        "edited"
    );
    Ok(())
}

#[test]
fn recovery_session_stays_dirty_and_retains_its_original_recovery_binding() -> Result<(), String> {
    let file = CdocFile::new(caret_doc::Document::blank()).map_err(|error| error.to_string())?;
    let bytes = file.to_bytes().map_err(|error| error.to_string())?;
    let (mut service, _) = DocumentSessionService::prepare_recovery(
        &bytes,
        "recovery-id".to_owned(),
        "/documents/old.cdoc".to_owned(),
        Some("baseline".to_owned()),
        2,
    )?;
    assert!(service.is_dirty());
    assert_eq!(service.recovery_id(), Some("recovery-id"));
    assert!(service.recovery_binding_matches(
        "recovery-id",
        "/documents/old.cdoc",
        Some("baseline")
    ));
    assert!(!service.recovery_binding_matches(
        "recovery-id",
        "/documents/new.cdoc",
        Some("baseline")
    ));
    assert!(service.status_json().contains("\"path\":\"\""));

    service.view_mut().insert_text_json("new edit".to_owned())?;
    let snapshot = service.save_snapshot()?;
    assert_eq!(snapshot.recovery_id.as_deref(), Some("recovery-id"));
    assert_eq!(
        snapshot.recovery_original_path.as_deref(),
        Some("/documents/old.cdoc")
    );
    assert_eq!(
        snapshot.recovery_baseline_digest.as_deref(),
        Some("baseline")
    );
    assert!(service.is_dirty());
    Ok(())
}
