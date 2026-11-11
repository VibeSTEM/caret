//! 比较结构选区端点、派生块节点覆盖与文本边界。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::{
    Affinity, Direction, DocumentRangeEndpoint, EditError, HostCaretKind, HostCaretLocalPosition,
    HostCaretPosition, Pos, Session, StructureSelection,
};
use crate::editing::paragraph_ops::compare_positions;
use crate::editing::text_map::TextMap;
use caret_doc::{Block, NodeId};
use caret_structure::{
    DocumentNodeEdge, DocumentNodeKind, compare_document_node_edges, find_document_node,
};
use std::cmp::Ordering;

/// 端点外侧边界映射为共享宿主的块外侧停靠状态。
fn block_edge_host(
    block_id: &NodeId,
    side: Direction,
) -> HostCaretPosition<HostCaretLocalPosition> {
    match side {
        Direction::Backward => HostCaretPosition::Before {
            host_id: block_id.clone(),
            kind: HostCaretKind::Block,
        },
        Direction::Forward => HostCaretPosition::After {
            host_id: block_id.clone(),
            kind: HostCaretKind::Block,
        },
    }
}

fn document_edge(side: Direction) -> DocumentNodeEdge {
    match side {
        Direction::Backward => DocumentNodeEdge::Before,
        Direction::Forward => DocumentNodeEdge::After,
    }
}

impl Session {
    /// 比较两个结构选区端点的文档顺序。
    ///
    /// 文本端点使用段落顺序与亲和性；块外侧边界使用物理节点边界。端点不属于
    /// 可解析的 Section/Custom/Equation/Align 块或无法唯一排序时返回 `InvalidPosition`。
    pub(crate) fn compare_range_endpoints(
        &self,
        left: &DocumentRangeEndpoint,
        right: &DocumentRangeEndpoint,
    ) -> Result<Ordering, EditError> {
        match (left, right) {
            (DocumentRangeEndpoint::Text(left), DocumentRangeEndpoint::Text(right)) => {
                compare_positions(&self.document.body, &self.paragraph_index, left, right)
            }
            (
                DocumentRangeEndpoint::BlockEdge {
                    block_id,
                    side: block_side,
                },
                DocumentRangeEndpoint::Text(text),
            ) => self
                .compare_text_with_block_host_edge(
                    block_id,
                    &block_edge_host(block_id, *block_side),
                    text,
                )
                .map(Ordering::reverse),
            (
                DocumentRangeEndpoint::Text(text),
                DocumentRangeEndpoint::BlockEdge {
                    block_id,
                    side: block_side,
                },
            ) => self.compare_text_with_block_host_edge(
                block_id,
                &block_edge_host(block_id, *block_side),
                text,
            ),
            (
                DocumentRangeEndpoint::BlockEdge {
                    block_id: left_id,
                    side: left_side,
                },
                DocumentRangeEndpoint::BlockEdge {
                    block_id: right_id,
                    side: right_side,
                },
            ) => {
                self.validate_range_block_edge(left_id)?;
                self.validate_range_block_edge(right_id)?;
                compare_document_node_edges(
                    &self.document,
                    left_id,
                    document_edge(*left_side),
                    right_id,
                    document_edge(*right_side),
                )
                .ok_or(EditError::InvalidPosition)
            }
        }
    }

    /// 返回按文档顺序排列的结构选区两端；没有结构选区或端点不可比较时返回错误。
    pub(crate) fn ordered_structure_selection(
        &self,
    ) -> Result<(&DocumentRangeEndpoint, &DocumentRangeEndpoint), EditError> {
        let selection = self
            .structure_selection
            .as_ref()
            .ok_or(EditError::InvalidPosition)?;
        match self.compare_range_endpoints(&selection.anchor, &selection.focus)? {
            Ordering::Greater => Ok((&selection.focus, &selection.anchor)),
            _ => Ok((&selection.anchor, &selection.focus)),
        }
    }

    /// 返回结构选区覆盖的最外层块环境与公式块，均按文档顺序排列。
    ///
    /// 块的完整外侧边界都落在范围内才算覆盖；被覆盖环境的后代不单独列出，
    /// 因为删除父环境已经包含其全部子项。
    pub(in crate::editing::session) fn covered_block_nodes(
        &self,
        start: &DocumentRangeEndpoint,
        end: &DocumentRangeEndpoint,
    ) -> Result<(Vec<NodeId>, Vec<NodeId>), EditError> {
        if self.compare_range_endpoints(start, end)?.is_gt() {
            return Err(EditError::InvalidPosition);
        }
        let mut environments = Vec::new();
        let mut math_blocks = Vec::new();
        self.collect_covered_block_nodes(
            &self.document.body,
            start,
            end,
            &mut environments,
            &mut math_blocks,
        )?;
        Ok((environments, math_blocks))
    }

