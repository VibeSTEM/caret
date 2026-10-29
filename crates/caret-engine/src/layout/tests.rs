//! 验证文档布局、分页和命中测试。

use crate::{Affinity, Command, Pos, Selection, Session, StoryId};
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, ExplicitBreak, InlineNode, NodeId,
    OpaqueNode, Paragraph, Run, SoftBreak,
};
use caret_layout::{
    FontCatalog, FontCatalogError, ResolvedDocumentTextSettings, ShapingStyle, VisualAffinity,
};
use icu_locale_core::subtags::Script;
use std::collections::{BTreeMap, HashMap};

use super::{DocumentLayoutError, DocumentLayoutService, MAX_BASELINE_ANCHOR_VARIANTS};

fn script(value: &str) -> Script {
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid script subtag {value}: {error}"))
}

fn baseline_cache_contains(
    cache: &super::BaselineAnchorCache,
    language: Option<&str>,
    script: Option<&str>,
) -> bool {
    cache
        .metrics_by_language_script
        .get(language.unwrap_or(""))
        .is_some_and(|scripts| {
            scripts
                .keys()
                .any(|key| key.map(|(cached_script, _)| cached_script) == script.map(self::script))
        })
}

fn paragraph(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![InlineNode::Run(Run {
        text: text.to_owned(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    })];
    paragraph
}

fn single_line_y_offset(snapshot: &super::DocumentLayoutSnapshot, paragraph_id: &NodeId) -> f32 {
    snapshot.pages[0]
        .layout
        .fragments
        .iter()
        .find(|fragment| &fragment.paragraph_id == paragraph_id)
        .unwrap_or_else(|| panic!("paragraph {paragraph_id} is missing from the first page"))
        .y_offset_pt
}

#[test]
fn cjk_text_keeps_paragraph_spacing_fixed_and_environment_tracks_line_boxes() {
    fn layout_section(title_text: &str, body_text: &str) -> super::DocumentLayoutSnapshot {
        let before = paragraph("Before");
        let before_id = before.id.clone();
        let title = paragraph(title_text);
        let title_id = title.id.clone();
        let body = paragraph(body_text);
        let body_id = body.id.clone();
        let after = paragraph("After");
        let after_id = after.id.clone();
        let environment_id = NodeId::generate();

        let mut document = Document::blank();
        document.body = vec![
            Block::Paragraph(before),
            Block::Environment(BlockEnvironment {
                id: environment_id.clone(),
                kind: BlockEnvironmentKind::Section,
                definition_id: None,
                props: BTreeMap::new(),
                title: Some(title),
                children: vec![Block::Paragraph(body)],
                extra: BTreeMap::new(),
            }),
            Block::Paragraph(after),
        ];
        let mut service = DocumentLayoutService::new();
        let mut catalog =
            FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
        let snapshot = service
            .layout(&document, 1, &mut catalog, &ShapingStyle::new(1200))
            .unwrap_or_else(|error| panic!("document layout failed: {error}"));

        let before_height = snapshot
            .paragraph_by_id(&before_id)
            .unwrap_or_else(|| panic!("preceding paragraph is missing"))
            .layout
            .paragraph
            .lines[0]
            .metrics
            .height_pt;
        let title_height = snapshot
            .paragraph_by_id(&title_id)
            .unwrap_or_else(|| panic!("Section title paragraph is missing"))
            .layout
            .paragraph
            .lines[0]
            .metrics
            .height_pt;
        let body_height = snapshot
            .paragraph_by_id(&body_id)
            .unwrap_or_else(|| panic!("Section body paragraph is missing"))
            .layout
            .paragraph
            .lines[0]
            .metrics
            .height_pt;
        let after_y = single_line_y_offset(&snapshot, &after_id);
        let title_y = single_line_y_offset(&snapshot, &title_id);
        let body_y = single_line_y_offset(&snapshot, &body_id);
        assert!((title_y - before_height - 10.0).abs() < 0.01);
        assert!((body_y - title_y - title_height - 10.0).abs() < 0.01);
        assert!((after_y - body_y - body_height - 10.0).abs() < 0.01);

        let environment_range = snapshot
            .environment_ranges
            .iter()
            .find(|range| range.environment_id == environment_id)
            .unwrap_or_else(|| panic!("Section highlight range is missing"));
        assert!(
            (environment_range.y1_pt
                - environment_range.y0_pt
                - f64::from(title_height + 10.0 + body_height))
            .abs()
                < 0.01
        );

        snapshot
    }

    let latin = layout_section("Section title", "Section body");
    let cjk = layout_section("章节标题", "正文内容");
    let mixed = layout_section("Section 标题", "正文 content");
    let latin_line_height = latin.paragraphs[1].layout.paragraph.lines[0]
        .metrics
        .height_pt;
    let cjk_line_height = cjk.paragraphs[1].layout.paragraph.lines[0]
        .metrics
        .height_pt;

    // 行框和环境高亮容纳实际字体度量；固定段距仍由文档样式精确控制。
    let mixed_line_height = mixed.paragraphs[1].layout.paragraph.lines[0]
        .metrics
        .height_pt;
    assert!(
        latin_line_height.is_finite()
            && cjk_line_height.is_finite()
            && mixed_line_height.is_finite()
    );
}

#[test]
fn blank_page_click_uses_nearest_line_for_caret_placement() {
    let document = Document::blank();
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let snapshot = service
        .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("document layout failed: {error}"));
    let page = &snapshot.pages[0];

    let hit = snapshot
        .hit_test(0, page.width_pt / 2.0, page.height_pt / 2.0)
        .unwrap_or_else(|| panic!("blank page click did not find a caret stop"));

    assert_eq!(hit.position.offset, 0);
    assert_eq!(hit.position.paragraph_id, snapshot.paragraphs[0].id);
    assert_eq!(hit.caret.page_index, 0);
    assert_eq!(hit.caret.y0_pt, page.margin_top_pt);
}

