//! 验证文档视图布局快照、绘制与错误隔离。

use super::*;
use caret_doc::{
    BlockEnvironment, BlockEnvironmentKind, InlineEnvironment, InlineEnvironmentKind, NodeId,
    Paragraph, ParagraphContent, Run,
};
use std::collections::BTreeMap;

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
            source_range: None,
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
        source_range: None,
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
