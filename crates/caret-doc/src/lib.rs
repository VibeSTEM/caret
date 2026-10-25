//! Caret 文档的领域模型与 `.cdoc` 编解码。
//!
//! 本 crate 不执行文件 I/O，也不依赖 Qt、布局或会话状态。未知可选字段
//! 随对应对象保留；未知节点或必需能力产生只读安全预览。

mod model;
mod storage_format;

pub use model::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, DocumentSettings, InlineEnvironment,
    InlineEnvironmentKind, InlineNode, Media, NodeId, PageSection, Paragraph, Run, ThemeSnapshot,
};
pub use storage_format::{
    CURRENT_FORMAT_VERSION, CdocFile, DocumentError, OpenedDocument, PreviewReason,
    ReadOnlyPreview, open,
};
