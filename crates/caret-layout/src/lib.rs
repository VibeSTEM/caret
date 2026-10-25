//! 提供 Unicode 文字分析、系统字体目录、字体回退与 glyph/cluster 位置映射。

mod fonts;
mod shaping;

pub use fonts::{FontCatalog, FontCatalogError, FontFamily};
pub use shaping::{ShapedParagraph, ShapingError, ShapingStyle, TextShaper, UnicodeBoundaries};

#[cfg(test)]
mod tests;

/// Caret 固定使用的 Unicode 数据版本。
pub const UNICODE_DATA_VERSION: &str = "17.0.0";
