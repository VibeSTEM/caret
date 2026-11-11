//! 校验当前会话中的位置快照并比较可证明的文档顺序。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod host;
mod math;
#[cfg(test)]
mod tests;

use super::{DocumentCaretPosition, EditError, Session, SessionCaretSnapshot};
use crate::editing::paragraph_ops::compare_positions;
use std::cmp::Ordering;

impl Session {
    /// 比较同一会话、同一文档代际内的两个可解析 caret 快照。
    ///
    /// 成功时返回文档顺序；比较不修改会话。快照修订过期、宿主或局部坐标失效，
    /// 或两种坐标间缺少唯一映射时返回 `InvalidPosition`；Story 错配或段落 ID 无效
    /// 可能分别返回 `WrongStory` 或 `UnknownParagraph`。
    pub fn compare_caret_snapshots(
        &self,
        left: &SessionCaretSnapshot,
        right: &SessionCaretSnapshot,
    ) -> Result<Ordering, EditError> {
        self.validate_caret_snapshot_revision(left)?;
        self.validate_caret_snapshot_revision(right)?;
        match (&left.position, &right.position) {
            (DocumentCaretPosition::Text(left), DocumentCaretPosition::Text(right)) => {
                compare_positions(&self.document.body, &self.paragraph_index, left, right)
            }
            (DocumentCaretPosition::Text(position), DocumentCaretPosition::Host(host))
            | (DocumentCaretPosition::Host(host), DocumentCaretPosition::Text(position)) => {
                let (host_order, reversed) = match &left.position {
                    DocumentCaretPosition::Host(_) => (host, true),
                    DocumentCaretPosition::Text(_) => (host, false),
                };
                let order = self.compare_text_with_host(position, host_order)?;
                Ok(if reversed { order.reverse() } else { order })
            }
            (DocumentCaretPosition::Host(left), DocumentCaretPosition::Host(right)) => {
                self.compare_host_carets(left, right)
            }
        }
    }
}