#[test]
fn cloning_laid_out_paragraph_shares_immutable_shaping_data() {
    let document = Document::blank();
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let snapshot = service
        .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("document layout failed: {error}"));
    let paragraph = &snapshot.paragraphs[0];
    let reused = paragraph.reused_for_layout(StoryId::Body, &[]);

    assert!(std::sync::Arc::ptr_eq(&paragraph.text, &reused.text));
    assert!(std::sync::Arc::ptr_eq(&paragraph.layout, &reused.layout));
    assert!(std::sync::Arc::ptr_eq(
        &paragraph.utf16_checkpoints,
        &reused.utf16_checkpoints
    ));
    assert_eq!(paragraph.line_locations.len(), reused.line_locations.len());
    assert!(reused.line_locations.iter().all(Option::is_none));
}

#[test]
fn document_layout_snapshot_uses_parley_lines_and_pagination() {
    let mut document = Document::blank();
    let text = "A mixed run sequence: e\u{301}, family 👩‍👩‍👧‍👦, and text that wraps. ".repeat(64);
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.language = Some("en".to_owned());
    paragraph.inlines = vec![InlineNode::Run(Run {
        text: text.clone(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    })];

    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let snapshot = service
        .layout(&document, 7, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("document layout failed: {error}"));

    assert_eq!(snapshot.document_generation, 7);
    assert_eq!(snapshot.font_generation, catalog.generation());
    assert_eq!(snapshot.paragraphs.len(), 1);
    assert_eq!(snapshot.paragraphs[0].text.as_ref(), text);
    assert!(snapshot.paragraphs[0].layout.paragraph.lines.len() > 1);
    assert!(!snapshot.pages.is_empty());
    assert_eq!(
        snapshot.pages[0].layout.fragments[0].paragraph_id,
        snapshot.paragraphs[0].id
    );
    assert!(snapshot.pages.len() > 1);

    let first_line = &snapshot.paragraphs[0].layout.paragraph.lines[0];
    let first_cursor = snapshot.paragraphs[0]
        .layout
        .cursor_at(first_line.source_bytes.start, VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("first line start is not a cursor stop"));
    let cursor_rect = snapshot.paragraphs[0]
        .layout
        .cursor_geometry(first_cursor, 451.0);
    let hit = snapshot
        .hit_test(
            0,
            snapshot.pages[0].margin_left_pt + cursor_rect.x0 + 0.1,
            snapshot.pages[0].margin_top_pt + cursor_rect.y0 + 1.0,
        )
        .unwrap_or_else(|| panic!("first-page hit did not resolve"));
    assert_eq!(hit.position.paragraph_id, snapshot.paragraphs[0].id);
    assert_eq!(hit.position.offset, first_line.source_bytes.start);
    assert_eq!(hit.position.affinity, Affinity::Downstream);
    assert_eq!(hit.caret.page_index, 0);
    assert!((hit.caret.x_pt - (snapshot.pages[0].margin_left_pt + cursor_rect.x0)).abs() < 0.01);
    assert!((hit.caret.y0_pt - snapshot.pages[0].margin_top_pt).abs() < 0.01);
    assert!(
        (hit.caret.y1_pt - hit.caret.y0_pt - f64::from(first_line.metrics.height_pt)).abs() < 0.01
    );
    let hit_view = snapshot.view_for_hit(&hit);
    assert_eq!(hit_view.caret.page_index, 0);
    assert!((hit_view.caret.x_pt - hit.caret.x_pt).abs() < f64::EPSILON);

    let first_fragment = &snapshot.pages[0].layout.fragments[0];
    let fragment_line_index =
        first_fragment.line_range.start + (first_fragment.line_range.len().saturating_sub(1) / 2);
    let fragment_line = &snapshot.paragraphs[0].layout.paragraph.lines[fragment_line_index];
    let line_offset = snapshot.paragraphs[0].layout.paragraph.lines
        [first_fragment.line_range.start..fragment_line_index]
        .iter()
        .map(|line| f64::from(line.metrics.height_pt))
        .sum::<f64>();
    let fragment_line_cursor = snapshot.paragraphs[0]
        .layout
        .cursor_at(fragment_line.source_bytes.start, VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("fragment line start is not a cursor stop"));
    let fragment_line_cursor_rect = snapshot.paragraphs[0]
        .layout
        .cursor_geometry(fragment_line_cursor, 0.0);
    let fragment_line_hit = snapshot
        .hit_test(
            0,
            snapshot.pages[0].margin_left_pt + fragment_line_cursor_rect.x0,
            snapshot.pages[0].margin_top_pt
                + f64::from(first_fragment.y_offset_pt)
                + line_offset
                + f64::from(fragment_line.metrics.height_pt) / 2.0,
        )
        .unwrap_or_else(|| panic!("fragment line hit did not resolve"));
    assert_eq!(fragment_line_hit.caret.page_index, 0);
    assert!(
        (fragment_line_hit.caret.y0_pt
            - (snapshot.pages[0].margin_top_pt
                + f64::from(first_fragment.y_offset_pt)
                + line_offset))
            .abs()
            < 0.01
    );

    let last_page_index = snapshot.pages.len() - 1;
    let last_page = &snapshot.pages[last_page_index];
    let last_fragment = &last_page.layout.fragments[0];
    let last_line_index = last_fragment.line_range.start;
    let last_line = &snapshot.paragraphs[0].layout.paragraph.lines[last_line_index];
    let last_line_cursor = snapshot.paragraphs[0]
        .layout
        .cursor_at(last_line.source_bytes.start, VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("last page line start is not a cursor stop"));
    let last_line_cursor_rect = snapshot.paragraphs[0]
        .layout
        .cursor_geometry(last_line_cursor, 0.0);
    let last_page_hit = snapshot
        .hit_test(
            last_page_index,
            last_page.margin_left_pt + last_line_cursor_rect.x0,
            last_page.margin_top_pt
                + f64::from(last_fragment.y_offset_pt + last_line.metrics.height_pt / 2.0),
        )
        .unwrap_or_else(|| panic!("last page hit did not resolve"));
    assert_eq!(last_page_hit.caret.page_index, last_page_index);
    assert!(last_page_hit.caret.y0_pt >= last_page.margin_top_pt);
    assert!(last_page_hit.caret.y1_pt <= last_page.height_pt - last_page.margin_bottom_pt);

    let boundary_cursor = snapshot.paragraphs[0]
        .layout
        .cursor_at(1, VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("LTR boundary is not a cursor stop"));
    let boundary_rect = snapshot.paragraphs[0]
        .layout
        .cursor_geometry(boundary_cursor, 0.0);
    let boundary_y =
        snapshot.pages[0].margin_top_pt + f64::from(first_line.metrics.height_pt) / 2.0;
    let left_hit = snapshot
        .hit_test(
            0,
            snapshot.pages[0].margin_left_pt + boundary_rect.x0 - 0.01,
            boundary_y,
        )
        .unwrap_or_else(|| panic!("left boundary hit did not resolve"));
    let right_hit = snapshot
        .hit_test(
            0,
            snapshot.pages[0].margin_left_pt + boundary_rect.x0 + 0.01,
            boundary_y,
        )
        .unwrap_or_else(|| panic!("right boundary hit did not resolve"));
    assert_eq!(left_hit.position.offset, 1);
    assert_eq!(left_hit.position.affinity, Affinity::Upstream);
    assert_eq!(right_hit.position.offset, 1);
    assert_eq!(right_hit.position.affinity, Affinity::Downstream);

    let last_fragment = &snapshot.pages[0].layout.fragments[0];
    let last_page_line_index = last_fragment.line_range.end - 1;
    let last_page_line = &snapshot.paragraphs[0].layout.paragraph.lines[last_page_line_index];
    let fragment_preceding_height = snapshot.paragraphs[0].layout.paragraph.lines
        [last_fragment.line_range.start..last_page_line_index]
        .iter()
        .map(|line| line.metrics.height_pt)
        .sum::<f32>();
    let last_line_bottom = snapshot.pages[0].margin_top_pt
        + f64::from(last_fragment.y_offset_pt + fragment_preceding_height)
        + f64::from(last_page_line.metrics.height_pt);
    let bottom_hit = snapshot
        .hit_test(
            0,
            snapshot.pages[0].margin_left_pt + f64::from(last_page_line.advance_pt / 2.0),
            last_line_bottom - 1.0,
        )
        .unwrap_or_else(|| panic!("page-end line hit did not resolve"));
    assert_eq!(bottom_hit.position.paragraph_id, snapshot.paragraphs[0].id);
    assert!(
        (last_page_line.source_bytes.start..=last_page_line.consumed_bytes.end)
            .contains(&bottom_hit.position.offset)
    );

    let selection = Selection {
        anchor: Pos {
            story: StoryId::Body,
            paragraph_id: snapshot.paragraphs[0].id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        },
        focus: Pos {
            story: StoryId::Body,
            paragraph_id: snapshot.paragraphs[0].id.clone(),
            offset: text.len(),
            affinity: Affinity::Upstream,
        },
    };
    let selected = snapshot
        .selection_geometry(&selection)
        .unwrap_or_else(|| panic!("cross-page selection geometry failed"));
    assert!(selected.iter().any(|rect| rect.page_index == 0));
    assert!(
        selected
            .iter()
            .any(|rect| rect.page_index + 1 == snapshot.pages.len())
    );
    assert!(selected.iter().all(|rect| rect.x1_pt > rect.x0_pt));
    let first_page_rects = selected
        .iter()
        .filter(|rect| rect.page_index == 0)
        .collect::<Vec<_>>();
    let min_first_page_y = first_page_rects
        .iter()
        .map(|rect| rect.y0_pt)
        .fold(f64::INFINITY, f64::min);
    let max_first_page_y = first_page_rects
        .iter()
        .map(|rect| rect.y0_pt)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(max_first_page_y > min_first_page_y);
}

#[test]
fn long_paragraph_selection_uses_page_local_line_positions() {
    let mut document = Document::blank();
    let text = "a\n".repeat(20_000);
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines = vec![InlineNode::Run(Run {
        text: text.clone(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    })];

    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let snapshot = service
        .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("document layout failed: {error}"));
    let paragraph = &snapshot.paragraphs[0];
    let line_index = paragraph.layout.paragraph.lines.len() - 20;
    let line = &paragraph.layout.paragraph.lines[line_index];
    let page_index = snapshot
        .pages
        .iter()
        .position(|page| {
            page.layout.fragments.iter().any(|fragment| {
                fragment.paragraph_id == paragraph.id && fragment.line_range.contains(&line_index)
            })
        })
        .unwrap_or_else(|| panic!("late paragraph line has no page"));
    let position = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph.id.clone(),
        offset: line.source_bytes.start,
        affinity: Affinity::Downstream,
    };
    let caret = snapshot
        .caret_geometry(&position, VisualAffinity::Downstream)
        .unwrap_or_else(|| panic!("late paragraph caret geometry failed"));
    assert_eq!(caret.page_index, page_index);
    let selection = Selection {
        anchor: position.clone(),
        focus: Pos {
            offset: line.consumed_bytes.end,
            affinity: Affinity::Upstream,
            ..position
        },
    };
    let selected = snapshot
        .selection_geometry(&selection)
        .unwrap_or_else(|| panic!("late paragraph selection geometry failed"));
    let rect = selected
        .iter()
        .find(|rect| rect.page_index == page_index)
        .unwrap_or_else(|| panic!("late paragraph selection has no page-local rect"));

    assert!((rect.y0_pt - caret.y0_pt).abs() < 0.000_001);
    assert!((rect.y1_pt - caret.y1_pt).abs() < 0.000_001);
}

