#![allow(clippy::expect_used)]

use caret_dsl_core::{ArtifactSource, Kind, emit_ts, parse, parse_artifacts, source_prefix};

const CATALOG: &str = include_str!("fixtures/caret-cn.pa");

#[test]
fn artifact_metadata_matches_the_validated_payload_and_normalized_locale() {
    let snapshot = parse_artifacts([
        ArtifactSource {
            source_id: "theme.pa",
            source: "version: 1\nkind: theme\n",
        },
        ArtifactSource {
            source_id: "language.pa",
            source: "version: 1\nkind: language\nlanguage: cn\n\n[App]\nopen:\n  src: Open\n  tr: 打开\n",
        },
    ])
    .expect("valid documents should aggregate");
    let language = snapshot
        .documents()
        .iter()
        .find(|document| document.source_id() == "language.pa")
        .expect("language artifact is retained");
    assert_eq!(language.version(), 1);
    assert_eq!(language.kind(), Kind::Language);
    assert_eq!(language.locale(), Some("zh-CN"));
    assert_eq!(language.payload().language.as_deref(), language.locale());
    assert_eq!(language.payload().messages.len(), 1);

    let theme = snapshot
        .documents()
        .iter()
        .find(|document| document.source_id() == "theme.pa")
        .expect("theme artifact is retained");
    assert_eq!(theme.kind(), Kind::Theme);
    assert_eq!(theme.locale(), None);
    assert!(theme.payload().values.is_empty());
}

#[test]
fn fixture_is_a_locale_catalog() {
    let document = parse(CATALOG).expect("fixture should parse");
    assert_eq!(document.messages.len(), 3);
    assert_eq!(
        document
            .messages
            .values()
            .find(|message| message.context == "FileMenu" && message.id == "open")
            .map(|message| message.source.as_str()),
        Some("Open")
    );
}

#[test]
fn fixture_emits_locale_and_uses_longest_source() {
    let document = parse(CATALOG).expect("fixture should parse");
    let ts = emit_ts(&document, "cn").expect("locale should emit");
    assert!(ts.contains("language=\"zh_CN\""));
    assert_eq!(
        source_prefix(&document, "ok ok!").map(|(key, _)| key),
        Some("ok-pair")
    );
}

#[test]
fn artifact_outputs_match_the_pre_refactor_baseline() -> Result<(), caret_dsl_core::Diagnostics> {
    assert_eq!(format!("{:?}", caret_dsl_core::Rule::document), "document");
    let source = include_str!("../../../tests/fixtures/dsl/artifacts.pa");
    let canonical = include_str!("../../../tests/fixtures/dsl/artifacts.formatted.pa");
    let expected_ts = include_str!("../../../tests/fixtures/dsl/artifacts.ts");
    let document = parse(source)?;
    assert_eq!(caret_dsl_core::format_document(&document), canonical);
    assert_eq!(caret_dsl_core::format_source(canonical)?, canonical);
    assert_eq!(parse(canonical)?, document);
    assert_eq!(emit_ts(&document, "cn")?, expected_ts);
    assert_eq!(emit_ts(&document, "zh_CN")?, expected_ts);
    Ok(())
}

#[test]
fn artifact_snapshot_has_a_stable_cross_platform_encoding() {
    let input = ArtifactSource {
        source_id: "tests/fixtures/dsl/artifacts.pa",
        source: include_str!("../../../tests/fixtures/dsl/artifacts.pa"),
    };
    let snapshot = parse_artifacts([input]).expect("fixture should aggregate");
    let encoded = snapshot.canonical_json().expect("snapshot serializes");
    assert_eq!(
        encoded,
        include_str!("fixtures/artifact-snapshot.json").trim()
    );
}

#[test]
fn snapshot_emits_one_deterministic_ts_per_locale() {
    let snapshot = parse_artifacts([ArtifactSource {
        source_id: "caret-cn.pa",
        source: CATALOG,
    }])
    .expect("catalog should aggregate");
    let outputs = snapshot
        .emit_translation_sources()
        .expect("language Artifact should emit TS");
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].source_id(), "caret-cn.pa");
    assert_eq!(outputs[0].file_name(), "caret_zh_CN.ts");
    assert!(outputs[0].contents().contains("language=\"zh_CN\""));

    let duplicate = parse_artifacts([
        ArtifactSource {
            source_id: "first.pa",
            source: CATALOG,
        },
        ArtifactSource {
            source_id: "second.pa",
            source: CATALOG,
        },
    ])
    .expect("source documents have distinct IDs");
    let error = duplicate
        .emit_translation_sources()
        .expect_err("same catalog and locale cannot overwrite each other");
    assert_eq!(
        error.diagnostics[0].diagnostic.code,
        "pa.duplicate_translation_output"
    );
}

#[test]
fn translation_names_that_differ_only_by_case_are_rejected() {
    let english = "version: 1\nkind: language\ncatalog: Caret\nlanguage: en\n\n[Menu]\nopen:\n  src: Open\n  tr: Open\n";
    let chinese = english.replace("catalog: Caret", "catalog: caret");
    let snapshot = parse_artifacts([
        ArtifactSource {
            source_id: "upper.pa",
            source: english,
        },
        ArtifactSource {
            source_id: "lower.pa",
            source: &chinese,
        },
    ])
    .expect("source identifiers are distinct");

    let error = snapshot
        .emit_translation_sources()
        .expect_err("case-insensitive filesystems cannot represent both output names");
    assert_eq!(
        error.diagnostics[0].diagnostic.code,
        "pa.duplicate_translation_output"
    );
}

#[test]
fn artifact_order_is_canonical_and_failures_keep_source_ids() {
    let variables = "version: 1\nkind: variables\n\nvalues:\n  scale: real = 0.5\n";
    let theme = "version: 1\nkind: theme\n\nvalues:\n  control-height: int = 24\n";
    let left = parse_artifacts([
        ArtifactSource {
            source_id: "z-theme.pa",
            source: theme,
        },
        ArtifactSource {
            source_id: "b-vars.pa",
            source: variables,
        },
        ArtifactSource {
            source_id: "a-vars.pa",
            source: variables,
        },
    ])
    .expect("all documents should aggregate");
    let right = parse_artifacts([
        ArtifactSource {
            source_id: "a-vars.pa",
            source: variables,
        },
        ArtifactSource {
            source_id: "b-vars.pa",
            source: variables,
        },
        ArtifactSource {
            source_id: "z-theme.pa",
            source: theme,
        },
    ])
    .expect("same documents in another input order should aggregate");
    assert_eq!(left, right);
    assert_eq!(left.documents()[0].source_id(), "z-theme.pa");
    assert_eq!(left.documents()[1].source_id(), "a-vars.pa");

    let error = parse_artifacts([ArtifactSource {
        source_id: "broken.pa",
        source: "version: 2\nkind: variables\n\nvalues:\n  scale: real = 0.5\n",
    }])
    .expect_err("unsupported version should fail");
    assert_eq!(error.diagnostics[0].source_id, "broken.pa");
    assert_eq!(
        error.diagnostics[0].diagnostic.code,
        "pa.unsupported_version"
    );
}
