//! 恢复 envelope 往返、非法路径与过期清理测试。

use super::{DocumentRecoveryStorage, RecoveryKind};
use crate::document_session::DocumentSessionService;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

struct RecoveryFixture(PathBuf);

impl RecoveryFixture {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!(
            "caret-recovery-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for RecoveryFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn stores_local_snapshot_and_external_binary_in_one_atomic_envelope()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = RecoveryFixture::new()?;
    let storage = DocumentRecoveryStorage::new(fixture.0.join("recovery"));
    let source = PathBuf::from("/documents/draft.cdoc");
    let id = storage.store(
        RecoveryKind::LocalDocument,
        &source,
        Some("abc".to_owned()),
        b"cdoc",
    )?;
    let (envelope, payload) = storage.load(&id)?;
    assert_eq!(envelope.original_path, source.to_string_lossy());
    assert_eq!(payload, b"cdoc");
    let entries = storage.list_entries()?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, id);
    assert_eq!(entries[0].original_path, source.to_string_lossy());
    assert_eq!(entries[0].baseline_digest.as_deref(), Some("abc"));
    let raw = [0, 1, 0xff, 0x80];
    let external = storage.store(RecoveryKind::ExternalVersion, &source, None, &raw)?;
    assert_eq!(storage.load(&external)?.1, raw);
    assert_eq!(storage.list()?.len(), 2);
    let exported = fixture.0.join("external-backup.txt");
    storage.export_external(&external, &exported)?;
    assert_eq!(fs::read(&exported)?, raw);
    assert!(storage.export_external(&id, &exported).is_err());
    storage.update(
        &id,
        RecoveryKind::LocalDocument,
        &source,
        Some("def".to_owned()),
        b"latest cdoc",
    )?;
    let (updated, latest) = storage.load(&id)?;
    assert_eq!(updated.baseline_digest.as_deref(), Some("def"));
    assert_eq!(updated.created_unix_seconds, envelope.created_unix_seconds);
    assert!(updated.updated_unix_seconds >= updated.created_unix_seconds);
    assert_eq!(latest, b"latest cdoc");
    Ok(())
}

#[test]
fn invalid_id_cannot_escape_recovery_directory() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = RecoveryFixture::new()?;
    let storage = DocumentRecoveryStorage::new(fixture.0.join("recovery"));
    assert!(storage.load("../outside").is_err());
    assert!(storage.remove("../outside").is_err());
    Ok(())
}

#[test]
fn restoring_a_recovery_item_keeps_source_until_session_fork_succeeds()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = RecoveryFixture::new()?;
    let storage = DocumentRecoveryStorage::new(fixture.0.join("recovery"));
    let original_path = PathBuf::from("/documents/draft.cdoc");
    let id = storage.store(
        RecoveryKind::LocalDocument,
        &original_path,
        Some("baseline".to_owned()),
        b"recovered document",
    )?;

    let (envelope, payload, source_lease) = storage.load_for_restore(&id)?;
    let (session_id, session_lease) = storage.fork_for_session(&envelope, &payload)?;

    assert_ne!(session_id, id);
    assert!(matches!(storage.load(&id), Err(super::RecoveryError::Busy)));
    assert_eq!(payload, b"recovered document");
    assert_eq!(envelope.original_path, original_path.to_string_lossy());
    assert!(
        storage
            .list_entries()?
            .iter()
            .all(|entry| entry.id != session_id)
    );
    assert!(matches!(
        storage.load(&session_id),
        Err(super::RecoveryError::Busy)
    ));
    assert!(matches!(
        storage.remove(&session_id),
        Err(super::RecoveryError::Busy)
    ));
    storage.update(
        &session_id,
        RecoveryKind::LocalDocument,
        &original_path,
        Some("baseline".to_owned()),
        b"edited recovered document",
    )?;
    assert!(matches!(
        storage.load(&session_id),
        Err(super::RecoveryError::Busy)
    ));
    storage.remove_leased(&source_lease)?;
    assert!(storage.load(&id).is_err());
    drop(session_lease);
    assert_eq!(storage.load(&session_id)?.1, b"edited recovered document");
    Ok(())
}

#[test]
fn failed_restore_preparation_leaves_original_recovery_item_available()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = RecoveryFixture::new()?;
    let storage = DocumentRecoveryStorage::new(fixture.0.join("recovery"));
    let id = storage.store(
        RecoveryKind::LocalDocument,
        &PathBuf::from("/documents/draft.cdoc"),
        None,
        b"invalid payload",
    )?;

    let (_envelope, payload, lease) = storage.load_for_restore(&id)?;
    assert_eq!(payload, b"invalid payload");
    assert!(
        DocumentSessionService::prepare_recovery(
            &payload,
            "candidate".to_owned(),
            "/documents/draft.cdoc".to_owned(),
            None,
            1,
        )
        .is_err()
    );
    drop(lease);

    assert_eq!(storage.load(&id)?.1, b"invalid payload");
    assert!(storage.list_entries()?.iter().any(|entry| entry.id == id));
    Ok(())
}

