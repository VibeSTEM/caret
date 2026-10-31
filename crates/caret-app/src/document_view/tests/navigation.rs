//! 验证指针选区、命中图与视觉光标导航。

use super::*;
use caret_doc::{BlockEnvironment, BlockEnvironmentKind, NodeId, Paragraph, Run};
use std::collections::BTreeMap;

fn paragraph_with_text(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(InlineNode::Run(Run {
        text: text.to_owned(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }));
    paragraph
}

fn section(title: &str, body: &str) -> Block {
    Block::Environment(BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(paragraph_with_text(title)),
        children: vec![Block::Paragraph(paragraph_with_text(body))],
        extra: BTreeMap::new(),
    })
}

fn section_title_id(environment: &BlockEnvironment) -> Result<NodeId, std::io::Error> {
    environment
        .title
        .as_ref()
        .map(|title| title.id.clone())
        .ok_or_else(|| std::io::Error::other("Section title should exist"))
}

fn block_text(block: &Block) -> Option<&str> {
    let Block::Paragraph(paragraph) = block else {
        return None;
    };
    paragraph.inlines.iter().find_map(|inline| match inline {
        InlineNode::Run(run) => Some(run.text.as_str()),
        _ => None,
    })
}

#[test]
fn drag_hit_map_keeps_soft_break_carets_but_omits_its_highlight_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("你好".to_owned())?;
    runtime.insert_soft_break_json()?;
    runtime.insert_text_json("你好".to_owned())?;

    let snapshot: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    let text = snapshot["paragraphs"][0]["text"]
        .as_str()
        .ok_or_else(|| std::io::Error::other("paragraph text was omitted"))?;
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let segments = hit_map["segments"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("drag segments were omitted"))?;
    assert_eq!(segments.len(), 4);
    for segment in segments {
        let start = segment["startUtf8"]
            .as_str()
            .ok_or_else(|| std::io::Error::other("segment start was omitted"))?
            .parse::<usize>()?;
        let end = segment["endUtf8"]
            .as_str()
            .ok_or_else(|| std::io::Error::other("segment end was omitted"))?
            .parse::<usize>()?;
        assert_ne!(&text[start..end], "\n");
    }
    let stops = hit_map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("drag stops were omitted"))?;
    for offset in ["6", "7"] {
        assert!(
            stops
                .iter()
                .any(|stop| stop["position"]["offsetUtf8"] == offset)
        );
    }
    Ok(())
}

#[test]
fn visual_navigation_keeps_selection_and_vertical_line_target()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("alpha".to_owned())?;
    runtime.move_to_visual_line_edge_json(false, false)?;
    assert_eq!(runtime.session.selection().focus.offset, 0);

    let moved = runtime.move_visual_caret_json(true, false)?;
    let moved_snapshot: Value = serde_json::from_str(&moved)?;
    assert!(
        moved_snapshot["inputMethodContext"]
            .get("surroundingText")
            .is_none()
    );
    let selected = runtime.move_visual_caret_json(true, true)?;
    assert_eq!(runtime.session.selection().anchor.offset, 1);
    assert_eq!(runtime.session.selection().focus.offset, 2);
    let selected_snapshot: Value = serde_json::from_str(&selected)?;
    assert_eq!(
        selected_snapshot["selectionRects"].as_array().map(Vec::len),
        Some(1)
    );
    let replaced = runtime.insert_text_json("XY".to_owned())?;
    assert_eq!(
        serde_json::from_str::<Value>(&replaced)?["paragraphs"][0]["text"],
        "aXYpha"
    );

    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("ab".to_owned())?;
    runtime.insert_soft_break_json()?;
    runtime.insert_text_json("cd".to_owned())?;
    runtime.move_to_visual_line_edge_json(false, false)?;
    let before = serde_json::from_str::<Value>(&runtime.current_caret_json()?)?;
    runtime.move_vertical_caret_json(-1, false)?;
    let after = serde_json::from_str::<Value>(&runtime.current_caret_json()?)?;
    assert_eq!(runtime.session.selection().focus.offset, 0);
    let after_y = after["y0Pt"]
        .as_f64()
        .ok_or_else(|| std::io::Error::other("vertical navigation omitted caret y"))?;
    let before_y = before["y0Pt"]
        .as_f64()
        .ok_or_else(|| std::io::Error::other("caret geometry omitted y"))?;
    assert!(after_y < before_y);
    assert_eq!(runtime.session.document_generation(), 3);
    Ok(())
}

#[test]
fn visual_navigation_crosses_paragraph_boundaries() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("first".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("second".to_owned())?;
    let paragraph_ids: Vec<_> = runtime
        .current_snapshot()?
        .paragraphs
        .iter()
        .map(|paragraph| paragraph.id.clone())
        .collect();
    assert_eq!(paragraph_ids.len(), 2);

    runtime.move_to_visual_line_edge_json(false, false)?;
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        paragraph_ids[1]
    );
    assert_eq!(runtime.session.selection().focus.offset, 0);
    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(false, false)?)?;
    assert!(interaction.get("paragraphs").is_none());
    assert!(interaction.get("selectionRects").is_some());
    assert_eq!(
        interaction["inputMethodContext"]["surroundingText"],
        "first"
    );
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        paragraph_ids[0]
    );
    assert_eq!(runtime.session.selection().focus.offset, "first".len());
    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        paragraph_ids[1]
    );
    assert_eq!(runtime.session.selection().focus.offset, 0);
    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(false, false)?)?;
    assert_eq!(
        interaction["inputMethodContext"]["surroundingText"],
        "first"
    );

    runtime.move_to_visual_line_edge_json(false, false)?;
    runtime.move_vertical_caret_json(-1, false)?;
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        paragraph_ids[0]
    );
    Ok(())
}

