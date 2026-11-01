//! 编辑会话的命令、位置、文本映射与段落操作模块。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod command;
mod command_line;
mod command_registry;
mod environment;
mod error;
mod flow_map;
mod grapheme;
mod paragraph_ops;
mod position;
mod session;
#[cfg(test)]
mod tests;
mod text_map;

pub use command::{Command, Direction, EditorKey};
pub use command_line::{CommandLine, CommandLineMode};
pub use command_registry::CommandCandidate;
pub use environment::{
    Capabilities, CurrentEnvironmentView, EnvironmentEntry, EnvironmentKind, EnvironmentLevel,
    EnvironmentSource, InlineSpan,
};
pub use error::EditError;
pub use position::{Affinity, InlineEnvironmentRange, Pos, PositionMap, Selection, StoryId};
pub use session::{
    MathEdit, MathEditBranch, MathEditTarget, MathTextFlowPath, MathTextFlowPathStep, Session,
};
pub use text_map::{utf8_offset_to_utf16, utf16_offset_to_utf8};
pub use types::{Effect, InputComposition};

mod types;
