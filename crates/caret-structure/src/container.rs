//! 块容器路径：环境与表格单元格的递归定位，供编辑、布局和结构消费者共用。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{Block, NodeId};
use std::mem::size_of;

/// 表格单元格块流的稳定身份；行列下标只用于同一快照内的顺序定位。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCellPath {
    pub table_id: NodeId,
    pub row_id: NodeId,
    pub cell_id: NodeId,
    /// 行下标；同一快照内的顺序键，不持久化。
    pub row_index: usize,
    /// 单元格下标；同一快照内的顺序键，不持久化。
    pub cell_index: usize,
}

impl TableCellPath {
    pub fn new(
        table_id: NodeId,
        row_id: NodeId,
        cell_id: NodeId,
        row_index: usize,
        cell_index: usize,
    ) -> Self {
        Self {
            table_id,
            row_id,
            cell_id,
            row_index,
            cell_index,
        }
    }
}

/// 从根块序列到目标块容器的步骤；环境与表格单元格可任意嵌套。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerStep {
    Environment(NodeId),
    TableCell(TableCellPath),
}

/// 一块序列所属的容器路径；空路径表示文档根块序列。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockContainerPath {
    steps: Vec<ContainerStep>,
}

impl BlockContainerPath {
    /// 从既有环境父链构造容器路径；新增路径步骤应优先使用文档树解析。
    pub fn from_environment_ids(environment_ids: &[NodeId]) -> Self {
        Self {
            steps: environment_ids
                .iter()
                .cloned()
                .map(ContainerStep::Environment)
                .collect(),
        }
    }

    pub fn push(&mut self, step: ContainerStep) {
        self.steps.push(step);
    }

    pub fn pop(&mut self) {
        self.steps.pop();
    }

    /// 最后一步容器步骤；用于标题/普通段落区分。
    pub fn last_step(&self) -> Option<&ContainerStep> {
        self.steps.last()
    }

    /// 最内层表格单元格；用于推导段落 Story 与单元格相对几何。
    pub fn innermost_cell(&self) -> Option<&TableCellPath> {
        self.table_cells().next_back()
    }

    /// 按容器嵌套顺序遍历单元格，供跨格范围比较共同父链。
    pub fn table_cells(&self) -> impl DoubleEndedIterator<Item = &TableCellPath> {
        self.steps.iter().filter_map(|step| match step {
            ContainerStep::TableCell(cell) => Some(cell),
            ContainerStep::Environment(_) => None,
        })
    }

    /// 容器路径在历史估算中的近似字节数。
    pub fn estimated_bytes(&self) -> usize {
        size_of::<BlockContainerPath>()
            + self
                .steps
                .iter()
                .map(|step| match step {
                    ContainerStep::Environment(id) => id.as_str().len(),
                    ContainerStep::TableCell(cell) => {
                        cell.table_id.as_str().len()
                            + cell.row_id.as_str().len()
                            + cell.cell_id.as_str().len()
                    }
                })
                .sum::<usize>()
    }

    /// 块环境嵌套层数；表格单元格不影响行内格式的块环境深度。
    pub fn environment_depth(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| matches!(step, ContainerStep::Environment(_)))
            .count()
    }

    /// 容器内的全部环境 ID；表格单元格不改变环境父链语义。
    pub fn environment_ids(&self) -> Vec<NodeId> {
        self.steps
            .iter()
            .filter_map(|step| match step {
                ContainerStep::Environment(id) => Some(id.clone()),
                ContainerStep::TableCell(_) => None,
            })
            .collect()
    }

    /// 标题所属环境的 ID：标题容器路径的最后一步必须是该环境。
    pub fn title_environment_id(&self) -> Option<&NodeId> {
        match self.steps.last() {
            Some(ContainerStep::Environment(id)) => Some(id),
            _ => None,
        }
    }

    /// 最内层单元格之前的容器；表格块本身位于该容器的块序列中。
    pub fn container_before_innermost_cell(&self) -> Option<Self> {
        let cell_position = self
            .steps
            .iter()
            .rposition(|step| matches!(step, ContainerStep::TableCell(_)))?;
        Some(Self {
            steps: self.steps[..cell_position].to_vec(),
        })
    }

    /// 去掉最后一步容器；用于从标题环境内部回到其父块序列。
    pub fn without_last(&self) -> Option<Self> {
        let (_, remaining) = self.steps.split_last()?;
        Some(Self {
            steps: remaining.to_vec(),
        })
    }
}

