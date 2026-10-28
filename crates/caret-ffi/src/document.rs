//! 文档运行时句柄、布局命令转发与 glyph 二进制快照转换。

use crate::generated::bridge;

mod autosave;
mod recovery;
pub(crate) use autosave::*;
use caret_app::document_session::{DocumentSaveSnapshot, DocumentSessionService};
use caret_app::document_storage::DocumentStorage;
use caret_app::document_view::DocumentViewRuntime;
pub(crate) use recovery::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub(super) struct PreparedDocument {
    pub(super) session: DocumentSessionService,
    pub(super) expected_session_generation: u64,
    pub(super) expected_document_generation: u64,
    pub(super) recovery_source:
        Option<(String, caret_app::document_recovery::RecoverySessionLease)>,
    pub(super) recovery_lease: Option<caret_app::document_recovery::RecoverySessionLease>,
}

pub(super) fn prepared_documents() -> &'static Mutex<HashMap<u64, PreparedDocument>> {
    static PREPARED: OnceLock<Mutex<HashMap<u64, PreparedDocument>>> = OnceLock::new();
    PREPARED.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn next_prepared_document_ticket() -> u64 {
    static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);
    NEXT_TICKET.fetch_add(1, Ordering::Relaxed)
}

/// 持有文档会话、字体目录与布局缓存的 Rust 应用运行时。
pub struct DocumentRuntime {
    pub(super) runtime: Result<DocumentSessionService, String>,
    pub(super) lifecycle: caret_app::document_lifecycle::DocumentLifecycleService,
    pub(super) recovery_lease: Option<caret_app::document_recovery::RecoverySessionLease>,
    pub(super) monotonic_origin: Instant,
}

/// 创建空白文档运行时；初始化错误由首次快照请求返回。
pub(crate) fn new_blank_document_runtime() -> Box<DocumentRuntime> {
    Box::new(DocumentRuntime {
        runtime: DocumentSessionService::new_blank(),
        lifecycle: caret_app::document_lifecycle::DocumentLifecycleService::new(1),
        recovery_lease: None,
        monotonic_origin: Instant::now(),
    })
}

/// 在 worker 中读取并准备文档会话；安装由调用方校验 generation 后完成。
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
            expected_session_generation,
            expected_document_generation,
            recovery_source: None,
            recovery_lease: None,
        };
        prepared_documents()
            .lock()
            .map_err(|_| "document.open_registry_unavailable".to_owned())?
            .insert(ticket, prepared);
        Ok((ticket, layout_snapshot))
    })();
    match result {
        Ok((ticket, snapshot_json)) => bridge::DocumentOpenResult {
            ticket,
            snapshot_json,
            error: String::new(),
        },
        Err(error) => bridge::DocumentOpenResult {
            ticket: 0,
            snapshot_json: String::new(),
            error,
        },
    }
}

/// 释放尚未安装的候选会话。
pub(crate) fn cancel_prepared_document(ticket: u64) {
    if let Ok(mut documents) = prepared_documents().lock()
        && let Some(mut prepared) = documents.remove(&ticket)
        && let Some(lease) = prepared.recovery_lease.take()
        && let Ok(storage) = crate::document::recovery_storage_for_cleanup()
    {
        let _ = storage.remove_leased(&lease);
    }
}

/// 在 GUI 线程以 ticket 安装候选会话；捕获后发生的输入会令本次打开过期。
pub(crate) fn install_prepared_document(runtime: &mut DocumentRuntime, ticket: u64) -> String {
    let prepared = match prepared_documents().lock() {
        Ok(mut documents) => documents.remove(&ticket),
        Err(_) => return "error:document.open_registry_unavailable".to_owned(),
    };
    let Some(mut prepared) = prepared else {
        return "error:document.open_ticket_expired".to_owned();
    };
    let installed = match &mut runtime.runtime {
        Ok(session) => session
            .install_prepared_session(
                prepared.session,
                prepared.expected_session_generation,
                prepared.expected_document_generation,
            )
            .map_err(|error| format!("error:{error}")),
        Err(error) => Err(format!("error:{error}")),
    };
    if let Err(error) = installed {
        if let Some(lease) = prepared.recovery_lease.take()
            && let Ok(storage) = crate::document::recovery_storage_for_cleanup()
        {
            let _ = storage.remove_leased(&lease);
        }
        return error;
    }
    if let Some((_source_id, source_lease)) = prepared.recovery_source.take()
        && let Ok(storage) = crate::document::recovery_storage_for_cleanup()
    {
        let _ = storage.remove_leased(&source_lease);
    }
    runtime.recovery_lease = prepared.recovery_lease.take();
    if let Ok(installed) = &runtime.runtime {
        runtime.lifecycle.set_session(
            installed.session_generation(),
            installed.recovery_id().is_some(),
        );
    }
    String::new()
}

/// 在后台将已验证的快照写入同目录临时文件并原子替换目标。
pub(crate) fn write_document_file(path: String, bytes: Vec<u8>) -> String {
    DocumentStorage
        .save_snapshot(Path::new(&path), &bytes)
        .err()
        .map_or_else(String::new, |error| error.to_string())
}

/// 捕获当前文档的保存快照，并在目标文件冲突时先保存本地恢复副本。
pub(crate) fn document_save_snapshot(
    runtime: &mut DocumentRuntime,
) -> bridge::DocumentSaveSnapshot {
    match &mut runtime.runtime {
        Ok(runtime) => match runtime.save_snapshot() {
            Ok(snapshot) => to_bridge_snapshot(snapshot),
            Err(error) => empty_save_snapshot(error),
        },
        Err(error) => empty_save_snapshot(error.clone()),
    }
}

