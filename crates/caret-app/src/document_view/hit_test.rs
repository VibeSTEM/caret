//! 提供 caret 几何、字形栅格化和页面命中接口。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{DocumentViewRuntime, snapshot::caret_geometry_with_environment_exit};
use caret_engine::{PageCaretView, PageGlyphDrawCommand, Selection};

/// 页面绘制适配层传给 Qt 的共享 RGBA 图片资源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentGlyphImage {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub pixel_phase_applied: bool,
    pub pixels: Vec<u8>,
}

/// 稳定的有序页面绘制命令；字段仅包含 UI 绘制所需的几何和颜色。
#[derive(Debug, Clone, PartialEq)]
pub enum DocumentGlyphDrawCommand {
    Glyph {
        image_index: usize,
        x_pt: f64,
        baseline_y_pt: f64,
        pixel_phase: [f32; 2],
        line_clip_y_pt: Option<[f64; 2]>,
    },
    Rectangle {
        x_pt: f64,
        y_pt: f64,
        width_pt: f64,
        height_pt: f64,
        color: [u8; 4],
        line_clip_y_pt: Option<[f64; 2]>,
    },
    Line {
        x1_pt: f64,
        y1_pt: f64,
        x2_pt: f64,
        y2_pt: f64,
        thickness_pt: f64,
        color: [u8; 4],
        line_clip_y_pt: Option<[f64; 2]>,
    },
}

/// 单页已栅格化绘制帧，跨层不暴露 engine 或字体布局实现类型。
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentGlyphFrame {
    pub session_id: Option<String>,
    pub document_generation: u64,
    pub font_generation: u64,
    pub page_index: usize,
    pub images: Vec<DocumentGlyphImage>,
    pub commands: Vec<DocumentGlyphDrawCommand>,
}

impl DocumentViewRuntime {
    /// 返回当前 Rust 光标位置在最新布局中的页面几何。
    pub fn current_caret_json(&mut self) -> Result<String, String> {
        let display_session = self
            .composition_preview_session
            .as_ref()
            .unwrap_or(&self.session);
        let position = display_session.selection().focus.clone();
        let visual_affinity = display_session.selection_visual_affinity();
        let block_environment_exit = display_session.block_environment_exit();
        let snapshot = self.current_snapshot()?;
        let caret = snapshot
            .caret_geometry(&position, visual_affinity)
            .map(|caret| {
                PageCaretView::from(caret_geometry_with_environment_exit(
                    block_environment_exit,
                    snapshot,
                    caret,
                ))
            });
        serde_json::to_string(&caret).map_err(|error| format!("layout.caret_serialize: {error}"))
    }

    /// 栅格化指定页；调用方只请求当前可见页，并提供页面原点的设备像素坐标。
    pub fn rasterize_page_glyphs(
        &mut self,
        page_index: usize,
        px_per_pt: f64,
        page_origin_device_px: [f64; 2],
        default_foreground: [u8; 4],
    ) -> Result<DocumentGlyphFrame, String> {
        let page = self
            .current_snapshot()?
            .page_glyphs(page_index)
            .map_err(|error| error.to_string())?;
        let rasterized = self
            .glyph_rasterizer
            .rasterize(&page, px_per_pt, page_origin_device_px, default_foreground)
            .map_err(|error| error.to_string())?;
        let commands = rasterized
            .commands
            .into_iter()
            .map(|command| match command {
                PageGlyphDrawCommand::Glyph(placement) => DocumentGlyphDrawCommand::Glyph {
                    image_index: placement.image_index,
                    x_pt: placement.x_pt,
                    baseline_y_pt: placement.baseline_y_pt,
                    pixel_phase: placement.pixel_phase,
                    line_clip_y_pt: placement.line_clip_y_pt,
                },
                PageGlyphDrawCommand::Rectangle(rectangle) => DocumentGlyphDrawCommand::Rectangle {
                    x_pt: rectangle.x_pt,
                    y_pt: rectangle.y_pt,
                    width_pt: rectangle.width_pt,
                    height_pt: rectangle.height_pt,
                    color: rectangle.color.unwrap_or(default_foreground),
                    line_clip_y_pt: rectangle.line_clip_y_pt,
                },
                PageGlyphDrawCommand::Line(line) => DocumentGlyphDrawCommand::Line {
                    x1_pt: line.x1_pt,
                    y1_pt: line.y1_pt,
                    x2_pt: line.x2_pt,
                    y2_pt: line.y2_pt,
                    thickness_pt: line.thickness_pt,
                    color: line.color,
                    line_clip_y_pt: line.line_clip_y_pt,
                },
            })
            .collect();
        Ok(DocumentGlyphFrame {
            session_id: rasterized.session_id.map(|id| id.to_string()),
            document_generation: rasterized.document_generation,
            font_generation: rasterized.font_generation,
            page_index: rasterized.page_index,
            images: rasterized
                .images
                .into_iter()
                .map(|image| DocumentGlyphImage {
                    width: image.width,
                    height: image.height,
                    left: image.left,
                    top: image.top,
                    pixel_phase_applied: image.pixel_phase_applied,
                    pixels: image.pixels,
                })
                .collect(),
            commands,
        })
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
        let effect = self
            .session
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
        if effect.composition_changed {
            self.composition_id = None;
            self.composition_preview_session = None;
            self.composition_snapshot = None;
            self.composition_range = None;
        }
        let mut result = serde_json::to_value(result)
            .map_err(|error| format!("layout.place_caret_serialize: {error}"))?;
        if effect.composition_changed
            && let Some(object) = result.as_object_mut()
        {
            object.insert(
                "compositionCancelled".to_owned(),
                serde_json::Value::Bool(true),
            );
        }
        serde_json::to_string(&result)
            .map_err(|error| format!("layout.place_caret_serialize: {error}"))
    }
}