#[test]
fn session_recovery_snapshot_is_hidden_until_its_lease_is_released()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = RecoveryFixture::new()?;
    let storage = DocumentRecoveryStorage::new(fixture.0.join("recovery"));
    let other_instance = DocumentRecoveryStorage::new(fixture.0.join("recovery"));
    let (id, lease) = storage.store_for_session(
        &PathBuf::from("/documents/draft.cdoc"),
        Some("baseline".to_owned()),
        b"active snapshot",
    )?;

    assert!(other_instance.list_entries()?.is_empty());
    assert!(matches!(
        other_instance.load(&id),
        Err(super::RecoveryError::Busy)
    ));
    assert!(matches!(
        other_instance.remove(&id),
        Err(super::RecoveryError::Busy)
    ));

    drop(lease);
    assert_eq!(other_instance.load(&id)?.1, b"active snapshot");
    Ok(())
}

#[test]
fn cross_process_lease_child() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(directory) = std::env::var("CARET_RECOVERY_LOCK_TEST_DIRECTORY") else {
        return Ok(());
    };
    let id = std::env::var("CARET_RECOVERY_LOCK_TEST_ID")?;
    let handshake_path = PathBuf::from(std::env::var("CARET_RECOVERY_LOCK_TEST_HANDSHAKE")?);
    let storage = DocumentRecoveryStorage::new(PathBuf::from(directory));
    let _lease = storage.acquire_session_lease(&id)?;
    fs::write(handshake_path.with_extension("ready"), b"ready")?;
    let release_path = handshake_path.with_extension("release");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !release_path.exists() {
        if Instant::now() >= deadline {
            return Err("parent process did not release the recovery lease".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

#[test]
fn cross_process_lease_hides_and_protects_an_active_recovery_item()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = RecoveryFixture::new()?;
    let directory = fixture.0.join("recovery");
    let storage = DocumentRecoveryStorage::new(directory.clone());
    let id = storage.store(
        RecoveryKind::LocalDocument,
        &PathBuf::from("/documents/draft.cdoc"),
        None,
        b"cross-process snapshot",
    )?;
    let handshake_path = fixture.0.join("lease-handshake");
    let ready_path = handshake_path.with_extension("ready");
    let mut child = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "document_recovery::tests::cross_process_lease_child",
            "--nocapture",
        ])
        .env("CARET_RECOVERY_LOCK_TEST_DIRECTORY", &directory)
        .env("CARET_RECOVERY_LOCK_TEST_ID", &id)
        .env("CARET_RECOVERY_LOCK_TEST_HANDSHAKE", &handshake_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready_path.exists() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("child process did not acquire the recovery lease".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    let other_instance = DocumentRecoveryStorage::new(directory);
    let hidden = other_instance
        .list_entries()?
        .iter()
        .all(|entry| entry.id != id);
    let load_rejected = matches!(other_instance.load(&id), Err(super::RecoveryError::Busy));
    let remove_rejected = matches!(other_instance.remove(&id), Err(super::RecoveryError::Busy));

    fs::write(handshake_path.with_extension("release"), b"release")?;
    let child_status = child.wait()?;
    assert!(child_status.success());
    assert!(hidden);
    assert!(load_rejected);
    assert!(remove_rejected);
    assert_eq!(other_instance.load(&id)?.1, b"cross-process snapshot");
    Ok(())
}

#[test]
fn recovery_operation_lock_rejects_concurrent_load_update_and_remove()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = RecoveryFixture::new()?;
    let storage = DocumentRecoveryStorage::new(fixture.0.join("recovery"));
    let id = storage.store(
        RecoveryKind::LocalDocument,
        &PathBuf::from("/documents/draft.cdoc"),
        None,
        b"cdoc",
    )?;
    let _lease = storage.lock_operations()?;

    assert!(matches!(storage.load(&id), Err(super::RecoveryError::Busy)));
    assert!(matches!(
        storage.update(
            &id,
            RecoveryKind::LocalDocument,
            &PathBuf::from("/documents/draft.cdoc"),
            None,
            b"overwritten",
        ),
        Err(super::RecoveryError::Busy)
    ));
    assert!(matches!(
        storage.remove(&id),
        Err(super::RecoveryError::Busy)
    ));
    Ok(())
}

#[test]
fn expiry_cleanup_skips_entries_while_any_process_holds_the_recovery_lease()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = RecoveryFixture::new()?;
    let storage = DocumentRecoveryStorage::new(fixture.0.join("recovery"));
    let id = storage.store(
        RecoveryKind::LocalDocument,
        &PathBuf::from("/documents/draft.cdoc"),
        None,
        b"cdoc",
    )?;
    let envelope_path = fixture.0.join("recovery").join(format!("{id}.json"));
    let mut envelope: serde_json::Value = serde_json::from_slice(&fs::read(&envelope_path)?)?;
    envelope["created_unix_seconds"] = serde_json::json!(1);
    envelope["updated_unix_seconds"] = serde_json::json!(1);
    fs::write(&envelope_path, serde_json::to_vec(&envelope)?)?;

    let lease = storage.acquire_process_lease()?;
    assert_eq!(storage.purge_expired(u64::MAX)?, 0);
    assert!(envelope_path.exists());
    drop(lease);

    assert_eq!(storage.purge_expired(u64::MAX)?, 1);
    assert!(!envelope_path.exists());
    Ok(())
}
