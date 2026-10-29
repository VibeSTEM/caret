//! 验证键盘文本输入、段落命令与输入法上下文。

use super::*;

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
fn editor_escape_reports_ime_cancellation_to_the_view() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.input_method_event_json("preedit".to_owned(), String::new(), 0, 0, 3)?;
    assert!(runtime.session.command_line().is_none());
    assert!(runtime.session.composition().is_some());

    let composition_cancelled: Value =
        serde_json::from_str(&runtime.handle_editor_key_json("escape")?)?;
    assert_eq!(composition_cancelled["compositionCancelled"], true);
    assert!(runtime.session.composition().is_none());
    assert_eq!(composition_cancelled["paragraphs"][0]["text"], "");
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
