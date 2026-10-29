//! 从文档块树派生稳定 ID 结构索引和段落顺序范围。

use caret_doc::{Block, Document, NodeId};
use std::{collections::HashMap, ops::Range};

use crate::atoms::{BlockEnvironmentAtom, ParagraphAtom};

/// 以段落 ID 和 UTF-8 字节偏移表示的结构查询位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructurePosition {
    pub paragraph_id: NodeId,
    pub offset_utf8: usize,
}

/// 结构节点在文档中占据的半开段落序号范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureRange {
    pub paragraph_ordinals: Range<usize>,
}

/// 可导航结构节点类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureKind {
    Paragraph,
    Heading { level: u8 },
    Section,
    Custom,
}

/// 面向通用界面展示的结构形态，不暴露具体结构语义名称。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureClass {
    Paragraph,
    SingleLine,
    MultiLine,
    Table,
    Figure,
}

impl StructureKind {
    /// 将具体结构语义归并到通用结构形态，而非计算实际排版行数。
    pub const fn class(self) -> StructureClass {
        match self {
            Self::Paragraph => StructureClass::Paragraph,
            Self::Heading { .. } => StructureClass::SingleLine,
            Self::Section | Self::Custom => StructureClass::MultiLine,
        }
    }

    /// 是否为具有标题和子项的块环境。
    pub const fn is_block_environment(self) -> bool {
        matches!(self, Self::Section | Self::Custom)
    }
}

/// 索引中的一个结构节点，父链和节点身份均使用稳定 ID。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedStructure {
    pub id: NodeId,
    pub paragraph_id: Option<NodeId>,
    pub title_environment_id: Option<NodeId>,
    pub kind: StructureKind,
    parent_index: Option<usize>,
    pub range: StructureRange,
}

/// 由文档树派生的只读结构索引。
///
/// 建立索引需要遍历文档块；位置查询通过稳定 ID 查找段落，再沿父链解析结构，
/// 时间复杂度与嵌套深度相关，不随文档总段落数线性增长。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StructureIndex {
    structures: Vec<IndexedStructure>,
    paragraph_ordinals: Vec<NodeId>,
    paragraph_lookup: HashMap<NodeId, usize>,
    structure_lookup: HashMap<NodeId, usize>,
}

impl StructureIndex {
    /// 从完整文档构建索引；索引不写入文档或布局缓存。
    pub fn build(document: &Document) -> Self {
        let mut index = Self::default();
        index.collect_blocks(&document.body, &mut Vec::new());
        index
    }

    /// 返回索引结构的稳定文档顺序。
    pub fn structures(&self) -> &[IndexedStructure] {
        &self.structures
    }

    /// 通过稳定节点 ID 查询结构，不扫描整份文档索引。
    pub fn get(&self, id: &NodeId) -> Option<&IndexedStructure> {
        self.structure_lookup
            .get(id)
            .and_then(|index| self.structures.get(*index))
    }

    /// 判断 `ancestor_id` 是否位于 `descendant_id` 的父链上。
    pub fn is_ancestor(&self, ancestor_id: &NodeId, descendant_id: &NodeId) -> bool {
        let Some(&descendant_index) = self.structure_lookup.get(descendant_id) else {
            return false;
        };
        let mut parent_index = self
            .structures
            .get(descendant_index)
            .and_then(|entry| entry.parent_index);
        while let Some(index) = parent_index {
            let Some(parent) = self.structures.get(index) else {
                return false;
            };
            if &parent.id == ancestor_id {
                return true;
            }
            parent_index = parent.parent_index;
        }
        false
    }

    /// 返回由稳定段落 ID 定位的结构栈，从外层容器到当前段落。
    pub fn at(&self, position: &StructurePosition) -> Vec<&IndexedStructure> {
        let Some(&paragraph_index) = self.structure_lookup.get(&position.paragraph_id) else {
            return Vec::new();
        };
        let Some(paragraph) = self.structures.get(paragraph_index) else {
            return Vec::new();
        };
        if paragraph.paragraph_id.is_none() {
            return Vec::new();
        }

        let mut stack_indices = Vec::new();
        let mut parent_index = paragraph.parent_index;
        while let Some(index) = parent_index {
            let Some(parent) = self.structures.get(index) else {
                return Vec::new();
            };
            stack_indices.push(index);
            parent_index = parent.parent_index;
        }
        stack_indices.reverse();
        let mut stack = stack_indices
            .into_iter()
            .filter_map(|index| self.structures.get(index))
            .collect::<Vec<_>>();
        stack.push(paragraph);
        stack
    }

    /// 返回结构导航的段落停靠点。环境标题作为可编辑段落参与导航。
    pub fn navigation_paragraphs(&self) -> &[NodeId] {
        &self.paragraph_ordinals
    }

    /// 返回某个标题 Paragraph 的拥有环境 ID；普通正文段落返回 `None`。
    pub fn title_environment_id(&self, paragraph_id: &NodeId) -> Option<&NodeId> {
        self.structure_lookup
            .get(paragraph_id)
            .and_then(|index| self.structures.get(*index))?
            .title_environment_id
            .as_ref()
    }

