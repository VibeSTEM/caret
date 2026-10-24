//! 032 行为测试：theme/variables 校验、消息全字段、TS 写出与诊断路径。
//! 全部经公共 API（parse/emit_ts/format_source/source_prefix）驱动。

use std::error::Error;

use caret_dsl_core::{Kind, ValueType, emit_ts, format_source, parse, source_prefix};

const THEME_ALL_TYPES: &str = "version: 1\nkind: theme\n\nvalues:\n  color-bg: string = #1e1f22\n  spacing-small: int = 8\n  scale: real = 0.5\n  enable-shadows: bool = true\n  logo: resource = project:/assets/logo.svg\n";

const LANGUAGE_FEATURES: &str = "version: 1\nkind: language\nlanguage: zh-CN\nsourcelanguage: en\n\n// a retained comment\n[Menu]\nopen:\n  src: \"A\\\\B\"\n  oldsrc: Open\n  tr: 打开\n  st: finished\n  comment: translators note\n  extra: max 10 chars\n\nitems:\n  src: \"%n item\"\n  numerus: true\n  tr: [\"%n 项\", \"%n 项目\"]\n\ngone:\n  src: Vanished text\n  tr: 已消失\n  st: vanished\n\nmissing:\n  src: Missing text\n";

#[test]
fn theme_document_parses_all_value_types_and_formats_idempotently() -> Result<(), Box<dyn Error>> {
    let document = parse(THEME_ALL_TYPES)?;
    assert_eq!(document.kind, Kind::Theme);
    assert_eq!(document.values.len(), 5);
    let types: Vec<ValueType> = document
        .values
        .iter()
        .map(|value| value.value_type.clone())
        .collect();
    assert!(matches!(
        types.as_slice(),
        [
            ValueType::String,
            ValueType::Int,
            ValueType::Real,
            ValueType::Bool,
            ValueType::Resource
        ]
    ));
    let formatted = format_source(THEME_ALL_TYPES)?;
    assert_eq!(formatted, THEME_ALL_TYPES, "规范主题应幂等");
    assert_eq!(parse(&formatted)?, document);
    Ok(())
}

#[test]
fn variables_requires_values() -> Result<(), Box<dyn Error>> {
    assert!(matches!(
        parse("version: 1\nkind: variables\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.empty_values")
    ));
    Ok(())
}

#[test]
fn theme_allows_empty_overlay_to_inherit_defaults() -> Result<(), Box<dyn Error>> {
    let document = parse("version: 1\nkind: theme\n")?;
    assert_eq!(document.kind, Kind::Theme);
    assert!(document.values.is_empty());
    Ok(())
}

#[test]
fn theme_rejects_language_fields_and_values_reject_messages() -> Result<(), Box<dyn Error>> {
    assert!(matches!(
        parse("version: 1\nkind: theme\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.unexpected_language_field")
    ));
    assert!(matches!(
        parse("version: 1\nkind: language\nlanguage: en\n\nvalues:\n  scale: real = 0.5\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.unexpected_values")
    ));
    let source = "version: 1\nkind: variables\ncatalog: caret\n\nvalues:\n  scale: real = 0.5\n";
    let error = match parse(source) {
        Err(error) => error,
        Ok(_) => panic!("catalog is a language-only header"),
    };
    let diagnostic = error
        .diagnostics
        .iter()
        .find(|item| item.code == "pa.unexpected_language_field");
    assert_eq!(
        diagnostic.map(|item| item.start_offset),
        Some(source.find("catalog:").unwrap_or(usize::MAX))
    );
    Ok(())
}

#[test]
fn value_entries_outside_values_section_are_rejected() -> Result<(), Box<dyn Error>> {
    assert!(matches!(
        parse("version: 1\nkind: theme\n\n  shadow: bool = true\n\nvalues:\n  spacing: int = 4\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.entry_outside_section")
    ));
    Ok(())
}

#[test]
fn value_keys_require_kebab_case_and_uniqueness() -> Result<(), Box<dyn Error>> {
    assert!(matches!(
        parse("version: 1\nkind: variables\n\nvalues:\n  spacing_small: real = 8\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.invalid_key")
    ));
    assert!(matches!(
        parse("version: 1\nkind: variables\n\nvalues:\n  scale: real = 0.5\n  scale: real = 1\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.duplicate_key")
    ));
    Ok(())
}

