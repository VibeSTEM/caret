//! 验证文档模型、结构约束和默认值。

use super::{Block, Document, DocumentSettings};
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
fn missing_global_text_and_paragraph_settings_use_bundled_defaults()
-> Result<(), Box<dyn std::error::Error>> {
    let settings: DocumentSettings = serde_json::from_str("{}")?;
    let default = super::ParagraphLayoutStyle::default();

    assert_eq!(settings.paragraph_layout, default);
    assert_eq!(settings.default_font_size_centipoints, 1200);
    assert_eq!(settings.paragraph_layout.space_before_centipoints, 0);
    assert_eq!(settings.paragraph_layout.space_after_centipoints, 1000);
    assert!(settings.paragraph_layout.widow_orphan_control);
    Ok(())
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
                assert!(serde_json::from_value::<super::InlineNode>(invalid).is_err());
            }
        }
    }
}

#[test]
fn recognized_nodes_reject_non_object_or_malformed_content() {
    for invalid in [serde_json::json!(null), serde_json::json!([])] {
        assert!(serde_json::from_value::<Block>(invalid.clone()).is_err());
        assert!(serde_json::from_value::<super::InlineNode>(invalid).is_err());
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
        serde_json::from_value::<super::InlineNode>(serde_json::json!({
            "type": "run", "text": 42
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
                "children": [{"type": "run", "text": "标题", "language": "zh-Hans"}]
            }]
        }]},
        "children": [{
            "type": "environment", "scope": "block", "kind": "showcase", "id": "custom",
            "props": {"displayName": "Example"},
            "children": [{"type": "paragraph", "id": "paragraph", "inlines": [{
                "type": "environment", "scope": "inline", "kind": "italic", "id": "italic",
                "children": [{"type": "run", "text": "body"}]
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
            "caret.paragraph.layout-settings",
            "caret.text.default-font-size",
            "caret.text.language-tags",
        ]
    );
    Ok(())
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

    let mut inline = super::InlineNode::Run(super::Run {
        text: "x".to_owned(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    });
    for _ in 0..65 {
        inline = super::InlineNode::Environment(super::InlineEnvironment {
            id: super::NodeId::generate(),
            kind: super::InlineEnvironmentKind::Bold,
            definition_id: None,
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
