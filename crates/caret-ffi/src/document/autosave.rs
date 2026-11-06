//! 文档 AutoSave deadline、revision 合并与异步写入回报。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{
    DocumentRuntime, bridge, clear_recovery_after_saved, document_save_snapshot,
    document_status_json, empty_save_snapshot, stale_deadline_snapshot,
};
use caret_app::document_lifecycle::DocumentLifecycleDeadline;
use caret_app::document_session::DocumentSessionService;
use std::path::Path;

pub(crate) fn set_document_auto_save_enabled(runtime: &mut DocumentRuntime, enabled: bool) {
    let dirty_revision = runtime
        .runtime
        .as_ref()
        .ok()
        .filter(|session| session.is_dirty())
        .map(DocumentSessionService::document_generation);
    runtime.lifecycle.set_auto_save_enabled(
        enabled,
        dirty_revision,
        runtime.monotonic_origin.elapsed(),
    );
}

/// 返回 Rust 决定的单次唤醒延迟及其会话/deadline generation。
pub(crate) fn auto_save_deadline_json(runtime: &DocumentRuntime) -> String {
    match &runtime.runtime {
        Ok(_session) => {
            let now = runtime.monotonic_origin.elapsed();
            runtime
                .lifecycle
                .next_deadline(now)
                .map(format_lifecycle_deadline)
                .unwrap_or_default()
        }
        Err(_) => String::new(),
    }
}

fn format_lifecycle_deadline(deadline: DocumentLifecycleDeadline) -> String {
    format_deadline(
        caret_app::autosave::AutoSaveDeadline {
            session_generation: deadline.session_generation(),
            generation: deadline.generation(),
            delay: deadline.delay(),
        },
        deadline.is_recovery_update(),
    )
}

fn format_deadline(deadline: caret_app::autosave::AutoSaveDeadline, recovery: bool) -> String {
    format!(
        "{{\"sessionGeneration\":\"{}\",\"generation\":\"{}\",\"delayMs\":{},\"recovery\":{recovery}}}",
        deadline.session_generation,
        deadline.generation,
        deadline.delay.as_millis().min(u128::from(u64::MAX)) as u64,
    )
}

/// 处理恢复副本刷新唤醒；只捕获快照，不在 GUI 线程执行恢复文件 I/O。
pub(crate) fn recovery_update_deadline_fired(
    runtime: &mut DocumentRuntime,
    session_generation: u64,
    generation: u64,
) -> bridge::DocumentSaveSnapshot {
    let now = runtime.monotonic_origin.elapsed();
    let revision = runtime
        .lifecycle
        .recovery_deadline_fired(session_generation, generation, now);
    let Some(revision) = revision else {
        return stale_deadline_snapshot();
    };
    let snapshot = document_save_snapshot(runtime);
    if snapshot.error.is_empty()
        && snapshot.document_generation == revision
        && !snapshot.recovery_id.is_empty()
    {
        snapshot
    } else {
        let _ = runtime
            .lifecycle
            .recovery_update_finished(revision, false, now);
        empty_save_snapshot(if snapshot.error.is_empty() {
            "recovery.stale_revision".to_owned()
        } else {
            snapshot.error
        })
    }
}

/// 报告恢复文件刷新结果；失败不自动重试，后续编辑可再次触发。
#[cfg(test)]
pub(crate) fn recovery_update_write_finished(
    runtime: &mut DocumentRuntime,
    session_generation: u64,
    revision: u64,
    recovery_id: String,
    recovery_binding_generation: u64,
    succeeded: bool,
) -> String {
    recovery_update_write_finished_with_binding(
        runtime,
        session_generation,
        revision,
        recovery_id,
        recovery_binding_generation,
        recovery_binding_generation,
        succeeded,
    )
}

