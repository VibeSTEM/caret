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
    // 公式树内部扩选发布本地选区矩形：覆盖被选符号，且纵向范围与焦点 caret 一致。
    let selection_rectangles = interaction["selectionRects"]
        .as_array()
        .ok_or_else(|| "selectionRects must be an array".to_owned())?;
    assert!(
        !selection_rectangles.is_empty(),
        "formula tree selection must publish visible rects"
    );
    assert_rectangles_within_page(&mut runtime, &interaction["selectionRects"])?;
    let rectangle = &selection_rectangles[0];
    for (key, expected) in [
        ("x0Pt", expected_caret.x_pt),
        ("y0Pt", expected_caret.y0_pt),
        ("y1Pt", expected_caret.y1_pt),
    ] {
        let actual = rectangle[key]
            .as_f64()
            .ok_or_else(|| format!("selectionRects[0].{key} must be a number"))?;
        assert!(
            (actual - expected).abs() < 1e-9,
            "selectionRects[0].{key} differs: {actual} vs {expected}"
        );
    }
    let full: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)
        .map_err(|error| error.to_string())?;
    assert_eq!(full["selectionRects"], interaction["selectionRects"]);
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

#[test]
fn equation_tree_shift_selection_publishes_page_selection_rects() -> Result<(), String> {
    let mut document = caret_doc::Document::blank();
    let paragraph_id = document
        .body
        .iter()
        .find_map(|block| match block {
            caret_doc::Block::Paragraph(paragraph) => Some(paragraph.id.clone()),
            _ => None,
        })
        .ok_or_else(|| "blank document should contain a paragraph".to_owned())?;
    document.body.insert(
        0,
        caret_doc::Block::Equation(caret_doc::Equation {
            id: caret_doc::NodeId::generate(),
            math_tree: symbol_math_tree(&["x", "+", "y"]),
            extra: Default::default(),
        }),
    );
    let mut runtime = DocumentViewRuntime::new(document)?;
    let caret = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id,
        offset: 0,
        affinity: caret_engine::Affinity::Downstream,
    };
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: caret.clone(),
                focus: caret,
            },
        )))
        .map_err(|error| error.to_string())?;
    runtime
        .session
        .run(caret_engine::Command::EnterMath(
            caret_engine::Direction::Backward,
        ))
        .map_err(|error| error.to_string())?;

    // 反向扩选公式末尾符号：焦点复用本地序列，选区矩形从此发布。
    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(false, true)?)
        .map_err(|error| error.to_string())?;
    let rectangles = interaction["selectionRects"]
        .as_array()
        .ok_or_else(|| "selectionRects must be an array".to_owned())?;
    assert!(
        !rectangles.is_empty(),
        "equation tree selection must publish visible rects"
    );
    assert_rectangles_within_page(&mut runtime, &interaction["selectionRects"])?;
    let (target, anchor, focus) = runtime
        .session
        .math_tree_selection_context()
        .ok_or_else(|| "equation tree selection context is missing".to_owned())?;
    assert_tree_selection_projection(
        &mut runtime,
        &interaction["selectionRects"],
        &target,
        &anchor,
        &focus,
    )?;
    let full: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)
        .map_err(|error| error.to_string())?;
    assert_eq!(full["selectionRects"], interaction["selectionRects"]);

    // 继续扩选仍然发布矩形，并向前扩展左边界。
    let extended: Value = serde_json::from_str(&runtime.move_visual_caret_json(false, true)?)
        .map_err(|error| error.to_string())?;
    let extended_rectangles = extended["selectionRects"]
        .as_array()
        .ok_or_else(|| "extended selectionRects must be an array".to_owned())?;
    assert!(!extended_rectangles.is_empty());
    let first_left = rectangles
        .iter()
        .filter_map(|rectangle| rectangle["x0Pt"].as_f64())
        .fold(f64::INFINITY, f64::min);
    let extended_left = extended_rectangles
        .iter()
        .filter_map(|rectangle| rectangle["x0Pt"].as_f64())
        .fold(f64::INFINITY, f64::min);
    assert!(extended_left < first_left);
    Ok(())
}

