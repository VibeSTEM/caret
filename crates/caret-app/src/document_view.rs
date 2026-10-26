//! 为界面提供由 Rust 文档会话生成的只读布局快照。

use caret_doc::Document;
use caret_engine::{
    Command, DocumentLayoutRuntime, DocumentLayoutSnapshot, PageCaretView, PageGlyphRasterizer,
    RasterizedPageGlyphs, Selection, Session,
};
use std::convert::TryFrom;

/// 当前文档会话与可复用的字体/布局运行时。
pub struct DocumentViewRuntime {
    session: Session,
    layout: DocumentLayoutRuntime,
    glyph_rasterizer: PageGlyphRasterizer,
    last_snapshot: Option<DocumentLayoutSnapshot>,
}

impl DocumentViewRuntime {
    /// 创建空白文档会话并初始化一次系统字体目录。
    pub fn new_blank() -> Result<Self, String> {
        Self::new(Document::blank())
    }

    fn new(document: Document) -> Result<Self, String> {
        let session = Session::new(document).map_err(|error| error.to_string())?;
        let layout = DocumentLayoutRuntime::new_system().map_err(|error| error.to_string())?;
        Ok(Self {
            session,
            layout,
            glyph_rasterizer: PageGlyphRasterizer::default(),
            last_snapshot: None,
        })
    }

    /// 按当前会话和字体代际生成界面快照 JSON。
    pub fn layout_snapshot_json(&mut self) -> Result<String, String> {
        let snapshot = self.current_snapshot()?;
        serde_json::to_string(&caret_engine::DocumentLayoutView::from(snapshot))
            .map_err(|error| format!("layout.serialize: {error}"))
    }

    /// 将 Qt 直接输入的文本交给 Rust Session，再返回新布局快照。
    pub fn insert_text_json(&mut self, text: String) -> Result<String, String> {
        self.session
            .run(Command::InsertText(text))
            .map_err(|error| format!("text.insert: {error}"))?;
        self.layout_snapshot_json()
    }

    /// 返回当前 Rust 光标位置在最新布局中的页面几何。
    pub fn current_caret_json(&mut self) -> Result<String, String> {
        let position = self.session.selection().focus.clone();
        let visual_affinity = self.session.selection_visual_affinity();
        let caret = self
            .current_snapshot()?
            .caret_geometry(&position, visual_affinity)
            .map(PageCaretView::from);
        serde_json::to_string(&caret).map_err(|error| format!("layout.caret_serialize: {error}"))
    }

    /// 栅格化指定页；调用方只请求当前可见页，并提供页面原点的设备像素坐标。
    pub fn rasterize_page_glyphs(
        &mut self,
        page_index: usize,
        px_per_pt: f64,
        page_origin_device_px: [f64; 2],
        default_foreground: [u8; 4],
    ) -> Result<RasterizedPageGlyphs, String> {
        let page = self
            .current_snapshot()?
            .page_glyphs(page_index)
            .map_err(|error| error.to_string())?;
        self.glyph_rasterizer
            .rasterize(&page, px_per_pt, page_origin_device_px, default_foreground)
            .map_err(|error| error.to_string())
    }

    /// 在当前已发布或惰性重建的快照中命中，并返回稳定的 Rust 文档位置。
    pub fn hit_test_json(
        &mut self,
        page_index: i32,
        x_pt: f64,
        y_pt: f64,
    ) -> Result<String, String> {
        let hit = match usize::try_from(page_index) {
            Ok(page_index) => self
                .current_snapshot()?
                .hit_test_view(page_index, x_pt, y_pt),
            Err(_) => None,
        };
        serde_json::to_string(&hit).map_err(|error| format!("layout.hit_test_serialize: {error}"))
    }

    /// 在 Rust 会话中提交页面命中的插入点，并返回提交位置与输入代际。
    pub fn place_caret_at_page_json(
        &mut self,
        page_index: i32,
        x_pt: f64,
        y_pt: f64,
    ) -> Result<String, String> {
        let page_index = match usize::try_from(page_index) {
            Ok(page_index) => page_index,
            Err(_) => return Ok("null".to_owned()),
        };
        let (hit, result) = {
            let snapshot = self.current_snapshot()?;
            let Some(hit) = snapshot.hit_test(page_index, x_pt, y_pt) else {
                return Ok("null".to_owned());
            };
            (hit.clone(), snapshot.view_for_hit(&hit))
        };
        let position = hit.position;
        self.session
            .set_selection_with_visual_affinity(
                Some(Selection {
                    anchor: position.clone(),
                    focus: position,
                }),
                hit.visual_affinity,
            )
            .map_err(|error| format!("selection.set: {error}"))?;
        serde_json::to_string(&result)
            .map_err(|error| format!("layout.place_caret_serialize: {error}"))
    }

