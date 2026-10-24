//! 数据文档的规范格式；声明顺序与 TS 的索引顺序相互独立。

use std::collections::BTreeSet;

use crate::{Diagnostics, Document, Kind, Message, Status, ValueType, parse};

/// 解析并校验源码后，输出规范格式；错误原样返回。
pub fn format_source(source: &str) -> Result<String, Diagnostics> {
    let document = parse(source)?;
    Ok(format_document(&document))
}

/// 格式化已校验文档，保留声明顺序和注释；不重新校验手工构造的 AST。
pub fn format_document(document: &Document) -> String {
    let mut lines = vec![
        format!("version: {}", document.version),
        format!("kind: {}", kind_name(document.kind)),
    ];
    if let Some(catalog) = document.catalog.as_deref() {
        lines.push(format!("catalog: {catalog}"));
    }
    if let Some(language) = document.language.as_deref() {
        lines.push(format!("language: {language}"));
    }
    if document.kind == Kind::Language {
        lines.push(format!("sourcelanguage: {}", document.source_language));
    }
    lines.push(String::new());
    for comment in &document.comments {
        lines.push(comment.clone());
    }
    if !document.comments.is_empty() {
        lines.push(String::new());
    }

    match document.kind {
        Kind::Language => format_messages(document, &mut lines),
        Kind::Theme | Kind::Variables => format_values(document, &mut lines),
    }

    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.push(String::new());
    lines.join("\n")
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Language => "language",
        Kind::Theme => "theme",
        Kind::Variables => "variables",
    }
}

fn format_messages(document: &Document, lines: &mut Vec<String>) {
    let messages = ordered_messages(document);
    let mut context: Option<&str> = None;
    for message in messages {
        if context != Some(message.context.as_str()) {
            ensure_blank(lines);
            lines.push(format!("[{}]", message.context));
            context = Some(message.context.as_str());
        } else {
            ensure_blank(lines);
        }
        lines.push(format!("{}:", render_key(&message.id)));
        lines.push(format!("  src: {}", render_scalar(&message.source)));
        if let Some(old_source) = message.old_source.as_deref() {
            lines.push(format!("  oldsrc: {}", render_scalar(old_source)));
        }
        if message.numerus {
            lines.push("  numerus: true".to_owned());
        }
        let translation = document
            .language
            .as_ref()
            .and_then(|locale| message.translations.get(locale))
            .or_else(|| message.translations.values().next());
        if let Some(translation) = translation {
            if message.numerus {
                let forms = translation
                    .forms
                    .iter()
                    .map(|form| render_quoted(form))
                    .collect::<Vec<_>>();
                if forms.is_empty() {
                    lines.push("  tr: \"\"".to_owned());
                } else {
                    lines.push(format!("  tr: [{}]", forms.join(", ")));
                }
            } else {
                lines.push(format!(
                    "  tr: {}",
                    translation
                        .forms
                        .first()
                        .map_or_else(|| "\"\"".to_owned(), |form| render_translation_scalar(form))
                ));
            }
        }
        if let Some(status) = message.status {
            lines.push(format!("  st: {}", status_name(status)));
        }
        if let Some(comment) = message.comment.as_deref() {
            lines.push(format!("  comment: {}", render_scalar(comment)));
        }
        if let Some(extra) = message.extra.as_deref() {
            lines.push(format!("  extra: {}", render_scalar(extra)));
        }
    }
}

fn format_values(document: &Document, lines: &mut Vec<String>) {
    lines.push("values:".to_owned());
    for value in &document.values {
        lines.push(format!(
            "  {}: {} = {}",
            value.name,
            value_type_name(&value.value_type),
            value.expression
        ));
    }
}

fn ordered_messages(document: &Document) -> Vec<&Message> {
    let mut result = Vec::with_capacity(document.messages.len());
    let mut seen = BTreeSet::new();
    for key in &document.message_order {
        if let Some(message) = document.messages.get(key)
            && seen.insert(key)
        {
            result.push(message);
        }
    }
    for (key, message) in &document.messages {
        if seen.insert(key) {
            result.push(message);
        }
    }
    result
}

fn ensure_blank(lines: &mut Vec<String>) {
    if !lines.last().is_some_and(String::is_empty) {
        lines.push(String::new());
    }
}

fn render_key(key: &str) -> String {
    if is_bare_key(key) {
        key.to_owned()
    } else {
        render_quoted(key)
    }
}