#[test]
fn shift_navigation_selects_an_environment_across_its_title_story_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Title", "body");
    let (environment_id, title_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            environment
                .title
                .as_ref()
                .ok_or_else(|| std::io::Error::other("Section title is missing"))?
                .id
                .clone(),
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![section_block];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let title_end = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "5",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": title_end.clone(), "focus": title_end}).to_string(),
    )?;

    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, true)?)?;

    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    let selection_rects = interaction["selectionRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("environment selection geometry is missing"))?;
    let environment_ranges = &runtime.current_snapshot()?.environment_ranges;
    assert_eq!(selection_rects.len(), environment_ranges.len());
    assert!(!selection_rects.is_empty());
    assert_eq!(
        interaction["blockEnvironmentHighlightRects"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );

    let section_block = section("Title", "body");
    let (environment_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => return Err("Section body should begin with a paragraph".into()),
            },
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        section_block,
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let body_end = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "4",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": body_end.clone(), "focus": body_end}).to_string(),
    )?;
    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, true)?)?;
    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    assert!(
        !interaction["selectionRects"]
            .as_array()
            .ok_or_else(|| std::io::Error::other("environment selection geometry is missing"))?
            .is_empty()
    );
    Ok(())
}

#[test]
fn pointer_selection_rejects_invalid_utf8_positions_before_selecting_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("标题", "正文");
    let (environment_id, title_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            section_title_id(environment)?,
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => return Err("Section body should be a paragraph".into()),
            },
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![section_block];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let anchor = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let invalid_focus = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "1",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });

    let result = runtime.set_selection_json(
        &serde_json::json!({"anchor": anchor, "focus": invalid_focus}).to_string(),
    );

    assert!(result.is_err());
    assert_eq!(runtime.session.selected_block_environment(), None);
    assert_ne!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    Ok(())
}

#[test]
fn pointer_selection_preserves_text_before_a_section_and_selects_the_section()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Section", "body");
    let (environment_id, title_id) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("outside")),
        section_block,
    ];
    let outside_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    let anchor = serde_json::json!({
        "paragraphId": outside_id.to_string(),
        "offsetUtf8": "2",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let focus = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "7",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });

    let interaction: Value =
        serde_json::from_str(&runtime.set_selection_json(
            &serde_json::json!({"anchor": anchor, "focus": focus}).to_string(),
        )?)?;

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[environment_id]
    );
    let environment_range_count = runtime.current_snapshot()?.environment_ranges.len();
    assert!(
        interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| rectangles.len() > environment_range_count)
    );
    assert_eq!(
        runtime
            .session
            .structure_selection_anchor()
            .ok_or_else(|| std::io::Error::other("structure selection anchor is missing"))?
            .paragraph_id,
        outside_id
    );
    Ok(())
}

#[test]
fn pointer_selection_can_cover_two_adjacent_block_environments()
-> Result<(), Box<dyn std::error::Error>> {
    let first = section("First", "one");
    let second = section("Second", "two");
    let (first_id, first_title_id) = match &first {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let (second_id, second_title_id) = match &second {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        first,
        Block::Paragraph(paragraph_with_text("middle")),
        second,
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let anchor = serde_json::json!({
        "paragraphId": first_title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let focus = serde_json::json!({
        "paragraphId": second_title_id.to_string(),
        "offsetUtf8": "6",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });

    let interaction: Value =
        serde_json::from_str(&runtime.set_selection_json(
            &serde_json::json!({"anchor": anchor, "focus": focus}).to_string(),
        )?)?;

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[first_id, second_id]
    );
    let selection_rects = interaction["selectionRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("selection rectangles are missing"))?;
    assert!(selection_rects.len() > runtime.current_snapshot()?.environment_ranges.len());
    Ok(())
}

#[test]
fn pointer_selection_from_outside_to_outside_includes_the_intervening_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Section", "body");
    let (environment_id, _) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        section_block,
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let before_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let after_id = match &document.body[2] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    let anchor = serde_json::json!({
        "paragraphId": before_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let focus = serde_json::json!({
        "paragraphId": after_id.to_string(),
        "offsetUtf8": "5",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });

    let interaction: Value =
        serde_json::from_str(&runtime.set_selection_json(
            &serde_json::json!({"anchor": anchor, "focus": focus}).to_string(),
        )?)?;

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[environment_id]
    );
    let environment_range_count = runtime.current_snapshot()?.environment_ranges.len();
    assert!(
        interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| { rectangles.len() >= environment_range_count + 2 })
    );
    Ok(())
}

#[test]
fn shift_right_keeps_a_contiguous_selection_across_paragraph_section_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Section", "inside");
    let (environment_id, _) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        section_block,
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let before_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let after_id = match &document.body[2] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    let start = serde_json::json!({
        "paragraphId": before_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": start.clone(), "focus": start}).to_string(),
    )?;

    for _ in 0..32 {
        runtime.move_visual_caret_json(true, true)?;
        if let Some((_, focus)) = runtime.session.structure_selection()
            && focus.paragraph_id == after_id
            && focus.offset == 5
        {
            break;
        }
    }

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[environment_id]
    );
    assert_eq!(
        runtime
            .session
            .structure_selection()
            .ok_or_else(|| std::io::Error::other("structure selection was lost"))?
            .1
            .paragraph_id,
        after_id
    );
    let copied: Value = serde_json::from_str(&runtime.selected_text_json()?)?;
    assert_eq!(copied["text"], "before\nSection\ninside\nafter");
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    let environment_range_count = runtime.current_snapshot()?.environment_ranges.len();
    assert!(
        interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| { rectangles.len() >= environment_range_count + 2 })
    );
    Ok(())
}

#[test]
fn shift_down_keeps_a_contiguous_selection_across_paragraph_section_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Section", "inside");
    let (environment_id, _) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        section_block,
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let before_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let after_id = match &document.body[2] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    let start = serde_json::json!({
        "paragraphId": before_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": start.clone(), "focus": start}).to_string(),
    )?;

    for _ in 0..8 {
        runtime.move_vertical_caret_json(1, true)?;
        if runtime
            .session
            .structure_selection()
            .is_some_and(|(_, focus)| focus.paragraph_id == after_id)
        {
            break;
        }
    }

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[environment_id]
    );
    assert_eq!(
        runtime
            .session
            .structure_selection()
            .ok_or_else(|| std::io::Error::other("structure selection was lost"))?
            .1
            .paragraph_id,
        after_id
    );
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    let environment_range_count = runtime.current_snapshot()?.environment_ranges.len();
    assert!(
        interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| { rectangles.len() > environment_range_count })
    );
    Ok(())
}

