//! 持有 LaTeX 编辑源码及其解析后范围映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::{borrow::Cow, mem::size_of, ops::Range};

use caret_doc::{Align, MathTree};

use crate::{MAX_MATH_SOURCE_BYTES, MathInputKind, ParsedMath};

mod source_map;
pub use source_map::{
    MathContentSourceMap, MathFlowSourceMap, MathRangeSourceMap, MathTextSourceMap,
    MathTextSourceSegment, MathTreeSourceMap,
};

/// 对齐环境及其各单元格在完整输入中的原始字节范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignSourceMap {
    environment_range: Range<usize>,
    content_range: Range<usize>,
    /// 每个 cell 的语义源码范围，不含分隔符和尾随空白。
    cell_ranges: Vec<Vec<Range<usize>>>,
    /// 每个 cell 的导航范围，包含语义范围裁掉的前后 trivia。
    cell_navigation_ranges: Vec<Vec<Range<usize>>>,
    /// Align cell 的树映射，范围坐标已转换到完整公式源码。
    cell_trees: Vec<Vec<MathTreeSourceMap>>,
}

pub(crate) struct AlignSourceRanges {
    pub(crate) environment: Range<usize>,
    pub(crate) content: Range<usize>,
    pub(crate) cells: Vec<Vec<Range<usize>>>,
    pub(crate) navigation: Vec<Vec<Range<usize>>>,
}

impl AlignSourceMap {
    /// 返回 `aligned` 环境在完整输入中的范围。
    pub fn environment_range(&self) -> Range<usize> {
        self.environment_range.clone()
    }

    /// 返回对齐环境内容（不含 `\begin` / `\end` 命令）的原始字节范围。
    pub fn content_range(&self) -> Range<usize> {
        self.content_range.clone()
    }

    /// 返回指定行列单元格在完整输入中的范围。
    pub fn cell_range(&self, row: usize, cell: usize) -> Option<Range<usize>> {
        self.cell_ranges.get(row)?.get(cell).cloned()
    }

    /// 返回导航用范围；它保留 cell 内容边界附近的注释与空白。
    pub fn cell_navigation_ranges(&self) -> &[Vec<Range<usize>>] {
        &self.cell_navigation_ranges
    }

    /// 返回单元格数学树对应的源码映射。
    pub fn cell_tree(&self, row: usize, cell: usize) -> Option<&MathTreeSourceMap> {
        self.cell_trees.get(row)?.get(cell)
    }

    /// 校验 Align 环境、导航范围、cell 范围与每棵 cell 树映射。
    pub fn same_locations(&self, other: &Self) -> bool {
        self.environment_range == other.environment_range
            && self.content_range == other.content_range
            && self.cell_ranges == other.cell_ranges
            && self.cell_navigation_ranges == other.cell_navigation_ranges
            && self.cell_trees.len() == other.cell_trees.len()
            && self
                .cell_trees
                .iter()
                .zip(&other.cell_trees)
                .all(|(left_row, right_row)| {
                    left_row.len() == right_row.len()
                        && left_row
                            .iter()
                            .zip(right_row)
                            .all(|(left, right)| left.same_locations(right))
                })
    }
}

/// 持有公式源码及同次解析生成的范围映射。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathSourceMap<'a> {
    source: Cow<'a, str>,
    kind: MathInputKind,
    body_range: Range<usize>,
    edit_range: Range<usize>,
    align: Option<AlignSourceMap>,
    tree: Option<MathTreeSourceMap>,
}

impl<'a> MathSourceMap<'a> {
    /// 返回这次解析所绑定的完整源码草稿。
    pub fn source(&self) -> &str {
        &self.source
    }

    /// 返回输入所属的数学对象种类。
    pub fn kind(&self) -> MathInputKind {
        self.kind
    }

    /// 返回剥除可选外层定界符后的源码范围。
    pub fn body_range(&self) -> Range<usize> {
        self.body_range.clone()
    }

    /// 返回当前编辑分支对应的源码视图。Align 使用环境内部，其余类型使用完整输入。
    pub fn edit_source(&self) -> &str {
        &self.source[self.edit_range.clone()]
    }

