//! 协调文档冲突保存、恢复绑定、租约与后台快照回报。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{
    DocumentRecoveryStorage, RecoveryError, RecoveryKind, RecoverySessionLease,
    RecoverySnapshotIdentity,
};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// 协调同一恢复条目的后台快照更新、绑定代际与待安装租约。
///
/// FFI 只传递此服务返回的快照身份；gate 的串行化和过期裁决由应用层持有。
#[derive(Debug)]
pub struct RecoveryUpdateGate {
    operation: Mutex<()>,
    pending: std::sync::atomic::AtomicBool,
    target_revision: AtomicU64,
    binding_generation: AtomicU64,
}

impl RecoveryUpdateGate {
    fn new(binding_generation: u64) -> Self {
        Self {
            operation: Mutex::new(()),
            pending: std::sync::atomic::AtomicBool::new(false),
            target_revision: AtomicU64::new(0),
            binding_generation: AtomicU64::new(binding_generation),
        }
    }

    /// 当前是否正等待后台重绑恢复项。
    pub fn is_rebind_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }

    /// 返回当前恢复绑定代际。
    pub fn binding_generation(&self) -> u64 {
        self.binding_generation.load(Ordering::Acquire)
    }

    /// 返回待重绑任务绑定的文档修订。
    pub fn target_revision(&self) -> u64 {
        self.target_revision.load(Ordering::Acquire)
    }

    /// 锁定此条目的串行操作；用于在调用方需要与 worker 原子排序时持锁。
    pub fn lock_operation(&self) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        self.operation
            .lock()
            .map_err(|_| "recovery.update_gate_unavailable".to_owned())
    }
}

/// 进程内恢复更新协调器；存储对象仍由应用配置时注入。
#[derive(Debug, Clone, Copy, Default)]
pub struct DocumentRecoveryCoordinator;

fn update_gates()
-> &'static Mutex<std::collections::HashMap<String, std::sync::Arc<RecoveryUpdateGate>>> {
    static GATES: OnceLock<
        Mutex<std::collections::HashMap<String, std::sync::Arc<RecoveryUpdateGate>>>,
    > = OnceLock::new();
    GATES.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn pending_leases() -> &'static Mutex<std::collections::HashMap<String, RecoverySessionLease>> {
    static LEASES: OnceLock<Mutex<std::collections::HashMap<String, RecoverySessionLease>>> =
        OnceLock::new();
    LEASES.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// 恢复条目的应用层异步协调操作。
impl DocumentRecoveryCoordinator {
    /// 条件保存文档，并在外部版本冲突时创建或刷新会话恢复副本。
    pub fn save_checked(
        &self,
        document_storage: &crate::document_storage::DocumentStorage,
        recovery_storage: Option<&DocumentRecoveryStorage>,
        request: CheckedRecoverySave<'_>,
    ) -> CheckedRecoverySaveOutcome {
        let baseline =
            (!request.baseline_digest.is_empty()).then(|| request.baseline_digest.to_owned());
        match document_storage.save_snapshot_if_unchanged_or_queued(
            request.target,
            request.bytes,
            request.baseline_digest,
            request.queued_digest,
        ) {
            Ok(()) => CheckedRecoverySaveOutcome {
                conflict: false,
                replace_allowed: false,
                recovery_id: None,
                recovery_created: false,
                error: None,
            },
            Err(
                error @ (crate::document_storage::DocumentStorageError::ExternalConflict
                | crate::document_storage::DocumentStorageError::ExternalTargetUnavailable(
                    _,
                )),
            ) => {
                let replace_allowed = matches!(
                    error,
                    crate::document_storage::DocumentStorageError::ExternalConflict
                );
                let recovery_created = request.recovery_id.is_none();
                let stored = if let Some(recovery_storage) = recovery_storage {
                    if let Some(id) = request.recovery_id {
                        match self.existing_gate(id) {
                            Ok(gate) => {
                                let update = self.update_snapshot(
                                    recovery_storage,
                                    &gate,
                                    id,
                                    RecoverySnapshotIdentity::new(
                                        request.target,
                                        baseline.as_deref(),
                                        request.binding_generation,
                                        request.document_revision,
                                    ),
                                    false,
                                    request.bytes,
                                );
                                if update.error.is_empty() {
                                    Ok(id.to_owned())
                                } else {
                                    Err(update.error)
                                }
                            }
                            Err(error) => Err(error),
                        }
                    } else {
                        recovery_storage
                            .store_for_session(request.target, baseline, request.bytes)
                            .and_then(|(id, lease)| {
                                self.register_pending_lease(recovery_storage, lease)
                                    .map(|()| id)
                            })
                            .map_err(|error| error.to_string())
                    }
                } else {
                    Err("recovery.storage_unavailable".to_owned())
                };
                match stored {
                    Ok(id) => CheckedRecoverySaveOutcome {
                        conflict: true,
                        replace_allowed,
                        recovery_id: Some(id),
                        recovery_created,
                        error: None,
                    },
                    Err(error) => CheckedRecoverySaveOutcome {
                        conflict: true,
                        replace_allowed: false,
                        recovery_id: None,
                        recovery_created: false,
                        error: Some(error),
                    },
                }
            }
            Err(error) => CheckedRecoverySaveOutcome {
                conflict: false,
                replace_allowed: false,
                recovery_id: None,
                recovery_created: false,
                error: Some(error.to_string()),
            },
        }
    }

