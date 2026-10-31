//! 编辑会话的命令、位置、文本映射与段落操作模块。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod command;
mod command_line;
mod command_registry;
mod error;
mod paragraph_ops;
mod position;
mod session;
#[cfg(test)]
mod tests;
mod text_map;

pub use command::{Command, Direction, EditorKey, MathDirection};
pub use command_line::{CommandLine, CommandLineMode};
pub use command_registry::CommandCandidate;
pub use error::EditError;
pub use position::{
    Affinity, InlineEnvironmentRange, InlineMathTarget, MathFocus, MathPos, Pos, PositionMap,
    Selection, StoryId,
};
pub use session::{CompositionUpdateCandidate, Session, SessionCheckpoint};
pub use text_map::{utf8_offset_to_utf16, utf16_offset_to_utf8};
pub use types::{Effect, InputComposition};

mod types;
