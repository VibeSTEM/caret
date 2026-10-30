//! 验证字体偏好的持久化、脚本隔离、语言逐级回退和候选优先级。

use std::collections::BTreeMap;

use caret_doc::{CdocFile, Document, DocumentSettings, OpenedDocument};
use icu_locale_core::subtags::Script;

use super::{FontPreferenceError, FontPreferenceRules, FontPreferenceSource, FontSelectionContext};

fn script(tag: &str) -> Script {
    tag.parse()
        .unwrap_or_else(|error| panic!("invalid test script: {error}"))
}

fn compiled(settings: &DocumentSettings) -> FontPreferenceRules {
    FontPreferenceRules::from_document_settings(settings)
        .unwrap_or_else(|error| panic!("valid settings rejected: {error}"))
}

fn context(script_tag: &str) -> FontSelectionContext<'_> {
    FontSelectionContext {
        script: script(script_tag),
        language: None,
        math_context: false,
        explicit_family: None,
        code: false,
    }
}

#[test]
fn font_settings_roundtrip_preserves_unknown_script_and_declares_capability() {
    let mut document = Document::blank();
    document.global_settings = DocumentSettings {
        default_font_family: Some("Latin Default".to_owned()),
        default_cjk_font_family: Some("CJK Default".to_owned()),
        default_math_font_family: Some("Math Default".to_owned()),
        script_font_families: BTreeMap::from([("Qaaa".to_owned(), "Future Font".to_owned())]),
        language_script_font_families: BTreeMap::from([(
            "Cyrl".to_owned(),
            BTreeMap::from([("sr-Cyrl".to_owned(), "Serbian Font".to_owned())]),
        )]),
        ..DocumentSettings::default()
    };
    let encoded = serde_json::to_value(&document)
        .unwrap_or_else(|error| panic!("document serialization failed: {error}"));
    let decoded: Document = serde_json::from_value(encoded)
        .unwrap_or_else(|error| panic!("document deserialization failed: {error}"));

    assert_eq!(decoded.global_settings, document.global_settings);
    assert!(
        decoded
            .required_features()
            .iter()
            .any(|feature| feature == "caret.font.family-preferences")
    );
    assert!(
        !compiled(&decoded.global_settings)
            .family_candidates(context("Zzzz"))
            .iter()
            .any(|candidate| candidate.family == "Future Font")
    );
    let file = CdocFile::new(decoded)
        .unwrap_or_else(|error| panic!("font settings document rejected: {error}"));
    let bytes = file
        .to_bytes()
        .unwrap_or_else(|error| panic!("cdoc serialization failed: {error}"));
    let reopened = caret_doc::open(&bytes)
        .unwrap_or_else(|error| panic!("font settings cdoc rejected: {error}"));
    assert!(matches!(reopened, OpenedDocument::Editable(_)));
}

#[test]
fn explicit_math_language_script_and_defaults_keep_their_order() {
    let settings = DocumentSettings {
        default_font_family: Some("Latin".to_owned()),
        default_cjk_font_family: Some("CJK".to_owned()),
        default_math_font_family: Some("Math".to_owned()),
        script_font_families: BTreeMap::from([("Hani".to_owned(), "Han".to_owned())]),
        language_script_font_families: BTreeMap::from([(
            "Hani".to_owned(),
            BTreeMap::from([("ja".to_owned(), "Japanese Han".to_owned())]),
        )]),
        ..DocumentSettings::default()
    };
    let rules = compiled(&settings);
    let candidates = rules.family_candidates(FontSelectionContext {
        language: Some("ja-JP"),
        math_context: true,
        explicit_family: Some("Local"),
        ..context("Hani")
    });

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.family)
            .collect::<Vec<_>>(),
        ["Local", "Math", "Japanese Han", "Han", "CJK", "Latin"]
    );
    assert_eq!(candidates[0].source, FontPreferenceSource::Explicit);
    assert_eq!(candidates[1].source, FontPreferenceSource::Math);
}

