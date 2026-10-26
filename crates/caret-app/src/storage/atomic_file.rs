//! 同目录临时写入与原子替换；替换后的目录同步失败作为持久性警告返回。

use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
const TEMP_NAME_ATTEMPTS: usize = 128;

#[derive(Debug)]
pub(crate) enum AtomicFileError {
    Io(std::io::Error),
    MissingParent,
    MissingFileName,
}

/// 能返回此结果就表示目标已替换；警告仅表示目录同步无法确认崩溃持久性。
#[derive(Debug)]
pub(crate) struct AtomicWriteOutcome {
    pub(crate) durability_warning: Option<std::io::Error>,
}

pub(crate) fn write(path: &Path, payload: &[u8]) -> Result<AtomicWriteOutcome, AtomicFileError> {
    write_with(
        path,
        payload,
        |file, bytes| file.write_all(bytes),
        sync_parent,
    )
}

fn write_with(
    path: &Path,
    payload: &[u8],
    write_payload: impl FnOnce(&mut fs::File, &[u8]) -> std::io::Result<()>,
    sync_directory: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<AtomicWriteOutcome, AtomicFileError> {
    let parent = path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .ok_or(AtomicFileError::MissingParent)?;
    let file_name = path.file_name().ok_or(AtomicFileError::MissingFileName)?;
    fs::create_dir_all(parent).map_err(AtomicFileError::Io)?;

    let (temporary, mut file) = create_temp_file(parent, file_name)?;
    let written = write_payload(&mut file, payload).and_then(|()| file.sync_all());
    drop(file);
    let replaced = written.and_then(|()| fs::rename(&temporary, path));
    if let Err(error) = replaced {
        let _ = fs::remove_file(&temporary);
        return Err(AtomicFileError::Io(error));
    }
    // rename 是提交点；此后不能返回“未写入”，否则内存与目标文件会发生分歧。
    Ok(AtomicWriteOutcome {
        durability_warning: sync_directory(parent).err(),
    })
}

fn create_temp_file(
    parent: &Path,
    file_name: &OsStr,
) -> Result<(PathBuf, fs::File), AtomicFileError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    for _ in 0..TEMP_NAME_ATTEMPTS {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let mut name = OsString::from(".");
        name.push(file_name);
        name.push(format!(
            ".{}.{timestamp:x}.{sequence:x}.tmp",
            std::process::id()
        ));
        let path = parent.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(AtomicFileError::Io(error)),
        }
    }
    Err(AtomicFileError::Io(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "temporary file names exhausted",
    )))
}

fn sync_parent(parent: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
            |_| Ok(()),
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
                |_| Ok(()),
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
    fn file_in_parent_path_is_preserved_without_writing() -> Result<(), Box<dyn std::error::Error>>
    {
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
            |_| Ok(()),
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
            |file, payload| file.write_all(payload),
            |_| Ok(()),
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
}