/// 按容器路径取得块序列；空路径返回根块序列。
pub fn block_list_by_container<'a>(
    blocks: &'a [Block],
    container: &BlockContainerPath,
) -> Option<&'a [Block]> {
    let mut current = blocks;
    for step in &container.steps {
        match step {
            ContainerStep::Environment(environment_id) => {
                current = environment_children(current, environment_id)?
            }
            ContainerStep::TableCell(cell) => current = table_cell_blocks(current, cell)?,
        }
    }
    Some(current)
}

/// 按容器路径可变取得块序列，不修改路径上的文档内容。
pub fn block_list_by_container_mut<'a>(
    blocks: &'a mut Vec<Block>,
    container: &BlockContainerPath,
) -> Option<&'a mut Vec<Block>> {
    let mut current = blocks;
    for (index, step) in container.steps.iter().enumerate() {
        let next = match step {
            ContainerStep::Environment(environment_id) => {
                let slot = unique_environment_slot(current, environment_id)?;
                match current.get_mut(slot)? {
                    Block::Environment(environment) => &mut environment.children,
                    _ => return None,
                }
            }
            ContainerStep::TableCell(cell) => {
                let cell_blocks = table_cell_blocks_mut(current, cell)?;
                if index + 1 == container.steps.len() {
                    return Some(cell_blocks);
                }
                current = cell_blocks;
                continue;
            }
        };
        current = next;
    }
    Some(current)
}

fn environment_children<'a>(blocks: &'a [Block], environment_id: &NodeId) -> Option<&'a [Block]> {
    let mut matches = blocks.iter().filter_map(|block| match block {
        Block::Environment(environment) if &environment.id == environment_id => {
            Some(environment.children.as_slice())
        }
        _ => None,
    });
    let children = matches.next()?;
    matches.next().is_none().then_some(children)
}

fn unique_environment_slot(blocks: &[Block], environment_id: &NodeId) -> Option<usize> {
    let mut slot = None;
    for (index, block) in blocks.iter().enumerate() {
        if matches!(block, Block::Environment(environment) if &environment.id == environment_id)
            && slot.replace(index).is_some()
        {
            return None;
        }
    }
    slot
}

fn table_cell_blocks<'a>(blocks: &'a [Block], cell: &TableCellPath) -> Option<&'a [Block]> {
    let table = blocks.iter().find_map(|block| match block {
        Block::Table(table) if table.id == cell.table_id => Some(table),
        _ => None,
    })?;
    let row = table.rows.get(cell.row_index)?;
    if row.id != cell.row_id {
        return None;
    }
    let value = row.cells.get(cell.cell_index)?;
    (value.id == cell.cell_id).then_some(value.blocks.as_slice())
}

fn table_cell_blocks_mut<'a>(
    blocks: &'a mut [Block],
    cell: &TableCellPath,
) -> Option<&'a mut Vec<Block>> {
    let table = blocks.iter_mut().find_map(|block| match block {
        Block::Table(table) if table.id == cell.table_id => Some(table),
        _ => None,
    })?;
    let row = table.rows.get_mut(cell.row_index)?;
    if row.id != cell.row_id {
        return None;
    }
    let value = row.cells.get_mut(cell.cell_index)?;
    (value.id == cell.cell_id).then_some(&mut value.blocks)
}
