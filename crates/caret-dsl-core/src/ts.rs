//! 确定性 Qt TS 输出；公共 AST 可手工修改，输出前复核目标 locale 与占位符。

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::string::FromUtf8Error;

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

use crate::validation::{normalize_locale, placeholders, valid_locale};
use crate::{Diagnostic, Diagnostics, Document, Kind, Message, Status};

fn writer_error(error: std::io::Error) -> Diagnostics {
    Diagnostics::one("pa.ts_write", error.to_string(), 0, "")
}

fn output_encoding_error(error: FromUtf8Error) -> Diagnostics {
    Diagnostics::one("pa.ts_write", error.to_string(), 0, "")
}

/// 为 language 文档生成目标 locale 的 TS；拒绝非法 locale 与占位符不一致。
pub fn emit_ts(document: &Document, locale: &str) -> Result<String, Diagnostics> {
    if document.kind != Kind::Language {
        return Err(Diagnostics::one(
            "pa.ts_kind",
            "TS output requires kind language",
            0,
            "",
        ));
    }
    let locale = normalize_locale(locale);
    if !valid_locale(&locale) {
        return Err(Diagnostics::one(
            "pa.ts_locale",
            "invalid output locale",
            0,
            "",
        ));
    }
    let mut diagnostics = Vec::new();
    let mut qualified_ids = HashSet::new();
    for message in document.messages.values() {
        let qualified_id = qualified_id(message);
        if !qualified_ids.insert(qualified_id.clone()) {
            diagnostics.push(Diagnostic {
                code: "pa.duplicate_ts_id".to_owned(),
                message: format!("multiple messages produce Qt ID '{qualified_id}'"),
                start_offset: 0,
                end_offset: 0,
                line: 1,
                column: 1,
            });
        }
        if let Some(translation) = message.translations.get(&locale)
            && message.status != Some(Status::Unfinished)
            && message.status != Some(Status::Vanished)
        {
            for form in &translation.forms {
                if placeholders(&message.source) != placeholders(form) {
                    diagnostics.push(Diagnostic {
                        code: "pa.placeholder_mismatch".to_owned(),
                        message: format!("message '{}' has different placeholders", message.id),
                        start_offset: 0,
                        end_offset: 0,
                        line: 1,
                        column: 1,
                    });
                    break;
                }
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(Diagnostics { diagnostics });
    }
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    writer
        .write_event(Event::Decl(BytesDecl::new(
            "1.0",
            Some("UTF-8"),
            Some("yes"),
        )))
        .map_err(writer_error)?;
    let mut ts = BytesStart::new("TS");
    ts.push_attribute(("version", "2.1"));
    ts.push_attribute(("language", locale.replace('-', "_").as_str()));
    ts.push_attribute(("sourcelanguage", document.source_language.as_str()));
    writer.write_event(Event::Start(ts)).map_err(writer_error)?;
    let mut contexts: BTreeMap<&str, Vec<&Message>> = BTreeMap::new();
    for message in document.messages.values() {
        contexts
            .entry(message.context.as_str())
            .or_default()
            .push(message);
    }
    for (context, messages) in contexts {
        writer
            .write_event(Event::Start(BytesStart::new("context")))
            .map_err(writer_error)?;
        write_text_element(&mut writer, "name", context)?;
        for message in messages {
            emit_message(&mut writer, message, &locale)?;
        }
        writer
            .write_event(Event::End(BytesEnd::new("context")))
            .map_err(writer_error)?;
    }
    writer
        .write_event(Event::End(BytesEnd::new("TS")))
        .map_err(writer_error)?;
    String::from_utf8(writer.into_inner()).map_err(output_encoding_error)
}

fn emit_message(
    writer: &mut Writer<Vec<u8>>,
    message: &Message,
    locale: &str,
) -> Result<(), Diagnostics> {
    let mut node = BytesStart::new("message");
    // Qt 的 ID 在整个 TS 中唯一，.pa 的 ID 只在 context 内唯一。
    // 为 qtTrId 限定命名空间，同时保持 context + source 查找语义。
    let qualified_id = qualified_id(message);
    node.push_attribute(("id", qualified_id.as_str()));
    if message.numerus {
        node.push_attribute(("numerus", "yes"));
    }
    writer
        .write_event(Event::Start(node))
        .map_err(writer_error)?;
    if let Some(comment) = &message.comment {
        write_text_element(writer, "comment", comment)?;
    }
    if let Some(extra) = &message.extra {
        write_text_element(writer, "extracomment", extra)?;
    }
    if let Some(old_source) = &message.old_source {
        write_text_element(writer, "oldsource", old_source)?;
    }
    write_text_element(writer, "source", &message.source)?;
    let translation = message.translations.get(locale);
    let mut translation_node = BytesStart::new("translation");
    if message.status == Some(Status::Vanished) {
        translation_node.push_attribute(("type", "vanished"));
    } else if message.status == Some(Status::Unfinished) || translation.is_none() {
        translation_node.push_attribute(("type", "unfinished"));
    }
    writer
        .write_event(Event::Start(translation_node))
        .map_err(writer_error)?;
    let forms = translation
        .map(|value| value.forms.as_slice())
        .unwrap_or(&[]);
    if message.numerus {
        for form in forms {
            write_text_element(writer, "numerusform", form)?;
        }
        if forms.is_empty() {
            write_text_element(writer, "numerusform", &message.source)?;
        }
    } else {
        writer
            .write_event(Event::Text(BytesText::new(
                forms.first().unwrap_or(&message.source),
            )))
            .map_err(writer_error)?;
    }
    writer
        .write_event(Event::End(BytesEnd::new("translation")))
        .map_err(writer_error)?;
    writer
        .write_event(Event::End(BytesEnd::new("message")))
        .map_err(writer_error)?;
    Ok(())
}

fn qualified_id(message: &Message) -> String {
    format!("{}.{}", message.context, message.id)
}

fn write_text_element(
    writer: &mut Writer<Vec<u8>>,
    name: &str,
    value: &str,
) -> Result<(), Diagnostics> {
    writer
        .write_event(Event::Start(BytesStart::new(name)))
        .map_err(writer_error)?;
    writer
        .write_event(Event::Text(BytesText::new(value)))
        .map_err(writer_error)?;
    writer
        .write_event(Event::End(BytesEnd::new(name)))
        .map_err(writer_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{output_encoding_error, writer_error};

    #[test]
    fn output_failures_keep_the_ts_diagnostic_code() -> Result<(), std::io::Error> {
        let writer_diagnostic = writer_error(std::io::Error::other("writer failed"));
        assert_eq!(writer_diagnostic.diagnostics[0].code, "pa.ts_write");
        assert!(
            writer_diagnostic.diagnostics[0]
                .message
                .contains("writer failed")
        );

        let invalid_utf8 = String::from_utf8(vec![0xff])
            .err()
            .ok_or(std::io::Error::other("invalid byte decoded as UTF-8"))?;
        let encoding_diagnostic = output_encoding_error(invalid_utf8);
        assert_eq!(encoding_diagnostic.diagnostics[0].code, "pa.ts_write");
        assert!(!encoding_diagnostic.diagnostics[0].message.is_empty());
        Ok(())
    }
}
