//! 验证文档视图布局快照、绘制与错误隔离。

use super::*;
use caret_doc::{
    BlockEnvironment, BlockEnvironmentKind, Equation, InlineEnvironment, InlineEnvironmentKind,
    InlineMath, MathContent, MathFlow, MathTree, NodeId, Paragraph, ParagraphContent, Run,
    TextFlow,
};
use std::collections::BTreeMap;

#[test]
fn blank_document_snapshot_contains_page_and_paragraph_layout()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    let json = runtime.layout_snapshot_json()?;
    let snapshot: Value = serde_json::from_str(&json)?;

    assert_eq!(snapshot["documentGeneration"], "0");
    assert!(
        snapshot["fontGeneration"]
            .as_str()
            .and_then(|generation| generation.parse::<u64>().ok())
            .is_some_and(|generation| generation > 0)
    );
    assert_eq!(snapshot["pages"].as_array().map(Vec::len), Some(1));
    assert_eq!(snapshot["pages"][0]["widthPt"], 595.2755905511812);
    assert_eq!(snapshot["paragraphs"].as_array().map(Vec::len), Some(1));
    assert_eq!(snapshot["paragraphs"][0]["text"], "");
    assert_eq!(
        snapshot["blockEnvironmentHighlightRects"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(snapshot["paragraphs"][0]["lines"][0]["sourceStart"], 0);
    assert!(
        snapshot["paragraphs"][0]["lines"][0]["heightPt"]
            .as_f64()
            .is_some_and(|height| height > 0.0)
    );

    let hit_json = runtime.hit_test_json(0, 73.0, 73.0)?;
    let hit: Value = serde_json::from_str(&hit_json)?;
    assert_eq!(hit["position"]["offsetUtf8"], "0");
    assert_eq!(hit["position"]["affinity"], "downstream");
    assert_eq!(hit["visualAffinity"], "downstream");
    assert_eq!(hit["caret"]["pageIndex"], 0);
    assert!(hit["caret"]["xPt"].as_f64().is_some());
    assert!(hit["caret"]["y0Pt"].as_f64().is_some());
    assert!(hit["caret"]["y1Pt"].as_f64().is_some());
    assert_eq!(runtime.hit_test_json(-1, 73.0, 73.0)?, "null");
    Ok(())
}

#[test]
fn block_environment_highlight_geometry_tracks_the_active_block_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let environment_id = NodeId::generate();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(Paragraph::empty(NodeId::generate())),
        children: vec![Block::Paragraph(Paragraph::empty(NodeId::generate()))],
        extra: BTreeMap::new(),
    })];
    let mut runtime = DocumentViewRuntime::new(document)?;

    let snapshot: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    let rectangles = snapshot["blockEnvironmentHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("focus rectangles were omitted"))?;
    assert_eq!(rectangles.len(), 1);
    assert_eq!(rectangles[0]["pageIndex"], 0);
    assert!(rectangles[0]["x1Pt"].as_f64() > rectangles[0]["x0Pt"].as_f64());
    assert!(rectangles[0]["y1Pt"].as_f64() > rectangles[0]["y0Pt"].as_f64());

    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    assert_eq!(
        interaction["blockEnvironmentHighlightRects"],
        snapshot["blockEnvironmentHighlightRects"]
    );
    Ok(())
}

#[test]
fn empty_inline_and_pending_format_carets_publish_visible_highlight_slots()
-> Result<(), Box<dyn std::error::Error>> {
    let mut pending_runtime = DocumentViewRuntime::new_blank()?;
    pending_runtime
        .session
        .run(caret_engine::Command::ToggleInlineFormat(
            InlineEnvironmentKind::Bold,
        ))?;
    let pending: Value = serde_json::from_str(&pending_runtime.layout_snapshot_json()?)?;
    let pending_rectangles = pending["inlineEnvironmentHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("pending style slot was omitted"))?;
    assert_eq!(pending_rectangles.len(), 1);
    assert!(pending_rectangles[0]["x1Pt"].as_f64() > pending_rectangles[0]["x0Pt"].as_f64());
    assert!(pending_rectangles[0]["y1Pt"].as_f64() > pending_rectangles[0]["y0Pt"].as_f64());
    pending_runtime
        .session
        .run(caret_engine::Command::ToggleInlineFormat(
            InlineEnvironmentKind::Bold,
        ))?;
    let inactive: Value = serde_json::from_str(&pending_runtime.layout_snapshot_json()?)?;
    assert_eq!(
        inactive["inlineEnvironmentHighlightRects"],
        serde_json::json!([])
    );

    let mut formatted_document = Document::blank();
    let Block::Paragraph(paragraph) = &mut formatted_document.body[0] else {
        return Err(std::io::Error::other("blank document has no paragraph").into());
    };
    paragraph.inlines = vec![ParagraphContent::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::from([("enabled".to_owned(), serde_json::Value::Bool(true))]),
        children: vec![ParagraphContent::Text(TextFlow {
            runs: vec![Run {
                text: "ab".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    })];
    let mut formatted_runtime = DocumentViewRuntime::new(formatted_document)?;
    let mut position = formatted_runtime.session.selection().focus.clone();
    position.offset = 1;
    position.affinity = caret_engine::Affinity::Downstream;
    formatted_runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: position.clone(),
                focus: position,
            },
        )))?;
    formatted_runtime
        .session
        .run(caret_engine::Command::ToggleInlineFormat(
            InlineEnvironmentKind::Italic,
        ))?;
    let formatted: Value = serde_json::from_str(&formatted_runtime.layout_snapshot_json()?)?;
    assert!(
        formatted["inlineEnvironmentHighlightRects"]
            .as_array()
            .is_some_and(|rects| {
                rects.len() >= 2
                    && rects.iter().all(|rectangle| {
                        rectangle["x1Pt"].as_f64() > rectangle["x0Pt"].as_f64()
                            && rectangle["y1Pt"].as_f64() > rectangle["y0Pt"].as_f64()
                    })
            })
    );

    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        return Err(std::io::Error::other("blank document has no paragraph").into());
    };
    paragraph.inlines = vec![ParagraphContent::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Italic,
        definition_id: None,
        props: BTreeMap::from([("enabled".to_owned(), serde_json::Value::Bool(true))]),
        children: Vec::new(),
        extra: BTreeMap::new(),
    })];
    let mut empty_runtime = DocumentViewRuntime::new(document)?;
    let mut outside = empty_runtime.session.selection().focus.clone();
    outside.affinity = caret_engine::Affinity::Upstream;
    empty_runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: outside.clone(),
                focus: outside,
            },
        )))?;
    empty_runtime.session.run(caret_engine::Command::MoveCaret(
        caret_engine::Direction::Forward,
    ))?;
    let empty: Value = serde_json::from_str(&empty_runtime.layout_snapshot_json()?)?;
    let empty_rectangles = empty["inlineEnvironmentHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("empty inline slot was omitted"))?;
    assert_eq!(empty_rectangles.len(), 1);
    assert!(empty_rectangles[0]["x1Pt"].as_f64() > empty_rectangles[0]["x0Pt"].as_f64());
    assert!(empty_rectangles[0]["y1Pt"].as_f64() > empty_rectangles[0]["y0Pt"].as_f64());
    Ok(())
}