#[test]
fn shift_vertical_selects_section_across_its_title_story_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Title", "body");
    let (environment_id, title_id) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![section_block];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let title_start = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": title_start.clone(), "focus": title_start}).to_string(),
    )?;

    runtime.move_vertical_caret_json(1, true)?;

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[environment_id]
    );
    Ok(())
}

#[test]
fn shift_horizontal_keeps_the_original_paragraph_anchor_when_entering_a_section()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Section", "body");
    let (environment_id, title_id) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("outside")),
        section_block,
    ];
    let outside_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    let outside_end = serde_json::json!({
        "paragraphId": outside_id.to_string(),
        "offsetUtf8": "7",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": outside_end.clone(), "focus": outside_end}).to_string(),
    )?;

    for _ in 0..4 {
        runtime.move_visual_caret_json(true, true)?;
        if !runtime.session.selected_block_environments().is_empty() {
            break;
        }
    }

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[environment_id]
    );
    assert_eq!(
        runtime
            .session
            .structure_selection_anchor()
            .ok_or_else(|| std::io::Error::other("structure selection anchor is missing"))?
            .paragraph_id,
        outside_id
    );
    let focus = runtime
        .session
        .structure_selection()
        .ok_or_else(|| std::io::Error::other("structure selection is missing"))?
        .1;
    assert_eq!(focus.paragraph_id, title_id);
    Ok(())
}

#[test]
fn visual_navigation_stops_between_adjacent_environments_without_creating_a_line()
-> Result<(), Box<dyn std::error::Error>> {
    let first = section("first", "body");
    let second = section("second", "body");
    let first_body_id = match &first {
        Block::Environment(environment) => match &environment.children[0] {
            Block::Paragraph(paragraph) => paragraph.id.clone(),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![first, second];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let endpoint = serde_json::json!({
        "paragraphId": first_body_id.to_string(),
        "offsetUtf8": "4",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": endpoint.clone(), "focus": endpoint}).to_string(),
    )?;

    let outside: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, false)?)?;
    assert_eq!(runtime.document().body.len(), 2);
    assert!(runtime.session.block_environment_exit().is_some());
    assert_eq!(
        outside["currentStructure"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(
        outside["blockEnvironmentHighlightRects"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert!(matches!(runtime.document().body[0], Block::Environment(_)));
    assert!(matches!(runtime.document().body[1], Block::Environment(_)));
    Ok(())
}

#[test]
fn repeated_arrow_from_outer_stop_enters_the_adjacent_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let first = section("First", "one");
    let second = section("Second", "two");
    let (first_environment_id, first_body_id) = match &first {
        Block::Environment(environment) => (
            environment.id.clone(),
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => unreachable!(),
            },
        ),
        _ => unreachable!(),
    };
    let (second_environment_id, second_title_id) = match &second {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![first, second];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let first_body_end = serde_json::json!({
        "paragraphId": first_body_id.to_string(),
        "offsetUtf8": "3",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": first_body_end.clone(), "focus": first_body_end}).to_string(),
    )?;
    let generation = runtime.document_generation();

    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(
        runtime.session.block_environment_exit(),
        Some((first_environment_id, caret_engine::Direction::Forward))
    );
    runtime.move_visual_caret_json(true, false)?;

    assert_eq!(runtime.document_generation(), generation);
    assert!(runtime.session.block_environment_exit().is_none());
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        second_title_id
    );
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    assert_eq!(
        interaction["currentStructure"][0]["id"],
        second_environment_id.to_string()
    );
    assert_eq!(interaction["currentStructure"][0]["kind"], "section");

    let first = section("First", "one");
    let second = section("Second", "two");
    let (first_body_id, second_environment_id, second_title_id) = match (&first, &second) {
        (Block::Environment(first), Block::Environment(second)) => (
            match &first.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => unreachable!(),
            },
            second.id.clone(),
            section_title_id(second)?,
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![first, second];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let second_title_start = serde_json::json!({
        "paragraphId": second_title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": second_title_start.clone(), "focus": second_title_start})
            .to_string(),
    )?;
    runtime.move_visual_caret_json(false, false)?;
    assert_eq!(
        runtime.session.block_environment_exit(),
        Some((second_environment_id, caret_engine::Direction::Backward))
    );
    runtime.move_visual_caret_json(false, false)?;
    assert!(runtime.session.block_environment_exit().is_none());
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        first_body_id
    );
    assert_eq!(runtime.session.selection().focus.offset, "one".len());
    Ok(())
}

#[test]
fn typing_after_structural_navigation_uses_the_visible_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Title", "body");
    let (section_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => return Err("Section body should be a paragraph".into()),
            },
        ),
        _ => unreachable!(),
    };
    let after = paragraph_with_text("after");
    let after_paragraph_id = after.id.clone();
    let mut document = Document::blank();
    document.body = vec![section_block, Block::Paragraph(after)];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let end_of_body = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "4",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": end_of_body.clone(), "focus": end_of_body}).to_string(),
    )?;

    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(
        runtime.session.block_environment_exit(),
        Some((section_id, caret_engine::Direction::Forward))
    );
    runtime.move_structure_json("next")?;

    let snapshot: Value = serde_json::from_str(&runtime.insert_text_delta_json("X".to_owned())?)?;
    assert_eq!(snapshot["partial"], true);
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        after_paragraph_id
    );
    assert_eq!(block_text(&runtime.document().body[1]), Some("Xafter"));
    Ok(())
}