#[test]
fn code_font_uses_only_explicit_code_family_then_monospace_fallback() {
    let settings = DocumentSettings {
        default_font_family: Some("Body Latin".to_owned()),
        default_cjk_font_family: Some("Body CJK".to_owned()),
        default_math_font_family: Some("Math".to_owned()),
        default_code_font_family: "Code Family".to_owned(),
        script_font_families: BTreeMap::from([("Latn".to_owned(), "Script Latin".to_owned())]),
        ..DocumentSettings::default()
    };
    let rules = compiled(&settings);
    let candidates = rules.family_candidates(FontSelectionContext {
        code: true,
        math_context: true,
        explicit_family: Some("Inline Override"),
        ..context("Latn")
    });

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.family)
            .collect::<Vec<_>>(),
        ["Inline Override", "Code Family"]
    );
    assert_eq!(candidates[1].source, FontPreferenceSource::Code);

    let default_rules = compiled(&DocumentSettings::default());
    assert!(
        default_rules
            .family_candidates(FontSelectionContext {
                code: true,
                ..context("Latn")
            })
            .is_empty()
    );
}

#[test]
fn language_overrides_only_apply_inside_the_actual_script() {
    let settings = DocumentSettings {
        default_font_family: Some("Latin".to_owned()),
        script_font_families: BTreeMap::from([("Cyrl".to_owned(), "Cyrillic".to_owned())]),
        language_script_font_families: BTreeMap::from([(
            "Cyrl".to_owned(),
            BTreeMap::from([
                ("sr-Cyrl".to_owned(), "Serbian Cyrillic".to_owned()),
                ("sr".to_owned(), "Serbian Shared".to_owned()),
            ]),
        )]),
        ..DocumentSettings::default()
    };
    let rules = compiled(&settings);
    let cyrillic = rules.family_candidates(FontSelectionContext {
        language: Some("sr-Cyrl-RS"),
        ..context("Cyrl")
    });
    let latin = rules.family_candidates(FontSelectionContext {
        language: Some("sr-Latn-RS"),
        ..context("Latn")
    });

    assert_eq!(
        cyrillic
            .iter()
            .map(|candidate| candidate.family)
            .collect::<Vec<_>>(),
        ["Serbian Cyrillic", "Serbian Shared", "Cyrillic", "Latin"]
    );
    assert_eq!(
        latin
            .iter()
            .map(|candidate| candidate.family)
            .collect::<Vec<_>>(),
        ["Latin"]
    );
}

#[test]
fn cjk_defaults_cover_all_cjk_scripts_and_allow_a_script_override() {
    let settings = DocumentSettings {
        default_font_family: Some("Latin".to_owned()),
        default_cjk_font_family: Some("CJK".to_owned()),
        script_font_families: BTreeMap::from([("Hang".to_owned(), "Korean".to_owned())]),
        ..DocumentSettings::default()
    };
    let rules = compiled(&settings);
    for tag in ["Hani", "Hira", "Kana", "Bopo"] {
        assert_eq!(rules.family_candidates(context(tag))[0].family, "CJK");
    }
    let korean = rules.family_candidates(context("Hang"));
    assert_eq!(
        korean
            .iter()
            .map(|candidate| candidate.family)
            .collect::<Vec<_>>(),
        ["Korean", "CJK", "Latin"]
    );
    assert_eq!(rules.family_candidates(context("Arab"))[0].family, "Latin");
}

#[test]
fn language_lookup_keeps_untracked_locales_and_removes_extensions_before_parent() {
    let settings = DocumentSettings {
        language_script_font_families: BTreeMap::from([(
            "Adlm".to_owned(),
            BTreeMap::from([
                ("ff-Adlm-GN".to_owned(), "Guinean Adlam".to_owned()),
                ("ff".to_owned(), "Adlam Shared".to_owned()),
            ]),
        )]),
        ..DocumentSettings::default()
    };
    let rules = compiled(&settings);
    let candidates = rules.family_candidates(FontSelectionContext {
        language: Some("ff-Adlm-GN-u-nu-latn"),
        ..context("Adlm")
    });
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.family)
            .collect::<Vec<_>>(),
        ["Guinean Adlam", "Adlam Shared"]
    );
    assert!(rules.family_candidates(context("Adlm")).is_empty());
}

