//! native 与 Rust 的类型桥接入口；手写转发按文档和设置分类。

mod document;
mod settings;
#[cfg(test)]
mod tests;

fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

// 宏展开允许生成胶水使用 unsafe；手写运行时和转发保持普通 workspace lint。
#[allow(unsafe_code, clippy::all)]
mod generated {
    use crate::document::{
        DocumentRuntime, auto_save_deadline_fired, auto_save_deadline_json,
        auto_save_write_finished, cancel_pending_recovery_lease, cancel_prepared_document,
        clear_selection_json, configure_document_recovery, current_caret_json,
        delete_backward_json, discard_document_recovery, discard_recovery, document_hit_test_json,
        document_save_snapshot, document_session_generation, document_status_json,
        drag_hit_map_json, export_recovery, input_method_event_json, insert_soft_break_json,
        insert_text_json, install_prepared_document, interaction_state_json, layout_snapshot_json,
        mark_document_saved_json, move_to_visual_line_edge_json, move_vertical_caret_json,
        move_visual_caret_json, new_blank_document_runtime, paste_text_json,
        place_caret_at_page_json, prepare_document_file, prepare_recovery_document,
        rasterize_page_glyphs, recovery_entries_json, recovery_update_deadline_fired,
        recovery_update_write_finished, redo_json, selected_text_json,
        set_document_auto_save_enabled, set_document_recovery_id, set_selection_json,
        split_paragraph_json, undo_json, update_recovery_document, write_document_file,
        write_document_file_checked, write_document_file_replace_external,
    };
    use crate::settings::{
        SettingsRuntime, builtin_theme_catalog, new_settings_runtime, set_auto_save_enabled,
        set_caret_blink_enabled, settings_constraints, settings_load_warning, settings_snapshot,
        settings_theme_catalog,
    };
    use crate::version;

    #[cxx::bridge(namespace = "caret::ffi")]
    pub(crate) mod bridge {
        struct GlyphRasterRequest {
            page_index: u32,
            px_per_pt: f64,
            page_origin_device_x_px: f64,
            page_origin_device_y_px: f64,
            foreground_red: u8,
            foreground_green: u8,
            foreground_blue: u8,
            foreground_alpha: u8,
        }

        struct GlyphImage {
            width: u32,
            height: u32,
            left: i32,
            top: i32,
            pixel_phase_applied: bool,
            pixels: Vec<u8>,
        }

        struct GlyphPlacement {
            image_index: u32,
            x_pt: f64,
            baseline_y_pt: f64,
            pixel_phase_x: f32,
            pixel_phase_y: f32,
            clip_enabled: bool,
            clip_y0_pt: f64,
            clip_y1_pt: f64,
        }

        struct PageGlyphFrame {
            document_generation: u64,
            font_generation: u64,
            page_index: u32,
            error: String,
            images: Vec<GlyphImage>,
            placements: Vec<GlyphPlacement>,
        }

        struct DocumentOpenResult {
            ticket: u64,
            snapshot_json: String,
            error: String,
        }

        struct DocumentWriteResult {
            conflict: bool,
            replace_allowed: bool,
            recovery_id: String,
            recovery_created: bool,
            error: String,
        }

        struct DocumentSaveSnapshot {
            stale_deadline: bool,
            session_generation: u64,
            document_generation: u64,
            history_state_id: u64,
            bytes: Vec<u8>,
            content_digest: String,
            baseline_digest: String,
            recovery_id: String,
            recovery_original_path: String,
            recovery_baseline_digest: String,
            error: String,
        }

        struct SettingsSnapshot {
            caret_blink_enabled: bool,
            auto_save_enabled: bool,
            window_width: u32,
            window_height: u32,
            theme_id: String,
            language: String,
            error: String,
            warning: String,
        }

        struct SettingsConstraints {
            minimum_window_width: u32,
            minimum_window_height: u32,
        }