#[test]
fn align_tree_shift_selection_publishes_page_selection_rects() -> Result<(), String> {
    let mut document = caret_doc::Document::blank();
    let paragraph_id = document
        .body
        .iter()
        .find_map(|block| match block {
            caret_doc::Block::Paragraph(paragraph) => Some(paragraph.id.clone()),
            _ => None,
        })
        .ok_or_else(|| "blank document should contain a paragraph".to_owned())?;
    document
        .body
        .push(caret_doc::Block::Align(caret_doc::Align {
            id: caret_doc::NodeId::generate(),
            rows: vec![
                caret_doc::MathAlignRow {
                    id: caret_doc::NodeId::generate(),
                    cells: vec![symbol_math_tree(&["a", "+", "b"])],
                    extra: Default::default(),
                },
                caret_doc::MathAlignRow {
                    id: caret_doc::NodeId::generate(),
                    cells: vec![symbol_math_tree(&["c", "+", "d"])],
                    extra: Default::default(),
                },
            ],
            extra: Default::default(),
        }));
    let mut runtime = DocumentViewRuntime::new(document)?;
    let caret = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id,
        offset: 0,
        affinity: caret_engine::Affinity::Downstream,
    };
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: caret.clone(),
                focus: caret,
            },
        )))
        .map_err(|error| error.to_string())?;
    runtime
        .session
        .run(caret_engine::Command::EnterMath(
            caret_engine::Direction::Forward,
        ))
        .map_err(|error| error.to_string())?;

    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, true)?)
        .map_err(|error| error.to_string())?;
    let rectangles = interaction["selectionRects"]
        .as_array()
        .ok_or_else(|| "selectionRects must be an array".to_owned())?;
    assert!(
        !rectangles.is_empty(),
        "align tree selection must publish visible rects"
    );
    assert_rectangles_within_page(&mut runtime, &interaction["selectionRects"])?;
    let (target, anchor, focus) = runtime
        .session
        .math_tree_selection_context()
        .ok_or_else(|| "align tree selection context is missing".to_owned())?;
    assert_tree_selection_projection(
        &mut runtime,
        &interaction["selectionRects"],
        &target,
        &anchor,
        &focus,
    )?;
    let full: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)
        .map_err(|error| error.to_string())?;
    assert_eq!(full["selectionRects"], interaction["selectionRects"]);
    Ok(())
}

/// 逐符号拆分的公式树，供公式树内部选区快照测试使用。
fn symbol_math_tree(symbols: &[&str]) -> caret_doc::MathTree {
    caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: caret_doc::NodeId::generate(),
            content: symbols
                .iter()
                .map(|symbol| {
                    caret_doc::MathContent::Math(caret_doc::MathNode {
                        id: caret_doc::NodeId::generate(),
                        kind: caret_doc::MathNodeKind::Symbol,
                        text: Some((*symbol).to_owned()),
                        attributes: std::collections::BTreeMap::new(),
                        children: Vec::new(),
                        extra: std::collections::BTreeMap::new(),
                    })
                })
                .collect(),
            extra: std::collections::BTreeMap::new(),
        },
        extra: std::collections::BTreeMap::new(),
    }
}

/// 断言发布给界面的选区矩形与同代引擎几何一致。
fn assert_tree_selection_projection(
    runtime: &mut DocumentViewRuntime,
    rectangles: &Value,
    target: &caret_engine::MathEditTarget,
    anchor: &caret_engine::MathTreePosition,
    focus: &caret_engine::MathTreePosition,
) -> Result<(), String> {
    let expected = runtime
        .current_snapshot()?
        .math_tree_selection_geometry(target, anchor, focus)
        .ok_or_else(|| "formula tree selection geometry is missing".to_owned())?;
    let actual = rectangles
        .as_array()
        .ok_or_else(|| "selectionRects must be an array".to_owned())?;
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(
            actual["pageIndex"].as_u64(),
            Some(expected.page_index as u64)
        );
        for (key, expected) in [
            ("x0Pt", expected.x0_pt),
            ("y0Pt", expected.y0_pt),
            ("x1Pt", expected.x1_pt),
            ("y1Pt", expected.y1_pt),
        ] {
            let actual = actual[key]
                .as_f64()
                .ok_or_else(|| format!("selectionRects[0].{key} must be a number"))?;
            assert!((actual - expected).abs() < 1e-9);
        }
    }
    Ok(())
}

