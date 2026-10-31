//! 验证布局视图投影、命中映射和分页增量。

use caret_doc::{Document, NodeId};

use super::*;
use crate::{Command, DocumentLayoutRuntime, PageCaretGeometry, Pos, Session};

#[test]
fn layout_projection_keeps_utf8_ranges_and_large_generation_ids_in_json()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::new(Document::blank())?;
    for command in [
        Command::InsertText("é".to_owned()),
        Command::InsertParagraphBreak,
        Command::InsertText("中".to_owned()),
    ] {
        session.run(command)?;
    }
    let mut runtime = DocumentLayoutRuntime::new_system()?;
    let mut snapshot = runtime.layout_session(&session)?;
    snapshot.document_generation = u64::MAX;
    let view = DocumentLayoutView::from(&snapshot);
    let json = serde_json::to_value(&view)?;

    assert_eq!(json["documentGeneration"], u64::MAX.to_string());
    assert_eq!(json["sessionId"], session.session_id().to_string());
    assert_eq!(json["paragraphs"][0]["text"], "é");
    assert_eq!(json["paragraphs"][1]["text"], "中");
    assert_eq!(json["paragraphs"][0]["lines"][0]["sourceEnd"], 2);
    assert_eq!(json["paragraphs"][0]["lines"][0]["consumedEnd"], 2);
    assert_eq!(json["paragraphs"][1]["lines"][0]["sourceStart"], 0);
    assert_eq!(json["paragraphs"][1]["lines"][0]["sourceEnd"], 3);
    assert_eq!(json["pages"][0]["fragments"][0]["firstLine"], 0);
    assert_eq!(json["pages"][0]["fragments"][0]["lineEnd"], 1);
    assert_eq!(
        json["pages"][0]["fragments"][1]["paragraphId"],
        session.selection().focus.paragraph_id.to_string()
    );
    assert!(view.margins_pt.left > 0.0);
    assert!(view.pages[0].fragments[1].y_offset_pt > view.pages[0].fragments[0].y_offset_pt);

    let page = &snapshot.pages[0];
    let hit = snapshot
        .hit_test(0, page.width_pt / 2.0, page.height_pt / 2.0)
        .ok_or("page hit should resolve")?;
    let hit_view = snapshot
        .hit_test_view(0, page.width_pt / 2.0, page.height_pt / 2.0)
        .ok_or("page hit view should resolve")?;
    assert_eq!(hit_view, snapshot.view_for_hit(&hit));
    assert_eq!(hit_view.document_generation, u64::MAX.to_string());
    assert_eq!(hit_view.session_id.as_deref(), view.session_id.as_deref());
    assert!(
        snapshot
            .hit_test_view(snapshot.pages.len(), 0.0, 0.0)
            .is_none()
    );
    Ok(())
}

#[test]
fn paragraph_delta_starts_at_the_first_page_changed_by_reflow() {
    let target = NodeId::generate();
    let predecessor = NodeId::generate();
    let later = NodeId::generate();
    let previous_pages = vec![
        page_with_paragraphs(std::slice::from_ref(&target)),
        page_with_paragraphs(std::slice::from_ref(&later)),
    ];
    let current_pages = vec![
        page_with_paragraphs(std::slice::from_ref(&predecessor)),
        page_with_paragraphs(std::slice::from_ref(&target)),
    ];
    assert_eq!(
        page_delta_start_index(&previous_pages, &current_pages, 1),
        0,
        "前驱段落因孤行规则移页时，增量必须从旧页开始"
    );
}

#[test]
fn paragraph_delta_reuses_unchanged_pages_before_the_edited_paragraph() {
    let first = NodeId::generate();
    let target = NodeId::generate();
    let pages = vec![
        page_with_paragraphs(std::slice::from_ref(&first)),
        page_with_paragraphs(std::slice::from_ref(&target)),
    ];
    assert_eq!(page_delta_start_index(&pages, &pages, 1), 1);
}

fn page_with_paragraphs(paragraph_ids: &[NodeId]) -> PageLayoutSnapshot {
    PageLayoutSnapshot {
        width_pt: 600.0,
        height_pt: 800.0,
        margin_left_pt: 50.0,
        margin_top_pt: 50.0,
        margin_right_pt: 50.0,
        margin_bottom_pt: 50.0,
        layout: caret_layout::PageLayout {
            fragments: paragraph_ids
                .iter()
                .map(|paragraph_id| caret_layout::ParagraphFragment {
                    paragraph_id: paragraph_id.clone(),
                    line_range: 0..1,
                    y_offset_pt: 0.0,
                })
                .collect(),
            blocks: Vec::new(),
            used_height_pt: 12.0,
        },
    }
}

#[test]
fn hit_projection_keeps_environment_story_and_logical_visual_sides_independent()
-> Result<(), Box<dyn std::error::Error>> {
    let environment_id = NodeId::generate();
    let hit = PageHit {
        position: Pos {
            story: StoryId::EnvironmentTitle(environment_id.clone()),
            paragraph_id: NodeId::generate(),
            offset: usize::MAX,
            affinity: Affinity::Upstream,
        },
        visual_affinity: VisualAffinity::Downstream,
        caret: PageCaretGeometry {
            page_index: 3,
            x_pt: 72.5,
            y0_pt: 144.25,
            y1_pt: 156.25,
        },
    };
    let position = DocumentPositionView::from(&hit);
    let json = serde_json::to_value(&position)?;
    assert_eq!(json["story"]["kind"], "environmentTitle");
    assert_eq!(json["story"]["environmentId"], environment_id.to_string());
    assert_eq!(json["offsetUtf8"], usize::MAX.to_string());
    assert_eq!(json["affinity"], "upstream");
    assert_eq!(visual_affinity_name(hit.visual_affinity), "downstream");
    let caret = PageCaretView::from(hit.caret);
    assert_eq!(caret.page_index, 3);
    assert_eq!(caret.x_pt, 72.5);
    assert_eq!(caret.y1_pt - caret.y0_pt, 12.0);
    Ok(())
}