        extern "Rust" {
            type DocumentRuntime;
            type SettingsRuntime;

            fn version() -> String;
            fn builtin_theme_catalog() -> String;
            /// Rust 分配运行时；C++ 通过 rust::Box 持有并负责析构，句柄有效期随 Box。
            fn new_blank_document_runtime() -> Box<DocumentRuntime>;
            /// 后台解析文档、构建 Session 并完成首次布局；返回临时 ticket。
            fn prepare_document_file(
                path: String,
                expected_session_generation: u64,
                expected_document_generation: u64,
            ) -> DocumentOpenResult;
            /// 释放未安装的候选文档 ticket。
            fn cancel_prepared_document(ticket: u64);
            /// 在 GUI 线程安装候选文档；打开期间文档若被修改则拒绝替换。
            fn install_prepared_document(runtime: &mut DocumentRuntime, ticket: u64) -> String;
            /// 后台线程原子写入已验证快照；路径 I/O 由 Rust 应用服务完成。
            fn write_document_file(path: String, bytes: Vec<u8>) -> String;
            /// 校验文件打开时的摘要；外部冲突会先保存本地恢复 envelope。
            fn write_document_file_checked(
                path: String,
                bytes: Vec<u8>,
                baseline_digest: String,
                queued_digest: String,
                recovery_id: String,
            ) -> DocumentWriteResult;
            /// 释放未领取的新恢复租约，保留恢复条目。
            fn cancel_pending_recovery_lease(id: String);
            /// 将冲突恢复项绑定至仍然活动的文档会话。
            fn set_document_recovery_id(
                runtime: &mut DocumentRuntime,
                session_generation: u64,
                id: String,
                original_path: String,
                baseline_digest: String,
            ) -> String;
            /// 用户确认替换前备份外部原始字节，并原子写入本地快照。
            fn write_document_file_replace_external(path: String, bytes: Vec<u8>) -> String;
            /// 配置由 Qt 提供的本机应用数据目录。
            fn configure_document_recovery(root: String) -> String;
            /// 列出启动恢复提示所需的本地文档元数据。
            fn recovery_entries_json() -> String;
            /// 丢弃用户明确选择的恢复条目。
            fn discard_recovery(id: String) -> String;
            /// 将恢复区中的外部原始文件备份写到用户选择的位置。
            fn export_recovery(id: String, path: String) -> String;
            /// 丢弃当前文档时删除其关联恢复条目。
            fn discard_document_recovery(runtime: &mut DocumentRuntime) -> String;
            /// 后台准备恢复文档，安装前仍校验当前会话代际。
            fn prepare_recovery_document(
                id: String,
                expected_session_generation: u64,
                expected_document_generation: u64,
            ) -> DocumentOpenResult;
            /// 将恢复会话的新快照原子更新到同一恢复项。
            fn update_recovery_document(
                id: String,
                original_path: String,
                baseline_digest: String,
                bytes: Vec<u8>,
            ) -> String;
            /// 捕获恢复副本刷新 deadline 对应的最新文档快照。
            fn recovery_update_deadline_fired(
                runtime: &mut DocumentRuntime,
                session_generation: u64,
                generation: u64,
            ) -> DocumentSaveSnapshot;
            /// 完成恢复副本写入并返回下一个有效 deadline。
            fn recovery_update_write_finished(
                runtime: &mut DocumentRuntime,
                session_generation: u64,
                revision: u64,
                succeeded: bool,
            ) -> String;
            /// 捕获不可变文档字节和会话/文档代际。
            fn document_save_snapshot(runtime: &mut DocumentRuntime) -> DocumentSaveSnapshot;
            /// 轻量读取当前会话代际，避免仅校验回调时序列化整份文档。
            fn document_session_generation(runtime: &DocumentRuntime) -> u64;
            /// 设置当前会话 AutoSave 运行状态；持久化由 Preferences 服务负责。
            fn set_document_auto_save_enabled(runtime: &mut DocumentRuntime, enabled: bool);
            /// 返回 Rust 决定的单次 deadline；空字符串表示没有待办触发。
            fn auto_save_deadline_json(runtime: &DocumentRuntime) -> String;
            /// 校验 timer generation 后捕获自动保存快照。
            fn auto_save_deadline_fired(
                runtime: &mut DocumentRuntime,
                session_generation: u64,
                generation: u64,
            ) -> DocumentSaveSnapshot;
            /// 报告自动保存结果并校验快照对应的保存点。
            fn auto_save_write_finished(
                runtime: &mut DocumentRuntime,
                session_generation: u64,
                document_generation: u64,
                history_state_id: u64,
                path: String,
                content_digest: String,
                succeeded: bool,
            ) -> String;
            /// 写入成功后推进对应会话的保存点。
            fn mark_document_saved_json(
                runtime: &mut DocumentRuntime,
                session_generation: u64,
                document_generation: u64,
                history_state_id: u64,
                path: String,
                content_digest: String,
            ) -> String;
            /// 返回文档名、路径和未保存状态。
            fn document_status_json(runtime: &mut DocumentRuntime) -> String;
            /// 在 Rust Session 中撤销/重做并返回最新布局快照。
            fn undo_json(runtime: &mut DocumentRuntime) -> String;
            fn redo_json(runtime: &mut DocumentRuntime) -> String;
            /// 从宿主给定的配置路径载入 Rust 偏好服务；无效文件会按服务规则隔离并回退默认值。
            fn new_settings_runtime(settings_path: String) -> Box<SettingsRuntime>;
            /// 获取 Rust SettingsService 派生的界面快照。
            fn settings_snapshot(runtime: &mut SettingsRuntime) -> SettingsSnapshot;
            /// 获取当前 SettingsService 中活动主题对应的完整主题目录。
            fn settings_theme_catalog(runtime: &mut SettingsRuntime) -> String;
            /// 返回由 Rust Preferences 校验的窗口尺寸约束。
            fn settings_constraints() -> SettingsConstraints;
            /// 返回启动时设置加载的非致命诊断；无诊断时为空。
            fn settings_load_warning(runtime: &mut SettingsRuntime) -> String;
            /// 更新单项偏好；校验或持久化失败时保留当前内存快照。
            fn set_caret_blink_enabled(
                runtime: &mut SettingsRuntime,
                enabled: bool,
            ) -> SettingsSnapshot;
            /// 更新 AutoSave 全局偏好；失败时设置服务保留原快照。
            fn set_auto_save_enabled(
                runtime: &mut SettingsRuntime,
                enabled: bool,
            ) -> SettingsSnapshot;
            /// 同步调用期间借用句柄，不得留存引用或跨线程调用；返回 UTF-8 JSON 或 error: 错误。
            fn layout_snapshot_json(runtime: &mut DocumentRuntime) -> String;
            /// 返回仅含 caret、选区和 IME 上下文的轻量交互快照。
            fn interaction_state_json(runtime: &mut DocumentRuntime) -> String;
            /// 仅在持有句柄的 UI 线程同步调用，不保留借用；无位置返回 JSON null，错误以 error: 开头。
            fn document_hit_test_json(
                runtime: &mut DocumentRuntime,
                page_index: i32,
                x_pt: f64,
                y_pt: f64,
            ) -> String;
            /// 在命中位置设置 Rust 会话插入点；无位置返回 JSON null。
            fn place_caret_at_page_json(
                runtime: &mut DocumentRuntime,
                page_index: i32,
                x_pt: f64,
                y_pt: f64,
            ) -> String;
            /// 将 Qt 文本输入提交给 Rust Session，返回完整分页和当前段落的布局增量。
            fn insert_text_json(runtime: &mut DocumentRuntime, text: String) -> String;
            /// 原子插入剪贴板文本，换行拆段并以 Rust 快照返回结果。
            fn paste_text_json(runtime: &mut DocumentRuntime, text: String) -> String;
            /// 在 Rust Session 中执行 Enter 对应的段落拆分。
            fn split_paragraph_json(runtime: &mut DocumentRuntime) -> String;
            /// 在 Rust Session 中执行 Backspace，对向后删除与段首合并进行处理。
            fn delete_backward_json(runtime: &mut DocumentRuntime) -> String;
            /// 在 Rust Session 中插入段内软换行。
            fn insert_soft_break_json(runtime: &mut DocumentRuntime) -> String;
            /// 将 Qt 输入法预编辑、提交或取消事件提交给 Rust Session。
            fn input_method_event_json(
                runtime: &mut DocumentRuntime,
                preedit: String,
                commit: String,
                replacement_start: i32,
                replacement_length: i32,
                cursor_position_utf16: usize,
            ) -> String;
            /// 为一次鼠标拖选请求当前页的 caret 命中缓存。
            fn drag_hit_map_json(runtime: &mut DocumentRuntime, page_index: i32) -> String;
            /// 在 Rust Session 中提交拖选释放后的稳定位置。
            fn set_selection_json(runtime: &mut DocumentRuntime, selection_json: String) -> String;
            /// 清除当前选区并保留 Rust 会话中的焦点位置。
            fn clear_selection_json(runtime: &mut DocumentRuntime) -> String;
            /// 以 JSON 返回按文档顺序提取的选中文本。
            fn selected_text_json(runtime: &mut DocumentRuntime) -> String;
            /// 按 Parley 视觉顺序移动 caret，可选择扩展当前选区。
            fn move_visual_caret_json(
                runtime: &mut DocumentRuntime,
                forward: bool,
                extend: bool,
            ) -> String;
            /// 在相邻视觉行间移动并保留期望横坐标。
            fn move_vertical_caret_json(
                runtime: &mut DocumentRuntime,
                line_delta: i32,
                extend: bool,
            ) -> String;
            /// 移到当前视觉行首尾，可选择扩展当前选区。
            fn move_to_visual_line_edge_json(
                runtime: &mut DocumentRuntime,
                end: bool,
                extend: bool,
            ) -> String;
            /// 返回当前 Session 插入点在最新布局中的页面几何。
            fn current_caret_json(runtime: &mut DocumentRuntime) -> String;
            /// 只栅格化请求页，图像字节走二进制向量，不经 JSON/Base64。
            fn rasterize_page_glyphs(
                runtime: &mut DocumentRuntime,
                request: &GlyphRasterRequest,
            ) -> PageGlyphFrame;
        }
    }
}
