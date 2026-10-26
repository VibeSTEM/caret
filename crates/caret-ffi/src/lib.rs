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
        DocumentRuntime, current_caret_json, delete_backward_json, document_hit_test_json,
        insert_soft_break_json, insert_text_json, layout_snapshot_json, new_blank_document_runtime,
        place_caret_at_page_json, rasterize_page_glyphs, split_paragraph_json,
    };
    use crate::settings::{
        SettingsRuntime, builtin_theme_catalog, new_settings_runtime, set_caret_blink_enabled,
        settings_constraints, settings_load_warning, settings_snapshot, settings_theme_catalog,
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

        struct SettingsSnapshot {
            caret_blink_enabled: bool,
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
            /// 同步调用期间借用句柄，不得留存引用或跨线程调用；返回 UTF-8 JSON 或 error: 错误。
            fn layout_snapshot_json(runtime: &mut DocumentRuntime) -> String;
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
            /// 将 Qt 文本输入提交给 Rust Session，成功时返回新的布局快照。
            fn insert_text_json(runtime: &mut DocumentRuntime, text: String) -> String;
            /// 在 Rust Session 中执行 Enter 对应的段落拆分。
            fn split_paragraph_json(runtime: &mut DocumentRuntime) -> String;
            /// 在 Rust Session 中执行 Backspace，对向后删除与段首合并进行处理。
            fn delete_backward_json(runtime: &mut DocumentRuntime) -> String;
            /// 在 Rust Session 中插入段内软换行。
            fn insert_soft_break_json(runtime: &mut DocumentRuntime) -> String;
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
