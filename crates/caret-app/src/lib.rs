//! 应用服务：路径、设置、语言、主题和运行时上下文。
//!
//! 还不打开文档，也不创建编辑会话。主题不读文件。语言目录不依赖 Qt。
//! 设置文件由调用方传入路径。

pub mod document_storage;
pub mod document_view;
pub mod language;
pub mod path;
pub mod runtime;
pub mod settings;
pub mod theme;