    /// 按段落顺序规范化一对文本端点后派生覆盖的块环境与公式块。
    pub(in crate::editing::session) fn covered_block_nodes_for_text_pair(
        &self,
        anchor: &Pos,
        focus: &Pos,
    ) -> Result<(Vec<NodeId>, Vec<NodeId>), EditError> {
        let (start, end) = if compare_positions(
            &self.document.body,
            &self.paragraph_index,
            anchor,
            focus,
        )?
        .is_gt()
        {
            (
                DocumentRangeEndpoint::Text(focus.clone()),
                DocumentRangeEndpoint::Text(anchor.clone()),
            )
        } else {
            (
                DocumentRangeEndpoint::Text(anchor.clone()),
                DocumentRangeEndpoint::Text(focus.clone()),
            )
        };
        self.covered_block_nodes(&start, &end)
    }

    /// 将结构选区端点投影为可排版、可命中的正文文本边界。
    ///
    /// 文本端点保持不变；块外侧边界映射到该侧最近的段落位置。没有对应段落时
    /// 使用文档顺序上最近的段落，供布局在不合成段落的前提下给出稳定范围。
    /// 端点之间只有块对象而没有可截断文字时，两边界折叠到起点，保证返回值按
    /// 文档顺序排列；对象本身由结构选区的块节点列表投影，不伪造文本切口。
    pub fn structure_selection_text_bounds(&self) -> Option<(Pos, Pos)> {
        let (start, end) = self.ordered_structure_selection().ok()?;
        let start_text = self.text_endpoint_at_or_after(start)?;
        let end_text = self.text_endpoint_at_or_before(end)?;
        let ordered = !self
            .compare_range_endpoints(
                &DocumentRangeEndpoint::Text(start_text.clone()),
                &DocumentRangeEndpoint::Text(end_text.clone()),
            )
            .ok()?
            .is_gt();
        if ordered {
            Some((start_text.clone(), end_text))
        } else {
            Some((start_text.clone(), start_text))
        }
    }

    /// 结构选区的文本锚点：文本端点原样返回，块外侧边界投影到该侧最近的文本位置。
    ///
    /// 供 Home/End 与纵向候选等只接受文本锚点的扩展路径使用；投影保留边界侧向，
    /// 并校验结果确实位于边界所求一侧，使扩展后的文本范围仍覆盖原块对象。
    pub(in crate::editing::session) fn structure_selection_text_anchor(&self) -> Option<Pos> {
        let selection = self.structure_selection.as_ref()?;
        match &selection.anchor {
            DocumentRangeEndpoint::Text(position) => Some(position.clone()),
            endpoint @ DocumentRangeEndpoint::BlockEdge { side, .. } => {
                let projected = self.text_endpoint_clamped(endpoint)?;
                let ordering = self
                    .compare_range_endpoints(
                        &DocumentRangeEndpoint::Text(projected.clone()),
                        endpoint,
                    )
                    .ok()?;
                // 兜底投影可能落到边界另一侧的最近段落；只有仍位于所求侧时才可用，
                // 否则扩展范围会把原块对象移出覆盖。
                let on_requested_side = match side {
                    Direction::Backward => !ordering.is_gt(),
                    Direction::Forward => !ordering.is_lt(),
                };
                on_requested_side.then_some(projected)
            }
        }
    }

    /// 将单个端点映射到同侧最近的段落位置；文本端点原样返回。
    pub(in crate::editing::session) fn text_endpoint_clamped(
        &self,
        endpoint: &DocumentRangeEndpoint,
    ) -> Option<Pos> {
        match endpoint {
            DocumentRangeEndpoint::Text(position) => Some(position.clone()),
            DocumentRangeEndpoint::BlockEdge {
                side: Direction::Forward,
                ..
            } => self.text_endpoint_at_or_after(endpoint),
            DocumentRangeEndpoint::BlockEdge {
                side: Direction::Backward,
                ..
            } => self.text_endpoint_at_or_before(endpoint),
        }
    }

