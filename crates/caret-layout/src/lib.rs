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
pub mod table;

pub use font_selection::{FontPreferenceError, FontPreferenceRules, ResolvedDocumentTextSettings};
pub use fontique::Synthesis as GlyphSynthesis;
pub use fonts::{FontCatalog, FontCatalogError, FontFamily};
pub use glyph_raster::{GlyphRasterOptions, GlyphRasterizer, RasterizedGlyph, RasterizedGlyphKey};
pub use line_metrics::{
    FontLineMetrics, InkBounds, InlineObjectMetrics, LineMetrics, LineMetricsError, LineSpacing,
    calculate_content_line_metrics_with_grid, calculate_line_metrics,
    calculate_line_metrics_with_grid, row_spacing_for_content_line_spacing,
    row_spacing_for_line_spacing,
};
pub use pagination::{
    FragmentClip, FragmentHost, MathBlockFragment, MathBlockPaginationInput, PageLayout,
    PaginationError, PaginationItem, ParagraphFragment, ParagraphPaginationInput, TableFragment,
    TablePaginationInput, paginate_paragraphs,
};
pub use paragraph::{
    HitAffinity, InlineObjectLayout, ParagraphHit, ParagraphLayout, ParagraphLayoutError,
    ParagraphLayoutOptions, ParagraphLayouter, ParagraphLine, ParleyParagraphLayout,
    TextIslandError, TextIslandLayout, TextIslandMetrics, TextIslandSpan, VisualAffinity,
    VisualCursor, has_empty_line, paragraph_has_empty_line,
};
pub use paragraph_style::{ParagraphStyleError, ResolvedParagraphStyle, resolve_paragraph_style};
pub use parley::{FontData as GlyphFontData, PositionedLayoutItem as PositionedGlyphLayoutItem};
pub use shaping::{FontRole, ShapingStyle, UnicodeBoundaries, paragraph_text, run_has_underline};
pub use table::{
    TableCellGeometry, TableCellMeasurement, TableGridLayout, TableLayoutError, TableLayoutOptions,
    TableRowGeometry, TableRowMeasurement, layout_table_grid, table_column_width_pt,
};

#[cfg(test)]
mod tests;

/// Caret 固定使用的 Unicode 数据版本。
pub const UNICODE_DATA_VERSION: &str = "17.0.0";
