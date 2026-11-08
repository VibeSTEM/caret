//! 验证输入法预编辑、替换提交与失败原子性。

use super::*;
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, InlineEnvironment,
    InlineEnvironmentKind, MathContent, NodeId, Paragraph, ParagraphContent, Run,
};
use caret_engine::Command;
use std::collections::BTreeMap;

fn enter_math_text(runtime: &mut DocumentViewRuntime, source: &str) -> Result<(), String> {
    let session = runtime.session_mut_for_test();
    session
        .run(Command::InsertInlineMath(source.to_owned()))
        .map_err(|error| error.to_string())?;
    session
        .run(Command::EnterMath(caret_engine::Direction::Backward))
        .map_err(|error| error.to_string())?;
    for _ in 0..64 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(caret_engine::MathEditBranch::Text { .. })
        ) {
            return Ok(());
        }
        session
            .run(Command::MoveCaret(caret_engine::Direction::Backward))
            .map_err(|error| error.to_string())?;
    }
    Err("math TextFlow was not reached".to_owned())
}

fn inline_math_text(runtime: &DocumentViewRuntime) -> Result<String, String> {
    let math = runtime
        .session
        .document()
        .body
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(&paragraph.inlines),
            _ => None,
        })
        .flatten()
        .find_map(|inline| match inline {
            ParagraphContent::Math(math) => Some(math),
            _ => None,
        })
        .ok_or_else(|| "expected inline formula".to_owned())?;
    let Some(MathContent::Math(fraction)) = math.math_tree.root.content.first() else {
        return Err("expected fraction".to_owned());
    };
    let Some(text) = fraction.children.get(1).and_then(|flow| {
        flow.content.iter().find_map(|content| match content {
            MathContent::Text(text) => Some(text),
            _ => None,
        })
    }) else {
        return Err("expected denominator TextFlow".to_owned());
    };
    Ok(text.runs.iter().map(|run| run.text.as_str()).collect())
}

#[test]
fn clipped_math_text_line_keeps_publishable_caret_and_accepts_input() -> Result<(), String> {
    let mut document = Document::blank();
    document.global_settings.paragraph_layout.line_spacing =
        caret_doc::ParagraphLineSpacing::Exactly {
            centipoints: 600,
            extra: Default::default(),
        };
    let mut runtime = DocumentViewRuntime::new(document).map_err(|error| error.to_string())?;
    enter_math_text(&mut runtime, r"\frac{}{\text{first}}")?;

    for _ in 0..16 {
        let context = runtime
            .session
            .math_text_input_context()
            .ok_or_else(|| "math text focus is missing".to_owned())?;
        if context.focus.offset == context.surrounding_text.len() {
            break;
        }
        runtime.move_visual_caret_json(true, false)?;
    }
    runtime.insert_text_json("\nsecond".to_owned())?;
    let caret: serde_json::Value = serde_json::from_str(
        &runtime
            .current_caret_json()
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let interaction: serde_json::Value = serde_json::from_str(
        &runtime
            .interaction_state_json()
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(interaction["caretGeometry"]["y0Pt"], caret["y0Pt"]);
    assert!(caret["y1Pt"].as_f64() > caret["y0Pt"].as_f64());

    runtime.insert_text_json("X".to_owned())?;
    let context = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| "input lost math text focus".to_owned())?;
    assert!(context.surrounding_text.contains("secondX"));
    Ok(())
}

#[test]
fn clipped_math_text_selection_collapses_by_unclipped_visual_line_order() -> Result<(), String> {
    let mut document = Document::blank();
    document.global_settings.paragraph_layout.line_spacing =
        caret_doc::ParagraphLineSpacing::Exactly {
            centipoints: 600,
            extra: Default::default(),
        };
    let mut runtime = DocumentViewRuntime::new(document).map_err(|error| error.to_string())?;
    enter_math_text(&mut runtime, r"\sqrt{\text{first}}")?;
    runtime.insert_text_json("\nsecond".to_owned())?;
    let context = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| "math text focus is missing".to_owned())?;
    let path = context.focus.path.clone();
    runtime
        .session
        .set_math_text_selection_positions(
            caret_engine::MathTextPosition::new(
                path.clone(),
                5,
                caret_engine::MathPositionAffinity::Downstream,
            ),
            caret_engine::MathTextPosition::new(
                path,
                6,
                caret_engine::MathPositionAffinity::Downstream,
            ),
        )
        .map_err(|error| error.to_string())?;

    runtime.move_visual_caret_json(true, false)?;
    let collapsed = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| "collapsed selection lost math text focus".to_owned())?;
    assert_eq!(collapsed.focus.offset, 6);
    Ok(())
}