    /// 返回指定段落序号的稳定 ID。
    pub fn paragraph_id_at(&self, ordinal: usize) -> Option<&NodeId> {
        self.paragraph_ordinals.get(ordinal)
    }

    /// 将结构位置映射到段落顺序序号。
    pub fn paragraph_ordinal(&self, paragraph_id: &NodeId) -> Option<usize> {
        self.paragraph_lookup.get(paragraph_id).copied()
    }

    /// 在文档顺序中查找相邻的结构段落停靠点。
    pub fn adjacent_paragraph(&self, paragraph_id: &NodeId, forward: bool) -> Option<&NodeId> {
        let ordinal = *self.paragraph_lookup.get(paragraph_id)?;
        if forward {
            self.paragraph_ordinals.get(ordinal + 1)
        } else {
            ordinal
                .checked_sub(1)
                .and_then(|previous| self.paragraph_ordinals.get(previous))
        }
    }

    fn collect_blocks(&mut self, blocks: &[Block], parent_indices: &mut Vec<usize>) {
        let parent_depth = parent_indices.len();
        let mut active_headings: Vec<(u8, usize, usize)> = Vec::new();
        let mut local_headings: Vec<(usize, u8, usize)> = Vec::new();

        for block in blocks {
            if let Block::Paragraph(paragraph) = block
                && let Some(level) = ParagraphAtom::new(paragraph).outline_level()
            {
                while active_headings
                    .last()
                    .is_some_and(|(active_level, _, _)| *active_level >= level)
                {
                    if let Some((_, path_depth, _)) = active_headings.pop() {
                        parent_indices.truncate(path_depth);
                    }
                }
                let entry_index = self.push_paragraph(
                    &ParagraphAtom::new(paragraph),
                    parent_indices.last().copied(),
                    None,
                );
                let start = self.paragraph_ordinals.len() - 1;
                let path_depth = parent_indices.len();
                parent_indices.push(entry_index);
                active_headings.push((level, path_depth, entry_index));
                local_headings.push((start, level, entry_index));
                continue;
            }

            self.collect_block(block, parent_indices);
        }

        let end = self.paragraph_ordinals.len();
        for (heading_position, (_, level, entry_index)) in local_headings.iter().enumerate() {
            let scope_end = local_headings
                .iter()
                .skip(heading_position + 1)
                .find(|(_, next_level, _)| next_level <= level)
                .map_or(end, |(start, _, _)| *start);
            if let Some(entry) = self.structures.get_mut(*entry_index) {
                let start = entry.range.paragraph_ordinals.start;
                entry.range.paragraph_ordinals.end = scope_end.max(start + 1);
            }
        }
        parent_indices.truncate(parent_depth);
    }

    fn collect_block(&mut self, block: &Block, parent_indices: &mut Vec<usize>) {
        match block {
            Block::Paragraph(paragraph) => {
                self.push_paragraph(
                    &ParagraphAtom::new(paragraph),
                    parent_indices.last().copied(),
                    None,
                );
            }
            Block::Environment(environment) => {
                let atom = BlockEnvironmentAtom::new(environment);
                let start = self.paragraph_ordinals.len();
                let entry_index = self.structures.len();
                self.structures.push(IndexedStructure {
                    id: atom.id().clone(),
                    paragraph_id: None,
                    title_environment_id: None,
                    kind: atom.kind(),
                    parent_index: parent_indices.last().copied(),
                    range: StructureRange {
                        paragraph_ordinals: start..start,
                    },
                });
                self.structure_lookup.insert(atom.id().clone(), entry_index);
                parent_indices.push(entry_index);
                if let Some(title) = atom.title() {
                    self.push_paragraph(
                        &ParagraphAtom::new(title),
                        Some(entry_index),
                        Some(atom.id().clone()),
                    );
                }
                self.collect_blocks(atom.children(), parent_indices);
                parent_indices.pop();
                if let Some(entry) = self.structures.get_mut(entry_index) {
                    entry.range.paragraph_ordinals.end = self.paragraph_ordinals.len();
                }
            }
            Block::ExplicitBreak(_) | Block::Opaque(_) => {}
        }
    }

    fn push_paragraph(
        &mut self,
        paragraph: &ParagraphAtom<'_>,
        parent_index: Option<usize>,
        title_environment_id: Option<NodeId>,
    ) -> usize {
        let ordinal = self.paragraph_ordinals.len();
        let id = paragraph.id().clone();
        self.paragraph_ordinals.push(id.clone());
        let kind = paragraph.kind();
        let entry_index = self.structures.len();
        self.structures.push(IndexedStructure {
            id: id.clone(),
            paragraph_id: Some(id.clone()),
            title_environment_id,
            kind,
            parent_index,
            range: StructureRange {
                paragraph_ordinals: ordinal..ordinal + 1,
            },
        });
        self.paragraph_lookup.insert(id.clone(), ordinal);
        self.structure_lookup.insert(id, entry_index);
        entry_index
    }
}

#[cfg(test)]
#[path = "index/tests.rs"]
mod tests;
