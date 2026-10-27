//! 验证键盘文本输入、段落命令与输入法上下文。

use super::*;

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
