//! 验证 DSL 产物的生成和一致性。

#![allow(clippy::expect_used)]

use super::{
    ArtifactSource, MAX_ARTIFACT_BYTES, MAX_ARTIFACT_DECLARATIONS, MAX_ARTIFACT_SOURCES,
    parse_artifacts,
};

#[test]
fn empty_and_duplicate_sources_return_stable_diagnostics() {
    let empty = parse_artifacts([]).expect_err("empty input has no snapshot");
    assert_eq!(empty.diagnostics[0].diagnostic.code, "pa.empty_artifact");

    let source = "version: 1\nkind: variables\n\nvalues:\n  scale: real = 0.5\n";
    let duplicate = parse_artifacts([
        ArtifactSource {
            source_id: "same.pa",
            source,
        },
        ArtifactSource {
            source_id: "same.pa",
            source,
        },
    ])
    .expect_err("source IDs must be unique");
    assert_eq!(
        duplicate.diagnostics[0].diagnostic.code,
        "pa.duplicate_source_id"
    );
    assert_eq!(duplicate.diagnostics[0].source_id, "same.pa");
}

#[test]
fn artifact_source_count_is_bounded() {
    let source = "version: 1\nkind: variables\n\nvalues:\n  scale: real = 0.5\n";
    let source_ids = (0..=MAX_ARTIFACT_SOURCES)
        .map(|index| format!("source-{index}.pa"))
        .collect::<Vec<_>>();
    let inputs = source_ids
        .iter()
        .map(|source_id| ArtifactSource { source_id, source });
    let error = parse_artifacts(inputs).expect_err("too many sources must fail");
    assert!(
        error
            .diagnostics
            .iter()
            .any(|item| { item.diagnostic.code == "pa.too_many_artifact_sources" })
    );
}

#[test]
fn aggregate_input_bytes_are_bounded_before_parsing() {
    let oversized = "x".repeat(MAX_ARTIFACT_BYTES + 1);
    let error = parse_artifacts([ArtifactSource {
        source_id: "oversized.pa",
        source: &oversized,
    }])
    .expect_err("aggregate byte limit must fail before parsing the document");
    assert_eq!(
        error.diagnostics[0].diagnostic.code,
        "pa.artifact_too_large"
    );
    assert_eq!(error.diagnostics[0].source_id, "oversized.pa");
}

#[test]
#[cfg_attr(miri, ignore)]
fn artifact_declaration_total_is_bounded() {
    let sources = (0..4)
        .map(|source_index| {
            let mut source = String::from("version: 1\nkind: language\nlanguage: en\n\n[App]\n");
            for index in 0..16_384 {
                source.push_str(&format!(
                    "message-{source_index}-{index}:\n  src: Source {source_index}-{index}\n"
                ));
            }
            source
        })
        .collect::<Vec<_>>();
    let exact_limit = ["a.pa", "b.pa", "c.pa", "d.pa"]
        .iter()
        .zip(&sources)
        .map(|(source_id, source)| ArtifactSource { source_id, source });
    let snapshot = parse_artifacts(exact_limit).expect("exact aggregate limit is valid");
    assert_eq!(
        snapshot
            .documents()
            .iter()
            .map(|document| document.payload().messages.len())
            .sum::<usize>(),
        MAX_ARTIFACT_DECLARATIONS
    );

    let extra = "version: 1\nkind: language\nlanguage: en\n\n[App]\nextra:\n  src: Extra\n";
    let over_limit = ["a.pa", "b.pa", "c.pa", "d.pa"]
        .iter()
        .zip(&sources)
        .map(|(source_id, source)| ArtifactSource { source_id, source })
        .chain(std::iter::once(ArtifactSource {
            source_id: "e.pa",
            source: extra,
        }));
    let error = parse_artifacts(over_limit).expect_err("aggregate limit must be enforced");
    assert_eq!(
        error
            .diagnostics
            .last()
            .map(|item| item.diagnostic.code.as_str()),
        Some("pa.too_many_artifact_declarations")
    );
}
