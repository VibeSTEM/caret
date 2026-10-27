//! `.pa` DSL 公共门面；内部模块分离模型、诊断、解析、校验及输出。

mod artifact;
mod ast;
mod diagnostic;
mod evaluation;
mod format;
mod parser;
mod syntax;
mod ts;
mod validation;

pub mod fsm;

pub use artifact::{
    ArtifactDiagnostic, ArtifactDiagnostics, ArtifactDocument, ArtifactSnapshot, ArtifactSource,
    MAX_ARTIFACT_BYTES, MAX_ARTIFACT_SOURCES, TranslationSource, parse_artifacts,
};
pub use ast::{
    Document, Kind, Message, SourceKind, Status, Translation, ValueType, Variable, source_prefix,
};
pub use diagnostic::{Diagnostic, Diagnostics};
pub use evaluation::{EvaluatedValue, EvaluatedVariables, evaluate_theme, evaluate_variables};
pub use format::{format_document, format_source};
pub use parser::parse;
// Pest 派生的 Rule 原已公开，移动 grammar 时保留其根路径。
pub use syntax::{MAX_SOURCE_BYTES, Rule, source_kind};
pub use ts::emit_ts;

#[cfg(test)]
mod tests;
