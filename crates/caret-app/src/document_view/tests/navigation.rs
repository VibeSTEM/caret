//! 验证指针选区、命中图与视觉光标导航。

use super::*;

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
