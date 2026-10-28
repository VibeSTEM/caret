//! 协调单文档的自动保存与恢复快照刷新策略。

use crate::autosave::{AutoSaveDeadline, AutoSavePolicy};
use std::time::Duration;

/// 需要由宿主单次唤醒的文档生命周期任务。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentLifecycleDeadline {
    /// 将当前文档快照写入用户文件。
    AutoSave(AutoSaveDeadline),
    /// 将当前文档快照刷新到恢复存储。
    RecoveryUpdate(AutoSaveDeadline),
}

impl DocumentLifecycleDeadline {
    /// 返回此任务所绑定的会话代际。
    pub const fn session_generation(self) -> u64 {
        match self {
            Self::AutoSave(deadline) | Self::RecoveryUpdate(deadline) => {
                deadline.session_generation
            }
        }
    }

    /// 返回此任务的策略代际。
    pub const fn generation(self) -> u64 {
        match self {
            Self::AutoSave(deadline) | Self::RecoveryUpdate(deadline) => deadline.generation,
        }
    }

    /// 返回距任务执行时间的剩余时长。
    pub const fn delay(self) -> Duration {
        match self {
            Self::AutoSave(deadline) | Self::RecoveryUpdate(deadline) => deadline.delay,
        }
    }

    /// 指示任务是否刷新恢复副本。
    pub const fn is_recovery_update(self) -> bool {
        matches!(self, Self::RecoveryUpdate(_))
    }
}

/// 应用层协调两个文档保存策略；宿主只负责按返回 deadline 唤醒和执行 I/O。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentLifecycleService {
    auto_save: AutoSavePolicy,
    recovery_update: AutoSavePolicy,
}

impl DocumentLifecycleService {
    /// 为新文档会话创建关闭状态的生命周期服务。
    pub const fn new(session_generation: u64) -> Self {
        Self {
            auto_save: AutoSavePolicy::new(session_generation),
            recovery_update: AutoSavePolicy::new(session_generation),
        }
    }

    /// 文档会话替换时使两个策略的旧唤醒同时失效。
    pub fn set_session(&mut self, session_generation: u64, has_recovery: bool) {
        self.auto_save.set_session_generation(session_generation);
        self.recovery_update
            .set_session_generation(session_generation);
        self.recovery_update.set_enabled(has_recovery);
    }

    /// 切换用户自动保存设置，并在需要时安排当前 dirty revision。
    pub fn set_auto_save_enabled(
        &mut self,
        enabled: bool,
        dirty_revision: Option<u64>,
        now: Duration,
    ) {
        let changed = self.auto_save.set_enabled(enabled);
        if enabled
            && changed
            && let Some(revision) = dirty_revision
        {
            let _ = self.auto_save.resume_dirty_revision(revision, now);
        }
    }

    /// 记录文档修改，并仅在当前会话绑定恢复项时刷新恢复策略。
    pub fn edited(&mut self, revision: u64, has_recovery: bool, now: Duration) {
        let _ = self.auto_save.edited(revision, now);
        if has_recovery {
            let _ = self.recovery_update.edited(revision, now);
        }
    }

    /// 选择最早的唤醒；时间相同则优先保存用户文件。
    pub fn next_deadline(&self, now: Duration) -> Option<DocumentLifecycleDeadline> {
        match (
            self.auto_save.deadline(now),
            self.recovery_update.deadline(now),
        ) {
            (Some(auto_save), Some(recovery)) if recovery.delay < auto_save.delay => {
                Some(DocumentLifecycleDeadline::RecoveryUpdate(recovery))
            }
            (Some(auto_save), _) => Some(DocumentLifecycleDeadline::AutoSave(auto_save)),
            (None, Some(recovery)) => Some(DocumentLifecycleDeadline::RecoveryUpdate(recovery)),
            (None, None) => None,
        }
    }

    /// 校验并消费 AutoSave 唤醒，返回应捕获的文档 revision。
    pub fn auto_save_deadline_fired(
        &mut self,
        session_generation: u64,
        generation: u64,
        now: Duration,
    ) -> Option<u64> {
        self.auto_save
            .deadline_fired(session_generation, generation, now)
    }

    /// 校验并消费恢复快照唤醒，返回应捕获的文档 revision。
    pub fn recovery_deadline_fired(
        &mut self,
        session_generation: u64,
        generation: u64,
        now: Duration,
    ) -> Option<u64> {
        self.recovery_update
            .deadline_fired(session_generation, generation, now)
    }

    /// 报告 AutoSave 写入结果并返回可能的新 deadline。
    pub fn auto_save_finished(
        &mut self,
        revision: u64,
        succeeded: bool,
        now: Duration,
    ) -> Option<DocumentLifecycleDeadline> {
        let _ = self.auto_save.save_finished(revision, succeeded, now);
        self.next_deadline(now)
    }

    /// 报告恢复快照写入结果并返回可能的新 deadline。
    pub fn recovery_update_finished(
        &mut self,
        revision: u64,
        succeeded: bool,
        now: Duration,
    ) -> Option<DocumentLifecycleDeadline> {
        let _ = self.recovery_update.save_finished(revision, succeeded, now);
        self.next_deadline(now)
    }

    /// 手动保存覆盖待执行的 AutoSave revision 时取消重复写入。
    pub fn manually_saved(&mut self, revision: u64) {
        self.auto_save.manually_saved(revision);
    }

    /// 同步当前恢复绑定状态，避免无恢复项时继续刷新恢复文件。
    pub fn set_recovery_enabled(&mut self, enabled: bool) {
        self.recovery_update.set_enabled(enabled);
    }
}

#[cfg(test)]
mod tests;
