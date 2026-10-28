//! 将 Rust 内部布局结果投影为供 FFI 与界面消费的稳定只读数据。

use crate::{
    Affinity, DocumentLayoutSnapshot, PageCaretGeometry, PageHit, PageLayoutSnapshot, StoryId,
};
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

/// 单段编辑使用的增量投影；分页结果完整，段落内容只携带发生变化的段。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentLayoutDeltaView {
    pub partial: bool,
    pub session_id: Option<String>,
    pub base_document_generation: String,
    pub document_generation: String,
    pub font_generation: String,
    pub margins_pt: PageMarginsView,
    pub page_start_index: usize,
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
        Self {
            session_id: snapshot.session_id.as_ref().map(ToString::to_string),
            document_generation: snapshot.document_generation.to_string(),
            font_generation: snapshot.font_generation.to_string(),
            margins_pt: margins_view(snapshot),
            pages: page_views(snapshot, 0),
            paragraphs: snapshot.paragraphs.iter().map(paragraph_view).collect(),
        }
    }
}

impl DocumentLayoutDeltaView {
    /// 为一个已重排段落生成增量视图，并仅投影首个受影响页及其后的分页。
    pub fn for_paragraph(
        snapshot: &DocumentLayoutSnapshot,
        paragraph_id: &caret_doc::NodeId,
        page_start_index: usize,
        base_document_generation: u64,
    ) -> Option<Self> {
        let paragraph = snapshot.paragraph_by_id(paragraph_id)?;
        if page_start_index >= snapshot.pages.len() {
            return None;
        }
        Some(Self {
            partial: true,
            session_id: snapshot.session_id.as_ref().map(ToString::to_string),
            base_document_generation: base_document_generation.to_string(),
            document_generation: snapshot.document_generation.to_string(),
            font_generation: snapshot.font_generation.to_string(),
            margins_pt: margins_view(snapshot),
            page_start_index,
            pages: page_views(snapshot, page_start_index),
            paragraphs: vec![paragraph_view(paragraph)],
        })
    }

    /// 找出段落重排后需要替换的最早分页；保留更早且未变化的页面。
    pub fn first_affected_page_index(
        previous: &DocumentLayoutSnapshot,
        current: &DocumentLayoutSnapshot,
        paragraph_id: &caret_doc::NodeId,
    ) -> Option<usize> {
        let paragraph_page = current.pages.iter().position(|page| {
            page.layout
                .fragments
                .iter()
                .any(|fragment| &fragment.paragraph_id == paragraph_id)
        })?;
        Some(page_delta_start_index(
            &previous.pages,
            &current.pages,
            paragraph_page,
        ))
    }
}

fn page_delta_start_index(
    previous_pages: &[PageLayoutSnapshot],
    current_pages: &[PageLayoutSnapshot],
    paragraph_page: usize,
) -> usize {
    let shared_page_count = previous_pages.len().min(current_pages.len());
    let first_page_difference = previous_pages
        .iter()
        .zip(current_pages)
        .position(|(old_page, new_page)| old_page != new_page)
        .or_else(|| (previous_pages.len() != current_pages.len()).then_some(shared_page_count));
    first_page_difference.map_or(paragraph_page, |index| index.min(paragraph_page))
}

fn margins_view(snapshot: &DocumentLayoutSnapshot) -> PageMarginsView {
    let first_page = snapshot.pages.first();
    first_page.map_or(
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
    )
}

fn page_views(snapshot: &DocumentLayoutSnapshot, page_start_index: usize) -> Vec<PageLayoutView> {
    snapshot
        .pages
        .iter()
        .skip(page_start_index)
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
        .collect()
}

fn paragraph_view(paragraph: &crate::LaidOutParagraph) -> ParagraphLayoutView {
    ParagraphLayoutView {
        paragraph_id: paragraph.id.to_string(),
        text: paragraph.text.to_string(),
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
mod tests;