#[test]
fn clicking_math_text_installs_tree_focus_and_publishes_its_caret()
-> Result<(), Box<dyn std::error::Error>> {
    let math_tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: caret_doc::NodeId::generate(),
            content: vec![caret_doc::MathContent::Text(caret_doc::TextFlow {
                runs: vec![Run {
                    text: "ab".to_owned(),
                    language: None,
                    props: BTreeMap::new(),
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    let mut document = Document::blank();
    document.body.push(Block::Equation(caret_doc::Equation {
        id: caret_doc::NodeId::generate(),
        math_tree,
        extra: BTreeMap::new(),
    }));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;

    let (x_pt, y_pt) = {
        let snapshot = runtime.current_snapshot()?;
        let page = &snapshot.pages[0];
        let fragment = page
            .layout
            .math_fragments
            .first()
            .ok_or_else(|| std::io::Error::other("equation fragment is missing"))?;
        let left = page.margin_left_pt + f64::from(fragment.x_offset_pt);
        let top = page.margin_top_pt + f64::from(fragment.y_offset_pt);
        let right = left + f64::from(fragment.width_pt);
        let bottom = top + f64::from(fragment.height_pt);
        let mut hit_point = None;
        let mut y = top;
        while y <= bottom && hit_point.is_none() {
            let mut x = left;
            while x <= right {
                if snapshot.hit_test_math_text(0, x, y).is_some() {
                    hit_point = Some((x, y));
                    break;
                }
                x += 0.5;
            }
            y += 0.5;
        }
        hit_point.ok_or_else(|| std::io::Error::other("equation TextFlow was not hittable"))?
    };

    let placed: Value = serde_json::from_str(&runtime.place_caret_at_page_json(0, x_pt, y_pt)?)?;
    assert_eq!(placed["valid"], true);
    assert_eq!(placed["mathText"], true);
    assert!(placed["caret"]["xPt"].as_f64().is_some());
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    assert_eq!(interaction["inputMethodContext"]["enabled"], true);
    assert_eq!(interaction["inputMethodContext"]["surroundingText"], "ab");
    assert_eq!(
        interaction["blockEnvironmentHighlightRects"],
        serde_json::json!([])
    );
    assert_eq!(
        interaction["inlineEnvironmentHighlightRects"],
        serde_json::json!([])
    );
    let math_block_rectangles = interaction["mathBlockHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("math block highlight was omitted"))?;
    assert_eq!(math_block_rectangles.len(), 1);
    assert_eq!(math_block_rectangles[0]["pageIndex"], 0);
    assert!(math_block_rectangles[0]["x1Pt"].as_f64() > math_block_rectangles[0]["x0Pt"].as_f64());
    assert!(math_block_rectangles[0]["y1Pt"].as_f64() > math_block_rectangles[0]["y0Pt"].as_f64());
    let math_text_rectangles = interaction["mathTextHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("math text highlight was omitted"))?;
    assert!(!math_text_rectangles.is_empty());
    assert_eq!(
        interaction["selectionRects"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(interaction["caretGeometry"]["xPt"], placed["caret"]["xPt"]);
    assert_eq!(
        interaction["caretGeometry"]["y0Pt"],
        placed["caret"]["y0Pt"]
    );
    let caret: Value = serde_json::from_str(&runtime.current_caret_json()?)?;
    assert_eq!(caret["xPt"], placed["caret"]["xPt"]);
    assert_eq!(caret["y0Pt"], placed["caret"]["y0Pt"]);
    assert_eq!(caret["y1Pt"], placed["caret"]["y1Pt"]);
    assert!(matches!(
        runtime.session.math_edit().map(|edit| &edit.branch),
        Some(caret_engine::MathEditBranch::Text { .. })
    ));

    let original_focus = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("math text input context is missing"))?
        .focus;
    runtime.move_visual_caret_json(true, false)?;
    let moved_focus = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("visual movement left math text"))?
        .focus;
    assert_ne!(moved_focus, original_focus);
    let moved_interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    let moved_caret: Value = serde_json::from_str(&runtime.current_caret_json()?)?;
    assert_eq!(
        moved_interaction["caretGeometry"]["xPt"],
        moved_caret["xPt"]
    );
    assert_eq!(
        moved_interaction["caretGeometry"]["y0Pt"],
        moved_caret["y0Pt"]
    );
    runtime.move_visual_caret_json(true, true)?;
    let selected_interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    assert!(
        selected_interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    let selected_text: Value = serde_json::from_str(&runtime.selected_text_json()?)?;
    assert!(matches!(selected_text["text"].as_str(), Some("a" | "b")));
    let refreshed: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    assert!(
        refreshed["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    assert_eq!(
        refreshed["caretGeometry"]["xPt"],
        selected_interaction["caretGeometry"]["xPt"]
    );
    runtime.move_to_visual_line_edge_json(false, false)?;
    runtime.move_to_visual_line_edge_json(true, true)?;
    let deleted: Value = serde_json::from_str(&runtime.delete_backward_json()?)?;
    assert!(
        runtime.session.math_text_input_context().is_some(),
        "math edit: {:?}",
        runtime.session.math_edit()
    );
    assert_eq!(deleted["inputMethodContext"]["surroundingText"], "");
    let empty_interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    assert_eq!(
        empty_interaction["caretGeometry"]["fontSizeCentipoints"],
        1200
    );
    assert!(
        empty_interaction["caretGeometry"]["y1Pt"].as_f64()
            > empty_interaction["caretGeometry"]["y0Pt"].as_f64()
    );
    let resumed: Value = serde_json::from_str(&runtime.paste_text_json("z".to_owned())?)?;
    assert_eq!(resumed["inputMethodContext"]["surroundingText"], "z");

    runtime.insert_text_json("\\".to_owned())?;
    assert_eq!(
        runtime.session.command_line().map(|line| line.mode),
        Some(caret_engine::CommandLineMode::Math)
    );
    runtime.insert_text_json("alpha".to_owned())?;
    let cursor_before_navigation = runtime
        .session
        .command_line()
        .ok_or_else(|| std::io::Error::other("math command line was not retained"))?
        .cursor;
    runtime.move_visual_caret_json(false, false)?;
    let cursor_after_navigation = runtime
        .session
        .command_line()
        .ok_or_else(|| std::io::Error::other("math command line was lost by navigation"))?
        .cursor;
    assert_eq!(cursor_after_navigation, cursor_before_navigation - 1);
    runtime.move_vertical_caret_json(1, false)?;
    runtime.move_to_visual_line_edge_json(false, false)?;
    runtime.move_to_visual_line_edge_json(true, false)?;
    assert_eq!(
        runtime.session.command_line().map(|line| line.mode),
        Some(caret_engine::CommandLineMode::Math)
    );
    Ok(())
}

#[test]
fn equation_and_align_text_focus_publish_full_width_block_highlights()
-> Result<(), Box<dyn std::error::Error>> {
    for align in [false, true] {
        let tree = MathTree {
            schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
            root: MathFlow {
                id: NodeId::generate(),
                content: vec![MathContent::Text(TextFlow {
                    runs: vec![Run {
                        text: "x".to_owned(),
                        language: None,
                        props: BTreeMap::new(),
                        extra: BTreeMap::new(),
                    }],
                    extra: BTreeMap::new(),
                })],
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        };
        let block_id = NodeId::generate();
        let block = if align {
            Block::Align(caret_doc::Align {
                id: block_id.clone(),
                rows: vec![caret_doc::MathAlignRow {
                    id: NodeId::generate(),
                    cells: vec![tree],
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            })
        } else {
            Block::Equation(Equation {
                id: block_id.clone(),
                math_tree: tree,
                extra: BTreeMap::new(),
            })
        };
        let mut document = Document::blank();
        let section_id = NodeId::generate();
        let mut section_children = (0..48)
            .map(|_| Block::Paragraph(Paragraph::empty(NodeId::generate())))
            .collect::<Vec<_>>();
        section_children.push(block);
        document.body.push(Block::Environment(BlockEnvironment {
            id: section_id.clone(),
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: BTreeMap::new(),
            title: Some(Paragraph::empty(NodeId::generate())),
            children: section_children,
            extra: BTreeMap::new(),
        }));
        let mut runtime = DocumentViewRuntime::new(document)?;
        runtime.layout_snapshot_json()?;
        let unfocused: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
        assert_eq!(unfocused["mathBlockHighlightRects"], serde_json::json!([]));

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
                        .find(|fragment| fragment.math_id == block_id)
                        .map(|fragment| (page_index, page, fragment))
                })
                .ok_or_else(|| std::io::Error::other("math host fragment is missing"))?;
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
            let (x, y) =
                hit.ok_or_else(|| std::io::Error::other("math TextFlow was not hittable"))?;
            (page_index, x, y)
        };
        assert!(page_index > 0, "the focus case must exercise a later page");
        let placed: Value = serde_json::from_str(&runtime.place_caret_at_page_json(
            i32::try_from(page_index)?,
            x_pt,
            y_pt,
        )?)?;
        assert_eq!(placed["mathText"], true);

        let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
        let block_rectangles = interaction["mathBlockHighlightRects"]
            .as_array()
            .ok_or_else(|| std::io::Error::other("math block highlight was omitted"))?;
        assert_eq!(block_rectangles.len(), 1, "align={align}");
        assert_eq!(block_rectangles[0]["pageIndex"], page_index);
        let page = &runtime.current_snapshot()?.pages[page_index];
        assert_eq!(block_rectangles[0]["x0Pt"], page.margin_left_pt);
        assert_eq!(
            block_rectangles[0]["x1Pt"],
            page.width_pt - page.margin_right_pt
        );
        assert!(
            interaction["mathTextHighlightRects"]
                .as_array()
                .is_some_and(|rectangles| !rectangles.is_empty())
        );
        assert_eq!(
            interaction["blockEnvironmentHighlightRects"],
            serde_json::json!([])
        );
        assert_eq!(
            interaction["inlineEnvironmentHighlightRects"],
            serde_json::json!([])
        );
        runtime.session.select_block_environment(section_id)?;
        let selected: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
        assert_eq!(selected["mathBlockHighlightRects"], serde_json::json!([]));
        assert_eq!(selected["mathTextHighlightRects"], serde_json::json!([]));
        assert_eq!(
            selected["blockEnvironmentHighlightRects"],
            serde_json::json!([])
        );
    }
    Ok(())
}

#[test]
fn overwide_block_math_text_highlight_is_clipped_to_the_page()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    document.body.push(Block::Equation(Equation {
        id: NodeId::generate(),
        math_tree: MathTree {
            schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
            root: MathFlow {
                id: NodeId::generate(),
                content: vec![MathContent::Text(TextFlow {
                    runs: vec![Run {
                        text: "x".repeat(300),
                        language: None,
                        props: BTreeMap::new(),
                        extra: BTreeMap::new(),
                    }],
                    extra: BTreeMap::new(),
                })],
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let (page_width, page_height, page_index, x_pt, y_pt) = {
        let snapshot = runtime.current_snapshot()?;
        let page = snapshot
            .pages
            .first()
            .ok_or_else(|| std::io::Error::other("math page is missing"))?;
        let fragment = page
            .layout
            .math_fragments
            .first()
            .ok_or_else(|| std::io::Error::other("equation fragment is missing"))?;
        let x = page.width_pt / 2.0;
        let top = page.margin_top_pt + f64::from(fragment.y_offset_pt);
        let bottom = top + f64::from(fragment.height_pt);
        let mut hit = None;
        let mut y = top;
        while y <= bottom.min(top + 30.0) && hit.is_none() {
            if snapshot.hit_test_math_text(0, x, y).is_some() {
                hit = Some((x, y));
            }
            y += 0.5;
        }
        let (x, y) = hit.ok_or_else(|| std::io::Error::other("wide TextFlow was not hittable"))?;
        (page.width_pt, page.height_pt, 0, x, y)
    };
    let placed: Value =
        serde_json::from_str(&runtime.place_caret_at_page_json(page_index, x_pt, y_pt)?)?;
    assert_eq!(placed["mathText"], true);

    let context = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("wide TextFlow input context is missing"))?;
    let target = context.target.clone();
    let island_start = context.island_start.clone();
    let island_end = context.island_end.clone();
    let raw_geometry = runtime
        .current_snapshot()?
        .math_text_selection_geometry(&target, &island_start, &island_end)
        .ok_or_else(|| std::io::Error::other("wide TextFlow geometry is missing"))?;
    assert!(
        raw_geometry
            .iter()
            .any(|rectangle| rectangle.x0_pt < 0.0 || rectangle.x1_pt > page_width)
    );
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    let rectangles = interaction["mathTextHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("clipped math highlight was omitted"))?;
    assert!(!rectangles.is_empty());
    assert!(rectangles.iter().all(|rectangle| {
        rectangle["pageIndex"] == page_index
            && rectangle["x0Pt"]
                .as_f64()
                .is_some_and(|x| (0.0..page_width).contains(&x))
            && rectangle["x1Pt"].as_f64().is_some_and(|x| x <= page_width)
            && rectangle["y0Pt"]
                .as_f64()
                .is_some_and(|y| (0.0..page_height).contains(&y))
            && rectangle["y1Pt"].as_f64().is_some_and(|y| y <= page_height)
    }));
    Ok(())
}

#[test]
fn inline_math_focus_uses_the_existing_inline_highlight_without_a_block_highlight()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime
        .session
        .run(caret_engine::Command::InsertInlineMath("x".to_owned()))?;
    runtime.session.run(caret_engine::Command::EnterMath(
        caret_engine::Direction::Backward,
    ))?;
    runtime.layout_snapshot_json()?;

    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    assert_eq!(
        interaction["mathBlockHighlightRects"],
        serde_json::json!([])
    );
    assert_eq!(interaction["mathTextHighlightRects"], serde_json::json!([]));
    assert!(
        interaction["inlineEnvironmentHighlightRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    Ok(())
}

#[test]
fn empty_align_cell_focus_publishes_the_shared_parent_block_highlight()
-> Result<(), Box<dyn std::error::Error>> {
    let align_id = NodeId::generate();
    let tree = MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: MathFlow {
            id: NodeId::generate(),
            content: Vec::new(),
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    let mut document = Document::blank();
    document.body.push(Block::Align(caret_doc::Align {
        id: align_id.clone(),
        rows: vec![caret_doc::MathAlignRow {
            id: NodeId::generate(),
            cells: vec![tree],
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    }));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let (x_pt, y_pt) = {
        let snapshot = runtime.current_snapshot()?;
        let page = &snapshot.pages[0];
        let fragment = page
            .layout
            .math_fragments
            .iter()
            .find(|fragment| fragment.math_id == align_id)
            .ok_or_else(|| std::io::Error::other("empty Align cell fragment is missing"))?;
        (
            page.margin_left_pt
                + f64::from(fragment.x_offset_pt)
                + f64::from(fragment.width_pt) / 2.0,
            page.margin_top_pt
                + f64::from(fragment.y_offset_pt)
                + f64::from(fragment.height_pt) / 2.0,
        )
    };
    let placed: Value = serde_json::from_str(&runtime.place_caret_at_page_json(0, x_pt, y_pt)?)?;
    assert_eq!(placed["mathTree"], true);
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    let block_rectangles = interaction["mathBlockHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("empty Align block highlight is missing"))?;
    assert_eq!(block_rectangles.len(), 1);
    assert_eq!(interaction["mathTextHighlightRects"], serde_json::json!([]));
    assert_eq!(
        interaction["blockEnvironmentHighlightRects"],
        serde_json::json!([])
    );
    Ok(())
}

#[test]
fn vertical_math_navigation_moves_between_align_rows_and_edits_the_new_cell()
-> Result<(), Box<dyn std::error::Error>> {
    let text_tree = |text: &str| caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![caret_doc::MathContent::Text(caret_doc::TextFlow {
                runs: vec![Run {
                    text: text.to_owned(),
                    language: None,
                    props: BTreeMap::new(),
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    let align = caret_doc::Align {
        id: NodeId::generate(),
        rows: vec![
            caret_doc::MathAlignRow {
                id: NodeId::generate(),
                cells: vec![text_tree("ab"), text_tree("x")],
                extra: BTreeMap::new(),
            },
            caret_doc::MathAlignRow {
                id: NodeId::generate(),
                cells: vec![text_tree("cd"), text_tree("y")],
                extra: BTreeMap::new(),
            },
        ],
        extra: BTreeMap::new(),
    };
    let mut document = Document::blank();
    document.body.push(Block::Align(align));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;

    let (page_index, x_pt, y_pt) = {
        let snapshot = runtime.current_snapshot()?;
        let mut hit_point = None;
        'pages: for (page_index, page) in snapshot.pages.iter().enumerate() {
            for fragment in &page.layout.math_fragments {
                let left = page.margin_left_pt + f64::from(fragment.x_offset_pt);
                let top = page.margin_top_pt + f64::from(fragment.y_offset_pt);
                let right = left + f64::from(fragment.width_pt);
                let bottom = top + f64::from(fragment.height_pt);
                let mut y = top;
                while y <= bottom {
                    let mut x = left;
                    while x <= right {
                        if snapshot
                            .hit_test_math_text(page_index, x, y)
                            .is_some_and(|hit| hit.position.path.align_cell() == Some((0, 0)))
                        {
                            hit_point = Some((page_index, x, y));
                            break 'pages;
                        }
                        x += 0.5;
                    }
                    y += 0.5;
                }
            }
        }
        hit_point.ok_or_else(|| std::io::Error::other("first Align text cell is not hittable"))?
    };
    let placed: Value =
        serde_json::from_str(&runtime.place_caret_at_page_json(page_index as i32, x_pt, y_pt)?)?;
    assert_eq!(placed["mathText"], true);
    runtime.move_vertical_caret_json(1, false)?;
    let context = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("vertical navigation left math text"))?;
    assert_eq!(context.focus.path.align_cell(), Some((1, 0)));
    assert!(context.surrounding_text.contains("cd"));
    runtime.insert_text_json("Z".to_owned())?;
    let context = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("typing left math TextFlow"))?;
    assert!(context.surrounding_text.contains('Z'));
    assert!(context.surrounding_text.contains("cd"));
    Ok(())
}

#[test]
fn empty_equation_host_keeps_hit_caret_edit_and_history_without_placeholder_box()
-> Result<(), Box<dyn std::error::Error>> {
    let equation_id = NodeId::generate();
    let math_tree = MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: MathFlow {
            id: NodeId::generate(),
            content: Vec::new(),
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(Paragraph::empty(NodeId::generate())),
        Block::Equation(caret_doc::Equation {
            id: equation_id.clone(),
            math_tree,
            extra: BTreeMap::new(),
        }),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;

    let (x_pt, y_pt) = {
        let snapshot = runtime.current_snapshot()?;
        let page = &snapshot.pages[0];
        let fragment = page
            .layout
            .math_fragments
            .iter()
            .find(|fragment| fragment.math_id == equation_id)
            .ok_or_else(|| std::io::Error::other("empty Equation fragment is missing"))?;
        let left = page.margin_left_pt + f64::from(fragment.x_offset_pt);
        let top = page.margin_top_pt + f64::from(fragment.y_offset_pt);
        assert!(fragment.width_pt > 0.0 && fragment.height_pt > 0.0);
        let glyphs = snapshot
            .page_glyphs(0)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        assert!(
            !glyphs
                .shapes
                .iter()
                .any(|shape| shape.kind == caret_engine::PageMathShapeKind::Rectangle)
        );
        (
            left + f64::from(fragment.width_pt) / 2.0,
            top + f64::from(fragment.height_pt) / 2.0,
        )
    };

    let placed: Value = serde_json::from_str(&runtime.place_caret_at_page_json(0, x_pt, y_pt)?)?;
    assert_eq!(placed["mathTree"], true);
    let empty_interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    assert!(
        empty_interaction["mathBlockHighlightRects"]
            .as_array()
            .is_some_and(|rectangles| rectangles.len() == 1)
    );
    assert_eq!(
        empty_interaction["blockEnvironmentHighlightRects"],
        serde_json::json!([])
    );
    let focus_before_edit = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("empty Equation did not receive tree focus"))?;
    assert_eq!(focus_before_edit.1.offset, 0);
    let caret: Value = serde_json::from_str(&runtime.current_caret_json()?)?;
    assert!(caret["y1Pt"].as_f64() > caret["y0Pt"].as_f64());

    runtime.insert_text_json("x".to_owned())?;
    assert!(matches!(
        &runtime.document().body[1],
        Block::Equation(equation) if !equation.math_tree.root.content.is_empty()
    ));
    runtime.undo_json()?;
    assert!(matches!(
        &runtime.document().body[1],
        Block::Equation(equation) if equation.math_tree.root.content.is_empty()
    ));
    assert!(runtime.session.math_tree_input_context().is_some());
    runtime.redo_json()?;
    assert!(matches!(
        &runtime.document().body[1],
        Block::Equation(equation) if !equation.math_tree.root.content.is_empty()
    ));
    Ok(())
}

#[test]
fn clicking_empty_math_operand_installs_tree_focus_and_accepts_text()
-> Result<(), Box<dyn std::error::Error>> {
    let empty_numerator = caret_doc::MathFlow {
        id: NodeId::generate(),
        content: Vec::new(),
        extra: BTreeMap::new(),
    };
    let denominator = caret_doc::MathFlow {
        id: NodeId::generate(),
        content: vec![caret_doc::MathContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: "x".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    };
    let math_tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![caret_doc::MathContent::Math(caret_doc::MathNode {
                id: NodeId::generate(),
                kind: caret_doc::MathNodeKind::Fraction,
                text: None,
                attributes: BTreeMap::from([("style".to_owned(), "frac".to_owned())]),
                children: vec![empty_numerator, denominator],
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    let mut document = Document::blank();
    document.body.push(Block::Equation(caret_doc::Equation {
        id: NodeId::generate(),
        math_tree,
        extra: BTreeMap::new(),
    }));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;

    let (x_pt, y_pt) = {
        let snapshot = runtime.current_snapshot()?;
        let page = &snapshot.pages[0];
        let fragment = page
            .layout
            .math_fragments
            .first()
            .ok_or_else(|| std::io::Error::other("equation fragment is missing"))?;
        let left = page.margin_left_pt + f64::from(fragment.x_offset_pt);
        let top = page.margin_top_pt + f64::from(fragment.y_offset_pt);
        let right = left + f64::from(fragment.width_pt);
        let bottom = top + f64::from(fragment.height_pt);
        let mut hit_point = None;
        let mut y = top;
        while y <= bottom && hit_point.is_none() {
            let mut x = left;
            while x <= right {
                if snapshot.hit_test_math_structure(0, x, y).is_some() {
                    hit_point = Some((x, y));
                    break;
                }
                x += 0.5;
            }
            y += 0.5;
        }
        hit_point.ok_or_else(|| std::io::Error::other("empty fraction operand was not hittable"))?
    };

    let placed: Value = serde_json::from_str(&runtime.place_caret_at_page_json(0, x_pt, y_pt)?)?;
    assert_eq!(placed["valid"], true);
    assert_eq!(placed["mathTree"], true);
    assert!(runtime.session.math_tree_input_context().is_some());
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    let full: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    let caret: Value = serde_json::from_str(&runtime.current_caret_json()?)?;
    assert_eq!(caret["xPt"], placed["caret"]["xPt"]);
    assert_eq!(caret["y0Pt"], placed["caret"]["y0Pt"]);
    assert_eq!(interaction["caretGeometry"]["xPt"], caret["xPt"]);
    assert_eq!(interaction["caretGeometry"]["fontSizeCentipoints"], 1200);
    assert_eq!(full["caretGeometry"]["xPt"], caret["xPt"]);
    assert_eq!(full["caretGeometry"]["fontSizeCentipoints"], 1200);

    let generation = runtime.document_generation();
    let inserted: Value = serde_json::from_str(&runtime.insert_text_json("n".to_owned())?)?;
    assert_eq!(runtime.document_generation(), generation + 1);
    assert!(matches!(
        runtime.session.math_edit().map(|edit| &edit.branch),
        Some(caret_engine::MathEditBranch::Tree { .. })
    ));
    let updated_caret: Value = serde_json::from_str(&runtime.current_caret_json()?)?;
    let updated_interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    let updated_full: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    assert!(updated_caret["xPt"].as_f64().is_some());
    assert_eq!(inserted["caretGeometry"]["xPt"], updated_caret["xPt"]);
    assert_eq!(
        updated_interaction["caretGeometry"]["xPt"],
        updated_caret["xPt"]
    );
    assert_eq!(updated_full["caretGeometry"]["xPt"], updated_caret["xPt"]);
    Ok(())
}

#[test]
fn nonempty_math_structure_can_be_hit_and_focused() -> Result<(), Box<dyn std::error::Error>> {
    let mut editing_runtime = DocumentViewRuntime::new_blank()?;
    editing_runtime.command_line_command_json("begin", String::new())?;
    editing_runtime.command_line_command_json("type", "math".to_owned())?;
    editing_runtime.command_line_command_json("accept", String::new())?;
    editing_runtime.insert_text_json("x".to_owned())?;
    let bytes = caret_doc::CdocFile::new(editing_runtime.session.document().clone())?.to_bytes()?;
    let opened = caret_doc::open(&bytes)?;
    let caret_doc::OpenedDocument::Editable(reopened) = opened else {
        return Err("saved math document should reopen as editable".into());
    };
    let mut runtime = DocumentViewRuntime::new(reopened.document().clone())?;
    runtime.layout_snapshot_json()?;

    let hit_point = {
        let snapshot = runtime.current_snapshot()?;
        let page = &snapshot.pages[0];
        let left = page.margin_left_pt;
        let top = page.margin_top_pt;
        let right = left + 120.0;
        let bottom = top + 100.0;
        let mut found = None;
        let mut y = top;
        while y <= bottom && found.is_none() {
            let mut x = left;
            while x <= right {
                if snapshot.hit_test_math_structure(0, x, y).is_some() {
                    found = Some((x, y));
                    break;
                }
                x += 1.0;
            }
            y += 1.0;
        }
        found.ok_or("nonempty math structure should be hittable")?
    };
    let placed: Value =
        serde_json::from_str(&runtime.place_caret_at_page_json(0, hit_point.0, hit_point.1)?)?;
    assert_eq!(placed["mathTree"], true);
    assert!(runtime.session.math_tree_input_context().is_some());
    Ok(())
}

#[test]
fn math_split_inline_publishes_visible_structure_geometry_and_restores_it_in_history()
-> Result<(), Box<dyn std::error::Error>> {
    let equation_id = NodeId::generate();
    let mut document = caret_doc::Document::blank();
    document.body.push(caret_doc::Block::Equation(Equation {
        id: equation_id.clone(),
        math_tree: MathTree {
            schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
            root: MathFlow {
                id: NodeId::generate(),
                content: vec![MathContent::Text(TextFlow {
                    runs: vec![Run {
                        text: "ab".to_owned(),
                        language: None,
                        props: BTreeMap::new(),
                        extra: BTreeMap::new(),
                    }],
                    extra: BTreeMap::new(),
                })],
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;

    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let split_caret = hit_map["stops"]
        .as_array()
        .and_then(|stops| {
            stops.iter().find(|stop| {
                stop["position"]["kind"] == "math"
                    && stop["position"]["orderUtf8"] == "1"
                    && stop["visualAffinity"] == "downstream"
            })
        })
        .ok_or("middle math text caret stop is missing")?;
    runtime.set_selection_json(
        &serde_json::json!({
            "anchor": split_caret["position"],
            "focus": split_caret["position"],
        })
        .to_string(),
    )?;
    assert!(runtime.session.math_text_input_context().is_some());

    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "math".to_owned())?;
    assert!(
        runtime
            .session
            .command_line_candidates()
            .iter()
            .any(|candidate| candidate.command_id == "math.split_inline" && candidate.available)
    );
    runtime.command_line_command_json("accept", String::new())?;
    assert!(matches!(
        runtime.session.math_edit().map(|edit| &edit.branch),
        Some(caret_engine::MathEditBranch::Tree { .. })
    ));

    let published: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    let caret = &published["caretGeometry"];
    let caret_top = caret["y0Pt"].as_f64().ok_or("tree caret has no top edge")?;
    let caret_bottom = caret["y1Pt"]
        .as_f64()
        .ok_or("tree caret has no bottom edge")?;
    assert!(
        caret_bottom > caret_top,
        "split caret must have visible height"
    );
    let snapshot = runtime.current_snapshot()?;
    let math_layout = snapshot
        .math_blocks
        .get(&equation_id)
        .ok_or("split equation layout is missing")?;
    assert!(math_layout.width > 0.0 && math_layout.ascent + math_layout.descent > 0.0);
    assert!(
        snapshot
            .hit_test_math_structure(
                0,
                caret["xPt"].as_f64().ok_or("tree caret has no x")?,
                (caret_top + caret_bottom) / 2.0
            )
            .is_some(),
        "split structure caret must remain hittable in the published snapshot"
    );

    runtime.session.run(caret_engine::Command::Undo)?;
    let undone: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    assert!(undone["caretGeometry"]["y1Pt"].as_f64() > undone["caretGeometry"]["y0Pt"].as_f64());
    runtime.session.run(caret_engine::Command::Redo)?;
    let redone: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    assert!(redone["caretGeometry"]["y1Pt"].as_f64() > redone["caretGeometry"]["y0Pt"].as_f64());
    Ok(())
}

#[test]
fn leaving_math_text_inside_environment_publishes_tree_caret_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    let text_flow = caret_doc::TextFlow {
        runs: vec![Run {
            text: "a\nb".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    };
    let mut document = Document::blank();
    let equation_id = NodeId::generate();
    document.body.push(Block::Equation(caret_doc::Equation {
        id: equation_id,
        math_tree: caret_doc::MathTree {
            schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
            root: caret_doc::MathFlow {
                id: NodeId::generate(),
                content: vec![caret_doc::MathContent::Environment(InlineEnvironment {
                    id: NodeId::generate(),
                    kind: InlineEnvironmentKind::Bold,
                    definition_id: None,
                    props: BTreeMap::new(),
                    children: vec![
                        caret_doc::MathContent::Text(text_flow),
                        caret_doc::MathContent::Math(caret_doc::MathNode {
                            id: NodeId::generate(),
                            kind: caret_doc::MathNodeKind::Symbol,
                            text: Some("x".to_owned()),
                            attributes: BTreeMap::new(),
                            children: Vec::new(),
                            extra: BTreeMap::new(),
                        }),
                    ],
                    extra: BTreeMap::new(),
                })],
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;

    let (x_pt, y_pt, math_height) = {
        let snapshot = runtime.current_snapshot()?;
        let page = &snapshot.pages[0];
        let fragment = page
            .layout
            .math_fragments
            .first()
            .ok_or_else(|| std::io::Error::other("equation fragment is missing"))?;
        let left = page.margin_left_pt + f64::from(fragment.x_offset_pt);
        let top = page.margin_top_pt + f64::from(fragment.y_offset_pt);
        let right = left + f64::from(fragment.width_pt);
        let bottom = top + f64::from(fragment.height_pt);
        let mut hit_point = None;
        let mut y = top;
        while y <= bottom && hit_point.is_none() {
            let mut x = left;
            while x <= right {
                if snapshot.hit_test_math_text(0, x, y).is_some() {
                    hit_point = Some((x, y));
                    break;
                }
                x += 0.5;
            }
            y += 0.5;
        }
        let (x_pt, y_pt) = hit_point
            .ok_or_else(|| std::io::Error::other("environment TextFlow was not hittable"))?;
        (x_pt, y_pt, f64::from(fragment.height_pt))
    };
    let placed: Value = serde_json::from_str(&runtime.place_caret_at_page_json(0, x_pt, y_pt)?)?;
    assert_eq!(placed["mathText"], true);
    runtime
        .session
        .run(caret_engine::Command::SetMathTextSelection {
            anchor: 3,
            focus: 3,
        })?;
    let moved: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, false)?)?;
    assert!(runtime.session.math_tree_input_context().is_some());
    let caret_height = moved["caretGeometry"]["y1Pt"]
        .as_f64()
        .ok_or_else(|| std::io::Error::other("tree caret has no lower edge"))?
        - moved["caretGeometry"]["y0Pt"]
            .as_f64()
            .ok_or_else(|| std::io::Error::other("tree caret has no upper edge"))?;
    assert!(caret_height < math_height);

    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    let full: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    let caret: Value = serde_json::from_str(&runtime.current_caret_json()?)?;
    assert_eq!(interaction["caretGeometry"]["xPt"], caret["xPt"]);
    assert_eq!(full["caretGeometry"]["xPt"], caret["xPt"]);
    assert!(caret["y1Pt"].as_f64() > caret["y0Pt"].as_f64());
    Ok(())
}

#[test]
fn space_only_math_tree_keeps_a_published_tree_caret() -> Result<(), Box<dyn std::error::Error>> {
    for source in [r"\,", r"\!"] {
        let mut runtime = DocumentViewRuntime::new_blank()?;
        runtime
            .session
            .run(caret_engine::Command::InsertInlineMath(source.to_owned()))?;
        runtime.session.run(caret_engine::Command::EnterMath(
            caret_engine::Direction::Backward,
        ))?;

        let full: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
        let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
        let caret: Value = serde_json::from_str(&runtime.current_caret_json()?)?;
        assert!(runtime.session.math_tree_input_context().is_some());
        assert!(caret["xPt"].as_f64().is_some(), "{source} has no caret x");
        let caret_height = caret["y1Pt"]
            .as_f64()
            .ok_or_else(|| std::io::Error::other("space-only math caret has no lower edge"))?
            - caret["y0Pt"]
                .as_f64()
                .ok_or_else(|| std::io::Error::other("space-only math caret has no upper edge"))?;
        assert!(caret_height > 0.0, "{source} has no caret height");
        assert_eq!(full["caretGeometry"]["xPt"], caret["xPt"]);
        assert_eq!(interaction["caretGeometry"]["xPt"], caret["xPt"]);
    }
    Ok(())
}

#[test]
fn inserting_mandatory_break_into_math_text_commits_and_publishes_both_lines()
-> Result<(), Box<dyn std::error::Error>> {
    let math_tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![caret_doc::MathContent::Text(caret_doc::TextFlow {
                runs: vec![Run {
                    text: "ab".to_owned(),
                    language: None,
                    props: BTreeMap::new(),
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    let mut document = Document::blank();
    let equation_id = NodeId::generate();
    document.body.push(Block::Equation(caret_doc::Equation {
        id: equation_id.clone(),
        math_tree,
        extra: BTreeMap::new(),
    }));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;

    let (x_pt, y_pt) = {
        let snapshot = runtime.current_snapshot()?;
        let page = &snapshot.pages[0];
        let fragment = page
            .layout
            .math_fragments
            .first()
            .ok_or_else(|| std::io::Error::other("equation fragment is missing"))?;
        let left = page.margin_left_pt + f64::from(fragment.x_offset_pt);
        let top = page.margin_top_pt + f64::from(fragment.y_offset_pt);
        let right = left + f64::from(fragment.width_pt);
        let bottom = top + f64::from(fragment.height_pt);
        let mut hit_point = None;
        let mut y = top;
        while y <= bottom && hit_point.is_none() {
            let mut x = left;
            while x <= right {
                if snapshot
                    .hit_test_math_text(0, x, y)
                    .is_some_and(|hit| hit.position.offset == 1)
                {
                    hit_point = Some((x, y));
                    break;
                }
                x += 0.5;
            }
            y += 0.5;
        }
        hit_point.ok_or_else(|| std::io::Error::other("equation TextFlow was not hittable"))?
    };
    let placed: Value = serde_json::from_str(&runtime.place_caret_at_page_json(0, x_pt, y_pt)?)?;
    assert_eq!(placed["mathText"], true);
    let generation_before = runtime.document_generation();
    runtime.insert_text_json("\u{0085}".to_owned())?;

    assert_eq!(runtime.document_generation(), generation_before + 1);
    let context = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("mandatory break left the math text branch"))?;
    assert!(context.surrounding_text.contains('\u{0085}'));
    let caret: Value = serde_json::from_str(&runtime.current_caret_json()?)?;
    assert!(caret["y0Pt"].as_f64() > placed["caret"]["y0Pt"].as_f64());
    let second_line_hit = runtime
        .current_snapshot()?
        .hit_test_math_text(
            0,
            caret["xPt"].as_f64().ok_or("caret x is missing")?,
            (caret["y0Pt"].as_f64().ok_or("caret y0 is missing")?
                + caret["y1Pt"].as_f64().ok_or("caret y1 is missing")?)
                / 2.0,
        )
        .ok_or_else(|| std::io::Error::other("second line text was not hittable"))?;
    assert_eq!(second_line_hit.position.offset, 3);
    let page = runtime
        .current_snapshot()?
        .page_glyphs(0)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let baselines = page
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.baseline_y_pt))
        .collect::<Vec<_>>();
    let min_baseline = baselines
        .iter()
        .copied()
        .reduce(f64::min)
        .ok_or_else(|| std::io::Error::other("first-line glyph is missing"))?;
    let max_baseline = baselines
        .iter()
        .copied()
        .reduce(f64::max)
        .ok_or_else(|| std::io::Error::other("second-line glyph is missing"))?;
    assert!(
        max_baseline - min_baseline > 5.0,
        "glyph baselines: {baselines:?}; text: {:?}; generation: {}",
        context.surrounding_text,
        runtime.document_generation()
    );
    Ok(())
}

#[test]
fn inline_environment_highlight_uses_layout_geometry_and_replaces_block_highlight()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        return Err("blank document should start with a paragraph".into());
    };
    let prefix = "outside ";
    let text = "Latin العربية 日本語 mixed content ".repeat(24);
    let paragraph_id = paragraph.id.clone();
    paragraph.inlines = vec![
        text_flow(Run {
            text: prefix.to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: BTreeMap::new(),
            children: vec![text_flow(Run {
                text: text.clone(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        }),
        text_flow(Run {
            text: " outside".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let caret = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: prefix.len(),
        affinity: caret_engine::Affinity::Downstream,
    };
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: caret.clone(),
                focus: caret,
            },
        )))?;

    let snapshot: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    let rectangles = snapshot["inlineEnvironmentHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("inline environment rectangles were omitted"))?;
    assert!(
        rectangles.len() > 1,
        "wrapped text should produce per-line rectangles"
    );
    assert_eq!(
        snapshot["blockEnvironmentHighlightRects"],
        serde_json::json!([])
    );
    assert!(rectangles.iter().all(|rectangle| {
        rectangle["x1Pt"].as_f64() > rectangle["x0Pt"].as_f64()
            && rectangle["y1Pt"].as_f64() > rectangle["y0Pt"].as_f64()
    }));
    let range = runtime
        .session
        .current_inline_environment()?
        .ok_or_else(|| std::io::Error::other("caret did not resolve the inline environment"))?;
    assert_eq!(range.start_utf8, prefix.len());
    assert_eq!(range.end_utf8, prefix.len() + text.len());
    let selection = caret_engine::Selection {
        anchor: caret_engine::Pos {
            story: range.story.clone(),
            paragraph_id: range.paragraph_id.clone(),
            offset: range.start_utf8,
            affinity: caret_engine::Affinity::Downstream,
        },
        focus: caret_engine::Pos {
            story: range.story,
            paragraph_id: range.paragraph_id,
            offset: range.end_utf8,
            affinity: caret_engine::Affinity::Upstream,
        },
    };
    let expected = runtime
        .current_snapshot()?
        .selection_geometry(&selection)
        .ok_or_else(|| std::io::Error::other("bidi environment selection has no geometry"))?
        .into_iter()
        .map(|rectangle| {
            serde_json::json!({
                "pageIndex": rectangle.page_index,
                "x0Pt": rectangle.x0_pt,
                "y0Pt": rectangle.y0_pt,
                "x1Pt": rectangle.x1_pt,
                "y1Pt": rectangle.y1_pt,
            })
        })
        .collect::<Vec<_>>();
    let actual = snapshot["inlineEnvironmentHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("inline highlight snapshot is not an array"))?;
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual["pageIndex"], expected["pageIndex"]);
        for coordinate in ["x0Pt", "y0Pt", "x1Pt", "y1Pt"] {
            let actual = actual[coordinate]
                .as_f64()
                .ok_or_else(|| std::io::Error::other("inline rectangle coordinate is missing"))?;
            let expected = expected[coordinate]
                .as_f64()
                .ok_or_else(|| std::io::Error::other("expected rectangle coordinate is missing"))?;
            assert!(
                (actual - expected).abs() < 1e-6,
                "{coordinate} differs: {actual} != {expected}"
            );
        }
    }

    let outside = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id,
        offset: prefix.len() + text.len(),
        affinity: caret_engine::Affinity::Downstream,
    };
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: outside.clone(),
                focus: outside,
            },
        )))?;
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    assert_eq!(
        interaction["inlineEnvironmentHighlightRects"],
        serde_json::json!([])
    );
    Ok(())
}

#[test]
fn run_property_environment_highlight_uses_its_inline_range()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        return Err("blank document should start with a paragraph".into());
    };
    let paragraph_id = paragraph.id.clone();
    paragraph.inlines = vec![text_flow(Run {
        text: "underlined text".to_owned(),
        language: None,
        props: BTreeMap::from([("underline".to_owned(), Value::Bool(true))]),
        extra: BTreeMap::new(),
    })];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let caret = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id,
        offset: 3,
        affinity: caret_engine::Affinity::Downstream,
    };
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: caret.clone(),
                focus: caret,
            },
        )))?;

    let view = runtime.session.current_environment_view()?;
    assert_eq!(
        view.innermost_inline().map(|entry| entry.source),
        Some(caret_engine::EnvironmentSource::RunProperty)
    );
    assert_eq!(
        view.innermost_inline().map(|entry| entry.kind),
        Some(caret_engine::EnvironmentKind::Underline)
    );

    let snapshot: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    let rectangles = snapshot["inlineEnvironmentHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("inline environment rectangles were omitted"))?;
    assert_eq!(rectangles.len(), 1);
    assert!(rectangles[0]["x1Pt"].as_f64() > rectangles[0]["x0Pt"].as_f64());
    Ok(())
}

#[test]
fn empty_inline_environment_does_not_hide_visible_block_highlight()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let section_id = NodeId::generate();
    let mut paragraph = Paragraph::empty(NodeId::generate());
    let paragraph_id = paragraph.id.clone();
    paragraph.inlines = vec![ParagraphContent::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::new(),
        children: Vec::new(),
        extra: BTreeMap::new(),
    })];
    document.body = vec![Block::Environment(BlockEnvironment {
        id: section_id,
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(Paragraph::empty(NodeId::generate())),
        children: vec![Block::Paragraph(paragraph)],
        extra: BTreeMap::new(),
    })];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let position = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id,
        offset: 0,
        affinity: caret_engine::Affinity::Downstream,
    };
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: position.clone(),
                focus: position,
            },
        )))?;

    let snapshot: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    assert_eq!(
        snapshot["inlineEnvironmentHighlightRects"],
        serde_json::json!([])
    );
    assert!(
        snapshot["blockEnvironmentHighlightRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    Ok(())
}

#[test]
fn block_environment_geometry_is_emitted_as_page_fragments()
-> Result<(), Box<dyn std::error::Error>> {
    let environment_id = NodeId::generate();
    let children = (0..80)
        .map(|_| Block::Paragraph(Paragraph::empty(NodeId::generate())))
        .collect();
    let mut document = Document::blank();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(Paragraph::empty(NodeId::generate())),
        children,
        extra: BTreeMap::new(),
    })];

    let mut runtime = DocumentViewRuntime::new(document)?;
    let snapshot: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    let ranges = snapshot["blockEnvironmentHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("focus rectangles were omitted"))?;

    let page_indices = ranges
        .iter()
        .filter_map(|range| range["pageIndex"].as_u64())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(page_indices.len() > 1, "the environment should span pages");
    assert!(ranges.iter().all(|range| {
        range["x1Pt"].as_f64() > range["x0Pt"].as_f64()
            && range["y1Pt"].as_f64() > range["y0Pt"].as_f64()
    }));

    let layout = runtime.current_snapshot()?;
    let environment_ranges = layout
        .environment_ranges
        .iter()
        .filter(|range| range.environment_id == environment_id)
        .collect::<Vec<_>>();
    assert_eq!(ranges.len(), environment_ranges.len());
    assert_eq!(environment_ranges.len(), page_indices.len());
    for (snapshot_range, layout_range) in ranges.iter().zip(&environment_ranges) {
        assert_eq!(
            snapshot_range["pageIndex"].as_u64(),
            Some(layout_range.page_index as u64)
        );
        assert_eq!(snapshot_range["x0Pt"].as_f64(), Some(layout_range.x0_pt));
        assert_eq!(snapshot_range["y0Pt"].as_f64(), Some(layout_range.y0_pt));
        assert_eq!(snapshot_range["x1Pt"].as_f64(), Some(layout_range.x1_pt));
        assert_eq!(snapshot_range["y1Pt"].as_f64(), Some(layout_range.y1_pt));
        let page = &layout.pages[layout_range.page_index];
        assert!(layout_range.x0_pt >= 0.0 && layout_range.x1_pt <= page.width_pt);
        assert!(layout_range.y0_pt >= 0.0 && layout_range.y1_pt <= page.height_pt);
    }
    assert!(environment_ranges.windows(2).all(|ranges| {
        ranges[0].page_index < ranges[1].page_index
            && ranges[0].y0_pt < ranges[0].y1_pt
            && ranges[1].y0_pt < ranges[1].y1_pt
    }));
    Ok(())
}

#[test]
fn visible_page_rasterization_returns_binary_glyph_images() -> Result<(), Box<dyn std::error::Error>>
{
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should contain a paragraph");
    };
    paragraph.inlines = vec![text_flow(Run {
        text: "A".to_owned(),
        language: Some("en".to_owned()),
        props: Default::default(),
        extra: Default::default(),
    })];
    let mut runtime = DocumentViewRuntime::new(document)?;

    let page = runtime.rasterize_page_glyphs(0, 1.0, [0.25, 0.75], [32, 32, 32, 255])?;
    assert_eq!(page.page_index, 0);
    assert_eq!(page.document_generation, 0);
    assert!(page.font_generation > 0);
    assert_eq!(page.images.len(), 1);
    assert_eq!(page.placements.len(), 1);
    assert_eq!(page.placements[0].image_index, 0);
    assert!(page.images[0].pixels.len() > 4);
    assert!(page.images[0].pixels.as_chunks::<4>().1.is_empty());
    Ok(())
}

#[test]
fn rejected_edit_and_invalid_page_requests_preserve_the_published_document()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    let snapshot = runtime.insert_text_json("valid".to_owned())?;
    let selection = runtime.session.selection().clone();

    assert_eq!(
        runtime.insert_text_json("invalid\ninput".to_owned()),
        Err("text.insert: edit.newline_requires_paragraph_command".to_owned())
    );
    assert_eq!(
        runtime.rasterize_page_glyphs(usize::MAX, 1.0, [0.0, 0.0], [0, 0, 0, 255]),
        Err("layout.glyphs.invalid_page_index".to_owned())
    );
    assert_eq!(
        runtime.rasterize_page_glyphs(0, f64::NAN, [0.0, 0.0], [0, 0, 0, 255]),
        Err("layout.glyphs.invalid_raster_geometry".to_owned())
    );
    assert_eq!(runtime.place_caret_at_page_json(0, f64::NAN, 73.0)?, "null");
    assert_eq!(runtime.session.selection(), &selection);
    assert_eq!(runtime.layout_snapshot_json()?, snapshot);
    Ok(())
}

#[test]
fn ordinary_same_paragraph_input_reuses_the_rest_of_the_document_layout()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("first paragraph".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("unchanged paragraph".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("active paragraph".to_owned())?;

    let updated: Value = serde_json::from_str(&runtime.insert_text_json("!".to_owned())?)?;
    assert_eq!(updated["paragraphs"][0]["text"], "first paragraph");
    assert_eq!(updated["paragraphs"][1]["text"], "unchanged paragraph");
    assert_eq!(updated["paragraphs"][2]["text"], "active paragraph!");

    let incremental = runtime
        .last_snapshot
        .as_ref()
        .ok_or("incremental layout snapshot was not installed")?;
    let full = runtime.layout.layout_session(&runtime.session)?;
    assert_eq!(
        caret_engine::DocumentLayoutView::from(incremental),
        caret_engine::DocumentLayoutView::from(&full)
    );
    Ok(())
}

#[test]
fn paragraph_split_and_single_paragraph_paste_reuse_untouched_layouts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("first paragraph".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("active paragraph".to_owned())?;

    let split: Value = serde_json::from_str(&runtime.split_paragraph_json()?)?;
    assert_eq!(split["paragraphs"].as_array().map(Vec::len), Some(3));
    let split_layout = runtime
        .last_snapshot
        .as_ref()
        .ok_or("split layout snapshot was not installed")?;
    let full_after_split = runtime.layout.layout_session(&runtime.session)?;
    assert_eq!(
        caret_engine::DocumentLayoutView::from(split_layout),
        caret_engine::DocumentLayoutView::from(&full_after_split)
    );

    let pasted: Value =
        serde_json::from_str(&runtime.paste_text_json("multi-line\npaste".to_owned())?)?;
    assert_eq!(pasted["paragraphs"].as_array().map(Vec::len), Some(4));
    let paste_layout = runtime
        .last_snapshot
        .as_ref()
        .ok_or("paste layout snapshot was not installed")?;
    let full_after_paste = runtime.layout.layout_session(&runtime.session)?;
    assert_eq!(
        caret_engine::DocumentLayoutView::from(paste_layout),
        caret_engine::DocumentLayoutView::from(&full_after_paste)
    );
    Ok(())
}

#[test]
fn ordinary_input_delta_contains_only_the_changed_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("first".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("second".to_owned())?;

    let delta: Value = serde_json::from_str(&runtime.insert_text_delta_json("!".to_owned())?)?;
    assert_eq!(delta["partial"], true);
    assert_eq!(delta["paragraphs"].as_array().map(Vec::len), Some(1));
    assert_eq!(delta["paragraphs"][0]["text"], "second!");
    assert_eq!(delta["pages"].as_array().map(Vec::len), Some(1));

    let full: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    assert_eq!(full["paragraphs"].as_array().map(Vec::len), Some(2));
    assert_eq!(full["paragraphs"][0]["text"], "first");
    assert_eq!(full["paragraphs"][1]["text"], "second!");
    Ok(())
}

#[test]
fn invalid_model_is_rejected_and_unknown_content_does_not_publish_a_partial_layout()
-> Result<(), Box<dyn std::error::Error>> {
    let mut invalid = Document::blank();
    invalid.page_sections.clear();
    assert!(matches!(
        DocumentViewRuntime::new(invalid),
        Err(error) if error.starts_with("edit.invalid_document:")
    ));

    let mut future = Document::blank();
    future.body.push(Block::Opaque(caret_doc::OpaqueNode {
        raw: serde_json::json!({"type": "futureBlock", "payload": true}),
    }));
    let mut runtime = DocumentViewRuntime::new(future)?;
    assert_eq!(
        runtime.layout_snapshot_json(),
        Err("layout.unsupported_content".to_owned())
    );
    assert!(runtime.last_snapshot.is_none());
    assert_eq!(runtime.session.document_generation(), 0);
    Ok(())
}

/// 构造带一个文本片段的段落。
fn paragraph_with_id(id: &NodeId, text: &str) -> Paragraph {
    let mut paragraph = Paragraph::empty(id.clone());
    paragraph.inlines = vec![ParagraphContent::Text(TextFlow {
        runs: vec![Run {
            text: text.to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    })];
    paragraph
}

/// 构造带一个文本片段的显示公式块。
fn equation_block(id: &NodeId, text: &str) -> Block {
    Block::Equation(Equation {
        id: id.clone(),
        math_tree: MathTree {
            schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
            root: MathFlow {
                id: NodeId::generate(),
                content: vec![MathContent::Text(TextFlow {
                    runs: vec![Run {
                        text: text.to_owned(),
                        language: None,
                        props: BTreeMap::new(),
                        extra: BTreeMap::new(),
                    }],
                    extra: BTreeMap::new(),
                })],
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    })
}

fn collapsed_selection(paragraph_id: &NodeId, offset: usize) -> caret_engine::Selection {
    let position = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset,
        affinity: caret_engine::Affinity::Upstream,
    };
    caret_engine::Selection {
        anchor: position.clone(),
        focus: position,
    }
}

fn rectangle_close(
    actual: &(i64, f64, f64, f64, f64),
    expected: (i64, f64, f64, f64, f64),
) -> bool {
    actual.0 == expected.0
        && (actual.1 - expected.1).abs() < 1e-9
        && (actual.2 - expected.2).abs() < 1e-9
        && (actual.3 - expected.3).abs() < 1e-9
        && (actual.4 - expected.4).abs() < 1e-9
}

/// 连续扩选直到公式块被结构范围覆盖；单步的边界停靠不属于本测试的契约。
fn extend_until_math_selected(
    runtime: &mut DocumentViewRuntime,
    forward: bool,
    math_id: &NodeId,
) -> Result<(), Box<dyn std::error::Error>> {
    for _ in 0..4 {
        if runtime.session.selected_math_blocks() == std::slice::from_ref(math_id) {
            return Ok(());
        }
        runtime.move_visual_caret_json(forward, true)?;
    }
    Err(std::io::Error::other("structural extension did not cover the equation").into())
}

fn rectangle_tuples(value: &Value) -> Vec<(i64, f64, f64, f64, f64)> {
    value
        .as_array()
        .map(|rectangles| {
            rectangles
                .iter()
                .map(|rectangle| {
                    (
                        rectangle["pageIndex"].as_i64().unwrap_or_default(),
                        rectangle["x0Pt"].as_f64().unwrap_or_default(),
                        rectangle["y0Pt"].as_f64().unwrap_or_default(),
                        rectangle["x1Pt"].as_f64().unwrap_or_default(),
                        rectangle["y1Pt"].as_f64().unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn structural_selection_geometry_covers_equation_hosts_across_host_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let before_id = NodeId::generate();
    let after_id = NodeId::generate();
    let math_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_id(&before_id, "before")),
        equation_block(&math_id, "x"),
        Block::Paragraph(paragraph_with_id(&after_id, "after")),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let page = runtime.current_snapshot()?.pages[0].clone();
    let math_rectangles = runtime
        .current_snapshot()?
        .math_block_geometry(std::slice::from_ref(&math_id))
        .ok_or_else(|| std::io::Error::other("equation fragment is missing"))?;
    assert_eq!(math_rectangles.len(), 1);
    let math_rectangle = math_rectangles[0];
    let math_tuple = (
        i64::try_from(math_rectangle.page_index)?,
        math_rectangle.x0_pt,
        math_rectangle.y0_pt,
        math_rectangle.x1_pt,
        math_rectangle.y1_pt,
    );
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            collapsed_selection(&before_id, "before".len()),
        )))?;

    // 第一次 Shift+Right：公式整体进入结构选区，选区层由公式整栏矩形表示。
    let first: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, true)?)?;
    assert_eq!(
        runtime.session.selected_math_blocks(),
        std::slice::from_ref(&math_id)
    );
    assert!(
        runtime.session.structure_selection_range().is_some(),
        "cross-host selection must publish structural endpoints"
    );
    let first_rectangles = rectangle_tuples(&first["selectionRects"]);
    assert_eq!(
        first_rectangles.len(),
        1,
        "the selected equation must be the only visible selection geometry"
    );
    assert!(
        rectangle_close(&first_rectangles[0], math_tuple),
        "selection geometry must match the equation host rectangle"
    );
    assert!(
        (math_rectangle.x1_pt - (page.width_pt - page.margin_right_pt)).abs() < 1e-9
            && (math_rectangle.x0_pt - page.margin_left_pt).abs() < 1e-9,
        "the equation selection must span the page content width"
    );
    assert_eq!(
        first["mathBlockHighlightRects"],
        serde_json::json!([]),
        "a structural selection belongs to the selection layer, not the block highlight layer"
    );

    // 继续 Shift+Right：结构范围保留公式并延伸到后一段文字。
    runtime.move_visual_caret_json(true, true)?;
    assert_eq!(
        runtime.session.selected_math_blocks(),
        std::slice::from_ref(&math_id)
    );
    assert_eq!(runtime.session.selection().focus.paragraph_id, after_id);
    let extended: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, true)?)?;
    let extended_rectangles = rectangle_tuples(&extended["selectionRects"]);
    assert!(
        extended_rectangles
            .iter()
            .any(|rectangle| rectangle_close(rectangle, math_tuple)),
        "the equation rectangle must survive further extension"
    );
    assert!(
        extended_rectangles
            .iter()
            .any(|(_, x0, _, x1, _)| { (x1 - x0) < (math_rectangle.x1_pt - math_rectangle.x0_pt) }),
        "extension into the following paragraph must add its text rectangle"
    );
    Ok(())
}

#[test]
fn block_edge_structural_selection_geometry_resolves_without_a_preceding_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let after_id = NodeId::generate();
    let math_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![
        equation_block(&math_id, "x"),
        Block::Paragraph(paragraph_with_id(&after_id, "after")),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let math_rectangle = runtime
        .current_snapshot()?
        .math_block_geometry(std::slice::from_ref(&math_id))
        .and_then(|rectangles| rectangles.first().copied())
        .ok_or_else(|| std::io::Error::other("equation fragment is missing"))?;
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            collapsed_selection(&after_id, 0),
        )))?;

    extend_until_math_selected(&mut runtime, false, &math_id)?;
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    assert!(
        runtime
            .session
            .structure_selection_range()
            .is_some_and(|(anchor, focus)| {
                [anchor, focus].into_iter().any(|endpoint| {
                    matches!(
                        endpoint,
                        caret_engine::DocumentRangeEndpoint::BlockEdge {
                            side: caret_engine::Direction::Backward,
                            ..
                        }
                    )
                })
            }),
        "an equation without a preceding paragraph must use the block backward edge"
    );
    let bounds = runtime
        .session
        .structure_selection_text_bounds()
        .ok_or_else(|| std::io::Error::other("block edge text bounds are missing"))?;
    assert_eq!(bounds.0.paragraph_id, after_id);
    assert_eq!(bounds.1.paragraph_id, after_id);
    let interaction_rectangles = rectangle_tuples(&interaction["selectionRects"]);
    assert_eq!(interaction_rectangles.len(), 1);
    assert!(rectangle_close(
        &interaction_rectangles[0],
        (
            i64::try_from(math_rectangle.page_index)?,
            math_rectangle.x0_pt,
            math_rectangle.y0_pt,
            math_rectangle.x1_pt,
            math_rectangle.y1_pt,
        )
    ));
    Ok(())
}

#[test]
fn block_edge_forward_endpoint_projects_a_trailing_equation_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let before_id = NodeId::generate();
    let math_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_id(&before_id, "before")),
        equation_block(&math_id, "x"),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let math_rectangle = runtime
        .current_snapshot()?
        .math_block_geometry(std::slice::from_ref(&math_id))
        .and_then(|rectangles| rectangles.first().copied())
        .ok_or_else(|| std::io::Error::other("equation fragment is missing"))?;
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            collapsed_selection(&before_id, "before".len()),
        )))?;

    extend_until_math_selected(&mut runtime, true, &math_id)?;
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    let bounds = runtime
        .session
        .structure_selection_text_bounds()
        .ok_or_else(|| std::io::Error::other("block edge text bounds are missing"))?;
    assert_eq!(bounds.0.paragraph_id, before_id);
    assert_eq!(bounds.1.paragraph_id, before_id);
    assert!(
        runtime
            .session
            .structure_selection_range()
            .is_some_and(|(anchor, focus)| {
                [anchor, focus].into_iter().any(|endpoint| {
                    matches!(
                        endpoint,
                        caret_engine::DocumentRangeEndpoint::BlockEdge {
                            side: caret_engine::Direction::Forward,
                            ..
                        }
                    )
                })
            }),
        "a trailing equation selection must include the block forward edge"
    );
    let rectangles = rectangle_tuples(&interaction["selectionRects"]);
    assert_eq!(rectangles.len(), 1);
    assert!(rectangle_close(
        &rectangles[0],
        (
            i64::try_from(math_rectangle.page_index)?,
            math_rectangle.x0_pt,
            math_rectangle.y0_pt,
            math_rectangle.x1_pt,
            math_rectangle.y1_pt,
        )
    ));
    Ok(())
}

#[test]
fn equation_selection_inside_a_section_suppresses_block_and_inline_highlights()
-> Result<(), Box<dyn std::error::Error>> {
    let section_id = NodeId::generate();
    let title_id = NodeId::generate();
    let p0_id = NodeId::generate();
    let math_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: section_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(paragraph_with_id(&title_id, "title")),
        children: vec![
            Block::Paragraph(paragraph_with_id(&p0_id, "p0")),
            equation_block(&math_id, "x"),
        ],
        extra: BTreeMap::new(),
    })];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let math_rectangle = runtime
        .current_snapshot()?
        .math_block_geometry(std::slice::from_ref(&math_id))
        .and_then(|rectangles| rectangles.first().copied())
        .ok_or_else(|| std::io::Error::other("equation fragment is missing"))?;
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            collapsed_selection(&p0_id, "p0".len()),
        )))?;

    extend_until_math_selected(&mut runtime, true, &math_id)?;
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;

    // 该范围一端是公式块外侧边界：`structure_selection()` 为 `None`，但结构选区实际存在。
    assert!(runtime.session.has_structure_selection());
    assert!(runtime.session.structure_selection().is_none());
    assert!(
        runtime
            .session
            .structure_selection_range()
            .is_some_and(|(_, focus)| matches!(
                focus,
                caret_engine::DocumentRangeEndpoint::BlockEdge { .. }
            )),
        "the regression case needs a block edge endpoint"
    );
    assert_eq!(
        runtime.session.selected_math_blocks(),
        std::slice::from_ref(&math_id)
    );
    assert!(runtime.session.selected_block_environment().is_none());
    // 结构选区激活时不得回落到外层 Section 的块高亮、公式块高亮或行内空槽。
    assert_eq!(
        interaction["blockEnvironmentHighlightRects"],
        serde_json::json!([])
    );
    assert_eq!(
        interaction["mathBlockHighlightRects"],
        serde_json::json!([])
    );
    assert_eq!(
        interaction["inlineEnvironmentHighlightRects"],
        serde_json::json!([])
    );
    assert!(!runtime.session.has_pending_inline_style());
    // 选中的公式只由选区层对象矩形呈现。
    let rectangles = rectangle_tuples(&interaction["selectionRects"]);
    assert_eq!(rectangles.len(), 1);
    assert!(rectangle_close(
        &rectangles[0],
        (
            i64::try_from(math_rectangle.page_index)?,
            math_rectangle.x0_pt,
            math_rectangle.y0_pt,
            math_rectangle.x1_pt,
            math_rectangle.y1_pt,
        )
    ));
    Ok(())
}

