//! 恢复条目、冲突快照与恢复会话的 FFI 持久化边界。
use super::{
    DocumentRuntime, PreparedDocument, bridge, next_prepared_document_ticket, prepared_documents,
};
use caret_app::document_session::DocumentSessionService;
use caret_app::document_storage::DocumentStorage;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

fn recovery_storage()
-> &'static Mutex<Option<caret_app::document_recovery::DocumentRecoveryStorage>> {
    static STORAGE: OnceLock<Mutex<Option<caret_app::document_recovery::DocumentRecoveryStorage>>> =
        OnceLock::new();
    STORAGE.get_or_init(|| Mutex::new(None))
}

pub(super) fn recovery_storage_for_cleanup()
-> Result<caret_app::document_recovery::DocumentRecoveryStorage, String> {
    recovery_storage()
        .lock()
        .map_err(|_| "recovery.storage_unavailable".to_owned())?
        .as_ref()
        .cloned()
        .ok_or_else(|| "recovery.storage_unavailable".to_owned())
}

fn recovery_process_lease()
-> &'static Mutex<Option<caret_app::document_recovery::RecoveryProcessLease>> {
    static LEASE: OnceLock<Mutex<Option<caret_app::document_recovery::RecoveryProcessLease>>> =
        OnceLock::new();
    LEASE.get_or_init(|| Mutex::new(None))
}

fn pending_recovery_leases()
-> &'static Mutex<HashMap<String, caret_app::document_recovery::RecoverySessionLease>> {
    static LEASES: OnceLock<
        Mutex<HashMap<String, caret_app::document_recovery::RecoverySessionLease>>,
    > = OnceLock::new();
    LEASES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 成功保存后清理关联恢复项；删除失败时保留绑定供下一次编辑刷新。
pub(super) fn clear_recovery_after_saved(runtime: &mut DocumentRuntime) {
    let id = runtime
        .runtime
        .as_ref()
        .ok()
        .and_then(|session| session.recovery_id().map(str::to_owned));
    if let Some(id) = id {
        let removed = recovery_storage()
            .lock()
            .ok()
            .and_then(|storage| {
                storage
                    .as_ref()
                    .map(|storage| match runtime.recovery_lease.as_ref() {
                        Some(lease) if lease.id() == id => storage.remove_leased(lease),
                        _ => storage.remove(&id),
                    })
            })
            .is_some_and(|result| result.is_ok());
        if removed && let Ok(session) = &mut runtime.runtime {
            session.clear_recovery_id();
            runtime.recovery_lease = None;
        }
    }
    let recovery_enabled = runtime
        .runtime
        .as_ref()
        .is_ok_and(|session| session.recovery_id().is_some());
    runtime.lifecycle.set_recovery_enabled(recovery_enabled);
}

/// 由宿主注入 AppLocalDataLocation；恢复策略仍由 Rust 存储服务管理。
pub(crate) fn configure_document_recovery(root: String) -> String {
    let directory = Path::new(&root).join("recovery");
    match recovery_storage().lock() {
        Ok(mut storage) => {
            let recovery = caret_app::document_recovery::DocumentRecoveryStorage::new(directory);
            *storage = Some(recovery);
            String::new()
        }
        Err(_) => "recovery.storage_unavailable".to_owned(),
    }
}

