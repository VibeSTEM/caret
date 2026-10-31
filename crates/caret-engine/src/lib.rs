//! 管理 Caret 文档的编辑命令与会话状态，不依赖 Qt。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod editing;
mod layout;
mod layout_view;
mod math_edit;
mod math_import;
mod math_layout;
mod page_glyphs;

pub use caret_doc::DocumentFontTarget;
pub use caret_layout::{HitAffinity, VisualAffinity};
pub use caret_structure::{
    IndexedStructure, StructureClass, StructureIndex, StructureKind, StructurePosition,
};
pub use math_layout::{layout_math_align, layout_math_tree};

pub use editing::{
    Affinity, Command, CommandCandidate, CommandLine, CommandLineMode, CompositionUpdateCandidate,
    Direction, EditError, EditorKey, Effect, InlineEnvironmentRange, InputComposition,
    MathDirection, MathFocus, MathPos, Pos, PositionMap, Selection, Session, SessionCheckpoint,
    StoryId, utf8_offset_to_utf16, utf16_offset_to_utf8,
};
pub use layout::{
    DocumentLayoutError, DocumentLayoutRuntime, DocumentLayoutService, DocumentLayoutSnapshot,
    LaidOutParagraph, PageCaretGeometry, PageEnvironmentRange, PageHit, PageLayoutSnapshot,
    PageSelectionRect,
};
pub use layout_view::{
    DocumentLayoutDeltaView, DocumentLayoutView, DocumentPositionView, EnvironmentRangeView,
    LayoutLineView, PageCaretView, PageFragmentView, PageHitView, PageLayoutView, PageMarginsView,
    ParagraphLayoutView, StoryView,
};
pub use math_edit::{MathEdit, MathEditError, MathEditResult, apply_math_edit};
pub use math_import::{ImportedMath, MathImportError, import_parsed_math};
pub use page_glyphs::{
    PageGlyph, PageGlyphDecoration, PageGlyphDrawCommand, PageGlyphError, PageGlyphImage,
    PageGlyphLine, PageGlyphPlacement, PageGlyphRasterizer, PageGlyphRun, PageGlyphSnapshot,
    RasterizedPageGlyphs,
};