/// 轻量读取活动会话代际，供 Qt 拒绝过期 worker 回调。
pub(crate) fn document_session_generation(runtime: &DocumentRuntime) -> u64 {
    runtime
        .runtime
        .as_ref()
        .map(DocumentSessionService::session_generation)
        .unwrap_or_default()
}

fn to_bridge_snapshot(snapshot: DocumentSaveSnapshot) -> bridge::DocumentSaveSnapshot {
    let recovery_id = snapshot.recovery_id.unwrap_or_default();
    bridge::DocumentSaveSnapshot {
        stale_deadline: false,
        session_generation: snapshot.session_generation,
        document_generation: snapshot.document_generation,
        history_state_id: snapshot.history_state_id,
        bytes: snapshot.bytes,
        content_digest: snapshot.content_digest,
        baseline_digest: snapshot.baseline_digest.unwrap_or_default(),
        recovery_id,
        recovery_original_path: snapshot.recovery_original_path.unwrap_or_default(),
        recovery_baseline_digest: snapshot.recovery_baseline_digest.unwrap_or_default(),
        error: String::new(),
    }
}

pub(super) fn empty_save_snapshot(error: String) -> bridge::DocumentSaveSnapshot {
    bridge::DocumentSaveSnapshot {
        stale_deadline: false,
        session_generation: 0,
        document_generation: 0,
        history_state_id: 0,
        bytes: Vec::new(),
        content_digest: String::new(),
        baseline_digest: String::new(),
        recovery_id: String::new(),
        recovery_original_path: String::new(),
        recovery_baseline_digest: String::new(),
        error,
    }
}

pub(super) fn stale_deadline_snapshot() -> bridge::DocumentSaveSnapshot {
    let mut snapshot = empty_save_snapshot(String::new());
    snapshot.stale_deadline = true;
    snapshot
}

/// 返回当前文档身份和保存状态的轻量 JSON 投影。
pub(crate) fn document_status_json(runtime: &mut DocumentRuntime) -> String {
    match &runtime.runtime {
        Ok(runtime) => runtime.status_json(),
        Err(error) => format!("error:{error}"),
    }
}

pub(crate) fn undo_json(runtime: &mut DocumentRuntime) -> String {
    json_response(runtime, DocumentViewRuntime::undo_json)
}

pub(crate) fn redo_json(runtime: &mut DocumentRuntime) -> String {
    json_response(runtime, DocumentViewRuntime::redo_json)
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
    let before_generation = runtime
        .runtime
        .as_ref()
        .ok()
        .map(|value| value.document_generation());
    let result = match &mut runtime.runtime {
        Ok(runtime) => operation(runtime.view_mut()),
        Err(error) => Err(error.clone()),
    };
    if result.is_ok()
        && let (Some(before), Ok(session)) = (before_generation, runtime.runtime.as_ref())
    {
        let after = session.document_generation();
        if after > before {
            let now = runtime.monotonic_origin.elapsed();
            let has_recovery = runtime
                .runtime
                .as_ref()
                .is_ok_and(|session| session.recovery_id().is_some());
            runtime.lifecycle.edited(after, has_recovery, now);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_failure_is_reported_by_document_commands_and_binary_results() {
        let mut runtime = DocumentRuntime {
            runtime: Err("document.initialization_failed".to_owned()),
            lifecycle: caret_app::document_lifecycle::DocumentLifecycleService::new(0),
            recovery_lease: None,
            monotonic_origin: Instant::now(),
        };
        let expected = "error:document.initialization_failed";
        let responses = [
            document_status_json(&mut runtime),
            mark_document_saved_json(&mut runtime, 1, 0, 0, "test.cdoc".to_owned(), String::new()),
            layout_snapshot_json(&mut runtime),
            interaction_state_json(&mut runtime),
            document_hit_test_json(&mut runtime, 0, 1.0, 1.0),
            place_caret_at_page_json(&mut runtime, 0, 1.0, 1.0),
            insert_text_json(&mut runtime, "text".to_owned()),
            paste_text_json(&mut runtime, "text".to_owned()),
            split_paragraph_json(&mut runtime),
            delete_backward_json(&mut runtime),
            insert_soft_break_json(&mut runtime),
            input_method_event_json(&mut runtime, "ni".to_owned(), String::new(), 0, 0, 2),
            drag_hit_map_json(&mut runtime, 0),
            set_selection_json(&mut runtime, "{}".to_owned()),
            clear_selection_json(&mut runtime),
            selected_text_json(&mut runtime),
            move_visual_caret_json(&mut runtime, true, false),
            move_vertical_caret_json(&mut runtime, 1, false),
            move_to_visual_line_edge_json(&mut runtime, true, false),
            current_caret_json(&mut runtime),
        ];
        assert!(responses.iter().all(|response| response == expected));

        let snapshot = document_save_snapshot(&mut runtime);
        assert_eq!(snapshot.error, "document.initialization_failed");
        assert!(snapshot.bytes.is_empty());
        let frame = rasterize_page_glyphs(
            &mut runtime,
            &bridge::GlyphRasterRequest {
                page_index: 2,
                px_per_pt: 1.0,
                page_origin_device_x_px: 0.0,
                page_origin_device_y_px: 0.0,
                foreground_red: 0,
                foreground_green: 0,
                foreground_blue: 0,
                foreground_alpha: 255,
            },
        );
        assert_eq!(frame.error, "document.initialization_failed");
        assert_eq!(frame.page_index, 2);
        assert!(frame.images.is_empty());
        assert!(frame.placements.is_empty());
    }
}