#[test]
fn replacing_an_equation_only_structural_range_publishes_a_full_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let before_id = NodeId::generate();
    let after_id = NodeId::generate();
    let math_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_id(&before_id, "before")),
        equation_block(&math_id, "x"),
        Block::Paragraph(paragraph_with_id(&after_id, "after")),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            collapsed_selection(&before_id, "before".len()),
        )))?;
    runtime.move_visual_caret_json(true, true)?;
    assert_eq!(
        runtime.session.selected_math_blocks(),
        std::slice::from_ref(&math_id)
    );
    assert!(runtime.session.selected_block_environment().is_none());

    // 只覆盖公式块的结构范围也必须发布完整快照，不能走单段增量路径。
    let result: Value = serde_json::from_str(&runtime.insert_text_delta_json("Z".to_owned())?)?;
    assert!(
        result.get("partial").is_none(),
        "a structural replacement must not publish a delta view"
    );
    assert!(
        result.get("baseDocumentGeneration").is_none(),
        "a structural replacement must not publish delta base metadata"
    );
    assert_eq!(result["documentGeneration"], "1");
    let paragraphs = result["paragraphs"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("full snapshot paragraphs are missing"))?
        .iter()
        .map(|paragraph| paragraph["text"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    // 两端文本端点与纯文本跨段替换一致：合并两端段落后在切口插入，公式块移除。
    assert_eq!(paragraphs, ["beforeZafter"]);
    // 文档中不再有公式块，替换后的插入点折叠在合并段落切口处。
    assert!(
        runtime
            .session
            .document()
            .body
            .iter()
            .all(|block| { matches!(block, Block::Paragraph(_)) })
    );
    assert!(!runtime.session.has_structure_selection());
    assert!(runtime.session.selected_math_blocks().is_empty());
    let selection = runtime.session.selection();
    assert_eq!(selection.anchor, selection.focus);
    assert_eq!(selection.focus.paragraph_id, before_id);
    assert_eq!(selection.focus.offset, "beforeZ".len());
    assert_eq!(runtime.document().body.len(), 1);
    Ok(())
}

#[test]
fn incremental_and_full_snapshots_publish_equal_structural_selection_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    let before_id = NodeId::generate();
    let after_id = NodeId::generate();
    let math_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_id(&before_id, "before")),
        equation_block(&math_id, "x"),
        Block::Paragraph(paragraph_with_id(&after_id, "after")),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            collapsed_selection(&before_id, "before".len()),
        )))?;
    runtime.move_visual_caret_json(true, true)?;
    runtime.move_visual_caret_json(true, true)?;
    let full: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    let generation = runtime
        .last_snapshot
        .as_ref()
        .ok_or_else(|| std::io::Error::other("layout snapshot is missing"))?
        .document_generation;

    let delta: Value = serde_json::from_str(
        &runtime.layout_snapshot_json_with_delta(Some((&after_id, 0, generation)))?,
    )?;
    assert_eq!(delta["partial"], true);
    assert_eq!(delta["documentGeneration"], full["documentGeneration"]);
    assert_eq!(delta["fontGeneration"], full["fontGeneration"]);
    assert_eq!(delta["selectionRects"], full["selectionRects"]);
    assert_eq!(
        delta["mathBlockHighlightRects"],
        full["mathBlockHighlightRects"]
    );
    assert_eq!(
        delta["blockEnvironmentHighlightRects"],
        full["blockEnvironmentHighlightRects"]
    );
    assert_eq!(
        delta["inlineEnvironmentHighlightRects"],
        full["inlineEnvironmentHighlightRects"]
    );
    Ok(())
}