/// 断言选区矩形落在所属页面内，避免 Qt 快照校验拒绝整份投影。
fn assert_rectangles_within_page(
    runtime: &mut DocumentViewRuntime,
    rectangles: &Value,
) -> Result<(), String> {
    let snapshot = runtime.current_snapshot()?;
    let rectangles = rectangles
        .as_array()
        .ok_or_else(|| "selectionRects must be an array".to_owned())?;
    for rectangle in rectangles {
        let page_index = rectangle["pageIndex"]
            .as_u64()
            .ok_or_else(|| "selectionRects pageIndex must be an integer".to_owned())?;
        let page = snapshot
            .pages
            .get(usize::try_from(page_index).map_err(|error| error.to_string())?)
            .ok_or_else(|| "selectionRects pageIndex is out of range".to_owned())?;
        let mut values = [0.0; 4];
        for (index, key) in ["x0Pt", "y0Pt", "x1Pt", "y1Pt"].into_iter().enumerate() {
            values[index] = rectangle[key]
                .as_f64()
                .ok_or_else(|| format!("selectionRects {key} must be a number"))?;
        }
        let [x0, y0, x1, y1] = values;
        assert!(x0 >= 0.0 && y0 >= 0.0 && x1 > x0 && y1 > y0);
        assert!(x1 <= page.width_pt && y1 <= page.height_pt);
    }
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

/// 构造 `before + InlineMath(逐符号) + after` 的单段落文档。
fn runtime_with_inline_math(
    before: &str,
    symbols: &[&str],
    after: &str,
) -> Result<(DocumentViewRuntime, caret_doc::NodeId, caret_doc::NodeId), String> {
    let text_content = |text: &str| {
        caret_doc::ParagraphContent::Text(caret_doc::TextFlow {
            runs: vec![caret_doc::Run {
                text: text.to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }],
            extra: Default::default(),
        })
    };
    let mut paragraph = caret_doc::Paragraph::empty(caret_doc::NodeId::generate());
    if !before.is_empty() {
        paragraph.inlines.push(text_content(before));
    }
    let math_id = caret_doc::NodeId::generate();
    paragraph
        .inlines
        .push(caret_doc::ParagraphContent::Math(caret_doc::InlineMath {
            id: math_id.clone(),
            math_tree: symbol_math_tree(symbols),
            extra: Default::default(),
        }));
    if !after.is_empty() {
        paragraph.inlines.push(text_content(after));
    }
    let paragraph_id = paragraph.id.clone();
    let mut document = caret_doc::Document::blank();
    document.body = vec![caret_doc::Block::Paragraph(paragraph)];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    Ok((runtime, paragraph_id, math_id))
}

/// 把文档选区放到 InlineMath 外前/外后侧并进入公式编辑。
fn enter_inline_math(
    runtime: &mut DocumentViewRuntime,
    paragraph_id: &caret_doc::NodeId,
    offset: usize,
    backward: bool,
) -> Result<(), String> {
    let position = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset,
        affinity: if backward {
            caret_engine::Affinity::Upstream
        } else {
            caret_engine::Affinity::Downstream
        },
    };
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: position.clone(),
                focus: position,
            },
        )))
        .map_err(|error| error.to_string())?;
    runtime
        .session
        .run(caret_engine::Command::EnterMath(if backward {
            caret_engine::Direction::Backward
        } else {
            caret_engine::Direction::Forward
        }))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn interaction_json(runtime: &mut DocumentViewRuntime) -> Result<Value, String> {
    serde_json::from_str(&runtime.interaction_state_json()?).map_err(|error| error.to_string())
}

fn caret_x(value: &Value) -> Result<f64, String> {
    value["caretGeometry"]["xPt"]
        .as_f64()
        .ok_or_else(|| "caretGeometry.xPt must be a number".to_owned())
}

fn single_selection_rect(value: &Value) -> Result<(f64, f64), String> {
    let rectangles = value["selectionRects"]
        .as_array()
        .ok_or_else(|| "selectionRects must be an array".to_owned())?;
    let [rectangle] = rectangles.as_slice() else {
        return Err(format!(
            "converted object selection should publish exactly one rectangle, got {}",
            rectangles.len()
        ));
    };
    let x0 = rectangle["x0Pt"]
        .as_f64()
        .ok_or_else(|| "selectionRects[0].x0Pt must be a number".to_owned())?;
    let x1 = rectangle["x1Pt"]
        .as_f64()
        .ok_or_else(|| "selectionRects[0].x1Pt must be a number".to_owned())?;
    Ok((x0, x1))
}

