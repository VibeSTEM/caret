//! 编辑会话的命令、位置、文本映射与段落操作模块。

mod command;
mod error;
mod paragraph_ops;
mod position;
mod session;
#[cfg(test)]
mod tests;
mod text_map;

pub use command::{Command, Direction};
pub use error::EditError;
pub use position::{Affinity, Pos, PositionMap, Selection, StoryId};
pub use session::Session;
pub use text_map::{utf8_offset_to_utf16, utf16_offset_to_utf8};
pub use types::{Effect, InputComposition};

mod types;