#[test]
fn plain_arrow_collapses_a_structural_selection_instead_of_moving_from_its_focus()
-> Result<(), Box<dyn std::error::Error>> {
    let before_id = NodeId::generate();
    let after_id = NodeId::generate();
    let math_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_id(&before_id, "before")),
        equation_block(&math_id, "x"),
        Block::Paragraph(paragraph_with_id(&after_id, "after")),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            collapsed_selection(&before_id, "before".len()),
        )))?;

    // 布局候选路径的 Shift+Right 把公式对象纳入文本范围：终点是公式后段落起点。
    runtime.move_visual_caret_json(true, true)?;
    assert!(runtime.session.has_structure_selection());

    let collapsed: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, false)?)?;

    // 普通 Right 折叠到范围终点并停靠在公式后边界，而不是从焦点再前移一个字符。
    assert!(!runtime.session.has_structure_selection());
    assert!(runtime.session.selected_math_blocks().is_empty());
    assert_eq!(
        runtime.session.selection().anchor,
        runtime.session.selection().focus
    );
    assert_eq!(runtime.session.selection().focus.paragraph_id, after_id);
    assert_eq!(runtime.session.selection().focus.offset, 0);
    assert_eq!(
        collapsed["selectionRects"],
        serde_json::json!([]),
        "折叠后不应继续绘制结构选区矩形"
    );

    // 折叠消费了本次按键；再从折叠落点按 Right 正常前进一个视觉步长。
    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(runtime.session.selection().focus.paragraph_id, after_id);
    assert_eq!(runtime.session.selection().focus.offset, 1);
    Ok(())
}

