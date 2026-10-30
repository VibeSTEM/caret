//! Pest 节点到数据文档的转换；构造后统一执行语义校验。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::collections::{BTreeMap, HashSet};

use pest::Parser as PestParser;
use pest::iterators::Pair;

use crate::diagnostic::{SourcePositions, push_diagnostic};
use crate::syntax::{MAX_DECLARATIONS, MAX_SOURCE_BYTES, PaParser, Rule, pest_diagnostic};
use crate::validation::{DocumentLocations, MessageLocations, normalize_locale, validate_document};
use crate::{
    Diagnostic, Diagnostics, Document, Kind, Message, Status, Translation, ValueType, Variable,
};

/// 解析并校验 language / theme / variables；FSM 文档应使用专用解析入口。
/// 语法、容量或语义错误返回诊断。
pub fn parse(source: &str) -> Result<Document, Diagnostics> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Diagnostics::one(
            "pa.source_too_large",
            format!("source exceeds {MAX_SOURCE_BYTES} bytes"),
            0,
            source,
        ));
    }
    if let Some(offset) = source.find('\t') {
        return Err(Diagnostics::one(
            "pa.tab_indentation",
            "tabs are not allowed; use two spaces for indentation",
            offset,
            source,
        ));
    }

    let positions = SourcePositions::new(source);
    let source = &positions;
    let Some(document_pair) = PaParser::parse(Rule::document, source)
        .map_err(|error| pest_diagnostic(error, source))?
        .next()
    else {
        return Err(Diagnostics::one(
            "pa.syntax",
            "document is empty",
            0,
            source,
        ));
    };

    let mut result = Document {
        version: 0,
        kind: Kind::Variables,
        catalog: None,
        language: None,
        source_language: "en".to_owned(),
        messages: BTreeMap::new(),
        message_order: Vec::new(),
        comments: Vec::new(),
        values: Vec::new(),
    };
    let mut seen_version = false;
    let mut seen_kind = false;
    let mut seen_language = false;
    let mut seen_source_language = false;
    let mut section: Option<String> = None;
    let mut current_context = "Caret".to_owned();
    let mut current_message: Option<(String, Message, usize)> = None;
    let mut current_message_source_seen = false;
    let mut current_message_numerus_seen = false;
    let mut declaration_count = 0usize;
    let mut seen_contexts = HashSet::new();
    let mut declaration_limit_exceeded = false;
    let mut diagnostics = Vec::new();
    let mut locations = DocumentLocations::default();

    for line in document_pair.into_inner() {
        let Some(body) = line.into_inner().next() else {
            continue;
        };
        let body_offset = body.as_span().start();
        if declaration_limit_exceeded {
            continue;
        }
        match body.as_rule() {
            Rule::blank => {}
            Rule::comment_line => {
                let comment = body.as_str().trim();
                if !comment.is_empty() {
                    result.comments.push(comment.to_owned());
                }
            }
            Rule::version => {
                if seen_version {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.duplicate_header",
                        "version is repeated",
                        body_offset,
                        source,
                    );
                } else {
                    seen_version = true;
                    locations.version = Some(body_offset);
                    result.version = find_text(body, Rule::integer)
                        .and_then(|value| value.parse::<u32>().ok())
                        .unwrap_or_default();
                }
            }
            Rule::kind => {
                if seen_kind {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.duplicate_header",
                        "kind is repeated",
                        body_offset,
                        source,
                    );
                } else {
                    seen_kind = true;
                    if let Some(kind) = find_text(body, Rule::kind_name) {
                        match kind.as_str() {
                            "language" => result.kind = Kind::Language,
                            "theme" => result.kind = Kind::Theme,
                            "variables" => result.kind = Kind::Variables,
                            _ => push_diagnostic(
                                &mut diagnostics,
                                "pa.unsupported_kind",
                                format!("kind '{kind}' is not supported by this parser"),
                                body_offset,
                                source,
                            ),
                        }
                    }
                }
            }
            Rule::catalog => {
                locations.catalog = Some(body_offset);
                set_header(
                    &mut result.catalog,
                    body,
                    "catalog",
                    source,
                    &mut diagnostics,
                    Rule::catalog_name,
                );
            }
            Rule::language => {
                if seen_language {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.duplicate_header",
                        "language is repeated",
                        body_offset,
                        source,
                    );
                } else {
                    seen_language = true;
                    locations.language = Some(body_offset);
                    result.language =
                        find_text(body, Rule::locale_name).map(|value| normalize_locale(&value));
                }
            }
            Rule::sourcelanguage => {
                if seen_source_language {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.duplicate_header",
                        "sourcelanguage is repeated",
                        body_offset,
                        source,
                    );
                } else {
                    seen_source_language = true;
                    locations.source_language = Some(body_offset);
                    result.source_language = match find_text(body, Rule::locale_name) {
                        Some(value) => normalize_locale(&value),
                        None => "en".to_owned(),
                    };
                }
            }
            Rule::context => {
                flush_message(&mut current_message, &mut result, &mut diagnostics, source);
                current_context = match find_text(body, Rule::context_name) {
                    Some(value) => value.trim().to_owned(),
                    None => "Caret".to_owned(),
                };
                if !seen_contexts.insert(current_context.clone()) {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.duplicate_context",
                        format!("context '{}' is repeated", current_context),
                        body_offset,
                        source,
                    );
                }
                section = None;
            }
            Rule::section => {
                section = find_text(body, Rule::section_name);
            }
            Rule::message => {
                flush_message(&mut current_message, &mut result, &mut diagnostics, source);
                declaration_limit_exceeded = count_declaration(
                    &mut declaration_count,
                    &mut diagnostics,
                    body_offset,
                    source,
                );
                if declaration_limit_exceeded {
                    continue;
                }
                seen_contexts.insert(current_context.clone());
                let id = find_message_key(body, source, &mut diagnostics).unwrap_or_default();
                if id.is_empty() {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.empty_id",
                        "language entry requires an id",
                        body_offset,
                        source,
                    );
                    continue;
                }
                let key = message_key(&current_context, &id);
                locations.messages.insert(
                    key.clone(),
                    MessageLocations {
                        declaration: body_offset,
                        ..MessageLocations::default()
                    },
                );
                current_message = Some((
                    key,
                    Message {
                        context: current_context.clone(),
                        id,
                        source: String::new(),
                        old_source: None,
                        translations: BTreeMap::new(),
                        status: None,
                        comment: None,
                        extra: None,
                        numerus: false,
                    },
                    body_offset,
                ));
                current_message_numerus_seen = false;
                current_message_source_seen = false;
            }
            Rule::src => {
                if let Some((_, message, _)) = current_message.as_mut() {
                    let value = field_value(body, source, &mut diagnostics);
                    if !current_message_source_seen {
                        current_message_source_seen = true;
                        message.source = value;
                    } else {
                        duplicate_field(&mut diagnostics, "src", body_offset, source);
                    }
                } else {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.field_without_message",
                        "src must follow a message id",
                        body_offset,
                        source,
                    );
                }
            }
            Rule::oldsrc => {
                if let Some((_, message, _)) = current_message.as_mut() {
                    let value = field_value(body, source, &mut diagnostics);
                    if message.old_source.is_none() {
                        message.old_source = Some(value);
                    } else {
                        duplicate_field(&mut diagnostics, "oldsrc", body_offset, source);
                    }
                } else {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.field_without_message",
                        "oldsrc must follow a message id",
                        body_offset,
                        source,
                    );
                }
            }
            Rule::translation => {
                if let Some((key, message, _)) = current_message.as_mut() {
                    locations
                        .messages
                        .entry(key.clone())
                        .or_default()
                        .translation = Some(body_offset);
                    let forms = translation_value(body, source, &mut diagnostics);
                    let locale = result
                        .language
                        .clone()
                        .unwrap_or_else(|| result.source_language.clone());
                    if message
                        .translations
                        .insert(locale, Translation { forms })
                        .is_some()
                    {
                        duplicate_field(&mut diagnostics, "tr", body_offset, source);
                    }
                } else {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.field_without_message",
                        "tr must follow a message id",
                        body_offset,
                        source,
                    );
                }
            }
            Rule::status => {
                if let Some((_, message, _)) = current_message.as_mut() {
                    if message.status.is_some() {
                        duplicate_field(&mut diagnostics, "st", body_offset, source);
                    }
                    if let Some(value) = find_text(body, Rule::status_name) {
                        message.status = match value.as_str() {
                            "finished" => Some(Status::Finished),
                            "unfinished" => Some(Status::Unfinished),
                            "vanished" => Some(Status::Vanished),
                            _ => None,
                        };
                    }
                } else {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.field_without_message",
                        "st must follow a message id",
                        body_offset,
                        source,
                    );
                }
            }
            Rule::comment_field => {
                if let Some((_, message, _)) = current_message.as_mut() {
                    if message.comment.is_some() {
                        duplicate_field(&mut diagnostics, "comment", body_offset, source);
                    }
                    message.comment = Some(field_value(body, source, &mut diagnostics));
                } else {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.field_without_message",
                        "comment must follow a message id",
                        body_offset,
                        source,
                    );
                }
            }
            Rule::extra_field => {
                if let Some((_, message, _)) = current_message.as_mut() {
                    if message.extra.is_some() {
                        duplicate_field(&mut diagnostics, "extra", body_offset, source);
                    }
                    message.extra = Some(field_value(body, source, &mut diagnostics));
                } else {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.field_without_message",
                        "extra must follow a message id",
                        body_offset,
                        source,
                    );
                }
            }
            Rule::numerus => {
                if let Some((_, message, _)) = current_message.as_mut() {
                    if current_message_numerus_seen {
                        duplicate_field(&mut diagnostics, "numerus", body_offset, source);
                    } else if let Some(value) = find_text(body, Rule::boolean) {
                        current_message_numerus_seen = true;
                        message.numerus = value == "true";
                    }
                } else {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.field_without_message",
                        "numerus must follow a message id",
                        body_offset,
                        source,
                    );
                }
            }
            Rule::entry => {
                declaration_limit_exceeded = count_declaration(
                    &mut declaration_count,
                    &mut diagnostics,
                    body_offset,
                    source,
                );
                if declaration_limit_exceeded {
                    continue;
                }
                let mut parts = body.into_inner();
                let Some(key) = parts.next().map(|pair| pair.as_str().to_owned()) else {
                    continue;
                };
                let Some(value_pair) = parts.next() else {
                    continue;
                };
                if section.as_deref() != Some("values") {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.entry_outside_section",
                        "value entry must be inside values",
                        body_offset,
                        source,
                    );
                    continue;
                }
                if key.contains('_') {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.invalid_key",
                        format!("value key '{key}' must use kebab-case"),
                        body_offset,
                        source,
                    );
                    continue;
                }
                let (value_type, expression, expression_start, expression_end) =
                    typed_entry(value_pair, source, &mut diagnostics);
                if result.values.iter().any(|item| item.name == key) {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.duplicate_key",
                        format!("value key '{key}' is repeated"),
                        body_offset,
                        source,
                    );
                } else if let Some(value_type) = value_type {
                    result.values.push(Variable {
                        name: key,
                        value_type,
                        expression,
                        expression_start,
                        expression_end,
                    });
                }
            }
            _ => {}
        }
    }
    flush_message(&mut current_message, &mut result, &mut diagnostics, source);

    validate_document(
        &result,
        seen_version,
        seen_kind,
        source,
        &locations,
        &mut diagnostics,
    );
    if diagnostics.is_empty() {
        Ok(result)
    } else {
        Err(Diagnostics { diagnostics })
    }
}

