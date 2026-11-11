//! 验证命令候选跨层快照只传递界面所需的数据。

use super::DocumentViewRuntime;
use serde_json::Value;

#[test]
fn insertion_slot_stays_visible_at_page_edges_and_centers_on_the_caret() {
    let (left_x0, left_x1) = super::insertion_slot_horizontal_bounds(0.0, 600.0, 6.0);
    assert_eq!((left_x0, left_x1), (0.0, 6.0));

    let (right_x0, right_x1) = super::insertion_slot_horizontal_bounds(600.0, 600.0, 6.0);
    assert_eq!((right_x0, right_x1), (594.0, 600.0));

    let (middle_x0, middle_x1) = super::insertion_slot_horizontal_bounds(120.0, 600.0, 6.0);
    assert_eq!((middle_x0, middle_x1), (117.0, 123.0));
}

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
    let assert_rectangles_match = |actual: &Value| -> Result<(), String> {
        let actual_rectangles = actual.as_array().ok_or_else(|| {
            "block environment highlight rectangles should be an array".to_owned()
        })?;
        let expected_rectangles = expected_parent_rectangles
            .as_array()
            .ok_or_else(|| "expected parent rectangles should be an array".to_owned())?;
        if actual_rectangles.len() != expected_rectangles.len() {
            return Err(format!(
                "rectangle counts differ: {} vs {}",
                actual_rectangles.len(),
                expected_rectangles.len()
            ));
        }
        for (actual, expected) in actual_rectangles.iter().zip(expected_rectangles) {
            if actual["pageIndex"] != expected["pageIndex"] {
                return Err(format!(
                    "rectangle page differs: {} vs {}",
                    actual["pageIndex"], expected["pageIndex"]
                ));
            }
            for key in ["x0Pt", "y0Pt", "x1Pt", "y1Pt"] {
                let actual = actual[key].as_f64().ok_or_else(|| {
                    format!("actual rectangle coordinate {key} should be numeric")
                })?;
                let expected = expected[key].as_f64().ok_or_else(|| {
                    format!("expected rectangle coordinate {key} should be numeric")
                })?;
                if (actual - expected).abs() > 1e-9 {
                    return Err(format!(
                        "rectangle coordinate {key} differs: {actual} vs {expected}"
                    ));
                }
            }
        }
        Ok(())
    };
    assert_rectangles_match(&layout["blockEnvironmentHighlightRects"])?;
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)
        .map_err(|error| error.to_string())?;
    assert_rectangles_match(&interaction["blockEnvironmentHighlightRects"])?;

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
fn tree_math_shift_movement_extends_the_formula_selection_without_leaving_the_host()
-> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "math".to_owned())?;
    runtime.command_line_command_json("accept", String::new())?;
    runtime.insert_text_json("ab".to_owned())?;

    let (target_before, focus_before) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| "math tree focus should exist".to_owned())?;
    assert_eq!(focus_before.offset, 2);
    // Shift+Left 在公式树内本地扩展：锚点保留，光标前移，宿主焦点不变。
    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(false, true)?)
        .map_err(|error| error.to_string())?;
    let (target_after, focus_after) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| "Shift+Left must keep the math tree focus".to_owned())?;
    assert_eq!(target_after, target_before);
    assert_eq!(focus_after.offset, 1);
    let Some(caret_engine::MathEditBranch::Tree { anchor, .. }) =
        runtime.session.math_edit().map(|edit| &edit.branch)
    else {
        return Err("Shift+Left must keep the math tree branch".to_owned());
    };
    assert_eq!(anchor.offset, 2);
    let (before_caret, _) = runtime
        .current_snapshot()?
        .math_tree_caret(&target_before, &focus_before)
        .ok_or_else(|| "math tree caret before the extension is missing".to_owned())?;
    let (expected_caret, expected_font_size) = runtime
        .current_snapshot()?
        .math_tree_caret(&target_after, &focus_after)
        .ok_or_else(|| "math tree caret after the extension is missing".to_owned())?;
    // 光标几何必须来自扩展后的公式树焦点，而不是未移动的旧位置。
    assert!(
        expected_caret.x_pt < before_caret.x_pt,
        "extending left in the math tree must move the caret geometry"
    );
    let caret = interaction["caretGeometry"]
        .as_object()
        .ok_or_else(|| "caretGeometry must be an object".to_owned())?;
    let caret_page = u64::try_from(expected_caret.page_index)
        .map_err(|error| format!("page index conversion failed: {error}"))?;
    assert_eq!(caret["pageIndex"].as_u64(), Some(caret_page));
    for (key, expected) in [
        ("xPt", expected_caret.x_pt),
        ("y0Pt", expected_caret.y0_pt),
        ("y1Pt", expected_caret.y1_pt),
    ] {
        let actual = caret[key]
            .as_f64()
            .ok_or_else(|| format!("caretGeometry.{key} must be a number"))?;
        assert!(
            (actual - expected).abs() < 1e-9,
            "caretGeometry.{key} differs: {actual} vs {expected}"
        );
    }
    assert_eq!(
        caret["fontSizeCentipoints"].as_u64(),
        Some(u64::from(expected_font_size))
    );
    // 目前公式树内部扩选没有文本层矩形；错误的正文投影会在此暴露。
    assert_eq!(interaction["selectionRects"], serde_json::json!([]));
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
    let math = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            caret_doc::ParagraphContent::Math(math) => Some(math),
            _ => None,
        })
        .ok_or_else(|| "math insertion should keep the inline math node".to_owned())?;
    assert_eq!(math_tree_text(&math.math_tree), "aX");
    assert!(runtime.session.math_tree_input_context().is_some());
    Ok(())
}

