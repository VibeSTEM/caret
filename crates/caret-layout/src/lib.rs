//! 提供 Unicode 文字分析、系统字体目录、字体回退与 glyph/cluster 位置映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pub mod font_selection;
mod fonts;
mod glyph_raster;
mod line_metrics;
pub mod pagination;
mod paragraph;
mod paragraph_style;
mod shaping;

pub use font_selection::{FontPreferenceError, FontPreferenceRules, ResolvedDocumentTextSettings};
pub use fontique::Synthesis as GlyphSynthesis;
pub use fonts::{FontCatalog, FontCatalogError, FontFamily};
pub use glyph_raster::{GlyphRasterOptions, GlyphRasterizer, RasterizedGlyph, RasterizedGlyphKey};
pub use line_metrics::{
    FontLineMetrics, InkBounds, InlineObjectMetrics, LineMetrics, LineMetricsError, LineSpacing,
    calculate_line_metrics, calculate_line_metrics_with_grid,
};
pub use pagination::{
    PageLayout, PaginationError, PaginationItem, ParagraphFragment, ParagraphPaginationInput,
    paginate_paragraphs,
};
pub use paragraph::{
    HitAffinity, ParagraphHit, ParagraphLayout, ParagraphLayoutError, ParagraphLayoutOptions,
    ParagraphLayouter, ParagraphLine, ParagraphObjects, ParleyParagraphLayout, VisualAffinity,
    VisualCursor, has_empty_line, paragraph_has_empty_line,
};
pub use paragraph_style::{ParagraphStyleError, ResolvedParagraphStyle, resolve_paragraph_style};
pub use parley::{FontData as GlyphFontData, PositionedLayoutItem as PositionedGlyphLayoutItem};
pub use shaping::{FontRole, ShapingStyle, UnicodeBoundaries, paragraph_text};

#[cfg(test)]
mod tests;

/// Caret 固定使用的 Unicode 数据版本。
pub const UNICODE_DATA_VERSION: &str = "17.0.0";

/// 把进程内静态字体字节包装为字形资源；同一字节切片应只包装一次，以便共享栅格缓存键。
pub fn static_glyph_font_data(bytes: &'static [u8], index: u32) -> GlyphFontData {
    GlyphFontData::new(
        parley::fontique::Blob::new(std::sync::Arc::new(bytes)),
        index,
    )
}
