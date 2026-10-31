//! 统一的环境模型：块环境、行内格式环境与公式共用的种类、能力与当前环境栈。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#[cfg(test)]
mod tests;

use caret_doc::{BlockEnvironmentKind, InlineEnvironmentKind, NodeId};

use super::StoryId;

/// 环境所处的排版层级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvironmentLevel {
    /// 段落内部，随文字排版。
    Inline,
    /// 独立占据一个或多个块。
    Block,
}

impl EnvironmentLevel {
    /// 返回 DTO 中使用的稳定名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::Block => "block",
        }
    }
}

/// 编辑器认识的全部环境种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvironmentKind {
    Section,
    Custom,
    Bold,
    Italic,
    Font,
    InlineMath,
    Equation,
    Align,
}

/// 环境种类声明的编辑能力；命令据此决定是否可执行，而非逐种类硬编码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Mod+Backspace 可解除环境并保留其内容。
    pub unwrap_preserving_content: bool,
    /// 光标可进入环境内部按字符编辑。
    pub editable_text: bool,
    /// 环境作为不可拆分的整体参与光标移动和删除。
    pub atomic: bool,
}

impl EnvironmentKind {
    /// 由行内格式环境种类映射。
    pub const fn from_inline(kind: InlineEnvironmentKind) -> Self {
        match kind {
            InlineEnvironmentKind::Bold => Self::Bold,
            InlineEnvironmentKind::Italic => Self::Italic,
            InlineEnvironmentKind::Font => Self::Font,
        }
    }

    /// 由块环境种类映射。
    pub const fn from_block(kind: BlockEnvironmentKind) -> Self {
        match kind {
            BlockEnvironmentKind::Section => Self::Section,
            BlockEnvironmentKind::Custom => Self::Custom,
        }
    }

    /// 环境所处层级。
    pub const fn level(self) -> EnvironmentLevel {
        match self {
            Self::Bold | Self::Italic | Self::Font | Self::InlineMath => EnvironmentLevel::Inline,
            Self::Section | Self::Custom | Self::Equation | Self::Align => EnvironmentLevel::Block,
        }
    }

    /// 环境种类的编辑能力。
    pub const fn capabilities(self) -> Capabilities {
        match self {
            Self::Section | Self::Custom | Self::Bold | Self::Italic | Self::Font => Capabilities {
                unwrap_preserving_content: true,
                editable_text: true,
                atomic: false,
            },
            Self::InlineMath | Self::Equation | Self::Align => Capabilities {
                unwrap_preserving_content: false,
                editable_text: false,
                atomic: true,
            },
        }
    }

    /// 返回 DTO 中使用的稳定名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Section => "section",
            Self::Custom => "custom",
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Font => "font",
            Self::InlineMath => "inline_math",
            Self::Equation => "equation",
            Self::Align => "align",
        }
    }
}

/// 当前环境栈中的一项。行内环境带有所在段落与 UTF-8 范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentEnvironment {
    pub id: NodeId,
    pub kind: EnvironmentKind,
    pub inline_range: Option<InlineSpan>,
}

/// 行内环境在段落中的位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineSpan {
    pub paragraph_id: NodeId,
    pub story: StoryId,
    pub start_utf8: usize,
    pub end_utf8: usize,
}

/// 焦点所处的全部环境，由外到内排列：先块环境，后行内环境。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CurrentEnvironmentStack {
    entries: Vec<CurrentEnvironment>,
}

impl CurrentEnvironmentStack {
    pub(crate) fn new(entries: Vec<CurrentEnvironment>) -> Self {
        Self { entries }
    }

    /// 由外到内的全部环境。
    pub fn entries(&self) -> &[CurrentEnvironment] {
        &self.entries
    }

    /// 最内层环境。
    pub fn innermost(&self) -> Option<&CurrentEnvironment> {
        self.entries.last()
    }

    /// 最内层可“解除并保留内容”的环境。
    pub fn innermost_unwrappable(&self) -> Option<&CurrentEnvironment> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.kind.capabilities().unwrap_preserving_content)
    }
}
