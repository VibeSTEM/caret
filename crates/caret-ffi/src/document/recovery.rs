//! 恢复条目、冲突快照与恢复会话的 FFI 持久化边界。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{
    DocumentRuntime, PreparedDocument, bridge, next_prepared_document_ticket, prepared_documents,
};
use caret_app::document_session::DocumentSessionService;
use caret_app::document_storage::DocumentStorage;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub(crate) struct RecoveryUpdateGate {
    operation: Mutex<()>,
    pub(super) pending: AtomicBool,
    target_revision: AtomicU64,
    pub(super) binding_generation: AtomicU64,
}

impl RecoveryUpdateGate {
    fn new(binding_generation: u64) -> Self {
        Self {
            operation: Mutex::new(()),
            pending: AtomicBool::new(false),
            target_revision: AtomicU64::new(0),
            binding_generation: AtomicU64::new(binding_generation),
        }
    }
}

fn recovery_update_gates() -> &'static Mutex<HashMap<String, Arc<RecoveryUpdateGate>>> {
    static GATES: OnceLock<Mutex<HashMap<String, Arc<RecoveryUpdateGate>>>> = OnceLock::new();
    GATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn recovery_update_gate(id: &str) -> Result<Arc<RecoveryUpdateGate>, String> {
    let mut gates = recovery_update_gates()
        .lock()
        .map_err(|_| "recovery.update_gate_unavailable".to_owned())?;
    Ok(gates
        .entry(id.to_owned())
        .or_insert_with(|| Arc::new(RecoveryUpdateGate::new(0)))
        .clone())
}

fn existing_recovery_update_gate(id: &str) -> Result<Arc<RecoveryUpdateGate>, String> {
    recovery_update_gates()
        .lock()
        .map_err(|_| "recovery.update_gate_unavailable".to_owned())?
        .get(id)
        .cloned()
        .ok_or_else(|| "recovery.stale_identity".to_owned())
}

/// 只在恢复文件已成功删除后解除注册；在途任务仍持有原 gate。
pub(super) fn forget_recovery_gate(id: &str) {
    if let Ok(mut gates) = recovery_update_gates().lock() {
        gates.remove(id);
    }
}

pub(super) fn recovery_gate_for_runtime(
    id: &str,
    binding_generation: u64,
) -> Result<Arc<RecoveryUpdateGate>, String> {
    let gate = recovery_update_gate(id)?;
    if !gate.pending.load(Ordering::Acquire) {
        gate.binding_generation
            .store(binding_generation, Ordering::Release);
    }
    Ok(gate)
}

pub(super) fn mark_recovery_rebind_pending(runtime: &DocumentRuntime, document_generation: u64) {
    if let Some(gate) = runtime.recovery_update_gate.as_ref() {
        gate.target_revision
            .store(document_generation, Ordering::Relaxed);
        gate.pending.store(true, Ordering::Release);
    }
}

pub(super) fn advance_pending_recovery_revision(
    runtime: &DocumentRuntime,
    document_generation: u64,
) {
    if let Some(gate) = runtime.recovery_update_gate.as_ref()
        && gate.pending.load(Ordering::Acquire)
    {
        gate.target_revision
            .store(document_generation, Ordering::Release);
    }
}

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
        if removed {
            forget_recovery_gate(&id);
        }
        if removed && let Ok(session) = &mut runtime.runtime {
            if session.clear_recovery_id().is_some() {
                runtime.recovery_binding_generation =
                    runtime.recovery_binding_generation.saturating_add(1);
                runtime.recovery_update_gate = None;
            }
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
    match storage.remove(&id) {
        Ok(()) => {
            forget_recovery_gate(&id);
            String::new()
        }
        Err(error) => error.to_string(),
    }
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
    forget_recovery_gate(&id);
    if let Ok(session) = &mut runtime.runtime
        && session.clear_recovery_id().is_some()
    {
        runtime.recovery_binding_generation = runtime.recovery_binding_generation.saturating_add(1);
        runtime.recovery_update_gate = None;
    }
    runtime.recovery_lease = None;
    runtime.lifecycle.set_recovery_enabled(false);
    String::new()
}

/// 在后台将恢复会话的最新 `.cdoc` 字节更新至原恢复项。
#[cfg(test)]
pub(crate) fn update_recovery_document(
    id: String,
    original_path: String,
    baseline_digest: String,
    binding_generation: u64,
    document_generation: u64,
    bytes: Vec<u8>,
) -> String {
    update_recovery_document_with_binding(
        id,
        original_path,
        baseline_digest,
        binding_generation,
        document_generation,
        false,
        bytes,
    )
    .error
}

/// 后台按 ID gate 串行执行必要的重绑与快照更新，结果携带可安装的持久化代际。
pub(crate) fn update_recovery_document_for_snapshot(
    id: String,
    original_path: String,
    baseline_digest: String,
    binding_generation: u64,
    document_generation: u64,
    rebind_pending: bool,
    bytes: Vec<u8>,
) -> bridge::RecoveryUpdateResult {
    update_recovery_document_with_binding(
        id,
        original_path,
        baseline_digest,
        binding_generation,
        document_generation,
        rebind_pending,
        bytes,
    )
}

