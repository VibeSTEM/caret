//! 持有 LaTeX 编辑源码及其解析后范围映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::{borrow::Cow, mem::size_of, ops::Range};

use caret_doc::{Align, MathTree};

use crate::{MAX_MATH_SOURCE_BYTES, MathInputKind, ParsedMath};

mod reconcile;
mod source_map;
pub use reconcile::{
    reconcile_align_ranges, reconcile_tree_ranges, source_environment_props_match,
};
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
    /// 每行结束 `\\` 分隔符的完整源码范围。
    row_break_ranges: Vec<Option<Range<usize>>>,
    /// Align cell 的树映射，范围坐标已转换到完整公式源码。
    cell_trees: Vec<Vec<MathTreeSourceMap>>,
}
pub(crate) struct AlignSourceRanges {
    pub(crate) environment: Range<usize>,
    pub(crate) content: Range<usize>,
    pub(crate) cells: Vec<Vec<Range<usize>>>,
    pub(crate) navigation: Vec<Vec<Range<usize>>>,
    pub(crate) row_breaks: Vec<Option<Range<usize>>>,
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

    /// 返回一行所有 cell 及其分隔符覆盖的导航范围。
    pub fn row_navigation_range(&self, row: usize) -> Option<Range<usize>> {
        let cells = self.cell_navigation_ranges.get(row)?;
        Some(cells.first()?.start..cells.last()?.end)
    }

    /// 返回指定逻辑行结束分隔符的完整源码范围；最后一行没有分隔符时为 `None`。
    pub fn row_break_range_after(&self, row: usize) -> Option<Range<usize>> {
        self.row_break_ranges.get(row)?.clone()
    }

    /// 返回单元格数学树对应的源码映射。
    pub fn cell_tree(&self, row: usize, cell: usize) -> Option<&MathTreeSourceMap> {
        self.cell_trees.get(row)?.get(cell)
    }

