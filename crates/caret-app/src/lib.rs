//! 应用服务：文档会话、布局视图、路径、设置、语言、主题和运行时上下文。
//!
//! 文档视图创建 Rust `Session` 并持有字体/布局运行时；存储路径和设置文件路径由调用方传入。
//! 主题不读文件，语言目录不依赖 Qt。

pub mod document_session;
pub mod document_storage;
pub mod document_view;
pub mod path;
pub mod runtime;
pub mod settings;
mod storage;
