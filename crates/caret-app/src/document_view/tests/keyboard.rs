//! 验证键盘文本输入、段落命令与输入法上下文。

use super::*;

fn paragraph_with_text(text: &str) -> caret_doc::Paragraph {
    let mut paragraph = caret_doc::Paragraph::empty(caret_doc::NodeId::generate());
    paragraph.inlines.push(InlineNode::Run(Run {
        text: text.to_owned(),
        language: None,
        props: Default::default(),
        extra: Default::default(),
    }));
    paragraph
}

fn paragraph_text(paragraph: &caret_doc::Paragraph) -> String {
    paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            InlineNode::Run(run) => Some(run.text.as_str()),
            _ => None,
        })
        .collect()
}

fn document_with_paragraph_section_paragraph() -> (Document, String, String) {
    let before = paragraph_with_text("before");
    let title = paragraph_with_text("heading");
    let body = paragraph_with_text("inside");
    let body_id = body.id.to_string();
    let after = paragraph_with_text("after");
    let after_id = after.id.to_string();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        Block::Environment(caret_doc::BlockEnvironment {
            id: caret_doc::NodeId::generate(),
            kind: caret_doc::BlockEnvironmentKind::Section,
            definition_id: None,
            props: Default::default(),
            title: Some(title),
            children: vec![Block::Paragraph(body)],
            extra: Default::default(),
        }),
        Block::Paragraph(after),
    ];
    (document, body_id, after_id)
}

#[test]
fn editor_enter_publishes_the_new_paragraph_layout() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("first".to_owned())?;

    let snapshot: Value = serde_json::from_str(&runtime.handle_editor_key_json("enter")?)?;

    assert_eq!(snapshot["documentGeneration"], "2");
    assert_eq!(snapshot["paragraphs"].as_array().map(Vec::len), Some(2));
    assert_eq!(snapshot["paragraphs"][0]["text"], "first");
    assert_eq!(snapshot["paragraphs"][1]["text"], "");
    Ok(())
}

#[test]
fn editor_enter_accepts_command_candidate_and_publishes_its_document_change()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "newparagraph".to_owned())?;
    runtime.command_line_command_json("select", "0".to_owned())?;

    let snapshot: Value = serde_json::from_str(&runtime.handle_editor_key_json("enter")?)?;

    assert_eq!(snapshot["documentGeneration"], "1");
    assert_eq!(snapshot["paragraphs"].as_array().map(Vec::len), Some(2));
    assert_eq!(snapshot["commandLine"]["active"], false);
    Ok(())
}

#[test]
fn editor_escape_cancels_ime_preview_and_publishes_the_base_layout()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    let preedit: Value = serde_json::from_str(&runtime.input_method_event_json(
        "にほん".to_owned(),
        String::new(),
        0,
        0,
        3,
    )?)?;
    assert_eq!(preedit["compositionPreview"], true);

    let cancelled: Value = serde_json::from_str(&runtime.handle_editor_key_json("escape")?)?;

    assert_eq!(cancelled["documentGeneration"], "0");
    assert_ne!(cancelled["compositionPreview"], true);
    assert!(runtime.composition_id.is_none());
    assert!(runtime.composition_snapshot.is_none());
    assert_eq!(runtime.session.document().body.len(), 1);
    Ok(())
}

#[test]
fn editor_escape_cancels_command_line_ime_preedit_without_starting_document_composition()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    let preedit: Value = serde_json::from_str(&runtime.input_method_event_json(
        "preedit".to_owned(),
        String::new(),
        0,
        0,
        3,
    )?)?;
    assert_eq!(preedit["commandLine"]["preedit"], "preedit");
    assert!(runtime.session.command_line().is_some());
    assert!(runtime.session.composition().is_none());

    let command_line_cancelled: Value =
        serde_json::from_str(&runtime.handle_editor_key_json("escape")?)?;
    assert_eq!(command_line_cancelled["commandLine"]["active"], false);
    assert!(runtime.session.composition().is_none());
    assert_eq!(
        command_line_cancelled["inputMethodContext"]["surroundingText"],
        ""
    );
    Ok(())
}

