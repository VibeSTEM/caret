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
mod tests {
    use super::{DocumentStorage, DocumentStorageError};
    use caret_doc::{CdocFile, Document, DocumentError};
    use std::fs;
    use std::path::Path;

    #[test]
    fn open_failures_distinguish_file_access_from_invalid_document_contents()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "caret-document-storage-open-errors-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root)?;
        let path = root.join("document.cdoc");
        let Err(missing) = DocumentStorage.open_file(&path) else {
            panic!("missing file should not open");
        };
        assert!(matches!(missing, DocumentStorageError::Io(_)));
        assert!(
            missing
                .to_string()
                .starts_with("document.storage_access_error:")
        );

        fs::write(&path, b"not a document")?;
        let Err(invalid) = DocumentStorage.open_file(&path) else {
            panic!("invalid document contents should not open");
        };
        assert!(matches!(
            invalid,
            DocumentStorageError::Document(DocumentError::InvalidJson(_))
        ));
        assert!(invalid.to_string().starts_with("cdoc.invalid_json:"));
        assert_eq!(fs::read(&path)?, b"not a document");
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn save_rejects_paths_without_a_target_filename_or_explicit_parent()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = CdocFile::new(Document::blank())?;
        let Err(missing_parent) = DocumentStorage.save_file(Path::new("document.cdoc"), &file)
        else {
            panic!("relative target without a parent should not save");
        };
        assert!(matches!(
            missing_parent,
            DocumentStorageError::MissingParent
        ));
        assert_eq!(
            missing_parent.to_string(),
            "document.storage_missing_parent"
        );

        let Err(missing_name) = DocumentStorage.save_file(Path::new("unused/.."), &file) else {
            panic!("target without a filename should not save");
        };
        assert!(matches!(
            missing_name,
            DocumentStorageError::MissingFileName
        ));
        assert_eq!(
            missing_name.to_string(),
            "document.storage_missing_filename"
        );
        Ok(())
    }

    #[test]
    fn snapshot_write_validates_before_replacing_and_round_trips()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "caret-document-storage-snapshot-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root)?;
        let path = root.join("document.cdoc");
        let file = CdocFile::new(caret_doc::Document::blank())?;
        let bytes = file.to_bytes()?;

        DocumentStorage.save_snapshot(&path, &bytes)?;
        assert_eq!(DocumentStorage.read_file_bytes(&path)?, bytes);
        assert!(matches!(
            DocumentStorage.open_file(&path)?,
            caret_doc::OpenedDocument::Editable(_)
        ));

        let Err(invalid) = DocumentStorage.save_snapshot(&path, b"invalid") else {
            panic!("invalid bytes must not replace the existing document");
        };
        assert!(matches!(
            invalid,
            DocumentStorageError::Document(DocumentError::InvalidJson(_))
        ));
        assert_eq!(DocumentStorage.read_file_bytes(&path)?, bytes);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
