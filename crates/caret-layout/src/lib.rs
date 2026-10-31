//! 提供 Unicode 文字分析、系统字体目录、字体回退与 glyph/cluster 位置映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pub mod font_selection;
mod fonts;
mod glyph_raster;
mod line_metrics;
mod math_text;
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
pub use math_text::{MathInlineBoxMetrics, ParleyMathTextShaper, stix_two_math_font_data};
pub use pagination::{
    PageLayout, PaginationError, PaginationItem, ParagraphFragment, ParagraphPaginationInput,
    paginate_paragraphs,
};
pub use paragraph::{
    HitAffinity, InlineObjectPlacement, ParagraphHit, ParagraphLayout, ParagraphLayoutError,
    ParagraphLayoutOptions, ParagraphLayouter, ParagraphLine, ParleyParagraphLayout,
    VisualAffinity, VisualCursor, has_empty_line, paragraph_has_empty_line,
};
pub use paragraph_style::{ParagraphStyleError, ResolvedParagraphStyle, resolve_paragraph_style};
pub use parley::{FontData as GlyphFontData, PositionedLayoutItem as PositionedGlyphLayoutItem};
pub use shaping::{FontRole, ShapingStyle, UnicodeBoundaries, paragraph_text};

#[cfg(test)]
mod tests;

/// Caret 固定使用的 Unicode 数据版本。
pub const UNICODE_DATA_VERSION: &str = "17.0.0";