fn paragraph_with_text(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(text_flow(Run {
        text: text.to_owned(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }));
    paragraph
}

fn section_with_body(title: &str, body: &str) -> (Block, NodeId) {
    let paragraph = paragraph_with_text(body);
    let id = paragraph.id.clone();
    (
        Block::Environment(BlockEnvironment {
            id: NodeId::generate(),
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: BTreeMap::new(),
            title: Some(paragraph_with_text(title)),
            children: vec![Block::Paragraph(paragraph)],
            extra: BTreeMap::new(),
        }),
        id,
    )
}

#[test]
fn inline_math_text_edit_reuses_unrelated_paragraph_layouts() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank().map_err(|error| error.to_string())?;
    runtime.insert_text_json("unchanged".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("host".to_owned())?;
    let unchanged_id = match runtime.session.document().body.first() {
        Some(Block::Paragraph(paragraph)) => paragraph.id.clone(),
        _ => return Err("expected first paragraph".to_owned()),
    };
    enter_math_text(&mut runtime, r"\frac{}{\text{word}}")?;
    runtime.layout_snapshot_json()?;
    let before_layout = std::sync::Arc::clone(
        &runtime
            .current_snapshot()
            .map_err(|error| error.to_string())?
            .paragraph_by_id(&unchanged_id)
            .ok_or_else(|| "unchanged paragraph is missing".to_owned())?
            .layout,
    );
    let response: Value = serde_json::from_str(&runtime.insert_text_delta_json("X".to_owned())?)
        .map_err(|error| error.to_string())?;
    assert!(response.get("partial").is_none());
    assert_eq!(inline_math_text(&runtime)?, "wordX");

    let updated = runtime
        .last_snapshot
        .as_ref()
        .ok_or_else(|| "math edit did not publish a layout snapshot".to_owned())?;
    let after = updated
        .paragraph_by_id(&unchanged_id)
        .ok_or_else(|| "unchanged paragraph disappeared".to_owned())?;
    assert!(std::sync::Arc::ptr_eq(&before_layout, &after.layout));
    Ok(())
}

#[test]
fn block_math_text_edit_reuses_all_paragraph_layouts() -> Result<(), String> {
    let paragraph_before = paragraph_with_text("before");
    let paragraph_after = paragraph_with_text("after");
    let before_id = paragraph_before.id.clone();
    let after_id = paragraph_after.id.clone();
    let math_tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![MathContent::Text(caret_doc::TextFlow {
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
    let equation_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_before),
        Block::Equation(caret_doc::Equation {
            id: equation_id.clone(),
            math_tree,
            extra: BTreeMap::new(),
        }),
        Block::Paragraph(paragraph_after),
    ];
    let mut runtime = DocumentViewRuntime::new(document).map_err(|error| error.to_string())?;
    runtime.layout_snapshot_json()?;
    let (before_layouts, before_math) = {
        let snapshot = runtime
            .current_snapshot()
            .map_err(|error| error.to_string())?;
        let before = snapshot
            .paragraph_by_id(&before_id)
            .ok_or_else(|| "preceding paragraph is missing".to_owned())?;
        let after = snapshot
            .paragraph_by_id(&after_id)
            .ok_or_else(|| "following paragraph is missing".to_owned())?;
        let math = std::sync::Arc::clone(
            snapshot
                .math_blocks
                .get(&equation_id)
                .ok_or_else(|| "equation layout is missing".to_owned())?,
        );
        (
            [
                std::sync::Arc::clone(&before.layout),
                std::sync::Arc::clone(&after.layout),
            ],
            math,
        )
    };
    let (x_pt, y_pt) = {
        let snapshot = runtime
            .current_snapshot()
            .map_err(|error| error.to_string())?;
        let page = &snapshot.pages[0];
        let fragment = page
            .layout
            .math_fragments
            .first()
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
                if snapshot.hit_test_math_text(0, x, y).is_some() {
                    hit = Some((x, y));
                    break;
                }
                x += 0.5;
            }
            y += 0.5;
        }
        hit.ok_or_else(|| "equation TextFlow was not hittable".to_owned())?
    };
    runtime.place_caret_at_page_json(0, x_pt, y_pt)?;
    runtime.insert_text_json("X".to_owned())?;
    let updated = runtime
        .last_snapshot
        .as_ref()
        .ok_or_else(|| "math edit did not publish a layout snapshot".to_owned())?;
    for (id, old_layout) in [before_id, after_id].into_iter().zip(before_layouts) {
        let paragraph = updated
            .paragraph_by_id(&id)
            .ok_or_else(|| "unchanged paragraph disappeared".to_owned())?;
        assert!(std::sync::Arc::ptr_eq(&old_layout, &paragraph.layout));
    }
    assert!(!std::sync::Arc::ptr_eq(
        &before_math,
        updated
            .math_blocks
            .get(&equation_id)
            .ok_or_else(|| "updated equation layout is missing".to_owned())?,
    ));
    Ok(())
}

#[test]
fn ime_preedit_is_transient_after_committing_its_utf16_replacement_range()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("A😀B".to_owned())?;
    runtime.move_to_visual_line_edge_json(true, false)?;

    let preedit = runtime.input_method_event_json("に".to_owned(), String::new(), -3, 2, 1)?;
    let snapshot: Value = serde_json::from_str(&preedit)?;
    assert_eq!(snapshot["paragraphs"][0]["text"], "AにB");
    assert!(
        snapshot["compositionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    assert_eq!(snapshot["documentGeneration"], "2");
    let composition = runtime
        .session
        .composition()
        .ok_or_else(|| std::io::Error::other("IME preedit was not retained"))?;
    assert_eq!(composition.preedit, "に");
    assert_eq!(runtime.session.document().body.len(), 1);
    assert_eq!(runtime.session.selection().anchor.offset, 1);
    assert_eq!(runtime.session.selection().focus.offset, 1);
    assert_eq!(
        runtime
            .composition_preview_session
            .as_ref()
            .map(|session| session.selection().focus.offset),
        Some(4)
    );
    assert_eq!(snapshot["inputMethodContext"]["surroundingText"], "AB");
    assert_eq!(snapshot["inputMethodContext"]["cursorPosition"], 1);
    assert_eq!(snapshot["inputMethodContext"]["anchorPosition"], 1);

    let committed = runtime.input_method_event_json(String::new(), "Z".to_owned(), 0, 0, 0)?;
    let snapshot: Value = serde_json::from_str(&committed)?;
    assert_eq!(snapshot["documentGeneration"], "3");
    assert_eq!(snapshot["paragraphs"][0]["text"], "AZB");
    assert!(runtime.session.composition().is_none());

    let mut runtime = DocumentViewRuntime::new_blank()?;
    let preedit = runtime.input_method_event_json("preedit".to_owned(), String::new(), 0, 0, 3)?;
    assert!(
        serde_json::from_str::<Value>(&preedit)?["compositionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    assert_eq!(
        runtime
            .composition_preview_session
            .as_ref()
            .map(|session| session.selection().focus.offset),
        Some(3)
    );
    let cancelled = runtime.input_method_event_json(String::new(), String::new(), 0, 0, 0)?;
    assert_eq!(
        serde_json::from_str::<Value>(&cancelled)?["documentGeneration"],
        "0"
    );
    assert!(runtime.session.composition().is_none());
    assert_eq!(
        serde_json::from_str::<Value>(&cancelled)?["paragraphs"][0]["text"],
        ""
    );
    assert!(
        serde_json::from_str::<Value>(&cancelled)?["compositionRects"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    Ok(())
}

#[test]
fn math_source_disables_ime_and_rejects_late_input_atomically() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime
        .session
        .run(Command::InsertInlineMath("x".to_owned()))
        .map_err(|error| error.to_string())?;
    runtime
        .session
        .run(Command::EnterMath(caret_engine::Direction::Backward))
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        runtime.session.math_edit().map(|edit| &edit.branch),
        Some(caret_engine::MathEditBranch::Source | caret_engine::MathEditBranch::Tree { .. })
    ));
    let before_document = runtime.session.document().clone();
    let before_generation = runtime.session.document_generation();
    let before_edit = runtime.session.math_edit().cloned();

    let snapshot: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)
        .map_err(|error| error.to_string())?;
    assert_eq!(snapshot["inputMethodContext"]["enabled"], false);
    assert_eq!(snapshot["inputMethodContext"]["preferLatin"], false);
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)
        .map_err(|error| error.to_string())?;
    assert_eq!(interaction["inputMethodContext"]["enabled"], false);
    assert_eq!(interaction["inputMethodContext"]["preferLatin"], false);
    assert!(
        runtime
            .input_method_event_json("に".to_owned(), String::new(), 0, 0, 1)
            .is_err()
    );
    assert!(
        runtime
            .input_method_event_json(String::new(), "你好".to_owned(), 0, 0, 0)
            .is_err()
    );
    assert_eq!(runtime.session.document(), &before_document);
    assert_eq!(runtime.session.document_generation(), before_generation);
    assert_eq!(runtime.session.math_edit().cloned(), before_edit);

    runtime
        .session
        .run(Command::BeginCommandLine)
        .map_err(|error| error.to_string())?;
    let before_command_line = runtime.session.command_line().cloned();
    let command_line_state: Value = serde_json::from_str(&runtime.interaction_state_json()?)
        .map_err(|error| error.to_string())?;
    assert_eq!(command_line_state["inputMethodContext"]["enabled"], false);
    assert_eq!(
        command_line_state["inputMethodContext"]["preferLatin"],
        false
    );
    assert!(
        runtime
            .input_method_event_json("ni".to_owned(), String::new(), 0, 0, 2)
            .is_err()
    );
    assert_eq!(runtime.session.command_line().cloned(), before_command_line);
    runtime
        .session
        .run(Command::CancelCommandLine)
        .map_err(|error| error.to_string())?;
    runtime
        .session
        .run(Command::EditorKey(caret_engine::EditorKey::Escape))
        .map_err(|error| error.to_string())?;
    let snapshot: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)
        .map_err(|error| error.to_string())?;
    assert_eq!(snapshot["inputMethodContext"]["enabled"], true);
    assert_eq!(snapshot["inputMethodContext"]["preferLatin"], false);
    Ok(())
}