#[test]
fn normalization_rejects_ambiguous_keys_and_keeps_one_duplicate_family() {
    let invalid = DocumentSettings {
        script_font_families: BTreeMap::from([("Cyrillic".to_owned(), "Font".to_owned())]),
        ..DocumentSettings::default()
    };
    assert!(matches!(
        FontPreferenceRules::from_document_settings(&invalid),
        Err(FontPreferenceError::InvalidScriptTag(_))
    ));
    for (writing_system_variant, actual_scripts) in [
        ("Hans", ["Hani"].as_slice()),
        ("Hant", ["Hani"].as_slice()),
        ("Cyrs", ["Cyrl"].as_slice()),
        ("Jpan", ["Hani", "Hira", "Kana"].as_slice()),
        ("Kore", ["Hani", "Hang"].as_slice()),
    ] {
        let invalid = DocumentSettings {
            script_font_families: BTreeMap::from([(
                writing_system_variant.to_owned(),
                "Font".to_owned(),
            )]),
            ..DocumentSettings::default()
        };
        let rules = compiled(&invalid);
        for script_tag in actual_scripts {
            assert!(
                !rules
                    .family_candidates(context(script_tag))
                    .iter()
                    .any(|candidate| candidate.family == "Font"),
                "writing-system variant {writing_system_variant} must not alias {script_tag}"
            );
        }
    }
    let duplicate = DocumentSettings {
        script_font_families: BTreeMap::from([
            ("Cyrl".to_owned(), "A".to_owned()),
            ("cyrl".to_owned(), "B".to_owned()),
        ]),
        ..DocumentSettings::default()
    };
    assert!(matches!(
        FontPreferenceRules::from_document_settings(&duplicate),
        Err(FontPreferenceError::DuplicateScriptTag(_))
    ));
    let settings = DocumentSettings {
        default_font_family: Some("Same Font".to_owned()),
        default_math_font_family: Some("same font".to_owned()),
        ..DocumentSettings::default()
    };
    let rules = compiled(&settings);
    let candidates = rules.family_candidates(FontSelectionContext {
        math_context: true,
        ..context("Latn")
    });
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].source, FontPreferenceSource::Math);
}

#[test]
fn normalized_language_maps_reject_duplicate_script_and_language_keys() {
    let duplicate_script = DocumentSettings {
        language_script_font_families: BTreeMap::from([
            (
                "Cyrl".to_owned(),
                BTreeMap::from([("sr".to_owned(), "A".to_owned())]),
            ),
            (
                "cyrl".to_owned(),
                BTreeMap::from([("ru".to_owned(), "B".to_owned())]),
            ),
        ]),
        ..DocumentSettings::default()
    };
    let Err(error) = FontPreferenceRules::from_document_settings(&duplicate_script) else {
        panic!("normalized duplicate script must be rejected");
    };
    assert_eq!(
        error,
        FontPreferenceError::DuplicateScriptTag("cyrl".to_owned())
    );
    assert_eq!(error.to_string(), "font.duplicate_script_tag: cyrl");

    let duplicate_language = DocumentSettings {
        language_script_font_families: BTreeMap::from([(
            "Latn".to_owned(),
            BTreeMap::from([
                ("en-US".to_owned(), "A".to_owned()),
                ("en-us".to_owned(), "B".to_owned()),
            ]),
        )]),
        ..DocumentSettings::default()
    };
    let Err(error) = FontPreferenceRules::from_document_settings(&duplicate_language) else {
        panic!("normalized duplicate language must be rejected");
    };
    assert_eq!(
        error,
        FontPreferenceError::DuplicateLanguageTag("en-us".to_owned())
    );
    assert_eq!(error.to_string(), "font.duplicate_language_tag: en-us");
}

#[test]
fn empty_requested_families_are_rejected_and_grandfathered_languages_match_exactly() {
    let empty = DocumentSettings {
        default_font_family: Some(" \t".to_owned()),
        ..DocumentSettings::default()
    };
    let Err(error) = FontPreferenceRules::from_document_settings(&empty) else {
        panic!("blank requested family must be rejected");
    };
    assert_eq!(error, FontPreferenceError::EmptyFamily);
    assert_eq!(error.to_string(), "font.empty_family");

    let rules = compiled(&DocumentSettings {
        language_script_font_families: BTreeMap::from([(
            "Latn".to_owned(),
            BTreeMap::from([("i-klingon".to_owned(), "Private Latin".to_owned())]),
        )]),
        ..DocumentSettings::default()
    });
    let candidates = rules.family_candidates(FontSelectionContext {
        language: Some("I-KLINGON"),
        ..context("Latn")
    });
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].family, "Private Latin");
    assert_eq!(candidates[0].source, FontPreferenceSource::LanguageScript);
}
