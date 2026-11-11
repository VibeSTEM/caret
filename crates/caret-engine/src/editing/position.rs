//! 定义文档位置、边界亲和性、选区及事务位置映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::NodeId;

use super::host_caret::{HostCaretLocalPosition, HostCaretPosition};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StoryId {
    Body,
    EnvironmentTitle(NodeId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Affinity {
    Upstream,
    Downstream,
}

pub(super) fn compare_affinity(left: Affinity, right: Affinity) -> std::cmp::Ordering {
    match (left, right) {
        (Affinity::Upstream, Affinity::Downstream) => std::cmp::Ordering::Less,
        (Affinity::Downstream, Affinity::Upstream) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pos {
    pub story: StoryId,
    pub paragraph_id: NodeId,
    pub offset: usize,
    pub affinity: Affinity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Pos,
    pub focus: Pos,
}

/// 当前会话和文档代际内的一种完整 caret 位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentCaretPosition {
    /// Paragraph/TextMap 中的位置。
    Text(Pos),
    /// Block 或 Inline 宿主中的三态位置。
    Host(HostCaretPosition<HostCaretLocalPosition>),
}

/// 将 caret 位置绑定到产生它的 Session 与文档代际。
///
/// 该值是短期比较/解析凭据，不持久化；事务后须通过位置映射重新取得。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCaretSnapshot {
    pub(super) session_id: NodeId,
    pub(super) document_generation: u64,
    pub(super) position: DocumentCaretPosition,
}

impl SessionCaretSnapshot {
    /// 返回绑定的会话身份。
    pub fn session_id(&self) -> &NodeId {
        &self.session_id
    }

    /// 返回绑定的文档代际。
    pub fn document_generation(&self) -> u64 {
        self.document_generation
    }

    /// 返回该快照中的文本或宿主位置。
    pub fn position(&self) -> &DocumentCaretPosition {
        &self.position
    }
}

/// 当前插入点所在的行内环境范围，使用段落内 UTF-8 半开偏移。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineEnvironmentRange {
    pub environment_id: NodeId,
    pub paragraph_id: NodeId,
    pub story: StoryId,
    pub start_utf8: usize,
    pub end_utf8: usize,
}

/// 将编辑事务中的 Paragraph 位置映射到结果文档坐标。
///
/// 这是纯坐标变换，不携带 Session、文档代际或来源快照，也不证明输入位置有效。
/// 生产者须依据事务前文档构造映射；消费者若要将结果安装为当前 caret，仍须按结果
/// 文档验证 Story、段落、偏移和字素边界。跨文档重用映射不受支持。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionMap {
    Composite(Vec<PositionMap>),
    TextEdit {
        paragraph_id: NodeId,
        start: usize,
        end: usize,
        inserted_len: usize,
        snap_graphemes: bool,
        result_boundaries: Vec<usize>,
    },
    ParagraphSplit {
        source_id: NodeId,
        right_id: NodeId,
        split_offset: usize,
        source_story: StoryId,
        right_story: StoryId,
    },
    ParagraphMerge {
        left_id: NodeId,
        right_id: NodeId,
        left_len: usize,
        result_boundaries: Vec<usize>,
    },
    ParagraphRangeMerge {
        paragraph_id: NodeId,
        end_paragraph_id: NodeId,
        removed_ids: Vec<NodeId>,
        start_offset: usize,
        end_offset: usize,
        snap_graphemes: bool,
        result_boundaries: Vec<usize>,
    },
    StoryChange {
        paragraph_id: NodeId,
        from: StoryId,
        to: StoryId,
    },
}

impl PositionMap {
    /// 应用坐标变换；未匹配映射规则的位置原样返回。
    ///
    /// 本方法不检查输入是否属于映射来源文档，也不校验映射结果；调用方负责校验其
    /// 来源和目标文档契约。
    pub fn map(&self, position: &Pos) -> Pos {
        match self {
            Self::Composite(maps) => maps
                .iter()
                .fold(position.clone(), |position, map| map.map(&position)),
            Self::TextEdit {
                paragraph_id,
                start,
                end,
                inserted_len,
                snap_graphemes,
                result_boundaries,
            } if &position.paragraph_id == paragraph_id => {
                let offset = if position.offset < *start {
                    position.offset
                } else if position.offset == *start && position.affinity == Affinity::Upstream {
                    *start
                } else if position.offset > *end {
                    position.offset - (*end - *start) + *inserted_len
                } else {
                    *start + *inserted_len
                };
                Pos {
                    offset: if *snap_graphemes {
                        snap_to_boundary(offset, position.affinity, result_boundaries)
                    } else {
                        offset
                    },
                    ..position.clone()
                }
            }
            Self::ParagraphSplit {
                source_id,
                right_id,
                split_offset,
                source_story,
                right_story,
            } if &position.paragraph_id == source_id => {
                let into_right = position.offset > *split_offset
                    || (position.offset == *split_offset
                        && position.affinity == Affinity::Downstream);
                if into_right {
                    Pos {
                        story: right_story.clone(),
                        paragraph_id: right_id.clone(),
                        offset: position.offset.saturating_sub(*split_offset),
                        ..position.clone()
                    }
                } else {
                    Pos {
                        story: source_story.clone(),
                        ..position.clone()
                    }
                }
            }
            Self::ParagraphMerge {
                left_id,
                right_id,
                left_len,
                result_boundaries,
            } if &position.paragraph_id == left_id || &position.paragraph_id == right_id => {
                let offset = if &position.paragraph_id == right_id {
                    left_len + position.offset
                } else {
                    position.offset
                };
                Pos {
                    paragraph_id: left_id.clone(),
                    offset: snap_to_boundary(offset, position.affinity, result_boundaries),
                    story: StoryId::Body,
                    ..position.clone()
                }
            }
            Self::ParagraphRangeMerge {
                paragraph_id,
                end_paragraph_id,
                removed_ids,
                start_offset,
                end_offset,
                snap_graphemes,
                result_boundaries,
            } if &position.paragraph_id == paragraph_id
                || removed_ids.contains(&position.paragraph_id) =>
            {
                let offset = if &position.paragraph_id == end_paragraph_id {
                    start_offset + position.offset.saturating_sub(*end_offset)
                } else if &position.paragraph_id == paragraph_id && position.offset <= *start_offset
                {
                    position.offset
                } else {
                    *start_offset
                };
                Pos {
                    paragraph_id: paragraph_id.clone(),
                    story: StoryId::Body,
                    offset: if *snap_graphemes {
                        snap_to_boundary(offset, position.affinity, result_boundaries)
                    } else {
                        offset
                    },
                    ..position.clone()
                }
            }
            Self::StoryChange {
                paragraph_id,
                from,
                to,
            } if &position.paragraph_id == paragraph_id && &position.story == from => Pos {
                story: to.clone(),
                ..position.clone()
            },
            _ => position.clone(),
        }
    }
}

fn snap_to_boundary(offset: usize, affinity: Affinity, boundaries: &[usize]) -> usize {
    match boundaries.binary_search(&offset) {
        Ok(_) => offset,
        Err(index) => match affinity {
            Affinity::Upstream => boundaries
                .get(index.saturating_sub(1))
                .copied()
                .unwrap_or(0),
            Affinity::Downstream => boundaries
                .get(index)
                .copied()
                .or_else(|| boundaries.last().copied())
                .unwrap_or(offset),
        },
    }
}
