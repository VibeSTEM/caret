use caret_app::document_session::DocumentSessionService;
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

    let generated = CdocFile::new(Document::blank());
    let generated = must(generated);
    let value: Value = must(serde_json::from_slice(&must(generated.to_bytes())));
    let Some(document) = value.get("document").and_then(Value::as_object) else {
        panic!("serialized file should contain a document object");
    };
    for runtime_field in ["layout", "selection", "imeComposition", "glyphRuns"] {
        assert!(!document.contains_key(runtime_field));
    }
}

#[test]
fn invalid_document_serialization_does_not_mutate_the_model() {
    let mut file = must(CdocFile::new(Document::blank()));
    file.document_mut().page_sections.clear();
    let before = file.clone();

    assert!(matches!(
        file.to_bytes(),
        Err(DocumentError::InvalidDocument(_))
    ));
    assert_eq!(file, before);
}

#[test]
fn nested_environment_tree_round_trips() {
    let mut source: Value = must(serde_json::from_str(&minimal_document(json!([{
        "type": "environment",
        "scope": "block",
        "kind": "section",
        "id": "section-1",
        "definitionId": "outline.section",
        "props": {"numbering": {"enabled": true}},
        "futureEnvironmentField": "preserved",
        "title": {
            "id": "title-1",
            "inlines": [{"type": "text", "runs": [{"text": "Heading", "props": {}}]}]
        },
        "children": [{
            "type": "environment",
            "scope": "block",
            "kind": "custom",
            "id": "custom-1",
            "props": {},
            "children": [{
                "type": "paragraph",
                "id": "paragraph-1",
                "inlines": [{
                    "type": "environment",
                    "scope": "inline",
                    "kind": "bold",
                    "id": "bold-1",
                    "props": {"enabled": true},
                    "futureInlineField": 2,
                    "children": [{"type": "text", "runs": [{"text": "text", "props": {}}]}]
                }]
            }]
        }]
    }]))));
    source["document"]["globalSettings"]["theme"] = json!({
        "formatVersion": 1,
        "namedStyles": {},
        "environmentTemplates": {},
        "futureThemeField": {"preserved": true}
    });
    let opened = must(open(source.to_string().as_bytes()));
    let OpenedDocument::Editable(file) = opened else {
        panic!("known nested environments should be editable");
    };
    let reopened = must(open(&must(file.to_bytes())));
    assert_eq!(reopened, OpenedDocument::Editable(file));
}

#[test]
fn first_line_indent_round_trips_through_cdoc_storage() {
    let source = minimal_document(json!([{
        "type": "paragraph",
        "id": "p-1",
        "layout": {"firstLineIndentChars": 2},
        "inlines": [{"type": "text", "runs": [{"text": "indented", "props": {}}]}]
    }]));
    let OpenedDocument::Editable(file) = must(open(source.as_bytes())) else {
        panic!("known first-line indent data should be editable");
    };

    let saved = must(file.to_bytes());
    let encoded: Value = must(serde_json::from_slice(&saved));
    assert_eq!(
        encoded["document"]["body"][0]["layout"]["firstLineIndentChars"],
        2
    );

    let OpenedDocument::Editable(reopened) = must(open(&saved)) else {
        panic!("saved first-line indent data should remain editable");
    };
    let reopened_value: Value = must(serde_json::from_slice(&must(reopened.to_bytes())));
    assert_eq!(
        reopened_value["document"]["body"][0]["layout"]["firstLineIndentChars"],
        2
    );
}