/// 读取公式树根流中的可见文本，用于断言本地扩展的实际结果。
fn math_tree_text(tree: &caret_doc::MathTree) -> String {
    fn flow_text(flow: &caret_doc::MathFlow) -> String {
        flow.content.iter().map(content_text).collect()
    }
    fn content_text(content: &caret_doc::MathContent) -> String {
        match content {
            caret_doc::MathContent::Text(flow) => {
                flow.runs.iter().map(|run| run.text.as_str()).collect()
            }
            caret_doc::MathContent::Math(node) => {
                let mut text = node.text.clone().unwrap_or_default();
                text.extend(node.children.iter().map(flow_text));
                text
            }
            caret_doc::MathContent::Environment(environment) => {
                environment.children.iter().map(content_text).collect()
            }
            caret_doc::MathContent::SoftBreak(_) | caret_doc::MathContent::Opaque(_) => {
                String::new()
            }
        }
    }
    flow_text(&tree.root)
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

#[test]
fn require_current_snapshot_rejects_stale_document_or_font_generations() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.layout_snapshot_json()?;
    let stale = runtime
        .last_snapshot
        .clone()
        .ok_or_else(|| "layout snapshot is missing".to_owned())?;
    let font_generation = runtime.layout.font_generation();
    super::require_current_snapshot(&stale, &runtime.session, font_generation)?;

    runtime
        .session
        .run(caret_engine::Command::InsertText("x".to_owned()))
        .map_err(|error| error.to_string())?;
    assert_eq!(
        super::require_current_snapshot(&stale, &runtime.session, font_generation),
        Err("layout.stale_highlight_snapshot".to_owned()),
        "an older document generation must not publish selection or highlight geometry"
    );

    let current = runtime.current_snapshot()?.clone();
    super::require_current_snapshot(&current, &runtime.session, font_generation)?;
    assert_eq!(
        super::require_current_snapshot(&current, &runtime.session, font_generation + 1),
        Err("layout.stale_highlight_snapshot".to_owned()),
        "a stale font catalog generation must be rejected as well"
    );
    Ok(())
}

