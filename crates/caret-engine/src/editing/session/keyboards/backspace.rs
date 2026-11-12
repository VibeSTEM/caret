//! 管理 Backspace 在命令行、公式、输入法和正文状态中的优先级。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::Session;
use crate::editing::{Command, Direction, EditError, Effect, HostCaretKind, HostCaretPosition};
use caret_doc::NodeId;

impl Session {
    /// 先尊重组合输入与命令行焦点，再清理光标处的空行内环境，最后委托共享领域删除事务。
    pub(super) fn handle_backspace(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Ok(Effect::default());
        }
        if self.command_line.is_some() {
            return self.run(Command::BackspaceCommandLine);
        }
        if let Some(environment_id) = self.empty_inline_environment_before_caret()? {
            return self.run(Command::RemoveInlineEnvironmentPreservingContent(
                environment_id,
            ));
        }
        self.run(Command::Delete(Direction::Backward))
    }

    /// 光标位于空 InlineEnvironment 内部或右侧边界时，返回该环境以先于文本删除清理。
    ///
    /// 非空环境保持普通文本删除：删完最后一个字素后环境变空，下一次 Backspace 才移除
    /// 环境，与空 InlineMath 的先删内容再删对象一致。光标在环境左外侧时不清理，继续
    /// 删除前面的文字，避免把文本删除替换成不可见节点操作。结构选区、块边界停靠和
    /// 公式编辑各有更高的按键优先级，不进入该规则。
    fn empty_inline_environment_before_caret(&mut self) -> Result<Option<NodeId>, EditError> {
        if self.math_edit.is_some()
            || self.has_structure_selection()
            || self.block_boundary_exit().is_some()
            || self.selection.anchor != self.selection.focus
        {
            return Ok(None);
        }
        let host_caret_position = self.host_caret_position.clone();
        let focus = self.selection.focus.clone();
        let map = self.text_map_for(&focus)?;
        if let Some(position) = &host_caret_position {
            match position {
                HostCaretPosition::Inside {
                    host_id,
                    kind: HostCaretKind::Inline,
                    local,
                } if local == &focus && map.is_empty_inline_environment(focus.offset, host_id) => {
                    return Ok(Some(host_id.clone()));
                }
                HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Inline,
                } if map.is_empty_inline_environment(focus.offset, host_id) => {
                    return Ok(Some(host_id.clone()));
                }
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Inline,
                } if map.is_empty_inline_environment(focus.offset, host_id) => {
                    return Ok(None);
                }
                _ => {}
            }
        }
        Ok(map
            .empty_inline_environment_stack_at(focus.offset, focus.affinity)
            .last()
            .and_then(|span| span.id.clone()))
    }
}
