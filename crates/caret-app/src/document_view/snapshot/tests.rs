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
fn structure_snapshot_keeps_parent_highlight_after_virtual_inner_exit() -> Result<(), String> {
    use caret_doc::{Block, BlockEnvironment, BlockEnvironmentKind, Document, NodeId, Paragraph};
    use std::collections::BTreeMap;

    let paragraph = || Paragraph::empty(NodeId::generate());
    let inner = Block::Environment(BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Custom,
        definition_id: None,
        props: BTreeMap::new(),
        title: None,
        children: vec![Block::Paragraph(paragraph())],
        extra: BTreeMap::new(),
    });
    let outer = BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(paragraph()),
        children: vec![inner],
        extra: BTreeMap::new(),
    };
    let outer_node_id = outer.id.clone();
    let outer_id = outer.id.to_string();
    let mut runtime = DocumentViewRuntime::new(Document {
        body: vec![Block::Environment(outer)],
        ..Document::blank()
    })
    .map_err(|error| error.to_string())?;
    runtime
        .session
        .run(caret_engine::Command::MoveStructure(
            caret_engine::Direction::Forward,
        ))
        .map_err(|error| error.to_string())?;
    runtime
        .session
        .run(caret_engine::Command::ExitBlockEnvironment(
            caret_engine::Direction::Forward,
        ))
        .map_err(|error| error.to_string())?;

    let state = super::current_structure_state(&runtime.session);
    assert_eq!(state.as_array().map(Vec::len), Some(1));
    assert_eq!(state[0]["id"], outer_id);
    assert_eq!(state[0]["kind"], "section");

    let layout: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)
        .map_err(|error| error.to_string())?;
    let expected_parent_rectangles: Vec<_> = runtime
        .last_snapshot
        .as_ref()
        .into_iter()
        .flat_map(|snapshot| &snapshot.environment_ranges)
        .filter(|range| range.environment_id == outer_node_id)
        .map(|range| {
            serde_json::json!({
                "pageIndex": range.page_index,
                "x0Pt": range.x0_pt,
                "y0Pt": range.y0_pt,
                "x1Pt": range.x1_pt,
                "y1Pt": range.y1_pt,
            })
        })
        .collect();
    assert!(!expected_parent_rectangles.is_empty());
    let expected_parent_rectangles = Value::Array(expected_parent_rectangles);
    let assert_rectangles_match = |actual: &Value| {
        let actual_rectangles = actual
            .as_array()
            .expect("block environment highlight rectangles should be an array");
        let expected_rectangles = expected_parent_rectangles
            .as_array()
            .expect("expected parent rectangles should be an array");
        assert_eq!(actual_rectangles.len(), expected_rectangles.len());
        for (actual, expected) in actual_rectangles.iter().zip(expected_rectangles) {
            assert_eq!(actual["pageIndex"], expected["pageIndex"]);
            for key in ["x0Pt", "y0Pt", "x1Pt", "y1Pt"] {
                let actual = actual[key]
                    .as_f64()
                    .expect("actual rectangle coordinate should be numeric");
                let expected = expected[key]
                    .as_f64()
                    .expect("expected rectangle coordinate should be numeric");
                assert!(
                    (actual - expected).abs() <= 1e-9,
                    "rectangle coordinate {key} differs: {actual} vs {expected}"
                );
            }
        }
    };
    assert_rectangles_match(&layout["blockEnvironmentHighlightRects"]);
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)
        .map_err(|error| error.to_string())?;
    assert_rectangles_match(&interaction["blockEnvironmentHighlightRects"]);

    let root_environment = BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(paragraph()),
        children: vec![Block::Paragraph(paragraph())],
        extra: BTreeMap::new(),
    };
    let mut root_runtime = DocumentViewRuntime::new(Document {
        body: vec![Block::Environment(root_environment)],
        ..Document::blank()
    })
    .map_err(|error| error.to_string())?;
    root_runtime
        .session
        .run(caret_engine::Command::MoveStructure(
            caret_engine::Direction::Forward,
        ))
        .map_err(|error| error.to_string())?;
    root_runtime
        .session
        .run(caret_engine::Command::ExitBlockEnvironment(
            caret_engine::Direction::Forward,
        ))
        .map_err(|error| error.to_string())?;
    let root_exit_layout: Value = serde_json::from_str(&root_runtime.layout_snapshot_json()?)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        root_exit_layout["blockEnvironmentHighlightRects"],
        serde_json::json!([])
    );
    Ok(())
}

#[test]
fn math_command_snapshot_enables_supported_inline_math_and_publishes_focus() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "math".to_owned())?;
    let snapshot = runtime.command_line_state_json()?;
    let value: Value = serde_json::from_str(&snapshot).map_err(|error| error.to_string())?;
    let candidate = value["candidates"]
        .as_array()
        .and_then(|candidates| candidates.iter().find(|item| item["alias"] == "\\math"))
        .ok_or_else(|| "matching math candidate should be present".to_owned())?;
    assert_eq!(candidate["available"], true);
    let accepted: Value =
        serde_json::from_str(&runtime.command_line_command_json("accept", String::new())?)
            .map_err(|error| error.to_string())?;
    assert!(runtime.session.math_edit().is_some());
    assert!(accepted["caretGeometry"].is_object());
    Ok(())
}

#[test]
fn tree_math_visual_movement_keeps_subsequent_text_inside_the_formula() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "math".to_owned())?;
    runtime.command_line_command_json("accept", String::new())?;
    runtime.insert_text_json("ab".to_owned())?;

    runtime.move_visual_caret_json(false, false)?;
    assert!(runtime.session.math_tree_input_context().is_some());
    runtime.insert_text_json("X".to_owned())?;

    let paragraph = runtime
        .session
        .document()
        .body
        .iter()
        .find_map(|block| match block {
            caret_doc::Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .ok_or_else(|| "math insertion should keep the paragraph".to_owned())?;
    assert!(paragraph.inlines.iter().all(|inline| {
        !matches!(inline, caret_doc::ParagraphContent::Text(text) if text.runs.iter().any(|run| !run.text.is_empty()))
    }));
    assert!(paragraph.inlines.iter().any(|inline| {
        matches!(inline, caret_doc::ParagraphContent::Math(math) if !math.math_tree.root.content.is_empty())
    }));
    assert!(runtime.session.math_tree_input_context().is_some());
    Ok(())
}

#[test]
fn tree_math_shift_movement_keeps_formula_focus_until_selection_geometry_exists()
-> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "math".to_owned())?;
    runtime.command_line_command_json("accept", String::new())?;
    runtime.insert_text_json("ab".to_owned())?;

    let focus_before = runtime.session.math_tree_input_context();
    runtime.move_visual_caret_json(false, true)?;
    assert_eq!(runtime.session.math_tree_input_context(), focus_before);
    runtime.insert_text_json("X".to_owned())?;

    let paragraph = runtime
        .session
        .document()
        .body
        .iter()
        .find_map(|block| match block {
            caret_doc::Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .ok_or_else(|| "math insertion should keep the paragraph".to_owned())?;
    assert!(paragraph.inlines.iter().all(|inline| {
        !matches!(inline, caret_doc::ParagraphContent::Text(text) if text.runs.iter().any(|run| !run.text.is_empty()))
    }));
    assert!(runtime.session.math_tree_input_context().is_some());
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
    assert_eq!(
        default["codeFontFamily"],
        caret_font_roles::DEFAULT_CODE_FONT_FAMILY
    );
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