#[test]
fn stale_composition_snapshot_cannot_be_published_after_the_document_changes() -> Result<(), String>
{
    let mut runtime = DocumentViewRuntime::new_blank()?;
    let published: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)
        .map_err(|error| error.to_string())?;
    let published_generation = published["documentGeneration"]
        .as_str()
        .and_then(|generation| generation.parse::<u64>().ok())
        .ok_or_else(|| "published document generation is missing".to_owned())?;

    // 真实输入法预编辑建立组合预览快照；此时发布路径通过同代校验。
    runtime.input_method_event_json("a".to_owned(), String::new(), 0, 0, 1)?;
    let composition_id = runtime
        .composition_id
        .ok_or_else(|| "composition should be active".to_owned())?;
    let preview_generation = runtime
        .composition_snapshot
        .as_ref()
        .map(|snapshot| snapshot.document_generation)
        .ok_or_else(|| "composition preview snapshot is missing".to_owned())?;
    assert_eq!(preview_generation, published_generation);

    // 提交改变文档代际，但保留已抓取的组合预览快照：模拟迟到的旧快照。
    runtime
        .session
        .run(caret_engine::Command::CommitComposition {
            id: composition_id,
            text: "a".to_owned(),
        })
        .map_err(|error| error.to_string())?;
    assert!(runtime.session.document_generation() > published_generation);
    assert_eq!(
        runtime
            .composition_snapshot
            .as_ref()
            .map(|snapshot| snapshot.document_generation),
        Some(preview_generation)
    );

    // 真实发布入口读取该旧快照时必须拒绝，而不是发布过期几何。
    let error = match runtime.layout_snapshot_json() {
        Ok(_) => {
            return Err("a stale composition snapshot must not be published".to_owned());
        }
        Err(error) => error,
    };
    assert!(
        error.contains("layout.stale_highlight_snapshot"),
        "unexpected publish error: {error}"
    );
    let error = match runtime.interaction_state_json() {
        Ok(_) => {
            return Err(
                "a stale composition snapshot must not be published as an interaction".to_owned(),
            );
        }
        Err(error) => error,
    };
    assert!(
        error.contains("layout.stale_highlight_snapshot"),
        "unexpected interaction error: {error}"
    );
    Ok(())
}

/// 构造只包含一个文本片段的公式树。
fn equation_math_tree(text: &str) -> caret_doc::MathTree {
    caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: caret_doc::NodeId::generate(),
            content: vec![caret_doc::MathContent::Text(caret_doc::TextFlow {
                runs: vec![caret_doc::Run {
                    text: text.to_owned(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                }],
                extra: Default::default(),
            })],
            extra: Default::default(),
        },
        extra: Default::default(),
    }
}

fn runtime_with_trailing_equation(
    math_id: &caret_doc::NodeId,
) -> Result<DocumentViewRuntime, String> {
    let mut document = caret_doc::Document::blank();
    document
        .body
        .push(caret_doc::Block::Equation(caret_doc::Equation {
            id: math_id.clone(),
            math_tree: equation_math_tree("x"),
            extra: Default::default(),
        }));
    DocumentViewRuntime::new(document)
}

fn clear_cached_math_fragments(runtime: &mut DocumentViewRuntime) -> Result<(), String> {
    let snapshot = runtime
        .last_snapshot
        .as_mut()
        .ok_or_else(|| "layout snapshot is missing".to_owned())?;
    for page in &mut snapshot.pages {
        page.layout.math_fragments.clear();
    }
    Ok(())
}

#[test]
fn selection_projection_without_text_bounds_is_rejected_for_structural_ranges() -> Result<(), String>
{
    let math_id = caret_doc::NodeId::generate();
    let mut runtime = runtime_with_trailing_equation(&math_id)?;
    runtime.layout_snapshot_json()?;
    let selection = runtime.session.selection().clone();
    let snapshot = runtime.current_snapshot()?.clone();

    // 结构选区无文本端点时不能回落到普通选区发布空数组。
    assert_eq!(
        super::selection_rectangles_json(
            &selection,
            None,
            true,
            &[],
            std::slice::from_ref(&math_id),
            &snapshot,
        ),
        Err("layout.stale_selection_geometry".to_owned())
    );
    // 非结构选区的普通文本投影保持原有行为。
    assert_eq!(
        super::selection_rectangles_json(&selection, None, false, &[], &[], &snapshot),
        Ok(serde_json::json!([]))
    );
    Ok(())
}

