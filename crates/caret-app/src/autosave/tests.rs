//! AutoSave deadline 的单调时钟和过期事件测试。

use super::{AutoSavePolicy, MAX_WAIT, QUIET_PERIOD};
use std::time::Duration;

#[test]
fn quiet_period_resets_and_old_deadline_is_ignored() -> Result<(), Box<dyn std::error::Error>> {
    let mut policy = AutoSavePolicy::new(4);
    policy.set_enabled(true);
    let first = policy
        .edited(1, Duration::ZERO)
        .ok_or_else(|| std::io::Error::other("enabled deadline missing"))?;
    let second = policy
        .edited(2, Duration::from_millis(500))
        .ok_or_else(|| std::io::Error::other("refreshed deadline missing"))?;
    assert_eq!(first.delay, QUIET_PERIOD);
    assert_eq!(second.delay, QUIET_PERIOD);
    assert_ne!(first.generation, second.generation);
    assert_eq!(
        policy.deadline_fired(4, first.generation, Duration::from_secs(2)),
        None
    );
    assert_eq!(
        policy.deadline_fired(4, second.generation, Duration::from_millis(1499)),
        None
    );
    assert_eq!(
        policy.deadline_fired(4, second.generation, Duration::from_millis(1500)),
        Some(2)
    );
    Ok(())
}

#[test]
fn continuous_edits_are_bounded_by_maximum_wait() -> Result<(), Box<dyn std::error::Error>> {
    let mut policy = AutoSavePolicy::new(1);
    policy.set_enabled(true);
    let mut deadline = None;
    for tick in 0..31 {
        deadline = policy.edited(tick + 1, Duration::from_secs(tick));
    }
    let deadline = deadline.ok_or_else(|| std::io::Error::other("deadline is no longer armed"))?;
    assert_eq!(deadline.delay, Duration::ZERO);
    assert_eq!(
        policy.deadline_fired(1, deadline.generation, MAX_WAIT),
        Some(31)
    );
    Ok(())
}

#[test]
fn edits_during_write_collapse_to_latest_revision() -> Result<(), Box<dyn std::error::Error>> {
    let mut policy = AutoSavePolicy::new(3);
    policy.set_enabled(true);
    let deadline = policy
        .edited(1, Duration::ZERO)
        .ok_or_else(|| std::io::Error::other("deadline missing"))?;
    assert_eq!(
        policy.deadline_fired(3, deadline.generation, QUIET_PERIOD),
        Some(1)
    );
    assert_eq!(policy.edited(2, Duration::from_secs(2)), None);
    let next = policy
        .save_finished(1, true, Duration::from_secs(3))
        .ok_or_else(|| std::io::Error::other("new revision deadline missing"))?;
    assert_eq!(next.delay, QUIET_PERIOD);
    assert_eq!(
        policy.deadline_fired(3, next.generation, Duration::from_secs(4)),
        Some(2)
    );
    Ok(())
}

#[test]
fn disabling_invalidates_pending_and_inflight_completion_does_not_rearm()
-> Result<(), Box<dyn std::error::Error>> {
    let mut policy = AutoSavePolicy::new(7);
    policy.set_enabled(true);
    let deadline = policy
        .edited(1, Duration::ZERO)
        .ok_or_else(|| std::io::Error::other("deadline missing"))?;
    assert_eq!(
        policy.deadline_fired(7, deadline.generation, QUIET_PERIOD),
        Some(1)
    );
    policy.set_enabled(false);
    assert_eq!(policy.save_finished(1, true, Duration::from_secs(2)), None);
    Ok(())
}

#[test]
fn reenabling_reschedules_the_current_dirty_revision() -> Result<(), Box<dyn std::error::Error>> {
    let mut policy = AutoSavePolicy::new(8);
    policy.set_enabled(true);
    let initial = policy
        .edited(3, Duration::ZERO)
        .ok_or_else(|| std::io::Error::other("deadline missing"))?;
    policy.set_enabled(false);
    policy.set_enabled(true);

    let resumed = policy
        .resume_dirty_revision(3, Duration::from_secs(2))
        .ok_or_else(|| std::io::Error::other("resumed deadline missing"))?;
    assert_ne!(initial.generation, resumed.generation);
    assert_eq!(resumed.delay, QUIET_PERIOD);
    assert_eq!(
        policy.deadline_fired(8, initial.generation, Duration::from_secs(3)),
        None
    );
    assert_eq!(
        policy.deadline_fired(8, resumed.generation, Duration::from_secs(3)),
        Some(3)
    );
    Ok(())
}

#[test]
fn enabling_reports_only_a_real_state_transition() {
    let mut policy = AutoSavePolicy::new(11);
    assert!(!policy.set_enabled(false));
    assert!(policy.set_enabled(true));
    let deadline = policy.edited(1, Duration::ZERO);
    assert!(!policy.set_enabled(true));
    assert_eq!(policy.deadline(Duration::ZERO), deadline);
    assert!(policy.set_enabled(false));
}

#[test]
fn reenable_during_write_does_not_duplicate_and_schedules_newer_revision()
-> Result<(), Box<dyn std::error::Error>> {
    let mut policy = AutoSavePolicy::new(9);
    policy.set_enabled(true);
    let deadline = policy
        .edited(1, Duration::ZERO)
        .ok_or_else(|| std::io::Error::other("deadline missing"))?;
    assert_eq!(
        policy.deadline_fired(9, deadline.generation, QUIET_PERIOD),
        Some(1)
    );
    policy.set_enabled(false);
    policy.set_enabled(true);
    assert_eq!(
        policy.resume_dirty_revision(2, Duration::from_secs(2)),
        None
    );
    let next = policy
        .save_finished(1, true, Duration::from_secs(3))
        .ok_or_else(|| std::io::Error::other("newer revision was not scheduled"))?;
    assert_eq!(next.delay, QUIET_PERIOD);
    assert_eq!(
        policy.deadline_fired(9, next.generation, Duration::from_secs(4)),
        Some(2)
    );
    Ok(())
}

#[test]
fn failed_revision_can_be_rescheduled_after_auto_save_is_reenabled()
-> Result<(), Box<dyn std::error::Error>> {
    let mut policy = AutoSavePolicy::new(10);
    policy.set_enabled(true);
    let deadline = policy
        .edited(4, Duration::ZERO)
        .ok_or_else(|| std::io::Error::other("deadline missing"))?;
    assert_eq!(
        policy.deadline_fired(10, deadline.generation, QUIET_PERIOD),
        Some(4)
    );
    assert_eq!(policy.save_finished(4, false, Duration::from_secs(2)), None);
    policy.set_enabled(false);
    policy.set_enabled(true);

    let resumed = policy
        .resume_dirty_revision(4, Duration::from_secs(3))
        .ok_or_else(|| std::io::Error::other("failed revision was not rescheduled"))?;
    assert_eq!(resumed.delay, QUIET_PERIOD);
    Ok(())
}

#[test]
fn manual_save_invalidates_a_pending_automatic_write() -> Result<(), Box<dyn std::error::Error>> {
    let mut policy = AutoSavePolicy::new(2);
    policy.set_enabled(true);
    let deadline = policy
        .edited(5, Duration::ZERO)
        .ok_or_else(|| std::io::Error::other("deadline missing"))?;
    policy.manually_saved(5);
    assert_eq!(
        policy.deadline_fired(2, deadline.generation, QUIET_PERIOD),
        None
    );
    assert_eq!(policy.deadline(Duration::from_secs(2)), None);
    Ok(())
}