#[test]
fn typing_at_environment_exit_uses_a_full_snapshot_for_the_new_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Title", "body");
    let (section_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => return Err("Section body should be a paragraph".into()),
            },
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        section_block,
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let end_of_body = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "4",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": end_of_body.clone(), "focus": end_of_body}).to_string(),
    )?;
    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(
        runtime.session.block_environment_exit(),
        Some((section_id, caret_engine::Direction::Forward))
    );

    let snapshot: Value = serde_json::from_str(&runtime.insert_text_delta_json("X".to_owned())?)?;

    assert!(snapshot.get("partial").is_none());
    assert_eq!(runtime.document().body.len(), 3);
    assert_eq!(block_text(&runtime.document().body[1]), Some("X"));
    assert_eq!(runtime.session.block_environment_exit(), None);
    Ok(())
}

#[test]
fn pointer_selection_across_section_title_and_body_selects_the_whole_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Title", "body");
    let (environment_id, title_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            section_title_id(environment)?,
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => return Err("Section body should be a paragraph".into()),
            },
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![section_block];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let title_start = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let body_end = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "4",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });

    let interaction: Value = serde_json::from_str(&runtime.set_selection_json(
        &serde_json::json!({"anchor": title_start, "focus": body_end}).to_string(),
    )?)?;

    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    assert_eq!(runtime.session.selection().focus.paragraph_id, body_id);
    assert_eq!(runtime.session.selection().focus.offset, 4);
    assert!(
        interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    let body_end = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "4",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    let title_start = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": body_end, "focus": title_start}).to_string(),
    )?;
    assert_eq!(runtime.session.selection().focus.paragraph_id, title_id);
    assert_eq!(runtime.session.selection().focus.offset, 0);
    Ok(())
}

#[test]
fn shift_left_from_section_start_selects_the_whole_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Title", "body");
    let (environment_id, title_id) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        section_block,
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let title_start = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": title_start.clone(), "focus": title_start}).to_string(),
    )?;

    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(false, true)?)?;

    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    assert!(
        interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    Ok(())
}

#[test]
fn escape_cancels_environment_selection_before_leaving_it() -> Result<(), Box<dyn std::error::Error>>
{
    let section = section("Heading", "body");
    let (environment_id, title_id) = match &section {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![section];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let title_end = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "7",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": title_end.clone(), "focus": title_end}).to_string(),
    )?;
    runtime.move_visual_caret_json(true, true)?;
    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    let generation = runtime.document_generation();

    let cancelled: Value = serde_json::from_str(&runtime.handle_editor_key_json("escape")?)?;

    assert!(runtime.session.selected_block_environment().is_none());
    assert_eq!(runtime.document_generation(), generation);
    assert_eq!(runtime.document().body.len(), 1);
    assert_eq!(
        cancelled["currentStructure"][0]["id"],
        environment_id.to_string()
    );
    Ok(())
}

#[test]
fn deleting_selected_environment_through_runtime_is_undoable()
-> Result<(), Box<dyn std::error::Error>> {
    let section = section("Heading", "body");
    let (environment_id, title_id) = match &section {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![section];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let title_end = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "7",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": title_end.clone(), "focus": title_end}).to_string(),
    )?;
    runtime.move_visual_caret_json(true, true)?;
    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    let generation = runtime.document_generation();

    runtime.delete_backward_json()?;

    assert!(runtime.session.selected_block_environment().is_none());
    assert_eq!(runtime.document_generation(), generation + 1);
    assert!(
        runtime
            .document()
            .body
            .iter()
            .all(|block| !matches!(block, Block::Environment(_)))
    );
    runtime.undo_json()?;
    assert!(
        matches!(&runtime.document().body[0], Block::Environment(environment) if environment.id == environment_id)
    );
    Ok(())
}

