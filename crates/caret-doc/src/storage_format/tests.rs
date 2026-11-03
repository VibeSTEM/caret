//! 验证文档文件格式、往返和兼容边界。

use super::{CdocFile, DocumentError, OpenedDocument, PreviewReason, open};
use crate::{
    Block, Document, MathContent, MathSourceRange, ParagraphContent, Run, TextFlow,
    TextFlowSourceMapEntry,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn blank_envelope() -> Result<Value, Box<dyn std::error::Error>> {
    let file = CdocFile::new(Document::blank())?;
    Ok(serde_json::from_slice(&file.to_bytes()?)?)
}

#[test]
fn math_tree_round_trip_does_not_persist_tex_projections() -> Result<(), Box<dyn std::error::Error>>
{
    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([
        {
            "type": "equation", "id": "equation", "mathTree": {
                "schemaVersion": 3,
                "root": {"id": "root", "content": [
                    {"type": "text", "runs": [{"text": "x"}]}
                ]}
            }
        }
    ]);
    envelope["requiredFeatures"] = json!([
        "caret.math.content-flow.v2",
        "caret.math.equation",
        "caret.math.syntax.v1",
        "caret.paragraph.content-flow.v1",
        "caret.paragraph.layout-settings",
        "caret.text.default-font-size"
    ]);

    let mut document: crate::Document = serde_json::from_value(envelope["document"].clone())?;
    let Block::Equation(equation) = &mut document.body[0] else {
        panic!("expected equation");
    };
    equation.math_tree.source = r"\text{x}".to_owned();
    equation.math_tree.root.source_range = Some(MathSourceRange {
        start: 0,
        end: 8,
        extra: BTreeMap::new(),
    });
    let MathContent::Text(text) = &mut equation.math_tree.root.content[0] else {
        panic!("expected text flow");
    };
    text.source_range = Some(MathSourceRange {
        start: 6,
        end: 7,
        extra: BTreeMap::new(),
    });
    text.source_map.push(TextFlowSourceMapEntry {
        text_start: 0,
        text_end: 1,
        source_range: MathSourceRange {
            start: 6,
            end: 7,
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    });
    let saved = serde_json::to_value(&document)?;
    assert!(saved["body"][0]["mathTree"].get("source").is_none());
    assert!(
        saved["body"][0]["mathTree"]["root"]
            .get("sourceRange")
            .is_none()
    );
    assert!(
        saved["body"][0]["mathTree"]["root"]["content"][0]
            .get("sourceRange")
            .is_none()
    );
    assert!(
        saved["body"][0]["mathTree"]["root"]["content"][0]
            .get("sourceMap")
            .is_none()
    );
    envelope["document"] = saved.clone();
    envelope["requiredFeatures"] = json!([
        "caret.math.content-flow.v2",
        "caret.math.equation",
        "caret.math.syntax.v1",
        "caret.paragraph.content-flow.v1",
        "caret.paragraph.layout-settings",
        "caret.text.default-font-size"
    ]);
    let OpenedDocument::SafePreview(opened) = open(envelope.to_string().as_bytes())? else {
        panic!("math remains read-only until layout support is enabled");
    };
    assert_eq!(opened.to_json_value()?["document"], saved);
    Ok(())
}

#[test]
fn retired_math_source_fields_and_schema_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            json!({"type":"equation","id":"eq","mathTree":{"schemaVersion":2,"source":"x","root":{"id":"root","content":[]}}}),
            "schema",
        ),
        (
            json!({"type":"equation","id":"eq","mathTree":{"schemaVersion":3,"source":"x","root":{"id":"root","content":[]}}}),
            "mathTree.source",
        ),
        (
            json!({"type":"equation","id":"eq","mathTree":{"schemaVersion":3,"root":{"id":"root","sourceRange":{"start":0,"end":1},"content":[]}}}),
            "MathFlow.sourceRange",
        ),
        (
            json!({"type":"equation","id":"eq","mathTree":{"schemaVersion":3,"root":{"id":"root","content":[{"type":"text","runs":[],"sourceMap":[]}]}}}),
            "TextFlow.sourceMap",
        ),
        (
            json!({"type":"equation","id":"eq","mathTree":{"schemaVersion":3,"root":{"id":"root","content":[{"type":"math","id":"x","kind":"symbol","text":"x","sourceRange":{"start":0,"end":1}}]}}}),
            "MathNode.sourceRange",
        ),
        (
            json!({"type":"align","id":"align","source":"x","rows":[{"id":"row","cells":[{"schemaVersion":3,"root":{"id":"root","content":[]}}]}]}),
            "align.source",
        ),
    ];
    for (node, expected_error) in cases {
        let mut envelope = blank_envelope()?;
        envelope["document"]["body"] = json!([node.clone()]);
        envelope["requiredFeatures"] = json!([
            "caret.math.align",
            "caret.math.content-flow.v2",
            "caret.math.equation",
            "caret.math.syntax.v1",
            "caret.paragraph.content-flow.v1",
            "caret.paragraph.layout-settings",
            "caret.text.default-font-size"
        ]);
        let Err(DocumentError::InvalidDocument(reason)) = open(envelope.to_string().as_bytes())
        else {
            panic!("retired math encoding should be rejected: {node:?}");
        };
        assert!(
            reason.contains(expected_error),
            "unexpected error: {reason}"
        );
    }

    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{"type":"futureBlock","source":"vendor payload"}]);
    assert!(matches!(
        open(envelope.to_string().as_bytes())?,
        OpenedDocument::SafePreview(_)
    ));

    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type":"paragraph", "id":"p", "inlines":[{
            "type":"environment", "scope":"inline", "kind":"vendor",
            "sourceRange":{"start":0,"end":1}, "children":[{
                "type":"text", "runs":[], "sourceMap":["vendor"]
            }]
        }]
    }]);
    assert!(matches!(
        open(envelope.to_string().as_bytes())?,
        OpenedDocument::SafePreview(_)
    ));
    Ok(())
}