    /// 为恢复条目获取进程内串行 gate。
    pub fn gate(
        &self,
        id: &str,
        binding_generation: u64,
    ) -> Result<std::sync::Arc<RecoveryUpdateGate>, String> {
        let mut gates = update_gates()
            .lock()
            .map_err(|_| "recovery.update_gate_unavailable".to_owned())?;
        let gate = gates
            .entry(id.to_owned())
            .or_insert_with(|| std::sync::Arc::new(RecoveryUpdateGate::new(0)))
            .clone();
        if !gate.is_rebind_pending() {
            gate.binding_generation
                .store(binding_generation, Ordering::Release);
        }
        Ok(gate)
    }

    /// 仅取已注册的 gate，未注册的身份视为过期。
    pub fn existing_gate(&self, id: &str) -> Result<std::sync::Arc<RecoveryUpdateGate>, String> {
        update_gates()
            .lock()
            .map_err(|_| "recovery.update_gate_unavailable".to_owned())?
            .get(id)
            .cloned()
            .ok_or_else(|| "recovery.stale_identity".to_owned())
    }

    /// 删除成功后解除 gate 注册；在途 worker 仍持有自己的 Arc。
    pub fn forget_gate(&self, id: &str) {
        if let Ok(mut gates) = update_gates().lock() {
            gates.remove(id);
        }
    }

    /// 将本次绑定更新标记为待后台重绑。
    pub fn mark_rebind_pending(&self, gate: &RecoveryUpdateGate, revision: u64) {
        gate.target_revision.store(revision, Ordering::Relaxed);
        gate.pending.store(true, Ordering::Release);
    }

    /// 更新待重绑任务目标修订。
    pub fn advance_pending_revision(&self, gate: &RecoveryUpdateGate, revision: u64) {
        if gate.is_rebind_pending() {
            gate.target_revision.store(revision, Ordering::Release);
        }
    }

    /// 暂存冲突创建出的新租约，等待 GUI 线程绑定到文档会话。
    pub fn register_pending_lease(
        &self,
        storage: &DocumentRecoveryStorage,
        lease: RecoverySessionLease,
    ) -> Result<(), RecoveryError> {
        match pending_leases().lock() {
            Ok(mut leases) => {
                leases.insert(lease.id().to_owned(), lease);
                Ok(())
            }
            Err(_) => {
                let _ = storage.remove_leased(&lease);
                Err(RecoveryError::Busy)
            }
        }
    }

