//! 验证文档文件格式、往返和兼容边界。

use super::{CdocFile, DocumentError, OpenedDocument, PreviewReason, open};
use crate::{Block, Document, InlineNode, Run};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn blank_envelope() -> Result<Value, Box<dyn std::error::Error>> {
    let file = CdocFile::new(Document::blank())?;
    Ok(serde_json::from_slice(&file.to_bytes()?)?)
}

#[test]
fn math_nodes_open_as_read_only_preview_until_layout_support_is_available()
-> Result<(), Box<dyn std::error::Error>> {
    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([
        {
            "type": "paragraph",
            "id": "paragraph",
            "inlines": [{
                "type": "inlineMath", "id": "inline", "mathTree": {
                    "schemaVersion": 1, "source": "x^2",
                    "root": {"id": "inline-root", "kind": "row", "children": [
                        {"id": "inline-x", "kind": "symbol", "text": "x", "sourceRange": {"start": 0, "end": 1, "vendorRangeHint": 42}},
                        {"id": "inline-power", "kind": "superscript", "children": [
                            {"id": "inline-base", "kind": "symbol", "text": "x"},
                            {"id": "inline-2", "kind": "symbol", "text": "2", "sourceRange": {"start": 2, "end": 3}}
                        ]}
                    ]}
                }
            }]
        },
        {
            "type": "equation", "id": "equation", "mathTree": {
                "schemaVersion": 1, "source": "a+b",
                "root": {"id": "equation-root", "kind": "row", "children": []}
            }
        },
        {
            "type": "align", "id": "align", "source": "a=b",
            "rows": [{
                "id": "align-row", "cells": [{
                    "schemaVersion": 1, "source": "a=b",
                    "root": {"id": "align-cell-root", "kind": "row", "children": []}
                }]
            }]
        }
    ]);
    envelope["requiredFeatures"] = json!([
        "caret.math.align",
        "caret.math.equation",
        "caret.math.inline",
        "caret.paragraph.layout-settings",
        "caret.text.default-font-size"
    ]);

    let opened = open(envelope.to_string().as_bytes())?;
    let OpenedDocument::SafePreview(preview) = opened else {
        panic!("formula nodes must remain read-only until layout support is integrated");
    };
    assert!(!preview.can_save());
    let preserved = preview.to_json_value()?;
    assert_eq!(
        preserved["document"]["body"][0]["inlines"][0]["mathTree"]["source"],
        "x^2"
    );
    assert_eq!(
        preserved["document"]["body"][1]["mathTree"]["source"],
        "a+b"
    );
    assert_eq!(
        preserved["document"]["body"][2]["rows"][0]["cells"][0]["source"],
        "a=b"
    );
    assert!(preview.reasons.iter().any(|reason| matches!(
        reason,
        PreviewReason::UnsupportedRequiredFeature(feature)
            if feature.starts_with("caret.math.")
    )));
    Ok(())
}

#[test]
fn math_nodes_require_their_declared_capability() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            json!({
                "type": "paragraph", "id": "paragraph", "inlines": [{
                    "type": "inlineMath", "id": "inline", "mathTree": {
                        "schemaVersion": 1, "source": "x",
                        "root": {"id": "root", "kind": "symbol", "text": "x"}
                    }
                }]
            }),
            "caret.math.inline",
        ),
        (
            json!({
                "type": "equation", "id": "equation", "mathTree": {
                    "schemaVersion": 1, "source": "x",
                    "root": {"id": "root", "kind": "symbol", "text": "x"}
                }
            }),
            "caret.math.equation",
        ),
        (
            json!({
                "type": "align", "id": "align", "source": "x", "rows": [{
                    "id": "row", "cells": [{
                        "schemaVersion": 1, "source": "x",
                        "root": {"id": "root", "kind": "symbol", "text": "x"}
                    }]
                }]
            }),
            "caret.math.align",
        ),
    ];
    for (node, missing_feature) in cases {
        let mut envelope = blank_envelope()?;
        envelope["document"]["body"] = json!([node]);
        let bytes = envelope.to_string();
        let opened = open(bytes.as_bytes())?;
        let OpenedDocument::SafePreview(preview) = opened else {
            panic!("math data without its required feature must be read-only");
        };
        assert!(
            preview
                .reasons
                .contains(&PreviewReason::MissingRequiredFeature(
                    missing_feature.to_owned()
                ))
        );
    }
    Ok(())
}

