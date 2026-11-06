//! 为同步事务失败保留可精确恢复的编辑会话状态。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::Session;

/// 不暴露内部字段的完整会话快照，仅供短期可失败同步事务回滚。
///
/// 快照会复制文档、索引、缓存与完整编辑历史。IME 提交目前使用它保护提交后布局失败时的
/// 状态原子性；其成本随文档和历史规模增长，尚无代表性性能测量，不应用于逐字符输入或热循环。
#[derive(Debug)]
pub struct SessionCheckpoint {
    session: Session,
}

impl SessionCheckpoint {
    /// 消耗回滚点并取回捕获时的会话。
    pub fn into_session(self) -> Session {
        self.session
    }
}

impl Session {
    /// 复制完整会话状态，包括身份、代际、编辑历史和各类缓存。
    pub fn checkpoint(&self) -> SessionCheckpoint {
        SessionCheckpoint {
            session: self.clone_preserving_identity(),
        }
    }

    /// 消耗回滚点并精确恢复捕获时的会话状态。
    pub fn restore_checkpoint(&mut self, checkpoint: SessionCheckpoint) {
        *self = checkpoint.into_session();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editing::{Command, Direction};
    use caret_doc::{Block, Document};

    #[test]
    fn checkpoint_restores_session_identity_revision_history_selection_and_math_edit() {
        let mut session = Session::new(Document::blank())
            .unwrap_or_else(|error| panic!("session creation failed: {error}"));
        session
            .run(Command::InsertInlineMath(r"\frac{1}{\text{ab}}".to_owned()))
            .unwrap_or_else(|error| panic!("formula insertion failed: {error}"));
        session
            .run(Command::EnterMath(Direction::Backward))
            .unwrap_or_else(|error| panic!("entering formula failed: {error}"));
        for _ in 0..64 {
            if matches!(
                session.math_edit().map(|edit| &edit.branch),
                Some(crate::editing::MathEditBranch::Text { .. })
            ) {
                break;
            }
            session
                .run(Command::MoveCaret(Direction::Backward))
                .unwrap_or_else(|error| panic!("moving in formula failed: {error}"));
        }
        assert!(session.math_text_input_context().is_some());

        let session_id = session.session_id().clone();
        let generation = session.document_generation();
        let history_state_id = session.history_state_id();
        let selection = session.selection().clone();
        let math_context = session.math_text_input_context();
        let checkpoint = session.checkpoint();
        session
            .run(Command::InsertText("q".to_owned()))
            .unwrap_or_else(|error| panic!("mutating after checkpoint failed: {error}"));
        assert_ne!(session.document_generation(), generation);

        session.restore_checkpoint(checkpoint);

        assert_eq!(session.session_id(), &session_id);
        assert_eq!(session.document_generation(), generation);
        assert_eq!(session.history_state_id(), history_state_id);
        assert_eq!(session.selection(), &selection);
        assert_eq!(session.math_text_input_context(), math_context);
        assert!(session.can_undo());
        session
            .run(Command::Undo)
            .unwrap_or_else(|error| panic!("undo after checkpoint restore failed: {error}"));
        assert!(matches!(
            session.document().body.first(),
            Some(Block::Paragraph(paragraph)) if paragraph.inlines.is_empty()
        ));
    }
}
