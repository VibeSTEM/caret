//! 数据文档语义、locale 和占位符规则；不依赖 Pest 或 XML。

use std::collections::BTreeMap;

use crate::diagnostic::push_diagnostic;
use crate::{Diagnostic, Document, Kind, Status};

#[derive(Default)]
pub(crate) struct DocumentLocations {
    pub version: Option<usize>,
    pub catalog: Option<usize>,
    pub language: Option<usize>,
    pub source_language: Option<usize>,
    pub messages: BTreeMap<String, MessageLocations>,
}

#[derive(Default)]
pub(crate) struct MessageLocations {
    pub declaration: usize,
    pub translation: Option<usize>,
}

pub(crate) fn normalize_locale(locale: &str) -> String {
    if locale.eq_ignore_ascii_case("cn")
        || locale.eq_ignore_ascii_case("zh-cn")
        || locale.eq_ignore_ascii_case("zh_cn")
    {
        "zh-CN".to_owned()
    } else {
        locale.to_owned()
    }
}

pub(crate) fn valid_locale(locale: &str) -> bool {
    let mut chars = locale.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    let mut previous_separator = false;
    for character in chars {
        if character.is_ascii_alphanumeric() {
            previous_separator = false;
        } else if character == '-' || character == '_' {
            if previous_separator {
                return false;
            }
            previous_separator = true;
        } else {
            return false;
        }
    }
    !previous_separator
}

pub(crate) fn validate_document(
    document: &Document,
    seen_version: bool,
    seen_kind: bool,
    source: &str,
    locations: &DocumentLocations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !seen_version {
        push_diagnostic(
            diagnostics,
            "pa.missing_version",
            "version is required",
            source.len(),
            source,
        );
    } else if document.version != 1 {
        push_diagnostic(
            diagnostics,
            "pa.unsupported_version",
            "only version 1 is supported",
            locations.version.unwrap_or(source.len()),
            source,
        );
    }
    if !seen_kind {
        push_diagnostic(
            diagnostics,
            "pa.missing_kind",
            "kind is required",
            source.len(),
            source,
        );
    }
    if let Some(language) = document.language.as_deref()
        && !valid_locale(language)
    {
        push_diagnostic(
            diagnostics,
            "pa.invalid_locale",
            format!("invalid language locale '{language}'"),
            locations.language.unwrap_or(source.len()),
            source,
        );
    }
    if !valid_locale(&document.source_language) {
        push_diagnostic(
            diagnostics,
            "pa.invalid_locale",
            format!("invalid source locale '{}'", document.source_language),
            locations.source_language.unwrap_or(source.len()),
            source,
        );
    }
    match document.kind {
        Kind::Language => {
            if document.language.is_none() {
                push_diagnostic(
                    diagnostics,
                    "pa.missing_language",
                    "language requires a target locale",
                    source.len(),
                    source,
                );
            }
            if document.messages.is_empty() {
                push_diagnostic(
                    diagnostics,
                    "pa.empty_language",
                    "language catalog has no messages",
                    source.len(),
                    source,
                );
            }
            if !document.values.is_empty() {
                push_diagnostic(
                    diagnostics,
                    "pa.unexpected_values",
                    "values are only valid for theme or variables",
                    source.len(),
                    source,
                );
            }
        }
        Kind::Theme | Kind::Variables => {
            if document.catalog.is_some()
                || document.language.is_some()
                || !document.messages.is_empty()
            {
                push_diagnostic(
                    diagnostics,
                    "pa.unexpected_language_field",
                    "language fields are only valid for language",
                    locations
                        .language
                        .or(locations.catalog)
                        .or(locations
                            .messages
                            .values()
                            .next()
                            .map(|item| item.declaration))
                        .unwrap_or(source.len()),
                    source,
                );
            }
            if document.kind == Kind::Variables && document.values.is_empty() {
                push_diagnostic(
                    diagnostics,
                    "pa.empty_values",
                    "variables requires values",
                    source.len(),
                    source,
                );
            }
        }
    }
    validate_messages(document, source, locations, diagnostics);
}

fn validate_messages(
    document: &Document,
    source: &str,
    locations: &DocumentLocations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut sources: BTreeMap<(String, String), String> = BTreeMap::new();
    for message in document.messages.values() {
        let message_locations = locations
            .messages
            .get(&format!("{}\u{1f}{}", message.context, message.id));
        let message_offset = message_locations.map_or(source.len(), |item| item.declaration);
        let source_key = (message.context.clone(), message.source.clone());
        if let Some(previous_id) = sources.insert(source_key, message.id.clone())
            && previous_id != message.id
        {
            push_diagnostic(
                diagnostics,
                "pa.duplicate_source",
                format!(
                    "context '{}' contains messages '{}' and '{}' with the same source text",
                    message.context, previous_id, message.id
                ),
                message_offset,
                source,
            );
        }
        if message.numerus
            && message
                .translations
                .values()
                .any(|translation| translation.forms.is_empty())
        {
            push_diagnostic(
                diagnostics,
                "pa.empty_plural",
                format!("message '{}' has no plural forms", message.id),
                message_offset,
                source,
            );
        }
        if !message.numerus
            && message
                .translations
                .values()
                .any(|translation| translation.forms.len() > 1)
        {
            push_diagnostic(
                diagnostics,
                "pa.unexpected_plural",
                format!("message '{}' is not numerus", message.id),
                message_offset,
                source,
            );
        }
        if message.status != Some(Status::Unfinished) && message.status != Some(Status::Vanished) {
            for translation in message.translations.values() {
                if translation
                    .forms
                    .iter()
                    .any(|form| placeholders(&message.source) != placeholders(form))
                {
                    push_diagnostic(
                        diagnostics,
                        "pa.placeholder_mismatch",
                        format!("message '{}' has different placeholders", message.id),
                        message_locations
                            .and_then(|item| item.translation)
                            .unwrap_or(message_offset),
                        source,
                    );
                    break;
                }
            }
        }
    }
}

pub(crate) fn placeholders(value: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    let mut result = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'%' {
            if bytes[index + 1] == b'n' {
                result.push("%n".to_owned());
                index += 2;
                continue;
            }
            if bytes[index + 1].is_ascii_digit() {
                let start = index;
                index += 1;
                while index < bytes.len() && bytes[index].is_ascii_digit() {
                    index += 1;
                }
                result.push(value[start..index].to_owned());
                continue;
            }
        }
        index += 1;
    }
    result.sort();
    result
}