    /// 返回编辑视图在完整源码中的字节范围。
    pub fn edit_range(&self) -> Range<usize> {
        self.edit_range.clone()
    }

    /// 返回 Align 的环境和单元格源码范围；其他公式类型没有行列范围。
    pub fn align(&self) -> Option<&AlignSourceMap> {
        self.align.as_ref()
    }

    /// 返回普通公式树与当前源码对应的位置映射；Align 通过各 cell 的映射访问。
    pub fn tree(&self) -> Option<&MathTreeSourceMap> {
        self.tree.as_ref()
    }

    /// 在调用方完成结构/语义校验后重建树映射；失败时保留原有效映射。
    ///
    /// 新源码编辑必须与其目标树在同一事务中校验后调用。该函数只验证并采集范围，
    /// 不负责证明任意目标树与 LaTeX 源码语义相同。
    pub fn bind_tree_after_validated_edit(&mut self, tree: &MathTree) -> bool {
        if self.kind == MathInputKind::Align {
            return false;
        }
        let previous_signature = self
            .tree
            .as_ref()
            .and_then(MathTreeSourceMap::semantic_signature);
        let Some(mapped) =
            MathTreeSourceMap::capture(tree, &self.source, 0, None, previous_signature.is_some())
        else {
            return false;
        };
        if previous_signature.is_some_and(|previous| mapped.semantic_signature() != Some(previous))
        {
            return false;
        }
        self.tree = Some(mapped);
        true
    }

    /// 在调用方完成结构/语义校验后重建 Align 单元格映射。
    ///
    /// 新源码编辑必须与其目标树在同一事务中校验后调用。该函数仅采集范围，
    /// 不负责证明任意单元格树与 LaTeX 源码语义相同。
    pub fn bind_align_after_validated_edit(&mut self, align: &Align) -> bool {
        let Some(source_map) = &mut self.align else {
            return false;
        };
        if align.rows.len() != source_map.cell_ranges.len()
            || align
                .rows
                .iter()
                .zip(&source_map.cell_ranges)
                .any(|(row, ranges)| row.cells.len() != ranges.len())
        {
            return false;
        }
        let mut rows = Vec::with_capacity(align.rows.len());
        for (row_index, (row, ranges)) in align.rows.iter().zip(&source_map.cell_ranges).enumerate()
        {
            let mut cells = Vec::with_capacity(row.cells.len());
            for (cell_index, (tree, range)) in row.cells.iter().zip(ranges).enumerate() {
                let Some(mapped) = MathTreeSourceMap::capture(
                    tree,
                    &self.source,
                    range.start,
                    Some(range),
                    source_map
                        .cell_tree(row_index, cell_index)
                        .and_then(MathTreeSourceMap::semantic_signature)
                        .is_some(),
                ) else {
                    return false;
                };
                if source_map
                    .cell_tree(row_index, cell_index)
                    .and_then(MathTreeSourceMap::semantic_signature)
                    .is_some_and(|previous| mapped.semantic_signature() != Some(previous))
                {
                    return false;
                }
                cells.push(mapped);
            }
            rows.push(cells);
        }
        source_map.cell_trees = rows;
        true
    }

