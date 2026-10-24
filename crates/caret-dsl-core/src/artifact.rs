//! 多文档 Artifact 快照聚合及来源关联。

use std::collections::{BTreeSet, HashSet};
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::diagnostic::source_position;
use crate::{Diagnostic, Diagnostics, Document, Kind, emit_ts, parse};

/// Artifact 聚合允许的来源文档上限。
pub const MAX_ARTIFACT_SOURCES: usize = 256;
/// Artifact 聚合允许读取的 UTF-8 源码总字节数。
pub const MAX_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;
const MAX_ARTIFACT_DECLARATIONS: usize = 65_536;

/// 提供给聚合器的逻辑来源 ID 与 UTF-8 `.pa` 内容。
#[derive(Debug, Clone, Copy)]
pub struct ArtifactSource<'a> {
    pub source_id: &'a str,
    pub source: &'a str,
}

/// 经过统一 parser 校验的单个来源文档。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactDocument {
    source_id: String,
    version: u32,
    kind: Kind,
    locale: Option<String>,
    payload: Document,
}

impl ArtifactDocument {
    /// 返回稳定的调用方来源 ID；建议使用仓库相对 POSIX 路径。
    pub fn source_id(&self) -> &str {
        &self.source_id
    }

    /// 返回已经验证的 DSL 版本。
    pub fn version(&self) -> u32 {
        self.version
    }

    /// 返回领域种类。
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// 返回语言 Artifact 的规范化 locale；其他 kind 返回 `None`。
    pub fn locale(&self) -> Option<&str> {
        self.locale.as_deref()
    }

    /// 返回只读规范化 payload。
    pub fn payload(&self) -> &Document {
        &self.payload
    }
}

/// 按 kind 与来源 ID 排序的不可变文档快照集合。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactSnapshot {
    documents: Vec<ArtifactDocument>,
}

impl ArtifactSnapshot {
    /// 返回稳定顺序的只读文档集合。
    pub fn documents(&self) -> &[ArtifactDocument] {
        &self.documents
    }

    /// 返回序列化字段顺序固定、集合顺序规范化的 JSON 快照。
    pub fn canonical_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// 生成快照中所有语言文档的 TS；目标名冲突时拒绝整批输出。
    pub fn emit_translation_sources(&self) -> Result<Vec<TranslationSource>, ArtifactDiagnostics> {
        let mut outputs = Vec::new();
        let mut names = HashSet::new();
        let mut diagnostics = Vec::new();

        for artifact in &self.documents {
            if artifact.kind != Kind::Language {
                continue;
            }
            let Some(locale) = artifact.locale.as_deref() else {
                diagnostics.push(artifact_diagnostic(
                    &artifact.source_id,
                    "pa.missing_language",
                    "language Artifact requires a locale",
                    "",
                ));
                continue;
            };
            let catalog = artifact.payload.catalog.as_deref().unwrap_or("caret");
            let file_name = format!("{}_{}.ts", catalog, locale.replace('-', "_"));
            // Windows/macOS 常用文件系统大小写不敏感；同一输出集合必须在
            // 三个平台使用相同的冲突判定。
            if !names.insert(file_name.to_ascii_lowercase()) {
                diagnostics.push(artifact_diagnostic(
                    &artifact.source_id,
                    "pa.duplicate_translation_output",
                    format!("multiple language Artifacts generate '{file_name}'"),
                    "",
                ));
                continue;
            }
            match emit_ts(&artifact.payload, locale) {
                Ok(contents) => outputs.push(TranslationSource {
                    source_id: artifact.source_id.clone(),
                    file_name,
                    contents,
                }),
                Err(error) => diagnostics.extend(source_diagnostics(&artifact.source_id, error)),
            }
        }

        if outputs.is_empty() && diagnostics.is_empty() {
            diagnostics.push(artifact_diagnostic(
                "<artifact>",
                "pa.no_language_artifact",
                "artifact has no language documents to emit",
                "",
            ));
        }
        if diagnostics.is_empty() {
            Ok(outputs)
        } else {
            Err(ArtifactDiagnostics { diagnostics })
        }
    }
}

/// 已验证语言 Artifact 的单个 TS 输出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationSource {
    source_id: String,
    file_name: String,
    contents: String,
}

impl TranslationSource {
    /// 返回来源文档 ID。
    pub fn source_id(&self) -> &str {
        &self.source_id
    }

    /// 返回安全的单层文件名。
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// 返回规范化 Qt TS XML。
    pub fn contents(&self) -> &str {
        &self.contents
    }
}

/// 带来源 ID 的单条解析诊断。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDiagnostic {
    pub source_id: String,
    pub diagnostic: Diagnostic,
}