#[test]
fn command_line_cancel_api_publishes_ime_cancellation() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.input_method_event_json("preedit".to_owned(), String::new(), 0, 0, 3)?;

    let cancelled: Value =
        serde_json::from_str(&runtime.command_line_command_json("cancel", String::new())?)?;

    assert_eq!(cancelled["compositionCancelled"], true);
    assert_eq!(cancelled["commandLine"]["active"], false);
    assert_eq!(cancelled["inputMethodContext"]["surroundingText"], "");
    Ok(())
}

#[test]
fn invalid_empty_preedit_replacement_can_be_cancelled_and_recovered()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.input_method_event_json("ni".to_owned(), String::new(), 0, 0, 2)?;

    assert!(
        runtime
            .input_method_event_json(String::new(), String::new(), -1, 1, 0)
            .is_err()
    );
    assert_eq!(
        runtime
            .session
            .command_line()
            .map(|line| line.preedit.as_str()),
        Some("ni")
    );

    let recovered: Value = serde_json::from_str(&runtime.input_method_event_json(
        String::new(),
        String::new(),
        0,
        0,
        0,
    )?)?;
    assert_eq!(recovered["commandLine"]["preedit"], "");
    runtime.command_line_command_json("type", "section".to_owned())?;
    assert_eq!(
        runtime
            .session
            .command_line()
            .map(|line| line.query.as_str()),
        Some("section")
    );
    Ok(())
}

#[test]
fn tab_with_empty_query_inserts_literal_slash_even_when_font_candidate_is_selected()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    let font_candidate_index = runtime
        .session
        .command_line_candidates()
        .iter()
        .position(|candidate| candidate.command_id == "text.format.font")
        .ok_or_else(|| std::io::Error::other("font command candidate is missing"))?;
    runtime.command_line_command_json("select", font_candidate_index.to_string())?;

    runtime.command_line_command_json("tab", String::new())?;

    assert!(runtime.session.command_line().is_none());
    let Some(caret_doc::Block::Paragraph(paragraph)) = runtime.session.document().body.first()
    else {
        return Err(std::io::Error::other("literal slash paragraph is missing").into());
    };
    assert_eq!(paragraph_text(paragraph), "\\");
    Ok(())
}

#[test]
fn font_command_uses_system_catalog_and_applies_the_selected_family_locally()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    let family = runtime
        .layout
        .font_family_names()
        .into_iter()
        .next()
        .ok_or_else(|| std::io::Error::other("system font catalog is empty"))?;

    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "font".to_owned())?;
    let command_candidates = runtime.session.command_line_candidates();
    assert!(
        command_candidates
            .iter()
            .any(|candidate| { candidate.command_id == "text.format.font" && candidate.available })
    );
    let picker: Value =
        serde_json::from_str(&runtime.command_line_command_json("accept", String::new())?)?;
    assert_eq!(picker["commandLine"]["mode"], "fontFamily");
    assert_eq!(picker["commandLine"]["prefix"], "\\font ");

    runtime.command_line_command_json("type", family.clone())?;
    let candidates = runtime.session.command_line_candidates();
    assert!(candidates.iter().any(|candidate| {
        candidate.command_id == "text.format.font_family"
            && candidate.value.as_deref() == Some(family.as_str())
    }));
    runtime.command_line_command_json("accept", String::new())?;
    runtime.insert_text_json("x".to_owned())?;

    let Some(caret_doc::Block::Paragraph(paragraph)) = runtime.session.document().body.first()
    else {
        return Err(std::io::Error::other("font-styled paragraph is missing").into());
    };
    let Some(caret_doc::InlineNode::Environment(environment)) = paragraph.inlines.first() else {
        return Err(std::io::Error::other("font environment was not created").into());
    };
    assert_eq!(environment.kind, caret_doc::InlineEnvironmentKind::Font);
    assert_eq!(environment.props["fontFamily"], family);
    Ok(())
}

#[test]
fn pointer_selection_cancels_command_line_preedit_before_returning_to_document_input()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "section".to_owned())?;
    runtime.input_method_event_json("preedit".to_owned(), String::new(), 0, 0, 3)?;
    let focus = runtime.session.selection().focus.clone();
    let selection = serde_json::json!({
        "anchor": {
            "paragraphId": focus.paragraph_id.to_string(),
            "offsetUtf8": focus.offset.to_string(),
            "affinity": "downstream",
            "visualAffinity": "downstream"
        },
        "focus": {
            "paragraphId": focus.paragraph_id.to_string(),
            "offsetUtf8": focus.offset.to_string(),
            "affinity": "downstream",
            "visualAffinity": "downstream"
        }
    });

    let response: Value =
        serde_json::from_str(&runtime.set_selection_json(&serde_json::to_string(&selection)?)?)?;

    assert_eq!(response["compositionCancelled"], true);
    assert_eq!(response["commandLine"]["active"], false);
    assert_eq!(response["inputMethodContext"]["surroundingText"], "");
    assert!(runtime.session.command_line().is_none());
    Ok(())
}