#[test]
fn environment_virtual_stops_use_outer_geometry_and_insert_outside_paragraphs()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Heading", "inside");
    let (environment_id, title_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            section_title_id(environment)?,
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => unreachable!(),
            },
        ),
        _ => unreachable!(),
    };

    let mut document = Document::blank();
    document.body = vec![section_block.clone()];
    let mut before_runtime = DocumentViewRuntime::new(document)?;
    let title_start = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    before_runtime.set_selection_json(
        &serde_json::json!({"anchor": title_start.clone(), "focus": title_start}).to_string(),
    )?;
    let outside_before: Value =
        serde_json::from_str(&before_runtime.move_visual_caret_json(false, false)?)?;
    assert_eq!(
        before_runtime.session.block_environment_exit(),
        Some((environment_id.clone(), caret_engine::Direction::Backward))
    );
    assert_eq!(
        outside_before["blockEnvironmentHighlightRects"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    let range_x0 = before_runtime.current_snapshot()?.environment_ranges[0].x0_pt;
    assert_eq!(
        outside_before["caretGeometry"]["pageIndex"],
        before_runtime.current_snapshot()?.environment_ranges[0].page_index
    );
    assert_eq!(
        outside_before["caretGeometry"]["xPt"].as_f64(),
        Some(range_x0)
    );
    let complete_caret: Value = serde_json::from_str(&before_runtime.current_caret_json()?)?;
    super::assert_pt_close(complete_caret["xPt"].as_f64(), range_x0);

    before_runtime.insert_text_json("before".to_owned())?;
    assert_eq!(before_runtime.document().body.len(), 2);
    assert_eq!(
        block_text(&before_runtime.document().body[0]),
        Some("before")
    );
    assert_eq!(
        before_runtime.session.selection().focus.offset,
        "before".len()
    );
    assert_ne!(
        before_runtime.session.selection().focus.paragraph_id,
        title_id
    );
    assert!(matches!(
        &before_runtime.document().body[1],
        Block::Environment(environment) if environment.id == environment_id
    ));

    let mut document = Document::blank();
    document.body = vec![section_block];
    let mut after_runtime = DocumentViewRuntime::new(document)?;
    let body_end = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "6",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    after_runtime.set_selection_json(
        &serde_json::json!({"anchor": body_end.clone(), "focus": body_end}).to_string(),
    )?;
    let outside_after: Value =
        serde_json::from_str(&after_runtime.move_visual_caret_json(true, false)?)?;
    assert_eq!(
        after_runtime.session.block_environment_exit(),
        Some((environment_id.clone(), caret_engine::Direction::Forward))
    );
    let range = after_runtime
        .current_snapshot()?
        .environment_ranges
        .iter()
        .find(|range| range.environment_id == environment_id)
        .ok_or_else(|| std::io::Error::other("Section layout range is missing"))?;
    assert_eq!(
        outside_after["caretGeometry"]["pageIndex"],
        range.page_index
    );
    assert_eq!(
        outside_after["caretGeometry"]["xPt"].as_f64(),
        Some(range.x1_pt)
    );

    after_runtime.insert_text_json("after".to_owned())?;
    assert_eq!(after_runtime.document().body.len(), 2);
    assert!(matches!(
        &after_runtime.document().body[0],
        Block::Environment(environment) if environment.id == environment_id
    ));
    assert_eq!(block_text(&after_runtime.document().body[1]), Some("after"));
    assert_eq!(
        after_runtime.session.selection().focus.offset,
        "after".len()
    );
    assert_ne!(
        after_runtime.session.selection().focus.paragraph_id,
        body_id
    );
    Ok(())
}

#[test]
fn existing_paragraphs_do_not_skip_environment_outer_stops()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Heading", "inside");
    let (environment_id, title_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            section_title_id(environment)?,
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => unreachable!(),
            },
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        section_block,
        Block::Paragraph(paragraph_with_text("after")),
    ];

    let mut before_runtime = DocumentViewRuntime::new(document.clone())?;
    let start = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    before_runtime.set_selection_json(
        &serde_json::json!({"anchor": start.clone(), "focus": start}).to_string(),
    )?;
    let outside: Value =
        serde_json::from_str(&before_runtime.move_visual_caret_json(false, false)?)?;
    assert_eq!(
        before_runtime.session.block_environment_exit(),
        Some((environment_id.clone(), caret_engine::Direction::Backward))
    );
    assert_eq!(
        outside["blockEnvironmentHighlightRects"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    before_runtime.insert_text_json("inserted before".to_owned())?;
    assert_eq!(before_runtime.document().body.len(), 4);
    assert_eq!(
        block_text(&before_runtime.document().body[1]),
        Some("inserted before")
    );
    assert!(matches!(
        before_runtime.document().body[2],
        Block::Environment(_)
    ));

    let mut after_runtime = DocumentViewRuntime::new(document)?;
    let end = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "6",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    after_runtime.set_selection_json(
        &serde_json::json!({"anchor": end.clone(), "focus": end}).to_string(),
    )?;
    let outside: Value = serde_json::from_str(&after_runtime.move_visual_caret_json(true, false)?)?;
    assert_eq!(
        after_runtime.session.block_environment_exit(),
        Some((environment_id.clone(), caret_engine::Direction::Forward))
    );
    assert_eq!(
        outside["blockEnvironmentHighlightRects"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    let environment_range_x1 = after_runtime
        .current_snapshot()?
        .environment_ranges
        .iter()
        .find(|range| range.environment_id == environment_id)
        .ok_or_else(|| std::io::Error::other("Section outer geometry is missing"))?
        .x1_pt;
    let caret: Value = serde_json::from_str(&after_runtime.current_caret_json()?)?;
    super::assert_pt_close(caret["xPt"].as_f64(), environment_range_x1);
    after_runtime.insert_text_json("inserted after".to_owned())?;
    assert_eq!(after_runtime.document().body.len(), 4);
    assert!(matches!(
        after_runtime.document().body[1],
        Block::Environment(_)
    ));
    assert_eq!(
        block_text(&after_runtime.document().body[2]),
        Some("inserted after")
    );
    Ok(())
}

#[test]
fn rtl_visual_right_exit_uses_logical_start_boundary() -> Result<(), Box<dyn std::error::Error>> {
    let before = section("before", "body");
    let rtl = section("مرحبا", "متن");
    let title_id = match &rtl {
        Block::Environment(environment) => environment
            .title
            .as_ref()
            .ok_or_else(|| std::io::Error::other("RTL title is missing"))?
            .id
            .clone(),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![before, rtl];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let endpoint = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": endpoint.clone(), "focus": endpoint}).to_string(),
    )?;

    let outside: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, false)?)?;
    assert_eq!(runtime.document().body.len(), 2);
    assert!(runtime.session.block_environment_exit().is_some());
    assert_eq!(
        outside["currentStructure"].as_array().map(Vec::len),
        Some(0)
    );
    assert!(matches!(runtime.document().body[0], Block::Environment(_)));
    assert!(matches!(runtime.document().body[1], Block::Environment(_)));
    Ok(())
}

#[test]
fn exiting_nested_environment_reuses_existing_parent_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let outer_title = paragraph_with_text("outer");
    let inner_paragraph = paragraph_with_text("inner");
    let inner_id = inner_paragraph.id.clone();
    let parent_paragraph = paragraph_with_text("parent");
    let parent_id = parent_paragraph.id.clone();
    let inner = Block::Environment(BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Custom,
        definition_id: None,
        props: BTreeMap::new(),
        title: None,
        children: vec![Block::Paragraph(inner_paragraph)],
        extra: BTreeMap::new(),
    });
    let outer = Block::Environment(BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(outer_title),
        children: vec![inner, Block::Paragraph(parent_paragraph)],
        extra: BTreeMap::new(),
    });
    let mut document = Document::blank();
    document.body = vec![outer];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let endpoint = serde_json::json!({
        "paragraphId": inner_id.to_string(),
        "offsetUtf8": "5",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": endpoint.clone(), "focus": endpoint}).to_string(),
    )?;
    let generation = runtime.document_generation();

    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(runtime.document_generation(), generation);
    assert!(runtime.session.block_environment_exit().is_some());
    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(runtime.document().body.len(), 1);
    assert_eq!(runtime.document_generation(), generation);
    assert!(runtime.session.block_environment_exit().is_none());
    assert_eq!(runtime.session.selection().focus.paragraph_id, parent_id);
    Ok(())
}