#[test]
fn math_textflow_ime_preedit_commit_and_cancel_use_tree_text_context() -> Result<(), String> {
    let source = r"\frac{1}{\text{A😀B}}";
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("unchanged paragraph".to_owned())?;
    runtime.split_paragraph_json()?;
    enter_math_text(&mut runtime, source)?;
    runtime.layout_snapshot_json()?;
    let unchanged_id = match runtime.session.document().body.first() {
        Some(Block::Paragraph(paragraph)) => paragraph.id.clone(),
        _ => return Err("expected unchanged paragraph".to_owned()),
    };
    let unchanged_layout = std::sync::Arc::clone(
        &runtime
            .current_snapshot()
            .map_err(|error| error.to_string())?
            .paragraph_by_id(&unchanged_id)
            .ok_or_else(|| "unchanged paragraph is missing".to_owned())?
            .layout,
    );
    runtime
        .session_mut_for_test()
        .run(Command::SetMathTextSelection {
            anchor: 1,
            focus: 1,
        })
        .map_err(|error| error.to_string())?;

    let initial_generation = runtime.session.document_generation();
    let preview = runtime.input_method_event_json("に".to_owned(), String::new(), 0, 0, 1)?;
    let preview: Value = serde_json::from_str(&preview).map_err(|error| error.to_string())?;
    assert_eq!(preview["inputMethodContext"]["enabled"], true);
    assert_eq!(preview["inputMethodContext"]["preferLatin"], true);
    assert_eq!(preview["inputMethodContext"]["surroundingText"], "A😀B");
    assert_eq!(preview["inputMethodContext"]["cursorPosition"], 1);
    assert_eq!(preview["inputMethodContext"]["anchorPosition"], 1);
    assert!(
        preview["compositionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    assert_eq!(runtime.session.document_generation(), initial_generation);
    assert_eq!(inline_math_text(&runtime)?, "A😀B");
    let preview_snapshot = runtime
        .composition_snapshot
        .as_ref()
        .ok_or_else(|| "math composition snapshot was not installed".to_owned())?;
    assert!(std::sync::Arc::ptr_eq(
        &unchanged_layout,
        &preview_snapshot
            .paragraph_by_id(&unchanged_id)
            .ok_or_else(|| "unchanged paragraph is missing from preview".to_owned())?
            .layout,
    ));
    let updated_preview =
        runtime.input_method_event_json("にほ".to_owned(), String::new(), 0, 0, 2)?;
    let updated_preview: Value =
        serde_json::from_str(&updated_preview).map_err(|error| error.to_string())?;
    assert!(updated_preview.get("partial").is_none());
    assert!(std::sync::Arc::ptr_eq(
        &unchanged_layout,
        &runtime
            .composition_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.paragraph_by_id(&unchanged_id))
            .ok_or_else(|| "unchanged paragraph is missing from updated preview".to_owned())?
            .layout,
    ));
    assert_eq!(
        runtime
            .session
            .composition()
            .map(|value| value.preedit.as_str()),
        Some("にほ")
    );

    let committed = runtime.input_method_event_json(String::new(), "にほ".to_owned(), 0, 0, 0)?;
    let committed: Value = serde_json::from_str(&committed).map_err(|error| error.to_string())?;
    assert!(committed.get("partial").is_none());
    assert_eq!(committed["inputMethodContext"]["enabled"], true);
    assert_eq!(committed["inputMethodContext"]["preferLatin"], true);
    assert_eq!(inline_math_text(&runtime)?, "Aにほ😀B");
    assert!(runtime.session.composition().is_none());
    assert_eq!(
        runtime.session.document_generation(),
        initial_generation + 1
    );
    assert!(std::sync::Arc::ptr_eq(
        &unchanged_layout,
        &runtime
            .last_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.paragraph_by_id(&unchanged_id))
            .ok_or_else(|| "unchanged paragraph is missing after commit".to_owned())?
            .layout,
    ));

    runtime
        .session_mut_for_test()
        .run(Command::Undo)
        .map_err(|error| error.to_string())?;
    assert_eq!(inline_math_text(&runtime)?, "A😀B");

    let before_cancel_generation = runtime.session.document_generation();
    runtime.input_method_event_json("preedit".to_owned(), String::new(), 0, 0, 3)?;
    let cancelled = runtime.input_method_event_json(String::new(), String::new(), 0, 0, 0)?;
    let cancelled: Value = serde_json::from_str(&cancelled).map_err(|error| error.to_string())?;
    assert_eq!(cancelled["inputMethodContext"]["enabled"], true);
    assert_eq!(cancelled["inputMethodContext"]["preferLatin"], true);
    assert_eq!(inline_math_text(&runtime)?, "A😀B");
    assert!(runtime.session.composition().is_none());
    assert_eq!(
        runtime.session.document_generation(),
        before_cancel_generation
    );
    Ok(())
}

#[test]
fn math_textflow_short_preedit_replacement_publishes_preview_caret_and_underline()
-> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    enter_math_text(&mut runtime, r"\frac{1}{\text{abcdef}}")?;
    runtime
        .session_mut_for_test()
        .run(Command::SetMathTextSelection {
            anchor: 0,
            focus: 6,
        })
        .map_err(|error| error.to_string())?;

    let preview = runtime.input_method_event_json("xy".to_owned(), String::new(), 0, 0, 1)?;
    let preview: Value = serde_json::from_str(&preview).map_err(|error| error.to_string())?;
    assert_eq!(preview["inputMethodContext"]["surroundingText"], "abcdef");
    assert!(preview["caretGeometry"]["xPt"].as_f64().is_some());
    assert!(
        preview["compositionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    let current_caret: Value =
        serde_json::from_str(&runtime.current_caret_json()?).map_err(|error| error.to_string())?;
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)
        .map_err(|error| error.to_string())?;
    assert_eq!(preview["caretGeometry"]["xPt"], current_caret["xPt"]);
    assert_eq!(preview["caretGeometry"]["y0Pt"], current_caret["y0Pt"]);
    assert_eq!(interaction["caretGeometry"]["xPt"], current_caret["xPt"]);
    assert_eq!(interaction["caretGeometry"]["y0Pt"], current_caret["y0Pt"]);
    assert_eq!(inline_math_text(&runtime)?, "abcdef");
    Ok(())
}

