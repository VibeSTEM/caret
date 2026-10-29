//! 验证文档文件格式、往返和兼容边界。

use super::{CdocFile, DocumentError, OpenedDocument, PreviewReason, open};
use crate::Document;
use serde_json::{Value, json};

fn blank_envelope() -> Result<Value, Box<dyn std::error::Error>> {
    let file = CdocFile::new(Document::blank())?;
    Ok(serde_json::from_slice(&file.to_bytes()?)?)
}

#[test]
fn malformed_envelope_metadata_is_rejected_before_document_decoding()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(matches!(
        open(b"null"),
        Err(DocumentError::InvalidDocument(_))
    ));
    for value in [Value::Null, json!(42), json!("other")] {
        let mut envelope = blank_envelope()?;
        envelope["format"] = value;
        assert!(matches!(
            open(envelope.to_string().as_bytes()),
            Err(DocumentError::InvalidFormat(_))
        ));
    }
    for value in [json!(0), json!(-1), json!(1.5), json!("1"), json!(u64::MAX)] {
        let mut envelope = blank_envelope()?;
        envelope["version"] = value;
        assert!(matches!(
            open(envelope.to_string().as_bytes()),
            Err(DocumentError::InvalidDocument(_))
        ));
    }
    Ok(())
}

#[test]
fn required_feature_metadata_rejects_non_string_and_empty_identifiers()
-> Result<(), Box<dyn std::error::Error>> {
    for features in [json!("caret.env.bold"), json!([42]), json!([""])] {
        let mut envelope = blank_envelope()?;
        envelope["requiredFeatures"] = features;
        let Err(error) = open(envelope.to_string().as_bytes()) else {
            panic!("malformed required features should not open");
        };
        assert!(matches!(error, DocumentError::InvalidRequiredFeatures(_)));
        assert!(
            error
                .to_string()
                .starts_with("cdoc.invalid_required_features:")
        );
    }
    Ok(())
}

#[test]
fn legacy_showcase_documents_open_editably_and_normalize_when_saved()
-> Result<(), Box<dyn std::error::Error>> {
    let mut envelope = blank_envelope()?;
    envelope["requiredFeatures"] = json!(["caret.env.showcase"]);
    envelope["document"]["body"] = json!([{
        "type": "environment",
        "scope": "block",
        "kind": "showcase",
        "id": "legacy-showcase",
        "props": {"displayName": "Old example"},
        "title": null,
        "children": [{"type": "paragraph", "id": "body", "inlines": []}]
    }]);

    let OpenedDocument::Editable(file) = open(envelope.to_string().as_bytes())? else {
        return Err("legacy Showcase should remain editable".into());
    };
    let normalized: Value = serde_json::from_slice(&file.to_bytes()?)?;
    assert_eq!(normalized["document"]["body"][0]["kind"], "custom");
    assert!(
        normalized["requiredFeatures"]
            .as_array()
            .is_some_and(|features| {
                features.iter().any(|feature| feature == "caret.env.custom")
                    && !features
                        .iter()
                        .any(|feature| feature == "caret.env.showcase")
            })
    );
    assert_eq!(
        normalized["document"]["body"][0]["props"]["displayName"],
        "Old example"
    );
    Ok(())
}

#[test]
fn legacy_showcase_with_invalid_new_custom_name_opens_as_preserved_preview()
-> Result<(), Box<dyn std::error::Error>> {
    for display_name in [json!(" "), json!("字".repeat(129)), json!(42)] {
        let mut envelope = blank_envelope()?;
        envelope["document"]["body"] = json!([{
            "type": "environment",
            "scope": "block",
            "kind": "showcase",
            "id": "legacy-showcase",
            "props": {"displayName": display_name},
            "title": null,
            "children": [{"type": "paragraph", "id": "body", "inlines": []}]
        }]);

        let OpenedDocument::SafePreview(preview) = open(envelope.to_string().as_bytes())? else {
            return Err("legacy data conflicting with Custom semantics should be read-only".into());
        };
        assert_eq!(preview.reasons, [PreviewReason::UnsupportedNodeKind]);
        let preserved = preview.to_json_value()?;
        assert_eq!(preserved["document"]["body"][0]["kind"], "showcase");
        assert_eq!(
            preserved["document"]["body"][0]["props"]["displayName"],
            display_name
        );
        assert!(!preview.can_save());
    }
    Ok(())
}

