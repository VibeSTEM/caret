//! 验证文档模型、结构约束和默认值。

use super::{
    Align, Block, Document, DocumentFontTarget, DocumentSettings, Equation, InlineEnvironment,
    InlineEnvironmentKind, MATH_TREE_SCHEMA_VERSION, MAX_MATH_SOURCE_BYTES, MAX_MATH_TREE_DEPTH,
    MAX_MATH_TREE_PAYLOAD_BYTES, MathAlignRow, MathContent, MathFlow, MathNode, MathNodeKind,
    MathSourceRange, MathTree, NodeId, Run, TextFlow, is_valid_language_tag,
};
use std::collections::BTreeMap;

#[test]
fn document_validation_rejects_empty_font_names_and_malformed_script_keys() {
    let mut document = Document::blank();
    document.global_settings = DocumentSettings {
        default_font_family: Some("  ".to_owned()),
        ..DocumentSettings::default()
    };
    assert!(document.validate().is_err());

    document.global_settings = DocumentSettings {
        script_font_families: BTreeMap::from([("Cyrillic".to_owned(), "Font".to_owned())]),
        ..DocumentSettings::default()
    };
    assert!(document.validate().is_err());

    document.global_settings = DocumentSettings {
        script_font_families: BTreeMap::from([
            ("Cyrl".to_owned(), "Font A".to_owned()),
            ("cyrl".to_owned(), "Font B".to_owned()),
        ]),
        ..DocumentSettings::default()
    };
    assert!(document.validate().is_err());
}

#[test]
fn document_validation_retains_well_formed_unknown_script_keys() {
    let mut document = Document::blank();
    document.global_settings = DocumentSettings {
        script_font_families: BTreeMap::from([("Qaaa".to_owned(), "Future Font".to_owned())]),
        ..DocumentSettings::default()
    };

    assert!(document.validate().is_ok());
}

#[test]
fn font_targets_match_case_insensitively_and_updates_canonicalize_keys() {
    let mut settings = DocumentSettings {
        script_font_families: BTreeMap::from([("cyrl".to_owned(), "Old Font".to_owned())]),
        language_script_font_families: BTreeMap::from([(
            "cYRL".to_owned(),
            BTreeMap::from([("SR-latn-rs".to_owned(), "Serbian Font".to_owned())]),
        )]),
        ..DocumentSettings::default()
    };

    assert_eq!(
        settings.font_family(&DocumentFontTarget::Script("Cyrl".to_owned())),
        Some("Old Font")
    );
    assert_eq!(
        settings.font_family(&DocumentFontTarget::ScriptLanguage {
            script: "CYRL".to_owned(),
            language: "sr-Latn-RS".to_owned(),
        }),
        Some("Serbian Font")
    );

    assert!(
        settings
            .set_font_family(
                &DocumentFontTarget::Script("CYRL".to_owned()),
                Some("New Font".to_owned()),
            )
            .is_ok()
    );
    assert!(
        settings
            .set_font_family(
                &DocumentFontTarget::ScriptLanguage {
                    script: "cyrl".to_owned(),
                    language: "SR-latn-RS".to_owned(),
                },
                Some("Updated Serbian Font".to_owned()),
            )
            .is_ok()
    );

    assert_eq!(settings.script_font_families.len(), 1);
    assert!(settings.script_font_families.contains_key("Cyrl"));
    assert_eq!(settings.language_script_font_families.len(), 1);
    assert!(settings.language_script_font_families.contains_key("Cyrl"));
    assert_eq!(
        settings.language_script_font_families["Cyrl"]
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["sr-latn-rs"]
    );
    assert!(
        Document {
            global_settings: settings,
            ..Document::blank()
        }
        .validate()
        .is_ok()
    );
}

#[test]
fn font_target_language_validation_matches_document_language_validation() {
    assert!(is_valid_language_tag("en-US"));
    assert!(is_valid_language_tag("x-private"));
    assert!(is_valid_language_tag("i-klingon"));
    assert!(!is_valid_language_tag("en-a"));

    let mut settings = DocumentSettings::default();
    let before = settings.clone();
    let result = settings.set_font_family(
        &DocumentFontTarget::ScriptLanguage {
            script: "Latn".to_owned(),
            language: "en-a".to_owned(),
        },
        Some("Example Font".to_owned()),
    );
    assert!(result.is_err());
    assert_eq!(settings, before);
}

