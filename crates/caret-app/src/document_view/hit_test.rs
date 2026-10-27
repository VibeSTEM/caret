//! 提供 caret 几何、字形栅格化和页面命中接口。
use super::DocumentViewRuntime;
use caret_engine::{PageCaretView, RasterizedPageGlyphs, Selection};

impl DocumentViewRuntime {
    /// 返回当前 Rust 光标位置在最新布局中的页面几何。
    pub fn current_caret_json(&mut self) -> Result<String, String> {
        let display_session = self
            .composition_preview_session
            .as_ref()
            .unwrap_or(&self.session);
        let position = display_session.selection().focus.clone();
        let visual_affinity = display_session.selection_visual_affinity();
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
        self.anchor_visual_affinity = hit.visual_affinity;
        self.preferred_visual_x_pt = None;
        serde_json::to_string(&result)
            .map_err(|error| format!("layout.place_caret_serialize: {error}"))
    }
}