#[test]
fn duplicate_headers_are_rejected() -> Result<(), Box<dyn Error>> {
    assert!(matches!(
        parse("version: 1\nversion: 1\nkind: language\nlanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.duplicate_header")
    ));
    assert!(matches!(
        parse("version: 1\nkind: language\nkind: language\nlanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.duplicate_header")
    ));
    assert!(matches!(
        parse("version: 1\nkind: language\nlanguage: en\nlanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.duplicate_header")
    ));
    assert!(matches!(
        parse("version: 1\nkind: language\nsourcelanguage: en\nsourcelanguage: en\nlanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.duplicate_header")
    ));
    Ok(())
}

#[test]
fn duplicate_contexts_are_rejected_but_ids_are_scoped_to_context() -> Result<(), Box<dyn Error>> {
    assert!(matches!(
        parse("version: 1\nkind: language\nlanguage: en\n\n[App]\nfirst:\n  src: First\n[App]\nsecond:\n  src: Second\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.duplicate_context")
    ));
    assert!(matches!(
        format_source(
            "version: 1\nkind: language\nlanguage: en\n\nimplicit:\n  src: Implicit\n[App]\nexplicit:\n  src: Explicit\n[Caret]\nlast:\n  src: Last\n"
        ),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.duplicate_context")
    ));
    let document = parse(
        "version: 1\nkind: language\nlanguage: en\n\n[Menu]\nopen:\n  src: Open\n[Toolbar]\nopen:\n  src: Open toolbar\n",
    )?;
    assert_eq!(document.messages.len(), 2);
    Ok(())
}

#[test]
fn duplicate_and_orphan_message_fields_are_rejected() -> Result<(), Box<dyn Error>> {
    let duplicated = "version: 1\nkind: language\nlanguage: en\n\n[App]\ntitle:\n  src: Hi\n  src: Hi\n  oldsrc: Old\n  oldsrc: Old\n  tr: Hi\n  tr: Hi\n  st: finished\n  st: finished\n  comment: a\n  comment: a\n  extra: b\n  extra: b\n  numerus: true\n  numerus: false\n";
    assert!(matches!(
        parse(duplicated),
        Err(ref diagnostics) if diagnostics.to_string().matches("pa.duplicate_field").count() >= 7
    ));

    // 孤儿字段必须缩进:0 列字段会被语法直接拒绝。
    let orphans = "version: 1\nkind: language\nlanguage: en\n\n[App]\n  src: Hi\n  oldsrc: Old\n  tr: Hi\n  st: finished\n  comment: a\n  extra: b\n  numerus: true\n";
    assert!(matches!(
        parse(orphans),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.field_without_message")
    ));
    Ok(())
}

#[test]
fn messages_require_src_and_headers_are_enforced() -> Result<(), Box<dyn Error>> {
    assert!(matches!(
        parse("version: 1\nkind: language\nlanguage: en\n\ntitle:\n  tr: Hi\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.missing_src")
    ));
    assert!(matches!(
        parse("kind: language\nlanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.missing_version")
    ));
    assert!(matches!(
        parse("version: 2\nkind: language\nlanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.unsupported_version")
    ));
    assert!(matches!(
        parse("version: 1\nlanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.missing_kind")
    ));
    Ok(())
}

#[test]
fn language_requires_locale_and_messages_and_rejects_bad_locales() -> Result<(), Box<dyn Error>> {
    assert!(matches!(
        parse("version: 1\nkind: language\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.missing_language")
    ));
    assert!(matches!(
        parse("version: 1\nkind: language\nlanguage: en\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.empty_language")
    ));
    assert!(matches!(
        parse("version: 1\nkind: language\nlanguage: zh-\nsourcelanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.invalid_locale")
    ));
    Ok(())
}

#[test]
fn language_full_feature_document_formats_and_round_trips() -> Result<(), Box<dyn Error>> {
    let document = parse(LANGUAGE_FEATURES)?;
    assert_eq!(document.kind, Kind::Language);
    assert_eq!(document.messages.len(), 4);
    let formatted = format_source(LANGUAGE_FEATURES)?;
    // 转义保留（引号内反斜杠双写）、注释保留、numerus 列表与状态保留。
    assert!(formatted.contains("  src: \"A\\\\B\"\n"), "{formatted}");
    assert!(formatted.contains("// a retained comment\n"));
    assert!(formatted.contains("  oldsrc: Open\n"));
    assert!(formatted.contains("  numerus: true\n"));
    assert!(formatted.contains("  tr: [\"%n 项\", \"%n 项目\"]\n"));
    assert!(formatted.contains("  st: vanished\n"));
    assert!(formatted.contains("  comment: translators note\n"));
    assert!(formatted.contains("  extra: max 10 chars\n"));
    assert_eq!(format_source(&formatted)?, formatted, "格式化必须幂等");
    assert_eq!(parse(&formatted)?, document);
    Ok(())
}

