//! 验证文档视图布局快照、绘制与错误隔离。

use super::*;

#[test]
fn blank_document_snapshot_contains_page_and_paragraph_layout()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    let json = runtime.layout_snapshot_json()?;
    let snapshot: Value = serde_json::from_str(&json)?;

    assert_eq!(snapshot["documentGeneration"], "0");
    assert_eq!(snapshot["fontGeneration"], "1");
    assert_eq!(snapshot["pages"].as_array().map(Vec::len), Some(1));
    assert_eq!(snapshot["pages"][0]["widthPt"], 595.2755905511812);
    assert_eq!(snapshot["paragraphs"].as_array().map(Vec::len), Some(1));
    assert_eq!(snapshot["paragraphs"][0]["text"], "");
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
fn visible_page_rasterization_returns_binary_glyph_images() -> Result<(), Box<dyn std::error::Error>>
{
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should contain a paragraph");
    };
    paragraph.inlines = vec![InlineNode::Run(Run {
        text: "A".to_owned(),
        language: Some("en".to_owned()),
        props: Default::default(),
        extra: Default::default(),
    })];
    let mut runtime = DocumentViewRuntime::new(document)?;

    let page = runtime.rasterize_page_glyphs(0, 1.0, [0.25, 0.75], [32, 32, 32, 255])?;
    assert_eq!(page.page_index, 0);
    assert_eq!(page.document_generation, 0);
    assert_eq!(page.font_generation, 1);
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