#[test]
fn section_title_navigation_and_backspace_keep_story_and_layout_in_sync()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("Heading".to_owned())?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "section".to_owned())?;
    runtime.command_line_command_json("accept", String::new())?;
    runtime.handle_editor_key_json("enter")?;
    runtime.insert_text_json("body".to_owned())?;

    let section_snapshot: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    assert_eq!(
        section_snapshot["currentStructure"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(section_snapshot["currentStructure"][0]["kind"], "section");

    let generation = runtime.document_generation();
    runtime.move_visual_caret_json(true, true)?;
    assert_eq!(runtime.document_generation(), generation);
    assert!(runtime.session.selected_block_environment().is_some());
    assert_eq!(runtime.document().body.len(), 2);

    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(runtime.document().body.len(), 2);
    assert!(matches!(runtime.document().body[1], Block::Environment(_)));
    assert!(runtime.session.block_environment_exit().is_some());

    let Block::Environment(section) = &runtime.document().body[1] else {
        return Err("预期文档第二项为 Section".into());
    };
    let body_id = match &section.children[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => return Err("预期 Section 正文为段落".into()),
    };
    assert_eq!(
        runtime.session.selection().focus.story,
        caret_engine::StoryId::Body
    );
    let body_start = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": body_start.clone(), "focus": body_start}).to_string(),
    )?;
    let deleted: Value = serde_json::from_str(&runtime.delete_backward_json()?)?;
    assert_eq!(
        deleted["documentGeneration"],
        runtime.session.document_generation().to_string()
    );
    let Block::Environment(section) = &runtime.document().body[1] else {
        return Err("退格后 Section 应保留".into());
    };
    assert!(section.children.is_empty());
    assert_eq!(
        section.title.as_ref().map(|title| {
            title
                .inlines
                .iter()
                .filter_map(|inline| match inline {
                    InlineNode::Run(run) => Some(run.text.as_str()),
                    _ => None,
                })
                .collect::<String>()
        }),
        Some("body".to_owned())
    );

    let restored: Value = serde_json::from_str(&runtime.undo_json()?)?;
    assert_eq!(
        restored["documentGeneration"],
        runtime.session.document_generation().to_string()
    );
    let Block::Environment(section) = &runtime.document().body[1] else {
        return Err("撤销后 Section 应保留".into());
    };
    assert_eq!(section.children.len(), 1);
    Ok(())
}