/// 返回恢复条目的轻量元数据，不读取正文负载。
pub(crate) fn recovery_entries_json() -> String {
    let storage = match recovery_storage().lock() {
        Ok(storage) => storage.as_ref().cloned(),
        Err(_) => return "error:recovery.storage_unavailable".to_owned(),
    };
    let Some(storage) = storage else {
        return "error:recovery.storage_unavailable".to_owned();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = storage.purge_expired(now);
    if let Ok(mut lease) = recovery_process_lease().lock()
        && lease.is_none()
        && let Ok(active) = storage.acquire_process_lease()
    {
        *lease = Some(active);
    }
    let entries = match storage.list_entries() {
        Ok(entries) => entries,
        Err(error) => return format!("error:{error}"),
    };
    let values: Vec<_> = entries
        .into_iter()
        .map(|entry| {
            serde_json::json!({
                "id": entry.id,
                "kind": match entry.kind {
                    caret_app::document_recovery::RecoveryKind::LocalDocument => "local_document",
                    caret_app::document_recovery::RecoveryKind::ExternalVersion => "external_version",
                },
                "originalPath": entry.original_path,
                "baselineDigest": entry.baseline_digest,
                "createdUnixSeconds": entry.created_unix_seconds,
                "updatedUnixSeconds": entry.updated_unix_seconds,
            })
        })
        .collect();
    serde_json::Value::Array(values).to_string()
}

/// 在 worker 中将备份的外部原始字节导出到用户明确选择的路径。
pub(crate) fn export_recovery(id: String, path: String) -> String {
    let storage = match recovery_storage().lock() {
        Ok(storage) => storage.as_ref().cloned(),
        Err(_) => return "recovery.storage_unavailable".to_owned(),
    };
    let Some(storage) = storage else {
        return "recovery.storage_unavailable".to_owned();
    };
    storage
        .export_external(&id, Path::new(&path))
        .err()
        .map_or_else(String::new, |error| error.to_string())
}

/// 丢弃用户明确选择的本地恢复项。
pub(crate) fn discard_recovery(id: String) -> String {
    let storage = match recovery_storage().lock() {
        Ok(storage) => storage.as_ref().cloned(),
        Err(_) => return "recovery.storage_unavailable".to_owned(),
    };
    let Some(storage) = storage else {
        return "recovery.storage_unavailable".to_owned();
    };
    storage
        .remove(&id)
        .err()
        .map_or_else(String::new, |error| error.to_string())
}

/// 用户在关闭确认中选择丢弃文档时，先删除恢复项再解除会话绑定。
pub(crate) fn discard_document_recovery(runtime: &mut DocumentRuntime) -> String {
    let id = match &runtime.runtime {
        Ok(session) => session.recovery_id().map(str::to_owned),
        Err(error) => return format!("error:{error}"),
    };
    let Some(id) = id else {
        return String::new();
    };
    let storage = match recovery_storage().lock() {
        Ok(storage) => storage.as_ref().cloned(),
        Err(_) => return "recovery.storage_unavailable".to_owned(),
    };
    let Some(storage) = storage else {
        return "recovery.storage_unavailable".to_owned();
    };
    let result = match runtime.recovery_lease.as_ref() {
        Some(lease) if lease.id() == id => storage.remove_leased(lease),
        _ => storage.remove(&id),
    };
    if let Err(error) = result {
        return error.to_string();
    }
    if let Ok(session) = &mut runtime.runtime {
        session.clear_recovery_id();
    }
    runtime.recovery_lease = None;
    runtime.lifecycle.set_recovery_enabled(false);
    String::new()
}

/// 在后台将恢复会话的最新 `.cdoc` 字节更新至原恢复项。
pub(crate) fn update_recovery_document(
    id: String,
    original_path: String,
    baseline_digest: String,
    bytes: Vec<u8>,
) -> String {
    let storage = match recovery_storage().lock() {
        Ok(storage) => storage.as_ref().cloned(),
        Err(_) => return "recovery.storage_unavailable".to_owned(),
    };
    let Some(storage) = storage else {
        return "recovery.storage_unavailable".to_owned();
    };
    storage
        .update(
            &id,
            caret_app::document_recovery::RecoveryKind::LocalDocument,
            Path::new(&original_path),
            if baseline_digest.is_empty() {
                None
            } else {
                Some(baseline_digest)
            },
            &bytes,
        )
        .err()
        .map_or_else(String::new, |error| error.to_string())
}

/// 在 worker 中准备本地恢复文档；安装后仍为未命名、dirty 会话。
pub(crate) fn prepare_recovery_document(
    id: String,
    expected_session_generation: u64,
    expected_document_generation: u64,
) -> bridge::DocumentOpenResult {
    let result = (|| {
        let storage = recovery_storage()
            .lock()
            .map_err(|_| "recovery.storage_unavailable".to_owned())?
            .as_ref()
            .cloned()
            .ok_or_else(|| "recovery.storage_unavailable".to_owned())?;
        let (envelope, payload, source_lease) = storage
            .load_for_restore(&id)
            .map_err(|error| error.to_string())?;
        let next_generation = expected_session_generation
            .checked_add(1)
            .ok_or_else(|| "document.session_generation_exhausted".to_owned())?;
        let (session_recovery_id, recovery_lease) = storage
            .fork_for_session(&envelope, &payload)
            .map_err(|error| error.to_string())?;
        let prepared_session = DocumentSessionService::prepare_recovery(
            &payload,
            session_recovery_id,
            envelope.original_path.clone(),
            envelope.baseline_digest.clone(),
            next_generation,
        );
        let (session, snapshot_json) = match prepared_session {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = storage.remove_leased(&recovery_lease);
                return Err(error);
            }
        };
        let ticket = next_prepared_document_ticket();
        if ticket == 0 {
            let _ = storage.remove_leased(&recovery_lease);
            return Err("document.open_ticket_exhausted".to_owned());
        }
        let mut documents = match prepared_documents().lock() {
            Ok(documents) => documents,
            Err(_) => {
                let _ = storage.remove_leased(&recovery_lease);
                return Err("document.open_registry_unavailable".to_owned());
            }
        };
        documents.insert(
            ticket,
            PreparedDocument {
                session,
                expected_session_generation,
                expected_document_generation,
                recovery_source: Some((id.clone(), source_lease)),
                recovery_lease: Some(recovery_lease),
            },
        );
        Ok((ticket, snapshot_json))
    })();
    match result {
        Ok((ticket, snapshot_json)) => bridge::DocumentOpenResult {
            ticket,
            snapshot_json,
            error: String::new(),
        },
        Err(error) => bridge::DocumentOpenResult {
            ticket: 0,
            snapshot_json: String::new(),
            error,
        },
    }
}