#[test]
fn stale_document_or_font_layout_snapshot_is_rejected() {
    let session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let snapshot = service
        .layout_session(&session, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("document layout failed: {error}"));

    assert!(snapshot.is_current_for(&session, catalog.generation()));
    assert!(!snapshot.is_current_for(&session, catalog.generation().saturating_add(1)));

    let mut edited = session;
    edited
        .run(Command::InsertText("changed".to_owned()))
        .unwrap_or_else(|error| panic!("document edit failed: {error}"));
    assert!(!snapshot.is_current_for(&edited, catalog.generation()));
}

#[test]
fn layout_errors_reject_the_entire_document_and_leave_the_service_reusable() {
    let mut invalid_model = Document::blank();
    invalid_model.page_sections.clear();
    let mut unknown_break = Document::blank();
    unknown_break.body.push(Block::ExplicitBreak(ExplicitBreak {
        kind: "column".to_owned(),
        extra: BTreeMap::new(),
    }));
    let mut opaque_block = Document::blank();
    opaque_block.body.push(Block::Opaque(OpaqueNode {
        raw: serde_json::json!({"type": "future_block", "content": "preserved"}),
    }));
    let mut unknown_empty_mark = Document::blank();
    let Block::Paragraph(empty) = &mut unknown_empty_mark.body[0] else {
        unreachable!();
    };
    empty
        .mark_style
        .insert("futureMetric".to_owned(), serde_json::json!(true));
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));

    for (document, expected_prefix) in [
        (invalid_model, "layout.invalid_document:"),
        (unknown_break, "layout.unknown_break_kind: column"),
        (opaque_block, "layout.unsupported_content"),
        (
            unknown_empty_mark,
            "layout.unsupported_paragraph_mark_style",
        ),
    ] {
        let Err(error) = service.layout(&document, 7, &mut catalog, &ShapingStyle::new(1200))
        else {
            panic!("unsupported input must not publish a partial snapshot");
        };
        assert!(error.to_string().starts_with(expected_prefix));
        let valid = service
            .layout(
                &Document::blank(),
                8,
                &mut catalog,
                &ShapingStyle::new(1200),
            )
            .unwrap_or_else(|error| panic!("service failed after rejected input: {error}"));
        assert_eq!(valid.document_generation, 8);
        assert_eq!(valid.paragraphs.len(), 1);
        assert_eq!(valid.pages.len(), 1);
    }
}