#[test]
fn failed_math_text_preedit_publication_restores_the_committed_session() -> Result<(), String> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    enter_math_text(&mut runtime, r"\frac{1}{\text{abcdef}}")?;
    let document_before = runtime.session.document().clone();
    let context_before = runtime.session.math_text_input_context();
    let edit_before = runtime.session.math_edit().cloned();
    let last_snapshot_before = runtime.last_snapshot.as_ref().map(|snapshot| {
        (
            snapshot.session_id.clone(),
            snapshot.document_generation,
            snapshot.pages.len(),
        )
    });

    super::super::composition::fail_next_ime_preview_publish_for_test();
    let result = runtime.input_method_event_json("preedit".to_owned(), String::new(), 0, 0, 4);

    assert!(result.is_err());
    assert_eq!(runtime.session.document(), &document_before);
    assert_eq!(runtime.session.math_text_input_context(), context_before);
    assert_eq!(runtime.session.math_edit().cloned(), edit_before);
    assert!(runtime.session.composition().is_none());
    assert!(runtime.composition_preview_session.is_none());
    assert!(runtime.composition_snapshot.is_none());
    assert!(runtime.composition_range.is_none());
    assert_eq!(
        runtime.last_snapshot.as_ref().map(|snapshot| (
            snapshot.session_id.clone(),
            snapshot.document_generation,
            snapshot.pages.len(),
        )),
        last_snapshot_before
    );
    Ok(())
}

#[test]
fn failed_math_text_ime_full_layout_restores_tree_and_retries_as_one_undoable_edit()
-> Result<(), Box<dyn std::error::Error>> {
    let source = r"\frac{1}{\text{A😀B}}";
    let mut runtime = DocumentViewRuntime::new_blank()?;
    enter_math_text(&mut runtime, source)?;
    runtime
        .session_mut_for_test()
        .run(Command::SetMathTextSelection {
            anchor: 1,
            focus: 1,
        })?;

    let document_before = runtime.session.document().clone();
    let session_id_before = runtime.session.session_id().clone();
    let generation_before = runtime.session.document_generation();
    let history_state_before = runtime.session.history_state_id();
    let selection_before = runtime.session.selection().clone();
    let math_edit_before = runtime.session.math_edit().cloned();
    let math_context_before = runtime.session.math_text_input_context();

    runtime.input_method_event_json("に".to_owned(), String::new(), 0, 0, 1)?;
    let published_before = runtime.layout_snapshot_json()?;
    let composition_before = runtime.session.composition().cloned();
    let composition_id_before = runtime.composition_id;
    let composition_range_before = runtime.composition_range.clone();
    let composition_snapshot_before = runtime.composition_snapshot.as_ref().map(|snapshot| {
        (
            snapshot.session_id.clone(),
            snapshot.document_generation,
            snapshot.font_generation,
            snapshot.paragraphs.len(),
            snapshot.pages.len(),
        )
    });
    let last_snapshot_before = runtime.last_snapshot.as_ref().map(|snapshot| {
        (
            snapshot.session_id.clone(),
            snapshot.document_generation,
            snapshot.font_generation,
            snapshot.paragraphs.len(),
            snapshot.pages.len(),
        )
    });

    super::super::composition::fail_next_ime_full_layout_for_test();
    let failed = runtime.input_method_event_json(String::new(), "に".to_owned(), 0, 0, 0);

    assert!(failed.is_err());
    assert_eq!(runtime.session.document(), &document_before);
    assert_eq!(inline_math_text(&runtime)?, "A😀B");
    assert_eq!(runtime.session.session_id(), &session_id_before);
    assert_eq!(runtime.session.document_generation(), generation_before);
    assert_eq!(runtime.session.history_state_id(), history_state_before);
    assert_eq!(runtime.session.selection(), &selection_before);
    assert_eq!(runtime.session.math_edit().cloned(), math_edit_before);
    assert_eq!(
        runtime.session.math_text_input_context(),
        math_context_before
    );
    assert_eq!(runtime.session.composition(), composition_before.as_ref());
    assert_eq!(runtime.composition_id, composition_id_before);
    assert_eq!(runtime.composition_range, composition_range_before);
    assert_eq!(
        runtime.composition_snapshot.as_ref().map(|snapshot| (
            snapshot.session_id.clone(),
            snapshot.document_generation,
            snapshot.font_generation,
            snapshot.paragraphs.len(),
            snapshot.pages.len(),
        )),
        composition_snapshot_before
    );
    assert_eq!(
        runtime.last_snapshot.as_ref().map(|snapshot| (
            snapshot.session_id.clone(),
            snapshot.document_generation,
            snapshot.font_generation,
            snapshot.paragraphs.len(),
            snapshot.pages.len(),
        )),
        last_snapshot_before
    );
    assert_eq!(runtime.layout_snapshot_json()?, published_before);

    let committed = runtime.input_method_event_json(String::new(), "に".to_owned(), 0, 0, 0)?;
    let committed: Value = serde_json::from_str(&committed)?;
    assert_eq!(committed["inputMethodContext"]["enabled"], true);
    assert_eq!(inline_math_text(&runtime)?, "Aに😀B");
    assert!(runtime.session.composition().is_none());
    assert_eq!(runtime.session.document_generation(), generation_before + 1);
    runtime.session_mut_for_test().run(Command::Undo)?;
    assert_eq!(runtime.session.document(), &document_before);
    assert_eq!(inline_math_text(&runtime)?, "A😀B");
    Ok(())
}

#[test]
fn math_textflow_ime_invalid_replacement_keeps_formula_and_focus() -> Result<(), String> {
    let source = r"\frac{1}{\text{abcd}}";
    let mut runtime = DocumentViewRuntime::new_blank()?;
    enter_math_text(&mut runtime, source)?;
    runtime
        .session_mut_for_test()
        .run(Command::SetMathTextSelection {
            anchor: 2,
            focus: 2,
        })
        .map_err(|error| error.to_string())?;
    let before_generation = runtime.session.document_generation();
    let before_edit = runtime.session.math_edit().cloned();
    let before_document = runtime.session.document().clone();
    assert!(
        runtime
            .input_method_event_json(String::new(), "X".to_owned(), -100, 1, 0)
            .is_err()
    );
    assert_eq!(runtime.session.document(), &before_document);
    assert_eq!(runtime.session.document_generation(), before_generation);
    assert_eq!(runtime.session.math_edit().cloned(), before_edit);
    assert!(runtime.session.composition().is_none());
    assert_eq!(inline_math_text(&runtime)?, "abcd");
    Ok(())
}

