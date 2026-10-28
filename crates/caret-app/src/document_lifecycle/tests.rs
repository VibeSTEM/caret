//! 文档生命周期策略组合的回归测试。

use super::{DocumentLifecycleDeadline, DocumentLifecycleService};
use std::time::Duration;

#[test]
fn autosave_wins_when_both_deadlines_match() {
    let mut lifecycle = DocumentLifecycleService::new(1);
    lifecycle.set_auto_save_enabled(true, None, Duration::ZERO);
    lifecycle.set_recovery_enabled(true);
    lifecycle.edited(1, true, Duration::ZERO);

    assert!(matches!(
        lifecycle.next_deadline(Duration::ZERO),
        Some(DocumentLifecycleDeadline::AutoSave(_))
    ));
}

#[test]
fn earlier_recovery_deadline_wins() {
    let mut lifecycle = DocumentLifecycleService::new(1);
    lifecycle.set_recovery_enabled(true);
    lifecycle.edited(1, true, Duration::ZERO);
    lifecycle.set_auto_save_enabled(true, Some(1), Duration::from_millis(500));

    assert!(matches!(
        lifecycle.next_deadline(Duration::from_millis(500)),
        Some(DocumentLifecycleDeadline::RecoveryUpdate(_))
    ));
}

#[test]
fn session_replacement_invalidates_old_deadlines_and_keeps_autosave_enabled() {
    let mut lifecycle = DocumentLifecycleService::new(1);
    lifecycle.set_auto_save_enabled(true, None, Duration::ZERO);
    lifecycle.edited(1, false, Duration::ZERO);
    let old = lifecycle.next_deadline(Duration::ZERO);
    assert!(old.is_some());
    let Some(old) = old else { return };

    lifecycle.set_session(2, false);

    assert_eq!(
        lifecycle.auto_save_deadline_fired(
            old.session_generation(),
            old.generation(),
            Duration::from_secs(2),
        ),
        None
    );
}

#[test]
fn failed_write_does_not_schedule_automatic_retry() {
    let mut lifecycle = DocumentLifecycleService::new(1);
    lifecycle.set_auto_save_enabled(true, None, Duration::ZERO);
    lifecycle.edited(1, false, Duration::ZERO);
    let deadline = lifecycle.next_deadline(Duration::ZERO);
    assert!(deadline.is_some());
    let Some(deadline) = deadline else { return };
    let revision = lifecycle.auto_save_deadline_fired(
        deadline.session_generation(),
        deadline.generation(),
        Duration::from_secs(1),
    );
    assert_eq!(revision, Some(1));

    let next = lifecycle.auto_save_finished(1, false, Duration::from_secs(1));

    assert_eq!(next, None);
}