#[test]
fn block_environment_titles_and_children_share_document_layout_and_hit_testing() {
    let mut document = Document::blank();
    let title = paragraph("Section title");
    let title_id = title.id.clone();
    let environment_id = NodeId::generate();
    let child = paragraph("nested body");
    let child_id = child.id.clone();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(title),
        children: vec![Block::Paragraph(child)],
        extra: BTreeMap::new(),
    })];
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let snapshot = service
        .layout(&document, 1, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("environment layout failed: {error}"));

    assert_eq!(snapshot.paragraphs.len(), 2);
    assert_eq!(
        snapshot.paragraph_by_id(&title_id).map(|p| &p.story),
        Some(&StoryId::EnvironmentTitle(environment_id.clone()))
    );
    assert_eq!(
        snapshot
            .paragraph_by_id(&child_id)
            .map(|p| p.environment_ids.as_ref()),
        Some([environment_id.clone()].as_slice())
    );
    assert_eq!(snapshot.environment_ranges.len(), 1);
    assert_eq!(
        snapshot.environment_ranges[0].environment_id,
        environment_id
    );
    assert_eq!(snapshot.environment_ranges[0].page_index, 0);
    assert!(snapshot.environment_ranges[0].y1_pt > snapshot.environment_ranges[0].y0_pt);
    let title_y = f64::from(snapshot.pages[0].layout.fragments[0].y_offset_pt);
    let Some(title_hit) = snapshot.hit_test(
        0,
        snapshot.pages[0].margin_left_pt + 1.0,
        snapshot.pages[0].margin_top_pt + title_y + 1.0,
    ) else {
        panic!("laid-out environment title should be hittable");
    };
    assert_eq!(
        title_hit.position.story,
        StoryId::EnvironmentTitle(environment_id)
    );
}

