//! 验证原子文件写入和失败后的旧文件保留。

use super::{AtomicFileError, write_with};
use std::cell::Cell;
use std::fs;
use std::io::{Error, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir {
    root: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let root = std::env::temp_dir().join(format!(
            "caret-atomic-save-{}-{label}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn failed_temporary_write_preserves_existing_file() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new("write-failure")?;
    let target = directory.root.join("document.cdoc");
    fs::write(&target, b"previous document")?;

    let result = write_with(
        &target,
        b"replacement document",
        |file, payload| {
            file.write_all(&payload[..5])?;
            Err(Error::new(ErrorKind::WriteZero, "injected write failure"))
        },
        super::sync_parent,
    );

    assert!(matches!(result, Err(AtomicFileError::Io(_))));
    assert_eq!(fs::read(&target)?, b"previous document");
    let entries = fs::read_dir(&directory.root)?.count();
    assert_eq!(entries, 1, "failed save must remove its temporary file");
    Ok(())
}

#[test]
fn missing_path_components_are_rejected_before_writing() {
    for (target, missing_parent) in [
        (Path::new("document.cdoc"), true),
        (Path::new("parent/.."), false),
    ] {
        let wrote = Cell::new(false);
        let result = write_with(
            target,
            b"replacement",
            |_, _| {
                wrote.set(true);
                Ok(())
            },
            super::sync_parent,
        );
        assert!(match result {
            Err(AtomicFileError::MissingParent) => missing_parent,
            Err(AtomicFileError::MissingFileName) => !missing_parent,
            _ => false,
        });
        assert!(!wrote.get(), "invalid target must not reach the writer");
    }
}

#[test]
fn file_in_parent_path_is_preserved_without_writing() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new("blocked-parent")?;
    let blocker = directory.root.join("blocked");
    fs::write(&blocker, b"existing file")?;
    let wrote = Cell::new(false);
    let result = write_with(
        &blocker.join("document.cdoc"),
        b"replacement",
        |_, _| {
            wrote.set(true);
            Ok(())
        },
        super::sync_parent,
    );

    assert!(matches!(result, Err(AtomicFileError::Io(_))));
    assert!(!wrote.get(), "parent failure must not reach the writer");
    assert_eq!(fs::read(&blocker)?, b"existing file");
    assert_eq!(fs::read_dir(&directory.root)?.count(), 1);
    Ok(())
}

#[test]
fn failed_replacement_preserves_target_directory_and_removes_temporary_file()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new("rename-failure")?;
    let target = directory.root.join("document.cdoc");
    fs::create_dir(&target)?;
    let previous = target.join("previous.txt");
    fs::write(&previous, b"previous contents")?;

    let result = write_with(
        &target,
        b"replacement document",
        fs::File::write_all,
        super::sync_parent,
    );

    assert!(matches!(result, Err(AtomicFileError::Io(_))));
    assert_eq!(fs::read(previous)?, b"previous contents");
    assert_eq!(fs::read_dir(&directory.root)?.count(), 1);
    Ok(())
}
#[test]
fn directory_sync_failure_reports_committed_content_with_warning()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new("sync-warning")?;
    let target = directory.root.join("settings.json");
    fs::write(&target, b"previous")?;
    let result = write_with(
        &target,
        b"committed",
        |file, bytes| file.write_all(bytes),
        |_| {
            Err(Error::new(
                ErrorKind::PermissionDenied,
                "injected directory sync failure",
            ))
        },
    );
    let outcome = result.map_err(|error| format!("{error:?}"))?;
    assert!(outcome.durability_warning.is_some());
    assert_eq!(fs::read(&target)?, b"committed");
    assert_eq!(fs::read_dir(&directory.root)?.count(), 1);
    Ok(())
}

#[test]
fn unusable_temporary_filename_preserves_the_directory() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new("long-filename")?;
    let target = directory.root.join(format!("{}.cdoc", "a".repeat(512)));

    let result = super::write(&target, b"replacement");
    assert!(matches!(result, Err(AtomicFileError::Io(_))));
    assert_eq!(fs::read_dir(&directory.root)?.count(), 0);
    Ok(())
}