#[test]
fn section_title_drag_selection_uses_the_title_story() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("Heading".to_owned())?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "section".to_owned())?;
    runtime.command_line_command_json("accept", String::new())?;

    let section_id = match &runtime.document().body[1] {
        Block::Environment(section) => section.id.clone(),
        _ => return Err("预期文档首项为 Section".into()),
    };
    let title_id = match &runtime.document().body[1] {
        Block::Environment(section) => section
            .title
            .as_ref()
            .ok_or_else(|| std::io::Error::other("Section 标题缺失"))?
            .id
            .clone(),
        _ => return Err("预期 Section 有标题".into()),
    };
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let stops = hit_map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("标题没有拖选停靠点"))?;
    let first = stops
        .iter()
        .find(|stop| stop["position"]["paragraphId"] == title_id.to_string())
        .ok_or_else(|| std::io::Error::other("标题首停靠点不存在"))?;
    let last = stops
        .iter()
        .filter(|stop| stop["position"]["paragraphId"] == title_id.to_string())
        .max_by_key(|stop| {
            stop["position"]["offsetUtf8"]
                .as_str()
                .and_then(|offset| offset.parse::<usize>().ok())
                .unwrap_or(0)
        })
        .ok_or_else(|| std::io::Error::other("标题末停靠点不存在"))?;
    let endpoint = |stop: &Value| {
        serde_json::json!({
            "paragraphId": stop["position"]["paragraphId"],
            "offsetUtf8": stop["position"]["offsetUtf8"],
            "affinity": stop["position"]["affinity"],
            "visualAffinity": stop["visualAffinity"],
        })
    };
    runtime.set_selection_json(
        &serde_json::json!({"anchor": endpoint(first), "focus": endpoint(last)}).to_string(),
    )?;
    let selected_text: Value = serde_json::from_str(&runtime.selected_text_json()?)?;
    assert_eq!(selected_text["text"], "");
    assert_eq!(
        runtime.session.selection().focus.story,
        caret_engine::StoryId::EnvironmentTitle(section_id)
    );
    Ok(())
}

#[test]
fn structure_navigation_commands_update_interaction_snapshot_without_editing_document()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("first".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("second".to_owned())?;
    let generation = runtime.session.document_generation();

    let previous: Value = serde_json::from_str(&runtime.move_structure_json("previous")?)?;
    assert_eq!(runtime.session.selection().focus.offset, "first".len());
    assert!(previous.get("caretGeometry").is_some());
    assert_eq!(runtime.session.document_generation(), generation);

    let next: Value = serde_json::from_str(&runtime.move_structure_json("next")?)?;
    assert_eq!(runtime.session.selection().focus.offset, 0);
    assert!(next.get("caretGeometry").is_some());
    assert_eq!(runtime.session.document_generation(), generation);
    Ok(())
}

#[test]
fn structure_selection_action_expands_through_the_public_runtime_entry_point()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("alpha".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("beta".to_owned())?;
    runtime.move_to_visual_line_edge_json(false, false)?;

    let first: Value = serde_json::from_str(&runtime.move_structure_json("expand-selection")?)?;
    assert!(first["selectionRects"].is_array());
    assert_eq!(runtime.session.selection().anchor.offset, 0);
    assert_eq!(runtime.session.selection().focus.offset, "beta".len());
    Ok(())
}