#[test]
fn future_math_schema_opens_as_read_only_opaque_preview() -> Result<(), Box<dyn std::error::Error>>
{
    let cases = [
        (
            json!({
                "type": "paragraph", "id": "paragraph", "inlines": [{
                    "type": "inlineMath", "id": "inline", "mathTree": {
                        "schemaVersion": 2, "source": "future",
                        "root": {"id": "root", "kind": "row", "children": []}
                    }
                }]
            }),
            "caret.math.inline",
        ),
        (
            json!({
                "type": "equation", "id": "equation", "mathTree": {
                    "schemaVersion": 2, "source": "future",
                    "root": {"id": "root", "kind": "row", "children": []}
                }
            }),
            "caret.math.equation",
        ),
        (
            json!({
                "type": "align", "id": "align", "source": "future", "rows": [{
                    "id": "row", "cells": [{
                        "schemaVersion": 2, "source": "future",
                        "root": {"id": "root", "kind": "row", "children": []}
                    }]
                }]
            }),
            "caret.math.align",
        ),
        (
            json!({
                "type": "paragraph", "id": "paragraph", "inlines": [{
                    "type": "inlineMath", "id": "inline", "mathTree": {
                        "schemaVersion": 1, "source": "future",
                        "root": {"id": "root", "kind": "future_kind", "children": []}
                    }
                }]
            }),
            "caret.math.inline",
        ),
    ];
    for (node, feature) in cases {
        let mut envelope = blank_envelope()?;
        envelope["document"]["body"] = json!([node]);
        envelope["requiredFeatures"] = json!([
            feature,
            "caret.paragraph.layout-settings",
            "caret.text.default-font-size"
        ]);
        let opened = open(envelope.to_string().as_bytes())?;
        let OpenedDocument::SafePreview(preview) = opened else {
            panic!("future or unknown math data must remain opaque and read-only");
        };
        assert!(!preview.can_save());
        assert!(preview.to_json_value()?["document"]["body"][0].is_object());
    }
    Ok(())
}

#[test]
fn unknown_math_attribute_value_opens_as_read_only_preview()
-> Result<(), Box<dyn std::error::Error>> {
    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type": "paragraph", "id": "paragraph", "inlines": [{
            "type": "inlineMath", "id": "inline", "mathTree": {
                "schemaVersion": 1, "source": "\\frac{a}{b}",
                "root": {"id": "root", "kind": "fraction", "attributes": {"style": "future_fraction"}, "children": []}
            }
        }]
    }]);
    envelope["requiredFeatures"] = json!([
        "caret.math.inline",
        "caret.math.syntax.v1",
        "caret.paragraph.layout-settings",
        "caret.text.default-font-size"
    ]);
    let opened = open(envelope.to_string().as_bytes())?;
    let OpenedDocument::SafePreview(preview) = opened else {
        panic!("unknown math attribute values must not be editable");
    };
    assert!(!preview.can_save());
    Ok(())
}

#[test]
fn incomplete_or_malformed_math_nodes_open_as_read_only_preview()
-> Result<(), Box<dyn std::error::Error>> {
    for root in [
        json!({"id": "root", "kind": "sized_delimiter", "text": "char:(", "attributes": {"atomClass": "open"}, "children": []}),
        json!({"id": "root", "kind": "fraction", "attributes": {"style": "frac"}, "children": []}),
        json!({"id": "root", "kind": "text", "text": "中文", "attributes": {"style": "text"}, "children": []}),
        json!({"id": "root", "kind": "matrix", "attributes": {"style": "equation", "columns": ""}, "children": []}),
        json!({"id": "root", "kind": "hline", "children": []}),
    ] {
        let mut envelope = blank_envelope()?;
        envelope["document"]["body"] = json!([{
            "type": "paragraph", "id": "paragraph", "inlines": [{
                "type": "inlineMath", "id": "inline", "mathTree": {
                    "schemaVersion": 1, "source": "x", "root": root
                }
            }]
        }]);
        envelope["requiredFeatures"] = json!([
            "caret.math.inline",
            "caret.math.syntax.v1",
            "caret.paragraph.layout-settings",
            "caret.text.default-font-size"
        ]);
        let opened = open(envelope.to_string().as_bytes())?;
        let OpenedDocument::SafePreview(preview) = opened else {
            panic!("incomplete or malformed math nodes must not be editable");
        };
        assert!(!preview.can_save());
    }
    Ok(())
}