#[test]
fn accepting_font_candidate_during_preedit_keeps_the_ime_context_active()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "font".to_owned())?;
    runtime.command_line_command_json("accept", String::new())?;
    runtime.command_line_command_json("type", "Avenir".to_owned())?;
    runtime.input_method_event_json("preedit".to_owned(), String::new(), 0, 0, 3)?;

    let response: Value =
        serde_json::from_str(&runtime.command_line_command_json("accept", String::new())?)?;

    assert_eq!(response["commandLine"]["mode"], "fontFamily");
    assert_eq!(response["commandLine"]["preedit"], "preedit");
    assert_eq!(
        response["inputMethodContext"]["surroundingText"],
        "\\font Avenir"
    );
    assert!(runtime.session.document().body.len() == 1);
    assert!(runtime.session.command_line().is_some());
    Ok(())
}

#[test]
fn keyboard_text_insertion_updates_rust_document_and_caret_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;

    let json = runtime.insert_text_json("Aλ".to_owned())?;
    let snapshot: Value = serde_json::from_str(&json)?;
    assert_eq!(snapshot["documentGeneration"], "1");
    assert_eq!(snapshot["paragraphs"][0]["text"], "Aλ");
    assert_eq!(runtime.session.selection().focus.offset, "Aλ".len());

    let caret_json = runtime.current_caret_json()?;
    let caret: Value = serde_json::from_str(&caret_json)?;
    assert_eq!(caret["pageIndex"], 0);
    assert!(caret["xPt"].as_f64().is_some());
    assert!(caret["y1Pt"].as_f64().is_some());
    Ok(())
}

#[test]
fn interaction_positions_use_compact_utf16_checkpoints_for_unicode_text()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    let text = format!("{}🙂x", "a".repeat(70));
    let snapshot: Value = serde_json::from_str(&runtime.insert_text_json(text)?)?;
    assert_eq!(snapshot["inputMethodContext"]["cursorPosition"], 73);

    let moved: Value = serde_json::from_str(&runtime.move_visual_caret_json(false, false)?)?;
    assert_eq!(moved["inputMethodContext"]["cursorPosition"], 72);
    assert!(moved["inputMethodContext"].get("surroundingText").is_none());
    Ok(())
}

