//! native 与 Rust 的边界。导出应用版本、主题目录和持久的文档布局运行时。

// cxx 展开的桥在子模块里。allow 只覆盖生成胶水，手写代码不写 unsafe。
#[allow(unsafe_code, clippy::all)]
mod generated {
    #[cxx::bridge(namespace = "caret::ffi")]
    mod bridge {
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

        extern "Rust" {
            type DocumentRuntime;

            fn version() -> String;
            fn builtin_theme_catalog() -> String;
            /// Rust 分配运行时；C++ 通过 rust::Box 持有并负责析构，句柄有效期随 Box。
            fn new_blank_document_runtime() -> Box<DocumentRuntime>;
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
            /// 返回当前 Session 插入点在最新布局中的页面几何。
            fn current_caret_json(runtime: &mut DocumentRuntime) -> String;
            /// 只栅格化请求页，图像字节走二进制向量，不经 JSON/Base64。
            fn rasterize_page_glyphs(
                runtime: &mut DocumentRuntime,
                page_index: u32,
                px_per_pt: f64,
                page_origin_device_x_px: f64,
                page_origin_device_y_px: f64,
                foreground_red: u8,
                foreground_green: u8,
                foreground_blue: u8,
                foreground_alpha: u8,
            ) -> PageGlyphFrame;
        }
    }

    /// 持有文档会话、字体目录与布局缓存的 Rust 应用运行时。
    pub struct DocumentRuntime {
        runtime: Result<caret_app::document_view::DocumentViewRuntime, String>,
    }

