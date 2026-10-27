//! Qt-agnostic editing commands and session state for Caret documents.

mod editing;
mod layout;
mod layout_view;
mod page_glyphs;

pub use caret_layout::{HitAffinity, VisualAffinity};

pub use editing::{
    Affinity, Command, Direction, EditError, Effect, InputComposition, Pos, PositionMap, Selection,
    Session, StoryId, utf8_offset_to_utf16, utf16_offset_to_utf8,
};
pub use layout::{
    DocumentLayoutError, DocumentLayoutRuntime, DocumentLayoutService, DocumentLayoutSnapshot,
    LaidOutParagraph, PageCaretGeometry, PageHit, PageLayoutSnapshot, PageSelectionRect,
};
pub use layout_view::{
    DocumentLayoutDeltaView, DocumentLayoutView, DocumentPositionView, LayoutLineView,
    PageCaretView, PageFragmentView, PageHitView, PageLayoutView, PageMarginsView,
    ParagraphLayoutView, StoryView,
};
pub use page_glyphs::{
    PageGlyph, PageGlyphError, PageGlyphImage, PageGlyphPlacement, PageGlyphRasterizer,
    PageGlyphRun, PageGlyphSnapshot, RasterizedPageGlyphs,
};
