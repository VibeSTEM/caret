//! 定义编辑命令使用的结构化错误。

use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    InvalidDocument(String),
    UnknownParagraph(String),
    WrongStory,
    InvalidPosition,
    CrossParagraphSelection,
    UnsupportedOpaqueContent,
    UnsupportedNewline,
    IncompatibleParagraphMerge,
    InvalidStructureSelection,
    InvalidUtf16Offset,
    InvalidCompositionId,
    CompositionPending,
    InvalidPageGeometry(String),
    UnknownCommand,
}

impl Display for EditError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDocument(reason) => write!(f, "edit.invalid_document: {reason}"),
            Self::UnknownParagraph(id) => write!(f, "edit.unknown_paragraph: {id}"),
            Self::WrongStory => f.write_str("edit.position_story_mismatch"),
            Self::InvalidPosition => f.write_str("edit.invalid_position"),
            Self::CrossParagraphSelection => f.write_str("edit.cross_paragraph_selection"),
            Self::UnsupportedOpaqueContent => f.write_str("edit.opaque_content_boundary"),
            Self::UnsupportedNewline => f.write_str("edit.newline_requires_paragraph_command"),
            Self::IncompatibleParagraphMerge => f.write_str("edit.incompatible_paragraph_merge"),
            Self::InvalidStructureSelection => f.write_str("edit.structure_requires_caret"),
            Self::InvalidUtf16Offset => f.write_str("edit.invalid_utf16_offset"),
            Self::InvalidCompositionId => f.write_str("edit.invalid_composition_id"),
            Self::CompositionPending => f.write_str("edit.composition_pending"),
            Self::InvalidPageGeometry(reason) => {
                write!(f, "edit.invalid_page_geometry: {reason}")
            }
            Self::UnknownCommand => f.write_str("edit.unknown_command"),
        }
    }
}

impl std::error::Error for EditError {}