    fn validate_range_block_edge(&self, block_id: &NodeId) -> Result<(), EditError> {
        let location =
            find_document_node(&self.document, block_id).ok_or(EditError::InvalidPosition)?;
        if location.kind.is_range_block() {
            Ok(())
        } else {
            Err(EditError::InvalidPosition)
        }
    }

    /// 判断端点是否位于指定环境内部；环境自身的外侧边界不算内部。
    pub(in crate::editing::session) fn endpoint_is_inside_environment(
        &self,
        endpoint: &DocumentRangeEndpoint,
        environment_id: &NodeId,
    ) -> bool {
        let node_id = match endpoint {
            DocumentRangeEndpoint::Text(position) => &position.paragraph_id,
            DocumentRangeEndpoint::BlockEdge { block_id, .. } => block_id,
        };
        let Some(location) = find_document_node(&self.document, node_id) else {
            return false;
        };
        if matches!(endpoint, DocumentRangeEndpoint::BlockEdge { .. })
            && location.kind == DocumentNodeKind::Environment
            && &location.id == environment_id
        {
            return false;
        }
        location
            .parent_environment_ids
            .iter()
            .any(|id| id == environment_id)
    }

    /// 把环境范围投影的端点扩展到整个环境外侧边界。
    ///
    /// 端点落在环境内部时，按锚点/焦点方向扩展到对应环境的 Before/After 边界，
    /// 使结构选区端点与派生出的环境列表描述同一范围（两个消费者不再各自解释）。
    pub(in crate::editing::session) fn normalized_environment_range(
        &self,
        anchor: &Pos,
        focus: &Pos,
        environments: &[NodeId],
    ) -> Result<StructureSelection, EditError> {
        let anchor_is_start =
            !compare_positions(&self.document.body, &self.paragraph_index, anchor, focus)?.is_gt();
        let mut anchor_endpoint = DocumentRangeEndpoint::Text(anchor.clone());
        let mut focus_endpoint = DocumentRangeEndpoint::Text(focus.clone());
        for environment_id in environments {
            if self.endpoint_is_inside_environment(&anchor_endpoint, environment_id) {
                anchor_endpoint = DocumentRangeEndpoint::BlockEdge {
                    block_id: environment_id.clone(),
                    side: if anchor_is_start {
                        Direction::Backward
                    } else {
                        Direction::Forward
                    },
                };
            }
            if self.endpoint_is_inside_environment(&focus_endpoint, environment_id) {
                focus_endpoint = DocumentRangeEndpoint::BlockEdge {
                    block_id: environment_id.clone(),
                    side: if anchor_is_start {
                        Direction::Forward
                    } else {
                        Direction::Backward
                    },
                };
            }
        }
        Ok(StructureSelection {
            anchor: anchor_endpoint,
            focus: focus_endpoint,
        })
    }

    /// 返回单个块的文档级范围单元：稳定 ID 与外侧起止端点。
    ///
    /// 块环境与 Equation/Align 以块外侧边界为端点；Paragraph 使用真实文本首尾。
    /// ExplicitBreak/Opaque 没有稳定身份或可编辑位置，不构成范围单元。
    pub(in crate::editing::session) fn block_range_unit(
        &self,
        block: &Block,
    ) -> Option<(NodeId, DocumentRangeEndpoint, DocumentRangeEndpoint)> {
        match block {
            Block::Paragraph(paragraph) => {
                let story = self.story_for_paragraph(&paragraph.id)?;
                let start = Pos {
                    story,
                    paragraph_id: paragraph.id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                };
                let text_len = TextMap::new(self.paragraph_for(&start).ok()?)
                    .ok()?
                    .text
                    .len();
                let end = Pos {
                    offset: text_len,
                    affinity: Affinity::Upstream,
                    ..start.clone()
                };
                Some((
                    paragraph.id.clone(),
                    DocumentRangeEndpoint::Text(start),
                    DocumentRangeEndpoint::Text(end),
                ))
            }
            Block::Environment(environment) => Some(range_block_unit(&environment.id)),
            Block::Equation(equation) => Some(range_block_unit(&equation.id)),
            Block::Align(align) => Some(range_block_unit(&align.id)),
            Block::ExplicitBreak(_) | Block::Opaque(_) => None,
        }
    }