#[test]
fn body_selection_geometry_skips_environment_title_stories() {
    let mut document = Document::blank();
    let before = paragraph("before");
    let before_id = before.id.clone();
    let title = paragraph("title");
    let environment_id = NodeId::generate();
    let child = paragraph("inside");
    let after = paragraph("after");
    let after_id = after.id.clone();
    document.body = vec![
        Block::Paragraph(before),
        Block::Environment(BlockEnvironment {
            id: environment_id.clone(),
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: BTreeMap::new(),
            title: Some(title),
            children: vec![Block::Paragraph(child)],
            extra: BTreeMap::new(),
        }),
        Block::Paragraph(after),
    ];
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let snapshot = service
        .layout(&document, 1, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("environment layout failed: {error}"));

    let rectangles = snapshot
        .selection_geometry(&Selection {
            anchor: Pos {
                story: StoryId::Body,
                paragraph_id: before_id,
                offset: 0,
                affinity: Affinity::Downstream,
            },
            focus: Pos {
                story: StoryId::Body,
                paragraph_id: after_id,
                offset: "after".len(),
                affinity: Affinity::Downstream,
            },
        })
        .unwrap_or_else(|| panic!("body selection across the environment was rejected"));

    assert_eq!(rectangles.len(), 3);
    let title_paragraph = snapshot
        .paragraph_by_id(
            snapshot
                .paragraphs
                .iter()
                .find(|paragraph| {
                    paragraph.story == StoryId::EnvironmentTitle(environment_id.clone())
                })
                .map(|paragraph| &paragraph.id)
                .unwrap_or_else(|| panic!("environment title layout is missing")),
        )
        .unwrap_or_else(|| panic!("environment title lookup failed"));
    let title_fragment = snapshot.pages[0]
        .layout
        .fragments
        .iter()
        .find(|fragment| fragment.paragraph_id == title_paragraph.id)
        .unwrap_or_else(|| panic!("environment title fragment is missing"));
    let title_y = snapshot.pages[0].margin_top_pt + f64::from(title_fragment.y_offset_pt);
    assert!(
        !rectangles
            .iter()
            .any(|rectangle| { rectangle.y0_pt <= title_y && title_y < rectangle.y1_pt })
    );
}

#[test]
fn unavailable_font_catalog_keeps_its_layout_error_category() {
    let error = DocumentLayoutError::from(FontCatalogError::Empty);
    assert!(
        matches!(&error, DocumentLayoutError::FontCatalog(detail) if detail == "layout.font_catalog_empty")
    );
    assert_eq!(
        error.to_string(),
        "layout.font_catalog: layout.font_catalog_empty"
    );
}

#[test]
fn failed_font_measurement_preserves_the_last_valid_baseline_cache() {
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let settings =
        ResolvedDocumentTextSettings::from_document_settings(&Document::blank().global_settings)
            .unwrap_or_else(|error| panic!("text settings rejected: {error}"));
    let style = ShapingStyle::new(1200);
    let valid = service
        .resolve_baseline_anchor_metrics(&mut catalog, &settings, &style, None, None)
        .unwrap_or_else(|error| panic!("font measurement failed: {error}"));
    let saved_cache = service.baseline_anchor_cache.clone();

    let Err(error) = service.resolve_baseline_anchor_metrics(
        &mut catalog,
        &settings,
        &ShapingStyle::new(0),
        None,
        None,
    ) else {
        panic!("zero-sized font must not produce baseline metrics");
    };
    assert!(matches!(error, DocumentLayoutError::Shaping(_)));
    assert_eq!(
        error.to_string(),
        "layout.shaping: layout.invalid_font_size"
    );
    assert_eq!(service.baseline_anchor_cache, saved_cache);
    assert_eq!(
        service
            .resolve_baseline_anchor_metrics(&mut catalog, &settings, &style, None, None)
            .unwrap_or_else(|error| panic!("valid cache was lost: {error}")),
        valid
    );
}

