//! `.cdoc` 文件 I/O。领域模型与 JSON 编解码由 `caret-doc` 提供。

use caret_doc::{CdocFile, DocumentError, OpenedDocument, open};
use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

/// 文档文件访问失败。解码/校验错误与本地 I/O 错误分开报告。
#[derive(Debug)]
pub enum DocumentStorageError {
    Io(std::io::Error),
    Document(DocumentError),
    MissingParent,
    MissingFileName,
}

impl Display for DocumentStorageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "document.storage_access_error: {error}"),
            Self::Document(error) => Display::fmt(error, formatter),
            Self::MissingParent => write!(formatter, "document.storage_missing_parent"),
            Self::MissingFileName => write!(formatter, "document.storage_missing_filename"),
        }
    }
}

impl std::error::Error for DocumentStorageError {}

impl From<DocumentError> for DocumentStorageError {
    fn from(error: DocumentError) -> Self {
        Self::Document(error)
    }
}

/// 以显式路径打开或保存 `.cdoc`，不发现平台目录、不依赖 Qt。
#[derive(Debug, Clone, Copy, Default)]
pub struct DocumentStorage;

impl DocumentStorage {
    pub fn open_file(&self, path: &Path) -> Result<OpenedDocument, DocumentStorageError> {
        let bytes = fs::read(path).map_err(DocumentStorageError::Io)?;
        Ok(open(&bytes)?)
    }

    /// 只接受可编辑文档类型；安全预览无法传入此方法。
    /// 先写入同目录临时文件并同步，再原子替换目标。
    pub fn save_file(&self, path: &Path, document: &CdocFile) -> Result<(), DocumentStorageError> {
        let payload = document.to_bytes()?;
        let parent = path
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .ok_or(DocumentStorageError::MissingParent)?;
        let file_name = path
            .file_name()
            .ok_or(DocumentStorageError::MissingFileName)?;
        fs::create_dir_all(parent).map_err(DocumentStorageError::Io)?;

        let (temporary, mut file) = create_temp_file(parent, file_name)?;
        let result = (|| {
            file.write_all(&payload).map_err(DocumentStorageError::Io)?;
            file.sync_all().map_err(DocumentStorageError::Io)?;
            drop(file);
            fs::rename(&temporary, path).map_err(DocumentStorageError::Io)?;
            sync_parent_best_effort(parent);
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn create_temp_file(
    parent: &Path,
    file_name: &std::ffi::OsStr,
) -> Result<(PathBuf, fs::File), DocumentStorageError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    for _ in 0..128 {
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
            Err(error) => return Err(DocumentStorageError::Io(error)),
        }
    }
    Err(DocumentStorageError::Io(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "temporary file names exhausted",
    )))
}

fn sync_parent_best_effort(parent: &Path) {
    #[cfg(unix)]
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    #[cfg(not(unix))]
    let _ = parent;
}