#[test]
fn ordinary_text_extensions_with_math_projection_names_round_trip()
-> Result<(), Box<dyn std::error::Error>> {
    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type":"paragraph", "id":"p", "inlines":[{
            "type":"text", "runs":[],
            "sourceRange":{"vendor":"preserve"},
            "sourceMap":{"vendor":"preserve"}
        }, {
            "type":"environment", "scope":"inline", "kind":"bold", "id":"bold",
            "sourceRange":{"vendor":"preserve"}, "children":[]
        }]
    }]);
    let OpenedDocument::Editable(file) = open(envelope.to_string().as_bytes())? else {
        panic!("plain text extension fields should remain ordinary unknown metadata");
    };
    let saved = serde_json::to_value(file.document())?;
    assert_eq!(
        saved["body"][0]["inlines"][0]["sourceRange"]["vendor"],
        "preserve"
    );
    assert_eq!(
        saved["body"][0]["inlines"][0]["sourceMap"]["vendor"],
        "preserve"
    );
    assert_eq!(
        saved["body"][0]["inlines"][1]["sourceRange"]["vendor"],
        "preserve"
    );
    Ok(())
}

#[test]
fn unpublished_inline_and_math_text_formats_are_not_migrated()
-> Result<(), Box<dyn std::error::Error>> {
    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type": "paragraph",
        "id": "paragraph",
        "inlines": [{"type": "run", "id": "old-run", "text": "legacy"}]
    }]);
    assert!(matches!(
        open(envelope.to_string().as_bytes()),
        Err(DocumentError::InvalidDocument(_))
    ));

    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type": "paragraph",
        "id": "paragraph",
        "inlines": [{
            "type": "inlineMath",
            "id": "inline",
            "mathTree": {
                "schemaVersion": 2,
                "root": {"id": "root", "content": [{
                    "type": "math", "id": "text-node", "kind": "text_content", "text": "x"
                }]}
            }
        }]
    }]);
    assert!(matches!(
        open(envelope.to_string().as_bytes()),
        Err(DocumentError::InvalidDocument(_))
    ));
    Ok(())
}

#[test]
fn legacy_string_text_nodes_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type": "paragraph",
        "id": "paragraph",
        "inlines": [{"type": "text", "text": "hello"}]
    }]);

    assert!(matches!(
        open(envelope.to_string().as_bytes()),
        Err(DocumentError::InvalidDocument(_))
    ));

    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type": "paragraph",
        "id": "paragraph",
        "inlines": [{"type": "text", "runs": [], "text": "hello"}]
    }]);
    assert!(matches!(
        open(envelope.to_string().as_bytes()),
        Err(DocumentError::InvalidDocument(_))
    ));
    Ok(())
}