#[test]
fn ime_commit_reuses_unchanged_paragraph_layouts() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("first paragraph".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("active paragraph".to_owned())?;
    runtime.input_method_event_json("ni".to_owned(), String::new(), 0, 0, 2)?;

    let committed = runtime.input_method_event_json(String::new(), "你好".to_owned(), 0, 0, 0)?;
    let committed: Value = serde_json::from_str(&committed)?;
    assert_eq!(committed["partial"], true);
    assert_eq!(committed["paragraphs"].as_array().map(Vec::len), Some(1));
    assert_eq!(committed["paragraphs"][0]["text"], "active paragraph你好");

    let reused = runtime
        .last_snapshot
        .as_ref()
        .ok_or("IME commit did not install a layout snapshot")?;
    let full = runtime.layout.layout_session(&runtime.session)?;
    assert_eq!(
        caret_engine::DocumentLayoutView::from(reused),
        caret_engine::DocumentLayoutView::from(&full)
    );
    Ok(())
}

#[test]
fn ime_preedit_updates_only_the_composition_paragraph() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("first paragraph".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("active paragraph".to_owned())?;

    let first: Value = serde_json::from_str(&runtime.input_method_event_json(
        "ni".to_owned(),
        String::new(),
        0,
        0,
        2,
    )?)?;
    assert_eq!(first["partial"], true);
    assert_eq!(first["compositionPreview"], true);
    assert_eq!(first["paragraphs"].as_array().map(Vec::len), Some(1));
    assert_eq!(first["paragraphs"][0]["text"], "active paragraphni");
    let previous_unchanged_layout = std::sync::Arc::clone(
        &runtime
            .composition_snapshot
            .as_ref()
            .ok_or("first IME composition snapshot was not installed")?
            .paragraphs[0]
            .layout,
    );

    let updated: Value = serde_json::from_str(&runtime.input_method_event_json(
        "ni hao".to_owned(),
        String::new(),
        0,
        0,
        5,
    )?)?;
    assert_eq!(updated["partial"], true);
    assert_eq!(updated["compositionPreview"], true);
    assert_eq!(updated["paragraphs"].as_array().map(Vec::len), Some(1));
    assert_eq!(updated["paragraphs"][0]["text"], "active paragraphni hao");
    assert!(
        updated["compositionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );

    let reused = runtime
        .composition_snapshot
        .as_ref()
        .ok_or("IME composition snapshot was not installed")?;
    assert!(std::sync::Arc::ptr_eq(
        &previous_unchanged_layout,
        &reused.paragraphs[0].layout
    ));
    let preview_session = runtime
        .composition_preview_session
        .as_ref()
        .ok_or("IME preview session was not installed")?;
    let full = runtime.layout.layout_session(preview_session)?;
    let reused_view = caret_engine::DocumentLayoutView::from(reused);
    let full_view = caret_engine::DocumentLayoutView::from(&full);
    assert_eq!(reused_view.pages, full_view.pages);
    assert_eq!(reused_view.paragraphs, full_view.paragraphs);
    Ok(())
}

#[test]
fn ime_preedit_inline_highlight_uses_the_preview_session_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        return Err("blank document should start with a paragraph".into());
    };
    let paragraph_id = paragraph.id.clone();
    paragraph.inlines = vec![ParagraphContent::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::new(),
        children: vec![text_flow(Run {
            text: "abcdef".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    })];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let position = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id,
        offset: 3,
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

    let actual: Value = serde_json::from_str(&runtime.input_method_event_json(
        "preedit text longer than the original".to_owned(),
        String::new(),
        0,
        0,
        37,
    )?)?;
    let range = runtime
        .composition_preview_session
        .as_mut()
        .ok_or("IME preview session was not installed")?
        .current_inline_environment()?
        .ok_or("IME preview lost the inline environment")?;
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
        .composition_snapshot
        .as_ref()
        .ok_or("IME composition snapshot was not installed")?
        .selection_geometry(&selection)
        .ok_or("preview inline range had no selection geometry")?
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

    assert_eq!(
        actual["inlineEnvironmentHighlightRects"],
        serde_json::json!(expected)
    );
    assert!(!expected.is_empty());
    assert!(
        actual["blockEnvironmentHighlightRects"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    Ok(())
}

#[test]
fn ime_deltas_include_pages_changed_by_an_earlier_preview() -> Result<(), Box<dyn std::error::Error>>
{
    let text = "paragraph content that is long enough to wrap across two lines ".repeat(4);
    let mut document = Document::blank();
    let make_paragraph = |text: &str| {
        let mut paragraph = caret_doc::Paragraph::empty(caret_doc::NodeId::generate());
        paragraph.layout = Some(caret_doc::ParagraphLayoutOverrides {
            line_spacing: Some(caret_doc::ParagraphLineSpacing::Exactly {
                centipoints: 2000,
                extra: Default::default(),
            }),
            space_before_centipoints: Some(0),
            space_after_centipoints: Some(0),
            widow_orphan_control: Some(false),
            keep_lines_together: Some(true),
            ..Default::default()
        });
        paragraph.inlines.push(text_flow(caret_doc::Run {
            text: text.to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        }));
        paragraph
    };
    let prefix = make_paragraph("x");
    let target = make_paragraph(&text);
    let prefix_id = prefix.id.clone();
    let target_id = target.id.clone();
    document.body = vec![Block::Paragraph(prefix), Block::Paragraph(target)];

    // 根据当前平台的实际换行确定页面高度，让短预编辑能回到前页，完整段落不能。
    let mut probe = DocumentViewRuntime::new(document.clone())?;
    probe.layout_snapshot_json()?;
    let snapshot = probe
        .last_snapshot
        .as_ref()
        .ok_or("probe layout unavailable")?;
    let paragraph_height = |id: &caret_doc::NodeId| -> Result<f64, Box<dyn std::error::Error>> {
        let paragraph = snapshot
            .paragraph_by_id(id)
            .ok_or("probe paragraph unavailable")?;
        Ok(paragraph
            .layout
            .paragraph
            .lines
            .iter()
            .map(|line| f64::from(line.metrics.height_pt))
            .sum())
    };
    let prefix_height = paragraph_height(&prefix_id)?;
    let target_height = paragraph_height(&target_id)?;
    assert!(
        target_height > 2.0 * prefix_height,
        "target paragraph must occupy at least three lines"
    );
    let content_height = target_height + prefix_height / 2.0;
    let mut geometry = document.page_sections[0].geometry()?;
    geometry.margins.bottom = geometry.page_size_pt().1 - geometry.margins.top - content_height;
    document.page_sections[0].set_geometry(geometry)?;
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;

    let paragraph_id = target_id;
    runtime.set_selection_json(
        &serde_json::json!({
            "anchor": {
                "paragraphId": paragraph_id.to_string(),
                "offsetUtf8": "0",
                "affinity": "upstream",
                "visualAffinity": "upstream"
            },
            "focus": {
                "paragraphId": paragraph_id.to_string(),
                "offsetUtf8": text.len().to_string(),
                "affinity": "downstream",
                "visualAffinity": "downstream"
            }
        })
        .to_string(),
    )?;
    let base = runtime
        .last_snapshot
        .as_ref()
        .ok_or("base layout snapshot was not installed")?
        .clone();
    let first_preview: Value = serde_json::from_str(&runtime.input_method_event_json(
        "x".to_owned(),
        String::new(),
        0,
        0,
        1,
    )?)?;
    let previous_preview = runtime
        .composition_snapshot
        .as_ref()
        .ok_or("first composition preview was not installed")?
        .clone();
    let page_of = |snapshot: &caret_engine::DocumentLayoutSnapshot| {
        snapshot.pages.iter().position(|page| {
            page.layout
                .fragments
                .iter()
                .any(|fragment| fragment.paragraph_id == paragraph_id)
        })
    };
    let base_page = page_of(&base).ok_or("paragraph was not paginated")?;
    let preview_page = page_of(&previous_preview).ok_or("preview was not paginated")?;

    assert_eq!(base_page, 1);
    assert_eq!(preview_page, 0);
    assert_eq!(first_preview["partial"], true);
    let updated: Value = serde_json::from_str(&runtime.input_method_event_json(
        text.clone(),
        String::new(),
        0,
        0,
        text.encode_utf16().count(),
    )?)?;
    let current_preview = runtime
        .composition_snapshot
        .as_ref()
        .ok_or("updated composition preview was not installed")?;
    let base_start = caret_engine::DocumentLayoutDeltaView::first_affected_page_index(
        &base,
        current_preview,
        &paragraph_id,
    )
    .ok_or("updated preview did not affect a base page")?;
    let preview_start = caret_engine::DocumentLayoutDeltaView::first_affected_page_index(
        &previous_preview,
        current_preview,
        &paragraph_id,
    )
    .ok_or("updated preview did not affect its prior page")?;
    let expected_start = base_start.min(preview_start);
    assert!(expected_start < base_start);
    assert_eq!(
        updated["pageStartIndex"].as_u64(),
        Some(expected_start as u64)
    );

    runtime.input_method_event_json(String::new(), String::new(), 0, 0, 0)?;
    let base_before_commit = runtime
        .last_snapshot
        .as_ref()
        .ok_or("base layout was not restored after cancellation")?
        .clone();
    runtime.set_selection_json(
        &serde_json::json!({
            "anchor": {
                "paragraphId": paragraph_id.to_string(),
                "offsetUtf8": "0",
                "affinity": "upstream",
                "visualAffinity": "upstream"
            },
            "focus": {
                "paragraphId": paragraph_id.to_string(),
                "offsetUtf8": text.len().to_string(),
                "affinity": "downstream",
                "visualAffinity": "downstream"
            }
        })
        .to_string(),
    )?;
    runtime.input_method_event_json("x".to_owned(), String::new(), 0, 0, 1)?;
    let short_preview = runtime
        .composition_snapshot
        .as_ref()
        .ok_or("short composition preview was not installed")?
        .clone();
    let committed: Value = serde_json::from_str(&runtime.input_method_event_json(
        String::new(),
        text.clone(),
        0,
        0,
        0,
    )?)?;
    let committed_layout = runtime
        .last_snapshot
        .as_ref()
        .ok_or("committed layout was not installed")?;
    let committed_start = caret_engine::DocumentLayoutDeltaView::first_affected_page_index(
        &base_before_commit,
        committed_layout,
        &paragraph_id,
    )
    .ok_or("commit did not affect a base page")?;
    let expected_commit_start = committed_start.min(
        caret_engine::DocumentLayoutDeltaView::first_affected_page_index(
            &base_before_commit,
            &short_preview,
            &paragraph_id,
        )
        .ok_or("initial preview did not affect a base page")?,
    );
    assert_eq!(committed["partial"], true);
    assert_eq!(
        committed["pageStartIndex"].as_u64(),
        Some(expected_commit_start as u64)
    );
    Ok(())
}

#[test]
fn failed_ime_commit_keeps_its_composition_id_cancellable() -> Result<(), Box<dyn std::error::Error>>
{
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.input_method_event_json("a".to_owned(), String::new(), 0, 0, 1)?;
    assert!(
        runtime
            .input_method_event_json(String::new(), "\n".to_owned(), 0, 0, 0)
            .is_err()
    );
    assert!(runtime.session.composition().is_some());
    assert!(runtime.composition_id.is_some());

    assert_eq!(
        runtime
            .session
            .composition()
            .map(|composition| composition.preedit.as_str()),
        Some("a")
    );
    runtime.input_method_event_json(String::new(), String::new(), 0, 0, 0)?;
    assert!(runtime.session.composition().is_none());
    assert!(runtime.composition_id.is_none());
    assert_eq!(runtime.session.document().body.len(), 1);
    Ok(())
}

#[test]
fn escape_cancels_math_text_preedit_and_keeps_formula_focus()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    enter_math_text(&mut runtime, r"\frac{1}{\text{ab}}")?;
    let before = inline_math_text(&runtime)?;
    runtime.input_method_event_json("中".to_owned(), String::new(), 0, 0, 1)?;
    assert!(runtime.session.composition().is_some());
    assert!(runtime.composition_id.is_some());

    runtime.handle_editor_key_json("escape")?;

    assert!(runtime.session.composition().is_none());
    assert!(runtime.composition_id.is_none());
    assert!(runtime.composition_preview_session.is_none());
    assert!(runtime.composition_snapshot.is_none());
    assert!(runtime.composition_range.is_none());
    assert_eq!(inline_math_text(&runtime)?, before);
    assert!(matches!(
        runtime.session.math_edit().map(|edit| &edit.branch),
        Some(caret_engine::MathEditBranch::Text { .. })
    ));
    Ok(())
}

#[test]
fn invalid_new_preedit_after_commit_publishes_committed_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.input_method_event_json("a".to_owned(), String::new(), 0, 0, 1)?;

    let response = runtime.input_method_event_json("\n".to_owned(), "b".to_owned(), 0, 0, 1)?;
    assert_eq!(runtime.session.document_generation(), 1);
    assert!(runtime.session.composition().is_none());
    assert!(runtime.composition_id.is_none());
    assert!(runtime.composition_preview_session.is_none());
    assert!(runtime.composition_snapshot.is_none());
    assert!(runtime.composition_range.is_none());
    let snapshot: Value = serde_json::from_str(&response)?;
    assert_eq!(snapshot["paragraphs"][0]["text"], "b");
    Ok(())
}

#[test]
fn ime_empty_commit_applies_replacement_as_a_deletion() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("ABCD".to_owned())?;

    let response = runtime.input_method_event_json(String::new(), String::new(), -1, 1, 0)?;
    let snapshot: Value = serde_json::from_str(&response)?;
    assert_eq!(snapshot["paragraphs"][0]["text"], "ABC");
    assert_eq!(runtime.session.document_generation(), 2);
    assert!(runtime.session.composition().is_none());
    Ok(())
}