#[test]
fn legacy_latin_script_mapping_remains_the_effective_latin_target() {
    let mut settings = DocumentSettings {
        default_font_family: Some("Document Default".to_owned()),
        script_font_families: BTreeMap::from([("Latn".to_owned(), "Legacy Latin".to_owned())]),
        ..DocumentSettings::default()
    };
    let target = DocumentFontTarget::Script("Latn".to_owned());

    assert_eq!(settings.font_family(&target), Some("Legacy Latin"));
    assert!(
        settings
            .set_font_family(&target, Some("Updated Latin".to_owned()))
            .is_ok()
    );
    assert_eq!(settings.font_family(&target), Some("Updated Latin"));
    assert!(!settings.script_font_families.contains_key("Latn"));
    assert_eq!(
        settings.default_font_family.as_deref(),
        Some("Updated Latin")
    );
    assert!(settings.set_font_family(&target, None).is_ok());
    assert_eq!(settings.font_family(&target), None);
}

#[test]
fn missing_global_text_and_paragraph_settings_use_bundled_defaults()
-> Result<(), Box<dyn std::error::Error>> {
    let settings: DocumentSettings = serde_json::from_str("{}")?;
    let default = super::ParagraphLayoutStyle::default();

    assert_eq!(settings.paragraph_layout, default);
    assert_eq!(settings.default_font_size_centipoints, 1200);
    assert_eq!(settings.paragraph_layout.space_before_centipoints, 0);
    assert_eq!(settings.paragraph_layout.space_after_centipoints, 800);
    assert!(settings.paragraph_layout.widow_orphan_control);
    assert_eq!(
        settings.default_code_font_family,
        super::DEFAULT_CODE_FONT_FAMILY
    );
    Ok(())
}

#[test]
fn code_font_family_default_marker_is_valid_and_empty_family_is_rejected() {
    let mut document = Document::blank();
    assert_eq!(
        document.global_settings.default_code_font_family,
        super::DEFAULT_CODE_FONT_FAMILY
    );
    assert!(document.validate().is_ok());

    document.global_settings.default_code_font_family = "Noto Sans Mono".to_owned();
    assert!(document.validate().is_ok());
    assert!(
        document
            .required_features()
            .iter()
            .any(|feature| feature == "caret.font.family-preferences")
    );

    document.global_settings.default_code_font_family = "  ".to_owned();
    assert!(matches!(
        document.validate(),
        Err(super::ModelError::InvalidDocument(reason))
            if reason.contains("defaultCodeFontFamily")
    ));
}

#[test]
fn documents_declare_paragraph_layout_semantics() {
    let document = Document::blank();
    assert!(
        document
            .required_features()
            .iter()
            .any(|feature| feature == "caret.paragraph.layout-settings")
    );
    assert!(
        document
            .required_features()
            .iter()
            .any(|feature| feature == "caret.text.default-font-size")
    );
}

#[test]
fn math_text_source_map_declares_its_required_feature() -> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    document.body = serde_json::from_value(serde_json::json!([{
        "type": "paragraph", "id": "paragraph", "inlines": [{
            "type": "inlineMath", "id": "inline", "mathTree": {
                "schemaVersion": MATH_TREE_SCHEMA_VERSION,
                "source": r"\text{x}",
                "root": {"id": "root", "content": [{
                    "type": "text",
                    "runs": [{"text": "x"}],
                    "sourceRange": {"start": 6, "end": 7},
                    "sourceMap": [{
                        "textStart": 0,
                        "textEnd": 1,
                        "sourceRange": {"start": 6, "end": 7}
                    }]
                }]}
            }
        }]
    }]))?;
    document.validate()?;

    assert!(
        document
            .required_features()
            .iter()
            .any(|feature| feature == "caret.math.text-flow-source-map.v1")
    );
    Ok(())
}

#[test]
fn custom_default_font_size_declares_its_required_capability() {
    let mut document = Document::blank();
    document.global_settings.default_font_size_centipoints = 1450;
    assert!(
        document
            .required_features()
            .iter()
            .any(|feature| feature == "caret.text.default-font-size")
    );
}

