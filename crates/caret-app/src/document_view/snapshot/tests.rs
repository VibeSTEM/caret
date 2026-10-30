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