#[test]
fn media_metadata_requires_unique_nonempty_ids_and_a_media_type()
-> Result<(), Box<dyn std::error::Error>> {
    let valid = json!({"id": "asset", "mediaType": "image/png", "data": "AQID"});
    for media in [
        json!([{"id": "", "mediaType": "image/png", "data": "AQID"}]),
        json!([{"id": "asset", "mediaType": "", "data": "AQID"}]),
        json!([valid.clone(), valid]),
    ] {
        let mut envelope = blank_envelope()?;
        envelope["media"] = media;
        let Err(error) = open(envelope.to_string().as_bytes()) else {
            panic!("malformed media metadata should not open");
        };
        assert!(matches!(error, DocumentError::InvalidMedia(_)));
        assert!(error.to_string().starts_with("cdoc.invalid_media:"));
    }
    Ok(())
}

#[test]
fn nested_unknown_content_accumulates_preview_reasons_without_losing_known_structure()
-> Result<(), Box<dyn std::error::Error>> {
    let mut envelope = blank_envelope()?;
    envelope["requiredFeatures"] = json!(["vendor.future"]);
    envelope["document"]["globalSettings"]["theme"] = json!({
        "formatVersion": 2,
        "namedStyles": {},
        "environmentTemplates": {}
    });
    envelope["document"]["body"] = json!([{
        "type": "environment", "scope": "block", "kind": "section", "id": "section",
        "title": {
            "id": "title",
            "inlines": [{
                "type": "environment", "scope": "inline", "kind": "bold", "id": "bold",
                "children": [{"type": "futureInline", "payload": {"preserved": true}}]
            }]
        },
        "children": [{"type": "paragraph", "id": "body", "inlines": []}]
    }]);

    let opened = open(envelope.to_string().as_bytes())?;
    assert!(opened.is_read_only());
    assert_eq!(opened.document().body.len(), 1);
    let OpenedDocument::SafePreview(preview) = opened else {
        panic!("unknown content should open as a safe preview");
    };
    assert_eq!(
        preview.reasons,
        [
            PreviewReason::UnsupportedRequiredFeature("vendor.future".to_owned()),
            PreviewReason::UnsupportedNodeKind,
            PreviewReason::UnsupportedThemeVersion(2),
        ]
    );
    let preserved = preview.to_json_value()?;
    assert_eq!(
        preserved["document"]["body"][0]["title"]["inlines"][0]["children"][0]["payload"],
        json!({"preserved": true})
    );
    assert_eq!(
        preserved["document"]["body"][0]["children"][0]["id"],
        "body"
    );
    assert!(!preview.can_save());
    Ok(())
}

#[test]
fn serialization_recalculates_features_for_an_edited_document_without_mutating_it()
-> Result<(), Box<dyn std::error::Error>> {
    let mut file = CdocFile::new(Document::blank())?;
    assert!(
        !file
            .required_features()
            .iter()
            .any(|feature| feature == "caret.text.language-tags")
    );
    file.document_mut().global_settings.default_language = Some("zh-Hans".to_owned());

    let bytes = file.to_bytes()?;
    assert!(
        !file
            .required_features()
            .iter()
            .any(|feature| feature == "caret.text.language-tags")
    );
    let opened = open(&bytes)?;
    assert!(!opened.is_read_only());
    assert_eq!(
        opened
            .document()
            .global_settings
            .default_language
            .as_deref(),
        Some("zh-Hans")
    );
    let OpenedDocument::Editable(reopened) = opened else {
        return Err("known language feature should remain editable".into());
    };
    assert!(
        reopened
            .required_features()
            .iter()
            .any(|feature| feature == "caret.text.language-tags")
    );
    Ok(())
}

#[test]
fn writing_rejects_incompatible_envelopes_and_preserves_the_document()
-> Result<(), Box<dyn std::error::Error>> {
    let original = CdocFile::new(Document::blank())?;
    let original_bytes = original.to_bytes()?;
    let mut invalid = original.clone();
    invalid.format = "other".to_owned();
    assert_eq!(
        invalid.to_bytes(),
        Err(DocumentError::InvalidFormat("other".to_owned()))
    );
    assert!(serde_json::to_vec(&invalid).is_err());
    assert_eq!(invalid.document(), original.document());

    invalid = original.clone();
    invalid.version = 2;
    assert_eq!(
        invalid.to_bytes(),
        Err(DocumentError::UnsupportedDocumentVersion(2))
    );

    invalid = original.clone();
    invalid.required_features = vec!["vendor.future".to_owned()];
    assert_eq!(invalid.to_bytes(), Err(DocumentError::ReadOnlyPreview));

    invalid = original.clone();
    invalid.required_features = vec!["z".to_owned(), "a".to_owned()];
    assert!(matches!(
        invalid.to_bytes(),
        Err(DocumentError::InvalidRequiredFeatures(_))
    ));
    assert_eq!(original.to_bytes()?, original_bytes);
    Ok(())
}