#[test]
fn ime_preedit_keeps_replacement_deletion_after_composition_cancel()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("AB".to_owned())?;

    let preview = runtime.input_method_event_json("X".to_owned(), String::new(), -1, 1, 1)?;
    let snapshot: Value = serde_json::from_str(&preview)?;
    assert_eq!(snapshot["paragraphs"][0]["text"], "AX");
    assert_eq!(runtime.session.document_generation(), 2);
    assert!(runtime.session.composition().is_some());

    let cancelled = runtime.input_method_event_json(String::new(), String::new(), 0, 0, 0)?;
    let snapshot: Value = serde_json::from_str(&cancelled)?;
    assert_eq!(snapshot["paragraphs"][0]["text"], "A");
    assert_eq!(runtime.session.document_generation(), 2);
    assert!(runtime.session.composition().is_none());
    Ok(())
}

#[test]
fn ime_active_composition_applies_commit_replacement_range()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("ab".to_owned())?;
    runtime.input_method_event_json("X".to_owned(), String::new(), 0, 0, 1)?;
    let failed = runtime.input_method_event_json(String::new(), "Z".to_owned(), -10, 1, 0);
    assert!(failed.is_err());
    assert_eq!(runtime.session.document_generation(), 1);
    assert_eq!(
        runtime
            .session
            .composition()
            .map(|value| value.preedit.as_str()),
        Some("X")
    );

    let response = runtime.input_method_event_json(String::new(), "Z".to_owned(), -1, 1, 0)?;
    let snapshot: Value = serde_json::from_str(&response)?;
    assert_eq!(snapshot["paragraphs"][0]["text"], "aZ");
    assert_eq!(runtime.session.document_generation(), 2);
    assert!(runtime.session.composition().is_none());
    Ok(())
}

