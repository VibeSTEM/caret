//! 文档运行时句柄、布局命令转发与 glyph 二进制快照转换。

use crate::generated::bridge;
use caret_app::document_session::{DocumentSaveSnapshot, DocumentSessionService};
use caret_app::document_storage::DocumentStorage;
use caret_app::document_view::DocumentViewRuntime;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

struct PreparedDocument {
    session: DocumentSessionService,
    layout_snapshot: String,
    expected_session_generation: u64,
    expected_document_generation: u64,
}

fn prepared_documents() -> &'static Mutex<HashMap<u64, PreparedDocument>> {
    static PREPARED: OnceLock<Mutex<HashMap<u64, PreparedDocument>>> = OnceLock::new();
    PREPARED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_prepared_document_ticket() -> u64 {
    static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);
    NEXT_TICKET.fetch_add(1, Ordering::Relaxed)
}

/// 持有文档会话、字体目录与布局缓存的 Rust 应用运行时。
pub struct DocumentRuntime {
    runtime: Result<DocumentSessionService, String>,
}

/// 创建空白文档运行时；初始化错误由首次快照请求返回。
pub(crate) fn new_blank_document_runtime() -> Box<DocumentRuntime> {
    Box::new(DocumentRuntime {
        runtime: DocumentSessionService::new_blank(),
    })
}

/// 在 worker 中读取、解析、创建 Session 並完成首次整篇排版。
pub(crate) fn prepare_document_file(
    path: String,
    expected_session_generation: u64,
    expected_document_generation: u64,
) -> bridge::DocumentOpenResult {
    let result = (|| {
        let bytes = DocumentStorage
            .read_file_bytes(Path::new(&path))
            .map_err(|error| error.to_string())?;
        let next_generation = expected_session_generation
            .checked_add(1)
            .ok_or_else(|| "document.session_generation_exhausted".to_owned())?;
        let (session, layout_snapshot) =
            DocumentSessionService::prepare_open(&bytes, Path::new(&path), next_generation)?;
        let ticket = next_prepared_document_ticket();
        if ticket == 0 {
            return Err("document.open_ticket_exhausted".to_owned());
        }
        let prepared = PreparedDocument {
            session,
            layout_snapshot,
            expected_session_generation,
            expected_document_generation,
        };
        prepared_documents()
            .lock()
            .map_err(|_| "document.open_registry_unavailable".to_owned())?
            .insert(ticket, prepared);
        Ok(ticket)
    })();
    match result {
        Ok(ticket) => bridge::DocumentOpenResult {
            ticket,
            error: String::new(),
        },
        Err(error) => bridge::DocumentOpenResult { ticket: 0, error },
    }
}

/// 在 GUI 线程以 ticket 安装候选会话；捕获后发生的输入会令本次打开过期。
pub(crate) fn install_prepared_document_json(runtime: &mut DocumentRuntime, ticket: u64) -> String {
    let prepared = match prepared_documents().lock() {
        Ok(mut documents) => documents.remove(&ticket),
        Err(_) => return "error:document.open_registry_unavailable".to_owned(),
    };
    let Some(prepared) = prepared else {
        return "error:document.open_ticket_expired".to_owned();
    };
    match &mut runtime.runtime {
        Ok(runtime) => runtime
            .install_prepared(
                prepared.session,
                prepared.layout_snapshot,
                prepared.expected_session_generation,
                prepared.expected_document_generation,
            )
            .unwrap_or_else(|error| format!("error:{error}")),
        Err(error) => format!("error:{error}"),
    }
}

/// 在后台将已验证的快照写入同目录临时文件并原子替换目标。
pub(crate) fn write_document_file(path: String, bytes: Vec<u8>) -> String {
    DocumentStorage
        .save_snapshot(Path::new(&path), &bytes)
        .err()
        .map_or_else(String::new, |error| error.to_string())
}

/// 捕获文档快照供后台写入；快照修订不会随后续输入变化。
pub(crate) fn document_save_snapshot(
    runtime: &mut DocumentRuntime,
) -> bridge::DocumentSaveSnapshot {
    match &runtime.runtime {
        Ok(runtime) => match runtime.save_snapshot() {
            Ok(snapshot) => to_bridge_snapshot(snapshot),
            Err(error) => empty_save_snapshot(error),
        },
        Err(error) => empty_save_snapshot(error.clone()),
    }
}

fn to_bridge_snapshot(snapshot: DocumentSaveSnapshot) -> bridge::DocumentSaveSnapshot {
    bridge::DocumentSaveSnapshot {
        session_generation: snapshot.session_generation,
        document_generation: snapshot.document_generation,
        bytes: snapshot.bytes,
        error: String::new(),
    }
}

fn empty_save_snapshot(error: String) -> bridge::DocumentSaveSnapshot {
    bridge::DocumentSaveSnapshot {
        session_generation: 0,
        document_generation: 0,
        bytes: Vec::new(),
        error,
    }
}

/// 在写入成功后推进对应会话的保存点；旧会话回报不改变当前状态。
pub(crate) fn mark_document_saved_json(
    runtime: &mut DocumentRuntime,
    session_generation: u64,
    document_generation: u64,
    path: String,
) -> String {
    match &mut runtime.runtime {
        Ok(runtime) => {
            let accepted =
                runtime.mark_saved(session_generation, document_generation, Path::new(&path));
            if accepted {
                runtime.status_json()
            } else {
                "error:document.stale_save_completion".to_owned()
            }
        }
        Err(error) => format!("error:{error}"),
    }
}

/// 返回当前文档身份和保存状态的轻量 JSON 投影。
pub(crate) fn document_status_json(runtime: &mut DocumentRuntime) -> String {
    match &runtime.runtime {
        Ok(runtime) => runtime.status_json(),
        Err(error) => format!("error:{error}"),
    }
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
    json_response(runtime, |runtime| runtime.insert_text_delta_json(text))
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
        Ok(runtime) => operation(runtime.view_mut()),
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
    let rasterized = match runtime.view_mut().rasterize_page_glyphs(
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