#[test]
fn plain_left_collapses_a_text_structural_selection_to_its_document_start()
-> Result<(), Box<dyn std::error::Error>> {
    let before_id = NodeId::generate();
    let after_id = NodeId::generate();
    let math_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_id(&before_id, "before")),
        equation_block(&math_id, "x"),
        Block::Paragraph(paragraph_with_id(&after_id, "after")),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            collapsed_selection(&before_id, "before".len()),
        )))?;
    // 两次 Shift+Right：先覆盖公式，再把焦点写进后一段文字。
    runtime.move_visual_caret_json(true, true)?;
    runtime.move_visual_caret_json(true, true)?;
    assert!(runtime.session.has_structure_selection());
    assert_eq!(runtime.session.selection().focus.paragraph_id, after_id);

    runtime.move_visual_caret_json(false, false)?;

    // Left 折叠到结构范围起点（前一端段落末尾），而不是从焦点向左退一个字符。
    assert!(!runtime.session.has_structure_selection());
    assert_eq!(
        runtime.session.selection().anchor,
        runtime.session.selection().focus
    );
    assert_eq!(runtime.session.selection().focus.paragraph_id, before_id);
    assert_eq!(runtime.session.selection().focus.offset, "before".len());
    Ok(())
}

fn empty_inline_math_runtime(
    font_size_centipoints: u16,
) -> Result<(DocumentViewRuntime, NodeId), Box<dyn std::error::Error>> {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![
        ParagraphContent::Text(TextFlow {
            runs: vec![Run {
                text: "A".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Math(InlineMath {
            id: NodeId::generate(),
            math_tree: MathTree {
                schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                root: MathFlow {
                    id: NodeId::generate(),
                    content: Vec::new(),
                    extra: BTreeMap::new(),
                },
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Text(TextFlow {
            runs: vec![Run {
                text: "B".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }),
    ];
    let paragraph_id = paragraph.id.clone();
    let mut document = Document::blank();
    document.global_settings.default_font_size_centipoints = font_size_centipoints;
    document.body = vec![Block::Paragraph(paragraph)];
    Ok((DocumentViewRuntime::new(document)?, paragraph_id))
}

fn focus_empty_inline_math(
    runtime: &mut DocumentViewRuntime,
    paragraph_id: &NodeId,
) -> Result<(), Box<dyn std::error::Error>> {
    let position = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 1,
        affinity: caret_engine::Affinity::Downstream,
    };
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: position.clone(),
                focus: position,
            },
        )))?;
    runtime.session.run(caret_engine::Command::EnterMath(
        caret_engine::Direction::Forward,
    ))?;
    runtime.layout_snapshot_json()?;
    Ok(())
}

#[test]
fn empty_inline_math_focus_publishes_the_shared_insertion_slot()
-> Result<(), Box<dyn std::error::Error>> {
    for font_size_centipoints in [900u16, 1200, 2000] {
        let (mut runtime, paragraph_id) = empty_inline_math_runtime(font_size_centipoints)?;
        focus_empty_inline_math(&mut runtime, &paragraph_id)?;
        let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
        let caret_x = interaction["caretGeometry"]["xPt"]
            .as_f64()
            .ok_or_else(|| std::io::Error::other("empty inline math caret is missing"))?;
        let rectangles = interaction["inlineEnvironmentHighlightRects"]
            .as_array()
            .ok_or_else(|| std::io::Error::other("inline highlight was omitted"))?;
        assert_eq!(rectangles.len(), 1);
        let x0 = rectangles[0]["x0Pt"]
            .as_f64()
            .ok_or_else(|| std::io::Error::other("slot x0 is missing"))?;
        let x1 = rectangles[0]["x1Pt"]
            .as_f64()
            .ok_or_else(|| std::io::Error::other("slot x1 is missing"))?;
        let expected_width = (f64::from(font_size_centipoints) / 100.0 * 0.5).max(4.0);
        assert!(
            (x1 - x0 - expected_width).abs() < 1e-6,
            "slot width mismatch at {font_size_centipoints}: {x0}..{x1}"
        );
        assert!(
            ((x0 + x1) / 2.0 - caret_x).abs() < 1e-6,
            "slot must be centered on the math caret at {font_size_centipoints}"
        );
        assert_eq!(interaction["selectionRects"], serde_json::json!([]));
        assert_eq!(
            interaction["mathBlockHighlightRects"],
            serde_json::json!([])
        );
    }

    // 与待输入格式槽完全同宽（12pt 时都是 6pt）。
    let (mut math_runtime, paragraph_id) = empty_inline_math_runtime(1200)?;
    focus_empty_inline_math(&mut math_runtime, &paragraph_id)?;
    let math_snapshot: Value = serde_json::from_str(&math_runtime.layout_snapshot_json()?)?;
    let math_rectangles = math_snapshot["inlineEnvironmentHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("empty inline math highlight is missing"))?;
    let mut pending_runtime = DocumentViewRuntime::new_blank()?;
    pending_runtime
        .session
        .run(caret_engine::Command::ToggleInlineFormat(
            InlineEnvironmentKind::Bold,
        ))?;
    let pending: Value = serde_json::from_str(&pending_runtime.layout_snapshot_json()?)?;
    let pending_rectangles = pending["inlineEnvironmentHighlightRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("pending format slot is missing"))?;
    assert_eq!(math_rectangles.len(), 1);
    assert_eq!(pending_rectangles.len(), 1);
    let math_x0 = math_rectangles[0]["x0Pt"]
        .as_f64()
        .ok_or_else(|| std::io::Error::other("math slot x0 is missing"))?;
    let math_x1 = math_rectangles[0]["x1Pt"]
        .as_f64()
        .ok_or_else(|| std::io::Error::other("math slot x1 is missing"))?;
    let pending_x0 = pending_rectangles[0]["x0Pt"]
        .as_f64()
        .ok_or_else(|| std::io::Error::other("pending slot x0 is missing"))?;
    let pending_x1 = pending_rectangles[0]["x1Pt"]
        .as_f64()
        .ok_or_else(|| std::io::Error::other("pending slot x1 is missing"))?;
    let math_width = math_x1 - math_x0;
    let pending_width = pending_x1 - pending_x0;
    assert!((math_width - pending_width).abs() < 1e-6);
    Ok(())
}

#[test]
fn selecting_only_an_empty_inline_math_publishes_no_zero_width_rectangles()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut runtime, paragraph_id) = empty_inline_math_runtime(1200)?;
    runtime.layout_snapshot_json()?;
    let position = |offset, affinity| caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset,
        affinity,
    };
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: position(1, caret_engine::Affinity::Downstream),
                focus: position(4, caret_engine::Affinity::Upstream),
            },
        )))?;

    for json in [
        runtime.interaction_state_json()?,
        runtime.layout_snapshot_json()?,
    ] {
        let value: Value = serde_json::from_str(&json)?;
        let rectangles = value["selectionRects"]
            .as_array()
            .ok_or_else(|| std::io::Error::other("selectionRects is missing"))?;
        for rectangle in rectangles {
            let x0 = rectangle["x0Pt"]
                .as_f64()
                .ok_or_else(|| std::io::Error::other("x0Pt is missing"))?;
            let x1 = rectangle["x1Pt"]
                .as_f64()
                .ok_or_else(|| std::io::Error::other("x1Pt is missing"))?;
            let y0 = rectangle["y0Pt"]
                .as_f64()
                .ok_or_else(|| std::io::Error::other("y0Pt is missing"))?;
            let y1 = rectangle["y1Pt"]
                .as_f64()
                .ok_or_else(|| std::io::Error::other("y1Pt is missing"))?;
            assert!(
                x1 > x0 && y1 > y0,
                "empty inline math selection produced a degenerate rectangle: {rectangle:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn empty_inline_math_click_hits_the_slot_without_swallowing_following_text()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut runtime, paragraph_id) = empty_inline_math_runtime(1200)?;
    let snapshot = runtime.current_snapshot()?.clone();

    // 右侧相邻文本内部仍归正文，不因空公式槽重叠而进入公式。
    let following_caret = snapshot
        .caret_geometry(
            &caret_engine::Pos {
                story: caret_engine::StoryId::Body,
                paragraph_id: paragraph_id.clone(),
                offset: 4,
                affinity: caret_engine::Affinity::Downstream,
            },
            caret_engine::VisualAffinity::Downstream,
        )
        .ok_or_else(|| std::io::Error::other("following text caret is missing"))?;
    let body: Value = serde_json::from_str(&runtime.place_caret_at_page_json(
        0,
        following_caret.x_pt + 2.5,
        following_caret.y0_pt + 2.0,
    )?)?;
    assert_ne!(body["mathTree"], true);
    let offset = body["position"]["offsetUtf8"]
        .as_str()
        .ok_or_else(|| std::io::Error::other("body position is missing"))?;
    assert!(
        offset == "4" || offset == "5",
        "unexpected body offset {offset}"
    );

    // 插入槽中心（公式 caret 处）仍可命中并进入公式。
    let (mut runtime, paragraph_id) = empty_inline_math_runtime(1200)?;
    let snapshot = runtime.current_snapshot()?.clone();
    let math_caret = snapshot
        .caret_geometry(
            &caret_engine::Pos {
                story: caret_engine::StoryId::Body,
                paragraph_id,
                offset: 1,
                affinity: caret_engine::Affinity::Downstream,
            },
            caret_engine::VisualAffinity::Upstream,
        )
        .ok_or_else(|| std::io::Error::other("empty inline math caret is missing"))?;
    let placed: Value = serde_json::from_str(&runtime.place_caret_at_page_json(
        0,
        math_caret.x_pt,
        math_caret.y0_pt + 2.0,
    )?)?;
    assert_eq!(placed["valid"], true);
    assert_eq!(placed["mathTree"], true);
    Ok(())
}

#[test]
fn empty_inline_math_zero_advance_keeps_delete_and_undo() -> Result<(), Box<dyn std::error::Error>>
{
    let (mut runtime, paragraph_id) = empty_inline_math_runtime(1200)?;
    focus_empty_inline_math(&mut runtime, &paragraph_id)?;
    runtime.delete_backward_json()?;
    assert_eq!(paragraph_plain_text(runtime.document()), "AB");
    runtime.undo_json()?;
    assert_eq!(paragraph_plain_text(runtime.document()), "A\u{fffc}B");
    Ok(())
}

fn paragraph_plain_text(document: &Document) -> String {
    fn collect(contents: &[ParagraphContent], out: &mut String) {
        for content in contents {
            match content {
                ParagraphContent::Text(flow) => {
                    for run in &flow.runs {
                        out.push_str(&run.text);
                    }
                }
                ParagraphContent::Math(_) => out.push('\u{fffc}'),
                ParagraphContent::Environment(environment) => {
                    collect(&environment.children, out);
                }
                ParagraphContent::SoftBreak(_) => out.push('\n'),
                ParagraphContent::Opaque(_) => out.push('\u{fffc}'),
            }
        }
    }
    let mut text = String::new();
    if let Some(Block::Paragraph(paragraph)) = document.body.first() {
        collect(&paragraph.inlines, &mut text);
    }
    text
}

/// 构造与 C++ `setSelection` 桥接一致的正文拖选载荷，模拟拖动中的每次提交。
fn body_drag_payload(anchor: (&NodeId, usize), focus: (&NodeId, usize)) -> String {
    let position = |paragraph_id: &NodeId, offset: usize| {
        serde_json::json!({
            "kind": "body",
            "domainId": "body",
            "paragraphId": paragraph_id.to_string(),
            "offsetUtf8": offset.to_string(),
            "orderUtf8": offset.to_string(),
            "affinity": "downstream",
            "visualAffinity": "downstream",
        })
    };
    serde_json::json!({
        "anchor": position(anchor.0, anchor.1),
        "focus": position(focus.0, focus.1),
    })
    .to_string()
}

#[test]
fn dragging_across_display_math_previews_the_math_block_rect_before_release()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_id(&NodeId::generate(), "abc");
    let before_id = before.id.clone();
    let after = paragraph_with_id(&NodeId::generate(), "xyz");
    let after_id = after.id.clone();
    let math_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        equation_block(&math_id, "x+y"),
        Block::Paragraph(after),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let math_rectangle = runtime
        .current_snapshot()?
        .math_block_geometry(std::slice::from_ref(&math_id))
        .and_then(|rectangles| rectangles.first().copied())
        .ok_or_else(|| std::io::Error::other("equation fragment is missing"))?;
    let math_tuple = (
        i64::try_from(math_rectangle.page_index)?,
        math_rectangle.x0_pt,
        math_rectangle.y0_pt,
        math_rectangle.x1_pt,
        math_rectangle.y1_pt,
    );

    // 根因确认：现有拖选命中映射只有正文片段，没有覆盖公式块对象的预览矩形。
    let drag_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let drag_map_rows = drag_map["segments"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("drag hit map segments are missing"))?;
    assert!(
        !drag_map_rows
            .iter()
            .any(|segment| rectangle_close(&tuple_from_rect(segment), math_tuple)),
        "drag hit map must not yet expose the display math rectangle"
    );

    // 拖动过程（松手前）：每次焦点变化提交同一套 Rust 选区投影，公式块矩形必须可见。
    let drag_payload = body_drag_payload((&before_id, 1), (&after_id, 1));
    let dragging: Value = serde_json::from_str(&runtime.set_selection_json(&drag_payload)?)?;
    let dragging_rectangles = rectangle_tuples(&dragging["selectionRects"]);
    assert!(
        dragging_rectangles
            .iter()
            .any(|rectangle| rectangle_close(rectangle, math_tuple)),
        "dragging across an equation must preview its full-width block rectangle"
    );
    assert!(runtime.session.has_structure_selection());
    assert_eq!(
        runtime.session.selected_math_blocks(),
        std::slice::from_ref(&math_id)
    );

    // 松手后的提交与拖动中几何一致。
    let released: Value = serde_json::from_str(&runtime.set_selection_json(&drag_payload)?)?;
    assert_eq!(released["selectionRects"], dragging["selectionRects"]);
    Ok(())
}

fn tuple_from_rect(value: &Value) -> (i64, f64, f64, f64, f64) {
    (
        value["pageIndex"].as_i64().unwrap_or_default(),
        value["x0Pt"].as_f64().unwrap_or_default(),
        value["y0Pt"].as_f64().unwrap_or_default(),
        value["x1Pt"].as_f64().unwrap_or_default(),
        value["y1Pt"].as_f64().unwrap_or_default(),
    )
}

#[test]
fn dragging_within_paragraph_before_display_math_does_not_preview_the_block()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_id(&NodeId::generate(), "abc");
    let before_id = before.id.clone();
    let math_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(before), equation_block(&math_id, "x+y")];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let math_rectangle = runtime
        .current_snapshot()?
        .math_block_geometry(std::slice::from_ref(&math_id))
        .and_then(|rectangles| rectangles.first().copied())
        .ok_or_else(|| std::io::Error::other("equation fragment is missing"))?;
    let math_tuple = (
        i64::try_from(math_rectangle.page_index)?,
        math_rectangle.x0_pt,
        math_rectangle.y0_pt,
        math_rectangle.x1_pt,
        math_rectangle.y1_pt,
    );
    let selection: Value = serde_json::from_str(
        &runtime.set_selection_json(&body_drag_payload((&before_id, 0), (&before_id, 2)))?,
    )?;
    let rectangles = rectangle_tuples(&selection["selectionRects"]);
    assert!(!rectangles.is_empty());
    assert!(!runtime.session.has_structure_selection());
    assert!(
        !rectangles
            .iter()
            .any(|rectangle| rectangle_close(rectangle, math_tuple))
    );
    Ok(())
}

