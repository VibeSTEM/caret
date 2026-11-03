//! 持有 LaTeX 编辑源码及其解析后范围映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::{borrow::Cow, ops::Range};

use caret_doc::{Align, MathTree};

use crate::{MathInputKind, ParsedMath};

/// 对齐环境及其各单元格在完整输入中的原始字节范围。
#[derive(Debug, Clone, PartialEq)]
pub struct AlignSourceMap {
    environment_range: Range<usize>,
    cell_ranges: Vec<Vec<Range<usize>>>,
}

impl AlignSourceMap {
    /// 返回 `aligned` 环境在完整输入中的范围。
    pub fn environment_range(&self) -> Range<usize> {
        self.environment_range.clone()
    }

    /// 返回指定行列单元格在完整输入中的范围。
    pub fn cell_range(&self, row: usize, cell: usize) -> Option<Range<usize>> {
        self.cell_ranges.get(row)?.get(cell).cloned()
    }
}

/// 将原始源码位置与一棵公式树绑定的映射 sidecar。
#[derive(Debug, Clone, PartialEq)]
pub struct MathSourceMap<'a> {
    source: Cow<'a, str>,
    kind: MathInputKind,
    body_range: Range<usize>,
    align: Option<AlignSourceMap>,
}

impl MathSourceMap<'_> {
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

    /// 返回 Align 的环境和单元格源码范围；其他公式类型没有行列范围。
    pub fn align(&self) -> Option<&AlignSourceMap> {
        self.align.as_ref()
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
        Self {
            content: ProjectedMath::Tree(tree),
            source_map: MathSourceMap {
                source,
                kind,
                body_range,
                align: None,
            },
        }
    }

    pub(crate) fn align(
        source: Cow<'a, str>,
        body_range: Range<usize>,
        tree: Align,
        environment_range: Range<usize>,
        cell_ranges: Vec<Vec<Range<usize>>>,
    ) -> Self {
        Self {
            content: ProjectedMath::Align(tree),
            source_map: MathSourceMap {
                source,
                kind: MathInputKind::Align,
                body_range,
                align: Some(AlignSourceMap {
                    environment_range,
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