/// 读取全部选区矩形 `(x0, y0, x1, y1)`，供平台无关的不重叠/不膨胀断言使用。
fn selection_rectangles(value: &Value) -> Result<Vec<(f64, f64, f64, f64)>, String> {
    let rectangles = value["selectionRects"]
        .as_array()
        .ok_or_else(|| "selectionRects must be an array".to_owned())?;
    rectangles
        .iter()
        .map(|rectangle| {
            Ok((
                rectangle["x0Pt"]
                    .as_f64()
                    .ok_or_else(|| "selectionRects x0Pt must be a number".to_owned())?,
                rectangle["y0Pt"]
                    .as_f64()
                    .ok_or_else(|| "selectionRects y0Pt must be a number".to_owned())?,
                rectangle["x1Pt"]
                    .as_f64()
                    .ok_or_else(|| "selectionRects x1Pt must be a number".to_owned())?,
                rectangle["y1Pt"]
                    .as_f64()
                    .ok_or_else(|| "selectionRects y1Pt must be a number".to_owned())?,
            ))
        })
        .collect()
}

fn assert_selection_rectangles_are_disjoint(rectangles: &[(f64, f64, f64, f64)], label: &str) {
    for (index, left) in rectangles.iter().enumerate() {
        assert!(
            left.2 > left.0,
            "{label}: selection rect {index} must have positive width"
        );
        for right in rectangles.iter().skip(index + 1) {
            let horizontal_overlap = left.2.min(right.2) - left.0.max(right.0);
            let vertical_overlap = left.3.min(right.3) - left.1.max(right.1);
            assert!(
                horizontal_overlap <= 0.01 || vertical_overlap <= 0.01,
                "{label}: selection rectangles must not overlap: {left:?} vs {right:?}"
            );
        }
    }
}

/// 断言对象盒区间在选区矩形中完整出现且只出现一次；未选中时完全不出现。
fn assert_object_interval_selected_once(
    rectangles: &[(f64, f64, f64, f64)],
    interval: (f64, f64),
    selected: bool,
    label: &str,
) {
    let (object_x0, object_x1) = interval;
    let mut overlap_total = 0.0;
    let mut containing = 0;
    for rectangle in rectangles {
        let overlap = rectangle.2.min(object_x1) - rectangle.0.max(object_x0);
        if overlap <= 0.01 {
            continue;
        }
        assert!(
            rectangle.0 <= object_x0 + 0.01 && rectangle.2 >= object_x1 - 0.01,
            "{label}: a rectangle may only cover the object interval completely: {rectangle:?}"
        );
        overlap_total += overlap;
        containing += 1;
    }
    let expected = if selected { object_x1 - object_x0 } else { 0.0 };
    assert!(
        (overlap_total - expected).abs() < 0.01,
        "{label}: object interval overlap {overlap_total} must equal {expected}"
    );
    assert!(
        containing <= 1,
        "{label}: the object interval must not be drawn twice"
    );
}

/// 用对象外前/外后 caret 几何取 InlineMath 的版面盒区间。
fn inline_math_object_span(
    runtime: &mut DocumentViewRuntime,
    paragraph_id: &caret_doc::NodeId,
    start: usize,
) -> Result<(f64, f64), String> {
    let snapshot = runtime.current_snapshot()?;
    let start_position = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: start,
        affinity: caret_engine::Affinity::Downstream,
    };
    let end_position = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: start + '\u{fffc}'.len_utf8(),
        affinity: caret_engine::Affinity::Upstream,
    };
    let left = snapshot
        .caret_geometry(&start_position, caret_engine::VisualAffinity::Downstream)
        .ok_or_else(|| "InlineMath left caret is missing".to_owned())?;
    let right = snapshot
        .caret_geometry(&end_position, caret_engine::VisualAffinity::Upstream)
        .ok_or_else(|| "InlineMath right caret is missing".to_owned())?;
    Ok((left.x_pt, right.x_pt))
}