#[test]
fn ime_replacement_after_selected_preedit_uses_the_selection_start()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("ABCD".to_owned())?;
    let paragraph_id = match &runtime.session.document().body[0] {
        Block::Paragraph(paragraph) => paragraph.id.to_string(),
        _ => return Err(std::io::Error::other("body block is not a paragraph").into()),
    };
    runtime.set_selection_json(
        &serde_json::json!({
            "anchor": {
                "paragraphId": paragraph_id,
                "offsetUtf8": "1",
                "affinity": "upstream",
                "visualAffinity": "upstream"
            },
            "focus": {
                "paragraphId": paragraph_id,
                "offsetUtf8": "3",
                "affinity": "downstream",
                "visualAffinity": "downstream"
            }
        })
        .to_string(),
    )?;

    let preview = runtime.input_method_event_json("X".to_owned(), String::new(), 0, 0, 1)?;
    assert_eq!(
        serde_json::from_str::<Value>(&preview)?["paragraphs"][0]["text"],
        "AXD"
    );
    let committed = runtime.input_method_event_json(String::new(), "Z".to_owned(), -1, 1, 0)?;
    let snapshot: Value = serde_json::from_str(&committed)?;
    assert_eq!(snapshot["paragraphs"][0]["text"], "ZD");
    assert_eq!(runtime.session.document_generation(), 2);
    assert!(runtime.session.composition().is_none());
    Ok(())
}

#[test]
fn ime_reverse_cross_paragraph_selection_previews_from_the_retained_start()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.paste_text_json("first\nsecond".to_owned())?;
    let first_id = match &runtime.session.document().body[0] {
        Block::Paragraph(paragraph) => paragraph.id.to_string(),
        _ => return Err(std::io::Error::other("first body block is not a paragraph").into()),
    };
    let second_id = match &runtime.session.document().body[1] {
        Block::Paragraph(paragraph) => paragraph.id.to_string(),
        _ => return Err(std::io::Error::other("second body block is not a paragraph").into()),
    };
    runtime.set_selection_json(
        &serde_json::json!({
            "anchor": {
                "paragraphId": second_id,
                "offsetUtf8": "6",
                "affinity": "downstream",
                "visualAffinity": "downstream"
            },
            "focus": {
                "paragraphId": first_id,
                "offsetUtf8": "0",
                "affinity": "upstream",
                "visualAffinity": "upstream"
            }
        })
        .to_string(),
    )?;

    let response = runtime.input_method_event_json("X".to_owned(), String::new(), 0, 0, 1)?;
    let snapshot: Value = serde_json::from_str(&response)?;
    assert!(snapshot.get("partial").is_none());
    assert_eq!(snapshot["paragraphs"].as_array().map(Vec::len), Some(1));
    assert_eq!(snapshot["paragraphs"][0]["text"], "X");
    assert!(runtime.composition_preview_session.is_some());
    assert!(runtime.session.composition().is_some());
    assert_eq!(runtime.session.document().body.len(), 2);
    Ok(())
}

#[test]
fn failed_ime_preview_does_not_install_composition_state() -> Result<(), Box<dyn std::error::Error>>
{
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("AB".to_owned())?;
    let selection_before = runtime.session.selection().clone();

    assert!(
        runtime
            .input_method_event_json("X".to_owned(), String::new(), -100, 1, 1)
            .is_err()
    );
    assert_eq!(runtime.session.document_generation(), 1);
    assert_eq!(runtime.session.selection(), &selection_before);
    assert!(runtime.session.composition().is_none());
    assert!(runtime.composition_id.is_none());
    assert!(runtime.composition_preview_session.is_none());
    assert!(runtime.composition_snapshot.is_none());

    let (candidate, preview, _) =
        runtime
            .session
            .composition_update_candidate(77, "X".to_owned(), None, 1)?;
    drop((candidate, preview));
    assert!(runtime.session.can_undo());
    runtime.session.run(Command::Undo)?;
    assert_eq!(runtime.session.document().body.len(), 1);
    assert_eq!(runtime.session.document_generation(), 2);
    Ok(())
}