#[test]
fn emit_ts_carries_status_comment_and_numerus_features() -> Result<(), Box<dyn Error>> {
    let document = parse(LANGUAGE_FEATURES)?;
    let ts = emit_ts(&document, "zh-CN")?;
    assert!(ts.contains("<name>Menu</name>"), "{ts}");
    assert!(ts.contains("<oldsource>Open</oldsource>"), "{ts}");
    assert!(ts.contains("<comment>translators note</comment>"), "{ts}");
    assert!(
        ts.contains("<extracomment>max 10 chars</extracomment>"),
        "{ts}"
    );
    assert!(ts.contains("numerus=\"yes\""), "{ts}");
    assert!(ts.contains("<numerusform>%n 项</numerusform>"), "{ts}");
    assert!(ts.contains("type=\"vanished\""), "{ts}");
    assert!(ts.contains("type=\"unfinished\""), "{ts}");

    // numerus 且无 tr 时回退为 source 文本
    // (emit_message 的空形式回退分支)。
    let fallback_source = "version: 1\nkind: language\nlanguage: en\nsourcelanguage: en\n\n[App]\ncount:\n  src: \"%n item\"\n  numerus: true\n";
    let fallback = parse(fallback_source)?;
    let ts_fallback = emit_ts(&fallback, "en")?;
    assert!(
        ts_fallback.contains("<numerusform>%n item</numerusform>"),
        "{ts_fallback}"
    );
    Ok(())
}

#[test]
fn emit_ts_preserves_every_plural_form() -> Result<(), Box<dyn Error>> {
    let document = parse(
        "version: 1\nkind: language\nlanguage: ru-RU\n\n[App]\nitems:\n  src: \"%n item\"\n  numerus: true\n  tr: [\"%n one\", \"%n few\", \"%n many\"]\n",
    )?;

    let ts = emit_ts(&document, "ru-RU")?;

    assert!(ts.contains("<numerusform>%n one</numerusform>"), "{ts}");
    assert!(ts.contains("<numerusform>%n few</numerusform>"), "{ts}");
    assert!(ts.contains("<numerusform>%n many</numerusform>"), "{ts}");
    Ok(())
}

#[test]
fn emit_ts_rejects_colliding_qualified_ids() -> Result<(), Box<dyn Error>> {
    let document = parse(
        "version: 1\nkind: language\nlanguage: en\n\n[A]\nb.c:\n  src: First\n\n[A.b]\nc:\n  src: Second\n",
    )?;

    let error = match emit_ts(&document, "en") {
        Err(error) => error,
        Ok(_) => panic!("colliding Qt IDs unexpectedly emitted"),
    };

    assert!(error.to_string().contains("pa.duplicate_ts_id"), "{error}");
    Ok(())
}