pub(crate) fn write_document_file_checked(
    path: String,
    bytes: Vec<u8>,
    baseline_digest: String,
    queued_digest: String,
    recovery_id: String,
) -> bridge::DocumentWriteResult {
    let target = Path::new(&path);
    let result = |conflict, replace_allowed, recovery_id, recovery_created, error| {
        bridge::DocumentWriteResult {
            conflict,
            replace_allowed,
            recovery_id,
            recovery_created,
            error,
        }
    };
    match DocumentStorage.save_snapshot_if_unchanged_or_queued(
        target,
        &bytes,
        &baseline_digest,
        (!queued_digest.is_empty()).then_some(queued_digest.as_str()),
    ) {
        Ok(()) => result(false, false, String::new(), false, String::new()),
        Err(
            error @ (caret_app::document_storage::DocumentStorageError::ExternalConflict
            | caret_app::document_storage::DocumentStorageError::ExternalTargetUnavailable(
                _,
            )),
        ) => {
            let replace_allowed = matches!(
                error,
                caret_app::document_storage::DocumentStorageError::ExternalConflict
            );
            let recovery_created = recovery_id.is_empty();
            let stored = recovery_storage()
                .lock()
                .ok()
                .and_then(|storage| storage.as_ref().cloned())
                .ok_or_else(|| "recovery.storage_unavailable".to_owned())
                .and_then(|storage| {
                    if recovery_id.is_empty() {
                        storage
                            .store_for_session(target, Some(baseline_digest), &bytes)
                            .and_then(|(id, lease)| match pending_recovery_leases().lock() {
                                Ok(mut leases) => {
                                    leases.insert(id.clone(), lease);
                                    Ok(id)
                                }
                                Err(_) => {
                                    let _ = storage.remove_leased(&lease);
                                    Err(caret_app::document_recovery::RecoveryError::Busy)
                                }
                            })
                    } else {
                        storage
                            .update(
                                &recovery_id,
                                caret_app::document_recovery::RecoveryKind::LocalDocument,
                                target,
                                Some(baseline_digest),
                                &bytes,
                            )
                            .map(|()| recovery_id)
                    }
                    .map_err(|error| error.to_string())
                });
            match stored {
                Ok(id) => result(true, replace_allowed, id, recovery_created, String::new()),
                Err(error) => result(true, false, String::new(), false, error),
            }
        }
        Err(error) => result(false, false, String::new(), false, error.to_string()),
    }
}

/// 只释放尚未绑定的新租约，不删除可供恢复的条目。
pub(crate) fn cancel_pending_recovery_lease(id: String) {
    if let Ok(mut leases) = pending_recovery_leases().lock() {
        leases.remove(&id);
    }
}