#[test]
fn keyboard_commands_split_and_merge_paragraphs_and_delete_text()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;

    let inserted = runtime.insert_text_json("Aλ".to_owned())?;
    assert!(inserted.contains("\"documentGeneration\":\"1\""));

    let split = runtime.split_paragraph_json()?;
    let split_snapshot: Value = serde_json::from_str(&split)?;
    assert_eq!(split_snapshot["documentGeneration"], "2");
    assert_eq!(
        split_snapshot["paragraphs"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(split_snapshot["paragraphs"][0]["text"], "Aλ");
    assert_eq!(split_snapshot["paragraphs"][1]["text"], "");

    let merged = runtime.delete_backward_json()?;
    let merged_snapshot: Value = serde_json::from_str(&merged)?;
    assert_eq!(merged_snapshot["documentGeneration"], "3");
    assert_eq!(
        merged_snapshot["paragraphs"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(merged_snapshot["paragraphs"][0]["text"], "Aλ");

    let deleted = runtime.delete_backward_json()?;
    let deleted_snapshot: Value = serde_json::from_str(&deleted)?;
    assert_eq!(deleted_snapshot["documentGeneration"], "4");
    assert_eq!(deleted_snapshot["paragraphs"][0]["text"], "A");
    Ok(())
}

#[test]
fn single_paragraph_undo_and_redo_publish_incremental_layout_deltas()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.layout_snapshot_json()?;
    runtime.insert_text_json("history".to_owned())?;

    let undone: Value = serde_json::from_str(&runtime.undo_json()?)?;
    assert_eq!(undone["partial"], true);
    assert_eq!(undone["baseDocumentGeneration"], "1");
    assert_eq!(undone["documentGeneration"], "2");
    assert_eq!(undone["paragraphs"][0]["text"], "");

    let redone: Value = serde_json::from_str(&runtime.redo_json()?)?;
    assert_eq!(redone["partial"], true);
    assert_eq!(redone["baseDocumentGeneration"], "2");
    assert_eq!(redone["documentGeneration"], "3");
    assert_eq!(redone["paragraphs"][0]["text"], "history");
    Ok(())
}

#[test]
fn deleting_within_a_paragraph_reuses_layout_and_returns_a_delta()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("first".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("second".to_owned())?;
    let paragraph_id = match &runtime.document().body[0] {
        caret_doc::Block::Paragraph(paragraph) => paragraph.id.to_string(),
        _ => return Err("预期第一项是段落".into()),
    };
    let endpoint = serde_json::json!({
        "paragraphId": paragraph_id,
        "offsetUtf8": "3",
        "affinity": "downstream",
        "visualAffinity": "downstream",
    });
    runtime.set_selection_json(
        &serde_json::json!({ "anchor": endpoint, "focus": endpoint }).to_string(),
    )?;

    let deleted: Value = serde_json::from_str(&runtime.delete_backward_json()?)?;

    assert_eq!(deleted["partial"], true);
    assert_eq!(deleted["baseDocumentGeneration"], "3");
    assert_eq!(deleted["documentGeneration"], "4");
    assert_eq!(deleted["paragraphs"].as_array().map(Vec::len), Some(1));
    assert_eq!(deleted["paragraphs"][0]["text"], "fist");
    Ok(())
}

#[test]
fn arrow_leaves_section_before_entering_following_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let (document, body_id, after_id) = document_with_paragraph_section_paragraph();
    let mut runtime = DocumentViewRuntime::new(document)?;
    let section_endpoint = serde_json::json!({
        "paragraphId": body_id,
        "offsetUtf8": "6",
        "affinity": "downstream",
        "visualAffinity": "downstream",
    });
    runtime.set_selection_json(
        &serde_json::json!({
            "anchor": section_endpoint,
            "focus": section_endpoint
        })
        .to_string(),
    )?;
    let outside: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, false)?)?;
    assert!(runtime.session.block_environment_exit().is_some());
    assert_eq!(
        outside["currentStructure"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(runtime.document().body.len(), 3);

    let outside_caret: Value = serde_json::from_str(&runtime.current_caret_json()?)?;
    assert_eq!(outside_caret["xPt"], outside["caretGeometry"]["xPt"]);

    runtime.move_visual_caret_json(true, false)?;
    assert!(runtime.session.block_environment_exit().is_none());
    assert_eq!(
        runtime.session.selection().focus.paragraph_id.to_string(),
        after_id
    );
    assert_eq!(runtime.session.selection().focus.offset, 0);
    for _ in 0..3 {
        runtime.move_visual_caret_json(true, false)?;
    }

    let deleted: Value = serde_json::from_str(&runtime.delete_backward_json()?)?;

    assert_eq!(deleted["paragraphs"].as_array().map(Vec::len), Some(1));
    assert_eq!(deleted["paragraphs"][0]["text"], "afer");
    assert_eq!(deleted["partial"], true);
    assert_eq!(runtime.document().body.len(), 3);
    assert!(matches!(runtime.document().body[1], Block::Environment(_)));
    Ok(())
}

#[test]
fn backspace_at_start_after_nonempty_section_does_not_delete_section_or_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let (document, _, after_id) = document_with_paragraph_section_paragraph();
    let mut runtime = DocumentViewRuntime::new(document)?;
    let endpoint = serde_json::json!({
        "paragraphId": after_id,
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream",
    });
    runtime.set_selection_json(
        &serde_json::json!({ "anchor": endpoint, "focus": endpoint }).to_string(),
    )?;

    let deleted: Value = serde_json::from_str(&runtime.delete_backward_json()?)?;

    assert_eq!(deleted["documentGeneration"], "0");
    assert_eq!(runtime.document().body.len(), 3);
    assert_eq!(deleted["paragraphs"][0]["text"], "before");
    assert_eq!(deleted["paragraphs"][1]["text"], "heading");
    assert_eq!(deleted["paragraphs"][2]["text"], "inside");
    assert_eq!(deleted["paragraphs"][3]["text"], "after");
    Ok(())
}