#[test]
fn default_code_font_family_is_persisted_and_restored() -> Result<(), Box<dyn std::error::Error>> {
    let file = CdocFile::new(Document::blank())?;
    let bytes = file.to_bytes()?;
    let json: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(
        json["document"]["globalSettings"]["defaultCodeFontFamily"],
        "<default>"
    );

    let reopened = open(&bytes)?;
    assert_eq!(
        reopened.document().global_settings.default_code_font_family,
        "<default>"
    );
    Ok(())
}

#[test]
fn underline_property_round_trips_with_its_required_feature()
-> Result<(), Box<dyn std::error::Error>> {
    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type": "paragraph",
        "id": "paragraph",
        "inlines": [{"type": "run", "text": "underlined", "props": {"underline": true}}]
    }]);
    envelope["requiredFeatures"] = json!(["caret.text.underline"]);

    let opened = open(envelope.to_string().as_bytes())?;
    let OpenedDocument::Editable(file) = opened else {
        panic!("supported underline data should be editable");
    };
    let saved: Value = serde_json::from_slice(&file.to_bytes()?)?;
    assert!(
        saved["requiredFeatures"]
            .as_array()
            .is_some_and(|features| features
                .iter()
                .any(|feature| feature == "caret.text.underline"))
    );
    assert_eq!(
        saved["document"]["body"][0]["inlines"][0]["props"]["underline"],
        json!(true)
    );

    envelope["requiredFeatures"] = json!([]);
    let opened = open(envelope.to_string().as_bytes())?;
    let OpenedDocument::SafePreview(preview) = opened else {
        panic!("unmarked underline property must be read-only");
    };
    assert!(
        preview
            .reasons
            .contains(&PreviewReason::MissingRequiredFeature(
                "caret.text.underline".to_owned()
            ))
    );
    assert!(!preview.can_save());
    Ok(())
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
fn unsupported_block_environment_kind_is_preserved_in_safe_preview()
-> Result<(), Box<dyn std::error::Error>> {
    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type": "environment",
        "scope": "block",
        "kind": "future_container",
        "id": "unknown-environment",
        "props": {"displayName": "Extension value"},
        "title": null,
        "children": [{"type": "paragraph", "id": "body", "inlines": []}]
    }]);

    let OpenedDocument::SafePreview(preview) = open(envelope.to_string().as_bytes())? else {
        return Err("unknown environment kinds must use safe preview".into());
    };
    assert!(
        preview
            .reasons
            .contains(&PreviewReason::UnsupportedNodeKind)
    );
    let preserved = preview.to_json_value()?;
    assert_eq!(preserved["document"]["body"][0]["kind"], "future_container");
    assert_eq!(
        preserved["document"]["body"][0]["props"]["displayName"],
        "Extension value"
    );
    assert!(!preview.can_save());
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
fn serialization_adds_underline_feature_after_first_edit_and_rejects_unknown_values()
-> Result<(), Box<dyn std::error::Error>> {
    let mut file = CdocFile::new(Document::blank())?;
    let Block::Paragraph(paragraph) = &mut file.document_mut().body[0] else {
        return Err("blank document should start with a paragraph".into());
    };
    paragraph.inlines.push(InlineNode::Run(Run {
        text: "text".to_owned(),
        language: None,
        props: BTreeMap::from([("underline".to_owned(), serde_json::json!(true))]),
        extra: BTreeMap::new(),
    }));

    let bytes = file.to_bytes()?;
    assert!(
        !file
            .required_features()
            .iter()
            .any(|feature| feature == "caret.text.underline"),
        "serializing an edited document must not mutate the open session"
    );
    let json: Value = serde_json::from_slice(&bytes)?;
    assert!(json["requiredFeatures"].as_array().is_some_and(|features| {
        features
            .iter()
            .any(|feature| feature == "caret.text.underline")
    }));
    assert!(!open(&bytes)?.is_read_only());

    let mut invalid = file;
    let Block::Paragraph(paragraph) = &mut invalid.document_mut().body[0] else {
        return Err("blank document should start with a paragraph".into());
    };
    let InlineNode::Run(run) = &mut paragraph.inlines[0] else {
        return Err("edited paragraph should contain a run".into());
    };
    run.props
        .insert("underline".to_owned(), serde_json::json!("extended"));
    assert_eq!(invalid.to_bytes(), Err(DocumentError::ReadOnlyPreview));
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
