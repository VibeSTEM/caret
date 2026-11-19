//! 定义编辑命令及其稳定命令 ID。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{
    BlockEnvironmentKind, DocumentFontTarget, InlineEnvironmentKind, NodeId, PageGeometry,
};

use super::{EnvironmentKind, MathTextPosition, Pos, Selection};
use caret_layout::VisualAffinity;

/// 命令稳定 ID 的单一来源：`Command::id`、候选表、命令行分派与字体选择器共用同一符号，
/// 避免同一 ID 在多个模块各写一份字面量后静默漂移。
pub(crate) mod command_ids {
    pub(crate) const TEXT_SOFT_BREAK: &str = "text.soft_break";
    pub(crate) const TEXT_SPLIT_PARAGRAPH: &str = "text.split_paragraph";
    pub(crate) const TEXT_INSERT_ONE_CHARACTER_WIDTH_SPACE: &str =
        "text.insert.one_character_width_space";
    pub(crate) const TEXT_INSERT_TWO_CHARACTER_WIDTH_SPACE: &str =
        "text.insert.two_character_width_space";
    pub(crate) const INSERT_INLINE_MATH: &str = "insert.inline_math";
    pub(crate) const INSERT_EQUATION: &str = "insert.equation";
    pub(crate) const INSERT_ALIGN: &str = "insert.align";
    pub(crate) const STRUCT_SECTION_CREATE: &str = "struct.section.create";
    pub(crate) const TEXT_FORMAT_BOLD: &str = "text.format.bold";
    pub(crate) const TEXT_FORMAT_ITALIC: &str = "text.format.italic";
    pub(crate) const TEXT_FORMAT_UNDERLINE: &str = "text.format.underline";
    pub(crate) const TEXT_FORMAT_FONT: &str = "text.format.font";
    pub(crate) const TEXT_FORMAT_FONT_FAMILY: &str = "text.format.font_family";
    pub(crate) const PARAGRAPH_FIRST_LINE_INDENT_SET: &str = "paragraph.first_line_indent.set";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Backward,
    Forward,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditorKey {
    Escape,
    Enter,
    Tab,
    ShiftEnter,
    Backspace,
    Backslash,
    Dollar,
    Delete,
    ModBackspace,
    ModC,
    ModV,
    ModZ {
        redo: bool,
    },
    ModSpace,
    LeftArrow {
        candidate: Option<NavigationCandidate>,
        extend: bool,
    },
    RightArrow {
        candidate: Option<NavigationCandidate>,
        extend: bool,
    },
    UpArrow {
        candidate: Option<NavigationCandidate>,
        extend: bool,
        structure: bool,
    },
    DownArrow {
        candidate: Option<NavigationCandidate>,
        extend: bool,
        structure: bool,
    },
    Home {
        candidate: Option<NavigationCandidate>,
        extend: bool,
    },
    End {
        candidate: Option<NavigationCandidate>,
        extend: bool,
    },
}

/// app 从同代布局快照算出的单次导航候选；不持有页面布局模型。
#[derive(Debug, Clone, PartialEq)]
pub enum NavigationCandidate {
    /// 布局中的真实块外侧停靠，保留宿主身份和方向。
    BlockEdge {
        block_id: NodeId,
        side: Direction,
        generation: NavigationGeneration,
    },
    Document {
        focus: Pos,
        visual_affinity: VisualAffinity,
        generation: NavigationGeneration,
    },
    MathText {
        focus: MathTextPosition,
        visual_affinity: VisualAffinity,
        generation: NavigationGeneration,
    },
    MathTextHit {
        hit: crate::PageMathTextHit,
        generation: NavigationGeneration,
    },
    MathTreeHit {
        hit: crate::PageMathStructureHit,
        generation: NavigationGeneration,
    },
}

/// 单次视觉候选来源布局快照的身份与代际。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationGeneration {
    pub session_id: NodeId,
    pub document_generation: u64,
    pub font_generation: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// 将宿主提交的字符按当前 Session 焦点送往命令输入、快捷键前缀或文本编辑。
    TextInput(String),
    InsertText(String),
    InsertOneCharacterWidthSpace,
    InsertTwoCharacterWidthSpace,
    PasteText(String),
    InsertSoftBreak,
    /// 以 LaTeX 源文本在插入点放入一个原子行内公式。
    InsertInlineMath(String),
    /// 进入光标旁的公式（行内公式或相邻显示公式块）编辑。
    EnterMath(Direction),
    /// 设置当前公式文本岛内的 UTF-8 字节选区；布局与命中仍由宿主适配器提供位置。
    SetMathTextSelection {
        anchor: usize,
        focus: usize,
    },
    InsertParagraphBreak,
    Delete(Direction),
    MoveCaret(Direction),
    MoveStructure(Direction),
    ExitBlockEnvironment(Direction),
    RemoveBlockEnvironmentPreservingContent,
    RemoveInlineEnvironmentPreservingContent(NodeId),
    RemoveRunPropertyPreservingContent(EnvironmentKind),
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
            Self::TextInput(_) => "text.input",
            Self::InsertText(_) => "text.insert",
            Self::InsertOneCharacterWidthSpace => {
                command_ids::TEXT_INSERT_ONE_CHARACTER_WIDTH_SPACE
            }
            Self::InsertTwoCharacterWidthSpace => {
                command_ids::TEXT_INSERT_TWO_CHARACTER_WIDTH_SPACE
            }
            Self::PasteText(_) => "clipboard.paste",
            Self::InsertSoftBreak => command_ids::TEXT_SOFT_BREAK,
            Self::InsertInlineMath(_) => command_ids::INSERT_INLINE_MATH,
            Self::EnterMath(_) => "math.enter",
            Self::SetMathTextSelection { .. } => "math.text_selection.set",
            Self::InsertParagraphBreak => command_ids::TEXT_SPLIT_PARAGRAPH,
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
            Self::RemoveRunPropertyPreservingContent(_) => {
                "text.format.remove_run_property_preserving_content"
            }
            Self::ExpandStructureSelection => "selection.expand_structure",
            Self::ExitStructure => "struct.exit",
            Self::CreateBlockEnvironment(BlockEnvironmentKind::Section) => {
                command_ids::STRUCT_SECTION_CREATE
            }
            Self::CreateBlockEnvironment(BlockEnvironmentKind::Custom) => "struct.custom.create",
            Self::ToggleInlineFormat(InlineEnvironmentKind::Bold) => command_ids::TEXT_FORMAT_BOLD,
            Self::ToggleInlineFormat(InlineEnvironmentKind::Italic) => {
                command_ids::TEXT_FORMAT_ITALIC
            }
            Self::ToggleInlineFormat(InlineEnvironmentKind::Font) => command_ids::TEXT_FORMAT_FONT,
            Self::ToggleRunUnderline => command_ids::TEXT_FORMAT_UNDERLINE,
            Self::SetInlineFontFamily(_) => command_ids::TEXT_FORMAT_FONT_FAMILY,
            Self::SetParagraphFirstLineIndentChars(_) => {
                command_ids::PARAGRAPH_FIRST_LINE_INDENT_SET
            }
            Self::SetDefaultFontFamily(_) => "document.default_font_family.set",
            Self::SetDocumentFontFamily { .. } => "document.font_family.set",
            Self::EditorKey(EditorKey::Escape) => "interaction.escape",
            Self::EditorKey(EditorKey::Enter) => "interaction.enter",
            Self::EditorKey(EditorKey::Tab) => "interaction.tab",
            Self::EditorKey(EditorKey::ShiftEnter) => "interaction.shift_enter",
            Self::EditorKey(EditorKey::ModBackspace) => "interaction.mod_backspace",
            Self::EditorKey(EditorKey::LeftArrow { .. }) => "navigation.left",
            Self::EditorKey(EditorKey::RightArrow { .. }) => "navigation.right",
            Self::EditorKey(EditorKey::Backspace) => "text.delete_backward",
            Self::EditorKey(EditorKey::Delete) => "text.delete_forward",
            Self::EditorKey(EditorKey::Backslash) => "cmdline.begin",
            Self::EditorKey(EditorKey::Dollar) => "math.begin_inline",
            Self::EditorKey(EditorKey::ModC) => "clipboard.copy",
            Self::EditorKey(EditorKey::ModV) => "clipboard.paste",
            Self::EditorKey(EditorKey::ModZ { redo: false }) => "history.undo",
            Self::EditorKey(EditorKey::ModZ { redo: true }) => "history.redo",
            Self::EditorKey(EditorKey::ModSpace) => "selection.expand_structure",
            Self::EditorKey(EditorKey::UpArrow {
                structure: true, ..
            }) => "struct.prev",
            Self::EditorKey(EditorKey::DownArrow {
                structure: true, ..
            }) => "struct.next",
            Self::EditorKey(EditorKey::UpArrow { .. }) => "navigation.up",
            Self::EditorKey(EditorKey::DownArrow { .. }) => "navigation.down",
            Self::EditorKey(EditorKey::Home { .. }) => "navigation.home",
            Self::EditorKey(EditorKey::End { .. }) => "navigation.end",
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