#[test]
fn unknown_math_projection_names_remain_on_unrelated_nodes()
-> Result<(), Box<dyn std::error::Error>> {
    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type": "equation", "id": "equation", "mathTree": {
            "schemaVersion": 3,
            "sourceRange": {"vendor": "tree"},
            "root": {"id": "root-flow", "source": "flow", "content": [
                {"type": "text", "runs": [{"text": "x"}], "source": "text"},
                {"type": "math", "id": "symbol", "kind": "symbol", "text": "y",
                 "sourceMap": {"vendor": "node"}}
            ]}
        }
    }]);
    envelope["requiredFeatures"] = json!([
        "caret.math.content-flow.v2",
        "caret.math.equation",
        "caret.math.syntax.v1",
        "caret.paragraph.content-flow.v1",
        "caret.paragraph.layout-settings",
        "caret.text.default-font-size"
    ]);
    let OpenedDocument::SafePreview(preview) = open(envelope.to_string().as_bytes())? else {
        panic!("math is held in safe preview until layout support is enabled");
    };
    let saved = preview.to_json_value()?;
    assert_eq!(
        saved["document"]["body"][0]["mathTree"]["sourceRange"]["vendor"],
        "tree"
    );
    assert_eq!(
        saved["document"]["body"][0]["mathTree"]["root"]["source"],
        "flow"
    );
    assert_eq!(
        saved["document"]["body"][0]["mathTree"]["root"]["content"][0]["source"],
        "text"
    );
    assert_eq!(
        saved["document"]["body"][0]["mathTree"]["root"]["content"][1]["sourceMap"]["vendor"],
        "node"
    );
    Ok(())
}

#[test]
fn math_nodes_require_their_declared_capability() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            json!({
                "type": "paragraph", "id": "paragraph", "inlines": [{
                    "type": "inlineMath", "id": "inline", "mathTree": {
                        "schemaVersion": 3,
                        "root": {"id": "root-flow", "content": [{"type": "math", "id": "root", "kind": "symbol", "text": "x"}]}
                    }
                }]
            }),
            "caret.math.inline",
        ),
        (
            json!({
                "type": "equation", "id": "equation", "mathTree": {
                    "schemaVersion": 3,
                    "root": {"id": "root-flow", "content": [{"type": "math", "id": "root", "kind": "symbol", "text": "x"}]}
                }
            }),
            "caret.math.equation",
        ),
        (
            json!({
                "type": "align", "id": "align", "rows": [{
                    "id": "row", "cells": [{
                        "schemaVersion": 3,
                        "root": {"id": "root-flow", "content": [{"type": "math", "id": "root", "kind": "symbol", "text": "x"}]}
                    }]
                }]
            }),
            "caret.math.align",
        ),
        (
            json!({
                "type": "equation", "id": "equation", "mathTree": {
                    "schemaVersion": 3,
                    "root": {"id": "root-flow", "content": [{"type": "math", "id": "root", "kind": "symbol", "text": "x"}]}
                }
            }),
            "caret.math.content-flow.v2",
        ),
        (
            json!({
                "type": "equation", "id": "equation", "mathTree": {
                    "schemaVersion": 3,
                    "root": {"id": "root-flow", "content": [{"type": "math", "id": "fraction", "kind": "fraction", "attributes": {"style": "frac"}, "children": [
                        {"id": "numerator", "content": [{"type": "math", "id": "x", "kind": "symbol", "text": "x"}]},
                        {"id": "denominator", "content": [{"type": "math", "id": "y", "kind": "symbol", "text": "y"}]}
                    ]}]}
                }
            }),
            "caret.math.syntax.v1",
        ),
    ];
    for (node, missing_feature) in cases {
        let mut envelope = blank_envelope()?;
        envelope["document"]["body"] = json!([node]);
        envelope["requiredFeatures"] = json!([
            "caret.math.align",
            "caret.math.content-flow.v2",
            "caret.math.equation",
            "caret.math.inline",
            "caret.math.syntax.v1",
            "caret.paragraph.content-flow.v1",
            "caret.paragraph.layout-settings",
            "caret.text.default-font-size"
        ]);
        let features = envelope["requiredFeatures"]
            .as_array_mut()
            .ok_or("requiredFeatures should be an array")?;
        features.retain(|feature| feature.as_str() != Some(missing_feature));
        features.sort_by_key(|feature| feature.as_str().unwrap_or_default().to_owned());
        let bytes = envelope.to_string();
        let result = open(bytes.as_bytes());
        let Err(DocumentError::InvalidDocument(reason)) = result else {
            panic!("missing math feature should be rejected: {missing_feature}; got {result:?}");
        };
        assert!(
            reason.contains(missing_feature),
            "unexpected error: {reason}"
        );
    }
    Ok(())
}