#[test]
fn baseline_cache_tracks_style_language_and_refreshed_font_generation() {
    let mut document = Document::blank();
    let session = Session::new(document.clone())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let style = ShapingStyle::new(1200);
    let first = service
        .layout_session(&session, &mut catalog, &style)
        .unwrap_or_else(|error| panic!("first layout failed: {error}"));
    let first_cache = service
        .baseline_anchor_cache
        .as_ref()
        .unwrap_or_else(|| panic!("successful layout needs a baseline cache"));
    let first_font_generation = first_cache.font_generation;
    let first_settings = first_cache.settings.clone();
    let first_style = first_cache.style.clone();
    let first_metrics = first_cache
        .metrics_by_language_script
        .iter()
        .flat_map(|(language, scripts)| {
            scripts
                .iter()
                .map(move |(script, cached)| ((language.clone(), *script), cached.metrics))
        })
        .collect::<HashMap<_, _>>();
    let same = service
        .layout_session(&session, &mut catalog, &style)
        .unwrap_or_else(|error| panic!("repeated layout failed: {error}"));
    let repeated_cache = service
        .baseline_anchor_cache
        .as_ref()
        .unwrap_or_else(|| panic!("repeated layout discarded its baseline cache"));
    assert_eq!(repeated_cache.font_generation, first_font_generation);
    assert_eq!(repeated_cache.settings, first_settings);
    assert_eq!(repeated_cache.style, first_style);
    assert_eq!(
        repeated_cache
            .metrics_by_language_script
            .iter()
            .flat_map(|(language, scripts)| {
                scripts
                    .iter()
                    .map(move |(script, cached)| ((language.clone(), *script), cached.metrics))
            })
            .collect::<HashMap<_, _>>(),
        first_metrics
    );
    assert_eq!(same.pages, first.pages);

    document.global_settings.default_language = Some("ru".to_owned());
    let changed_settings =
        ResolvedDocumentTextSettings::from_document_settings(&document.global_settings)
            .unwrap_or_else(|error| panic!("language settings rejected: {error}"));
    let larger_style = ShapingStyle {
        font_size_centipoints: 2400,
        bold: true,
        italic: true,
        language: Some("el".to_owned()),
        requested_family: Some("Caret Missing Baseline Font".to_owned()),
    };
    let larger_script = (script("Cyrl"), 'ж');
    let larger_metrics = service
        .resolve_baseline_anchor_metrics(
            &mut catalog,
            &changed_settings,
            &larger_style,
            changed_settings.default_language(),
            Some(&larger_script),
        )
        .unwrap_or_else(|error| panic!("changed baseline failed: {error}"));
    let changed_cache = service
        .baseline_anchor_cache
        .as_ref()
        .unwrap_or_else(|| panic!("changed style needs a new cache entry"));
    assert_eq!(changed_cache.settings, changed_settings);
    assert_eq!(changed_cache.style, larger_style);
    assert!(larger_metrics.ascent_pt > 0.0);
    assert_eq!(changed_cache.metrics_count, 1);
    assert!(baseline_cache_contains(
        changed_cache,
        Some("ru"),
        Some("Cyrl")
    ));
    assert_eq!(changed_cache.font_generation, catalog.generation());

    let refreshed_generation = catalog
        .refresh()
        .unwrap_or_else(|error| panic!("font refresh failed: {error}"));
    assert!(!first.is_current_for(&session, refreshed_generation));
    let refreshed = service
        .layout_session(&session, &mut catalog, &style)
        .unwrap_or_else(|error| panic!("layout after refresh failed: {error}"));
    assert!(refreshed.is_current_for(&session, refreshed_generation));
    assert_eq!(
        service
            .baseline_anchor_cache
            .as_ref()
            .map(|cache| cache.font_generation),
        Some(refreshed_generation)
    );
}

#[test]
fn baseline_metrics_do_not_depend_on_the_order_of_same_script_fallback_characters() {
    let first_paragraphs = [paragraph("a"), paragraph("ɏ")];
    let second_paragraphs = [paragraph("ɏ"), paragraph("a")];
    let first_ids = [
        first_paragraphs[0].id.clone(),
        first_paragraphs[1].id.clone(),
    ];
    let second_ids = [
        second_paragraphs[0].id.clone(),
        second_paragraphs[1].id.clone(),
    ];
    let mut first_document = Document::blank();
    first_document.body = first_paragraphs.into_iter().map(Block::Paragraph).collect();
    let mut second_document = Document::blank();
    second_document.body = second_paragraphs
        .into_iter()
        .map(Block::Paragraph)
        .collect();
    let mut first_service = DocumentLayoutService::new();
    let mut second_service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let style = ShapingStyle::new(1200);
    let first = first_service
        .layout(&first_document, 1, &mut catalog, &style)
        .unwrap_or_else(|error| panic!("first ordering failed: {error}"));
    let second = second_service
        .layout(&second_document, 1, &mut catalog, &style)
        .unwrap_or_else(|error| panic!("reversed ordering failed: {error}"));

    for (first_index, second_index) in [(0, 1), (1, 0)] {
        let first_paragraph = first
            .paragraph_by_id(&first_ids[first_index])
            .unwrap_or_else(|| panic!("first paragraph is missing"));
        let second_paragraph = second
            .paragraph_by_id(&second_ids[second_index])
            .unwrap_or_else(|| panic!("reordered paragraph is missing"));
        assert_eq!(
            first_paragraph.layout.paragraph.lines[0]
                .metrics
                .baseline_pt,
            second_paragraph.layout.paragraph.lines[0]
                .metrics
                .baseline_pt,
            "baseline for the same character changed with paragraph order"
        );
    }
}