    fn current_snapshot(&mut self) -> Result<&DocumentLayoutSnapshot, String> {
        let font_generation = self.layout.font_generation();
        let current = self
            .last_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.is_current_for(&self.session, font_generation));
        if !current {
            self.last_snapshot = Some(
                self.layout
                    .layout_session(&self.session)
                    .map_err(|error| error.to_string())?,
            );
        }
        self.last_snapshot
            .as_ref()
            .ok_or_else(|| "layout.snapshot_unavailable".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::DocumentViewRuntime;
    use caret_doc::{Block, Document, InlineNode, Run};
    use serde_json::Value;

    #[test]
    fn blank_document_snapshot_contains_page_and_paragraph_layout() {
        let mut runtime = DocumentViewRuntime::new_blank()
            .unwrap_or_else(|error| panic!("document view runtime failed: {error}"));
        let json = runtime
            .layout_snapshot_json()
            .unwrap_or_else(|error| panic!("document layout snapshot failed: {error}"));
        let snapshot: Value = serde_json::from_str(&json)
            .unwrap_or_else(|error| panic!("layout snapshot JSON failed: {error}"));

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

        let hit_json = runtime
            .hit_test_json(0, 73.0, 73.0)
            .unwrap_or_else(|error| panic!("document hit test failed: {error}"));
        let hit: Value = serde_json::from_str(&hit_json)
            .unwrap_or_else(|error| panic!("document hit JSON failed: {error}"));
        assert_eq!(hit["position"]["offsetUtf8"], "0");
        assert_eq!(hit["position"]["affinity"], "downstream");
        assert_eq!(hit["visualAffinity"], "downstream");
        assert_eq!(hit["caret"]["pageIndex"], 0);
        assert!(hit["caret"]["xPt"].as_f64().is_some());
        assert!(hit["caret"]["y0Pt"].as_f64().is_some());
        assert!(hit["caret"]["y1Pt"].as_f64().is_some());
        assert_eq!(
            runtime
                .hit_test_json(-1, 73.0, 73.0)
                .unwrap_or_else(|error| panic!("invalid page hit test failed: {error}")),
            "null"
        );
    }

    #[test]
    fn keyboard_text_insertion_updates_rust_document_and_caret_geometry() {
        let mut runtime = DocumentViewRuntime::new_blank()
            .unwrap_or_else(|error| panic!("document view runtime failed: {error}"));

        let json = runtime
            .insert_text_json("Aλ".to_owned())
            .unwrap_or_else(|error| panic!("keyboard text insertion failed: {error}"));
        let snapshot: Value = serde_json::from_str(&json)
            .unwrap_or_else(|error| panic!("layout snapshot JSON failed: {error}"));
        assert_eq!(snapshot["documentGeneration"], "1");
        assert_eq!(snapshot["paragraphs"][0]["text"], "Aλ");
        assert_eq!(runtime.session.selection().focus.offset, "Aλ".len());

        let caret_json = runtime
            .current_caret_json()
            .unwrap_or_else(|error| panic!("caret geometry failed: {error}"));
        let caret: Value = serde_json::from_str(&caret_json)
            .unwrap_or_else(|error| panic!("caret JSON failed: {error}"));
        assert_eq!(caret["pageIndex"], 0);
        assert!(caret["xPt"].as_f64().is_some());
        assert!(caret["y1Pt"].as_f64().is_some());
    }

    #[test]
    fn visible_page_rasterization_returns_binary_glyph_images() {
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
        let mut runtime = DocumentViewRuntime::new(document)
            .unwrap_or_else(|error| panic!("document view runtime failed: {error}"));

        let page = runtime
            .rasterize_page_glyphs(0, 1.0, [0.25, 0.75], [32, 32, 32, 255])
            .unwrap_or_else(|error| panic!("page rasterization failed: {error}"));
        assert_eq!(page.page_index, 0);
        assert_eq!(page.document_generation, 0);
        assert_eq!(page.font_generation, 1);
        assert_eq!(page.images.len(), 1);
        assert_eq!(page.placements.len(), 1);
        assert_eq!(page.placements[0].image_index, 0);
        assert!(page.images[0].pixels.len() > 4);
        assert!(page.images[0].pixels.as_chunks::<4>().1.is_empty());
    }

    #[test]
    fn page_click_updates_the_rust_session_selection() {
        let mut document = Document::blank();
        if let Some(Block::Paragraph(paragraph)) = document.body.first_mut() {
            paragraph.inlines.push(InlineNode::Run(Run {
                text: "a".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }));
        } else {
            panic!("blank document should start with a paragraph");
        }
        let mut runtime = DocumentViewRuntime::new(document)
            .unwrap_or_else(|error| panic!("document view runtime failed: {error}"));

        let result = runtime
            .place_caret_at_page_json(0, 550.0, 73.0)
            .unwrap_or_else(|error| panic!("page caret placement failed: {error}"));
        let hit: Value = serde_json::from_str(&result)
            .unwrap_or_else(|error| panic!("page caret JSON failed: {error}"));
        assert_eq!(hit["position"]["offsetUtf8"], "1");
        assert_eq!(runtime.session.selection().anchor.offset, 1);
        assert_eq!(runtime.session.selection().focus.offset, 1);
        assert_eq!(
            runtime.session.selection_visual_affinity(),
            caret_engine::VisualAffinity::Upstream
        );
        assert_eq!(runtime.session.document_generation(), 0);

        assert_eq!(
            runtime
                .place_caret_at_page_json(-1, 550.0, 73.0)
                .unwrap_or_else(|error| panic!("page miss placement failed: {error}")),
            "null"
        );
        assert_eq!(runtime.session.selection().focus.offset, 1);
    }
}