#[test]
fn paragraph_content_flow_requires_an_explicit_capability() -> Result<(), Box<dyn std::error::Error>>
{
    let mut envelope = blank_envelope()?;
    envelope["requiredFeatures"] = json!(["caret.paragraph.layout-settings"]);
    let OpenedDocument::SafePreview(preview) = open(envelope.to_string().as_bytes())? else {
        panic!("content-flow documents without their capability must be preview-only");
    };
    assert!(
        preview
            .reasons
            .contains(&PreviewReason::MissingRequiredFeature(
                "caret.paragraph.content-flow.v1".to_owned()
            ))
    );
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
                        "schemaVersion": 4,
                        "root": {"id": "root", "kind": "row", "children": []}
                    }
                }]
            }),
            "caret.math.inline",
        ),
        (
            json!({
                "type": "equation", "id": "equation", "mathTree": {
                    "schemaVersion": 4,
                    "root": {"id": "root", "kind": "row", "children": []}
                }
            }),
            "caret.math.equation",
        ),
        (
            json!({
                "type": "align", "id": "align", "rows": [{
                    "id": "row", "cells": [{
                        "schemaVersion": 4,
                        "root": {"id": "root", "kind": "row", "children": []}
                    }]
                }]
            }),
            "caret.math.align",
        ),
        (
            json!({
                "type": "align", "id": "align", "source": "future source", "rows": [{
                    "id": "row", "cells": [{
                        "schemaVersion": 4,
                        "root": {"id": "root", "content": []}
                    }]
                }]
            }),
            "caret.math.align",
        ),
        (
            json!({
                "type": "paragraph", "id": "paragraph", "inlines": [{
                    "type": "inlineMath", "id": "inline", "mathTree": {
                        "schemaVersion": 4,
                        "root": {"id": "root", "kind": "future_kind", "children": []}
                    }
                }]
            }),
            "caret.math.inline",
        ),
    ];
    for (node, feature) in cases {
        let expected_align_source = node.get("source").and_then(Value::as_str);
        let expected_node = node.clone();
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
        let preserved = preview.to_json_value()?;
        assert!(preserved["document"]["body"][0].is_object());
        if let Some(source) = expected_align_source {
            assert_eq!(preserved["document"]["body"][0], expected_node);
            assert_eq!(preserved["document"]["body"][0]["source"], source);
        }
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
                "schemaVersion": 3,
                "root": {"id": "root-flow", "content": [{
                    "type": "math", "id": "root", "kind": "symbol", "text": "x",
                    "attributes": {"atomClass": "future_atom"}
                }]}
            }
        }]
    }]);
    envelope["requiredFeatures"] = json!([
        "caret.math.content-flow.v2",
        "caret.math.inline",
        "caret.math.syntax.v1",
        "caret.paragraph.content-flow.v1",
        "caret.paragraph.layout-settings",
        "caret.text.default-font-size"
    ]);
    let opened = open(envelope.to_string().as_bytes())?;
    let OpenedDocument::SafePreview(preview) = opened else {
        panic!("unknown math attribute values must not be editable");
    };
    assert!(!preview.can_save());
    assert!(
        preview
            .reasons
            .contains(&PreviewReason::UnsupportedNodeKind)
    );
    let preserved = preview.to_json_value()?;
    assert_eq!(
        preserved["document"]["body"][0]["inlines"][0]["mathTree"]["root"]["content"][0]["attributes"]
            ["atomClass"],
        "future_atom"
    );
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
                    "schemaVersion": 3, "root": root
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
        "inlines": [{"type": "text", "runs": [{"text": "underlined", "props": {"underline": true}}]}]
    }]);
    envelope["requiredFeatures"] =
        json!(["caret.paragraph.content-flow.v1", "caret.text.underline"]);

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
        saved["document"]["body"][0]["inlines"][0]["runs"][0]["props"]["underline"],
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
    envelope["requiredFeatures"] = json!(["caret.paragraph.content-flow.v1", "vendor.future"]);
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
    paragraph.inlines.push(ParagraphContent::Text(TextFlow {
        runs: vec![Run {
            text: "text".to_owned(),
            language: None,
            props: BTreeMap::from([("underline".to_owned(), serde_json::json!(true))]),
            extra: BTreeMap::new(),
        }],
        source_range: None,
        source_map: Vec::new(),
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
    let ParagraphContent::Text(flow) = &mut paragraph.inlines[0] else {
        return Err("edited paragraph should contain a run".into());
    };
    let run = &mut flow.runs[0];
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