fn count_declaration(
    count: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    offset: usize,
    source: &SourcePositions<'_>,
) -> bool {
    *count += 1;
    if *count <= MAX_DECLARATIONS {
        return false;
    }

    push_diagnostic(
        diagnostics,
        "pa.too_many_declarations",
        format!("more than {MAX_DECLARATIONS} declarations"),
        offset,
        source,
    );
    true
}

fn duplicate_field(
    diagnostics: &mut Vec<Diagnostic>,
    field: &str,
    offset: usize,
    source: &SourcePositions<'_>,
) {
    push_diagnostic(
        diagnostics,
        "pa.duplicate_field",
        format!("{field} is repeated"),
        offset,
        source,
    );
}

fn set_header(
    target: &mut Option<String>,
    body: Pair<'_, Rule>,
    name: &str,
    source: &SourcePositions<'_>,
    diagnostics: &mut Vec<Diagnostic>,
    value_rule: Rule,
) {
    if target.is_some() {
        push_diagnostic(
            diagnostics,
            "pa.duplicate_header",
            format!("{name} is repeated"),
            body.as_span().start(),
            source,
        );
        return;
    }
    *target = find_text(body, value_rule);
}

fn flush_message(
    current: &mut Option<(String, Message, usize)>,
    result: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
    source: &SourcePositions<'_>,
) {
    let Some((key, message, message_offset)) = current.take() else {
        return;
    };
    if message.source.is_empty() {
        push_diagnostic(
            diagnostics,
            "pa.missing_src",
            format!("message '{}' requires src", message.id),
            message_offset,
            source,
        );
    }
    if result.messages.insert(key.clone(), message).is_some() {
        push_diagnostic(
            diagnostics,
            "pa.duplicate_message",
            "context and id are repeated",
            message_offset,
            source,
        );
    } else {
        result.message_order.push(key);
    }
}