    /// 按已有投影替换源码区间，并重定位公式体、编辑视图与 Align 单元格范围。
    /// `align_cell` 标识发生编辑的 cell，范围端点相接时据此区分所属单元格。
    /// 返回值的树映射在事务携带并验证候选树前不可用；调用方必须先完成树编辑，
    /// 再调用对应的 `bind_*_after_validated_edit`，之后才能把投影提交给会话或历史。
    pub fn replace_source_range(
        &self,
        range: Range<usize>,
        replacement: &str,
        align_cell: Option<(usize, usize)>,
    ) -> Option<MathSourceMap<'static>> {
        if range.start > range.end
            || range.end > self.source.len()
            || !self.source.is_char_boundary(range.start)
            || !self.source.is_char_boundary(range.end)
            || align_cell.is_some() != self.align.is_some()
        {
            return None;
        }
        let new_len = self
            .source
            .len()
            .checked_sub(range.len())?
            .checked_add(replacement.len())?;
        if new_len > MAX_MATH_SOURCE_BYTES {
            return None;
        }
        let mut source = self.source.to_string();
        source.replace_range(range.clone(), replacement);
        let delta = replacement.len() as isize - range.len() as isize;
        let rebase = |current: &Range<usize>, contains_edit: bool| {
            rebase_source_range(current, &range, delta, contains_edit)
        };
        let body_range = rebase(&self.body_range, true)?;
        let edit_range = rebase(&self.edit_range, true)?;
        let align = match &self.align {
            Some(align) => {
                let (edited_row, edited_cell_index) = align_cell?;
                let edited_cell = align.cell_range(edited_row, edited_cell_index)?;
                if range.start < edited_cell.start || range.end > edited_cell.end {
                    return None;
                }
                let environment_range = rebase(&align.environment_range, true)?;
                let content_range = rebase(&align.content_range, true)?;
                let cell_ranges = align
                    .cell_ranges
                    .iter()
                    .enumerate()
                    .map(|(row_index, row)| {
                        row.iter()
                            .enumerate()
                            .map(|(cell_index, cell_range)| {
                                rebase(
                                    cell_range,
                                    row_index == edited_row && cell_index == edited_cell_index,
                                )
                            })
                            .collect::<Option<Vec<_>>>()
                    })
                    .collect::<Option<Vec<_>>>()?;
                let cell_navigation_ranges = align
                    .cell_navigation_ranges
                    .iter()
                    .enumerate()
                    .map(|(row_index, row)| {
                        row.iter()
                            .enumerate()
                            .map(|(cell_index, cell_range)| {
                                rebase(
                                    cell_range,
                                    row_index == edited_row && cell_index == edited_cell_index,
                                )
                            })
                            .collect::<Option<Vec<_>>>()
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(AlignSourceMap {
                    environment_range,
                    content_range,
                    cell_ranges,
                    cell_navigation_ranges,
                    cell_trees: Vec::new(),
                })
            }
            None => None,
        };
        Some(MathSourceMap {
            source: Cow::Owned(source),
            kind: self.kind,
            body_range,
            edit_range,
            align: align.map(|mut align| {
                align.cell_trees.clear();
                align
            }),
            tree: None,
        })
    }

    /// 按编辑视图内的字节范围替换源码，并保留完整投影的其他范围。
    pub fn replace_edit_source_range(
        &self,
        range: Range<usize>,
        replacement: &str,
        align_cell: Option<(usize, usize)>,
    ) -> Option<MathSourceMap<'static>> {
        if range.end > self.edit_range.len() {
            return None;
        }
        let start = self.edit_range.start.checked_add(range.start)?;
        let end = self.edit_range.start.checked_add(range.end)?;
        self.replace_source_range(start..end, replacement, align_cell)
    }

    /// 估算该映射保留的内存，包括映射本身及拥有的源码、范围缓冲区。
    pub fn estimated_bytes(&self) -> usize {
        let source_bytes = match &self.source {
            Cow::Borrowed(_) => 0,
            Cow::Owned(source) => source.capacity(),
        };
        let align_bytes = self.align.as_ref().map_or(0, |align| {
            align.cell_ranges.capacity() * size_of::<Vec<Range<usize>>>()
                + align
                    .cell_ranges
                    .iter()
                    .map(|ranges| ranges.capacity() * size_of::<Range<usize>>())
                    .sum::<usize>()
                + align.cell_navigation_ranges.capacity() * size_of::<Vec<Range<usize>>>()
                + align
                    .cell_navigation_ranges
                    .iter()
                    .map(|ranges| ranges.capacity() * size_of::<Range<usize>>())
                    .sum::<usize>()
                + align
                    .cell_trees
                    .iter()
                    .flatten()
                    .map(MathTreeSourceMap::estimated_bytes)
                    .sum::<usize>()
        });
        size_of::<Self>()
            + source_bytes
            + align_bytes
            + self
                .tree
                .as_ref()
                .map_or(0, MathTreeSourceMap::estimated_bytes)
    }
}

