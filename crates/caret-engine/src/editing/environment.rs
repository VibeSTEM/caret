//! 按 Inline 与 Block 分层组织文档载体的只读交互视图。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

pub(crate) mod block_adapter;
pub(crate) mod inline_adapter;
#[cfg(test)]
mod tests;

use caret_doc::{BlockEnvironmentKind, InlineEnvironmentKind, NodeId};

use super::StoryId;

/// 编辑器环境适配的宿主层级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvironmentLevel {
    Inline,
    Block,
}

impl EnvironmentLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::Block => "block",
        }
    }
}

/// 当前环境视图条目的真实文档来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvironmentSource {
    InlineEnvironment,
    RunProperty,
    InlineMath,
    BlockEnvironment,
    MathBlock,
}

/// 格式与公式在交互视图中的稳定语义 kind。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvironmentKind {
    Section,
    Custom,
    Bold,
    Italic,
    Underline,
    Font,
    InlineMath,
    Equation,
    Align,
}

/// 环境来源支持的独立能力，不将宿主原子性与内部可编辑性合并。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub unwrap_preserving_content: bool,
    pub editable_interior: bool,
    pub atomic_in_host: bool,
}

impl EnvironmentKind {
    pub const fn from_inline(kind: InlineEnvironmentKind) -> Self {
        inline_adapter::for_inline_environment(kind).kind
    }

    pub const fn from_block(kind: BlockEnvironmentKind) -> Self {
        block_adapter::for_block_environment(kind).kind
    }

    pub const fn level(self) -> EnvironmentLevel {
        match self {
            Self::Bold | Self::Italic | Self::Underline | Self::Font | Self::InlineMath => {
                EnvironmentLevel::Inline
            }
            Self::Section | Self::Custom | Self::Equation | Self::Align => EnvironmentLevel::Block,
        }
    }

    pub const fn capabilities(self) -> Capabilities {
        match self {
            Self::Section | Self::Custom => Capabilities {
                unwrap_preserving_content: true,
                editable_interior: true,
                atomic_in_host: false,
            },
            Self::Bold | Self::Italic | Self::Underline | Self::Font => Capabilities {
                unwrap_preserving_content: true,
                editable_interior: true,
                atomic_in_host: false,
            },
            Self::InlineMath | Self::Equation | Self::Align => Capabilities {
                unwrap_preserving_content: false,
                editable_interior: true,
                atomic_in_host: true,
            },
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Section => "section",
            Self::Custom => "custom",
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Underline => "underline",
            Self::Font => "font",
            Self::InlineMath => "inline_math",
            Self::Equation => "equation",
            Self::Align => "align",
        }
    }
}

/// 只读视图条目；Run 属性派生项没有文档节点 ID。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentEntry {
    pub id: Option<NodeId>,
    pub kind: EnvironmentKind,
    pub source: EnvironmentSource,
    pub inline_range: Option<InlineSpan>,
    pub depth: usize,
}

/// 行内环境或 Run 属性在段落中的逻辑范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineSpan {
    pub paragraph_id: NodeId,
    pub story: StoryId,
    pub start_utf8: usize,
    pub end_utf8: usize,
}

/// 分层语义视图；Inline 与 Block 分开维护候选和最内层选择。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CurrentEnvironmentView {
    inline: Vec<EnvironmentEntry>,
    block: Vec<EnvironmentEntry>,
}

impl CurrentEnvironmentView {
    pub(crate) fn new(inline: Vec<EnvironmentEntry>, block: Vec<EnvironmentEntry>) -> Self {
        Self { inline, block }
    }

    pub fn inline(&self) -> &[EnvironmentEntry] {
        &self.inline
    }

    pub fn block(&self) -> &[EnvironmentEntry] {
        &self.block
    }

    pub fn innermost_inline(&self) -> Option<&EnvironmentEntry> {
        self.inline.last()
    }

    pub fn innermost_block(&self) -> Option<&EnvironmentEntry> {
        self.block.last()
    }

    pub fn innermost_unwrappable_inline(&self) -> Option<&EnvironmentEntry> {
        self.inline
            .iter()
            .rev()
            .find(|entry| entry.kind.capabilities().unwrap_preserving_content)
    }
}
