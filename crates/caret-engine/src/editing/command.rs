//! 定义编辑命令及其稳定命令 ID。

use caret_doc::PageGeometry;

use super::Selection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Backward,
    Forward,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    InsertText(String),
    PasteText(String),
    InsertSoftBreak,
    SplitParagraph,
    Delete(Direction),
    MoveCaret(Direction),
    SetSelection(Option<Selection>),
    UpdateComposition { id: u64, preedit: String },
    CommitComposition { id: u64, text: String },
    CancelComposition { id: u64 },
    SetPageGeometry(PageGeometry),
}

impl Command {
    pub const fn id(&self) -> &'static str {
        match self {
            Self::InsertText(_) => "text.insert",
            Self::PasteText(_) => "clipboard.paste",
            Self::InsertSoftBreak => "text.soft_break",
            Self::SplitParagraph => "text.split_paragraph",
            Self::Delete(_) => "text.delete",
            Self::MoveCaret(_) => "caret.move",
            Self::SetSelection(_) => "selection.set",
            Self::UpdateComposition { .. } => "ime.update",
            Self::CommitComposition { .. } => "ime.commit",
            Self::CancelComposition { .. } => "ime.cancel",
            Self::SetPageGeometry(_) => "page.geometry.set",
        }
    }
}