    fn block_unit_is_covered(
        &self,
        start: &DocumentRangeEndpoint,
        end: &DocumentRangeEndpoint,
        unit: &(NodeId, DocumentRangeEndpoint, DocumentRangeEndpoint),
    ) -> Result<bool, EditError> {
        Ok(self.compare_range_endpoints(start, &unit.1)?.is_le()
            && self.compare_range_endpoints(end, &unit.2)?.is_ge())
    }

    fn collect_covered_block_nodes(
        &self,
        blocks: &[Block],
        start: &DocumentRangeEndpoint,
        end: &DocumentRangeEndpoint,
        environments: &mut Vec<NodeId>,
        math_blocks: &mut Vec<NodeId>,
    ) -> Result<(), EditError> {
        for block in blocks {
            match block {
                Block::Environment(environment) => {
                    let Some(unit) = self.block_range_unit(block) else {
                        continue;
                    };
                    if self.block_unit_is_covered(start, end, &unit)? {
                        // 覆盖父环境已包含全部后代，不重复列出其内部公式块。
                        environments.push(unit.0);
                        continue;
                    }
                    self.collect_covered_block_nodes(
                        &environment.children,
                        start,
                        end,
                        environments,
                        math_blocks,
                    )?;
                }
                Block::Equation(_) | Block::Align(_) => {
                    let Some(unit) = self.block_range_unit(block) else {
                        continue;
                    };
                    if self.block_unit_is_covered(start, end, &unit)? {
                        math_blocks.push(unit.0);
                    }
                }
                Block::Paragraph(_) | Block::ExplicitBreak(_) | Block::Opaque(_) => {}
            }
        }
        Ok(())
    }

    /// 映射到边界同侧最近的段落起点；没有时退回文档顺序上最近的段落。
    fn text_endpoint_at_or_after(&self, endpoint: &DocumentRangeEndpoint) -> Option<Pos> {
        if let DocumentRangeEndpoint::Text(position) = endpoint {
            return Some(position.clone());
        }
        let length = self.structure_index.navigation_paragraphs().len();
        for ordinal in 0..length {
            let Some(position) = self.paragraph_edge_position(ordinal, false) else {
                continue;
            };
            if self
                .compare_range_endpoints(&DocumentRangeEndpoint::Text(position.clone()), endpoint)
                .ok()?
                .is_ge()
            {
                return Some(position);
            }
        }
        length
            .checked_sub(1)
            .and_then(|ordinal| self.paragraph_edge_position(ordinal, true))
    }

    /// 映射到边界同侧最近的段落终点；没有时退回文档顺序上最近的段落。
    fn text_endpoint_at_or_before(&self, endpoint: &DocumentRangeEndpoint) -> Option<Pos> {
        if let DocumentRangeEndpoint::Text(position) = endpoint {
            return Some(position.clone());
        }
        let length = self.structure_index.navigation_paragraphs().len();
        for ordinal in (0..length).rev() {
            let Some(position) = self.paragraph_edge_position(ordinal, true) else {
                continue;
            };
            if self
                .compare_range_endpoints(&DocumentRangeEndpoint::Text(position.clone()), endpoint)
                .ok()?
                .is_le()
            {
                return Some(position);
            }
        }
        self.paragraph_edge_position(0, false)
    }

    fn paragraph_edge_position(&self, ordinal: usize, at_end: bool) -> Option<Pos> {
        let paragraph_id = self.structure_index.paragraph_id_at(ordinal)?;
        let location = self.paragraph_index.get(paragraph_id)?;
        let paragraph =
            crate::editing::paragraph_ops::paragraph_at_location(&self.document.body, location)?;
        let offset = if at_end {
            TextMap::new(paragraph).ok()?.text.len()
        } else {
            0
        };
        Some(Pos {
            story: location.story.clone(),
            paragraph_id: paragraph_id.clone(),
            offset,
            affinity: if at_end {
                Affinity::Upstream
            } else {
                Affinity::Downstream
            },
        })
    }
}

/// 构造块范围单元：块外侧前/后边界与稳定 ID。
fn range_block_unit(block_id: &NodeId) -> (NodeId, DocumentRangeEndpoint, DocumentRangeEndpoint) {
    let before = DocumentRangeEndpoint::BlockEdge {
        block_id: block_id.clone(),
        side: Direction::Backward,
    };
    let after = DocumentRangeEndpoint::BlockEdge {
        block_id: block_id.clone(),
        side: Direction::Forward,
    };
    (block_id.clone(), before, after)
}
