//! 保存未解析公式源码及其有界的局部撤销历史。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::sync::Arc;

use caret_doc::NodeId;

use super::EditError;

/// 未解析源码只供当前会话编辑，绝不覆盖绑定的已提交源码投影。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::editing::session) struct MathSourceDraft {
    source: Arc<String>,
    id: NodeId,
    revision: u64,
    base_cursor: usize,
    undo: Vec<MathSourceChange>,
    redo: Vec<MathSourceChange>,
    history_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MathSourceChange {
    // 单个 UTF-8 连续差量；记录完整前后片段以验证 undo/redo 的基准仍匹配。
    start: usize,
    removed: String,
    inserted: String,
    cursor_before: usize,
    cursor_after: usize,
}

// 保留最近 256 个操作和最多 8 MiB 的变更片段；超出预算时丢弃最旧的撤销步。
const HISTORY_LIMIT: usize = 256;
const HISTORY_BYTES: usize = 8 * 1024 * 1024;

impl MathSourceDraft {
    pub(super) fn new(source: String, base_cursor: usize) -> Self {
        Self {
            source: Arc::new(source),
            id: NodeId::generate(),
            revision: 0,
            base_cursor,
            undo: Vec::new(),
            redo: Vec::new(),
            history_bytes: 0,
        }
    }

    pub(in crate::editing::session) fn source(&self) -> &str {
        self.source.as_str()
    }

    pub(in crate::editing::session) fn revision(&self) -> u64 {
        self.revision
    }

    pub(in crate::editing::session) fn id(&self) -> &NodeId {
        &self.id
    }

    pub(super) fn base_cursor(&self) -> usize {
        self.base_cursor
    }

    pub(super) fn command_snapshot(&self) -> Self {
        Self {
            source: Arc::clone(&self.source),
            id: self.id.clone(),
            revision: self.revision,
            base_cursor: self.base_cursor,
            undo: Vec::new(),
            redo: Vec::new(),
            history_bytes: 0,
        }
    }

    pub(super) fn record(
        &mut self,
        next_source: String,
        cursor_before: usize,
        cursor_after: usize,
    ) -> Result<(), EditError> {
        let (old_range, new_range) =
            super::changed_source_range(self.source.as_str(), &next_source)
                .ok_or(EditError::InvalidPosition)?;
        let source_changed = old_range.start != old_range.end || new_range.start != new_range.end;
        let next_revision = if source_changed {
            self.revision
                .checked_add(1)
                .ok_or(EditError::InvalidPosition)?
        } else {
            self.revision
        };
        if cursor_after > next_source.len() || !next_source.is_char_boundary(cursor_after) {
            return Err(EditError::InvalidPosition);
        }
        if source_changed {
            let removed = self.source[old_range.clone()].to_owned();
            let inserted = next_source[new_range].to_owned();
            self.history_bytes = self
                .history_bytes
                .saturating_add(removed.len())
                .saturating_add(inserted.len());
            self.undo.push(MathSourceChange {
                start: old_range.start,
                removed,
                inserted,
                cursor_before,
                cursor_after,
            });
            self.redo.clear();
            self.trim_history();
        }
        self.source = Arc::new(next_source);
        self.revision = next_revision;
        Ok(())
    }

    pub(super) fn undo(&mut self) -> Result<Option<usize>, EditError> {
        let Some(change) = self.undo.last() else {
            return Ok(None);
        };
        let end = change
            .start
            .checked_add(change.inserted.len())
            .ok_or(EditError::InvalidPosition)?;
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(EditError::InvalidPosition)?;
        if self.source.get(change.start..end) != Some(change.inserted.as_str()) {
            return Err(EditError::InvalidPosition);
        }
        let change = self.undo.pop().ok_or(EditError::InvalidPosition)?;
        Arc::make_mut(&mut self.source).replace_range(change.start..end, &change.removed);
        self.history_bytes = self
            .history_bytes
            .saturating_sub(change.removed.len())
            .saturating_sub(change.inserted.len());
        let cursor = change.cursor_before;
        self.redo.push(change);
        self.revision = next_revision;
        Ok(Some(cursor))
    }

    pub(super) fn redo(&mut self) -> Result<Option<usize>, EditError> {
        let Some(change) = self.redo.last() else {
            return Ok(None);
        };
        let end = change
            .start
            .checked_add(change.removed.len())
            .ok_or(EditError::InvalidPosition)?;
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(EditError::InvalidPosition)?;
        if self.source.get(change.start..end) != Some(change.removed.as_str()) {
            return Err(EditError::InvalidPosition);
        }
        let change = self.redo.pop().ok_or(EditError::InvalidPosition)?;
        Arc::make_mut(&mut self.source).replace_range(change.start..end, &change.inserted);
        self.history_bytes = self
            .history_bytes
            .saturating_add(change.removed.len())
            .saturating_add(change.inserted.len());
        let cursor = change.cursor_after;
        self.undo.push(change);
        self.trim_history();
        self.revision = next_revision;
        Ok(Some(cursor))
    }

    pub(super) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(super) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn trim_history(&mut self) {
        while self.undo.len() > HISTORY_LIMIT || self.history_bytes > HISTORY_BYTES {
            let Some(discarded) = self.undo.first() else {
                break;
            };
            let bytes = discarded.removed.len() + discarded.inserted.len();
            self.undo.remove(0);
            self.history_bytes = self.history_bytes.saturating_sub(bytes);
        }
    }
}
