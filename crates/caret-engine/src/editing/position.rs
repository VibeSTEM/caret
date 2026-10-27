//! 定义文档位置、边界亲和性、选区及事务位置映射。

use caret_doc::NodeId;

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
}

impl PositionMap {
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
                } else if position.offset > *end {
                    position.offset - (*end - *start) + *inserted_len
                } else if start == end
                    && position.offset == *start
                    && position.affinity == Affinity::Upstream
                {
                    *start
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