#[test]
fn baseline_cache_keeps_recent_metrics_when_variant_limit_is_reached() {
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let settings =
        ResolvedDocumentTextSettings::from_document_settings(&Document::blank().global_settings)
            .unwrap_or_else(|error| panic!("default text settings failed: {error}"));
    let style = ShapingStyle::new(1200);
    let metrics = service
        .resolve_baseline_anchor_metrics(&mut catalog, &settings, &style, None, None)
        .unwrap_or_else(|error| panic!("initial baseline failed: {error}"));
    let language = "".to_owned();
    let mut scripts = HashMap::new();
    let script_tags = [
        "Latn", "Cyrl", "Grek", "Hani", "Arab", "Hebr", "Deva", "Thai", "Armn", "Geor", "Ethi",
        "Beng", "Guru", "Gujr", "Orya", "Taml", "Telu", "Knda", "Mlym", "Sinh", "Laoo", "Tibt",
        "Mymr", "Khmr", "Hang", "Hira", "Kana", "Bopo", "Yiii", "Cher", "Cans", "Mong",
    ];
    assert_eq!(script_tags.len(), MAX_BASELINE_ANCHOR_VARIANTS);
    for (index, tag) in script_tags.into_iter().enumerate() {
        let script = script(tag);
        scripts.insert(
            Some((script, 'a')),
            super::CachedBaselineMetrics {
                metrics,
                last_used: index as u64 + 1,
            },
        );
    }
    scripts.insert(
        None,
        super::CachedBaselineMetrics {
            metrics,
            last_used: MAX_BASELINE_ANCHOR_VARIANTS as u64 + 1,
        },
    );
    let cache = service
        .baseline_anchor_cache
        .as_mut()
        .unwrap_or_else(|| panic!("successful layout must keep a baseline cache"));
    cache.metrics_by_language_script = HashMap::from([(language.clone(), scripts)]);
    cache.metrics_count = MAX_BASELINE_ANCHOR_VARIANTS + 1;
    cache.evict_least_recent();
    assert_eq!(cache.metrics_count, MAX_BASELINE_ANCHOR_VARIANTS);
    assert!(cache.metrics_by_language_script[&language].contains_key(&None));
    assert!(
        !cache.metrics_by_language_script[&language].contains_key(&Some((script("Latn"), 'a')))
    );
}

