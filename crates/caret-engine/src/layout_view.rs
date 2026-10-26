//! 将 Rust 内部布局结果投影为供 FFI 与界面消费的稳定只读数据。

use crate::DocumentLayoutSnapshot;
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
