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
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const CATALOG: &str = "version: 1\nkind: language\nlanguage: zh_CN\nsourcelanguage: en\n\n[FileMenu]\nopen:\n  src: Open\n  tr: 打开\n\nok:\n  src: ok\n  tr: 好的\n\nok-pair:\n  src: ok ok\n  tr: 好好的\n";

    #[test]
    fn parses_ts_shaped_catalog() {
        let document = parse(CATALOG).expect("valid catalog");
        assert_eq!(document.language.as_deref(), Some("zh-CN"));
        assert_eq!(document.messages.len(), 3);
        assert_eq!(document.messages["FileMenu\u{1f}open"].source, "Open");
    }

    #[test]
    fn parses_status_plural_and_comments() {
        let document = parse("version: 1\nkind: language\nlanguage: cn\n\n[FileMenu]\nfiles-selected:\n  src: \"%n file(s) selected\"\n  numerus: true\n  tr: [\"已选择 %n 个文件\"]\n  comment: 菜单项\n  extra: 翻译提示\n  st: unfinished\n").expect("valid plural");
        let message = &document.messages["FileMenu\u{1f}files-selected"];
        assert!(message.numerus);
        assert_eq!(message.translations["zh-CN"].forms.len(), 1);
        assert_eq!(message.status, Some(Status::Unfinished));
    }

    #[test]
    fn parses_variables_and_rejects_underscored_key() {
        let document = parse("version: 1\nkind: variables\n\nvalues:\n  spacing-small: real = 8\n")
            .expect("valid variables");
        assert_eq!(document.values[0].name, "spacing-small");
        assert!(
            parse("version: 1\nkind: variables\n\nvalues:\n  spacing_small: real = 8\n").is_err()
        );
    }

    #[test]
    fn evaluates_theme_values_with_theme_kind_only() {
        let theme = "version: 1\nkind: theme\n\nvalues:\n  color-background: string = \"#ffffff\"\n  spacing-small: int = 8\n";
        let evaluated = evaluate_theme(theme).expect("valid theme values");
        assert_eq!(
            evaluated.get("color-background"),
            Some(&EvaluatedValue::String("#ffffff".to_owned()))
        );
        assert_eq!(
            evaluated.get("spacing-small"),
            Some(&EvaluatedValue::Int(8))
        );
        assert_eq!(
            evaluate_theme("version: 1\nkind: variables\n\nvalues:\n  size: int = 8\n")
                .expect_err("theme evaluator rejects variables")
                .diagnostics[0]
                .code,
            "theme.wrong_document_kind"
        );
    }

    #[test]
    fn rejects_duplicate_context_id_and_missing_source() {
        let error = parse("version: 1\nkind: language\nlanguage: cn\n\n[FileMenu]\nopen:\n  src: Open\nopen:\n  src: Open again\n").expect_err("duplicate id");
        assert!(
            error
                .diagnostics
                .iter()
                .any(|item| item.code == "pa.duplicate_message")
        );
    }

    #[test]
    fn longest_source_and_ts_output_work() {
        let document = parse(CATALOG).expect("catalog");
        assert_eq!(
            source_prefix(&document, "ok ok!").map(|(id, _)| id),
            Some("ok-pair")
        );
        let ts = emit_ts(&document, "cn").expect("TS");
        assert!(ts.contains("language=\"zh_CN\""));
        assert!(ts.contains("<name>FileMenu</name>"));
        assert!(ts.contains("id=\"FileMenu.ok\""));
        assert!(ts.contains("<source>ok ok</source>"));
        assert!(!ts.contains("<numerusform>好的</numerusform>"));
    }

    #[test]
    fn rejects_duplicate_sources_and_invalid_locale() {
        let error = parse(
            "version: 1\nkind: language\nlanguage: en--US\n\n[Menu]\na:\n  src: One\n  tr: Uno\nb:\n  src: One\n  tr: Dos\n",
        )
        .expect_err("invalid locale and duplicate source");
        assert!(
            error
                .diagnostics
                .iter()
                .any(|item| item.code == "pa.invalid_locale")
        );
        assert!(
            error
                .diagnostics
                .iter()
                .any(|item| item.code == "pa.duplicate_source")
        );
    }

    #[test]
    fn accepts_distinct_equal_length_sources() {
        let document = parse(
            "version: 1\nkind: language\nlanguage: en\n\n[Menu]\na:\n  src: One\n  tr: Uno\nb:\n  src: Two\n  tr: Dos\n",
        )
        .expect("distinct equal-length sources are valid");
        assert_eq!(document.messages.len(), 2);
    }

    #[test]
    fn formatter_is_canonical_and_preserves_message_order() {
        let source = "version: 1\nkind: language\nlanguage: cn\nsourcelanguage: en\n\n// keep this note\n[Z]\nz:\n  src: \"A\\\\B\"\n  tr: \"甲\\\\乙\"\n\n[A]\na:\n  src: A\n  tr: A\n";
        let formatted = format_source(source).expect("format");
        assert!(formatted.starts_with("version: 1\nkind: language\nlanguage: zh-CN\n"));
        assert!(formatted.contains("// keep this note\n"));
        assert!(formatted.contains("  src: \"A\\\\B\"\n"));
        assert!(formatted.find("[Z]").expect("Z") < formatted.find("[A]").expect("A"));
        assert_eq!(format_source(&formatted).expect("idempotent"), formatted);
    }

    #[test]
    fn formatter_handles_variables_and_plural_forms() {
        let source = "version: 1\nkind: variables\n\nvalues:\n  scale: real = 0.5\n  mesh: resource = project:/assets/mesh.vtu\n";
        let formatted = format_source(source).expect("variables");
        assert_eq!(
            formatted,
            "version: 1\nkind: variables\n\nvalues:\n  scale: real = 0.5\n  mesh: resource = project:/assets/mesh.vtu\n"
        );

        let language = "version: 1\nkind: language\nlanguage: zh-CN\n\n[Menu]\nitems:\n  src: \"%n item\"\n  numerus: true\n  tr: [\"%n 项\", \"%n 项目\"]\n";
        let formatted_language = format_source(language).expect("plural");
        assert!(formatted_language.contains("  tr: [\"%n 项\", \"%n 项目\"]\n"));
        assert_eq!(
            format_source(&formatted_language).expect("plural idempotent"),
            formatted_language
        );

        let bracket = "version: 1\nkind: language\nlanguage: en\n\n[Menu]\nsyntax:\n  src: Brackets\n  tr: \"[literal]\"\n";
        let formatted_bracket = format_source(bracket).expect("bracket");
        assert!(formatted_bracket.contains("  tr: \"[literal]\"\n"));
        assert_eq!(
            format_source(&formatted_bracket).expect("bracket idempotent"),
            formatted_bracket
        );
    }

    #[test]
    fn validator_rejects_tabs_and_finished_placeholder_mismatch() {
        let tab_error = parse("version: 1\nkind: variables\n\nvalues:\n\twidth: int = 1\n")
            .expect_err("tabs are invalid");
        assert_eq!(tab_error.diagnostics[0].code, "pa.tab_indentation");

        let placeholder_error = parse(
            "version: 1\nkind: language\nlanguage: en\n\n[Menu]\ncount:\n  src: \"%n item\"\n  tr: item\n",
        )
        .expect_err("placeholder mismatch");
        assert!(
            placeholder_error
                .diagnostics
                .iter()
                .any(|item| item.code == "pa.placeholder_mismatch")
        );
    }

    #[test]
    fn classifies_source_kind_from_the_header_block() {
        assert_eq!(
            source_kind("// note\nkind: language\n\nignored: later\n"),
            Some(SourceKind::Language)
        );
        assert_eq!(source_kind("kind: theme\n"), Some(SourceKind::Theme));
        assert_eq!(
            source_kind("kind: variables\n"),
            Some(SourceKind::Variables)
        );
        assert_eq!(source_kind("kind: fsm\n"), Some(SourceKind::Fsm));
        assert_eq!(source_kind("kind: other\n"), None);
        assert_eq!(source_kind("\nkind: language\n"), None);
        assert_eq!(source_kind("version: 1\n"), None);
    }

    #[test]
    fn reports_duplicate_headers_fields_and_syntax() {
        let duplicates = parse(
            "version: 1\nkind: language\ncatalog: Alpha\ncatalog: Beta\n\n[Menu]\na:\n  src: One\n  src: Two\n  tr: Uno\n",
        )
        .expect_err("repeated catalog and src");
        assert!(
            duplicates
                .diagnostics
                .iter()
                .any(|item| item.code == "pa.duplicate_header")
        );
        assert!(
            duplicates
                .diagnostics
                .iter()
                .any(|item| item.code == "pa.duplicate_field")
        );
        let rendered = duplicates.to_string();
        assert!(rendered.contains("pa.duplicate_header at "));
        assert!(rendered.contains('\n'));

        let syntax = parse("version: nope\n").expect_err("pest syntax");
        assert_eq!(syntax.diagnostics[0].code, "pa.syntax");
        assert!(syntax.to_string().starts_with("pa.syntax at "));

        let orphan = parse("version: 1\nkind: language\n\n  src: Orphan\n").expect_err("orphan");
        assert!(
            orphan
                .diagnostics
                .iter()
                .any(|item| item.code == "pa.field_without_message")
        );

        let orphan_fields = parse(
            "version: 1\nkind: language\nlanguage: en\n\n  oldsrc: Old\n  tr: Uno\n  st: finished\n  comment: note\n  extra: extra\n  numerus: true\n",
        )
        .expect_err("orphan metadata");
        assert_eq!(
            orphan_fields
                .diagnostics
                .iter()
                .filter(|item| item.code == "pa.field_without_message")
                .count(),
            6
        );

        let empty_id = parse(
            "version: 1\nkind: language\nlanguage: en\n\n[Menu]\n\"\":\n  src: Empty key\n  tr: Empty key\n",
        )
        .expect_err("empty message id");
        assert!(
            empty_id
                .diagnostics
                .iter()
                .any(|item| item.code == "pa.empty_id")
        );

        let fsm = parse("version: 1\nkind: fsm\n").expect_err("FSM parser required");
        assert_eq!(fsm.diagnostics[0].code, "pa.unsupported_kind");

        let huge = "a".repeat(1_048_577);
        let too_large = parse(&huge).expect_err("source limit");
        assert_eq!(too_large.diagnostics[0].code, "pa.source_too_large");
    }

    #[test]
    fn formats_theme_kind_and_vanished_status() {
        let theme =
            format_source("version: 1\nkind: theme\n\nvalues:\n  gap: int = 4\n").expect("theme");
        assert!(theme.contains("kind: theme\n"));
        assert!(theme.contains("  gap: int = 4\n"));

        let vanished = format_source(
            "version: 1\nkind: language\nlanguage: en\n\n[Menu]\nold:\n  src: Old\n  tr: Old\n  st: vanished\n",
        )
        .expect("vanished");
        assert!(vanished.contains("  st: vanished\n"));
    }
}
