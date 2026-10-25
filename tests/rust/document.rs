use caret_app::document_storage::{DocumentStorage, DocumentStorageError};
use caret_doc::{CdocFile, Document, DocumentError, OpenedDocument, open};
use serde_json::{Value, json};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const MINIMAL_FIXTURE: &str = include_str!("../fixtures/document/v1-minimal.cdoc");

#[test]
fn current_format_fixture_opens_and_round_trips() {
    let opened = must(open(MINIMAL_FIXTURE.as_bytes()));
    let OpenedDocument::Editable(file) = opened else {
        panic!("fixture should be editable");
    };
    let reopened = must(open(&must(file.to_bytes())));
    assert_eq!(reopened, OpenedDocument::Editable(file));
}

#[test]
fn unicode_is_preserved_without_normalization() {
    let source = minimal_document(json!([{
        "type": "paragraph",
        "id": "p-1",
        "inlines": [{"type": "run", "text": "e\u{0301} 👩‍👩‍👧‍👦", "props": {}}]
    }]));
    let opened = must(open(source.as_bytes()));
    let OpenedDocument::Editable(file) = opened else {
        panic!("document should be editable");
    };
    let saved = must(String::from_utf8(must(file.to_bytes())));
    assert!(saved.contains("e\\u0301") || saved.contains("é"));
    assert!(saved.contains("👩‍👩‍👧‍👦"));
}

#[test]
fn unknown_optional_fields_survive_open_and_save() {
    let mut value: Value = must(serde_json::from_str(MINIMAL_FIXTURE));
    value["optionalExtension"] = json!({"revision": 4});
    value["document"]["futureDocumentField"] = json!("keep");
    value["document"]["globalSettings"]["futureSetting"] = json!([1, 2]);
    value["document"]["body"][0]["futureParagraphField"] = json!(true);
    value["document"]["body"][0]["inlines"][0]["futureRunField"] = json!({"x": 1});
    value["document"]["pageSections"][0]["futurePageField"] = json!("keep");

    let opened = must(open(value.to_string().as_bytes()));
    let OpenedDocument::Editable(file) = opened else {
        panic!("document should be editable");
    };
    let output: Value = must(serde_json::from_slice(&must(file.to_bytes())));
    assert_eq!(output["optionalExtension"], json!({"revision": 4}));
    assert_eq!(output["document"]["futureDocumentField"], "keep");
    assert_eq!(
        output["document"]["globalSettings"]["futureSetting"],
        json!([1, 2])
    );
    assert_eq!(output["document"]["body"][0]["futureParagraphField"], true);
    assert_eq!(
        output["document"]["body"][0]["inlines"][0]["futureRunField"],
        json!({"x": 1})
    );
    assert_eq!(
        output["document"]["pageSections"][0]["futurePageField"],
        "keep"
    );
}

#[test]
fn unknown_required_feature_opens_as_non_saveable_preview() {
    let mut value: Value = must(serde_json::from_str(MINIMAL_FIXTURE));
    value["requiredFeatures"] = json!(["vendor.future-feature"]);
    let opened = must(open(value.to_string().as_bytes()));
    let OpenedDocument::SafePreview(preview) = opened else {
        panic!("unknown required feature must be read-only");
    };
    assert!(!preview.can_save());
    assert_eq!(
        preview.raw_json()["requiredFeatures"][0],
        "vendor.future-feature"
    );
    assert!(matches!(
        preview.reasons.as_slice(),
        [caret_doc::PreviewReason::UnsupportedRequiredFeature(feature)]
            if feature == "vendor.future-feature"
    ));
}

#[test]
fn unsupported_theme_snapshot_version_opens_as_non_saveable_preview() {
    for version in [0, 2] {
        let mut value: Value = must(serde_json::from_str(MINIMAL_FIXTURE));
        value["document"]["globalSettings"]["theme"] = json!({
            "formatVersion": version,
            "namedStyles": {},
            "environmentTemplates": {}
        });
        let opened = must(open(value.to_string().as_bytes()));
        let OpenedDocument::SafePreview(preview) = opened else {
            panic!("unsupported theme should be read-only");
        };
        assert!(!preview.can_save());
        assert!(matches!(
            preview.reasons.as_slice(),
            [caret_doc::PreviewReason::UnsupportedThemeVersion(actual)] if *actual == version
        ));
    }
}

#[test]
fn unknown_node_kind_is_preserved_as_opaque_preview_data() {
    let source = minimal_document(json!([{
        "type": "futureBlock",
        "id": "future-1",
        "payload": {"keep": true}
    }]));
    let opened = must(open(source.as_bytes()));
    let OpenedDocument::SafePreview(preview) = opened else {
        panic!("unknown node kind must be read-only");
    };
    assert_eq!(
        preview.raw_json()["document"]["body"][0]["payload"]["keep"],
        true
    );
}