#[test]
fn invalid_paragraph_line_spacing_is_rejected() {
    let mut document = Document::blank();
    document.global_settings.paragraph_layout.line_spacing =
        super::ParagraphLineSpacing::Multiple {
            percent: 0,
            extra: BTreeMap::new(),
        };
    assert!(document.validate().is_err());

    let mut document = Document::blank();
    if let Some(super::Block::Paragraph(paragraph)) = document.body.first_mut() {
        paragraph.layout = Some(super::ParagraphLayoutOverrides {
            line_spacing: Some(super::ParagraphLineSpacing::Exactly {
                centipoints: 0,
                extra: BTreeMap::new(),
            }),
            ..super::ParagraphLayoutOverrides::default()
        });
    }
    assert!(document.validate().is_err());
}

#[test]
fn paragraph_layout_keeps_unknown_optional_fields_on_round_trip()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    document.global_settings.paragraph_layout.line_spacing =
        super::ParagraphLineSpacing::Multiple {
            percent: 115,
            extra: BTreeMap::from([(
                "futureLineSpacingValue".to_owned(),
                serde_json::json!("kept"),
            )]),
        };
    document.global_settings.paragraph_layout.extra.insert(
        "futureProfileValue".to_owned(),
        serde_json::json!({"enabled": true}),
    );
    if let Some(Block::Paragraph(paragraph)) = document.body.first_mut() {
        let mut overrides = super::ParagraphLayoutOverrides::default();
        overrides
            .extra
            .insert("futureParagraphValue".to_owned(), serde_json::json!(42));
        paragraph.layout = Some(overrides);
    }

    let encoded = serde_json::to_value(&document)?;
    let decoded: Document = serde_json::from_value(encoded)?;

    assert_eq!(
        decoded.global_settings.paragraph_layout.extra["futureProfileValue"],
        serde_json::json!({"enabled": true})
    );
    let super::ParagraphLineSpacing::Multiple { extra, .. } =
        &decoded.global_settings.paragraph_layout.line_spacing
    else {
        panic!("line spacing kind changed unexpectedly");
    };
    assert_eq!(extra["futureLineSpacingValue"], serde_json::json!("kept"));
    let Some(Block::Paragraph(paragraph)) = decoded.body.first() else {
        panic!("blank document paragraph missing");
    };
    assert_eq!(
        paragraph
            .layout
            .as_ref()
            .map(|layout| &layout.extra["futureParagraphValue"]),
        Some(&serde_json::json!(42))
    );
    Ok(())
}

#[test]
fn line_spacing_fields_serialize_as_camel_case() -> Result<(), Box<dyn std::error::Error>> {
    let encoded = serde_json::to_value(super::ParagraphLineSpacing::AtLeast {
        min_centipoints: 1200,
        extra: BTreeMap::new(),
    })?;

    assert_eq!(encoded["kind"], "atLeast");
    assert_eq!(encoded["minCentipoints"], 1200);
    Ok(())
}

#[test]
fn known_environment_fields_with_wrong_types_are_rejected_instead_of_becoming_opaque() {
    for scope in ["block", "inline"] {
        let kind = if scope == "block" { "section" } else { "bold" };
        let valid = serde_json::json!({
            "type": "environment", "scope": scope, "kind": kind, "id": "environment"
        });
        for (field, value) in [
            ("id", serde_json::json!(42)),
            ("definitionId", serde_json::json!([])),
            ("props", serde_json::json!([])),
            ("children", serde_json::json!({})),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            if scope == "block" {
                assert!(serde_json::from_value::<Block>(invalid).is_err());
            } else {
                assert!(serde_json::from_value::<super::ParagraphContent>(invalid).is_err());
            }
        }
    }
}

#[test]
fn unknown_inline_environment_kinds_remain_opaque_and_round_trip()
-> Result<(), Box<dyn std::error::Error>> {
    let raw = serde_json::json!({
        "type": "environment", "scope": "inline", "kind": "future_style",
        "id": "future", "children": [{"type": "text", "runs": [{"text": "kept"}]}]
    });
    let node: super::ParagraphContent = serde_json::from_value(raw.clone())?;
    assert_eq!(serde_json::to_value(node)?, raw);
    Ok(())
}