#[test]
fn emit_ts_rejects_placeholder_mismatch_and_invalid_locale() -> Result<(), Box<dyn Error>> {
    // TS 输出只接受 language 文档；theme / variables 仍可解析和格式化。
    let variables = parse("version: 1\nkind: variables\n\nvalues:\n  spacing-small: int = 8\n")?;
    assert!(matches!(
        emit_ts(&variables, "en"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.ts_kind")
    ));

    // 占位符不匹配指向错误翻译字段，方便编辑器定位。
    let source = "version: 1\nkind: language\nlanguage: en\n\n[App]\nrevision:\n  src: Revision %1\n  tr: Revision\n";
    let error = match parse(source) {
        Err(error) => error,
        Ok(_) => panic!("占位符不匹配意外通过"),
    };
    let diagnostic = error
        .diagnostics
        .iter()
        .find(|item| item.code == "pa.placeholder_mismatch");
    assert_eq!(
        diagnostic.map(|item| item.start_offset),
        Some(source.find("  tr: Revision").unwrap_or(usize::MAX))
    );
    // emit_ts 对请求 locale 复核:合法文档 + 非法输出 locale → pa.ts_locale。
    let document = parse(
        "version: 1\nkind: language\nlanguage: en\n\n[App]\nrevision:\n  src: Revision %1\n  tr: Revision %1\n",
    )?;
    assert!(matches!(
        emit_ts(&document, "e!"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.ts_locale")
    ));
    Ok(())
}

#[test]
fn source_prefix_prefers_longest_source_match() -> Result<(), Box<dyn Error>> {
    let document = parse(
        "version: 1\nkind: language\nlanguage: en\n\n[App]\nok:\n  src: ok\n  tr: ok\nok-ok:\n  src: ok ok\n  tr: ok ok\n",
    )?;
    let Some((long_id, _)) = source_prefix(&document, "ok ok tail") else {
        panic!("应命中")
    };
    assert_eq!(long_id, "ok-ok");
    let Some((short_id, _)) = source_prefix(&document, "ok tail") else {
        panic!("应命中")
    };
    assert_eq!(short_id, "ok");
    assert!(source_prefix(&document, "no match").is_none());
    Ok(())
}

// 两个容量压力用例的输入必须超过固定阈值（2^20 字节 / 16384 条声明）才能
// 触发拒绝诊断，无法在 Miri 解释执行下按可行时间完成；容量维度由常规
// cargo test 覆盖（任务 032 Miri 边界登记）。
#[test]
#[cfg_attr(miri, ignore)]
fn oversized_source_is_rejected() -> Result<(), Box<dyn Error>> {
    let oversized = format!("version: 1\nkind: language\n{}", "x".repeat(1_048_577));
    assert!(matches!(
        parse(&oversized),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.source_too_large")
    ));
    Ok(())
}

#[test]
#[cfg_attr(miri, ignore)]
fn declaration_overflow_is_rejected() -> Result<(), Box<dyn Error>> {
    let mut source = String::from("version: 1\nkind: variables\n\nvalues:\n");
    for index in 0..16_400 {
        source.push_str(&format!("  v{index}: int = {index}\n"));
    }
    assert!(matches!(
        parse(&source),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.too_many_declarations")
    ));
    Ok(())
}

#[test]
#[cfg_attr(miri, ignore)]
fn language_messages_obey_the_declaration_limit() -> Result<(), Box<dyn Error>> {
    const MAX_DECLARATIONS: usize = 16_384;
    let mut source = String::from("version: 1\nkind: language\nlanguage: en\n\n[App]\n");
    for index in 0..MAX_DECLARATIONS {
        source.push_str(&format!("message-{index}:\n  src: Source {index}\n"));
    }
    assert_eq!(parse(&source)?.messages.len(), MAX_DECLARATIONS);

    source.push_str("extra:\n  src: Extra\n");
    assert!(matches!(
        parse(&source),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.too_many_declarations")
    ));
    Ok(())
}

#[test]
fn diagnostics_render_code_line_and_column() -> Result<(), Box<dyn Error>> {
    let Err(error) = parse("version: 1\nkind: language\nlanguage: en\n\ntitle:\n  tr: Hi\n") else {
        panic!("缺 src 的消息意外通过");
    };
    let rendered = error.to_string();
    assert!(rendered.contains("pa.missing_src at "), "{rendered}");
    assert!(rendered.contains(": "), "{rendered}");
    Ok(())
}

// ── 第二批:校验边角与转义解码(032 缺口清零)──

#[test]
fn numerus_list_without_numerus_flag_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    // 非 numerus 却给了复数列表 → pa.unexpected_plural。
    assert!(matches!(
        parse("version: 1\nkind: language\nlanguage: en\n\n[App]\nitems:\n  src: Items\n  tr: [\"A\", \"B\"]\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.unexpected_plural")
    ));
    Ok(())
}

#[test]
fn value_entries_require_type_expression_form() -> Result<(), Box<dyn std::error::Error>> {
    assert!(matches!(
        parse("version: 1\nkind: variables\n\nvalues:\n  scale: 0.5\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.value_type")
    ));
    Ok(())
}

#[test]
fn source_locale_is_also_locale_validated() -> Result<(), Box<dyn std::error::Error>> {
    assert!(matches!(
        parse("version: 1\nkind: language\nsourcelanguage: zh-\nlanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n"),
        Err(ref diagnostics) if diagnostics.to_string().contains("pa.invalid_locale")
    ));
    Ok(())
}

#[test]
fn valid_escapes_decode_into_control_characters() -> Result<(), Box<dyn std::error::Error>> {
    let document =
        parse("version: 1\nkind: language\nlanguage: en\n\n[App]\ntext:\n  src: line\\nbreak\n")?;
    let text = document
        .messages
        .values()
        .find(|message| message.id == "text")
        .unwrap_or_else(|| panic!("text 缺失"));
    assert!(
        text.source.contains('\n'),
        "解码后的换行应存在: {}",
        text.source
    );
    Ok(())
}

#[test]
fn unsupported_escapes_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let source = "version: 1\nkind: language\nlanguage: en\n\n[中]\ntext:\n  src: \"前缀\\x\"\n";
    let escape_offset = source.find("\\x").unwrap_or(usize::MAX);
    let error = match parse(source) {
        Err(error) => error,
        Ok(_) => panic!("unsupported escape unexpectedly accepted"),
    };
    let diagnostic = error
        .diagnostics
        .iter()
        .find(|item| item.code == "pa.invalid_escape")
        .unwrap_or_else(|| panic!("missing invalid escape diagnostic: {error}"));

    assert_eq!(diagnostic.start_offset, escape_offset);
    assert_eq!(diagnostic.end_offset, escape_offset);
    assert_eq!(diagnostic.line, 7);
    assert_eq!(diagnostic.column, 11);
    Ok(())
}