    /// 取回冲突创建的新租约；没有暂存租约时调用方可向存储申请已有项租约。
    pub fn take_pending_lease(&self, id: &str) -> Option<RecoverySessionLease> {
        pending_leases()
            .lock()
            .ok()
            .and_then(|mut leases| leases.remove(id))
    }

    /// 返回尚未安装到文档会话的冲突恢复租约数量。
    pub fn pending_lease_count(&self) -> usize {
        pending_leases()
            .lock()
            .map(|leases| leases.len())
            .unwrap_or_default()
    }

    /// 取消未绑定的新租约但保留恢复条目。
    pub fn cancel_pending_lease(&self, id: &str) {
        if let Ok(mut leases) = pending_leases().lock() {
            leases.remove(id);
        }
    }

    /// 按 gate 顺序执行必要重绑和快照写入，拒绝被新修订或绑定取代的 worker。
    pub fn update_snapshot(
        &self,
        storage: &DocumentRecoveryStorage,
        gate: &RecoveryUpdateGate,
        id: &str,
        captured: RecoverySnapshotIdentity<'_>,
        rebind_requested: bool,
        bytes: &[u8],
    ) -> RecoveryUpdateOutcome {
        let RecoverySnapshotIdentity {
            original_path,
            baseline_digest,
            binding_generation: captured_binding_generation,
            document_revision,
        } = captured;
        let fail = |binding_generation, error| RecoveryUpdateOutcome {
            binding_generation,
            superseded: false,
            error,
        };
        let superseded = |binding_generation| RecoveryUpdateOutcome {
            binding_generation,
            superseded: true,
            error: "document.stale_recovery_completion".to_owned(),
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
            if !rebind_requested || document_revision != target_revision {
                return superseded(binding_generation);
            }
            match storage.bind_session(
                id,
                original_path,
                baseline_digest.map(str::to_owned),
                document_revision,
            ) {
                Ok(generation) => {
                    binding_generation = generation;
                    gate.binding_generation.store(generation, Ordering::Release);
                    gate.pending.store(false, Ordering::Release);
                }
                Err(error) => return fail(binding_generation, error.to_string()),
            }
        } else if captured_binding_generation != binding_generation
            && !(rebind_requested && target_revision == document_revision)
        {
            return superseded(binding_generation);
        }
        match storage.update_if_identity_matches(
            id,
            RecoveryKind::LocalDocument,
            RecoverySnapshotIdentity::new(
                original_path,
                baseline_digest,
                binding_generation,
                document_revision,
            ),
            bytes,
        ) {
            Ok(()) => RecoveryUpdateOutcome {
                binding_generation,
                superseded: false,
                error: String::new(),
            },
            Err(error) => fail(binding_generation, error.to_string()),
        }
    }

