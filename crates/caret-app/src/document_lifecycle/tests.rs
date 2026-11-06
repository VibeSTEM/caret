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
fn revision_epoch_rollover_rejects_old_deadlines_and_preserves_draft_state() {
    let mut lifecycle = DocumentLifecycleService::new(1);
    lifecycle.set_auto_save_enabled(true, None, Duration::ZERO);
    lifecycle.edited(u64::MAX, false, Duration::ZERO);
    let Some(old) = lifecycle.next_deadline(Duration::ZERO) else {
        panic!("old epoch should have an autosave deadline");
    };
    lifecycle.set_math_source_draft_pending(true, true);

    lifecycle.set_session_generation(2, false);
    lifecycle.edited(0, false, Duration::from_secs(2));

    assert_eq!(lifecycle.next_deadline(Duration::from_secs(2)), None);
    assert_eq!(
        lifecycle.auto_save_deadline_fired(
            old.session_generation(),
            old.generation(),
            Duration::from_secs(3),
        ),
        None
    );
    lifecycle.set_math_source_draft_pending(false, true);
    let Some(current) = lifecycle.next_deadline(Duration::from_secs(3)) else {
        panic!("zero revision should resume after the draft ends");
    };
    assert_eq!(current.session_generation(), 2);
    assert_eq!(
        lifecycle.auto_save_deadline_fired(
            current.session_generation(),
            current.generation(),
            Duration::from_secs(3),
        ),
        Some(0)
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

#[test]
fn math_source_draft_pauses_and_resumes_dirty_autosave_revision() {
    let mut lifecycle = DocumentLifecycleService::new(11);
    lifecycle.set_auto_save_enabled(true, None, Duration::ZERO);
    lifecycle.edited(7, false, Duration::ZERO);
    let Some(before_draft) = lifecycle.next_deadline(Duration::ZERO) else {
        panic!("dirty revision should have an autosave deadline");
    };

    lifecycle.set_math_source_draft_pending(true, true);

    assert_eq!(lifecycle.next_deadline(Duration::from_secs(2)), None);
    assert_eq!(
        lifecycle.auto_save_deadline_fired(
            before_draft.session_generation(),
            before_draft.generation(),
            Duration::from_secs(2),
        ),
        None
    );

    lifecycle.set_math_source_draft_pending(false, true);
    let Some(resumed) = lifecycle.next_deadline(Duration::from_secs(2)) else {
        panic!("cancelled draft should resume autosaving the retained dirty revision");
    };
    assert!(matches!(resumed, DocumentLifecycleDeadline::AutoSave(_)));
    assert_ne!(resumed.generation(), before_draft.generation());
    assert_eq!(
        lifecycle.auto_save_deadline_fired(
            before_draft.session_generation(),
            before_draft.generation(),
            Duration::from_secs(3),
        ),
        None
    );
    assert_eq!(
        lifecycle.auto_save_deadline_fired(
            resumed.session_generation(),
            resumed.generation(),
            Duration::from_secs(3),
        ),
        Some(7)
    );
}

#[test]
fn math_source_draft_pauses_and_resumes_recovery_revision() {
    let mut lifecycle = DocumentLifecycleService::new(12);
    lifecycle.set_recovery_enabled(true);
    lifecycle.edited(9, true, Duration::ZERO);
    let Some(before_draft) = lifecycle.next_deadline(Duration::ZERO) else {
        panic!("dirty recovery revision should have a deadline");
    };
    assert!(before_draft.is_recovery_update());

    lifecycle.set_math_source_draft_pending(true, true);
    assert_eq!(lifecycle.next_deadline(Duration::from_secs(2)), None);
    assert_eq!(
        lifecycle.recovery_deadline_fired(
            before_draft.session_generation(),
            before_draft.generation(),
            Duration::from_secs(2),
        ),
        None
    );

    lifecycle.set_math_source_draft_pending(false, false);
    let Some(resumed) = lifecycle.next_deadline(Duration::from_secs(2)) else {
        panic!("cancelled draft should resume the recovery revision");
    };
    assert!(resumed.is_recovery_update());
    assert_ne!(resumed.generation(), before_draft.generation());
    assert_eq!(
        lifecycle.recovery_deadline_fired(
            before_draft.session_generation(),
            before_draft.generation(),
            Duration::from_secs(3),
        ),
        None
    );
    assert_eq!(
        lifecycle.recovery_deadline_fired(
            resumed.session_generation(),
            resumed.generation(),
            Duration::from_secs(3),
        ),
        Some(9)
    );
}

#[test]
fn failed_inflight_autosave_is_not_retried_when_math_draft_is_cancelled() {
    let mut lifecycle = DocumentLifecycleService::new(14);
    lifecycle.set_auto_save_enabled(true, None, Duration::ZERO);
    lifecycle.edited(6, false, Duration::ZERO);
    let Some(deadline) = lifecycle.next_deadline(Duration::ZERO) else {
        panic!("dirty revision should have an autosave deadline");
    };
    assert_eq!(
        lifecycle.auto_save_deadline_fired(
            deadline.session_generation(),
            deadline.generation(),
            Duration::from_secs(1),
        ),
        Some(6)
    );
    lifecycle.set_math_source_draft_pending(true, true);
    assert_eq!(
        lifecycle.auto_save_finished(6, false, Duration::from_secs(2)),
        None
    );

    lifecycle.set_math_source_draft_pending(false, true);

    assert_eq!(lifecycle.next_deadline(Duration::from_secs(2)), None);
}

#[test]
fn cancelling_clean_math_draft_does_not_create_a_recovery_write() {
    let mut lifecycle = DocumentLifecycleService::new(15);
    lifecycle.set_recovery_enabled(true);
    lifecycle.set_math_source_draft_pending(true, true);

    lifecycle.set_math_source_draft_pending(false, false);

    assert_eq!(lifecycle.next_deadline(Duration::ZERO), None);
}

#[test]
fn enabling_lifecycle_while_math_draft_is_pending_hides_deadlines_until_resolution() {
    let mut lifecycle = DocumentLifecycleService::new(13);
    lifecycle.set_math_source_draft_pending(true, true);
    lifecycle.set_auto_save_enabled(true, Some(4), Duration::ZERO);
    lifecycle.set_recovery_enabled(true);
    lifecycle.edited(5, true, Duration::from_secs(1));

    assert_eq!(lifecycle.next_deadline(Duration::from_secs(2)), None);

    lifecycle.set_math_source_draft_pending(false, true);
    assert!(lifecycle.next_deadline(Duration::from_secs(2)).is_some());
}
