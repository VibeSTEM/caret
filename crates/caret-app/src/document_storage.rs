//! `.cdoc` 文件 I/O。领域模型与 JSON 编解码由 `caret-doc` 提供。

use crate::storage::atomic_file::{self, AtomicFileError};
use caret_doc::{CdocFile, DocumentError, OpenedDocument, open};
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::Path;

/// 文档文件访问失败。解码/校验错误与本地 I/O 错误分开报告。
#[derive(Debug)]
pub enum DocumentStorageError {
    Io(std::io::Error),
    Document(DocumentError),
    MissingParent,
    MissingFileName,
    ExternalConflict,
    ExternalTargetUnavailable(std::io::ErrorKind),
}

impl Display for DocumentStorageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "document.storage_access_error: {error}"),
            Self::Document(error) => Display::fmt(error, formatter),
            Self::MissingParent => write!(formatter, "document.storage_missing_parent"),
            Self::MissingFileName => write!(formatter, "document.storage_missing_filename"),
            Self::ExternalConflict => write!(formatter, "document.external_conflict"),
            Self::ExternalTargetUnavailable(kind) => {
                write!(formatter, "document.external_target_unavailable: {kind:?}")
            }
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
    /// 在后台读取文件字节；文档解析和会话替换由应用层在 Rust 中完成。
    pub fn read_file_bytes(&self, path: &Path) -> Result<Vec<u8>, DocumentStorageError> {
        fs::read(path).map_err(DocumentStorageError::Io)
    }

    pub fn open_file(&self, path: &Path) -> Result<OpenedDocument, DocumentStorageError> {
        let bytes = fs::read(path).map_err(DocumentStorageError::Io)?;
        Ok(open(&bytes)?)
    }

    /// 只接受可编辑文档类型；安全预览无法传入此方法。
    /// 先写入同目录临时文件并同步，再原子替换目标。
    pub fn save_file(&self, path: &Path, document: &CdocFile) -> Result<(), DocumentStorageError> {
        let payload = document.to_bytes()?;
        // 目录同步采用最佳努力；成功返回表示文件已替换，与 Settings 共用同一提交点。
        atomic_file::write(path, &payload)
            .map(|_| ())
            .map_err(DocumentStorageError::from)
    }

    /// 原子写入 Rust 已生成并验证的文档快照。
    pub fn save_snapshot(&self, path: &Path, bytes: &[u8]) -> Result<(), DocumentStorageError> {
        let opened = open(bytes)?;
        let OpenedDocument::Editable(_) = opened else {
            return Err(DocumentStorageError::Document(
                DocumentError::ReadOnlyPreview,
            ));
        };
        atomic_file::write(path, bytes)
            .map(|_| ())
            .map_err(DocumentStorageError::from)
    }

    /// 只有目标摘要仍与打开时基线一致才原子替换；缺失或变化一律拒绝。
    pub fn save_snapshot_if_unchanged(
        &self,
        path: &Path,
        bytes: &[u8],
        expected_digest: &str,
    ) -> Result<(), DocumentStorageError> {
        self.save_snapshot_if_unchanged_or_queued(path, bytes, expected_digest, None)
    }

    /// 允许串行保存队列中前一份本应用快照作为当前文件基线。
    pub fn save_snapshot_if_unchanged_or_queued(
        &self,
        path: &Path,
        bytes: &[u8],
        expected_digest: &str,
        queued_digest: Option<&str>,
    ) -> Result<(), DocumentStorageError> {
        let current = fs::read(path).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => {
                DocumentStorageError::ExternalTargetUnavailable(error.kind())
            }
            _ => DocumentStorageError::ExternalTargetUnavailable(error.kind()),
        })?;
        let current_digest = crate::document_session::content_digest(&current);
        if current_digest != expected_digest
            && queued_digest.is_none_or(|queued| current_digest != queued)
        {
            return Err(DocumentStorageError::ExternalConflict);
        }
        self.save_snapshot(path, bytes)
    }
}

impl From<AtomicFileError> for DocumentStorageError {
    fn from(error: AtomicFileError) -> Self {
        match error {
            AtomicFileError::Io(error) => Self::Io(error),
            AtomicFileError::MissingParent => Self::MissingParent,
            AtomicFileError::MissingFileName => Self::MissingFileName,
        }
    }
}

#[cfg(test)]
mod tests;