fn is_bare_key(value: &str) -> bool {
    value.split('.').all(|part| {
        let mut chars = part.chars();
        chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
            && chars.all(|character| {
                character.is_ascii_alphanumeric() || character == '_' || character == '-'
            })
    })
}

fn render_scalar(value: &str) -> String {
    if value.is_empty()
        || value.starts_with([' ', '\t'])
        || value.ends_with([' ', '\t'])
        || value.contains(['\\', '"', '\n', '\r', '\t'])
    {
        render_quoted(value)
    } else {
        value.to_owned()
    }
}

fn render_translation_scalar(value: &str) -> String {
    if value.starts_with('[') {
        render_quoted(value)
    } else {
        render_scalar(value)
    }
}

fn render_quoted(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Finished => "finished",
        Status::Unfinished => "unfinished",
        Status::Vanished => "vanished",
    }
}

fn value_type_name(value_type: &ValueType) -> &'static str {
    match value_type {
        ValueType::Bool => "bool",
        ValueType::Int => "int",
        ValueType::Real => "real",
        ValueType::String => "string",
        ValueType::Resource => "resource",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::format_document;
    use crate::{Document, Kind, Message, Translation};

    #[test]
    fn unmatched_locale_uses_first_translation_and_formats_empty_scalar() {
        let message = Message {
            context: "App".to_owned(),
            id: "empty".to_owned(),
            source: "Source".to_owned(),
            old_source: None,
            translations: BTreeMap::from([("en".to_owned(), Translation { forms: vec![] })]),
            status: None,
            comment: None,
            extra: None,
            numerus: false,
        };
        let document = Document {
            version: 1,
            kind: Kind::Language,
            catalog: None,
            language: Some("fr".to_owned()),
            source_language: "en".to_owned(),
            messages: BTreeMap::from([("App\u{1f}empty".to_owned(), message)]),
            message_order: vec!["App\u{1f}empty".to_owned()],
            comments: vec![],
            values: vec![],
        };

        assert!(format_document(&document).contains("  tr: \"\"\n"));
    }

    #[test]
    fn missing_message_order_entries_are_appended_once_in_key_order() {
        let message = |id: &str, source: &str| Message {
            context: "App".to_owned(),
            id: id.to_owned(),
            source: source.to_owned(),
            old_source: None,
            translations: BTreeMap::new(),
            status: None,
            comment: None,
            extra: None,
            numerus: false,
        };
        let declared_key = "App\u{1f}z-declared".to_owned();
        let first_fallback_key = "App\u{1f}a-fallback".to_owned();
        let second_fallback_key = "App\u{1f}m-fallback".to_owned();
        let document = Document {
            version: 1,
            kind: Kind::Language,
            catalog: Some("app".to_owned()),
            language: Some("en".to_owned()),
            source_language: "en".to_owned(),
            messages: BTreeMap::from([
                (
                    second_fallback_key.clone(),
                    message("m-fallback", "Fallback M"),
                ),
                (declared_key.clone(), message("z-declared", "Declared")),
                (
                    first_fallback_key.clone(),
                    message("a-fallback", "Fallback A"),
                ),
            ]),
            message_order: vec![declared_key.clone(), declared_key, "missing".to_owned()],
            comments: vec!["// generated".to_owned()],
            values: vec![],
        };

        let formatted = format_document(&document);
        let Some(declared) = formatted.find("  src: Declared\n") else {
            panic!("declared source was not emitted: {formatted}");
        };
        let Some(first_fallback) = formatted.find("  src: Fallback A\n") else {
            panic!("first fallback source was not emitted: {formatted}");
        };
        let Some(second_fallback) = formatted.find("  src: Fallback M\n") else {
            panic!("second fallback source was not emitted: {formatted}");
        };
        assert!(
            declared < first_fallback && first_fallback < second_fallback,
            "{formatted}"
        );
        assert_eq!(formatted.matches("z-declared:\n").count(), 1);
        assert_eq!(formatted.matches("a-fallback:\n").count(), 1);
        assert_eq!(formatted.matches("m-fallback:\n").count(), 1);
        assert!(formatted.starts_with(
            "version: 1\nkind: language\ncatalog: app\nlanguage: en\nsourcelanguage: en\n\n// generated\n"
        ));
    }
}
