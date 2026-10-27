//! 文档运行时句柄、布局命令转发与 glyph 二进制快照转换。

use crate::generated::bridge;
use caret_app::document_view::DocumentViewRuntime;

/// 持有文档会话、字体目录与布局缓存的 Rust 应用运行时。
pub struct DocumentRuntime {
    runtime: Result<DocumentViewRuntime, String>,
}

/// 创建空白文档运行时；初始化错误由首次快照请求返回。
pub(crate) fn new_blank_document_runtime() -> Box<DocumentRuntime> {
    Box::new(DocumentRuntime {
        runtime: DocumentViewRuntime::new_blank(),
    })
}

/// 返回页面/段落布局 JSON；错误以 `error:` 开头，调用方不可当作快照消费。
pub(crate) fn layout_snapshot_json(runtime: &mut DocumentRuntime) -> String {
    json_response(runtime, DocumentViewRuntime::layout_snapshot_json)
}

/// 返回不包含全文布局的 caret、选区与 IME 交互状态。
pub(crate) fn interaction_state_json(runtime: &mut DocumentRuntime) -> String {
    json_response(runtime, DocumentViewRuntime::interaction_state_json)
}

/// 将页面内命中委托给与快照相同的 Rust 布局结果。
pub(crate) fn document_hit_test_json(
    runtime: &mut DocumentRuntime,
    page_index: i32,
    x_pt: f64,
    y_pt: f64,
) -> String {
    json_response(runtime, |runtime| {
        runtime.hit_test_json(page_index, x_pt, y_pt)
    })
}

/// 在同一布局命中结果上更新会话选区，不跨越 FFI 暴露文档节点。
pub(crate) fn place_caret_at_page_json(
    runtime: &mut DocumentRuntime,
    page_index: i32,
    x_pt: f64,
    y_pt: f64,
) -> String {
    json_response(runtime, |runtime| {
        runtime.place_caret_at_page_json(page_index, x_pt, y_pt)
    })
}

pub(crate) fn insert_text_json(runtime: &mut DocumentRuntime, text: String) -> String {
    json_response(runtime, |runtime| runtime.insert_text_json(text))
}

pub(crate) fn paste_text_json(runtime: &mut DocumentRuntime, text: String) -> String {
    json_response(runtime, |runtime| runtime.paste_text_json(text))
}

pub(crate) fn split_paragraph_json(runtime: &mut DocumentRuntime) -> String {
    json_response(runtime, DocumentViewRuntime::split_paragraph_json)
}

pub(crate) fn delete_backward_json(runtime: &mut DocumentRuntime) -> String {
    json_response(runtime, DocumentViewRuntime::delete_backward_json)
}

pub(crate) fn insert_soft_break_json(runtime: &mut DocumentRuntime) -> String {
    json_response(runtime, DocumentViewRuntime::insert_soft_break_json)
}

pub(crate) fn input_method_event_json(
    runtime: &mut DocumentRuntime,
    preedit: String,
    commit: String,
    replacement_start: i32,
    replacement_length: i32,
    cursor_position_utf16: usize,
) -> String {
    json_response(runtime, |runtime| {
        runtime.input_method_event_json(
            preedit,
            commit,
            replacement_start,
            replacement_length,
            cursor_position_utf16,
        )
    })
}

pub(crate) fn drag_hit_map_json(runtime: &mut DocumentRuntime, page_index: i32) -> String {
    json_response(runtime, |runtime| runtime.drag_hit_map_json(page_index))
}

pub(crate) fn set_selection_json(runtime: &mut DocumentRuntime, selection: String) -> String {
    json_response(runtime, |runtime| runtime.set_selection_json(&selection))
}

pub(crate) fn clear_selection_json(runtime: &mut DocumentRuntime) -> String {
    json_response(runtime, DocumentViewRuntime::clear_selection_json)
}

pub(crate) fn selected_text_json(runtime: &mut DocumentRuntime) -> String {
    json_response(runtime, DocumentViewRuntime::selected_text_json)
}

