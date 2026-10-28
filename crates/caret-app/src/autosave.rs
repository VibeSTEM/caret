//! Rust 权威管理单文档 AutoSave 的事件 deadline 与写入合并。

use std::time::Duration;

/// 输入停止后的静默等待时间。
pub const QUIET_PERIOD: Duration = Duration::from_secs(1);
/// 持续输入时允许触发保存的最长等待时间。
pub const MAX_WAIT: Duration = Duration::from_secs(30);

/// Qt 单次唤醒应携带的 Rust deadline 身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoSaveDeadline {
    pub session_generation: u64,
    pub generation: u64,
    pub delay: Duration,
}

/// 通过注入的单调时间维护 AutoSave 状态；不创建线程或定时器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoSavePolicy {
    enabled: bool,
    session_generation: u64,
    generation: u64,
    first_edit: Option<Duration>,
    deadline: Option<Duration>,
    latest_revision: u64,
    in_flight_revision: Option<u64>,
}

impl AutoSavePolicy {
    /// 为文档会话创建关闭状态的策略实例。
    pub const fn new(session_generation: u64) -> Self {
        Self {
            enabled: false,
            session_generation,
            generation: 0,
            first_edit: None,
            deadline: None,
            latest_revision: 0,
            in_flight_revision: None,
        }
    }

    /// 切换开关；关闭时使所有已发出的 deadline 失效。
    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        let changed = self.enabled != enabled;
        if !changed {
            return false;
        }
        self.enabled = enabled;
        self.generation = self.generation.saturating_add(1);
        if !enabled {
            self.first_edit = None;
            self.deadline = None;
        }
        true
    }

    /// 文档会话替换时使旧回调失效，同时保留全局开关投影。
    pub fn set_session_generation(&mut self, session_generation: u64) {
        let enabled = self.enabled;
        *self = Self::new(session_generation);
        self.enabled = enabled;
    }

    /// 记录成功编辑并计算下一次单次唤醒 deadline。
    pub fn edited(&mut self, revision: u64, now: Duration) -> Option<AutoSaveDeadline> {
        if !self.enabled || revision <= self.latest_revision {
            return self.current_deadline(now);
        }
        self.latest_revision = revision;
        if self.in_flight_revision.is_none() {
            let first_edit = *self.first_edit.get_or_insert(now);
            let quiet_deadline = now.saturating_add(QUIET_PERIOD);
            let maximum_deadline = first_edit.saturating_add(MAX_WAIT);
            self.deadline = Some(quiet_deadline.min(maximum_deadline));
            self.generation = self.generation.saturating_add(1);
        }
        self.current_deadline(now)
    }

    /// AutoSave 重新启用时，为仍未保存的当前 revision 恢复单次唤醒。
    pub fn resume_dirty_revision(
        &mut self,
        revision: u64,
        now: Duration,
    ) -> Option<AutoSaveDeadline> {
        if !self.enabled || revision == 0 {
            return self.current_deadline(now);
        }
        self.latest_revision = self.latest_revision.max(revision);
        if self.in_flight_revision.is_none() && self.deadline.is_none() {
            self.first_edit = Some(now);
            self.deadline = Some(now.saturating_add(QUIET_PERIOD));
            self.generation = self.generation.saturating_add(1);
        }
        self.current_deadline(now)
    }

    /// 验证 timer 回调；有效时返回应捕获的最新 revision。
    pub fn deadline_fired(
        &mut self,
        session_generation: u64,
        generation: u64,
        now: Duration,
    ) -> Option<u64> {
        if !self.enabled
            || session_generation != self.session_generation
            || generation != self.generation
            || self.deadline.is_none_or(|deadline| now < deadline)
            || self.in_flight_revision.is_some()
            || self.latest_revision == 0
        {
            return None;
        }
        let revision = self.latest_revision;
        self.in_flight_revision = Some(revision);
        self.deadline = None;
        Some(revision)
    }

    /// 完成一次写入；失败不自动重试，成功后合并到最新编辑。
    pub fn save_finished(
        &mut self,
        revision: u64,
        succeeded: bool,
        now: Duration,
    ) -> Option<AutoSaveDeadline> {
        if self.in_flight_revision != Some(revision) {
            return self.current_deadline(now);
        }
        self.in_flight_revision = None;
        if !succeeded {
            self.first_edit = None;
            self.deadline = None;
            return None;
        }
        if self.latest_revision > revision && self.enabled {
            self.first_edit = Some(now);
            self.deadline = Some(now.saturating_add(QUIET_PERIOD));
            self.generation = self.generation.saturating_add(1);
        } else {
            self.first_edit = None;
            self.deadline = None;
        }
        self.current_deadline(now)
    }

    /// 手动保存覆盖了尚未触发的修订时，撤销重复的自动写入。
    pub fn manually_saved(&mut self, revision: u64) {
        if self.in_flight_revision.is_none() && self.latest_revision <= revision {
            self.latest_revision = revision;
            self.first_edit = None;
            self.deadline = None;
            self.generation = self.generation.saturating_add(1);
        }
    }

    /// 只读返回当前有效 deadline，不更改策略状态。
    pub fn deadline(&self, now: Duration) -> Option<AutoSaveDeadline> {
        self.current_deadline(now)
    }

    fn current_deadline(&self, now: Duration) -> Option<AutoSaveDeadline> {
        self.deadline.map(|deadline| AutoSaveDeadline {
            session_generation: self.session_generation,
            generation: self.generation,
            delay: deadline.saturating_sub(now),
        })
    }
}

#[cfg(test)]
mod tests;