#[test]
fn unicode_is_preserved_without_normalization() {
    let source = minimal_document(json!([{
        "type": "paragraph",
        "id": "p-1",
        "inlines": [{"type": "text", "runs": [{"text": "e\u{0301} 👩‍👩‍👧‍👦", "props": {}}]}]
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
    value["document"]["body"][0]["inlines"][0]["runs"][0]["futureRunField"] = json!({"x": 1});
    value["document"]["pageSections"][0]["futurePageField"] = json!("keep");
    value["media"] = json!([{
        "id": "media-1",
        "mediaType": "image/png",
        "data": "AQID",
        "futureMediaField": "keep"
    }]);

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
        output["document"]["body"][0]["inlines"][0]["runs"][0]["futureRunField"],
        json!({"x": 1})
    );
    assert_eq!(
        output["document"]["pageSections"][0]["futurePageField"],
        "keep"
    );
    assert_eq!(output["media"][0]["futureMediaField"], "keep");
}

#[test]
fn unknown_required_feature_opens_as_non_saveable_preview() {
    let mut value: Value = must(serde_json::from_str(MINIMAL_FIXTURE));
    value["requiredFeatures"] = json!(["caret.paragraph.content-flow.v1", "vendor.future-feature"]);
    let opened = must(open(value.to_string().as_bytes()));
    let OpenedDocument::SafePreview(preview) = opened else {
        panic!("unknown required feature must be read-only");
    };
    assert!(!preview.can_save());
    let preview_json = must(preview.to_json_value());
    assert!(
        preview_json["requiredFeatures"]
            .as_array()
            .is_some_and(|features| features.contains(&json!("vendor.future-feature")))
    );
    assert!(matches!(
        preview.reasons.as_slice(),
        [caret_doc::PreviewReason::UnsupportedRequiredFeature(feature)]
            if feature == "vendor.future-feature"
    ));

    let file: CdocFile = must(serde_json::from_value(value));
    assert!(matches!(
        file.to_bytes(),
        Err(DocumentError::ReadOnlyPreview)
    ));
    assert!(serde_json::to_vec(&file).is_err());
    let directory = test_dir();
    must(fs::create_dir_all(&directory));
    let path = directory.join("unsupported.cdoc");
    must(fs::write(&path, b"previous contents"));
    assert!(matches!(
        DocumentStorage.save_file(&path, &file),
        Err(DocumentStorageError::Document(
            DocumentError::ReadOnlyPreview
        ))
    ));
    assert_eq!(must(fs::read(&path)), b"previous contents");
    must(fs::remove_dir_all(directory));
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
        must(preview.to_json_value())["document"]["body"][0]["payload"]["keep"],
        true
    );

    let file: CdocFile = must(serde_json::from_str(&source));
    assert!(matches!(
        file.to_bytes(),
        Err(DocumentError::ReadOnlyPreview)
    ));
}

#[test]
fn unknown_inline_environment_remains_at_its_original_position() {
    let source = minimal_document(json!([{
        "type": "paragraph",
        "id": "paragraph-1",
        "inlines": [
            {"type": "text", "runs": [{"text": "before", "props": {}}]},
            {
                "type": "environment",
                "scope": "inline",
                "kind": "futureInline",
                "id": "unknown-inline-1",
                "children": [{"type": "text", "runs": [{"text": "opaque", "props": {}}]}]
            },
            {"type": "text", "runs": [{"text": "after", "props": {}}]}
        ]
    }]));
    let opened = must(open(source.as_bytes()));
    let OpenedDocument::SafePreview(preview) = opened else {
        panic!("unknown inline kind must be read-only");
    };
    let value = must(preview.to_json_value());
    assert_eq!(
        value["document"]["body"][0]["inlines"][1]["id"],
        "unknown-inline-1"
    );
}

#[test]
fn unlocatable_unknown_node_boundary_is_rejected() {
    let source = minimal_document(json!(["unknown node has no typed JSON boundary"]));
    assert!(matches!(
        open(source.as_bytes()),
        Err(DocumentError::InvalidDocument(_))
    ));
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
fn duplicate_or_unsorted_required_features_are_rejected() {
    for features in [
        json!(["caret.theme.snapshot", "caret.env.bold"]),
        json!(["caret.env.bold", "caret.env.bold"]),
    ] {
        let mut value: Value = must(serde_json::from_str(MINIMAL_FIXTURE));
        value["requiredFeatures"] = features;
        assert!(matches!(
            open(value.to_string().as_bytes()),
            Err(DocumentError::InvalidRequiredFeatures(_))
        ));
    }
}

#[test]
fn direct_run_bold_is_rejected() {
    let source = minimal_document(json!([{
        "type": "paragraph",
        "id": "p-1",
        "inlines": [{"type": "text", "runs": [{"text": "x", "props": {"bold": true}}]}]
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
    for data in ["A===", "AB==", "ABJ="] {
        let mut value: Value = must(serde_json::from_str(MINIMAL_FIXTURE));
        value["media"] = json!([{
            "id": "media-1",
            "mediaType": "image/png",
            "data": data
        }]);
        assert!(matches!(
            open(value.to_string().as_bytes()),
            Err(DocumentError::InvalidMedia(_))
        ));
    }

    for data in ["AQ==", "AQI="] {
        let mut value: Value = must(serde_json::from_str(MINIMAL_FIXTURE));
        value["media"] = json!([{
            "id": "media-1",
            "mediaType": "image/png",
            "data": data
        }]);
        assert!(matches!(
            open(value.to_string().as_bytes()),
            Ok(OpenedDocument::Editable(_))
        ));
    }
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
    fs::write(&path, b"old document")?;
    let file = CdocFile::new(Document::blank())?;
    let storage = DocumentStorage;
    storage.save_file(&path, &file)?;
    assert_eq!(storage.open_file(&path)?, OpenedDocument::Editable(file));
    fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn document_session_edits_saves_and_reopens_through_public_services()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = test_dir();
    fs::create_dir_all(&directory)?;
    let path = directory.join("edited.cdoc");
    let storage = DocumentStorage;
    let mut session = DocumentSessionService::new_blank()?;

    session.view_mut().insert_text_json("Hello".to_owned())?;
    session.view_mut().split_paragraph_json()?;
    session.view_mut().paste_text_json("你好".to_owned())?;
    let edited: Value = serde_json::from_str(&session.status_json())?;
    assert_eq!(edited["fileName"], "Untitled");
    assert_eq!(edited["dirty"], true);

    let captured = session.save_snapshot()?;
    storage.save_snapshot(&path, &captured.bytes)?;
    assert_eq!(storage.read_file_bytes(&path)?, captured.bytes);
    assert!(session.mark_saved(
        captured.session_generation,
        captured.document_generation,
        captured.history_state_id,
        &path,
        &captured.content_digest,
    ));
    let saved: Value = serde_json::from_str(&session.status_json())?;
    assert_eq!(saved["fileName"], "edited.cdoc");
    assert_eq!(saved["dirty"], false);
    assert_eq!(
        saved["savedGeneration"],
        captured.document_generation.to_string()
    );

    let mut reopened = DocumentSessionService::new_blank()?;
    let expected_session = reopened.session_generation();
    let expected_document = reopened.document_generation();
    let (candidate, snapshot) = DocumentSessionService::prepare_open(
        &storage.read_file_bytes(&path)?,
        &path,
        expected_session + 1,
    )?;
    let snapshot: Value = serde_json::from_str(&reopened.install_prepared(
        candidate,
        snapshot,
        expected_session,
        expected_document,
    )?)?;
    assert_eq!(snapshot["paragraphs"][0]["text"], "Hello");
    assert_eq!(snapshot["paragraphs"][1]["text"], "你好");
    let reopened_status: Value = serde_json::from_str(&reopened.status_json())?;
    assert_eq!(reopened_status["fileName"], "edited.cdoc");
    assert_eq!(reopened_status["dirty"], false);
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
        "requiredFeatures": ["caret.paragraph.content-flow.v1"],
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
