//! 文档 AutoSave deadline、revision 合并与异步写入回报。
use super::{
    DocumentRuntime, bridge, clear_recovery_after_saved, document_save_snapshot,
    document_status_json, empty_save_snapshot,
};
use std::path::Path;

pub(crate) fn set_document_auto_save_enabled(runtime: &mut DocumentRuntime, enabled: bool) {
    let changed = runtime.auto_save.set_enabled(enabled);
    if enabled
        && changed
        && let Ok(session) = &runtime.runtime
        && session.is_dirty()
    {
        let revision = session.document_generation();
        let _ = runtime
            .auto_save
            .resume_dirty_revision(revision, runtime.monotonic_origin.elapsed());
    }
}

/// 返回 Rust 决定的单次唤醒延迟及其会话/deadline generation。
pub(crate) fn auto_save_deadline_json(runtime: &DocumentRuntime) -> String {
    match &runtime.runtime {
        Ok(_session) => {
            let now = runtime.monotonic_origin.elapsed();
            match (
                runtime.auto_save.deadline(now),
                runtime.recovery_update.deadline(now),
            ) {
                (Some(auto_save), Some(recovery)) => {
                    let (deadline, is_recovery) = if recovery.delay < auto_save.delay {
                        (recovery, true)
                    } else {
                        (auto_save, false)
                    };
                    format_deadline(deadline, is_recovery)
                }
                (Some(deadline), None) => format_deadline(deadline, false),
                (None, Some(deadline)) => format_deadline(deadline, true),
                (None, None) => String::new(),
            }
        }
        Err(_) => String::new(),
    }
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
        .recovery_update
        .deadline_fired(session_generation, generation, now);
    let Some(revision) = revision else {
        return empty_save_snapshot("recovery.stale_deadline".to_owned());
    };
    let snapshot = document_save_snapshot(runtime);
    if snapshot.error.is_empty()
        && snapshot.document_generation == revision
        && !snapshot.recovery_id.is_empty()
    {
        snapshot
    } else {
        let _ = runtime.recovery_update.save_finished(revision, false, now);
        empty_save_snapshot(if snapshot.error.is_empty() {
            "recovery.stale_revision".to_owned()
        } else {
            snapshot.error
        })
    }
}

/// 报告恢复文件刷新结果；失败不自动重试，后续编辑可再次触发。
pub(crate) fn recovery_update_write_finished(
    runtime: &mut DocumentRuntime,
    session_generation: u64,
    revision: u64,
    succeeded: bool,
) -> String {
    if !runtime
        .runtime
        .as_ref()
        .is_ok_and(|session| session.session_generation() == session_generation)
    {
        return "error:document.stale_recovery_completion".to_owned();
    }
    let now = runtime.monotonic_origin.elapsed();
    let _ = runtime
        .recovery_update
        .save_finished(revision, succeeded, now);
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
        .auto_save
        .deadline_fired(session_generation, generation, now);
    let Some(revision) = revision else {
        return empty_save_snapshot("autosave.stale_deadline".to_owned());
    };
    let snapshot = document_save_snapshot(runtime);
    if snapshot.error.is_empty() && snapshot.document_generation == revision {
        snapshot
    } else {
        let _ = runtime.auto_save.save_finished(revision, false, now);
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
    let current = match &mut runtime.runtime {
        Ok(session) => {
            if session.session_generation() != session_generation {
                return "error:document.stale_save_completion".to_owned();
            }
            if succeeded {
                session.mark_saved(
                    session_generation,
                    document_generation,
                    history_state_id,
                    Path::new(&path),
                    &content_digest,
                );
            }
            session.document_generation()
        }
        Err(error) => return format!("error:{error}"),
    };
    let _ = runtime
        .auto_save
        .save_finished(document_generation, succeeded, now);
    if succeeded && current == document_generation {
        clear_recovery_after_saved(runtime);
    }
    if current == document_generation {
        document_status_json(runtime)
    } else {
        auto_save_deadline_json(runtime)
    }
}
