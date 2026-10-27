//! 验证文档存储、读写失败和原子替换。

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
    let Err(missing_parent) = DocumentStorage.save_file(Path::new("document.cdoc"), &file) else {
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