/// 多文档聚合失败；所有可解析来源的诊断会一并返回。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDiagnostics {
    pub diagnostics: Vec<ArtifactDiagnostic>,
}

impl Display for ArtifactDiagnostics {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        for (index, item) in self.diagnostics.iter().enumerate() {
            if index > 0 {
                formatter.write_str("\n")?;
            }
            write!(formatter, "{}: {}", item.source_id, item.diagnostic)?;
        }
        Ok(())
    }
}

impl std::error::Error for ArtifactDiagnostics {}

/// 解析一组 `.pa` 来源并构造确定性快照；任一来源失败时不返回部分快照。
pub fn parse_artifacts<'a>(
    sources: impl IntoIterator<Item = ArtifactSource<'a>>,
) -> Result<ArtifactSnapshot, ArtifactDiagnostics> {
    let mut seen_ids = BTreeSet::new();
    let mut documents = Vec::new();
    let mut diagnostics = Vec::new();
    let mut source_count = 0usize;
    let mut total_bytes = 0usize;
    let mut total_declarations = 0usize;

    for input in sources {
        source_count += 1;
        if source_count > MAX_ARTIFACT_SOURCES {
            diagnostics.push(artifact_diagnostic(
                input.source_id,
                "pa.too_many_artifact_sources",
                format!("more than {MAX_ARTIFACT_SOURCES} source documents"),
                input.source,
            ));
            break;
        }

        total_bytes = match total_bytes.checked_add(input.source.len()) {
            Some(bytes) if bytes <= MAX_ARTIFACT_BYTES => bytes,
            _ => {
                diagnostics.push(artifact_diagnostic(
                    input.source_id,
                    "pa.artifact_too_large",
                    format!("artifact sources exceed {MAX_ARTIFACT_BYTES} bytes"),
                    input.source,
                ));
                break;
            }
        };

        if input.source_id.trim().is_empty() {
            diagnostics.push(artifact_diagnostic(
                input.source_id,
                "pa.empty_source_id",
                "source ID must not be empty",
                input.source,
            ));
            continue;
        }
        if !seen_ids.insert(input.source_id) {
            diagnostics.push(artifact_diagnostic(
                input.source_id,
                "pa.duplicate_source_id",
                format!("source ID '{}' is repeated", input.source_id),
                input.source,
            ));
            continue;
        }

        match parse(input.source) {
            Ok(payload) => {
                total_declarations += payload.messages.len() + payload.values.len();
                if total_declarations > MAX_ARTIFACT_DECLARATIONS {
                    diagnostics.push(artifact_diagnostic(
                        input.source_id,
                        "pa.too_many_artifact_declarations",
                        format!("more than {MAX_ARTIFACT_DECLARATIONS} declarations"),
                        input.source,
                    ));
                    break;
                }
                documents.push(ArtifactDocument {
                    source_id: input.source_id.to_owned(),
                    version: payload.version,
                    kind: payload.kind,
                    locale: payload.language.clone(),
                    payload,
                });
            }
            Err(error) => diagnostics.extend(source_diagnostics(input.source_id, error)),
        }
    }

    if source_count == 0 {
        diagnostics.push(artifact_diagnostic(
            "<artifact>",
            "pa.empty_artifact",
            "artifact requires at least one source document",
            "",
        ));
    }
    if !diagnostics.is_empty() {
        return Err(ArtifactDiagnostics { diagnostics });
    }

    documents.sort_by(|left, right| {
        kind_order(left.kind)
            .cmp(&kind_order(right.kind))
            .then_with(|| left.source_id.cmp(&right.source_id))
    });
    Ok(ArtifactSnapshot { documents })
}

fn kind_order(kind: Kind) -> u8 {
    match kind {
        Kind::Language => 0,
        Kind::Theme => 1,
        Kind::Variables => 2,
    }
}

fn source_diagnostics(source_id: &str, error: Diagnostics) -> Vec<ArtifactDiagnostic> {
    error
        .diagnostics
        .into_iter()
        .map(|diagnostic| ArtifactDiagnostic {
            source_id: source_id.to_owned(),
            diagnostic,
        })
        .collect()
}

fn artifact_diagnostic(
    source_id: &str,
    code: &str,
    message: impl Into<String>,
    source: &str,
) -> ArtifactDiagnostic {
    let (line, column) = source_position(source, 0);
    let diagnostic = Diagnostic {
        code: code.to_owned(),
        message: message.into(),
        start_offset: 0,
        end_offset: 0,
        line,
        column,
    };
    ArtifactDiagnostic {
        source_id: source_id.to_owned(),
        diagnostic,
    }
}

#[cfg(test)]
mod tests {
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
                let mut source =
                    String::from("version: 1\nkind: language\nlanguage: en\n\n[App]\n");
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
}
