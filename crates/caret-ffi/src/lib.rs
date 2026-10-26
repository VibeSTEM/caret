//! native 与 Rust 的边界。导出应用版本、主题目录和持久的文档布局运行时。

// cxx 展开的桥在子模块里。allow 只覆盖生成胶水，手写代码不写 unsafe。
#[allow(unsafe_code, clippy::all)]
mod generated {
    #[cxx::bridge(namespace = "caret::ffi")]
    mod bridge {
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

    #[cfg(test)]
    mod tests {
        use super::{
            builtin_theme_catalog, document_hit_test_json, layout_snapshot_json,
            new_blank_document_runtime, place_caret_at_page_json, version,
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
        }
    }
}