#[test]
fn newer_format_version_is_rejected() {
    let mut value: Value = must(serde_json::from_str(MINIMAL_FIXTURE));
    value["version"] = json!(2);
    assert!(matches!(
        open(value.to_string().as_bytes()),
        Err(DocumentError::UnsupportedDocumentVersion(2))
    ));
}

#[test]
fn direct_run_bold_is_rejected() {
    let source = minimal_document(json!([{
        "type": "paragraph",
        "id": "p-1",
        "inlines": [{"type": "run", "text": "x", "props": {"bold": true}}]
    }]));
    assert!(matches!(
        open(source.as_bytes()),
        Err(DocumentError::InvalidDocument(_))
    ));
}

#[test]
fn duplicate_node_ids_and_multiple_page_sections_are_rejected() {
    let duplicate_id = minimal_document(json!([
        {"type": "paragraph", "id": "p-1", "inlines": []},
        {"type": "paragraph", "id": "p-1", "inlines": []}
    ]));
    assert!(matches!(
        open(duplicate_id.as_bytes()),
        Err(DocumentError::InvalidDocument(_))
    ));

    let mut multiple_pages: Value = must(serde_json::from_str(MINIMAL_FIXTURE));
    multiple_pages["document"]["pageSections"] = json!([
        {"id": "page-1"},
        {"id": "page-2"}
    ]);
    assert!(matches!(
        open(multiple_pages.to_string().as_bytes()),
        Err(DocumentError::InvalidDocument(_))
    ));
}

#[test]
fn malformed_media_base64_is_rejected() {
    let mut value: Value = must(serde_json::from_str(MINIMAL_FIXTURE));
    value["media"] = json!([{
        "id": "media-1",
        "mediaType": "image/png",
        "data": "A==="
    }]);
    assert!(matches!(
        open(value.to_string().as_bytes()),
        Err(DocumentError::InvalidMedia(_))
    ));
}

#[test]
fn invalid_language_tag_is_rejected_and_valid_original_case_is_retained() {
    let invalid = minimal_document_with_language("en--US");
    assert!(matches!(
        open(invalid.as_bytes()),
        Err(DocumentError::InvalidDocument(_))
    ));
    let duplicate_variant = minimal_document_with_language("en-1901-1901");
    assert!(matches!(
        open(duplicate_variant.as_bytes()),
        Err(DocumentError::InvalidDocument(_))
    ));
    let valid = minimal_document_with_language("ZH-hans-cn");
    let opened = must(open(valid.as_bytes()));
    let OpenedDocument::Editable(file) = opened else {
        panic!("document should be editable");
    };
    assert_eq!(
        file.document().global_settings.default_language.as_deref(),
        Some("ZH-hans-cn")
    );
}

#[test]
fn app_storage_saves_and_reopens_document() -> Result<(), Box<dyn std::error::Error>> {
    let directory = test_dir();
    fs::create_dir_all(&directory)?;
    let path = directory.join("sample.cdoc");
    let file = CdocFile::new(Document::blank())?;
    let storage = DocumentStorage;
    storage.save_file(&path, &file)?;
    assert_eq!(storage.open_file(&path)?, OpenedDocument::Editable(file));
    fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn invalid_save_does_not_replace_existing_file() -> Result<(), Box<dyn std::error::Error>> {
    let directory = test_dir();
    fs::create_dir_all(&directory)?;
    let path = directory.join("sample.cdoc");
    fs::write(&path, b"previous valid bytes")?;
    let mut file = CdocFile::new(Document::blank())?;
    file.format = "not-caret".to_owned();
    assert!(matches!(
        DocumentStorage.save_file(&path, &file),
        Err(DocumentStorageError::Document(_))
    ));
    assert_eq!(fs::read(&path)?, b"previous valid bytes");
    fs::remove_dir_all(directory)?;
    Ok(())
}

fn minimal_document(body: Value) -> String {
    json!({
        "format": "caret",
        "version": 1,
        "requiredFeatures": [],
        "media": [],
        "document": {
            "globalSettings": {},
            "body": body,
            "pageSections": [{"id": "page-1", "props": {}}]
        }
    })
    .to_string()
}

fn minimal_document_with_language(language: &str) -> String {
    let mut value: Value = must(serde_json::from_str(MINIMAL_FIXTURE));
    value["document"]["globalSettings"]["defaultLanguage"] = json!(language);
    value.to_string()
}

fn test_dir() -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("caret-document-storage-{nonce}"))
}

fn must<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("unexpected error: {error:?}"),
    }
}
