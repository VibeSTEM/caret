//! 校验当前会话中的位置快照、比较文档顺序并共享块宿主落点策略。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod host;
mod math;
mod range;
#[cfg(test)]
mod tests;

use super::{
    Affinity, Direction, DocumentCaretPosition, DocumentRangeEndpoint, EditError,
    HostCaretPosition, Pos, Session, SessionCaretSnapshot,
};
use crate::VisualAffinity;
use crate::editing::paragraph_ops::compare_positions;
use caret_doc::NodeId;
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

    /// 把块公式等复杂宿主停靠到其外侧边界：优先同方向相邻段落，否则同侧最近文本边界。
    ///
    /// 返回落点 TextMap 位置和共享外后/外前停靠状态；任何验证失败都返回
    /// `InvalidPosition` 且不改变会话。
    pub(in crate::editing::session) fn park_beside_block_host(
        &mut self,
        block_id: &NodeId,
        direction: Direction,
    ) -> Result<(Pos, HostCaretPosition<Pos>), EditError> {
        let endpoint = DocumentRangeEndpoint::BlockEdge {
            block_id: block_id.clone(),
            side: direction,
        };
        let caret = match self.paragraph_beside_block(block_id, direction)? {
            Some(position) => position,
            None => self
                .text_endpoint_clamped(&endpoint)
                .ok_or(EditError::InvalidPosition)?,
        };
        self.text_map_for(&caret)?.validate_pos(&caret)?;
        Ok((
            caret,
            HostCaretPosition::outside(block_id.clone(), direction),
        ))
    }
}

/// 将文本逻辑亲和性投影为同一侧的可视亲和性。
pub(in crate::editing::session) fn visual_affinity_for(affinity: Affinity) -> VisualAffinity {
    match affinity {
        Affinity::Upstream => VisualAffinity::Upstream,
        Affinity::Downstream => VisualAffinity::Downstream,
    }
}

/// 将块外侧边界方向投影为同侧的可视亲和性。
pub(in crate::editing::session) fn visual_affinity_for_block_edge(
    direction: Direction,
) -> VisualAffinity {
    match direction {
        Direction::Forward => VisualAffinity::Downstream,
        Direction::Backward => VisualAffinity::Upstream,
    }
}
