//! Caret 文档的领域模型与 `.cdoc` 编解码。
//!
//! 本 crate 不执行文件 I/O，也不依赖 Qt、布局或会话状态。未知可选字段
//! 随对应对象保留；未知节点或必需能力产生只读安全预览。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod model;
mod page_geometry;
mod storage_format;

pub use model::{
    Block, BlockEnvironment, BlockEnvironmentKind, DEFAULT_CODE_FONT_FAMILY, Document,
    DocumentFontTarget, DocumentSettings, ExplicitBreak, InlineEnvironment, InlineEnvironmentKind,
    InlineNode, MAX_DOCUMENT_NESTING_DEPTH, Media, ModelError, NodeId, OpaqueNode, PageSection,
    Paragraph, ParagraphLayoutOverrides, ParagraphLayoutStyle, ParagraphLineSpacing, Run,
    SoftBreak, ThemeSnapshot, default_font_size_centipoints, is_valid_language_tag,
};
pub use page_geometry::{
    Orientation, PageGeometry, PageGeometryError, PageMargins, PaperSize, millimeters_to_points,
    points_to_millimeters,
};
pub use storage_format::{
    CURRENT_FORMAT_VERSION, CURRENT_THEME_FORMAT_VERSION, CdocFile, DocumentError, OpenedDocument,
    PreviewReason, ReadOnlyPreview, open,
};