#[test]
fn inline_math_shift_boundary_caret_stays_on_the_object_outside_edge() -> Result<(), String> {
    // 覆盖用户报告的“公式位于段落末尾”以及前后有文本、位于段首/段尾的场景。
    for (before, after) in [("ab", ""), ("ab", "cd"), ("", "cd")] {
        for backward in [false, true] {
            let (mut runtime, paragraph_id, _math_id) =
                runtime_with_inline_math(before, &["x", "y"], after)?;
            let start = before.len();
            let end = start + 3;
            enter_inline_math(
                &mut runtime,
                &paragraph_id,
                if backward { end } else { start },
                backward,
            )?;
            while runtime.session.math_edit().is_some() {
                runtime
                    .move_visual_caret_json(!backward, true)
                    .map_err(|error| format!("shift boundary conversion failed: {error}"))?;
            }

            let interaction = interaction_json(&mut runtime)?;
            let (rect_x0, rect_x1) = single_selection_rect(&interaction)?;
            let caret = caret_x(&interaction)?;
            let expected = if backward { rect_x0 } else { rect_x1 };
            assert!(
                (caret - expected).abs() < 1e-9,
                "before={before:?} after={after:?} backward={backward}: caret {caret} must sit on \
                 the {} object edge {expected} (rect {rect_x0}..{rect_x1})",
                if backward { "left" } else { "right" }
            );
            assert!(
                rect_x1 - rect_x0 > 1.0,
                "the object selection rectangle must span the formula object"
            );
        }
    }
    Ok(())
}

#[test]
fn inline_math_shift_boundary_conversion_matches_the_non_shift_exit_side() -> Result<(), String> {
    for backward in [false, true] {
        let (mut runtime, paragraph_id, _math_id) =
            runtime_with_inline_math("ab", &["x", "y"], "cd")?;
        let start = 2;
        let end = start + 3;
        enter_inline_math(
            &mut runtime,
            &paragraph_id,
            if backward { end } else { start },
            backward,
        )?;
        while runtime.session.math_edit().is_some() {
            runtime
                .move_visual_caret_json(!backward, false)
                .map_err(|error| format!("non-shift boundary exit failed: {error}"))?;
        }
        let non_shift = interaction_json(&mut runtime)?;
        let non_shift_caret = caret_x(&non_shift)?;
        let non_shift_selection = runtime.session.selection().clone();
        assert_eq!(non_shift_selection.anchor, non_shift_selection.focus);
        let expected_affinity = if backward {
            caret_engine::Affinity::Downstream
        } else {
            caret_engine::Affinity::Upstream
        };
        assert_eq!(
            non_shift_selection.focus.offset,
            if backward { start } else { end }
        );
        assert_eq!(non_shift_selection.focus.affinity, expected_affinity);

        let (mut runtime, paragraph_id, _math_id) =
            runtime_with_inline_math("ab", &["x", "y"], "cd")?;
        enter_inline_math(
            &mut runtime,
            &paragraph_id,
            if backward { end } else { start },
            backward,
        )?;
        while runtime.session.math_edit().is_some() {
            runtime
                .move_visual_caret_json(!backward, true)
                .map_err(|error| format!("shift boundary conversion failed: {error}"))?;
        }
        let shift = interaction_json(&mut runtime)?;
        assert!(
            (caret_x(&shift)? - non_shift_caret).abs() < 1e-9,
            "shift and non-shift exits must land the caret on the same object side"
        );
    }
    Ok(())
}