    fn version() -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }

    /// 成功时是 JSON。失败时以 `error:` 开头，调用方不要把它当成快照。
    fn builtin_theme_catalog() -> String {
        match caret_app::theme::builtin_theme_catalog() {
            Ok(json) => json,
            Err(error) => format!("error:{error}"),
        }
    }

    /// 创建空白文档运行时；初始化错误由首次快照请求返回。
    fn new_blank_document_runtime() -> Box<DocumentRuntime> {
        Box::new(DocumentRuntime {
            runtime: caret_app::document_view::DocumentViewRuntime::new_blank(),
        })
    }

    /// 返回页面/段落布局 JSON；错误以 `error:` 开头，调用方不可当作快照消费。
    fn layout_snapshot_json(runtime: &mut DocumentRuntime) -> String {
        match &mut runtime.runtime {
            Ok(runtime) => match runtime.layout_snapshot_json() {
                Ok(json) => json,
                Err(error) => format!("error:{error}"),
            },
            Err(error) => format!("error:{error}"),
        }
    }

    /// 将页面内命中委托给与快照相同的 Rust 布局结果。
    fn document_hit_test_json(
        runtime: &mut DocumentRuntime,
        page_index: i32,
        x_pt: f64,
        y_pt: f64,
    ) -> String {
        match &mut runtime.runtime {
            Ok(runtime) => match runtime.hit_test_json(page_index, x_pt, y_pt) {
                Ok(json) => json,
                Err(error) => format!("error:{error}"),
            },
            Err(error) => format!("error:{error}"),
        }
    }

    /// 在同一布局命中结果上更新会话选区，不跨越 FFI 暴露文档节点。
    fn place_caret_at_page_json(
        runtime: &mut DocumentRuntime,
        page_index: i32,
        x_pt: f64,
        y_pt: f64,
    ) -> String {
        match &mut runtime.runtime {
            Ok(runtime) => match runtime.place_caret_at_page_json(page_index, x_pt, y_pt) {
                Ok(json) => json,
                Err(error) => format!("error:{error}"),
            },
            Err(error) => format!("error:{error}"),
        }
    }

    fn insert_text_json(runtime: &mut DocumentRuntime, text: String) -> String {
        match &mut runtime.runtime {
            Ok(runtime) => match runtime.insert_text_json(text) {
                Ok(json) => json,
                Err(error) => format!("error:{error}"),
            },
            Err(error) => format!("error:{error}"),
        }
    }

    fn current_caret_json(runtime: &mut DocumentRuntime) -> String {
        match &mut runtime.runtime {
            Ok(runtime) => match runtime.current_caret_json() {
                Ok(json) => json,
                Err(error) => format!("error:{error}"),
            },
            Err(error) => format!("error:{error}"),
        }
    }

    /// 将 Rust 页面布局和 glyph 栅格结果转换为紧凑 CXX 二进制 DTO。
    fn rasterize_page_glyphs(
        runtime: &mut DocumentRuntime,
        page_index: u32,
        px_per_pt: f64,
        page_origin_device_x_px: f64,
        page_origin_device_y_px: f64,
        foreground_red: u8,
        foreground_green: u8,
        foreground_blue: u8,
        foreground_alpha: u8,
    ) -> bridge::PageGlyphFrame {
        let runtime = match &mut runtime.runtime {
            Ok(runtime) => runtime,
            Err(error) => return empty_glyph_frame(page_index, error.clone()),
        };
        let rasterized = match runtime.rasterize_page_glyphs(
            usize::try_from(page_index).unwrap_or(usize::MAX),
            px_per_pt,
            [page_origin_device_x_px, page_origin_device_y_px],
            [
                foreground_red,
                foreground_green,
                foreground_blue,
                foreground_alpha,
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
        use super::{
            builtin_theme_catalog, current_caret_json, document_hit_test_json, insert_text_json,
            layout_snapshot_json, new_blank_document_runtime, place_caret_at_page_json,
            rasterize_page_glyphs, version,
        };

        #[test]
        fn ffi_version_matches_package_metadata() {
            assert_eq!(version(), env!("CARGO_PKG_VERSION"));
        }

        #[test]
        fn ffi_theme_catalog_contains_the_builtin_theme_snapshots() {
            let catalog = builtin_theme_catalog();
            assert!(!catalog.starts_with("error:"));
            assert!(catalog.contains("\"light\""));
            assert!(catalog.contains("\"dark\""));
            assert!(catalog.contains("\"colorBackground\""));
        }

        #[test]
        fn ffi_layout_snapshot_contains_rust_page_and_paragraph_layout() {
            let mut runtime = new_blank_document_runtime();
            let snapshot = layout_snapshot_json(&mut runtime);
            assert!(!snapshot.starts_with("error:"));
            assert!(snapshot.contains("\"widthPt\":595."));
            assert!(snapshot.contains("\"heightPt\":841."));
            assert!(snapshot.contains("\"paragraphs\":["));
            assert!(snapshot.contains("\"fontGeneration\":\"1\""));
            assert_eq!(layout_snapshot_json(&mut runtime), snapshot);
            let hit = document_hit_test_json(&mut runtime, 0, 73.0, 73.0);
            assert!(hit.contains("\"offsetUtf8\":\"0\""));
            assert_eq!(document_hit_test_json(&mut runtime, -1, 73.0, 73.0), "null");
            assert!(
                place_caret_at_page_json(&mut runtime, 0, 73.0, 73.0)
                    .contains("\"offsetUtf8\":\"0\"")
            );
            assert_eq!(
                place_caret_at_page_json(&mut runtime, -1, 73.0, 73.0),
                "null"
            );
            let inserted = insert_text_json(&mut runtime, "xλ".to_owned());
            assert!(!inserted.starts_with("error:"));
            assert!(inserted.contains("\"documentGeneration\":\"1\""));
            assert!(inserted.contains("\"text\":\"xλ\""));
            let caret = current_caret_json(&mut runtime);
            assert!(caret.contains("\"pageIndex\":0"));
            assert!(caret.contains("\"xPt\":"));
        }

        #[test]
        fn ffi_glyph_frame_uses_binary_pixels_and_reports_invalid_geometry() {
            let mut runtime = new_blank_document_runtime();
            let frame = rasterize_page_glyphs(&mut runtime, 0, 1.0, 0.0, 0.0, 32, 32, 32, 255);
            assert!(frame.error.is_empty());
            assert_eq!(frame.page_index, 0);
            assert_eq!(frame.document_generation, 0);
            assert_eq!(frame.font_generation, 1);
            assert!(frame.images.is_empty());
            assert!(frame.placements.is_empty());

            let invalid =
                rasterize_page_glyphs(&mut runtime, 0, f64::NAN, 0.0, 0.0, 32, 32, 32, 255);
            assert_eq!(invalid.error, "layout.glyphs.invalid_raster_geometry");
            assert!(invalid.images.is_empty());
            assert!(invalid.placements.is_empty());
        }
    }
}