#[test]
fn missing_selected_math_geometry_is_rejected_instead_of_cleared() -> Result<(), String> {
    let math_id = caret_doc::NodeId::generate();
    let mut runtime = runtime_with_trailing_equation(&math_id)?;
    runtime.layout_snapshot_json()?;
    let focus = runtime.session.selection().focus.clone();
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: focus.clone(),
                focus,
            },
        )))
        .map_err(|error| error.to_string())?;
    // 文档首段为空，Shift+Right 直接把公式块纳入结构选区。
    // 空段落后的第一次扩选停在公式块外前边界，第二次才覆盖公式块本身。
    runtime
        .move_visual_caret_json(true, true)
        .map_err(|error| error.to_string())?;
    runtime
        .move_visual_caret_json(true, true)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        runtime.session.selected_math_blocks(),
        std::slice::from_ref(&math_id)
    );

    clear_cached_math_fragments(&mut runtime)?;
    let error = match runtime.interaction_state_json() {
        Ok(_) => {
            return Err("missing selected math geometry must not be published empty".to_owned());
        }
        Err(error) => error,
    };
    assert!(
        error.contains("layout.stale_selection_geometry"),
        "unexpected interaction error: {error}"
    );
    let error = match runtime.layout_snapshot_json() {
        Ok(_) => {
            return Err("missing selected math geometry must not be published empty".to_owned());
        }
        Err(error) => error,
    };
    assert!(
        error.contains("layout.stale_selection_geometry"),
        "unexpected layout error: {error}"
    );
    Ok(())
}

#[test]
fn missing_active_math_highlight_geometry_is_rejected() -> Result<(), String> {
    let math_id = caret_doc::NodeId::generate();
    let mut runtime = runtime_with_trailing_equation(&math_id)?;
    runtime.layout_snapshot_json()?;
    let (page_index, x_pt, y_pt) = {
        let snapshot = runtime.current_snapshot()?;
        let (page_index, page, fragment) = snapshot
            .pages
            .iter()
            .enumerate()
            .find_map(|(page_index, page)| {
                page.layout
                    .math_fragments
                    .iter()
                    .find(|fragment| fragment.math_id == math_id)
                    .map(|fragment| (page_index, page, fragment))
            })
            .ok_or_else(|| "equation fragment is missing".to_owned())?;
        let left = page.margin_left_pt + f64::from(fragment.x_offset_pt);
        let top = page.margin_top_pt + f64::from(fragment.y_offset_pt);
        let right = left + f64::from(fragment.width_pt);
        let bottom = top + f64::from(fragment.height_pt);
        let mut hit = None;
        let mut y = top;
        while y <= bottom && hit.is_none() {
            let mut x = left;
            while x <= right {
                if snapshot.hit_test_math_text(page_index, x, y).is_some() {
                    hit = Some((x, y));
                    break;
                }
                x += 0.5;
            }
            y += 0.5;
        }
        let (x, y) = hit.ok_or_else(|| "equation text was not hittable".to_owned())?;
        (page_index, x, y)
    };
    let placed: Value = serde_json::from_str(
        &runtime
            .place_caret_at_page_json(
                i32::try_from(page_index).map_err(|error| error.to_string())?,
                x_pt,
                y_pt,
            )
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(placed["mathText"], true);
    assert!(matches!(
        runtime.session.current_math_edit_target(),
        Some(caret_engine::MathEditTarget::Block { .. })
    ));

    clear_cached_math_fragments(&mut runtime)?;
    let error = match runtime.interaction_state_json() {
        Ok(_) => {
            return Err("missing active math highlight geometry must be rejected".to_owned());
        }
        Err(error) => error,
    };
    assert!(
        error.contains("layout.stale_math_highlight_geometry"),
        "unexpected interaction error: {error}"
    );
    Ok(())
}
