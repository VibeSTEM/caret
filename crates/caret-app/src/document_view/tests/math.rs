//! 验证公式命令、公式内编辑与插入点几何的键盘端到端路径。

use super::*;
use serde_json::Value;

fn parse(json: &str) -> Result<Value, String> {
    serde_json::from_str(json).map_err(|error| error.to_string())
}

fn caret_x(value: &Value) -> Result<f64, String> {
    value["caretGeometry"]["xPt"]
        .as_f64()
        .ok_or_else(|| "快照缺少插入点横坐标".to_owned())
}

/// 从键盘路径端到端走通：`\math` → 回车 → 公式内输入 → `\frac` 子命令 → 退出。
#[test]
fn inline_math_is_reachable_from_command_line_and_subcommands() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("a".to_owned())?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "math".to_owned())?;
    runtime.handle_editor_key_json("enter")?;
    assert!(runtime.session.math_edit().is_some());
    let entered = parse(&runtime.interaction_state_json()?)?;
    let empty_x = caret_x(&entered)?;

    runtime.insert_text_json("x".to_owned())?;
    let typed = parse(&runtime.interaction_state_json()?)?;
    let typed_x = caret_x(&typed)?;
    assert!(typed_x > empty_x - 1e-6, "输入后插入点不应左移");

    let opened = parse(&runtime.command_line_command_json("begin", String::new())?)?;
    assert_eq!(opened["commandLine"]["mode"], "command");
    let listed = parse(&runtime.command_line_command_json("type", "frac".to_owned())?)?;
    let candidates = listed["commandLine"]["candidates"]
        .as_array()
        .ok_or_else(|| "公式子命令候选不是数组".to_owned())?;
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate["alias"] == "\\frac" && candidate["available"] == true)
    );
    runtime.handle_editor_key_json("enter")?;
    let source = runtime
        .session
        .math_edit()
        .map(|edit| edit.source.clone())
        .ok_or_else(|| "插入模板后应仍在公式编辑中".to_owned())?;
    assert_eq!(source, "x\\frac{}{}");

    runtime.handle_editor_key_json("escape")?;
    let exited = parse(&runtime.interaction_state_json()?)?;
    assert!(runtime.session.math_edit().is_none());
    assert!(exited["caretGeometry"].is_object());
    Ok(())
}

/// 正文光标紧邻行内公式时，方向键进入公式，在边界再按一次退出。
#[test]
fn arrow_keys_enter_and_leave_inline_math() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "math".to_owned())?;
    runtime.handle_editor_key_json("enter")?;
    runtime.insert_text_json("y".to_owned())?;
    runtime.handle_editor_key_json("escape")?;
    assert!(runtime.session.math_edit().is_none());

    runtime.move_visual_caret_json(false, false)?;
    let cursor = runtime
        .session
        .math_edit()
        .map(|edit| edit.cursor)
        .ok_or_else(|| "左键应进入公式".to_owned())?;
    assert_eq!(cursor, 1);
    runtime.move_visual_caret_json(false, false)?;
    runtime.move_visual_caret_json(false, false)?;
    assert!(runtime.session.math_edit().is_none());
    Ok(())
}

/// `\equation` 插入显示公式并进入编辑，插入点位于公式块内。
#[test]
fn equation_command_inserts_display_math_and_draws_caret() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "equation".to_owned())?;
    runtime.handle_editor_key_json("enter")?;
    assert!(runtime.session.math_edit().is_some());
    runtime.insert_text_json("E".to_owned())?;
    let typed = parse(&runtime.interaction_state_json()?)?;
    assert!(typed["caretGeometry"].is_object());
    let source = runtime
        .session
        .math_edit()
        .map(|edit| edit.source.clone())
        .ok_or_else(|| "应仍在显示公式编辑中".to_owned())?;
    assert_eq!(source, "E");
    Ok(())
}