/// 将写前冲突生成的恢复副本绑定至当前会话，后续编辑可更新同一条目。
pub(crate) fn set_document_recovery_id(
    runtime: &mut DocumentRuntime,
    session_generation: u64,
    id: String,
    original_path: String,
    baseline_digest: String,
) -> String {
    let baseline_digest = (!baseline_digest.is_empty()).then_some(baseline_digest);
    let lease = if runtime
        .recovery_lease
        .as_ref()
        .is_some_and(|lease| lease.id() == id)
    {
        None
    } else if let Some(lease) = pending_recovery_leases()
        .lock()
        .ok()
        .and_then(|mut leases| leases.remove(&id))
    {
        Some(lease)
    } else {
        let storage = match recovery_storage_for_cleanup() {
            Ok(storage) => storage,
            Err(error) => return error,
        };
        match storage.acquire_session_lease(&id) {
            Ok(lease) => Some(lease),
            Err(error) => return error.to_string(),
        }
    };
    let session = match &mut runtime.runtime {
        Ok(session) => session,
        Err(error) => return format!("error:{error}"),
    };
    if !session.set_recovery_id(session_generation, id, original_path, baseline_digest) {
        return "error:document.stale_recovery_completion".to_owned();
    }
    if let Some(lease) = lease {
        runtime.recovery_lease = Some(lease);
    }
    runtime.lifecycle.set_recovery_enabled(true);
    String::new()
}

/// 用户确认替换冲突版本前，先把当前外部原始字节存入恢复区再原子写回。
pub(crate) fn write_document_file_replace_external(path: String, bytes: Vec<u8>) -> String {
    let target = Path::new(&path);
    let external_bytes = match std::fs::read(target) {
        Ok(bytes) => bytes,
        Err(error) => return format!("document.external_backup_read_failed: {error}"),
    };
    let storage = match recovery_storage().lock() {
        Ok(storage) => storage.as_ref().cloned(),
        Err(_) => None,
    };
    let Some(storage) = storage else {
        return "recovery.storage_unavailable".to_owned();
    };
    if let Err(error) = storage.store(
        caret_app::document_recovery::RecoveryKind::ExternalVersion,
        target,
        Some(caret_app::document_session::content_digest(&external_bytes)),
        &external_bytes,
    ) {
        return format!("document.external_backup_failed: {error}");
    }
    DocumentStorage
        .save_snapshot(target, &bytes)
        .err()
        .map_or_else(String::new, |error| error.to_string())
}