#[test]
fn backspace_after_empty_section_merges_into_the_section_title()
-> Result<(), Box<dyn std::error::Error>> {
    let title = paragraph_with_text("Heading");
    let title_id = title.id.clone();
    let section_id = caret_doc::NodeId::generate();
    let following = paragraph_with_text("body");
    let following_id = following.id.clone();
    let mut document = Document::blank();
    document.body = vec![
        Block::Environment(caret_doc::BlockEnvironment {
            id: section_id.clone(),
            kind: caret_doc::BlockEnvironmentKind::Section,
            definition_id: None,
            props: Default::default(),
            title: Some(title),
            children: Vec::new(),
            extra: Default::default(),
        }),
        Block::Paragraph(following),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let start = serde_json::json!({
        "paragraphId": following_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": start.clone(), "focus": start}).to_string(),
    )?;

    runtime.delete_backward_json()?;

    assert_eq!(runtime.document().body.len(), 1);
    let Block::Environment(section) = &runtime.document().body[0] else {
        return Err("Section should remain after merging its following paragraph".into());
    };
    let title = section
        .title
        .as_ref()
        .ok_or_else(|| std::io::Error::other("Section title should remain"))?;
    assert_eq!(title.id, title_id);
    assert_eq!(paragraph_text(title), "Headingbody");
    assert_eq!(section.id, section_id);
    assert_eq!(runtime.session.selection().focus.paragraph_id, title_id);
    assert_eq!(
        runtime.session.selection().focus.story,
        caret_engine::StoryId::EnvironmentTitle(section_id)
    );
    assert_eq!(runtime.session.selection().focus.offset, "Heading".len());

    runtime.undo_json()?;
    assert_eq!(runtime.document().body.len(), 2);
    let Block::Environment(restored_section) = &runtime.document().body[0] else {
        return Err("Undo should restore the Section".into());
    };
    assert_eq!(
        restored_section.title.as_ref().map(paragraph_text),
        Some("Heading".to_owned())
    );
    assert_eq!(
        paragraph_text(match &runtime.document().body[1] {
            Block::Paragraph(paragraph) => paragraph,
            _ => return Err("Undo should restore the following paragraph".into()),
        }),
        "body"
    );

    runtime.redo_json()?;
    assert_eq!(runtime.document().body.len(), 1);
    let Block::Environment(redone_section) = &runtime.document().body[0] else {
        return Err("Redo should keep the Section".into());
    };
    assert_eq!(
        redone_section.title.as_ref().map(paragraph_text),
        Some("Headingbody".to_owned())
    );
    Ok(())
}

#[test]
fn backspace_on_keyboard_selected_environment_deletes_and_restores_its_subtree()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut document, body_id, _) = document_with_paragraph_section_paragraph();
    let Block::Environment(section) = &mut document.body[1] else {
        return Err("预期第二个块为 Section".into());
    };
    section
        .children
        .push(Block::Environment(caret_doc::BlockEnvironment {
            id: caret_doc::NodeId::generate(),
            kind: caret_doc::BlockEnvironmentKind::Custom,
            definition_id: None,
            props: Default::default(),
            title: None,
            children: vec![Block::Paragraph(paragraph_with_text("nested"))],
            extra: Default::default(),
        }));
    let section_id = section.id.clone();
    let nested_id = match &section.children[1] {
        Block::Environment(environment) => environment.id.clone(),
        _ => return Err("预期 Section 中包含嵌套环境".into()),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let endpoint = serde_json::json!({
        "paragraphId": body_id,
        "offsetUtf8": "6",
        "affinity": "upstream",
        "visualAffinity": "upstream",
    });
    runtime.set_selection_json(
        &serde_json::json!({ "anchor": endpoint, "focus": endpoint }).to_string(),
    )?;

    runtime.move_visual_caret_json(true, true)?;
    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&section_id)
    );
    let deleted: Value = serde_json::from_str(&runtime.delete_backward_json()?)?;
    assert_eq!(runtime.document().body.len(), 2);
    assert!(matches!(runtime.document().body[0], Block::Paragraph(_)));
    assert!(matches!(runtime.document().body[1], Block::Paragraph(_)));
    assert_eq!(deleted["documentGeneration"], "1");

    runtime.undo_json()?;
    assert_eq!(runtime.document().body.len(), 3);
    let Block::Environment(restored) = &runtime.document().body[1] else {
        return Err("撤销后应恢复 Section".into());
    };
    assert_eq!(restored.id, section_id);
    let Block::Environment(restored_nested) = &restored.children[1] else {
        return Err("撤销后应恢复嵌套环境".into());
    };
    assert_eq!(restored_nested.id, nested_id);

    runtime.redo_json()?;
    assert_eq!(runtime.document().body.len(), 2);
    assert!(matches!(runtime.document().body[1], Block::Paragraph(_)));
    Ok(())
}

