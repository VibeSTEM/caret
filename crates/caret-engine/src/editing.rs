//! 编辑会话的命令、位置、文本映射与段落操作模块。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod command;
mod command_line;
mod command_registry;
mod environment;
mod error;
mod flow_edit;
mod flow_format;
mod flow_map;
mod grapheme;
mod host_caret;
mod navigation;
mod paragraph_ops;
mod position;
mod session;
#[cfg(test)]
mod tests;
mod text_map;

pub use command::{Command, Direction, EditorKey, NavigationCandidate, NavigationGeneration};
pub use command_line::{CommandLine, CommandLineMode};
pub use command_registry::CommandCandidate;
pub use environment::{
    Capabilities, CurrentEnvironmentView, EnvironmentEntry, EnvironmentKind, EnvironmentLevel,
    EnvironmentSource, InlineSpan,
};
pub use error::EditError;
pub use host_caret::{
    HostCaretKind, HostCaretLocalPosition, HostCaretPosition, HostCaretTransition,
    MathCaretCoordinate, MathCaretLocalPosition,
};
pub use navigation::NavigationIntent;
pub use position::{
    Affinity, DocumentCaretPosition, InlineEnvironmentRange, Pos, PositionMap, Selection,
    SessionCaretSnapshot, StoryId,
};
pub use session::{
    CompositionRange, CompositionUpdateCandidate, MathEdit, MathEditBranch, MathEditTarget,
    MathPositionAffinity, MathPositionHost, MathSequencePath, MathSequenceStep, MathTextFlowPath,
    MathTextFlowPathStep, MathTextInputContext, MathTextPosition, MathTreePosition,
    MathTreePositionError, Session, SessionCheckpoint,
};
pub use text_map::{utf8_offset_to_utf16, utf16_offset_to_utf8};
pub use types::{ClipboardAction, Effect, InputComposition};

mod types;