/// 保存成功后推进对应会话的保存点，并按需移除恢复副本。
pub(crate) fn mark_document_saved_json(
    runtime: &mut DocumentRuntime,
    session_generation: u64,
    document_generation: u64,
    history_state_id: u64,
    path: String,
    content_digest: String,
) -> String {
    let accepted = match &mut runtime.runtime {
        Ok(session) => session.mark_saved(
            session_generation,
            document_generation,
            history_state_id,
            Path::new(&path),
            &content_digest,
        ),
        Err(error) => return format!("error:{error}"),
    };
    if !accepted {
        return "error:document.stale_save_completion".to_owned();
    }
    runtime.lifecycle.manually_saved(document_generation);
    if runtime
        .runtime
        .as_ref()
        .is_ok_and(|session| session.document_generation() == document_generation)
    {
        clear_recovery_after_saved(runtime);
    }
    runtime.runtime.as_ref().map_or_else(
        |error| format!("error:{error}"),
        DocumentSessionService::status_json,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        cancel_pending_recovery_lease, configure_document_recovery, discard_document_recovery,
        discard_recovery, export_recovery, mark_document_saved_json, pending_recovery_leases,
        prepare_recovery_document, recovery_entries_json, recovery_storage_for_cleanup,
        set_document_recovery_id, update_recovery_document, write_document_file_replace_external,
    };
    use crate::document::{document_save_snapshot, new_blank_document_runtime};
    use caret_app::document_recovery::{DocumentRecoveryStorage, RecoveryKind};
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn cancelling_an_unclaimed_lease_keeps_the_recovery_entry_available() {
        static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "caret-pending-recovery-lease-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let storage = DocumentRecoveryStorage::new(root.join("recovery"));
        let stored = storage.store_for_session(Path::new("document.cdoc"), None, b"snapshot");
        assert!(stored.is_ok());
        let Ok((id, lease)) = stored else { return };
        let registry = pending_recovery_leases().lock();
        assert!(registry.is_ok());
        let Ok(mut registry) = registry else { return };
        registry.insert(id.clone(), lease);
        drop(registry);

        cancel_pending_recovery_lease(id.clone());

        let acquired = storage.acquire_session_lease(&id);
        assert!(acquired.is_ok());
        let Ok(replacement_lease) = acquired else {
            return;
        };
        drop(replacement_lease);
        assert!(storage.remove(&id).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_file_commands_preserve_external_bytes_and_clear_saved_session_backups() {
        static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "caret-ffi-recovery-flow-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(configure_document_recovery(root.to_string_lossy().into_owned()).is_empty());
        assert_eq!(recovery_entries_json(), "[]");

        let document_path = root.join("document.cdoc");
        let original_external_bytes = b"external document version";
        assert!(std::fs::write(&document_path, original_external_bytes).is_ok());
        let mut runtime = new_blank_document_runtime();
        let snapshot = document_save_snapshot(&mut runtime);
        assert!(snapshot.error.is_empty());
        assert!(
            write_document_file_replace_external(
                document_path.to_string_lossy().into_owned(),
                snapshot.bytes.clone(),
            )
            .is_empty()
        );
        assert_eq!(
            std::fs::read(&document_path).ok(),
            Some(snapshot.bytes.clone())
        );

        let entries = recovery_entries_json();
        let parsed_result: Result<serde_json::Value, _> = serde_json::from_str(&entries);
        assert!(parsed_result.is_ok(), "recovery entries should be JSON");
        let parsed = parsed_result.unwrap_or_default();
        assert_eq!(parsed.as_array().map(Vec::len), Some(1));
        let external_id = parsed[0]["id"].as_str().unwrap_or_default().to_owned();
        assert!(
            !external_id.is_empty(),
            "external recovery entry should have an ID"
        );
        assert_eq!(parsed[0]["kind"], "external_version");
        let exported_path = root.join("external-backup.bin");
        assert!(
            export_recovery(
                external_id.clone(),
                exported_path.to_string_lossy().into_owned(),
            )
            .is_empty()
        );
        assert_eq!(
            std::fs::read(&exported_path).ok(),
            Some(original_external_bytes.to_vec())
        );
        assert!(discard_recovery(external_id).is_empty());

        let storage_result = recovery_storage_for_cleanup();
        assert!(
            storage_result.is_ok(),
            "recovery storage should be configured"
        );
        let Ok(storage) = storage_result else { return };
        let recovery_result = storage.store(
            RecoveryKind::LocalDocument,
            &document_path,
            Some(snapshot.baseline_digest.clone()),
            &snapshot.bytes,
        );
        assert!(
            recovery_result.is_ok(),
            "document recovery should be stored"
        );
        let Ok(recovery_id) = recovery_result else {
            return;
        };
        assert!(
            update_recovery_document(
                recovery_id.clone(),
                document_path.to_string_lossy().into_owned(),
                snapshot.baseline_digest.clone(),
                snapshot.bytes.clone(),
            )
            .is_empty()
        );
        assert!(
            set_document_recovery_id(
                &mut runtime,
                snapshot.session_generation,
                recovery_id.clone(),
                document_path.to_string_lossy().into_owned(),
                snapshot.baseline_digest.clone(),
            )
            .is_empty()
        );

        let saved = mark_document_saved_json(
            &mut runtime,
            snapshot.session_generation,
            snapshot.document_generation,
            snapshot.history_state_id,
            document_path.to_string_lossy().into_owned(),
            snapshot.content_digest,
        );
        assert!(!saved.starts_with("error:"));
        let saved_status_result: Result<serde_json::Value, _> = serde_json::from_str(&saved);
        assert!(saved_status_result.is_ok(), "saved status should be JSON");
        let saved_status = saved_status_result.unwrap_or_default();
        assert_eq!(
            saved_status["path"],
            document_path.to_string_lossy().as_ref()
        );
        assert_eq!(saved_status["dirty"], false);
        let remaining_result = storage.list();
        assert!(
            remaining_result.is_ok(),
            "recovery storage should remain readable"
        );
        let remaining = remaining_result.unwrap_or_default();
        assert!(!remaining.contains(&recovery_id));

        assert!(
            !prepare_recovery_document("missing-entry".to_owned(), 0, 0)
                .error
                .is_empty()
        );
        assert!(discard_document_recovery(&mut runtime).is_empty());
        cancel_pending_recovery_lease("missing-entry".to_owned());
        let _ = std::fs::remove_dir_all(root);
    }
}