#[test]
fn dragging_across_a_block_environment_previews_its_environment_rect()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_id(&NodeId::generate(), "before");
    let before_id = before.id.clone();
    let environment_id = NodeId::generate();
    let title_id = NodeId::generate();
    let child = paragraph_with_id(&NodeId::generate(), "inside");
    let child_id = child.id.clone();
    let after = paragraph_with_id(&NodeId::generate(), "after");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        Block::Environment(BlockEnvironment {
            id: environment_id.clone(),
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: BTreeMap::new(),
            title: Some(paragraph_with_id(&title_id, "title")),
            children: vec![Block::Paragraph(child)],
            extra: BTreeMap::new(),
        }),
        Block::Paragraph(after),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let environment_rectangles = runtime
        .current_snapshot()?
        .environment_ranges
        .iter()
        .filter(|range| range.environment_id == environment_id)
        .map(|range| {
            (
                i64::try_from(range.page_index).unwrap_or_default(),
                range.x0_pt,
                range.y0_pt,
                range.x1_pt,
                range.y1_pt,
            )
        })
        .collect::<Vec<_>>();
    assert!(!environment_rectangles.is_empty());
    let selection: Value = serde_json::from_str(
        &runtime.set_selection_json(&body_drag_payload((&before_id, 1), (&child_id, 1)))?,
    )?;
    let rectangles = rectangle_tuples(&selection["selectionRects"]);
    assert!(runtime.session.has_structure_selection());
    assert_eq!(
        runtime.session.selected_block_environments(),
        std::slice::from_ref(&environment_id)
    );
    for expected in environment_rectangles {
        assert!(
            rectangles
                .iter()
                .any(|rectangle| rectangle_close(rectangle, expected)),
            "dragging across a block environment must preview its environment rect"
        );
    }
    Ok(())
}

