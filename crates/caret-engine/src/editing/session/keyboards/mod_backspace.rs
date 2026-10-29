//! 按平台抽象修饰键与退格键移除当前块环境并保留内容。

use super::super::Session;
use crate::editing::{Command, EditError, Effect};

impl Session {
    /// 移除当前块环境外壳，将标题和正文保留在父级内容流中。
    pub(super) fn handle_mod_backspace(&mut self) -> Result<Effect, EditError> {
        self.run(Command::RemoveBlockEnvironmentPreservingContent)
    }
}
