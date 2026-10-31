//! 定义文档位置、边界亲和性、选区及事务位置映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{MathNode, MathNodeKind, MathTree, NodeId};
use unicode_segmentation::UnicodeSegmentation;

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

/// 公式内部位置。路径从公式根节点开始，以稳定数学节点 ID 标识；偏移是可编辑文本的
/// UTF-8 字素边界、原子节点的 0/1 边缘，或结构节点的子节点边界索引。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathPos {
    pub path: Vec<NodeId>,
    pub offset: usize,
}

/// Session 当前编辑的行内公式及公式内插入点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathFocus {
    pub paragraph_id: NodeId,
    pub inline_math_id: NodeId,
    pub position: MathPos,
}

impl MathPos {
    /// 在模型已通过 `Document::validate` 的前提下解析地址。
    ///
    /// 该方法检查路径、父子关系、路径上的重复子 ID 和节点内偏移；不映射编辑后的位置，
    /// 也不归一化线性结构中可能等价的边界表示。
    pub fn resolve<'tree>(&self, tree: &'tree MathTree) -> Option<&'tree MathNode> {
        let (root, descendants) = self.path.split_first()?;
        if root != &tree.root.id {
            return None;
        }
        let mut node = &tree.root;
        for id in descendants {
            let mut matches = node.children.iter().filter(|child| &child.id == id);
            let child = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            node = child;
        }
        math_node_offset_is_valid(node, self.offset).then_some(node)
    }
}

fn math_node_offset_is_valid(node: &MathNode, offset: usize) -> bool {
    match node.kind {
        MathNodeKind::Symbol
            if node.attributes.get("atomClass").map(String::as_str) == Some("ord") =>
        {
            text_offset_is_grapheme_boundary(node.text.as_deref(), offset)
        }
        MathNodeKind::TextContent => text_offset_is_grapheme_boundary(node.text.as_deref(), offset),
        MathNodeKind::Placeholder => offset == 0,
        MathNodeKind::Symbol
        | MathNodeKind::Operator
        | MathNodeKind::TextSymbol
        | MathNodeKind::SizedDelimiter
        | MathNodeKind::TextSpace
        | MathNodeKind::Space
        | MathNodeKind::Strut
        | MathNodeKind::Hline => offset <= 1,
        _ => offset <= node.children.len(),
    }
}

fn text_offset_is_grapheme_boundary(text: Option<&str>, offset: usize) -> bool {
    text.is_some_and(|text| {
        text.grapheme_indices(true)
            .map(|(index, _)| index)
            .chain(std::iter::once(text.len()))
            .any(|boundary| boundary == offset)
    })
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