#[test]
fn drag_hit_map_is_page_local_and_release_commits_one_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("ab😀cd".to_owned())?;
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    assert_eq!(hit_map["pageIndex"], 0);
    assert_eq!(hit_map["documentGeneration"], "1");
    let stops = hit_map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("drag map omitted caret stops"))?;
    assert!(!stops.is_empty());
    let text = "ab😀cd";
    let mut grapheme_boundaries = text
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    grapheme_boundaries.push(text.len());
    for stop in stops {
        assert_eq!(stop["pageIndex"], 0);
        let offset = stop["position"]["offsetUtf8"]
            .as_str()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| std::io::Error::other("drag stop omitted a valid UTF-8 offset"))?;
        assert!(grapheme_boundaries.contains(&offset));
    }
    assert!(
        hit_map["segments"]
            .as_array()
            .is_some_and(|segments| !segments.is_empty())
    );
    let mut ordered = stops.to_vec();
    ordered.sort_by_key(|stop| {
        stop["position"]["offsetUtf8"]
            .as_str()
            .unwrap_or_default()
            .parse::<usize>()
            .unwrap_or(usize::MAX)
    });
    let to_endpoint = |stop: &Value| {
        serde_json::json!({
            "paragraphId": stop["position"]["paragraphId"],
            "offsetUtf8": stop["position"]["offsetUtf8"],
            "affinity": stop["position"]["affinity"],
            "visualAffinity": stop["visualAffinity"],
        })
    };
    let selection = serde_json::json!({
        "anchor": to_endpoint(&ordered[0]),
        "focus": to_endpoint(ordered.last().ok_or_else(|| std::io::Error::other("drag map has no stops"))?),
    });
    let response = runtime.set_selection_json(&selection.to_string())?;
    let snapshot: Value = serde_json::from_str(&response)?;
    assert_eq!(snapshot["documentGeneration"], "1");
    assert!(
        snapshot["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    assert_eq!(runtime.session.selection().anchor.offset, 0);
    assert_eq!(runtime.session.selection().focus.offset, "ab😀cd".len());
    let selected_text: Value = serde_json::from_str(&runtime.selected_text_json()?)?;
    assert_eq!(selected_text["text"], "ab😀cd");
    Ok(())
}

#[test]
fn cross_paragraph_cjk_drag_stops_commit_a_valid_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let Block::Paragraph(first) = &mut document.body[0] else {
        unreachable!();
    };
    first.inlines = vec![InlineNode::Run(Run {
        text: "你好".to_owned(),
        language: Some("zh".to_owned()),
        props: Default::default(),
        extra: Default::default(),
    })];
    let mut second = first.clone();
    second.id = caret_doc::NodeId::generate();
    second.inlines = vec![InlineNode::Run(Run {
        text: "世界".to_owned(),
        language: Some("zh".to_owned()),
        props: Default::default(),
        extra: Default::default(),
    })];
    document.body.push(Block::Paragraph(second));
    let mut runtime = DocumentViewRuntime::new(document)?;
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let stops = hit_map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("drag map omitted caret stops"))?;
    let first_stop = stops
        .iter()
        .filter(|stop| stop["position"]["paragraphIndex"] == 0)
        .min_by_key(|stop| {
            stop["position"]["offsetUtf8"]
                .as_str()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(usize::MAX)
        })
        .ok_or_else(|| std::io::Error::other("first paragraph has no stop"))?;
    let last_stop = stops
        .iter()
        .filter(|stop| stop["position"]["paragraphIndex"] == 1)
        .max_by_key(|stop| {
            stop["position"]["offsetUtf8"]
                .as_str()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0)
        })
        .ok_or_else(|| std::io::Error::other("second paragraph has no stop"))?;
    let endpoint = |stop: &Value| {
        serde_json::json!({
            "paragraphId": stop["position"]["paragraphId"],
            "offsetUtf8": stop["position"]["offsetUtf8"],
            "affinity": stop["position"]["affinity"],
            "visualAffinity": stop["visualAffinity"],
        })
    };
    let result = runtime.set_selection_json(
        &serde_json::json!({
            "anchor": endpoint(first_stop),
            "focus": endpoint(last_stop),
        })
        .to_string(),
    )?;
    let snapshot: Value = serde_json::from_str(&result)?;
    assert_eq!(runtime.session.selection().anchor.offset, 0);
    assert_eq!(runtime.session.selection().focus.offset, "世界".len());
    assert!(
        snapshot["selectionRects"]
            .as_array()
            .is_some_and(|rects| rects.len() >= 2)
    );
    runtime.delete_backward_json()?;
    assert_eq!(runtime.session.document().body.len(), 1);
    Ok(())
}

#[test]
fn page_click_updates_the_rust_session_selection() -> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    if let Some(Block::Paragraph(paragraph)) = document.body.first_mut() {
        paragraph.inlines.push(InlineNode::Run(Run {
            text: "a".to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        }));
    } else {
        panic!("blank document should start with a paragraph");
    }
    let mut runtime = DocumentViewRuntime::new(document)?;

    let result = runtime.place_caret_at_page_json(0, 550.0, 73.0)?;
    let hit: Value = serde_json::from_str(&result)?;
    assert_eq!(hit["position"]["offsetUtf8"], "1");
    assert_eq!(runtime.session.selection().anchor.offset, 1);
    assert_eq!(runtime.session.selection().focus.offset, 1);
    assert_eq!(
        runtime.session.selection_visual_affinity(),
        caret_engine::VisualAffinity::Upstream
    );
    assert_eq!(runtime.session.document_generation(), 0);

    assert_eq!(runtime.place_caret_at_page_json(-1, 550.0, 73.0)?, "null");
    assert_eq!(runtime.session.selection().focus.offset, 1);

    runtime.set_selection_json(
        &serde_json::json!({
            "anchor": {
                "paragraphId": runtime.session.selection().focus.paragraph_id.to_string(),
                "offsetUtf8": "0",
                "affinity": "downstream",
                "visualAffinity": "downstream"
            },
            "focus": {
                "paragraphId": runtime.session.selection().focus.paragraph_id.to_string(),
                "offsetUtf8": "1",
                "affinity": "upstream",
                "visualAffinity": "upstream"
            }
        })
        .to_string(),
    )?;
    assert_ne!(
        runtime.session.selection().anchor,
        runtime.session.selection().focus
    );
    runtime.clear_selection_json()?;
    assert_eq!(
        runtime.session.selection().anchor,
        runtime.session.selection().focus
    );
    assert_eq!(runtime.session.selection().focus.offset, 1);
    Ok(())
}

#[test]
fn current_structure_exposes_heading_kind_and_level() -> Result<(), Box<dyn std::error::Error>> {
    let mut document = caret_doc::Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        return Err("blank document must contain a paragraph".into());
    };
    paragraph
        .props
        .insert("outlineLevel".into(), serde_json::json!(2));
    let session = caret_engine::Session::new(document)?;
    let state = super::super::snapshot::current_structure_state(&session);
    assert_eq!(state[0]["kind"], "heading");
    assert_eq!(state[0]["level"], 2);
    assert!(state[0].get("label").is_none());
    Ok(())
}
