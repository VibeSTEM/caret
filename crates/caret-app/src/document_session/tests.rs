//! 验证文档会话打开、编辑和保存状态。

use super::DocumentSessionService;
use caret_doc::CdocFile;
use serde_json::Value;
use serde_json::json;

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
    value["document"]["body"][0]["inlines"] = json!([{
        "type": "inlineMath",
        "id": "inline-math",
        "mathTree": {
            "schemaVersion": 1,
            "source": "x",
            "root": {
                "id": "root",
                "kind": "row",
                "children": [{"id": "x", "kind": "symbol", "text": "x"}]
            }
        }
    }]);
    value["requiredFeatures"] = json!(["caret.math.inline", "caret.math.syntax.v1"]);
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;

    let result = DocumentSessionService::prepare_open(
        &bytes,
        std::path::Path::new("math.cdoc"),
        service.session_generation() + 1,
    );

    assert!(matches!(result, Err(ref error) if error == "cdoc.safe_preview_read_only"));
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
        saved["document"]["body"][0]["inlines"][0]["text"],
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
    assert_eq!(saved["document"]["body"][0]["inlines"][0]["text"], "edited");
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