#[test]
fn inline_math_shift_right_after_conversion_continues_plain_text_extension() -> Result<(), String> {
    let (mut runtime, paragraph_id, _math_id) = runtime_with_inline_math("ab", &["x", "y"], "cd")?;
    enter_inline_math(&mut runtime, &paragraph_id, 2, false)?;
    while runtime.session.math_edit().is_some() {
        runtime
            .move_visual_caret_json(true, true)
            .map_err(|error| format!("shift boundary conversion failed: {error}"))?;
    }
    let converted = interaction_json(&mut runtime)?;
    let converted_rectangles = selection_rectangles(&converted)?;
    let (object_x0, object_x1) = inline_math_object_span(&mut runtime, &paragraph_id, 2)?;
    assert_selection_rectangles_are_disjoint(&converted_rectangles, "converted object selection");
    assert_object_interval_selected_once(
        &converted_rectangles,
        (object_x0, object_x1),
        true,
        "converted object selection",
    );
    let converted_x0 = converted_rectangles
        .iter()
        .map(|rectangle| rectangle.0)
        .fold(f64::INFINITY, f64::min);
    let converted_x1 = converted_rectangles
        .iter()
        .map(|rectangle| rectangle.2)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!((converted_x0 - object_x0).abs() < 0.01);
    assert!((converted_x1 - object_x1).abs() < 0.01);
    let converted_caret = caret_x(&converted)?;
    assert!((converted_caret - converted_x1).abs() < 1e-6);

    runtime
        .move_visual_caret_json(true, true)
        .map_err(|error| format!("continued shift-right failed: {error}"))?;
    let continued = interaction_json(&mut runtime)?;
    let selection = runtime.session.selection().clone();
    assert_eq!(selection.anchor.offset, 2);
    assert_eq!(
        selection.anchor.affinity,
        caret_engine::Affinity::Downstream
    );
    assert_eq!(selection.focus.offset, 6);
    let continued_rectangles = selection_rectangles(&continued)?;
    assert_selection_rectangles_are_disjoint(&continued_rectangles, "continued object selection");
    // 对象区间只能出现一次；选区继续到对象之后的文本时不能产生覆盖同一对象的第二个矩形。
    assert_object_interval_selected_once(
        &continued_rectangles,
        (object_x0, object_x1),
        true,
        "continued object selection",
    );
    let continued_x0 = continued_rectangles
        .iter()
        .map(|rectangle| rectangle.0)
        .fold(f64::INFINITY, f64::min);
    let continued_x1 = continued_rectangles
        .iter()
        .map(|rectangle| rectangle.2)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (continued_x0 - converted_x0).abs() < 0.01,
        "continued extension must keep the object anchor on the left edge"
    );
    let continued_caret = caret_x(&continued)?;
    assert!(
        continued_caret > converted_caret,
        "continued Shift+Right should move the caret past the object"
    );
    // 高亮不能越过选区焦点：否则就是被折进对象 advance 的下一字素膨胀。
    assert!(
        continued_x1 <= continued_caret + 0.01,
        "selection highlight must not extend past the focus caret"
    );
    assert!(
        continued_x1 >= object_x1 - 0.01,
        "selection highlight must cover the selected object and following text"
    );
    Ok(())
}

#[test]
fn inline_math_object_selection_with_surrounding_text_has_no_overlap_or_inflation()
-> Result<(), String> {
    let (mut runtime, paragraph_id, _math_id) = runtime_with_inline_math("ab", &["x", "y"], "cd")?;
    let object_start = 2;
    let (object_x0, object_x1) =
        inline_math_object_span(&mut runtime, &paragraph_id, object_start)?;
    // 覆盖对象前后文本、对象本身以及反向选区的组合；选区端点按字节偏移给出。
    for (anchor_offset, focus_offset) in [(0usize, 5usize), (2, 5), (2, 6), (0, 7), (5, 6), (6, 2)]
    {
        let anchor = caret_engine::Pos {
            story: caret_engine::StoryId::Body,
            paragraph_id: paragraph_id.clone(),
            offset: anchor_offset,
            affinity: caret_engine::Affinity::Downstream,
        };
        let focus = caret_engine::Pos {
            story: caret_engine::StoryId::Body,
            paragraph_id: paragraph_id.clone(),
            offset: focus_offset,
            affinity: caret_engine::Affinity::Upstream,
        };
        runtime
            .session
            .run(caret_engine::Command::SetSelection(Some(
                caret_engine::Selection { anchor, focus },
            )))
            .map_err(|error| error.to_string())?;
        let interaction = interaction_json(&mut runtime)?;
        let rectangles = selection_rectangles(&interaction)?;
        let label = format!("selection {anchor_offset}..{focus_offset}");
        assert!(
            !rectangles.is_empty(),
            "{label}: selection must publish rects"
        );
        assert_selection_rectangles_are_disjoint(&rectangles, &label);
        let start = anchor_offset.min(focus_offset);
        let end = anchor_offset.max(focus_offset);
        assert_object_interval_selected_once(
            &rectangles,
            (object_x0, object_x1),
            start <= object_start && object_start + 3 <= end,
            &label,
        );
        for (x0, y0, x1, y1) in &rectangles {
            assert!(x1 > x0 && y1 > y0, "{label}: rect must have positive area");
        }
        let union_x1 = rectangles
            .iter()
            .map(|rectangle| rectangle.2)
            .fold(f64::NEG_INFINITY, f64::max);
        if anchor_offset < focus_offset && focus_offset == 6 {
            let caret = caret_x(&interaction)?;
            assert!(
                union_x1 <= caret + 0.01,
                "{label}: highlight must not extend past the focus caret"
            );
        }
    }
    Ok(())
}