fn rebase_source_range(
    current: &Range<usize>,
    edit: &Range<usize>,
    delta: isize,
    contains_edit: bool,
) -> Option<Range<usize>> {
    if contains_edit {
        if current.start > edit.start || current.end < edit.end {
            return None;
        }
        return Some(current.start..current.end.checked_add_signed(delta)?);
    }
    if edit.is_empty() {
        if current.start >= edit.start {
            return Some(
                current.start.checked_add_signed(delta)?..current.end.checked_add_signed(delta)?,
            );
        }
        if current.end <= edit.start {
            return Some(current.clone());
        }
        return Some(current.start..current.end.checked_add_signed(delta)?);
    }
    if current.end <= edit.start {
        Some(current.clone())
    } else if current.start >= edit.end {
        Some(current.start.checked_add_signed(delta)?..current.end.checked_add_signed(delta)?)
    } else {
        None
    }
}

/// 由单次解析产生的公式树与独立源码映射。
#[derive(Debug, Clone, PartialEq)]
pub struct MathProjection<'a> {
    content: ProjectedMath,
    source_map: MathSourceMap<'a>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectedMath {
    Tree(MathTree),
    Align(Align),
}

impl<'a> MathProjection<'a> {
    pub(crate) fn tree(
        source: Cow<'a, str>,
        kind: MathInputKind,
        body_range: Range<usize>,
        tree: MathTree,
        capture_source_map: bool,
    ) -> Option<Self> {
        let edit_range = 0..source.len();
        let mapped_tree = if capture_source_map {
            Some(MathTreeSourceMap::capture(&tree, &source, 0, None, true)?)
        } else {
            None
        };
        Some(Self {
            content: ProjectedMath::Tree(tree),
            source_map: MathSourceMap {
                source,
                kind,
                body_range,
                edit_range,
                align: None,
                tree: mapped_tree,
            },
        })
    }

    pub(crate) fn align(
        source: Cow<'a, str>,
        body_range: Range<usize>,
        tree: Align,
        ranges: AlignSourceRanges,
        capture_source_map: bool,
    ) -> Option<Self> {
        if tree.rows.len() != ranges.cells.len() || tree.rows.len() != ranges.navigation.len() {
            return None;
        }
        let mut cell_trees = Vec::with_capacity(tree.rows.len());
        for ((row, ranges), navigation_ranges) in
            tree.rows.iter().zip(&ranges.cells).zip(&ranges.navigation)
        {
            if row.cells.len() != ranges.len() || row.cells.len() != navigation_ranges.len() {
                return None;
            }
            let mut maps = Vec::with_capacity(row.cells.len());
            for (cell, range) in row.cells.iter().zip(ranges) {
                if capture_source_map {
                    maps.push(MathTreeSourceMap::capture(
                        cell,
                        &source,
                        range.start,
                        Some(range),
                        true,
                    )?);
                }
            }
            cell_trees.push(maps);
        }
        Some(Self {
            content: ProjectedMath::Align(tree),
            source_map: MathSourceMap {
                source,
                kind: MathInputKind::Align,
                body_range,
                edit_range: ranges.content.clone(),
                align: Some(AlignSourceMap {
                    environment_range: ranges.environment,
                    content_range: ranges.content,
                    cell_ranges: ranges.cells,
                    cell_navigation_ranges: ranges.navigation,
                    cell_trees,
                }),
                tree: None,
            },
        })
    }

    /// 返回解析后的公式树，不包含独立持有的完整源码。
    pub fn content(&self) -> &ProjectedMath {
        &self.content
    }

    /// 返回与公式树同次解析生成的源码映射。
    pub fn source_map(&self) -> &MathSourceMap<'a> {
        &self.source_map
    }

    /// 将树与源码映射分别移动给编辑会话持有，避免复制整棵树。
    pub fn into_parts(self) -> (ProjectedMath, MathSourceMap<'a>) {
        (self.content, self.source_map)
    }

    /// 消费投影并返回 Caret 文档节点；源码与范围继续由映射 sidecar 持有。
    pub fn into_parsed(self) -> ParsedMath {
        match self.content {
            ProjectedMath::Tree(tree) => ParsedMath::Tree(tree),
            ProjectedMath::Align(align) => ParsedMath::Align(align),
        }
    }
}
