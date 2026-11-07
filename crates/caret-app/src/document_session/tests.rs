//! 验证文档会话打开、编辑和保存状态。

use super::DocumentSessionService;
use caret_doc::{Block, CdocFile, OpenedDocument, ParagraphContent};
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
fn math_carriers_open_save_and_reopen_through_document_session_and_disk() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    let blank = service.save_snapshot()?;
    let mut value: Value =
        serde_json::from_slice(&blank.bytes).map_err(|error| error.to_string())?;
    value["futureEnvelopeData"] = json!({"retained": true});
    value["document"]["body"] = json!([
        {
            "type": "paragraph", "id": "paragraph", "vendorParagraph": "kept",
            "inlines": [{"type": "inlineMath", "id": "inline-math", "mathTree": {
                "schemaVersion": 3,
                "vendorTree": "inline-tree-kept",
                "root": {"id": "inline-root", "content": [
                    {"type": "environment", "scope": "inline", "id": "inline-environment",
                     "kind": "bold", "vendorEnvironment": "kept", "children": [
                        {"type": "math", "id": "inline-x", "kind": "symbol", "text": "x"}
                    ]}
                ]}
            }}]
        },
        {
            "type": "equation", "id": "equation", "vendorEquation": {"kept": true}, "mathTree": {
                "schemaVersion": 3,
                "vendorEquationTree": "kept",
                "root": {"id": "equation-root", "content": [
                    {"type": "text", "vendorText": "kept", "runs": [
                        {"text": "f", "language": "en", "props": {"underline": true}, "vendorRun": "kept"}
                    ]},
                    {"type": "math", "id": "equation-y", "kind": "symbol", "text": "y", "vendorNode": "kept"}
                ]}
            }
        },
        {
            "type": "align", "id": "align", "vendorAlign": "kept", "rows": [{
                "id": "align-row", "vendorRow": "kept", "cells": [{"schemaVersion": 3, "vendorCellTree": "kept", "root": {
                    "id": "align-cell-root", "content": [
                        {"type": "text", "runs": [{"text": "g", "props": {"underline": true}}]},
                        {"type": "math", "id": "align-z", "kind": "symbol", "text": "z", "vendorAlignNode": "kept"}
                    ]
                }}]
            }, {"id": "align-row-2", "cells": [{"schemaVersion": 3, "root": {
                "id": "align-cell-root-2", "content": [
                    {"type": "math", "id": "align-w", "kind": "symbol", "text": "w"}
                ]
            }}]}]
        }
    ]);
    value["requiredFeatures"] = json!([
        "caret.env.bold",
        "caret.math.align",
        "caret.math.content-flow.v2",
        "caret.math.equation",
        "caret.math.inline",
        "caret.math.syntax.v1",
        "caret.paragraph.content-flow.v1",
        "caret.paragraph.layout-settings",
        "caret.text.default-font-size",
        "caret.text.language-tags",
        "caret.text.underline"
    ]);
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    let opened = caret_doc::open(&bytes).map_err(|error| error.to_string())?;
    assert!(
        matches!(&opened, OpenedDocument::Editable(_)),
        "math carrier fixture should be editable: {opened:?}"
    );
    let expected_body =
        serde_json::to_value(opened.document()).map_err(|error| error.to_string())?["body"].clone();

    let root = std::env::temp_dir().join(format!(
        "caret-math-session-roundtrip-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let path = root.join("math.cdoc");
    service.open_bytes(
        &bytes,
        &path,
        blank.session_generation,
        blank.document_generation,
    )?;
    assert!(!service.is_dirty());

    let save = service.save_snapshot()?;
    crate::document_storage::DocumentStorage
        .save_snapshot(&path, &save.bytes)
        .map_err(|error| error.to_string())?;
    assert!(service.mark_saved(
        save.session_generation,
        save.document_generation,
        save.history_state_id,
        &path,
        &save.content_digest,
    ));
    assert!(!service.is_dirty());

    let disk_file = crate::document_storage::DocumentStorage
        .open_file(&path)
        .map_err(|error| error.to_string())?;
    let OpenedDocument::Editable(disk_file) = disk_file else {
        return Err("saved math carriers should reopen as editable".to_owned());
    };
    assert_eq!(
        serde_json::to_value(disk_file.document()).map_err(|error| error.to_string())?["body"],
        expected_body
    );
    let saved_envelope: Value = serde_json::from_slice(
        &crate::document_storage::DocumentStorage
            .read_file_bytes(&path)
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        saved_envelope["futureEnvelopeData"],
        json!({"retained": true})
    );

    let reopened_bytes = crate::document_storage::DocumentStorage
        .read_file_bytes(&path)
        .map_err(|error| error.to_string())?;
    let mut reopened = DocumentSessionService::new_blank()?;
    reopened.open_bytes(&reopened_bytes, &path, 1, 0)?;
    assert!(!reopened.is_dirty());
    assert_eq!(
        serde_json::to_value(reopened.view.document()).map_err(|error| error.to_string())?["body"],
        expected_body
    );
    let reopened_document =
        serde_json::to_value(reopened.view.document()).map_err(|error| error.to_string())?;
    assert_eq!(
        reopened_document["body"][0]["inlines"][0]["mathTree"]["root"]["content"][0]["id"],
        "inline-environment"
    );
    assert_eq!(
        reopened_document["body"][1]["mathTree"]["root"]["content"][0]["runs"][0]["props"]["underline"],
        true
    );
    assert_eq!(
        reopened_document["body"][1]["vendorEquation"],
        json!({"kept": true})
    );
    assert_eq!(reopened_document["body"][2]["rows"][1]["id"], "align-row-2");
    assert_eq!(
        reopened_document["body"][2]["rows"][0]["cells"][0]["root"]["id"],
        "align-cell-root"
    );
    assert_eq!(
        reopened_document["body"][2]["rows"][0]["cells"][0]["vendorCellTree"],
        "kept"
    );
    std::fs::remove_dir_all(root).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn failed_math_edit_save_preserves_file_session_and_retries_cleanly() -> Result<(), String> {
    let mut service = DocumentSessionService::new_blank()?;
    enter_inline_math(&mut service)?;

    let root =
        std::env::temp_dir().join(format!("caret-math-save-conflict-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let path = root.join("math.cdoc");

    let base = service.save_snapshot()?;
    crate::document_storage::DocumentStorage
        .save_snapshot(&path, &base.bytes)
        .map_err(|error| error.to_string())?;
    assert!(service.mark_saved(
        base.session_generation,
        base.document_generation,
        base.history_state_id,
        &path,
        &base.content_digest,
    ));
    assert!(!service.is_dirty());

    type_math_text(&mut service)?;
    let edited = service.save_snapshot()?;
    let edited_generation = service.document_generation();
    let saved_snapshot = service.saved_generation;
    let saved_history = service.saved_history_state_id;
    let saved_path = service.path.clone();
    let saved_baseline = service.baseline_digest.clone();
    let external_bytes = b"external writer changed this file";
    std::fs::write(&path, external_bytes).map_err(|error| error.to_string())?;

    let conflict = crate::document_storage::DocumentStorage.save_snapshot_if_unchanged(
        &path,
        &edited.bytes,
        &base.content_digest,
    );
    assert!(matches!(
        conflict,
        Err(crate::document_storage::DocumentStorageError::ExternalConflict)
    ));
    assert_eq!(
        std::fs::read(&path).map_err(|error| error.to_string())?,
        external_bytes
    );
    assert!(service.is_dirty());
    assert_eq!(service.document_generation(), edited_generation);
    assert_eq!(service.saved_generation, saved_snapshot);
    assert_eq!(service.saved_history_state_id, saved_history);
    assert_eq!(service.path, saved_path);
    assert_eq!(service.baseline_digest, saved_baseline);

    let external_digest = super::content_digest(external_bytes);
    crate::document_storage::DocumentStorage
        .save_snapshot_if_unchanged(&path, &edited.bytes, &external_digest)
        .map_err(|error| error.to_string())?;
    assert!(service.mark_saved(
        edited.session_generation,
        edited.document_generation,
        edited.history_state_id,
        &path,
        &edited.content_digest,
    ));
    assert!(!service.is_dirty());

    let reopened = crate::document_storage::DocumentStorage
        .open_file(&path)
        .map_err(|error| error.to_string())?;
    let OpenedDocument::Editable(reopened) = reopened else {
        return Err("retried math save should reopen as editable".to_owned());
    };
    let reopened_body = serde_json::to_value(reopened.document())
        .map_err(|error| error.to_string())?["body"]
        .clone();
    let edited_body = serde_json::from_slice::<Value>(&edited.bytes)
        .map_err(|error| error.to_string())?["document"]["body"]
        .clone();
    assert_eq!(reopened_body, edited_body);

    std::fs::remove_dir_all(root).map_err(|error| error.to_string())?;
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
    let snapshot = service.save_snapshot()?;
    assert_eq!(snapshot.document_generation, service.document_generation());
    let saved_file = caret_doc::open(&snapshot.bytes).map_err(|error| error.to_string())?;
    assert!(matches!(saved_file, OpenedDocument::Editable(_)));
    assert!(
        serde_json::from_slice::<Value>(&snapshot.bytes)
            .map_err(|error| error.to_string())?["requiredFeatures"]
            .as_array()
            .is_some_and(|features| features.iter().any(|feature| feature == "caret.math.inline"))
    );
    let after_snapshot: Value =
        serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
    assert_eq!(after_snapshot["dirty"], true);
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