#[test]
fn baseline_anchor_uses_paragraph_language_then_document_language_and_stays_stable_on_soft_wraps() {
    let mut latin_then_cjk = paragraph("Latin first, then 中文");
    latin_then_cjk.inlines = vec![
        InlineNode::Run(Run {
            text: "Latin first".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
        InlineNode::SoftBreak(SoftBreak::default()),
        InlineNode::Run(Run {
            text: "中文 second line".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
    ];
    let mut chinese = paragraph("中文段落");
    chinese.language = Some("zh-Hans".to_owned());
    let mut document = Document::blank();
    document.global_settings.default_language = Some("en".to_owned());
    document.body = vec![
        Block::Paragraph(latin_then_cjk.clone()),
        Block::Paragraph(chinese.clone()),
    ];

    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let snapshot = service
        .layout(&document, 1, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("document layout failed: {error}"));
    let cache = service
        .baseline_anchor_cache
        .as_ref()
        .unwrap_or_else(|| panic!("paragraph layout must populate the baseline cache"));
    for expected in [(Some("en"), Some("Latn")), (Some("zh-Hans"), Some("Hani"))] {
        assert!(baseline_cache_contains(cache, expected.0, expected.1));
    }

    let wrapped = snapshot
        .paragraph_by_id(&latin_then_cjk.id)
        .unwrap_or_else(|| panic!("soft-wrapped paragraph is missing"));
    assert!(wrapped.layout.paragraph.lines.len() >= 2);
    let anchor_baseline = wrapped.layout.paragraph.lines[0].metrics.baseline_pt;
    assert!(
        wrapped
            .layout
            .paragraph
            .lines
            .iter()
            .all(|line| (line.metrics.baseline_pt - anchor_baseline).abs() < 0.01)
    );

    let no_language_sample = caret_layout::paragraph_first_strong_script_sample(&chinese)
        .unwrap_or_else(|| panic!("Chinese text must provide a strong script sample"));
    assert_eq!(no_language_sample, (script("Hani"), '中'));

    document.global_settings.default_language = None;
    let mut language_free_chinese = paragraph("中文段落");
    language_free_chinese.language = None;
    document.body = vec![Block::Paragraph(language_free_chinese)];
    let mut script_only_service = DocumentLayoutService::new();
    script_only_service
        .layout(&document, 2, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("script-only layout failed: {error}"));
    assert!(
        script_only_service
            .baseline_anchor_cache
            .as_ref()
            .is_some_and(|cache| baseline_cache_contains(cache, None, Some("Hani")))
    );
}

#[test]
fn page_hit_testing_rejects_nonfinite_and_outside_coordinates() {
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph("a"))];
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let snapshot = service
        .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("document layout failed: {error}"));
    let page = &snapshot.pages[0];
    for (x, y) in [
        (f64::NAN, 0.0),
        (0.0, f64::INFINITY),
        (-1.0, 0.0),
        (0.0, -1.0),
        (page.width_pt + 1.0, 0.0),
        (0.0, page.height_pt + 1.0),
    ] {
        assert!(snapshot.hit_test(0, x, y).is_none());
    }
    assert!(snapshot.hit_test(1, 0.0, 0.0).is_none());
    for (x, y, offset) in [
        (0.0, 0.0, 0),
        (0.0, page.height_pt, 0),
        (page.width_pt, 0.0, 1),
        (page.width_pt, page.height_pt, 1),
    ] {
        let hit = snapshot
            .hit_test(0, x, y)
            .unwrap_or_else(|| panic!("page edge should select nearest text"));
        assert_eq!(hit.position.offset, offset);
        assert_eq!(hit.caret.y0_pt, page.margin_top_pt);
    }
}

#[test]
fn reversed_cross_paragraph_selection_and_invalid_utf8_positions_are_handled() {
    let mut document = Document::blank();
    document.body = ["outside-before", "e\u{301}", "中", "outside-after"]
        .into_iter()
        .map(|text| Block::Paragraph(paragraph(text)))
        .collect();
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let snapshot = service
        .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("document layout failed: {error}"));
    let start = Pos {
        story: StoryId::Body,
        paragraph_id: snapshot.paragraphs[1].id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let end = Pos {
        paragraph_id: snapshot.paragraphs[2].id.clone(),
        offset: "中".len(),
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    let forward = snapshot
        .selection_geometry(&Selection {
            anchor: start.clone(),
            focus: end.clone(),
        })
        .unwrap_or_else(|| panic!("cross-paragraph selection rejected"));
    let reversed = snapshot
        .selection_geometry(&Selection {
            anchor: end.clone(),
            focus: start.clone(),
        })
        .unwrap_or_else(|| panic!("reversed selection rejected"));
    assert_eq!(forward, reversed);
    assert_eq!(forward.len(), 2);
    assert!(forward[0].y0_pt < forward[1].y0_pt);
    assert!(forward.iter().all(|rect| rect.x1_pt > rect.x0_pt));

    for invalid in [
        Pos {
            offset: 2,
            ..start.clone()
        },
        Pos {
            offset: 4,
            ..start.clone()
        },
        Pos {
            paragraph_id: NodeId::generate(),
            ..start.clone()
        },
        Pos {
            story: StoryId::EnvironmentTitle(NodeId::generate()),
            ..start.clone()
        },
    ] {
        assert!(
            snapshot
                .caret_geometry(&invalid, VisualAffinity::Downstream)
                .is_none()
        );
        assert!(
            snapshot
                .selection_geometry(&Selection {
                    anchor: invalid,
                    focus: end.clone(),
                })
                .is_none()
        );
    }
    assert!(
        snapshot
            .caret_geometry(
                &Pos {
                    offset: 1,
                    ..start.clone()
                },
                VisualAffinity::Downstream
            )
            .is_none()
    );
    assert_eq!(
        snapshot.selection_geometry(&Selection {
            anchor: start.clone(),
            focus: start
        }),
        Some(Vec::new())
    );
}

#[test]
fn soft_break_is_selected_logically_without_a_trailing_highlight_block() {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        unreachable!();
    };
    paragraph.inlines = vec![
        InlineNode::Run(Run {
            text: "你好".to_owned(),
            language: Some("zh".to_owned()),
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
        InlineNode::SoftBreak(SoftBreak::default()),
        InlineNode::Run(Run {
            text: "你好".to_owned(),
            language: Some("zh".to_owned()),
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
    ];
    let paragraph_id = paragraph.id.clone();
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let snapshot = service
        .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("document layout failed: {error}"));
    let start = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let text_end = Pos {
        offset: "你好".len(),
        affinity: Affinity::Upstream,
        ..start.clone()
    };
    let break_end = Pos {
        offset: "你好\n".len(),
        ..text_end.clone()
    };
    let text_rectangles = snapshot
        .selection_geometry(&Selection {
            anchor: start.clone(),
            focus: text_end,
        })
        .unwrap_or_else(|| panic!("text selection geometry failed"));
    let break_rectangles = snapshot
        .selection_geometry(&Selection {
            anchor: start,
            focus: break_end,
        })
        .unwrap_or_else(|| panic!("soft-break selection geometry failed"));
    assert_eq!(break_rectangles, text_rectangles);
}

#[test]
fn explicit_breaks_keep_empty_pages_without_inventing_text_hit_targets() {
    let first = paragraph("a");
    let last = paragraph("b");
    let first_id = first.id.clone();
    let last_id = last.id.clone();
    let page_break = || Block::ExplicitBreak(ExplicitBreak::default());
    let mut document = Document::blank();
    document.body = vec![
        page_break(),
        Block::Paragraph(first),
        page_break(),
        page_break(),
        Block::Paragraph(last),
        page_break(),
    ];
    let mut service = DocumentLayoutService::new();
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("font catalog failed: {error}"));
    let snapshot = service
        .layout(&document, 0, &mut catalog, &ShapingStyle::new(1200))
        .unwrap_or_else(|error| panic!("document layout failed: {error}"));
    assert_eq!(snapshot.pages.len(), 5);
    for page in [0, 2, 4] {
        assert!(snapshot.pages[page].layout.fragments.is_empty());
        assert!(snapshot.hit_test(page, 100.0, 100.0).is_none());
    }
    for (id, expected_page) in [(first_id, 1), (last_id, 3)] {
        let caret = snapshot
            .caret_geometry(
                &Pos {
                    story: StoryId::Body,
                    paragraph_id: id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                },
                VisualAffinity::Downstream,
            )
            .unwrap_or_else(|| panic!("text after page break has no caret"));
        assert_eq!(caret.page_index, expected_page);
        let hit = snapshot
            .hit_test(expected_page, caret.x_pt, caret.y0_pt)
            .unwrap_or_else(|| panic!("text page has no hit target"));
        assert_eq!(hit.position.paragraph_id, id);
        assert_eq!(hit.position.offset, 0);
    }
}
