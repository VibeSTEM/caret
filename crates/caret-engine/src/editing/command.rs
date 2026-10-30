//! 定义编辑命令及其稳定命令 ID。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{
    BlockEnvironmentKind, DocumentFontTarget, InlineEnvironmentKind, NodeId, PageGeometry,
};

use super::Selection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Backward,
    Forward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorKey {
    Escape,
    Enter,
    Tab,
    ShiftEnter,
    ModBackspace,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    InsertText(String),
    InsertOneCharacterWidthSpace,
    InsertTwoCharacterWidthSpace,
    PasteText(String),
    InsertSoftBreak,
    InsertParagraphBreak,
    Delete(Direction),
    MoveCaret(Direction),
    MoveStructure(Direction),
    ExitBlockEnvironment(Direction),
    RemoveBlockEnvironmentPreservingContent,
    RemoveInlineEnvironmentPreservingContent(NodeId),
    ExpandStructureSelection,
    ExitStructure,
    CreateBlockEnvironment(BlockEnvironmentKind),
    ToggleInlineFormat(InlineEnvironmentKind),
    ToggleRunUnderline,
    SetInlineFontFamily(String),
    SetParagraphFirstLineIndentChars(u16),
    SetDefaultFontFamily(Option<String>),
    SetDocumentFontFamily {
        target: DocumentFontTarget,
        family: Option<String>,
    },
    EditorKey(EditorKey),
    SetSelection(Option<Selection>),
    UpdateComposition {
        id: u64,
        preedit: String,
    },
    CommitComposition {
        id: u64,
        text: String,
    },
    CancelComposition {
        id: u64,
    },
    SetPageGeometry(PageGeometry),
    BeginCommandLine,
    TypeCommandLine(String),
    BackspaceCommandLine,
    MoveCommandLineCursor(Direction),
    MoveCommandLineSelection(Direction),
    SelectCommandLineCandidate(usize),
    TabCommandLine,
    AcceptCommandLine,
    CancelCommandLine,
    Undo,
    Redo,
}

impl Command {
    pub const fn id(&self) -> &'static str {
        match self {
            Self::InsertText(_) => "text.insert",
            Self::InsertOneCharacterWidthSpace => "text.insert.one_character_width_space",
            Self::InsertTwoCharacterWidthSpace => "text.insert.two_character_width_space",
            Self::PasteText(_) => "clipboard.paste",
            Self::InsertSoftBreak => "text.soft_break",
            Self::InsertParagraphBreak => "text.split_paragraph",
            Self::Delete(_) => "text.delete",
            Self::MoveCaret(_) => "caret.move",
            Self::MoveStructure(Direction::Forward) => "struct.next",
            Self::MoveStructure(Direction::Backward) => "struct.prev",
            Self::ExitBlockEnvironment(Direction::Forward) => "struct.exit_forward",
            Self::ExitBlockEnvironment(Direction::Backward) => "struct.exit_backward",
            Self::RemoveBlockEnvironmentPreservingContent => "struct.remove_preserving_content",
            Self::RemoveInlineEnvironmentPreservingContent(_) => {
                "text.format.remove_environment_preserving_content"
            }
            Self::ExpandStructureSelection => "selection.expand_structure",
            Self::ExitStructure => "struct.exit",
            Self::CreateBlockEnvironment(BlockEnvironmentKind::Section) => "struct.section.create",
            Self::CreateBlockEnvironment(BlockEnvironmentKind::Custom) => "struct.custom.create",
            Self::ToggleInlineFormat(InlineEnvironmentKind::Bold) => "text.format.bold",
            Self::ToggleInlineFormat(InlineEnvironmentKind::Italic) => "text.format.italic",
            Self::ToggleInlineFormat(InlineEnvironmentKind::Font) => "text.format.font",
            Self::ToggleRunUnderline => "text.format.underline",
            Self::SetInlineFontFamily(_) => "text.format.font_family",
            Self::SetParagraphFirstLineIndentChars(_) => "paragraph.first_line_indent.set",
            Self::SetDefaultFontFamily(_) => "document.default_font_family.set",
            Self::SetDocumentFontFamily { .. } => "document.font_family.set",
            Self::EditorKey(EditorKey::Escape) => "interaction.escape",
            Self::EditorKey(EditorKey::Enter) => "interaction.enter",
            Self::EditorKey(EditorKey::Tab) => "interaction.tab",
            Self::EditorKey(EditorKey::ShiftEnter) => "interaction.shift_enter",
            Self::EditorKey(EditorKey::ModBackspace) => "interaction.mod_backspace",
            Self::SetSelection(_) => "selection.set",
            Self::UpdateComposition { .. } => "ime.update",
            Self::CommitComposition { .. } => "ime.commit",
            Self::CancelComposition { .. } => "ime.cancel",
            Self::SetPageGeometry(_) => "page.geometry.set",
            Self::BeginCommandLine => "cmdline.begin",
            Self::TypeCommandLine(_) => "cmdline.type",
            Self::BackspaceCommandLine => "cmdline.backspace",
            Self::MoveCommandLineCursor(_) => "cmdline.move_cursor",
            Self::MoveCommandLineSelection(_) => "cmdline.move_selection",
            Self::SelectCommandLineCandidate(_) => "cmdline.select_candidate",
            Self::TabCommandLine => "cmdline.tab",
            Self::AcceptCommandLine => "cmdline.accept",
            Self::CancelCommandLine => "cmdline.cancel",
            Self::Undo => "history.undo",
            Self::Redo => "history.redo",
        }
    }
}