#[test]
fn dragging_across_inline_math_and_rtl_text_keeps_page_bounded_rects()
-> Result<(), Box<dyn std::error::Error>> {
    // InlineMath：先经命令面板插入并输入内容，再退出公式编辑后整体拖选。
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "math".to_owned())?;
    runtime.command_line_command_json("accept", String::new())?;
    runtime.insert_text_json("x".to_owned())?;
    runtime.handle_editor_key_json("escape")?;
    runtime.layout_snapshot_json()?;
    let paragraph_id = runtime
        .document()
        .body
        .first()
        .and_then(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph.id.clone()),
            _ => None,
        })
        .ok_or_else(|| std::io::Error::other("paragraph is missing"))?;
    let text_len = runtime
        .session
        .document()
        .body
        .first()
        .and_then(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph_text_len(paragraph)),
            _ => None,
        });
    let text_len = text_len.ok_or_else(|| std::io::Error::other("paragraph text is missing"))?;
    let selection: Value = serde_json::from_str(&runtime.set_selection_json(
        &body_drag_payload((&paragraph_id, 0), (&paragraph_id, text_len)),
    )?)?;
    let rectangles = rectangle_tuples(&selection["selectionRects"]);
    assert!(!rectangles.is_empty());
    assert!(!runtime.session.has_structure_selection());
    let page = &runtime.current_snapshot()?.pages[0];
    assert!(rectangles.iter().all(|rectangle| {
        rectangle.0 == 0
            && rectangle.1 >= 0.0
            && rectangle.2 >= 0.0
            && rectangle.3 <= page.width_pt
            && rectangle.4 <= page.height_pt
            && rectangle.1 < rectangle.3
    }));

    // RTL 文本：提交与释放路径几何一致，且不越出页面。
    let rtl = paragraph_with_id(&NodeId::generate(), "شكرا");
    let rtl_id = rtl.id.clone();
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(rtl)];
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let selection: Value = serde_json::from_str(
        &runtime.set_selection_json(&body_drag_payload((&rtl_id, 0), (&rtl_id, 8)))?,
    )?;
    let rectangles = rectangle_tuples(&selection["selectionRects"]);
    assert!(!rectangles.is_empty());
    let page = &runtime.current_snapshot()?.pages[0];
    assert!(rectangles.iter().all(|rectangle| {
        rectangle.1 >= 0.0
            && rectangle.2 >= 0.0
            && rectangle.3 <= page.width_pt
            && rectangle.4 <= page.height_pt
    }));
    Ok(())
}