#[test]
fn recognized_nodes_reject_non_object_or_malformed_content() {
    for invalid in [serde_json::json!(null), serde_json::json!([])] {
        assert!(serde_json::from_value::<Block>(invalid.clone()).is_err());
        assert!(serde_json::from_value::<super::ParagraphContent>(invalid).is_err());
    }
    assert!(
        serde_json::from_value::<Block>(serde_json::json!({
            "type": "paragraph", "id": "paragraph", "inlines": false
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<Block>(serde_json::json!({
            "type": "explicitBreak", "kind": 42
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<super::ParagraphContent>(serde_json::json!({
            "type": "text", "runs": [{"text": 42}]
        }))
        .is_err()
    );
}

#[test]
fn node_ids_preserve_valid_values_and_reject_invalid_input()
-> Result<(), Box<dyn std::error::Error>> {
    let id = super::NodeId::parse("节点-123".to_owned())?;
    assert_eq!(id.as_str(), "节点-123");
    assert_eq!(id.to_string(), "节点-123");
    assert!(super::NodeId::parse("x".repeat(128)).is_ok());
    for invalid in [String::new(), "x".repeat(129), "bad\nvalue".to_owned()] {
        let error = super::NodeId::parse(invalid.clone());
        assert_eq!(error, Err(super::ModelError::InvalidNodeId(invalid)));
    }
    Ok(())
}

#[test]
fn document_language_and_font_overrides_validate_each_level() {
    let mut document = Document::blank();
    for tag in [
        "en-US",
        "zh-Hant-TW",
        "i-klingon",
        "x-private",
        "en-u-ca-gregory",
    ] {
        document.global_settings.default_language = Some(tag.to_owned());
        assert!(document.validate().is_ok(), "valid language {tag}");
    }
    for tag in ["a", "en--US", "en-u", "x", "sl-rozaj-ROZAJ", "en-u-ca-u-nu"] {
        document.global_settings.default_language = Some(tag.to_owned());
        assert_eq!(
            document.validate(),
            Err(super::ModelError::InvalidLanguageTag(tag.to_owned()))
        );
    }
    document.global_settings.default_language = None;
    document.global_settings.language_script_font_families = BTreeMap::from([(
        "Cyrl".to_owned(),
        BTreeMap::from([
            ("sr".to_owned(), "Serbian Font".to_owned()),
            ("SR".to_owned(), "Other Font".to_owned()),
        ]),
    )]);
    assert!(
        matches!(document.validate(), Err(super::ModelError::InvalidDocument(reason)) if reason.contains("duplicate BCP 47"))
    );

    document
        .global_settings
        .language_script_font_families
        .clear();
    document.global_settings.default_cjk_font_family = Some(" ".to_owned());
    assert!(
        matches!(document.validate(), Err(super::ModelError::InvalidDocument(reason)) if reason.contains("defaultCjkFontFamily"))
    );
    document.global_settings.default_cjk_font_family = None;
    document.global_settings.default_math_font_family = Some("".to_owned());
    assert!(
        matches!(document.validate(), Err(super::ModelError::InvalidDocument(reason)) if reason.contains("defaultMathFontFamily"))
    );
    document.global_settings.default_math_font_family = None;
    document.global_settings.default_font_size_centipoints = 0;
    assert!(
        matches!(document.validate(), Err(super::ModelError::InvalidDocument(reason)) if reason.contains("font size"))
    );
}

#[test]
fn nested_environment_features_cover_format_language_and_template_dependencies()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    document.global_settings.default_cjk_font_family = Some("CJK Font".to_owned());
    document.body = vec![serde_json::from_value(serde_json::json!({
        "type": "environment", "scope": "block", "kind": "section", "id": "section",
        "definitionId": "chapter",
        "title": {"id": "title", "inlines": [{
            "type": "environment", "scope": "inline", "kind": "bold", "id": "bold",
            "props": {"enabled": false}, "children": [{
                "type": "environment", "scope": "inline", "kind": "font", "id": "font",
                "props": {"fontFamily": "CJK Font"},
                "children": [{"type": "text", "runs": [{"text": "标题", "language": "zh-Hans"}]}]
            }]
        }]},
        "children": [{
            "type": "environment", "scope": "block", "kind": "custom", "id": "custom",
            "props": {"displayName": "Example"},
            "children": [{"type": "paragraph", "id": "paragraph", "inlines": [{
                "type": "environment", "scope": "inline", "kind": "italic", "id": "italic",
                "children": [{"type": "text", "runs": [{"text": "body"}]}]
            }]}]
        }]
    }))?];
    document.validate()?;
    let features = document.required_features();
    assert_eq!(
        features.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "caret.env.bold",
            "caret.env.custom",
            "caret.env.explicit-format-state",
            "caret.env.font",
            "caret.env.italic",
            "caret.env.section",
            "caret.env.templates",
            "caret.font.family-preferences",
            "caret.paragraph.content-flow.v1",
            "caret.paragraph.layout-settings",
            "caret.text.default-font-size",
            "caret.text.language-tags",
        ]
    );
    Ok(())
}

#[test]
fn math_validation_rejects_non_boundary_ranges_and_duplicate_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    document.body = serde_json::from_value(serde_json::json!([{
        "type": "paragraph", "id": "paragraph", "inlines": [{
                "type": "inlineMath", "id": "inline", "mathTree": {
                    "schemaVersion": 2, "source": "甲x",
                    "root": {"id": "root", "content": [{
                        "type": "math", "id": "symbol", "kind": "symbol", "text": "甲",
                        "sourceRange": {"start": 1, "end": 2}
                    }]}
                }
        }]
    }]))?;
    assert!(document.validate().is_err());

    document.body = serde_json::from_value(serde_json::json!([
        {"type": "equation", "id": "equation", "mathTree": {
            "schemaVersion": 2, "source": "x",
            "root": {"id": "same", "content": [
                {"type": "math", "id": "same", "kind": "symbol", "text": "x"}
            ]}
        }}
    ]))?;
    assert!(document.validate().is_err());
    Ok(())
}

#[test]
fn align_validation_rejects_ids_shared_across_rows_and_cells() {
    fn as_flow(node: MathNode) -> MathFlow {
        MathFlow {
            id: NodeId::generate(),
            content: vec![MathContent::Math(node)],
            source_range: None,
            extra: BTreeMap::new(),
        }
    }
    fn tree(root_id: NodeId) -> MathTree {
        MathTree {
            schema_version: MATH_TREE_SCHEMA_VERSION,
            source: "x".into(),
            root: as_flow(MathNode {
                id: root_id,
                kind: MathNodeKind::Symbol,
                text: Some("x".into()),
                attributes: BTreeMap::from([("atomClass".into(), "ord".into())]),
                source_range: None,
                children: Vec::new(),
                extra: BTreeMap::new(),
            }),
            extra: BTreeMap::new(),
        }
    }

    let shared_id = NodeId::generate();
    let mut align = Align {
        id: NodeId::generate(),
        source: "x&x".into(),
        rows: vec![MathAlignRow {
            id: NodeId::generate(),
            cells: vec![tree(shared_id.clone()), tree(shared_id)],
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    };
    assert!(align.validate().is_err());

    let MathContent::Math(root) = &mut align.rows[0].cells[1].root.content[0] else {
        panic!("cell root should contain a math node");
    };
    root.id = NodeId::generate();
    assert!(align.validate().is_ok());
}

#[test]
fn math_validation_enforces_source_and_tree_limits() {
    fn as_flow(node: MathNode) -> MathFlow {
        MathFlow {
            id: NodeId::generate(),
            content: vec![MathContent::Math(node)],
            source_range: None,
            extra: BTreeMap::new(),
        }
    }
    fn tree_with_depth(depth: usize) -> MathTree {
        let mut root = MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Group,
            text: None,
            attributes: BTreeMap::new(),
            source_range: None,
            children: Vec::new(),
            extra: BTreeMap::new(),
        };
        for _ in 1..depth {
            root = MathNode {
                id: NodeId::generate(),
                kind: MathNodeKind::Group,
                text: None,
                attributes: BTreeMap::new(),
                source_range: None,
                children: vec![as_flow(root)],
                extra: BTreeMap::new(),
            };
        }
        MathTree {
            schema_version: MATH_TREE_SCHEMA_VERSION,
            source: String::new(),
            root: as_flow(root),
            extra: BTreeMap::new(),
        }
    }

    let mut document = Document::blank();
    document.body = vec![Block::Equation(Equation {
        id: NodeId::generate(),
        math_tree: tree_with_depth(MAX_MATH_TREE_DEPTH),
        extra: BTreeMap::new(),
    })];
    assert!(document.validate().is_ok());

    let mut too_deep = Document::blank();
    too_deep.body = vec![Block::Equation(Equation {
        id: NodeId::generate(),
        math_tree: tree_with_depth(MAX_MATH_TREE_DEPTH + 1),
        extra: BTreeMap::new(),
    })];
    assert!(too_deep.validate().is_err());

    let mut too_long = Document::blank();
    too_long.body = vec![Block::Align(Align {
        id: NodeId::generate(),
        source: "x".repeat(MAX_MATH_SOURCE_BYTES + 1),
        rows: vec![MathAlignRow {
            id: NodeId::generate(),
            cells: Vec::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    })];
    assert!(too_long.validate().is_err());

    let mut oversized_columns = Document::blank();
    let tree = MathTree {
        schema_version: MATH_TREE_SCHEMA_VERSION,
        source: String::new(),
        root: as_flow(MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Matrix,
            text: None,
            attributes: BTreeMap::from([
                ("style".to_owned(), "aligned".to_owned()),
                (
                    "columns".to_owned(),
                    "l".repeat(MAX_MATH_TREE_PAYLOAD_BYTES + 1),
                ),
            ]),
            source_range: None,
            children: vec![as_flow(MathNode {
                id: NodeId::generate(),
                kind: MathNodeKind::Row,
                text: None,
                attributes: BTreeMap::new(),
                source_range: None,
                children: Vec::new(),
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        }),
        extra: BTreeMap::new(),
    };
    oversized_columns.body = vec![Block::Equation(Equation {
        id: NodeId::generate(),
        math_tree: tree,
        extra: BTreeMap::new(),
    })];
    assert!(oversized_columns.validate().is_err());

    let oversized_range_extra = MathSourceRange {
        start: 0,
        end: 1,
        extra: BTreeMap::from([(
            "future".to_owned(),
            serde_json::Value::String("x".repeat(MAX_MATH_TREE_PAYLOAD_BYTES + 1)),
        )]),
    };
    let mut oversized_range = Document::blank();
    oversized_range.body = vec![Block::Equation(Equation {
        id: NodeId::generate(),
        math_tree: MathTree {
            schema_version: MATH_TREE_SCHEMA_VERSION,
            source: "x".to_owned(),
            root: as_flow(MathNode {
                id: NodeId::generate(),
                kind: MathNodeKind::Symbol,
                text: Some("x".to_owned()),
                attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
                source_range: Some(oversized_range_extra),
                children: Vec::new(),
                extra: BTreeMap::new(),
            }),
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    })];
    assert!(oversized_range.validate().is_err());
}

#[test]
fn math_inline_environments_obey_math_depth_limit() {
    let mut content = MathContent::Text(TextFlow {
        runs: vec![Run {
            text: "x".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        source_range: None,
        source_map: Vec::new(),
        extra: BTreeMap::new(),
    });
    for _ in 0..=MAX_MATH_TREE_DEPTH {
        content = MathContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            source_range: None,
            props: BTreeMap::new(),
            children: vec![content],
            extra: BTreeMap::new(),
        });
    }
    let tree = MathTree {
        schema_version: MATH_TREE_SCHEMA_VERSION,
        source: "x".to_owned(),
        root: MathFlow {
            id: NodeId::generate(),
            content: vec![content],
            source_range: None,
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    let mut document = Document::blank();
    document.body = vec![Block::Equation(Equation {
        id: NodeId::generate(),
        math_tree: tree,
        extra: BTreeMap::new(),
    })];
    assert_eq!(document.validate(), Err(super::ModelError::NestingLimit));
}

#[test]
fn math_text_runs_count_toward_the_tree_node_limit() {
    let tree = MathTree {
        schema_version: MATH_TREE_SCHEMA_VERSION,
        source: String::new(),
        root: MathFlow {
            id: NodeId::generate(),
            content: vec![MathContent::Text(TextFlow {
                runs: vec![
                    Run {
                        text: String::new(),
                        language: None,
                        props: BTreeMap::new(),
                        extra: BTreeMap::new(),
                    };
                    super::MAX_MATH_TREE_NODES
                ],
                source_range: None,
                source_map: Vec::new(),
                extra: BTreeMap::new(),
            })],
            source_range: None,
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    assert!(tree.validate().is_err());
}

#[test]
fn custom_environment_names_are_optional_and_validated_by_unicode_character_count() {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = document.body.remove(0) else {
        unreachable!();
    };
    document
        .body
        .push(Block::Environment(super::BlockEnvironment {
            id: super::NodeId::generate(),
            kind: super::BlockEnvironmentKind::Custom,
            definition_id: None,
            props: BTreeMap::from([(
                "displayName".to_owned(),
                serde_json::Value::String("自定义环境".to_owned()),
            )]),
            title: None,
            children: vec![Block::Paragraph(paragraph)],
            extra: BTreeMap::new(),
        }));
    assert!(document.validate().is_ok());

    let Block::Environment(environment) = &mut document.body[0] else {
        unreachable!();
    };
    environment.props.insert(
        "displayName".to_owned(),
        serde_json::Value::String("字".repeat(128)),
    );
    assert!(document.validate().is_ok());
    let Block::Environment(environment) = &mut document.body[0] else {
        unreachable!();
    };
    environment.props.insert(
        "displayName".to_owned(),
        serde_json::Value::String("字".repeat(129)),
    );
    assert!(document.validate().is_err());
}

#[test]
fn custom_environment_display_name_rejects_empty_and_non_text_values() {
    for value in [serde_json::json!("  "), serde_json::json!(42)] {
        let mut document = Document::blank();
        let Block::Paragraph(paragraph) = document.body.remove(0) else {
            unreachable!();
        };
        document
            .body
            .push(Block::Environment(super::BlockEnvironment {
                id: super::NodeId::generate(),
                kind: super::BlockEnvironmentKind::Custom,
                definition_id: None,
                props: BTreeMap::from([("displayName".to_owned(), value)]),
                title: None,
                children: vec![Block::Paragraph(paragraph)],
                extra: BTreeMap::new(),
            }));
        assert!(document.validate().is_err());
    }
}

#[test]
fn inline_format_values_and_nesting_are_validated() -> Result<(), Box<dyn std::error::Error>> {
    for (kind, props) in [
        ("bold", serde_json::json!({"enabled": "yes"})),
        ("italic", serde_json::json!({"enabled": 1})),
        ("font", serde_json::json!({"fontFamily": 42})),
    ] {
        let mut document = Document::blank();
        let Some(Block::Paragraph(paragraph)) = document.body.first_mut() else {
            return Err("blank paragraph is missing".into());
        };
        paragraph
            .inlines
            .push(serde_json::from_value(serde_json::json!({
                "type": "environment", "scope": "inline", "kind": kind,
                "id": "inline", "props": props, "children": []
            }))?);
        assert!(matches!(
            document.validate(),
            Err(super::ModelError::InvalidDocument(_))
        ));
    }

    let mut inline = super::ParagraphContent::Text(super::TextFlow {
        runs: vec![super::Run {
            text: "x".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        source_range: None,
        source_map: Vec::new(),
        extra: BTreeMap::new(),
    });
    for _ in 0..65 {
        inline = super::ParagraphContent::Environment(super::InlineEnvironment {
            id: super::NodeId::generate(),
            kind: super::InlineEnvironmentKind::Bold,
            definition_id: None,
            source_range: None,
            props: BTreeMap::new(),
            children: vec![inline],
            extra: BTreeMap::new(),
        });
    }
    let mut document = Document::blank();
    let Some(Block::Paragraph(paragraph)) = document.body.first_mut() else {
        return Err("blank paragraph is missing".into());
    };
    paragraph.inlines.push(inline);
    assert_eq!(document.validate(), Err(super::ModelError::NestingLimit));
    Ok(())
}

#[test]
fn paragraph_text_flow_rejects_math_source_ranges() {
    let mut document = Document::blank();
    let Some(Block::Paragraph(paragraph)) = document.body.first_mut() else {
        panic!("blank document should contain a paragraph");
    };
    paragraph
        .inlines
        .push(super::ParagraphContent::Text(TextFlow {
            runs: vec![Run {
                text: "text".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            source_range: Some(MathSourceRange {
                start: 0,
                end: 4,
                extra: BTreeMap::new(),
            }),
            source_map: Vec::new(),
            extra: BTreeMap::new(),
        }));

    assert!(matches!(
        document.validate(),
        Err(super::ModelError::InvalidDocument(message))
            if message == "TextFlow source mapping is only valid inside a math tree"
    ));
}