    /// 比较 Align 环境、导航范围、cell 范围及 cell 树中的源码位置。
    ///
    /// 仅比较位置身份，不比较映射扩展元数据；树内相邻线性文本片段可合并视为相同。
    pub fn same_locations(&self, other: &Self) -> bool {
        self.environment_range == other.environment_range
            && self.content_range == other.content_range
            && self.cell_ranges == other.cell_ranges
            && self.cell_navigation_ranges == other.cell_navigation_ranges
            && self.row_break_ranges == other.row_break_ranges
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

    /// 将已验证的独立范围映射绑定到普通公式树，成功后接管 `mapped`。
    ///
    /// 调用方须在同一事务中验证树版本、源码语义和各结构位置的映射身份；映射范围使用
    /// 完整源码坐标。本方法不证明映射与源码的语义对应；类型、范围、结构或已有语义签名
    /// 不匹配时返回 `false`，保留原映射。
    pub fn bind_tree_sidecar_after_validated_edit(
        &mut self,
        tree: &MathTree,
        mapped: MathTreeSourceMap,
    ) -> bool {
        self.bind_tree_sidecar(tree, mapped, true)
    }

    /// 绑定已由 `MathTreeSourceMap::reconcile_for_tree` 对齐到目标树的 sidecar。
    ///
    /// 重对齐允许树保留不改变 TeX 显示内容的空 `TextFlow` 编辑锚点，因此新旧解析签名可以
    /// 不同；目标树签名、完整源码范围及映射结构仍须通过校验。调用方不得跳过重对齐直接使用。
    pub fn bind_reconciled_tree_sidecar_after_validated_edit(
        &mut self,
        tree: &MathTree,
        mapped: MathTreeSourceMap,
    ) -> bool {
        self.bind_tree_sidecar(tree, mapped, false)
    }

    fn bind_tree_sidecar(
        &mut self,
        tree: &MathTree,
        mapped: MathTreeSourceMap,
        require_same_projection_signature: bool,
    ) -> bool {
        if self.kind == MathInputKind::Align
            || self.align.is_some()
            || !self.valid_source_ranges()
            || mapped
                .validates_projection(tree, &self.source, None)
                .is_none()
            || !mapped.signature_matches_tree(tree)
            || (require_same_projection_signature
                && self
                    .tree
                    .as_ref()
                    .and_then(MathTreeSourceMap::semantic_signature)
                    .is_some_and(|source_signature| {
                        mapped.semantic_signature() != Some(source_signature)
                    }))
        {
            return false;
        }
        self.tree = Some(mapped);
        true
    }

    /// 将独立映射绑定到 Align 的各个 cell，成功后接管 `mapped`。
    ///
    /// 每个映射使用完整源码坐标并限制在对应 cell 范围。调用方须在同一事务中验证树版本、
    /// 源码语义及各结构位置的映射身份；类型、行列、范围或任一 cell 结构/签名不匹配时
    /// 返回 `false`，原映射不变。
    pub fn bind_align_sidecars_after_validated_edit(
        &mut self,
        align: &Align,
        mapped: Vec<Vec<MathTreeSourceMap>>,
    ) -> bool {
        self.bind_align_sidecars(align, mapped, true)
    }

    /// 绑定逐 cell 经 `reconcile_for_tree` 对齐的映射，允许空 TextFlow 编辑锚点。
    pub fn bind_reconciled_align_sidecars_after_validated_edit(
        &mut self,
        align: &Align,
        mapped: Vec<Vec<MathTreeSourceMap>>,
    ) -> bool {
        self.bind_align_sidecars(align, mapped, false)
    }

    fn bind_align_sidecars(
        &mut self,
        align: &Align,
        mapped: Vec<Vec<MathTreeSourceMap>>,
        require_same_projection_signature: bool,
    ) -> bool {
        if self.kind != MathInputKind::Align || self.tree.is_some() || !self.valid_source_ranges() {
            return false;
        }
        let Some(current) = self.align.as_ref() else {
            return false;
        };
        if align.rows.len() != current.cell_ranges.len() || mapped.len() != align.rows.len() {
            return false;
        }
        for (row_index, ((row, ranges), maps)) in align
            .rows
            .iter()
            .zip(&current.cell_ranges)
            .zip(&mapped)
            .enumerate()
        {
            if row.cells.len() != ranges.len() || row.cells.len() != maps.len() {
                return false;
            }
            for (cell_index, ((tree, range), map)) in
                row.cells.iter().zip(ranges).zip(maps).enumerate()
            {
                if !valid_range(range, &self.source)
                    || range.start < current.content_range.start
                    || range.end > current.content_range.end
                    || map
                        .validates_projection(tree, &self.source, Some(range))
                        .is_none()
                    || !map.signature_matches_tree(tree)
                    || (require_same_projection_signature
                        && current
                            .cell_tree(row_index, cell_index)
                            .and_then(MathTreeSourceMap::semantic_signature)
                            .is_some_and(|source_signature| {
                                map.semantic_signature() != Some(source_signature)
                            }))
                {
                    return false;
                }
            }
        }
        let Some(current) = self.align.as_mut() else {
            return false;
        };
        current.cell_trees = mapped;
        true
    }

    fn valid_source_ranges(&self) -> bool {
        if !valid_range(&self.body_range, &self.source)
            || !valid_range(&self.edit_range, &self.source)
        {
            return false;
        }
        match &self.align {
            None => self.edit_range == (0..self.source.len()),
            Some(align) => {
                valid_range(&align.environment_range, &self.source)
                    && valid_range(&align.content_range, &self.source)
                    && self.body_range.start <= align.environment_range.start
                    && align.environment_range.end <= self.body_range.end
                    && align.environment_range.start <= align.content_range.start
                    && align.content_range.end <= align.environment_range.end
                    && align.content_range == self.edit_range
                    && align.cell_ranges.len() == align.cell_navigation_ranges.len()
                    && align.cell_ranges.len() == align.row_break_ranges.len()
                    && align
                        .cell_ranges
                        .iter()
                        .zip(&align.cell_navigation_ranges)
                        .all(|(cells, navigation)| {
                            cells.len() == navigation.len()
                                && cells.iter().zip(navigation).all(|(cell, nav)| {
                                    valid_range(cell, &self.source)
                                        && valid_range(nav, &self.source)
                                        && align.content_range.start <= nav.start
                                        && nav.end <= align.content_range.end
                                        && nav.start <= cell.start
                                        && cell.end <= nav.end
                                })
                        })
                    && align.row_break_ranges.iter().flatten().all(|row_break| {
                        valid_range(row_break, &self.source)
                            && align.content_range.start <= row_break.start
                            && row_break.end <= align.content_range.end
                    })
            }
        }
    }

    /// 过渡绑定入口：从当前源码重新生成 sidecar，再校验并绑定目标树。
    ///
    /// 编辑事务优先直接传入同次解析生成的 sidecar，避免重复解析。此入口保留给尚未迁移的
    /// 调用点；源码、树或旧映射不能唯一对应时返回 `false`，原映射保持不变。
    pub fn bind_tree_after_validated_edit(&mut self, tree: &MathTree) -> bool {
        if self.kind == MathInputKind::Align {
            return false;
        }
        let Ok(projection) = crate::parse_projection(&self.source, self.kind) else {
            return false;
        };
        let (ProjectedMath::Tree(parsed), mut projection) = projection.into_parts() else {
            return false;
        };
        let Some(parsed_map) = projection.tree().cloned() else {
            return false;
        };
        let previous = self.tree.clone();
        let Some(mapped) =
            parsed_map.reconcile_for_tree(&parsed, tree, previous.as_ref(), &self.source, None)
        else {
            return false;
        };
        if !projection.bind_tree_sidecar_after_validated_edit(tree, mapped) {
            return false;
        }
        self.tree = projection.tree;
        true
    }

    /// 过渡 Align 绑定入口：重新解析当前源码并逐 cell 重建独立映射。
    /// 编辑事务应优先传入同次解析的各 cell sidecar，避免重复解析。
    pub fn bind_align_after_validated_edit(&mut self, align: &Align) -> bool {
        if self.kind != MathInputKind::Align {
            return false;
        }
        let Ok(projection) = crate::parse_projection(&self.source, self.kind) else {
            return false;
        };
        let (ProjectedMath::Align(parsed), mut projection) = projection.into_parts() else {
            return false;
        };
        let (Some(parsed_map), Some(previous_map)) = (projection.align(), self.align.clone())
        else {
            return false;
        };
        let mut rows = Vec::with_capacity(align.rows.len());
        if parsed.rows.len() != align.rows.len()
            || parsed_map.cell_navigation_ranges.len() != align.rows.len()
        {
            return false;
        }
        for (row_index, (parsed_row, target_row)) in parsed.rows.iter().zip(&align.rows).enumerate()
        {
            if parsed_row.cells.len() != target_row.cells.len() {
                return false;
            }
            let mut cells = Vec::with_capacity(target_row.cells.len());
            for (cell_index, (parsed_cell, target_cell)) in
                parsed_row.cells.iter().zip(&target_row.cells).enumerate()
            {
                let (Some(parsed_cell_map), Some(scope)) = (
                    parsed_map.cell_tree(row_index, cell_index),
                    parsed_map.cell_range(row_index, cell_index),
                ) else {
                    return false;
                };
                let Some(mapped) = parsed_cell_map.reconcile_for_tree(
                    parsed_cell,
                    target_cell,
                    previous_map.cell_tree(row_index, cell_index),
                    &self.source,
                    Some(&scope),
                ) else {
                    return false;
                };
                cells.push(mapped);
            }
            rows.push(cells);
        }
        if !projection.bind_align_sidecars_after_validated_edit(align, rows) {
            return false;
        }
        self.align = projection.align;
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
                let row_break_ranges = align
                    .row_break_ranges
                    .iter()
                    .map(|row_break| match row_break {
                        Some(row_break) => Some(rebase(row_break, false)),
                        None => Some(None),
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(AlignSourceMap {
                    environment_range,
                    content_range,
                    cell_ranges,
                    cell_navigation_ranges,
                    row_break_ranges,
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
                + align.row_break_ranges.capacity() * size_of::<Option<Range<usize>>>()
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

fn valid_range(range: &Range<usize>, source: &str) -> bool {
    range.start <= range.end
        && range.end <= source.len()
        && source.is_char_boundary(range.start)
        && source.is_char_boundary(range.end)
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
        tree_map: Option<MathTreeSourceMap>,
    ) -> Option<Self> {
        let edit_range = 0..source.len();
        if let Some(tree_map) = &tree_map {
            tree_map.validates_projection(&tree, &source, None)?;
        }
        Some(Self {
            content: ProjectedMath::Tree(tree),
            source_map: MathSourceMap {
                source,
                kind,
                body_range,
                edit_range,
                align: None,
                tree: tree_map,
            },
        })
    }

    pub(crate) fn align(
        source: Cow<'a, str>,
        body_range: Range<usize>,
        tree: Align,
        ranges: AlignSourceRanges,
        source_maps: Option<Vec<Vec<MathTreeSourceMap>>>,
    ) -> Option<Self> {
        if tree.rows.len() != ranges.cells.len() || tree.rows.len() != ranges.navigation.len() {
            return None;
        }
        if tree.rows.len() != ranges.row_breaks.len() {
            return None;
        }
        if let Some(source_maps) = &source_maps
            && source_maps.len() != tree.rows.len()
        {
            return None;
        }
        for (row_index, ((row, cell_ranges), navigation_ranges)) in tree
            .rows
            .iter()
            .zip(&ranges.cells)
            .zip(&ranges.navigation)
            .enumerate()
        {
            if row.cells.len() != cell_ranges.len()
                || row.cells.len() != navigation_ranges.len()
                || source_maps
                    .as_ref()
                    .is_some_and(|maps| maps[row_index].len() != row.cells.len())
            {
                return None;
            }
            if let Some(maps) = &source_maps {
                for ((cell_tree, cell_map), cell_range) in
                    row.cells.iter().zip(&maps[row_index]).zip(cell_ranges)
                {
                    cell_map.validates_projection(cell_tree, &source, Some(cell_range))?;
                }
            }
        }
        let edit_range = ranges.content.clone();
        let align = source_maps.map(|cell_trees| AlignSourceMap {
            environment_range: ranges.environment,
            content_range: ranges.content,
            cell_ranges: ranges.cells,
            cell_navigation_ranges: ranges.navigation,
            row_break_ranges: ranges.row_breaks,
            cell_trees,
        });
        Some(Self {
            content: ProjectedMath::Align(tree),
            source_map: MathSourceMap {
                source,
                kind: MathInputKind::Align,
                body_range,
                edit_range,
                align,
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
    ///
    /// 树不含瞬时位置；源码映射随第二个返回值单独移动。
    pub fn into_parts(self) -> (ProjectedMath, MathSourceMap<'a>) {
        (self.content, self.source_map)
    }

    /// 只保留解析后的文档节点，临时源码与位置映射不得进入持久化模型。
    pub fn into_parsed(self) -> ParsedMath {
        match self.content {
            ProjectedMath::Tree(tree) => ParsedMath::Tree(tree),
            ProjectedMath::Align(align) => ParsedMath::Align(align),
        }
    }
}
