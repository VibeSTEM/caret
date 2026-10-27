//! 将 Rust 内部布局结果投影为供 FFI 与界面消费的稳定只读数据。

use crate::{Affinity, DocumentLayoutSnapshot, PageCaretGeometry, PageHit, StoryId};
use caret_layout::VisualAffinity;
use serde::Serialize;

/// 页面、段落和修订代际组成的界面布局快照。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentLayoutView {
    pub session_id: Option<String>,
    pub document_generation: String,
    pub font_generation: String,
    pub margins_pt: PageMarginsView,
    pub pages: Vec<PageLayoutView>,
    pub paragraphs: Vec<ParagraphLayoutView>,
}

/// 页面边距，单位为 pt。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageMarginsView {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

/// 页面尺寸及其段落行片段。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageLayoutView {
    pub width_pt: f64,
    pub height_pt: f64,
    pub fragments: Vec<PageFragmentView>,
}

/// 页面引用的段落与半开行范围。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageFragmentView {
    pub paragraph_id: String,
    pub first_line: usize,
    pub line_end: usize,
    pub y_offset_pt: f64,
}

/// 段落原文及其 Parley/Caret 共用的行框数据。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParagraphLayoutView {
    pub paragraph_id: String,
    pub text: String,
    pub lines: Vec<LayoutLineView>,
}

/// 一行引用的 UTF-8 源范围及页面排版所需度量。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutLineView {
    pub source_start: usize,
    pub source_end: usize,
    pub consumed_start: usize,
    pub consumed_end: usize,
    pub top_pt: f64,
    pub baseline_pt: f64,
    pub height_pt: f64,
    pub advance_pt: f64,
    pub overflows_width: bool,
}

/// 与具体屏幕坐标分离的稳定 UTF-8 文档位置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPositionView {
    pub story: StoryView,
    pub paragraph_id: String,
    pub offset_utf8: String,
    pub affinity: String,
}

/// 正文或环境标题所在的文档故事。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StoryView {
    Body,
    EnvironmentTitle {
        #[serde(rename = "environmentId")]
        environment_id: String,
    },
}

/// 文字命中位置，同时保留逻辑亲和性、视觉亲和性和输入代际。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageHitView {
    pub session_id: Option<String>,
    pub document_generation: String,
    pub position: DocumentPositionView,
    pub visual_affinity: String,
    pub caret: PageCaretView,
}

/// 页面局部坐标中的插入点；宽度由界面主题决定。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageCaretView {
    pub page_index: usize,
    pub x_pt: f64,
    pub y0_pt: f64,
    pub y1_pt: f64,
}

impl From<&DocumentLayoutSnapshot> for DocumentLayoutView {
    fn from(snapshot: &DocumentLayoutSnapshot) -> Self {
        let first_page = snapshot.pages.first();
        let margins_pt = first_page.map_or(
            PageMarginsView {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            },
            |page| PageMarginsView {
                top: page.margin_top_pt,
                right: page.margin_right_pt,
                bottom: page.margin_bottom_pt,
                left: page.margin_left_pt,
            },
        );
        let pages = snapshot
            .pages
            .iter()
            .map(|page| PageLayoutView {
                width_pt: page.width_pt,
                height_pt: page.height_pt,
                fragments: page
                    .layout
                    .fragments
                    .iter()
                    .map(|fragment| PageFragmentView {
                        paragraph_id: fragment.paragraph_id.to_string(),
                        first_line: fragment.line_range.start,
                        line_end: fragment.line_range.end,
                        y_offset_pt: f64::from(fragment.y_offset_pt),
                    })
                    .collect(),
            })
            .collect();
        let paragraphs = snapshot
            .paragraphs
            .iter()
            .map(|paragraph| ParagraphLayoutView {
                paragraph_id: paragraph.id.to_string(),
                text: paragraph.text.clone(),
                lines: paragraph
                    .layout
                    .paragraph
                    .lines
                    .iter()
                    .zip(&paragraph.layout.line_box_tops_pt)
                    .map(|(line, &top_pt)| LayoutLineView {
                        source_start: line.source_bytes.start,
                        source_end: line.source_bytes.end,
                        consumed_start: line.consumed_bytes.start,
                        consumed_end: line.consumed_bytes.end,
                        top_pt: f64::from(top_pt),
                        baseline_pt: f64::from(top_pt + line.metrics.baseline_pt),
                        height_pt: f64::from(line.metrics.height_pt),
                        advance_pt: f64::from(line.advance_pt),
                        overflows_width: line.overflows_width,
                    })
                    .collect(),
            })
            .collect();

        Self {
            session_id: snapshot.session_id.as_ref().map(ToString::to_string),
            document_generation: snapshot.document_generation.to_string(),
            font_generation: snapshot.font_generation.to_string(),
            margins_pt,
            pages,
            paragraphs,
        }
    }
}

impl DocumentLayoutSnapshot {
    /// 将页面点坐标命中结果投影为可通过 FFI 传输的稳定文档位置。
    pub fn hit_test_view(&self, page_index: usize, x_pt: f64, y_pt: f64) -> Option<PageHitView> {
        let hit = self.hit_test(page_index, x_pt, y_pt)?;
        Some(self.view_for_hit(&hit))
    }

    /// 将已有的 Rust 页面命中转换为界面 DTO。
    pub fn view_for_hit(&self, hit: &PageHit) -> PageHitView {
        PageHitView {
            session_id: self.session_id.as_ref().map(ToString::to_string),
            document_generation: self.document_generation.to_string(),
            position: DocumentPositionView::from(hit),
            visual_affinity: visual_affinity_name(hit.visual_affinity).to_owned(),
            caret: PageCaretView::from(hit.caret),
        }
    }
}

impl From<PageCaretGeometry> for PageCaretView {
    fn from(caret: PageCaretGeometry) -> Self {
        Self {
            page_index: caret.page_index,
            x_pt: caret.x_pt,
            y0_pt: caret.y0_pt,
            y1_pt: caret.y1_pt,
        }
    }
}

impl From<&PageHit> for DocumentPositionView {
    fn from(hit: &PageHit) -> Self {
        let story = match &hit.position.story {
            StoryId::Body => StoryView::Body,
            StoryId::EnvironmentTitle(environment_id) => StoryView::EnvironmentTitle {
                environment_id: environment_id.to_string(),
            },
        };
        Self {
            story,
            paragraph_id: hit.position.paragraph_id.to_string(),
            offset_utf8: hit.position.offset.to_string(),
            affinity: match hit.position.affinity {
                Affinity::Upstream => "upstream",
                Affinity::Downstream => "downstream",
            }
            .to_owned(),
        }
    }
}

fn visual_affinity_name(affinity: VisualAffinity) -> &'static str {
    match affinity {
        VisualAffinity::Upstream => "upstream",
        VisualAffinity::Downstream => "downstream",
    }
}

#[cfg(test)]
mod tests {
    use caret_doc::{Document, NodeId};

    use super::*;
    use crate::{Command, DocumentLayoutRuntime, PageCaretGeometry, Pos, Session};

    #[test]
    fn layout_projection_keeps_utf8_ranges_and_large_generation_ids_in_json()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut session = Session::new(Document::blank())?;
        for command in [
            Command::InsertText("é".to_owned()),
            Command::SplitParagraph,
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
}
