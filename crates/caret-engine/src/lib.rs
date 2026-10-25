//! Qt-agnostic editing commands and session state for Caret documents.

mod editing;

pub use editing::{
    Affinity, Command, Direction, EditError, Effect, InputComposition, Pos, PositionMap, Selection,
    Session, StoryId, utf8_offset_to_utf16, utf16_offset_to_utf8,
};