fn update_recovery_document_with_binding(
    id: String,
    original_path: String,
    baseline_digest: String,
    captured_binding_generation: u64,
    document_generation: u64,
    rebind_requested: bool,
    bytes: Vec<u8>,
) -> bridge::RecoveryUpdateResult {
    let fail = |binding_generation, error: String| bridge::RecoveryUpdateResult {
        binding_generation,
        superseded: false,
        error,
    };
    let superseded = |binding_generation| bridge::RecoveryUpdateResult {
        binding_generation,
        superseded: true,
        error: "document.stale_recovery_completion".to_owned(),
    };
    let gate = match existing_recovery_update_gate(&id) {
        Ok(gate) => gate,
        Err(error) => return fail(captured_binding_generation, error),
    };
    let _operation = match gate.operation.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return fail(
                captured_binding_generation,
                "recovery.update_gate_unavailable".to_owned(),
            );
        }
    };
    let pending = gate.pending.load(Ordering::Acquire);
    let target_revision = gate.target_revision.load(Ordering::Acquire);
    let mut binding_generation = gate.binding_generation.load(Ordering::Acquire);
    if pending {
        if !rebind_requested || document_generation != target_revision {
            return superseded(binding_generation);
        }
        let storage = match recovery_storage_for_cleanup() {
            Ok(storage) => storage,
            Err(error) => return fail(binding_generation, error),
        };
        match storage.bind_session(
            &id,
            Path::new(&original_path),
            (!baseline_digest.is_empty()).then_some(baseline_digest.clone()),
            document_generation,
        ) {
            Ok(new_generation) => {
                binding_generation = new_generation;
                gate.binding_generation
                    .store(new_generation, Ordering::Release);
                gate.pending.store(false, Ordering::Release);
            }
            Err(error) => return fail(binding_generation, error.to_string()),
        }
    } else if captured_binding_generation != binding_generation
        && !(rebind_requested && target_revision == document_generation)
    {
        return superseded(binding_generation);
    }
    let storage = match recovery_storage_for_cleanup() {
        Ok(storage) => storage,
        Err(error) => return fail(binding_generation, error),
    };
    match storage.update_if_identity_matches(
        &id,
        caret_app::document_recovery::RecoveryKind::LocalDocument,
        caret_app::document_recovery::RecoverySnapshotIdentity::new(
            Path::new(&original_path),
            (!baseline_digest.is_empty()).then_some(baseline_digest.as_str()),
            binding_generation,
            document_generation,
        ),
        &bytes,
    ) {
        Ok(()) => bridge::RecoveryUpdateResult {
            binding_generation,
            superseded: false,
            error: String::new(),
        },
        Err(error) => fail(binding_generation, error.to_string()),
    }
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
            session_recovery_id.clone(),
            envelope.original_path.clone(),
            envelope.baseline_digest.clone(),
            next_generation,
        );
        let (session, snapshot_json) = match prepared_session {
            Ok(prepared) => prepared,
            Err(error) => {
                if storage.remove_leased(&recovery_lease).is_ok() {
                    forget_recovery_gate(recovery_lease.id());
                }
                return Err(error);
            }
        };
        let binding_generation = match storage.bind_session(
            &session_recovery_id,
            Path::new(&envelope.original_path),
            envelope.baseline_digest.clone(),
            session.document_generation(),
        ) {
            Ok(generation) => generation,
            Err(error) => {
                if storage.remove_leased(&recovery_lease).is_ok() {
                    forget_recovery_gate(recovery_lease.id());
                }
                return Err(error.to_string());
            }
        };
        let update_gate = recovery_gate_for_runtime(&session_recovery_id, binding_generation)?;
        let ticket = next_prepared_document_ticket();
        if ticket == 0 {
            if storage.remove_leased(&recovery_lease).is_ok() {
                forget_recovery_gate(recovery_lease.id());
            }
            return Err("document.open_ticket_exhausted".to_owned());
        }
        let mut documents = match prepared_documents().lock() {
            Ok(documents) => documents,
            Err(_) => {
                if storage.remove_leased(&recovery_lease).is_ok() {
                    forget_recovery_gate(recovery_lease.id());
                }
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
                recovery_binding_generation: binding_generation,
                recovery_update_gate: Some(update_gate),
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
    binding_generation: u64,
    document_generation: u64,
) -> bridge::DocumentWriteResult {
    let target = Path::new(&path);
    let recovery_baseline_digest = (!baseline_digest.is_empty()).then_some(baseline_digest.clone());
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
                            .store_for_session(target, recovery_baseline_digest.clone(), &bytes)
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
                            .map_err(|error| error.to_string())
                    } else {
                        let update = update_recovery_document_with_binding(
                            recovery_id.clone(),
                            target.to_string_lossy().into_owned(),
                            baseline_digest.clone(),
                            binding_generation,
                            document_generation,
                            false,
                            bytes.clone(),
                        );
                        if update.error.is_empty() {
                            Ok(recovery_id)
                        } else {
                            Err(update.error)
                        }
                    }
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
    source_revision: u64,
    expected_binding_generation: u64,
    id: String,
    original_path: String,
    baseline_digest: String,
) -> String {
    let baseline_digest = (!baseline_digest.is_empty()).then_some(baseline_digest);
    let binding_unchanged = runtime.runtime.as_ref().is_ok_and(|session| {
        session.recovery_binding_matches(&id, &original_path, baseline_digest.as_deref())
    });
    if !runtime
        .runtime
        .as_ref()
        .is_ok_and(|session| session.session_generation() == session_generation)
        || runtime.recovery_binding_generation != expected_binding_generation
    {
        return "error:document.stale_recovery_completion".to_owned();
    }
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
    let binding_generation = if binding_unchanged {
        runtime.recovery_binding_generation
    } else {
        let storage = match recovery_storage_for_cleanup() {
            Ok(storage) => storage,
            Err(error) => return error,
        };
        match storage.bind_session(
            &id,
            Path::new(&original_path),
            baseline_digest.clone(),
            source_revision,
        ) {
            Ok(generation) => generation,
            Err(error) => return error.to_string(),
        }
    };
    let update_gate = match recovery_gate_for_runtime(&id, binding_generation) {
        Ok(gate) => gate,
        Err(error) => return error,
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
    runtime.recovery_update_gate = Some(update_gate);
    runtime.lifecycle.set_recovery_enabled(true);
    if let Ok(session) = &runtime.runtime {
        let current_revision = session.document_generation();
        if !binding_unchanged {
            runtime.recovery_binding_generation = binding_generation;
            runtime.lifecycle.recovery_binding_changed();
        }
        if current_revision > source_revision {
            runtime
                .lifecycle
                .edited(current_revision, true, runtime.monotonic_origin.elapsed());
        }
    }
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
    if runtime.runtime.as_ref().is_ok_and(|session| {
        session.document_generation() == document_generation
            && session.history_state_id() == history_state_id
    }) {
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
        set_document_recovery_id, update_recovery_document, update_recovery_document_for_snapshot,
        write_document_file_checked, write_document_file_replace_external,
    };
    use crate::document::{
        auto_save_deadline_json, document_revision_changed, document_save_snapshot,
        insert_text_json, new_blank_document_runtime, recovery_update_deadline_fired,
        recovery_update_write_finished, recovery_update_write_finished_with_binding,
    };
    use caret_app::document_recovery::{DocumentRecoveryStorage, RecoveryKind};
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

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
            (!snapshot.baseline_digest.is_empty()).then_some(snapshot.baseline_digest.clone()),
            &snapshot.bytes,
        );
        assert!(
            recovery_result.is_ok(),
            "document recovery should be stored"
        );
        let Ok(recovery_id) = recovery_result else {
            return;
        };
        assert!(super::recovery_gate_for_runtime(&recovery_id, 0).is_ok());
        assert!(
            update_recovery_document(
                recovery_id.clone(),
                document_path.to_string_lossy().into_owned(),
                snapshot.baseline_digest.clone(),
                0,
                snapshot.document_generation,
                snapshot.bytes.clone(),
            )
            .is_empty()
        );
        assert!(auto_save_deadline_json(&runtime).is_empty());
        assert!(
            serde_json::from_str::<serde_json::Value>(&crate::document::insert_text_json(
                &mut runtime,
                "newer revision".to_owned(),
            ))
            .is_ok()
        );
        assert!(
            set_document_recovery_id(
                &mut runtime,
                snapshot.session_generation.saturating_add(1),
                snapshot.document_generation,
                0,
                recovery_id.clone(),
                document_path.to_string_lossy().into_owned(),
                snapshot.baseline_digest.clone(),
            )
            .starts_with("error:")
        );
        assert!(auto_save_deadline_json(&runtime).is_empty());
        assert!(
            set_document_recovery_id(
                &mut runtime,
                snapshot.session_generation,
                snapshot.document_generation,
                0,
                recovery_id.clone(),
                document_path.to_string_lossy().into_owned(),
                snapshot.baseline_digest.clone(),
            )
            .is_empty()
        );
        let deadline = auto_save_deadline_json(&runtime);
        assert!(deadline.contains("\"recovery\":true"), "{deadline}");
        let deadline_result: Result<serde_json::Value, _> = serde_json::from_str(&deadline);
        assert!(deadline_result.is_ok());
        let deadline_value = deadline_result.unwrap_or_default();
        let deadline_generation = deadline_value["generation"]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok());
        assert!(deadline_generation.is_some());
        runtime.monotonic_origin = Instant::now() - Duration::from_secs(5);
        let captured = recovery_update_deadline_fired(
            &mut runtime,
            snapshot.session_generation,
            deadline_generation.unwrap_or_default(),
        );
        assert!(captured.error.is_empty());
        assert!(!captured.stale_deadline);
        assert_eq!(captured.recovery_id, recovery_id);
        assert_eq!(captured.document_generation, 1);

        let storage_result = recovery_storage_for_cleanup();
        assert!(storage_result.is_ok());
        let Ok(storage) = storage_result else { return };
        let replacement_result = storage.store(
            RecoveryKind::LocalDocument,
            &document_path,
            (!snapshot.baseline_digest.is_empty()).then_some(snapshot.baseline_digest.clone()),
            &snapshot.bytes,
        );
        assert!(replacement_result.is_ok());
        let Ok(replacement_id) = replacement_result else {
            return;
        };
        let replacement_expected_binding_generation = runtime.recovery_binding_generation;
        assert!(
            set_document_recovery_id(
                &mut runtime,
                snapshot.session_generation,
                snapshot.document_generation,
                replacement_expected_binding_generation,
                replacement_id.clone(),
                document_path.to_string_lossy().into_owned(),
                snapshot.baseline_digest.clone(),
            )
            .is_empty()
        );
        assert!(
            recovery_update_write_finished(
                &mut runtime,
                snapshot.session_generation,
                captured.document_generation,
                recovery_id.clone(),
                captured.recovery_binding_generation,
                true,
            )
            .starts_with("error:")
        );
        let replacement_deadline = auto_save_deadline_json(&runtime);
        assert!(
            replacement_deadline.contains("\"recovery\":true"),
            "{replacement_deadline}"
        );
        let rebound_expected_binding_generation = runtime.recovery_binding_generation;
        assert!(
            set_document_recovery_id(
                &mut runtime,
                snapshot.session_generation,
                snapshot.document_generation,
                rebound_expected_binding_generation,
                recovery_id.clone(),
                document_path.to_string_lossy().into_owned(),
                snapshot.baseline_digest.clone(),
            )
            .is_empty()
        );
        let rebound_deadline = auto_save_deadline_json(&runtime);
        assert!(
            rebound_deadline.contains("\"recovery\":true"),
            "{rebound_deadline}"
        );
        let rebound_deadline_result: Result<serde_json::Value, _> =
            serde_json::from_str(&rebound_deadline);
        assert!(rebound_deadline_result.is_ok());
        let rebound_generation = rebound_deadline_result.unwrap_or_default()["generation"]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok());
        assert!(rebound_generation.is_some());
        runtime.monotonic_origin = Instant::now() - Duration::from_secs(10);
        let rebound = recovery_update_deadline_fired(
            &mut runtime,
            snapshot.session_generation,
            rebound_generation.unwrap_or_default(),
        );
        assert!(!rebound.stale_deadline);
        assert_eq!(rebound.recovery_id, recovery_id);
        assert_eq!(rebound.document_generation, captured.document_generation);
        assert_ne!(
            rebound.recovery_binding_generation,
            captured.recovery_binding_generation
        );
        assert!(
            recovery_update_write_finished(
                &mut runtime,
                snapshot.session_generation,
                captured.document_generation,
                recovery_id.clone(),
                captured.recovery_binding_generation,
                true,
            )
            .starts_with("error:")
        );
        let still_scheduled = auto_save_deadline_json(&runtime);
        assert!(still_scheduled.is_empty());
        assert!(
            update_recovery_document(
                recovery_id.clone(),
                document_path.to_string_lossy().into_owned(),
                snapshot.baseline_digest.clone(),
                rebound.recovery_binding_generation,
                rebound.document_generation,
                rebound.bytes.clone(),
            )
            .is_empty()
        );
        assert!(
            recovery_update_write_finished(
                &mut runtime,
                snapshot.session_generation,
                rebound.document_generation,
                recovery_id.clone(),
                rebound.recovery_binding_generation,
                true,
            )
            .is_empty()
        );
        let current_snapshot = document_save_snapshot(&mut runtime);
        assert!(current_snapshot.error.is_empty());
        let stale_save = mark_document_saved_json(
            &mut runtime,
            current_snapshot.session_generation,
            current_snapshot.document_generation,
            current_snapshot.history_state_id.saturating_sub(1),
            document_path.to_string_lossy().into_owned(),
            current_snapshot.content_digest.clone(),
        );
        let stale_status: serde_json::Value = serde_json::from_str(&stale_save).unwrap_or_default();
        assert_eq!(stale_status["dirty"], true);
        assert_eq!(stale_status["recoveryId"], recovery_id);
        let remaining_after_stale_save = storage.list().unwrap_or_default();
        assert!(remaining_after_stale_save.contains(&recovery_id));

        let saved = mark_document_saved_json(
            &mut runtime,
            current_snapshot.session_generation,
            current_snapshot.document_generation,
            current_snapshot.history_state_id,
            document_path.to_string_lossy().into_owned(),
            current_snapshot.content_digest,
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
        assert!(storage.remove(&replacement_id).is_ok());
        let remaining_result = storage.list();
        assert!(
            remaining_result.is_ok(),
            "recovery storage should remain readable"
        );
        let remaining = remaining_result.unwrap_or_default();
        assert!(!remaining.contains(&recovery_id));
        assert!(!remaining.contains(&replacement_id));

        let rollover_path = root.join("rollover.cdoc");
        let mut rollover_runtime = new_blank_document_runtime();
        let rollover_initial = document_save_snapshot(&mut rollover_runtime);
        assert!(rollover_initial.error.is_empty());
        let rollover_store =
            storage.store_for_session(&rollover_path, None, &rollover_initial.bytes);
        assert!(rollover_store.is_ok());
        let Ok((rollover_id, rollover_lease)) = rollover_store else {
            return;
        };
        rollover_runtime.recovery_lease = Some(rollover_lease);
        let rollover_expected_binding_generation = rollover_runtime.recovery_binding_generation;
        assert!(
            set_document_recovery_id(
                &mut rollover_runtime,
                rollover_initial.session_generation,
                rollover_initial.document_generation,
                rollover_expected_binding_generation,
                rollover_id.clone(),
                rollover_path.to_string_lossy().into_owned(),
                String::new(),
            )
            .is_empty()
        );

        let old_binding_generation = storage
            .bind_session(&rollover_id, &rollover_path, None, u64::MAX - 1)
            .unwrap_or_default();
        rollover_runtime.recovery_binding_generation = old_binding_generation;
        if let Some(gate) = rollover_runtime.recovery_update_gate.as_ref() {
            gate.binding_generation
                .store(old_binding_generation, Ordering::Release);
        }
        assert!(
            update_recovery_document(
                rollover_id.clone(),
                rollover_path.to_string_lossy().into_owned(),
                String::new(),
                old_binding_generation,
                u64::MAX - 1,
                b"pending pre-wrap payload".to_vec(),
            )
            .is_empty()
        );

        document_revision_changed(&mut rollover_runtime, u64::MAX - 1, 0, true);
        let new_session_generation =
            crate::document::document_session_generation(&rollover_runtime);
        assert_ne!(new_session_generation, rollover_initial.session_generation);
        assert!(
            update_recovery_document(
                rollover_id.clone(),
                rollover_path.to_string_lossy().into_owned(),
                String::new(),
                old_binding_generation,
                u64::MAX,
                b"late pre-wrap worker payload".to_vec(),
            )
            .starts_with("document.stale_recovery_completion")
        );

        let rollover_deadline = auto_save_deadline_json(&rollover_runtime);
        let rollover_deadline_result: Result<serde_json::Value, _> =
            serde_json::from_str(&rollover_deadline);
        assert!(rollover_deadline_result.is_ok(), "{rollover_deadline}");
        let rollover_timer_generation = rollover_deadline_result.unwrap_or_default()["generation"]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok());
        assert!(rollover_timer_generation.is_some());
        rollover_runtime.monotonic_origin = Instant::now() - Duration::from_secs(5);
        let rollover_snapshot = recovery_update_deadline_fired(
            &mut rollover_runtime,
            new_session_generation,
            rollover_timer_generation.unwrap_or_default(),
        );
        assert!(rollover_snapshot.error.is_empty());
        assert_eq!(rollover_snapshot.document_generation, 0);
        assert_eq!(
            rollover_snapshot.recovery_binding_generation,
            old_binding_generation
        );
        assert!(rollover_snapshot.recovery_rebind_pending);
        assert_eq!(rollover_snapshot.recovery_id, rollover_id);
        let rollover_update = update_recovery_document_for_snapshot(
            rollover_id.clone(),
            rollover_path.to_string_lossy().into_owned(),
            String::new(),
            old_binding_generation,
            0,
            rollover_snapshot.recovery_rebind_pending,
            rollover_snapshot.bytes.clone(),
        );
        assert!(
            rollover_update.error.is_empty(),
            "{}",
            rollover_update.error
        );
        let new_binding_generation = rollover_update.binding_generation;
        assert_ne!(new_binding_generation, old_binding_generation);
        // worker 已写入旧快照，回调到达前用户继续编辑。
        assert!(!insert_text_json(&mut rollover_runtime, "newer".to_owned()).starts_with("error:"));
        let followup = recovery_update_write_finished_with_binding(
            &mut rollover_runtime,
            rollover_snapshot.session_generation,
            rollover_snapshot.document_generation,
            rollover_id.clone(),
            old_binding_generation,
            new_binding_generation,
            true,
        );
        let followup_json: serde_json::Value = serde_json::from_str(&followup).unwrap_or_default();
        assert_eq!(followup_json["recovery"], true, "{followup}");
        rollover_runtime.monotonic_origin = Instant::now() - Duration::from_secs(20);
        let next_generation = followup_json["generation"]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_default();
        let newer_snapshot = recovery_update_deadline_fired(
            &mut rollover_runtime,
            new_session_generation,
            next_generation,
        );
        assert!(newer_snapshot.error.is_empty(), "{}", newer_snapshot.error);
        assert_eq!(newer_snapshot.document_generation, 1);
        assert_eq!(
            newer_snapshot.recovery_binding_generation,
            new_binding_generation
        );
        rollover_runtime.recovery_lease = None;
        let rebound_entry = storage.load(&rollover_id);
        assert!(rebound_entry.is_ok());
        let Ok((rebound_envelope, rebound_payload)) = rebound_entry else {
            return;
        };
        assert_eq!(rebound_envelope.binding_generation, new_binding_generation);
        assert_eq!(rebound_envelope.document_revision, 0);
        assert_eq!(
            rebound_envelope.original_path,
            rollover_path.to_string_lossy()
        );
        assert_eq!(rebound_payload, rollover_snapshot.bytes);
        rollover_runtime.recovery_lease = storage.acquire_session_lease(&rollover_id).ok();
        let newer_update = update_recovery_document_for_snapshot(
            rollover_id.clone(),
            rollover_path.to_string_lossy().into_owned(),
            String::new(),
            newer_snapshot.recovery_binding_generation,
            newer_snapshot.document_generation,
            newer_snapshot.recovery_rebind_pending,
            newer_snapshot.bytes.clone(),
        );
        assert!(newer_update.error.is_empty(), "{}", newer_update.error);

        let queued_gate = rollover_runtime.recovery_update_gate.clone();
        let rollover_saved = mark_document_saved_json(
            &mut rollover_runtime,
            new_session_generation,
            newer_snapshot.document_generation,
            newer_snapshot.history_state_id,
            rollover_path.to_string_lossy().into_owned(),
            newer_snapshot.content_digest,
        );
        assert!(!rollover_saved.starts_with("error:"), "{rollover_saved}");
        assert!(
            rollover_runtime
                .runtime
                .as_ref()
                .is_ok_and(|session| session.recovery_id().is_none())
        );
        assert!(!storage.list().unwrap_or_default().contains(&rollover_id));
        assert!(super::existing_recovery_update_gate(&rollover_id).is_err());
        let Some(queued_gate) = queued_gate else {
            return;
        };
        let Ok(_held_gate) = queued_gate.operation.lock() else {
            return;
        };
        assert!(
            storage
                .update_if_identity_matches(
                    &rollover_id,
                    RecoveryKind::LocalDocument,
                    caret_app::document_recovery::RecoverySnapshotIdentity::new(
                        &rollover_path,
                        None,
                        new_binding_generation,
                        0,
                    ),
                    b"queued old snapshot",
                )
                .is_err()
        );
        assert!(
            update_recovery_document(
                rollover_id.clone(),
                rollover_path.to_string_lossy().into_owned(),
                String::new(),
                new_binding_generation,
                0,
                rollover_snapshot.bytes.clone(),
            )
            .contains("recovery.stale_identity")
        );

        // 重建一个真实恢复 envelope，覆盖回绕重绑失败后的进程内 fail-closed 与重试。
        let retry_path = root.join("retry-wrap.cdoc");
        let mut retry_runtime = new_blank_document_runtime();
        let retry_initial = document_save_snapshot(&mut retry_runtime);
        let retry_stored = storage.store_for_session(&retry_path, None, &retry_initial.bytes);
        assert!(retry_stored.is_ok());
        let Ok((retry_id, retry_lease)) = retry_stored else {
            return;
        };
        retry_runtime.recovery_lease = Some(retry_lease);
        let retry_expected_binding_generation = retry_runtime.recovery_binding_generation;
        assert!(
            set_document_recovery_id(
                &mut retry_runtime,
                retry_initial.session_generation,
                retry_initial.document_generation,
                retry_expected_binding_generation,
                retry_id.clone(),
                retry_path.to_string_lossy().into_owned(),
                String::new(),
            )
            .is_empty()
        );
        let retry_envelope_path = root.join("recovery").join(format!("{retry_id}.json"));
        let original_envelope = std::fs::read(&retry_envelope_path);
        assert!(original_envelope.is_ok());
        assert!(std::fs::write(&retry_envelope_path, b"invalid envelope").is_ok());
        let gate = retry_runtime
            .recovery_update_gate
            .as_ref()
            .cloned()
            .unwrap_or_else(|| panic!("recovery session owns an update gate"));
        let (gate_held_tx, gate_held_rx) = std::sync::mpsc::channel();
        let (release_gate_tx, release_gate_rx) = std::sync::mpsc::channel();
        let slow_worker = std::thread::spawn(move || {
            let _operation = gate
                .operation
                .lock()
                .unwrap_or_else(|_| panic!("gate lock available"));
            gate_held_tx
                .send(())
                .unwrap_or_else(|_| panic!("main thread is waiting"));
            release_gate_rx
                .recv()
                .unwrap_or_else(|_| panic!("main thread releases slow worker"));
        });
        gate_held_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or_else(|_| panic!("worker holds recovery gate"));
        let (edit_done_tx, edit_done_rx) = std::sync::mpsc::channel();
        let (runtime_tx, runtime_rx) = std::sync::mpsc::channel();
        let edit_thread = std::thread::spawn(move || {
            let mut runtime = retry_runtime;
            document_revision_changed(&mut runtime, u64::MAX - 1, 0, true);
            edit_done_tx
                .send(())
                .unwrap_or_else(|_| panic!("main thread is waiting"));
            runtime_tx
                .send(runtime)
                .unwrap_or_else(|_| panic!("main thread receives runtime"));
        });
        let edit_completed_while_gate_held = edit_done_rx.recv_timeout(Duration::from_secs(1));
        release_gate_tx
            .send(())
            .unwrap_or_else(|_| panic!("slow worker is waiting"));
        assert!(slow_worker.join().is_ok());
        let mut retry_runtime = runtime_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or_else(|_| panic!("edit thread returns runtime"));
        assert!(edit_thread.join().is_ok());
        assert!(
            edit_completed_while_gate_held.is_ok(),
            "revision change waited for recovery worker gate"
        );
        let start = std::sync::Arc::new(std::sync::Barrier::new(2));
        let worker_start = start.clone();
        let worker_id = retry_id.clone();
        let worker_path = retry_path.to_string_lossy().into_owned();
        let old_binding_generation = retry_runtime.recovery_binding_generation;
        let old_worker = std::thread::spawn(move || {
            worker_start.wait();
            update_recovery_document(
                worker_id,
                worker_path,
                String::new(),
                old_binding_generation,
                u64::MAX,
                b"racing old worker".to_vec(),
            )
        });
        start.wait();
        let old_worker_result = old_worker.join();
        assert!(old_worker_result.is_ok());
        assert!(!old_worker_result.unwrap_or_default().is_empty());
        let failed_envelope = storage.load(&retry_id);
        assert!(failed_envelope.is_err());
        let failed_rebind_deadline = auto_save_deadline_json(&retry_runtime);
        let failed_rebind_deadline_json: serde_json::Value =
            serde_json::from_str(&failed_rebind_deadline).unwrap_or_default();
        let failed_rebind_generation = failed_rebind_deadline_json["generation"]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok());
        let failed_rebind_session_generation = failed_rebind_deadline_json["sessionGeneration"]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok());
        assert!(
            failed_rebind_generation.is_some(),
            "{failed_rebind_deadline}"
        );
        assert!(
            failed_rebind_session_generation.is_some(),
            "{failed_rebind_deadline}"
        );
        retry_runtime.monotonic_origin = Instant::now() - Duration::from_secs(40);
        let failed_retry_snapshot = recovery_update_deadline_fired(
            &mut retry_runtime,
            failed_rebind_session_generation.unwrap_or_default(),
            failed_rebind_generation.unwrap_or_default(),
        );
        assert!(failed_retry_snapshot.error.is_empty());
        assert!(failed_retry_snapshot.recovery_rebind_pending);
        let failed_worker_result = update_recovery_document_for_snapshot(
            retry_id.clone(),
            retry_path.to_string_lossy().into_owned(),
            String::new(),
            failed_retry_snapshot.recovery_binding_generation,
            failed_retry_snapshot.document_generation,
            failed_retry_snapshot.recovery_rebind_pending,
            failed_retry_snapshot.bytes.clone(),
        );
        assert!(!failed_worker_result.superseded);
        assert!(!failed_worker_result.error.is_empty());
        assert!(
            recovery_update_write_finished_with_binding(
                &mut retry_runtime,
                failed_retry_snapshot.session_generation,
                failed_retry_snapshot.document_generation,
                retry_id.clone(),
                failed_retry_snapshot.recovery_binding_generation,
                failed_worker_result.binding_generation,
                false,
            )
            .is_empty()
        );
        assert!(auto_save_deadline_json(&retry_runtime).is_empty());
        assert!(
            std::fs::write(&retry_envelope_path, original_envelope.unwrap_or_default(),).is_ok()
        );
        assert!(
            !insert_text_json(&mut retry_runtime, "retry edit".to_owned()).starts_with("error:")
        );
        let retry_deadline = auto_save_deadline_json(&retry_runtime);
        assert!(
            retry_deadline.contains("\"recovery\":true"),
            "{retry_deadline}"
        );
        let retry_deadline_value: serde_json::Value =
            serde_json::from_str(&retry_deadline).unwrap_or_default();
        let retry_deadline_generation = retry_deadline_value["generation"]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok());
        let retry_deadline_session = retry_deadline_value["sessionGeneration"]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok());
        assert!(retry_deadline_generation.is_some());
        assert!(retry_deadline_session.is_some());
        retry_runtime.monotonic_origin = Instant::now() - Duration::from_secs(60);
        assert!(auto_save_deadline_json(&retry_runtime).contains("\"delayMs\":0"));
        let retry_snapshot = recovery_update_deadline_fired(
            &mut retry_runtime,
            retry_deadline_session.unwrap_or_default(),
            retry_deadline_generation.unwrap_or_default(),
        );
        assert!(retry_snapshot.error.is_empty(), "{}", retry_snapshot.error);
        assert!(
            retry_snapshot.recovery_rebind_pending,
            "snapshot={}, stale={}, pending={}, target={}, deadline={}",
            retry_snapshot.recovery_rebind_pending,
            retry_snapshot.stale_deadline,
            retry_runtime
                .recovery_update_gate
                .as_ref()
                .is_some_and(|gate| gate.pending.load(Ordering::Acquire)),
            retry_runtime
                .recovery_update_gate
                .as_ref()
                .map_or(u64::MAX, |gate| gate
                    .target_revision
                    .load(Ordering::Acquire)),
            retry_deadline,
        );
        // 快照排队后、worker 取得 gate 前发生编辑，旧快照必须被拒绝并重新排期。
        assert!(
            !insert_text_json(&mut retry_runtime, "newer edit".to_owned()).starts_with("error:")
        );
        let stale_worker = update_recovery_document_for_snapshot(
            retry_id.clone(),
            retry_path.to_string_lossy().into_owned(),
            String::new(),
            retry_snapshot.recovery_binding_generation,
            retry_snapshot.document_generation,
            retry_snapshot.recovery_rebind_pending,
            retry_snapshot.bytes.clone(),
        );
        assert!(stale_worker.superseded);
        assert!(!stale_worker.error.is_empty());
        let rescheduled = recovery_update_write_finished_with_binding(
            &mut retry_runtime,
            retry_snapshot.session_generation,
            retry_snapshot.document_generation,
            retry_id.clone(),
            retry_snapshot.recovery_binding_generation,
            stale_worker.binding_generation,
            false,
        );
        let deadline: serde_json::Value = serde_json::from_str(&rescheduled).unwrap_or_default();
        let session = deadline["sessionGeneration"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("session generation"));
        let generation = deadline["generation"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("deadline generation"));
        retry_runtime.monotonic_origin = Instant::now() - Duration::from_secs(80);
        let retry_snapshot =
            recovery_update_deadline_fired(&mut retry_runtime, session, generation);
        assert_eq!(retry_snapshot.document_generation, 2);
        let retry_worker_result = update_recovery_document_for_snapshot(
            retry_id.clone(),
            retry_path.to_string_lossy().into_owned(),
            String::new(),
            retry_snapshot.recovery_binding_generation,
            retry_snapshot.document_generation,
            retry_snapshot.recovery_rebind_pending,
            retry_snapshot.bytes.clone(),
        );
        assert!(
            retry_worker_result.error.is_empty(),
            "{}",
            retry_worker_result.error
        );
        assert!(
            recovery_update_write_finished_with_binding(
                &mut retry_runtime,
                retry_snapshot.session_generation,
                retry_snapshot.document_generation,
                retry_id.clone(),
                retry_snapshot.recovery_binding_generation,
                retry_worker_result.binding_generation,
                true,
            )
            .is_empty()
        );
        retry_runtime.recovery_lease = None;
        let retried_envelope = storage.load(&retry_id);
        assert!(retried_envelope.is_ok(), "{retried_envelope:?}");
        let Ok((retried_identity, retried_payload)) = retried_envelope else {
            return;
        };
        assert_eq!(retried_identity.document_revision, 2);
        assert_ne!(
            retried_identity.binding_generation,
            retry_initial.recovery_binding_generation
        );
        assert_eq!(retried_payload, retry_snapshot.bytes);
        assert!(storage.remove(&retry_id).is_ok());

        // 外部冲突保存复用同一 ID gate，不能绕过失败重绑屏障覆盖旧恢复 envelope。
        let conflict_path = root.join("conflict-wrap.cdoc");
        let original_disk = b"baseline document";
        assert!(std::fs::write(&conflict_path, original_disk).is_ok());
        let mut conflict_runtime = new_blank_document_runtime();
        let conflict_snapshot = document_save_snapshot(&mut conflict_runtime);
        let baseline_digest = caret_app::document_session::content_digest(original_disk);
        let conflict_stored = storage.store_for_session(
            &conflict_path,
            Some(baseline_digest.clone()),
            &conflict_snapshot.bytes,
        );
        assert!(conflict_stored.is_ok());
        let Ok((conflict_id, conflict_lease)) = conflict_stored else {
            return;
        };
        conflict_runtime.recovery_lease = Some(conflict_lease);
        let conflict_expected_binding = conflict_runtime.recovery_binding_generation;
        assert!(
            set_document_recovery_id(
                &mut conflict_runtime,
                conflict_snapshot.session_generation,
                conflict_snapshot.document_generation,
                conflict_expected_binding,
                conflict_id.clone(),
                conflict_path.to_string_lossy().into_owned(),
                baseline_digest.clone(),
            )
            .is_empty()
        );
        let conflict_envelope_path = root.join("recovery").join(format!("{conflict_id}.json"));
        let mut conflict_envelope: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&conflict_envelope_path).unwrap_or_default())
                .unwrap_or_default();
        conflict_envelope["binding_generation"] = serde_json::json!(u64::MAX);
        assert!(
            std::fs::write(
                &conflict_envelope_path,
                serde_json::to_vec(&conflict_envelope).unwrap_or_default(),
            )
            .is_ok()
        );
        conflict_runtime.recovery_binding_generation = u64::MAX;
        assert!(
            set_document_recovery_id(
                &mut conflict_runtime,
                conflict_snapshot.session_generation,
                conflict_snapshot.document_generation,
                u64::MAX,
                conflict_id.clone(),
                conflict_path.to_string_lossy().into_owned(),
                baseline_digest.clone(),
            )
            .is_empty()
        );
        let before_conflict_write = std::fs::read(&conflict_envelope_path).unwrap_or_default();
        assert!(std::fs::write(&conflict_path, b"external replacement").is_ok());
        document_revision_changed(&mut conflict_runtime, u64::MAX - 1, 0, true);
        let rejected_conflict = write_document_file_checked(
            conflict_path.to_string_lossy().into_owned(),
            b"stale document snapshot".to_vec(),
            baseline_digest,
            String::new(),
            conflict_id.clone(),
            u64::MAX,
            0,
        );
        assert!(!rejected_conflict.error.is_empty());
        assert_eq!(
            std::fs::read(&conflict_envelope_path).ok(),
            Some(before_conflict_write),
        );
        conflict_runtime.recovery_lease = None;
        assert!(storage.remove(&conflict_id).is_ok());

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