    /// 仲裁 worker 完成回报，并为完成期间产生的新修订恢复恢复快照排期。
    #[allow(clippy::too_many_arguments)]
    pub fn update_finished(
        &self,
        session: &crate::document_session::DocumentSessionService,
        lifecycle: &mut crate::document_lifecycle::DocumentLifecycleService,
        gate: &RecoveryUpdateGate,
        runtime_binding_generation: &mut u64,
        session_generation: u64,
        revision: u64,
        recovery_id: &str,
        recovery_binding_generation: u64,
        worker_binding_generation: u64,
        succeeded: bool,
        now: std::time::Duration,
    ) -> Result<Option<crate::document_lifecycle::DocumentLifecycleDeadline>, &'static str> {
        if session.session_generation() != session_generation
            || session.recovery_id() != Some(recovery_id)
        {
            return Err("document.stale_recovery_completion");
        }
        let pending = gate.is_rebind_pending();
        let current_generation = gate.binding_generation();
        let binding_is_current = !pending && current_generation == worker_binding_generation;
        let failed_rebind_is_current = pending
            && !succeeded
            && *runtime_binding_generation == recovery_binding_generation
            && worker_binding_generation == current_generation;
        if !binding_is_current && !failed_rebind_is_current {
            return Err("document.stale_recovery_completion");
        }
        let _ = lifecycle.recovery_update_finished(revision, succeeded, now);
        let binding_changed =
            binding_is_current && *runtime_binding_generation != worker_binding_generation;
        if binding_changed {
            *runtime_binding_generation = worker_binding_generation;
            lifecycle.recovery_binding_changed();
        }
        if (binding_changed || (pending && !succeeded))
            && session.document_generation() > revision
            && session.recovery_id().is_some()
        {
            lifecycle.resume_recovery_revision(session.document_generation(), now);
        }
        Ok(lifecycle.next_deadline(now))
    }

    /// 将冲突创建的恢复副本绑定到活动会话，并安装对应租约与刷新策略。
    #[allow(clippy::too_many_arguments)]
    pub fn bind_session(
        &self,
        storage: &DocumentRecoveryStorage,
        session: &mut crate::document_session::DocumentSessionService,
        lifecycle: &mut crate::document_lifecycle::DocumentLifecycleService,
        runtime_lease: &mut Option<RecoverySessionLease>,
        runtime_binding_generation: &mut u64,
        runtime_gate: &mut Option<std::sync::Arc<RecoveryUpdateGate>>,
        session_generation: u64,
        source_revision: u64,
        expected_binding_generation: u64,
        id: String,
        original_path: &Path,
        baseline_digest: Option<String>,
        now: std::time::Duration,
    ) -> Result<(), String> {
        let original_path_text = original_path.to_string_lossy();
        let binding_unchanged =
            session.recovery_binding_matches(&id, &original_path_text, baseline_digest.as_deref());
        if session.session_generation() != session_generation
            || *runtime_binding_generation != expected_binding_generation
        {
            return Err("document.stale_recovery_completion".to_owned());
        }
        let lease = if runtime_lease.as_ref().is_some_and(|lease| lease.id() == id) {
            None
        } else if let Some(lease) = self.take_pending_lease(&id) {
            Some(lease)
        } else {
            Some(
                storage
                    .acquire_session_lease(&id)
                    .map_err(|error| error.to_string())?,
            )
        };
        let binding_generation = if binding_unchanged {
            *runtime_binding_generation
        } else {
            storage
                .bind_session(&id, original_path, baseline_digest.clone(), source_revision)
                .map_err(|error| error.to_string())?
        };
        let update_gate = self.gate(&id, binding_generation)?;
        if !session.set_recovery_id(
            session_generation,
            id,
            original_path_text.into_owned(),
            baseline_digest,
        ) {
            return Err("document.stale_recovery_completion".to_owned());
        }
        if let Some(lease) = lease {
            *runtime_lease = Some(lease);
        }
        *runtime_gate = Some(update_gate);
        lifecycle.set_recovery_enabled(true);
        if !binding_unchanged {
            *runtime_binding_generation = binding_generation;
            lifecycle.recovery_binding_changed();
        }
        let current_revision = session.document_generation();
        if current_revision > source_revision {
            lifecycle.edited(current_revision, true, now);
        }
        Ok(())
    }
}

/// 恢复快照后台操作的完成状态，供 FFI 转换为稳定桥接结构。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryUpdateOutcome {
    pub binding_generation: u64,
    pub superseded: bool,
    pub error: String,
}

/// 带恢复副本的条件保存输入；FFI 只负责把其 DTO 值组装成该快照。
pub struct CheckedRecoverySave<'a> {
    pub target: &'a Path,
    pub bytes: &'a [u8],
    pub baseline_digest: &'a str,
    pub queued_digest: Option<&'a str>,
    pub recovery_id: Option<&'a str>,
    pub binding_generation: u64,
    pub document_revision: u64,
}

/// 条件保存与恢复副本协调结果，不含桥接层错误前缀或 JSON 格式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedRecoverySaveOutcome {
    pub conflict: bool,
    pub replace_allowed: bool,
    pub recovery_id: Option<String>,
    pub recovery_created: bool,
    pub error: Option<String>,
}