#[test]
fn backspace_deletes_the_entire_paragraph_section_paragraph_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let (document, _, after_id) = document_with_paragraph_section_paragraph();
    let before_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.to_string(),
        _ => return Err("预期第一个块为段落".into()),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    let anchor = serde_json::json!({
        "paragraphId": before_id,
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let focus = serde_json::json!({
        "paragraphId": after_id,
        "offsetUtf8": "5",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime
        .set_selection_json(&serde_json::json!({"anchor": anchor, "focus": focus}).to_string())?;

    runtime.delete_backward_json()?;

    assert_eq!(runtime.document().body.len(), 2);
    assert!(
        matches!(&runtime.document().body[0], Block::Paragraph(paragraph) if paragraph_text(paragraph).is_empty())
    );
    assert!(
        matches!(&runtime.document().body[1], Block::Paragraph(paragraph) if paragraph_text(paragraph).is_empty())
    );
    runtime.undo_json()?;
    assert_eq!(runtime.document().body.len(), 3);
    assert!(
        matches!(&runtime.document().body[0], Block::Paragraph(paragraph) if paragraph_text(paragraph) == "before")
    );
    assert!(matches!(&runtime.document().body[1], Block::Environment(_)));
    assert!(
        matches!(&runtime.document().body[2], Block::Paragraph(paragraph) if paragraph_text(paragraph) == "after")
    );

    runtime.redo_json()?;
    assert_eq!(runtime.document().body.len(), 2);
    assert!(
        matches!(&runtime.document().body[0], Block::Paragraph(paragraph) if paragraph_text(paragraph).is_empty())
    );
    assert!(
        matches!(&runtime.document().body[1], Block::Paragraph(paragraph) if paragraph_text(paragraph).is_empty())
    );
    Ok(())
}

#[test]
fn backspace_deletes_multiple_selected_sections_and_intervening_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let first_title = paragraph_with_text("first");
    let first_title_id = first_title.id.to_string();
    let second_title = paragraph_with_text("second");
    let second_title_id = second_title.id.to_string();
    let make_section = |title| {
        Block::Environment(caret_doc::BlockEnvironment {
            id: caret_doc::NodeId::generate(),
            kind: caret_doc::BlockEnvironmentKind::Section,
            definition_id: None,
            props: Default::default(),
            title: Some(title),
            children: vec![Block::Paragraph(paragraph_with_text("body"))],
            extra: Default::default(),
        })
    };
    let mut document = Document::blank();
    document.body = vec![
        make_section(first_title),
        Block::Paragraph(paragraph_with_text("middle")),
        make_section(second_title),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let anchor = serde_json::json!({
        "paragraphId": first_title_id,
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let focus = serde_json::json!({
        "paragraphId": second_title_id,
        "offsetUtf8": "6",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime
        .set_selection_json(&serde_json::json!({"anchor": anchor, "focus": focus}).to_string())?;
    assert_eq!(runtime.session.selected_block_environments().len(), 2);

    runtime.delete_backward_json()?;

    assert_eq!(runtime.document().body.len(), 1);
    assert!(
        matches!(&runtime.document().body[0], Block::Paragraph(paragraph) if paragraph_text(paragraph).is_empty())
    );
    runtime.undo_json()?;
    assert_eq!(runtime.document().body.len(), 3);
    assert!(matches!(&runtime.document().body[0], Block::Environment(_)));
    assert!(
        matches!(&runtime.document().body[1], Block::Paragraph(paragraph) if paragraph_text(paragraph) == "middle")
    );
    assert!(matches!(&runtime.document().body[2], Block::Environment(_)));
    Ok(())
}
