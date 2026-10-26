//! native 与 Rust 的边界。目前导出版本、主题目录和空白文档的布局快照。

// cxx 展开的桥在子模块里。allow 只覆盖生成胶水，手写代码不写 unsafe。
#[allow(unsafe_code, clippy::all)]
mod generated {
    #[cxx::bridge(namespace = "caret::ffi")]
    mod bridge {
        extern "Rust" {
            fn version() -> String;
            fn builtin_theme_catalog() -> String;
            fn blank_document_layout_snapshot() -> String;
        }
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

    /// 返回成功的页面/段落布局 JSON；错误以 `error:` 开头，调用方不可当作快照消费。
    fn blank_document_layout_snapshot() -> String {
        match caret_app::document_view::blank_document_layout_snapshot() {
            Ok(json) => json,
            Err(error) => format!("error:{error}"),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{blank_document_layout_snapshot, builtin_theme_catalog, version};

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
            let snapshot = blank_document_layout_snapshot();
            assert!(!snapshot.starts_with("error:"));
            assert!(snapshot.contains("\"widthPt\":595."));
            assert!(snapshot.contains("\"heightPt\":841."));
            assert!(snapshot.contains("\"paragraphs\":["));
            assert!(snapshot.contains("\"fontGeneration\":\"1\""));
        }
    }
}
