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

#[test]
fn checked_snapshot_rejects_external_changes_and_preserves_local_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "caret-document-storage-conflict-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    let path = root.join("document.cdoc");
    let initial = CdocFile::new(Document::blank())?.to_bytes()?;
    let local = CdocFile::new(Document::blank())?.to_bytes()?;
    DocumentStorage.save_snapshot(&path, &initial)?;
    let baseline = crate::document_session::content_digest(&initial);
    fs::write(&path, b"external edit")?;
    let Err(error) = DocumentStorage.save_snapshot_if_unchanged(&path, &local, &baseline) else {
        panic!("external bytes must not be overwritten");
    };
    assert!(matches!(error, DocumentStorageError::ExternalConflict));
    assert_eq!(fs::read(&path)?, b"external edit");
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn queued_snapshot_accepts_the_previous_serialized_app_write()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "caret-document-storage-queued-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    let path = root.join("document.cdoc");
    let initial = CdocFile::new(Document::blank())?.to_bytes()?;
    let mut queued_file = CdocFile::new(Document::blank())?;
    queued_file.extra.insert(
        "queueMarker".to_owned(),
        serde_json::json!("first app write"),
    );
    let queued = queued_file.to_bytes()?;
    let local = CdocFile::new(Document::blank())?.to_bytes()?;
    let baseline = crate::document_session::content_digest(&initial);
    let queued_digest = crate::document_session::content_digest(&queued);

    fs::write(&path, queued)?;
    DocumentStorage.save_snapshot_if_unchanged_or_queued(
        &path,
        &local,
        &baseline,
        Some(&queued_digest),
    )?;
    assert_eq!(fs::read(&path)?, local);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn checked_snapshot_distinguishes_missing_target_from_replaceable_conflict()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "caret-document-storage-missing-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    let path = root.join("removed.cdoc");
    let bytes = CdocFile::new(Document::blank())?.to_bytes()?;
    let error = match DocumentStorage.save_snapshot_if_unchanged(&path, &bytes, "old-digest") {
        Ok(()) => return Err(std::io::Error::other("missing target was recreated").into()),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        DocumentStorageError::ExternalTargetUnavailable(std::io::ErrorKind::NotFound)
    ));
    assert!(!path.exists());
    fs::remove_dir_all(root)?;
    Ok(())
}