#[test]
fn trailing_escape_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let mut source =
        String::from("version: 1\nkind: language\nlanguage: en\n\n[中]\ntext:\n  src: ");
    source.push_str(&"a".repeat(41));
    source.push('\\');
    let escape_offset = source.len() - 1;

    let error = match parse(&source) {
        Err(error) => error,
        Ok(_) => panic!("trailing escape unexpectedly accepted"),
    };
    let diagnostic = error
        .diagnostics
        .iter()
        .find(|item| item.code == "pa.invalid_escape")
        .unwrap_or_else(|| panic!("missing invalid escape diagnostic: {error}"));

    assert_eq!(diagnostic.start_offset, escape_offset);
    assert_eq!(diagnostic.end_offset, escape_offset);
    assert_eq!(diagnostic.line, 7);
    assert_eq!(diagnostic.column, 49);
    Ok(())
}

#[test]
fn pest_syntax_errors_render_with_position() {
    let source = "version: nope\n";
    let Err(error) = parse(source) else {
        panic!("语法错误文档意外通过")
    };
    let rendered = error.to_string();
    assert!(rendered.contains("pa.syntax"), "{rendered}");
    let diagnostic = &error.diagnostics[0];
    // Pest may report a zero-width position where a required token should begin.
    assert_eq!(
        diagnostic.start_offset,
        source.find("nope").unwrap_or(usize::MAX)
    );
    assert_eq!(diagnostic.start_offset, diagnostic.end_offset);
    assert!(diagnostic.end_offset <= source.len());
    assert_eq!(&source[diagnostic.start_offset..diagnostic.end_offset], "");
}

#[test]
fn quoted_escapes_round_trip_through_formatter() -> Result<(), Box<dyn std::error::Error>> {
    let source = "version: 1\nkind: language\nlanguage: en\n\n[App]\ntext:\n  src: \"a\\tb\\nc\"\n";
    let formatted = format_source(source)?;
    // 解码后的真实制表/换行以转义形式再次写出,且幂等。
    assert!(formatted.contains("\\t"), "{formatted}");
    assert!(formatted.contains("\\n"), "{formatted}");
    assert_eq!(format_source(&formatted)?, formatted);
    Ok(())
}

#[test]
fn quoted_message_ids_are_decoded() -> Result<(), Box<dyn Error>> {
    let document = parse(
        "version: 1\nkind: language\nlanguage: en\n\n[App]\n\"menu open\":\n  src: Open\n  tr: Open\n",
    )?;

    assert!(document.messages.contains_key("App\u{1f}menu open"));
    Ok(())
}

#[test]
fn diagnostic_order_and_text_match_the_pre_refactor_baseline() {
    let source = "version: 2\nkind: language\nlanguage: en--US\n\n[App]\nx:\n  tr: Hi\n";
    let Err(diagnostics) = parse(source) else {
        panic!("invalid document unexpectedly accepted");
    };
    assert_eq!(
        diagnostics.to_string(),
        "pa.missing_src at 6:1: message 'x' requires src\n\
pa.unsupported_version at 1:1: only version 1 is supported\n\
pa.invalid_locale at 3:1: invalid language locale 'en--US'"
    );
    assert_eq!(
        diagnostics.diagnostics[0].start_offset,
        source.find("x:").unwrap_or(usize::MAX)
    );
    assert_eq!(diagnostics.diagnostics[1].start_offset, 0);
    assert_eq!(
        diagnostics.diagnostics[2].start_offset,
        source.find("language:").unwrap_or(usize::MAX)
    );
}
