//! 验证文档文件格式、往返和兼容边界。

use super::{CdocFile, DocumentError, OpenedDocument, PreviewReason, open};
use crate::{
    Align, Block, Document, Equation, MathAlignRow, MathContent, MathFlow, MathTree, NodeId,
    OpaqueNode, ParagraphContent, Run, TextFlow,
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

    let document: crate::Document = serde_json::from_value(envelope["document"].clone())?;
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
fn math_carriers_decode_and_open_as_typed_safe_preview_preserving_ids_and_unknown_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let mut envelope = blank_envelope()?;
    let inline_math_tree = json!({
        "schemaVersion": 3, "vendorTree": "inline",
        "root": {"id": "inline-root-id", "content": [{
            "type": "environment", "scope": "inline", "id": "inline-env-id",
            "kind": "bold", "vendorEnvironment": "kept", "children": [{
                "type": "math", "id": "inline-fraction", "kind": "fraction",
                "attributes": {"style": "frac"}, "children": [
                    {"id": "inline-numerator", "content": [{
                        "type": "text", "vendorText": 1, "runs": [
                            {"text": "x", "props": {"underline": true}, "vendorRun": "kept"}
                        ]
                    }]},
                    {"id": "inline-denominator", "content": []}
                ]
            }]
        }]}
    });
    envelope["document"]["body"] = json!([
        {
            "type": "paragraph", "id": "paragraph-id", "vendorParagraph": {"kept": true},
            "inlines": [{
                "type": "inlineMath", "id": "inline-math-id", "vendorInlineMath": "kept",
            "mathTree": inline_math_tree
            }]
        },
        {
            "type": "equation", "id": "equation-id", "vendorEquation": [1, 2],
            "mathTree": {
                "schemaVersion": 3, "vendorEquationTree": "kept",
                "root": {"id": "equation-root-id", "content": [
                    {"type": "math", "id": "equation-symbol-id", "kind": "symbol", "text": "y", "vendorEquationNode": "kept"}
                ]}
            }
        },
        {
            "type": "align", "id": "align-id", "vendorAlign": "kept",
            "rows": [{
                "id": "align-row-id", "vendorRow": 3,
                "cells": [{
                    "schemaVersion": 3, "vendorCellTree": "kept",
                    "root": {"id": "align-cell-root-id", "content": [
                        {"type": "math", "id": "align-symbol-id", "kind": "symbol", "text": "z", "vendorAlignNode": "kept"}
                    ]}
                }]
            }]
        }
    ]);
    envelope["requiredFeatures"] = json!([
        "caret.env.bold",
        "caret.math.align",
        "caret.math.content-flow.v2",
        "caret.math.equation",
        "caret.math.inline",
        "caret.math.syntax.v1",
        "caret.paragraph.content-flow.v1",
        "caret.paragraph.layout-settings",
        "caret.text.default-font-size",
        "caret.text.underline"
    ]);

    let original_body = envelope["document"]["body"].clone();
    let document: crate::Document = serde_json::from_value(envelope["document"].clone())?;
    let [
        Block::Paragraph(paragraph),
        Block::Equation(equation),
        Block::Align(align),
    ] = document.body.as_slice()
    else {
        panic!("the three math carriers should decode as their typed document nodes");
    };
    let [ParagraphContent::Math(inline_math)] = paragraph.inlines.as_slice() else {
        panic!("InlineMath should decode as a paragraph math node");
    };
    let [MathContent::Environment(environment)] = inline_math.math_tree.root.content.as_slice()
    else {
        panic!("the inline math environment should decode as a math environment");
    };
    let [MathContent::Math(fraction)] = environment.children.as_slice() else {
        panic!("the inline environment child should decode as a math node");
    };
    let [MathContent::Text(text)] = fraction.children[0].content.as_slice() else {
        panic!("fraction operand text should remain a TextFlow inside MathTree");
    };
    assert_eq!(text.runs[0].props.get("underline"), Some(&json!(true)));
    assert_eq!(inline_math.id.as_str(), "inline-math-id");
    assert_eq!(inline_math.math_tree.root.id.as_str(), "inline-root-id");
    assert_eq!(environment.id.as_str(), "inline-env-id");
    assert_eq!(equation.id.as_str(), "equation-id");
    assert_eq!(equation.math_tree.root.id.as_str(), "equation-root-id");
    assert_eq!(align.id.as_str(), "align-id");
    assert_eq!(align.rows[0].id.as_str(), "align-row-id");
    assert_eq!(
        align.rows[0].cells[0].root.id.as_str(),
        "align-cell-root-id"
    );
    let [MathContent::Math(equation_node)] = equation.math_tree.root.content.as_slice() else {
        panic!("equation content should decode as a math node");
    };
    let [MathContent::Math(align_node)] = align.rows[0].cells[0].root.content.as_slice() else {
        panic!("Align cell content should decode as a math node");
    };
    assert_eq!(equation_node.id.as_str(), "equation-symbol-id");
    assert_eq!(align_node.id.as_str(), "align-symbol-id");
    let saved_document = serde_json::to_value(&document)?;
    assert_eq!(
        saved_document["body"][0]["vendorParagraph"],
        original_body[0]["vendorParagraph"]
    );
    assert_eq!(
        saved_document["body"][0]["inlines"][0]["vendorInlineMath"],
        original_body[0]["inlines"][0]["vendorInlineMath"]
    );
    assert_eq!(
        saved_document["body"][0]["inlines"][0]["mathTree"]["vendorTree"],
        original_body[0]["inlines"][0]["mathTree"]["vendorTree"]
    );
    assert_eq!(
        saved_document["body"][0]["inlines"][0]["mathTree"]["root"]["content"][0]["vendorEnvironment"],
        original_body[0]["inlines"][0]["mathTree"]["root"]["content"][0]["vendorEnvironment"]
    );
    assert_eq!(
        saved_document["body"][0]["inlines"][0]["mathTree"]["root"]["content"][0]["children"][0]["children"]
            [0]["content"][0]["vendorText"],
        original_body[0]["inlines"][0]["mathTree"]["root"]["content"][0]["children"][0]["children"]
            [0]["content"][0]["vendorText"]
    );
    assert_eq!(
        saved_document["body"][0]["inlines"][0]["mathTree"]["root"]["content"][0]["children"][0]["children"]
            [0]["content"][0]["runs"][0]["vendorRun"],
        original_body[0]["inlines"][0]["mathTree"]["root"]["content"][0]["children"][0]["children"]
            [0]["content"][0]["runs"][0]["vendorRun"]
    );
    assert_eq!(
        saved_document["body"][1]["vendorEquation"],
        original_body[1]["vendorEquation"]
    );
    assert_eq!(
        saved_document["body"][2]["vendorAlign"],
        original_body[2]["vendorAlign"]
    );
    assert_eq!(
        saved_document["body"][2]["rows"][0]["vendorRow"],
        original_body[2]["rows"][0]["vendorRow"]
    );
    for pointer in [
        "/0/id",
        "/0/inlines/0/id",
        "/0/inlines/0/mathTree/root/id",
        "/0/inlines/0/mathTree/root/content/0/id",
        "/0/inlines/0/mathTree/root/content/0/children/0/children/0/content/0/runs/0/props/underline",
        "/1/id",
        "/1/mathTree/root/id",
        "/1/mathTree/root/content/0/id",
        "/2/id",
        "/2/rows/0/id",
        "/2/rows/0/cells/0/root/id",
        "/2/rows/0/cells/0/root/content/0/id",
        "/1/mathTree/vendorEquationTree",
        "/1/mathTree/root/content/0/vendorEquationNode",
        "/2/rows/0/cells/0/vendorCellTree",
        "/2/rows/0/cells/0/root/content/0/vendorAlignNode",
    ] {
        let saved_pointer = format!("/body{pointer}");
        assert_eq!(
            saved_document.pointer(&saved_pointer),
            original_body.pointer(pointer),
            "unknown field changed at {saved_pointer}"
        );
    }
    envelope["document"] = saved_document.clone();
    let bytes = serde_json::to_vec(&envelope)?;
    let OpenedDocument::SafePreview(opened) = open(&bytes)? else {
        panic!("unsupported math features must open as a read-only preview");
    };
    let [
        Block::Paragraph(reopened_paragraph),
        Block::Equation(reopened_equation),
        Block::Align(reopened_align),
    ] = opened.document().body.as_slice()
    else {
        panic!("reopened math carriers should remain typed document nodes");
    };
    let [ParagraphContent::Math(reopened_inline_math)] = reopened_paragraph.inlines.as_slice()
    else {
        panic!("reopened InlineMath should remain typed");
    };
    let [MathContent::Environment(reopened_environment)] =
        reopened_inline_math.math_tree.root.content.as_slice()
    else {
        panic!("reopened inline environment should remain typed");
    };
    let [MathContent::Math(reopened_fraction)] = reopened_environment.children.as_slice() else {
        panic!("reopened environment child should remain a typed math node");
    };
    let [MathContent::Text(reopened_text)] = reopened_fraction.children[0].content.as_slice()
    else {
        panic!("reopened operand text should remain a typed TextFlow");
    };
    assert_eq!(
        reopened_text.runs[0].props.get("underline"),
        Some(&json!(true))
    );
    assert_eq!(reopened_inline_math.id.as_str(), "inline-math-id");
    assert_eq!(reopened_equation.id.as_str(), "equation-id");
    assert_eq!(reopened_align.id.as_str(), "align-id");
    let reopened = opened.to_json_value()?;
    assert_eq!(reopened["document"], saved_document);
    assert_eq!(reopened["document"]["body"][0]["id"], "paragraph-id");
    assert_eq!(
        reopened["document"]["body"][0]["inlines"][0]["id"],
        "inline-math-id"
    );
    assert_eq!(reopened["document"]["body"][1]["id"], "equation-id");
    assert_eq!(reopened["document"]["body"][2]["id"], "align-id");
    assert_eq!(
        reopened["document"]["body"][2]["rows"][0]["id"],
        "align-row-id"
    );
    assert_eq!(
        reopened["document"]["body"][0]["vendorParagraph"]["kept"],
        true
    );
    assert_eq!(
        reopened["document"]["body"][0]["inlines"][0]["vendorInlineMath"],
        "kept"
    );
    assert_eq!(reopened["document"]["body"][2]["rows"][0]["vendorRow"], 3);
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
fn retired_math_fields_inside_opaque_subtrees_remain_vendor_data()
-> Result<(), Box<dyn std::error::Error>> {
    let opaque_nodes = [
        json!({
            "type":"math", "id":"vendor", "kind":"future_kind",
            "sourceRange":{"vendor":"keep"},
            "children":[{"id":"operand","sourceRange":{"vendor":"keep"},"content":[
                {"type":"text","runs":[],"sourceMap":{"vendor":"keep"}}
            ]}]
        }),
        json!({
            "type":"math", "id":"unsupported-symbol", "kind":"symbol", "text":"x",
            "sourceRange":{"vendor":"keep"},
            "children":[{"id":"operand","sourceRange":{"vendor":"keep"},"content":[
                {"type":"text","runs":[],"sourceMap":{"vendor":"keep"}}
            ]}]
        }),
    ];
    for opaque_node in opaque_nodes {
        let mut envelope = blank_envelope()?;
        envelope["document"]["body"] = json!([{
            "type":"equation", "id":"eq", "mathTree":{
                "schemaVersion":3,
                "root":{"id":"root","content":[opaque_node]}
            }
        }]);
        envelope["requiredFeatures"] = json!([
            "caret.math.content-flow.v2",
            "caret.math.equation",
            "caret.paragraph.content-flow.v1",
            "caret.paragraph.layout-settings",
            "caret.text.default-font-size"
        ]);
        assert!(matches!(
            open(envelope.to_string().as_bytes())?,
            OpenedDocument::SafePreview(_)
        ));
    }

    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type":"equation", "id":"eq", "mathTree":{
            "schemaVersion":3,
            "root":{"id":"root","content":[
                {"type":"math", "id":"vendor", "kind":"future_kind",
                 "sourceRange":{"vendor":"keep"}},
                {"type":"math", "id":"known", "kind":"symbol", "text":"x",
                 "sourceRange":{"start":0,"end":1}}
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
    assert!(matches!(
        open(envelope.to_string().as_bytes()),
        Err(DocumentError::InvalidDocument(reason)) if reason.contains("MathNode.sourceRange")
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
fn opaque_current_math_still_requires_its_carrier_capabilities()
-> Result<(), Box<dyn std::error::Error>> {
    for missing in ["caret.math.inline", "caret.math.content-flow.v2"] {
        let mut envelope = blank_envelope()?;
        envelope["document"]["body"] = json!([{
            "type": "paragraph", "id": "paragraph", "inlines": [{
                "type": "inlineMath", "id": "inline", "mathTree": {
                    "schemaVersion": 3,
                    "root": {"id": "root", "content": [{
                        "type": "math", "id": "symbol", "kind": "symbol", "text": "x",
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
        let features = envelope["requiredFeatures"]
            .as_array_mut()
            .ok_or("requiredFeatures should be an array")?;
        features.retain(|feature| feature.as_str() != Some(missing));

        let result = open(envelope.to_string().as_bytes());
        let Err(DocumentError::InvalidDocument(reason)) = result else {
            panic!("opaque current math must reject missing {missing}; got {result:?}");
        };
        assert!(reason.contains(missing), "unexpected error: {reason}");
    }
    Ok(())
}

#[test]
fn math_feature_scan_ignores_unknown_payloads_outside_semantic_content()
-> Result<(), Box<dyn std::error::Error>> {
    let payload = json!({
        "type": "equation",
        "mathTree": {"schemaVersion": 3}
    });
    let mut envelope = blank_envelope()?;
    envelope["document"]["vendorPayload"] = payload.clone();
    let OpenedDocument::Editable(file) = open(envelope.to_string().as_bytes())? else {
        panic!("unknown optional document fields should not change editability");
    };
    assert_eq!(file.document.extra["vendorPayload"], payload);

    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type": "vendorBlock",
        "payload": payload
    }]);
    let OpenedDocument::SafePreview(preview) = open(envelope.to_string().as_bytes())? else {
        panic!("unknown blocks should remain a safe preview");
    };
    assert!(!preview.reasons.iter().any(|reason| matches!(
        reason,
        PreviewReason::MissingRequiredFeature(feature) if feature.starts_with("caret.math.")
    )));

    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type": "paragraph", "id": "paragraph", "inlines": [{
            "type": "environment", "scope": "inline", "kind": "vendor",
            "children": [payload]
        }]
    }]);
    let OpenedDocument::SafePreview(preview) = open(envelope.to_string().as_bytes())? else {
        panic!("unknown inline environments should remain a safe preview");
    };
    assert!(!preview.reasons.iter().any(|reason| matches!(
        reason,
        PreviewReason::MissingRequiredFeature(feature) if feature.starts_with("caret.math.")
    )));
    Ok(())
}

#[test]
fn empty_and_plain_symbol_math_need_no_syntax_capability() -> Result<(), Box<dyn std::error::Error>>
{
    for content in [
        json!([]),
        json!([{"type": "math", "id": "symbol", "kind": "symbol", "text": "x"}]),
    ] {
        let mut envelope = blank_envelope()?;
        envelope["document"]["body"] = json!([{
            "type": "paragraph", "id": "paragraph", "inlines": [{
                "type": "inlineMath", "id": "inline", "mathTree": {
                    "schemaVersion": 3,
                    "root": {"id": "root", "content": content}
                }
            }]
        }]);
        envelope["requiredFeatures"] = json!([
            "caret.math.content-flow.v2",
            "caret.math.inline",
            "caret.paragraph.content-flow.v1",
            "caret.paragraph.layout-settings",
            "caret.text.default-font-size"
        ]);
        let result = open(envelope.to_string().as_bytes())?;
        if let OpenedDocument::SafePreview(preview) = result {
            assert!(!preview.reasons.iter().any(|reason| matches!(
                reason,
                PreviewReason::MissingRequiredFeature(feature)
                    if feature == "caret.math.syntax.v1"
            )));
        }
    }
    Ok(())
}

#[test]
fn opaque_math_still_requires_syntax_for_known_siblings() -> Result<(), Box<dyn std::error::Error>>
{
    let tree = json!({
        "schemaVersion": 3,
        "root": {"id": "root", "content": [{
            "type": "math", "id": "fraction", "kind": "fraction",
            "attributes": {"style": "frac"},
            "children": [
                {"id": "numerator", "content": []},
                {"id": "denominator", "content": []}
            ]
        }, {"type": "futureContent", "payload": {"kind": "future"}}]}
    });
    let math_features = [
        "caret.math.content-flow.v2",
        "caret.math.syntax.v1",
        "caret.paragraph.content-flow.v1",
        "caret.paragraph.layout-settings",
        "caret.text.default-font-size",
    ];
    let carriers = [
        (
            json!({
                "type": "paragraph", "id": "paragraph", "inlines": [{
                    "type": "inlineMath", "id": "inline", "mathTree": tree
                }]
            }),
            "caret.math.inline",
        ),
        (
            json!({"type": "equation", "id": "equation", "mathTree": tree}),
            "caret.math.equation",
        ),
        (
            json!({"type": "align", "id": "align", "rows": [{
                "id": "row", "cells": [tree]
            }]}),
            "caret.math.align",
        ),
    ];
    for (block, carrier_feature) in carriers {
        let mut envelope = blank_envelope()?;
        envelope["document"]["body"] = json!([block]);
        envelope["requiredFeatures"] = json!(
            math_features
                .iter()
                .copied()
                .chain([carrier_feature])
                .collect::<Vec<_>>()
        );
        let features = envelope["requiredFeatures"]
            .as_array_mut()
            .ok_or("requiredFeatures should be an array")?;
        features.retain(|feature| feature.as_str() != Some("caret.math.syntax.v1"));
        features.sort_by_key(|feature| feature.as_str().unwrap_or_default().to_owned());
        let result = open(envelope.to_string().as_bytes());
        let Err(DocumentError::InvalidDocument(reason)) = result else {
            panic!("known syntax in opaque math must require math.syntax.v1: {result:?}");
        };
        assert!(
            reason.contains("caret.math.syntax.v1"),
            "unexpected error: {reason}"
        );
    }
    Ok(())
}

#[test]
fn raw_syntax_scan_does_not_enter_children_of_opaque_known_nodes()
-> Result<(), Box<dyn std::error::Error>> {
    let mut envelope = blank_envelope()?;
    envelope["document"]["body"] = json!([{
        "type": "paragraph", "id": "paragraph", "inlines": [{
            "type": "inlineMath", "id": "inline", "mathTree": {
                "schemaVersion": 3,
                "root": {"id": "root", "content": [{
                    "type": "math", "id": "symbol", "kind": "symbol", "text": "x",
                    "children": [{"id": "future-flow", "content": [{
                        "type": "math", "id": "fraction", "kind": "fraction",
                        "attributes": {"style": "frac"},
                        "children": [
                            {"id": "num", "content": []},
                            {"id": "den", "content": []}
                        ]
                    }]}]
                }]}
            }
        }]
    }]);
    envelope["requiredFeatures"] = json!([
        "caret.math.content-flow.v2",
        "caret.math.inline",
        "caret.paragraph.content-flow.v1",
        "caret.paragraph.layout-settings",
        "caret.text.default-font-size"
    ]);
    let result = open(envelope.to_string().as_bytes())?;
    let OpenedDocument::SafePreview(preview) = result else {
        panic!("unsupported Symbol shape should remain a safe preview: {result:?}");
    };
    assert!(!preview.reasons.iter().any(|reason| matches!(
        reason,
        PreviewReason::MissingRequiredFeature(feature)
            if feature == "caret.math.syntax.v1"
    )));
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
fn recursive_opaque_math_scan_covers_equation_and_align() -> Result<(), Box<dyn std::error::Error>>
{
    let opaque_flow = || MathFlow {
        id: NodeId::generate(),
        content: vec![MathContent::Opaque(OpaqueNode {
            raw: json!({"type": "future_content", "vendor": "preserve"}),
        })],
        extra: Default::default(),
    };
    let tree = || MathTree {
        schema_version: crate::MATH_TREE_SCHEMA_VERSION,
        root: opaque_flow(),
        extra: Default::default(),
    };
    let documents = [
        {
            let mut document = Document::blank();
            document.body.push(Block::Equation(Equation {
                id: NodeId::generate(),
                math_tree: tree(),
                extra: Default::default(),
            }));
            document
        },
        {
            let mut document = Document::blank();
            document.body.push(Block::Align(Align {
                id: NodeId::generate(),
                rows: vec![MathAlignRow {
                    id: NodeId::generate(),
                    cells: vec![tree()],
                    extra: Default::default(),
                }],
                extra: Default::default(),
            }));
            document
        },
    ];

    for document in documents {
        assert!(super::contains_opaque_block(&document.body));
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
            "caret.math.content-flow.v2",
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
