//! 验证命令候选跨层快照只传递界面所需的数据。

use super::DocumentViewRuntime;
use serde_json::Value;

#[test]
fn command_candidate_snapshot_omits_search_only_labels() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    let snapshot = runtime.command_line_command_json("type", "sec".to_owned())?;
    let value: Value = serde_json::from_str(&snapshot).map_err(|error| error.to_string())?;
    let candidates = value["commandLine"]["candidates"]
        .as_array()
        .ok_or_else(|| "命令候选快照不是数组".to_owned())?;

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0]["alias"], "\\section");
    assert_eq!(candidates[0]["commandId"], "struct.section.create");
    assert_eq!(candidates[0]["available"], true);
    assert!(candidates[0].get("label").is_none());
    Ok(())
}

#[test]
fn math_commands_remain_unavailable_in_the_command_line_snapshot() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "math".to_owned())?;
    let snapshot = runtime.command_line_state_json()?;
    let value: Value = serde_json::from_str(&snapshot).map_err(|error| error.to_string())?;
    let candidate = value["candidates"]
        .as_array()
        .and_then(|candidates| candidates.iter().find(|item| item["alias"] == "\\math"))
        .ok_or_else(|| "matching math candidate should remain discoverable".to_owned())?;
    assert_eq!(candidate["available"], false);
    runtime.command_line_command_json("accept", String::new())?;
    let after_accept: Value = serde_json::from_str(&runtime.command_line_state_json()?)
        .map_err(|error| error.to_string())?;
    assert_eq!(after_accept["mode"], "command");
    assert_eq!(after_accept["query"], "math");
    Ok(())
}

#[test]
fn math_source_command_line_snapshot_keeps_its_mode_and_cursor() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime
        .session
        .run(caret_engine::Command::InsertInlineMath("x".to_owned()))
        .map_err(|error| error.to_string())?;
    runtime
        .session
        .run(caret_engine::Command::EnterMath(
            caret_engine::Direction::Backward,
        ))
        .map_err(|error| error.to_string())?;
    runtime
        .session
        .run(caret_engine::Command::BeginCommandLine)
        .map_err(|error| error.to_string())?;
    let snapshot = runtime.command_line_state_json()?;
    let value: Value = serde_json::from_str(&snapshot).map_err(|error| error.to_string())?;
    assert_eq!(value["mode"], "math");
    assert_eq!(value["prefix"], "\\");
    assert_eq!(value["query"], "");
    assert_eq!(value["cursor"], 0);
    Ok(())
}

#[test]
fn command_line_snapshot_projects_the_document_code_font() -> Result<(), String> {
    let mut document = caret_doc::Document::blank();
    document.global_settings.default_code_font_family = "Noto Sans Mono".to_owned();
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.command_line_command_json("begin", String::new())?;
    let explicit: Value = serde_json::from_str(&runtime.command_line_state_json()?)
        .map_err(|error| error.to_string())?;
    assert_eq!(explicit["codeFontFamily"], "Noto Sans Mono");

    let mut default_document = caret_doc::Document::blank();
    default_document.global_settings.default_code_font_family =
        caret_doc::DEFAULT_FONT_FAMILY.to_owned();
    let mut default_runtime = DocumentViewRuntime::new(default_document)?;
    default_runtime.command_line_command_json("begin", String::new())?;
    let default: Value = serde_json::from_str(&default_runtime.command_line_state_json()?)
        .map_err(|error| error.to_string())?;
    assert_eq!(default["codeFontFamily"], caret_code::BUILTIN_FONT_FAMILY);
    Ok(())
}

#[test]
fn font_command_snapshot_exposes_unicode_picker_mode_and_utf16_cursor() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "font".to_owned())?;
    let opened = runtime.command_line_command_json("accept", String::new())?;
    let opened: Value = serde_json::from_str(&opened).map_err(|error| error.to_string())?;
    assert_eq!(opened["commandLine"]["mode"], "fontFamily");
    assert_eq!(opened["commandLine"]["prefix"], "\\font ");

    let typed = runtime.command_line_command_json("type", "字".to_owned())?;
    let typed: Value = serde_json::from_str(&typed).map_err(|error| error.to_string())?;
    assert_eq!(typed["commandLine"]["query"], "字");
    assert_eq!(typed["commandLine"]["cursorUtf16"], 7);
    Ok(())
}

#[test]
fn font_picker_ime_updates_query_without_modifying_document() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank().map_err(|error| error.to_string())?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "font".to_owned())?;
    runtime.command_line_command_json("accept", String::new())?;

    let preedit = runtime.input_method_event_json("zhong".to_owned(), String::new(), 0, 0, 5)?;
    let preedit: Value = serde_json::from_str(&preedit).map_err(|error| error.to_string())?;
    assert_eq!(preedit["commandLine"]["preedit"], "zhong");
    assert_eq!(preedit["inputMethodContext"]["surroundingText"], "\\font ");
    assert_eq!(runtime.session.document_generation(), 0);

    let committed =
        runtime.input_method_event_json(String::new(), "思源 宋体".to_owned(), 0, 0, 0)?;
    let committed: Value = serde_json::from_str(&committed).map_err(|error| error.to_string())?;
    assert_eq!(committed["commandLine"]["query"], "思源 宋体");
    assert_eq!(committed["commandLine"]["preedit"], "");
    assert_eq!(
        committed["inputMethodContext"]["surroundingText"],
        "\\font 思源 宋体"
    );
    let deleted = runtime.input_method_event_json(String::new(), String::new(), -2, 2, 0)?;
    let deleted: Value = serde_json::from_str(&deleted).map_err(|error| error.to_string())?;
    assert_eq!(deleted["commandLine"]["query"], "思源 ");
    assert_eq!(
        deleted["inputMethodContext"]["surroundingText"],
        "\\font 思源 "
    );
    assert_eq!(runtime.session.document_generation(), 0);
    Ok(())
}

#[test]
fn command_line_ime_never_commits_non_ascii_text_into_the_document() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank().map_err(|error| error.to_string())?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.input_method_event_json("nihao".to_owned(), String::new(), 0, 0, 5)?;
    let committed = runtime.input_method_event_json(String::new(), "你好".to_owned(), 0, 0, 0)?;
    let committed: Value = serde_json::from_str(&committed).map_err(|error| error.to_string())?;
    assert_eq!(committed["commandLine"]["query"], "");
    assert_eq!(committed["commandLine"]["preedit"], "");
    assert_eq!(runtime.session.document_generation(), 0);
    Ok(())
}