#[test]
fn dragging_across_pages_previews_page_clipped_rectangles() -> Result<(), Box<dyn std::error::Error>>
{
    let mut document = Document::blank();
    document.body = (0..60)
        .map(|index| {
            Block::Paragraph(paragraph_with_id(
                &NodeId::generate(),
                &format!("line {index}"),
            ))
        })
        .collect();
    let first_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let last_id = match document.body.last() {
        Some(Block::Paragraph(paragraph)) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let pages = runtime.current_snapshot()?.pages.len();
    assert!(pages > 1, "test document must paginate");
    let selection: Value = serde_json::from_str(
        &runtime.set_selection_json(&body_drag_payload((&first_id, 1), (&last_id, 5)))?,
    )?;
    let rectangles = rectangle_tuples(&selection["selectionRects"]);
    assert!(!rectangles.is_empty());
    let page_count = runtime.current_snapshot()?.pages.len();
    assert!(
        rectangles
            .iter()
            .all(|rectangle| usize::try_from(rectangle.0).is_ok_and(|page| page < page_count))
    );
    assert!(
        rectangles.iter().any(|rectangle| rectangle.0 > 0),
        "multi-page selection must include later-page rectangles"
    );
    Ok(())
}

fn paragraph_text_len(paragraph: &Paragraph) -> usize {
    let mut text = String::new();
    paragraph_text_into(paragraph, &mut text);
    text.len()
}

fn paragraph_text_into(paragraph: &Paragraph, out: &mut String) {
    for inline in &paragraph.inlines {
        match inline {
            ParagraphContent::Text(flow) => {
                for run in &flow.runs {
                    out.push_str(&run.text);
                }
            }
            ParagraphContent::Math(_) => out.push('\u{fffc}'),
            ParagraphContent::Environment(environment) => {
                for child in &environment.children {
                    if let ParagraphContent::Text(flow) = child {
                        for run in &flow.runs {
                            out.push_str(&run.text);
                        }
                    }
                }
            }
            ParagraphContent::SoftBreak(_) => out.push('\n'),
            ParagraphContent::Opaque(_) => out.push('\u{fffc}'),
        }
    }
}
