//! native 与 Rust 的边界。目前导出版本字符串，以及已校验的内置主题快照。
//! 没有文档、命令或页面快照类型。

// cxx 展开的桥在子模块里。allow 只覆盖生成胶水，手写代码不写 unsafe。
#[allow(unsafe_code, clippy::all)]
mod generated {
    #[cxx::bridge(namespace = "caret::ffi")]
    mod bridge {
        extern "Rust" {
            fn version() -> String;
            fn builtin_theme_catalog() -> String;
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
}
