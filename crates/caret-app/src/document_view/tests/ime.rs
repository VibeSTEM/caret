//! 验证输入法预编辑、替换提交与失败原子性。

use super::*;

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
fn ime_deltas_include_pages_changed_by_an_earlier_preview() -> Result<(), Box<dyn std::error::Error>>
{
    let text = "paragraph content that is long enough to wrap across two lines ".repeat(2);
    let mut document = Document::blank();
    document.body = (0..100)
        .map(|_| {
            let mut paragraph = caret_doc::Paragraph::empty(caret_doc::NodeId::generate());
            paragraph.layout = Some(caret_doc::ParagraphLayoutOverrides {
                keep_lines_together: Some(true),
                ..Default::default()
            });
            paragraph
                .inlines
                .push(caret_doc::InlineNode::Run(caret_doc::Run {
                    text: text.clone(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                }));
            Block::Paragraph(paragraph)
        })
        .collect();
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;

    for paragraph_index in 0..100 {
        let paragraph = match &runtime.session.document().body[paragraph_index] {
            Block::Paragraph(paragraph) => paragraph,
            _ => unreachable!("fixture only contains paragraphs"),
        };
        let paragraph_id = paragraph.id.clone();
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

        if preview_page < base_page {
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
            return Ok(());
        }

        runtime.input_method_event_json(String::new(), String::new(), 0, 0, 0)?;
    }
    Err(
        std::io::Error::other("fixture did not move a paragraph with a one-line IME preview")
            .into(),
    )
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