#[test]
fn failed_ime_commit_relayout_restores_session_and_published_composition_state()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("AB".to_owned())?;
    runtime.input_method_event_json("X".to_owned(), String::new(), 0, 0, 1)?;

    let published_before = runtime.layout_snapshot_json()?;
    let document_before = runtime.session.document().clone();
    let session_id_before = runtime.session.session_id().clone();
    let generation_before = runtime.session.document_generation();
    let history_state_before = runtime.session.history_state_id();
    let selection_before = runtime.session.selection().clone();
    let composition_before = runtime.session.composition().cloned();
    let composition_id_before = runtime.composition_id;
    let preview_session_id_before = runtime
        .composition_preview_session
        .as_ref()
        .map(|session| session.session_id().clone());
    let preview_generation_before = runtime
        .composition_preview_session
        .as_ref()
        .map(caret_engine::Session::document_generation);
    let preview_document_before = runtime
        .composition_preview_session
        .as_ref()
        .map(|session| session.document().clone());
    let composition_snapshot_before = runtime.composition_snapshot.as_ref().map(|snapshot| {
        (
            snapshot.session_id.clone(),
            snapshot.document_generation,
            snapshot.font_generation,
            snapshot.paragraphs.len(),
            snapshot.pages.len(),
        )
    });
    let composition_range_before = runtime.composition_range.clone();
    let last_snapshot_before = runtime.last_snapshot.as_ref().map(|snapshot| {
        (
            snapshot.session_id.clone(),
            snapshot.document_generation,
            snapshot.font_generation,
            snapshot.paragraphs.len(),
            snapshot.pages.len(),
        )
    });
    let cursor_position_before = runtime.preedit_cursor_position_utf16;

    super::super::composition::fail_next_ime_relayout_for_test();
    let result = runtime.input_method_event_json(String::new(), "Z".to_owned(), 0, 0, 0);

    assert!(result.is_err());
    assert_eq!(runtime.session.document(), &document_before);
    assert_eq!(runtime.session.session_id(), &session_id_before);
    assert_eq!(runtime.session.document_generation(), generation_before);
    assert_eq!(runtime.session.history_state_id(), history_state_before);
    assert_eq!(runtime.session.selection(), &selection_before);
    assert_eq!(runtime.session.composition(), composition_before.as_ref());
    assert_eq!(runtime.composition_id, composition_id_before);
    assert_eq!(
        runtime
            .composition_preview_session
            .as_ref()
            .map(|session| session.session_id().clone()),
        preview_session_id_before
    );
    assert_eq!(
        runtime
            .composition_preview_session
            .as_ref()
            .map(caret_engine::Session::document_generation),
        preview_generation_before
    );
    assert_eq!(
        runtime
            .composition_preview_session
            .as_ref()
            .map(|session| session.document().clone()),
        preview_document_before
    );
    assert_eq!(
        runtime.composition_snapshot.as_ref().map(|snapshot| (
            snapshot.session_id.clone(),
            snapshot.document_generation,
            snapshot.font_generation,
            snapshot.paragraphs.len(),
            snapshot.pages.len(),
        )),
        composition_snapshot_before
    );
    assert_eq!(runtime.composition_range, composition_range_before);
    assert_eq!(
        runtime.last_snapshot.as_ref().map(|snapshot| (
            snapshot.session_id.clone(),
            snapshot.document_generation,
            snapshot.font_generation,
            snapshot.paragraphs.len(),
            snapshot.pages.len(),
        )),
        last_snapshot_before
    );
    assert_eq!(
        runtime.preedit_cursor_position_utf16,
        cursor_position_before
    );
    assert_eq!(runtime.layout_snapshot_json()?, published_before);
    Ok(())
}

#[test]
fn ime_preedit_is_laid_out_at_its_insertion_point_and_moves_the_suffix()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("ABCD".to_owned())?;
    runtime.move_to_visual_line_edge_json(false, false)?;
    runtime.move_visual_caret_json(true, false)?;

    let before = serde_json::from_str::<Value>(&runtime.current_caret_json()?)?;
    let preedit = runtime.input_method_event_json("ni hao".to_owned(), String::new(), 0, 0, 2)?;
    let snapshot: Value = serde_json::from_str(&preedit)?;
    assert_eq!(snapshot["paragraphs"][0]["text"], "Ani haoBCD");
    assert_eq!(runtime.session.document_generation(), 1);
    let during = serde_json::from_str::<Value>(&runtime.current_caret_json()?)?;
    let during_x = during["xPt"].as_f64().ok_or("preedit caret x is missing")?;
    let before_x = before["xPt"]
        .as_f64()
        .ok_or("original caret x is missing")?;
    assert!(during_x > before_x);

    let cancelled = runtime.input_method_event_json(String::new(), String::new(), 0, 0, 0)?;
    let restored: Value = serde_json::from_str(&cancelled)?;
    assert_eq!(restored["paragraphs"][0]["text"], "ABCD");
    Ok(())
}

#[test]
fn ime_commit_at_environment_exit_inserts_after_the_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let (section, body_id) = section_with_body("Heading", "inside");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        section,
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let end = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "6",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": end.clone(), "focus": end}).to_string(),
    )?;
    runtime.move_visual_caret_json(true, false)?;
    assert!(runtime.session.block_environment_exit().is_some());

    let preview = runtime.input_method_event_json("ni".to_owned(), String::new(), 0, 0, 2)?;
    let preview: Value = serde_json::from_str(&preview)?;
    assert!(runtime.session.composition().is_some());
    assert_eq!(runtime.document().body.len(), 3);
    assert!(
        preview["paragraphs"].as_array().is_some_and(|paragraphs| {
            paragraphs.iter().any(|paragraph| paragraph["text"] == "ni")
        }),
        "composition preview must be outside the environment: {preview}"
    );

    let committed = runtime.input_method_event_json(String::new(), "你好".to_owned(), -1, 1, 0)?;
    let committed: Value = serde_json::from_str(&committed)?;
    assert_eq!(runtime.document().body.len(), 4);
    assert!(matches!(runtime.document().body[1], Block::Environment(_)));
    let Block::Paragraph(inserted) = &runtime.document().body[2] else {
        return Err("IME input after Section should be in a sibling paragraph".into());
    };
    let inserted_text = inserted
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            ParagraphContent::Text(flow) => flow.runs.first().map(|run| run.text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(inserted_text, "你好");
    assert!(
        committed["paragraphs"]
            .as_array()
            .is_some_and(|paragraphs| {
                paragraphs
                    .iter()
                    .any(|paragraph| paragraph["text"] == "你好")
            }),
        "committed layout should include inserted text: {committed}"
    );
    assert!(runtime.session.block_environment_exit().is_none());
    runtime.undo_json()?;
    assert_eq!(runtime.document().body.len(), 3);
    Ok(())
}

#[test]
fn multiline_paste_at_environment_exit_inserts_sibling_paragraphs_as_one_edit()
-> Result<(), Box<dyn std::error::Error>> {
    let (section, body_id) = section_with_body("Heading", "inside");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        section,
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let end = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "6",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": end.clone(), "focus": end}).to_string(),
    )?;
    runtime.move_visual_caret_json(true, false)?;

    runtime.paste_text_json("first\nsecond".to_owned())?;

    assert_eq!(runtime.document().body.len(), 5);
    assert!(matches!(runtime.document().body[1], Block::Environment(_)));
    for (index, expected) in [(2, "first"), (3, "second")] {
        let Block::Paragraph(paragraph) = &runtime.document().body[index] else {
            return Err("multiline paste should create sibling paragraphs".into());
        };
        let text = paragraph
            .inlines
            .iter()
            .filter_map(|inline| match inline {
                ParagraphContent::Text(flow) => flow.runs.first().map(|run| run.text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, expected);
    }
    runtime.undo_json()?;
    assert_eq!(runtime.document().body.len(), 3);
    assert!(matches!(runtime.document().body[1], Block::Environment(_)));
    Ok(())
}