fn find_text(body: Pair<'_, Rule>, rule: Rule) -> Option<String> {
    if body.as_rule() == rule {
        return Some(body.as_str().to_owned());
    }
    body.into_inner().find_map(|pair| find_text(pair, rule))
}

fn find_message_key(
    body: Pair<'_, Rule>,
    source: &SourcePositions<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    find_pair(body.clone(), Rule::bare_key)
        .or_else(|| find_pair(body, Rule::quoted))
        .map(|pair| decode_pair_value(pair, source, diagnostics))
}

fn field_value(
    body: Pair<'_, Rule>,
    source: &SourcePositions<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    find_pair(body.clone(), Rule::quoted)
        .or_else(|| find_pair(body, Rule::scalar))
        .map(|pair| decode_pair_value(pair, source, diagnostics))
        .unwrap_or_default()
}

fn translation_value(
    body: Pair<'_, Rule>,
    source: &SourcePositions<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<String> {
    if let Some(array) = find_pair(body.clone(), Rule::array) {
        return array
            .into_inner()
            .filter(|pair| pair.as_rule() == Rule::quoted)
            .map(|pair| decode_pair_value(pair, source, diagnostics))
            .collect();
    }
    vec![field_value(body, source, diagnostics)]
}

fn find_pair(body: Pair<'_, Rule>, rule: Rule) -> Option<Pair<'_, Rule>> {
    if body.as_rule() == rule {
        Some(body)
    } else {
        body.into_inner().find_map(|pair| find_pair(pair, rule))
    }
}

fn message_key(context: &str, id: &str) -> String {
    format!("{context}\u{1f}{id}")
}

fn typed_entry(
    value_pair: Pair<'_, Rule>,
    source: &SourcePositions<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Option<ValueType>, String, usize, usize) {
    if value_pair.as_rule() != Rule::typed_value {
        push_diagnostic(
            diagnostics,
            "pa.value_type",
            "values require type = expression",
            0,
            source,
        );
        return (None, String::new(), 0, 0);
    }
    let value_type =
        find_text(value_pair.clone(), Rule::type_name).and_then(|value| parse_value_type(&value));
    let expression = find_pair(value_pair, Rule::scalar);
    let (expression, start, end) = expression.map_or_else(
        || (String::new(), 0, 0),
        |pair| {
            let span = pair.as_span();
            let expression = trim_unescaped_trailing_whitespace(pair.as_str());
            (
                expression.to_owned(),
                span.start(),
                span.start() + expression.len(),
            )
        },
    );
    (value_type, expression, start, end)
}

fn trim_unescaped_trailing_whitespace(value: &str) -> &str {
    let bytes = value.as_bytes();
    let mut end = bytes.len();
    while end > 0 && matches!(bytes[end - 1], b' ' | b'\t') {
        let mut slash_count = 0;
        let mut cursor = end - 1;
        while cursor > 0 && bytes[cursor - 1] == b'\\' {
            slash_count += 1;
            cursor -= 1;
        }
        if slash_count % 2 == 1 {
            break;
        }
        end -= 1;
    }
    &value[..end]
}

fn decode_pair_value(
    pair: Pair<'_, Rule>,
    source: &SourcePositions<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    let span = pair.as_span();
    let raw = pair.as_str();
    if pair.as_rule() == Rule::quoted {
        let value = raw
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(raw);
        decode_scalar(value, source, span.start() + 1, diagnostics)
    } else {
        decode_scalar(
            trim_unescaped_trailing_whitespace(raw),
            source,
            span.start(),
            diagnostics,
        )
    }
}

fn decode_scalar(
    raw: &str,
    source: &SourcePositions<'_>,
    source_offset: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut escape_offset = None;
    for (relative_offset, character) in raw.char_indices() {
        if let Some(escape_start) = escape_offset {
            match character {
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                ':' | '=' | ' ' | '\\' | '"' => result.push(character),
                _ => {
                    push_diagnostic(
                        diagnostics,
                        "pa.invalid_escape",
                        format!("unsupported escape \\{character}"),
                        source_offset + escape_start,
                        source,
                    );
                    result.push(character);
                }
            }
            escape_offset = None;
        } else if character == '\\' {
            escape_offset = Some(relative_offset);
        } else {
            result.push(character);
        }
    }
    if let Some(escape_start) = escape_offset {
        push_diagnostic(
            diagnostics,
            "pa.invalid_escape",
            "trailing escape",
            source_offset + escape_start,
            source,
        );
    }
    result
}

fn parse_value_type(value: &str) -> Option<ValueType> {
    match value {
        "bool" => Some(ValueType::Bool),
        "int" => Some(ValueType::Int),
        "real" => Some(ValueType::Real),
        "string" => Some(ValueType::String),
        "resource" => Some(ValueType::Resource),
        _ => None,
    }
}
