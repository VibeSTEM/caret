//! 持有 LaTeX 编辑源码及其解析后范围映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::{borrow::Cow, mem::size_of, ops::Range};

use caret_doc::{Align, MathTree};

use crate::{MAX_MATH_SOURCE_BYTES, MathInputKind, ParsedMath};

/// 对齐环境及其各单元格在完整输入中的原始字节范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignSourceMap {
    environment_range: Range<usize>,
    content_range: Range<usize>,
    cell_ranges: Vec<Vec<Range<usize>>>,
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
}

/// 持有公式源码及同次解析生成的范围映射。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathSourceMap<'a> {
    source: Cow<'a, str>,
    kind: MathInputKind,
    body_range: Range<usize>,
    edit_range: Range<usize>,
    align: Option<AlignSourceMap>,
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

    /// 按已有投影替换源码区间，并重定位公式体、编辑视图与 Align 单元格范围。
    /// `align_cell` 标识发生编辑的 cell，范围端点相接时据此区分所属单元格。
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
                Some(AlignSourceMap {
                    environment_range,
                    content_range,
                    cell_ranges,
                })
            }
            None => None,
        };
        Some(MathSourceMap {
            source: Cow::Owned(source),
            kind: self.kind,
            body_range,
            edit_range,
            align,
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
        });
        size_of::<Self>() + source_bytes + align_bytes
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
    ) -> Self {
        let edit_range = 0..source.len();
        Self {
            content: ProjectedMath::Tree(tree),
            source_map: MathSourceMap {
                source,
                kind,
                body_range,
                edit_range,
                align: None,
            },
        }
    }

    pub(crate) fn align(
        source: Cow<'a, str>,
        body_range: Range<usize>,
        tree: Align,
        environment_range: Range<usize>,
        content_range: Range<usize>,
        cell_ranges: Vec<Vec<Range<usize>>>,
    ) -> Self {
        Self {
            content: ProjectedMath::Align(tree),
            source_map: MathSourceMap {
                source,
                kind: MathInputKind::Align,
                body_range,
                edit_range: content_range.clone(),
                align: Some(AlignSourceMap {
                    environment_range,
                    content_range,
                    cell_ranges,
                }),
            },
        }
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

    /// 移动拆分树与映射，同时回填迁移期 DTO 的临时源码字段。
    pub fn into_parts_with_legacy_fields(mut self) -> (ProjectedMath, MathSourceMap<'a>) {
        match &mut self.content {
            ProjectedMath::Tree(tree) => {
                tree.source = self.source_map.source().to_owned();
            }
            ProjectedMath::Align(align) => {
                if let Some(align_map) = &self.source_map.align {
                    align.source =
                        self.source_map.source[align_map.environment_range.clone()].to_owned();
                    for (row, ranges) in align.rows.iter_mut().zip(&align_map.cell_ranges) {
                        for (cell, range) in row.cells.iter_mut().zip(ranges) {
                            cell.source = self.source_map.source[range.clone()].to_owned();
                        }
                    }
                }
            }
        }
        (self.content, self.source_map)
    }

    /// 消费投影并恢复旧的 DTO 临时源码字段，供迁移期旧调用点使用。
    pub fn into_legacy_parsed(self) -> ParsedMath {
        let MathProjection {
            content,
            source_map,
        } = self;
        match content {
            ProjectedMath::Tree(mut tree) => {
                tree.source = source_map.source.into_owned();
                ParsedMath::Tree(tree)
            }
            ProjectedMath::Align(mut align) => {
                if let Some(align_map) = source_map.align {
                    align.source = source_map.source[align_map.environment_range].to_owned();
                    for (row, ranges) in align.rows.iter_mut().zip(align_map.cell_ranges) {
                        for (cell, range) in row.cells.iter_mut().zip(ranges) {
                            cell.source = source_map.source[range].to_owned();
                        }
                    }
                }
                ParsedMath::Align(align)
            }
        }
    }
}