pub(crate) fn recovery_update_write_finished_with_binding(
    runtime: &mut DocumentRuntime,
    session_generation: u64,
    revision: u64,
    recovery_id: String,
    recovery_binding_generation: u64,
    worker_binding_generation: u64,
    succeeded: bool,
) -> String {
    let is_current_session = runtime.runtime.as_ref().is_ok_and(|session| {
        session.session_generation() == session_generation
            && session.recovery_id() == Some(recovery_id.as_str())
    });
    if !is_current_session {
        return "error:document.stale_recovery_completion".to_owned();
    }
    let Some(gate) = runtime.recovery_update_gate.as_ref() else {
        return "error:document.stale_recovery_completion".to_owned();
    };
    let pending = gate.pending.load(std::sync::atomic::Ordering::Acquire);
    let current_generation = gate
        .binding_generation
        .load(std::sync::atomic::Ordering::Acquire);
    let binding_is_current = !pending && current_generation == worker_binding_generation;
    let failed_rebind_is_current = pending
        && !succeeded
        && runtime.recovery_binding_generation == recovery_binding_generation
        && worker_binding_generation == current_generation;
    if !binding_is_current && !failed_rebind_is_current {
        return "error:document.stale_recovery_completion".to_owned();
    }
    let now = runtime.monotonic_origin.elapsed();
    let _ = runtime
        .lifecycle
        .recovery_update_finished(revision, succeeded, now);
    let binding_changed =
        binding_is_current && runtime.recovery_binding_generation != worker_binding_generation;
    if binding_changed {
        runtime.recovery_binding_generation = worker_binding_generation;
        runtime.lifecycle.recovery_binding_changed();
    }
    // 写入期间的新编辑需要重新排期，失败的旧快照不能吞掉这次更新。
    if (binding_changed || (pending && !succeeded))
        && runtime.runtime.as_ref().is_ok_and(|session| {
            session.document_generation() > revision && session.recovery_id().is_some()
        })
    {
        let current_revision = runtime
            .runtime
            .as_ref()
            .map(DocumentSessionService::document_generation)
            .unwrap_or(revision);
        runtime
            .lifecycle
            .resume_recovery_revision(current_revision, now);
    }
    auto_save_deadline_json(runtime)
}

/// 校验 Qt 单次唤醒并在有效时捕获当前文档快照。
pub(crate) fn auto_save_deadline_fired(
    runtime: &mut DocumentRuntime,
    session_generation: u64,
    generation: u64,
) -> bridge::DocumentSaveSnapshot {
    let now = runtime.monotonic_origin.elapsed();
    let revision = runtime
        .lifecycle
        .auto_save_deadline_fired(session_generation, generation, now);
    let Some(revision) = revision else {
        return stale_deadline_snapshot();
    };
    let snapshot = document_save_snapshot(runtime);
    if snapshot.error.is_empty() && snapshot.document_generation == revision {
        snapshot
    } else {
        let _ = runtime.lifecycle.auto_save_finished(revision, false, now);
        empty_save_snapshot(if snapshot.error.is_empty() {
            "autosave.stale_revision".to_owned()
        } else {
            snapshot.error
        })
    }
}

/// 报告 AutoSave 写入结果，成功时只推进该快照对应的保存点。
pub(crate) fn auto_save_write_finished(
    runtime: &mut DocumentRuntime,
    session_generation: u64,
    document_generation: u64,
    history_state_id: u64,
    path: String,
    content_digest: String,
    succeeded: bool,
) -> String {
    let now = runtime.monotonic_origin.elapsed();
    let (current, current_history_state_id) = match &mut runtime.runtime {
        Ok(session) => {
            if session.session_generation() != session_generation {
                return "error:document.stale_save_completion".to_owned();
            }
            if succeeded
                && !session.mark_saved(
                    session_generation,
                    document_generation,
                    history_state_id,
                    Path::new(&path),
                    &content_digest,
                )
            {
                return "error:document.stale_save_completion".to_owned();
            }
            (session.document_generation(), session.history_state_id())
        }
        Err(error) => return format!("error:{error}"),
    };
    let _ = runtime
        .lifecycle
        .auto_save_finished(document_generation, succeeded, now);
    if succeeded && current == document_generation && current_history_state_id == history_state_id {
        clear_recovery_after_saved(runtime);
    }
    if current == document_generation {
        document_status_json(runtime)
    } else {
        auto_save_deadline_json(runtime)
    }
}