pub(crate) fn move_visual_caret_json(
    runtime: &mut DocumentRuntime,
    forward: bool,
    extend: bool,
) -> String {
    json_response(runtime, |runtime| {
        runtime.move_visual_caret_json(forward, extend)
    })
}

pub(crate) fn move_vertical_caret_json(
    runtime: &mut DocumentRuntime,
    line_delta: i32,
    extend: bool,
) -> String {
    json_response(runtime, |runtime| {
        runtime.move_vertical_caret_json(line_delta as isize, extend)
    })
}

pub(crate) fn move_to_visual_line_edge_json(
    runtime: &mut DocumentRuntime,
    end: bool,
    extend: bool,
) -> String {
    json_response(runtime, |runtime| {
        runtime.move_to_visual_line_edge_json(end, extend)
    })
}

pub(crate) fn current_caret_json(runtime: &mut DocumentRuntime) -> String {
    json_response(runtime, DocumentViewRuntime::current_caret_json)
}

// 初始化错误与命令错误在同一处转换，避免新增命令各自拼接边界诊断。
fn json_response(
    runtime: &mut DocumentRuntime,
    operation: impl FnOnce(&mut DocumentViewRuntime) -> Result<String, String>,
) -> String {
    let result = match &mut runtime.runtime {
        Ok(runtime) => operation(runtime),
        Err(error) => Err(error.clone()),
    };
    result.unwrap_or_else(|error| format!("error:{error}"))
}

/// 将 Rust 页面布局和 glyph 栅格结果转换为紧凑 CXX 二进制 DTO。
pub(crate) fn rasterize_page_glyphs(
    runtime: &mut DocumentRuntime,
    request: &bridge::GlyphRasterRequest,
) -> bridge::PageGlyphFrame {
    let page_index = request.page_index;
    let runtime = match &mut runtime.runtime {
        Ok(runtime) => runtime,
        Err(error) => return empty_glyph_frame(page_index, error.clone()),
    };
    let rasterized = match runtime.rasterize_page_glyphs(
        usize::try_from(page_index).unwrap_or(usize::MAX),
        request.px_per_pt,
        [
            request.page_origin_device_x_px,
            request.page_origin_device_y_px,
        ],
        [
            request.foreground_red,
            request.foreground_green,
            request.foreground_blue,
            request.foreground_alpha,
        ],
    ) {
        Ok(result) => result,
        Err(error) => return empty_glyph_frame(page_index, error),
    };
    let mut images = Vec::with_capacity(rasterized.images.len());
    for image in rasterized.images {
        images.push(bridge::GlyphImage {
            width: image.width,
            height: image.height,
            left: image.left,
            top: image.top,
            pixel_phase_applied: image.pixel_phase_applied,
            pixels: image.pixels,
        });
    }
    let mut placements = Vec::with_capacity(rasterized.placements.len());
    for placement in rasterized.placements {
        let Ok(image_index) = u32::try_from(placement.image_index) else {
            return empty_glyph_frame(page_index, "layout.glyphs.too_many_images".to_owned());
        };
        let (clip_enabled, clip_y0_pt, clip_y1_pt) = placement
            .line_clip_y_pt
            .map(|clip| (true, clip[0], clip[1]))
            .unwrap_or((false, 0.0, 0.0));
        placements.push(bridge::GlyphPlacement {
            image_index,
            x_pt: placement.x_pt,
            baseline_y_pt: placement.baseline_y_pt,
            pixel_phase_x: placement.pixel_phase[0],
            pixel_phase_y: placement.pixel_phase[1],
            clip_enabled,
            clip_y0_pt,
            clip_y1_pt,
        });
    }
    bridge::PageGlyphFrame {
        document_generation: rasterized.document_generation,
        font_generation: rasterized.font_generation,
        page_index,
        error: String::new(),
        images,
        placements,
    }
}

fn empty_glyph_frame(page_index: u32, error: String) -> bridge::PageGlyphFrame {
    bridge::PageGlyphFrame {
        document_generation: 0,
        font_generation: 0,
        page_index,
        error,
        images: Vec::new(),
        placements: Vec::new(),
    }
}
