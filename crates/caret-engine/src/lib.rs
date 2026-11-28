//! 管理 Caret 文档的编辑命令与会话状态，不依赖 Qt。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod editing;
mod layout;
mod layout_view;
mod math_validation;
mod page_glyphs;

pub use caret_doc::{ClipboardFragment, DocumentFontTarget};
pub use caret_layout::{HitAffinity, VisualAffinity};
pub use caret_structure::{
    IndexedStructure, StructureClass, StructureIndex, StructureKind, StructurePosition,
    TableCellPath,
};

pub use editing::{
    Affinity, Capabilities, ClipboardAction, ClipboardCopy, Command, CommandCandidate, CommandLine,
    CommandLineMode, CommandLineWireField, CompositionRange, CompositionUpdateCandidate,
    CurrentEnvironmentView, Direction, DocumentCaretPosition, DocumentRangeEndpoint, EditError,
    EditorKey, Effect, EnvironmentEntry, EnvironmentKind, EnvironmentLevel, EnvironmentSource,
    InlineEnvironmentRange, InlineMathRangeParts, InlineSpan, InputComposition, MathEdit,
    MathEditBranch, MathEditTarget, MathPositionAffinity, MathPositionHost, MathSequencePath,
    MathSequenceStep, MathTextFlowPath, MathTextFlowPathStep, MathTextInputContext,
    MathTextPosition, MathTreePosition, MathTreePositionError, NavigationCandidate,
    NavigationGeneration, NavigationIntent, Pos, PositionMap, Selection, Session,
    SessionCaretSnapshot, SessionCheckpoint, StoryId, StructureSelection, TableDeleteBase,
    utf8_offset_to_utf16, utf16_offset_to_utf8,
};
pub use layout::{
    DocumentLayoutError, DocumentLayoutRuntime, DocumentLayoutService, DocumentLayoutSnapshot,
    FragmentHost, LaidOutParagraph, MathTextVerticalTarget, MathTreeVerticalTarget,
    PageCaretGeometry, PageCellHitRegion, PageEnvironmentRange, PageHit, PageLayoutSnapshot,
    PageMathStructureHit, PageMathTextDragStop, PageMathTextHit, PageMathTreeDragStop,
    PageSelectionRect, TextLayoutDiagnostic,
};
pub use layout_view::{
    DocumentLayoutDeltaView, DocumentLayoutView, DocumentPositionView, EnvironmentRangeView,
    LayoutLineView, PageCaretView, PageFragmentView, PageHitView, PageLayoutView, PageMarginsView,
    ParagraphLayoutView, StoryView, TextLayoutDiagnosticView,
};
pub use page_glyphs::{
    PageGlyph, PageGlyphDecoration, PageGlyphError, PageGlyphImage, PageGlyphPlacement,
    PageGlyphRasterizer, PageGlyphRun, PageGlyphSnapshot, PageMathShape, PageMathShapeKind,
    RasterizedPageGlyphs,
};
