//! 按文档树物理结构解析节点父链、同级位置与全树顺序。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use crate::{BlockContainerPath, ContainerStep, TableCellPath};
use caret_doc::{Block, Document, FlowContent, NodeId, ParagraphContent};
use std::cmp::Ordering;

/// 文档物理树中可由稳定 ID 定位的节点种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentNodeKind {
    Paragraph,
    EnvironmentTitle,
    Environment,
    Equation,
    Align,
    Table,
    InlineMath,
    InlineEnvironment,
}

impl DocumentNodeKind {
    /// 该节点可作为文档级结构范围的原子块单元。
    ///
    /// Section/Custom 以环境子树参与范围；Equation/Align/Table 作为原子块参与范围。
    pub fn is_range_block(self) -> bool {
        self == Self::Environment || self.is_atomic_block()
    }

    /// 显示公式和表格的外侧边界覆盖整个块。
    pub fn is_atomic_block(self) -> bool {
        self.is_math_block() || self == Self::Table
    }

    /// 该节点是块级公式（Equation/Align），在结构范围中按原子对象处理。
    pub fn is_math_block(self) -> bool {
        matches!(self, Self::Equation | Self::Align)
    }
}

/// 仅供同一文档顺序查询内部比较的路径键。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DocumentOrderKey(Vec<usize>);

/// 稳定 ID 节点的物理父链、同级 slot 与文档顺序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentNodeLocation {
    pub id: NodeId,
    pub kind: DocumentNodeKind,
    /// 所属段落宿主；段落节点及其标题中的 Inline 子节点使用自身段落 ID。
    pub paragraph_id: Option<NodeId>,
    /// 所属 Paragraph 是环境标题时为真。
    pub is_title: bool,
    /// 从根到直接父环境的 ID；标题包含其所属环境。
    pub parent_environment_ids: Vec<NodeId>,
    /// 从外到内的行内格式环境 ID；行内数学自身不列入父链。
    pub parent_inline_environment_ids: Vec<NodeId>,
    /// 所属块序列中的真实下标；环境标题没有块序列 slot。
    pub sibling_index: Option<usize>,
    /// 从根到直接父容器的块容器路径（环境与表格单元格）。
    pub container: BlockContainerPath,
    order: DocumentOrderKey,
}

/// 稳定节点内容的外侧边界。
///
/// 这是物理顺序锚点，不单独声明该边界是用户可到达的光标位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentNodeEdge {
    /// 节点内容之前。
    Before,
    /// 节点内容之后。
    After,
}

/// 在完整文档树中查找节点；ExplicitBreak 与 Opaque 保留真实 slot，但不产生身份。
pub fn find_document_node(document: &Document, id: &NodeId) -> Option<DocumentNodeLocation> {
    find_block_node(&document.body, id)
}

/// 在块序列中查找节点；ExplicitBreak 与 Opaque 保留真实 slot，但不产生身份。
pub fn find_block_node(blocks: &[Block], id: &NodeId) -> Option<DocumentNodeLocation> {
    find_in_blocks(
        blocks,
        id,
        &mut TraversalState {
            parents: &mut Vec::new(),
            container: &mut BlockContainerPath::default(),
            prefix: &mut Vec::new(),
        },
    )
}

/// 按父环境 ID 路径取得块序列；空路径返回根块序列。
///
/// 每一级父 ID 必须在当前块序列的直接 BlockEnvironment 子项中唯一匹配。调用者仍须校验目标节点、slot 与文档代际。
pub fn block_list_by_environment_ids<'a>(
    blocks: &'a [Block],
    environment_ids: &[NodeId],
) -> Option<&'a [Block]> {
    let Some((environment_id, rest)) = environment_ids.split_first() else {
        return Some(blocks);
    };
    let mut matches = blocks.iter().filter_map(|block| match block {
        Block::Environment(environment) if &environment.id == environment_id => Some(environment),
        _ => None,
    });
    let environment = matches.next()?;
    matches.next().is_none().then_some(())?;
    block_list_by_environment_ids(&environment.children, rest)
}

/// 按父环境 ID 路径可变取得块序列，不修改路径上的文档内容。
///
/// 每一级父 ID 必须在当前块序列的直接 BlockEnvironment 子项中唯一匹配。调用者仍须校验目标节点、slot 与文档代际。
pub fn block_list_by_environment_ids_mut<'a>(
    blocks: &'a mut Vec<Block>,
    environment_ids: &[NodeId],
) -> Option<&'a mut Vec<Block>> {
    let Some((environment_id, rest)) = environment_ids.split_first() else {
        return Some(blocks);
    };
    let mut matching_index = None;
    for (index, block) in blocks.iter().enumerate() {
        if matches!(block, Block::Environment(environment) if &environment.id == environment_id)
            && matching_index.replace(index).is_some()
        {
            return None;
        }
    }
    let index = matching_index?;
    let Block::Environment(environment) = blocks.get_mut(index)? else {
        return None;
    };
    block_list_by_environment_ids_mut(&mut environment.children, rest)
}

/// 取得表格单元格内的块序列；先按环境 ID 路径定位表格，再按稳定 ID 定位行与单元格。
///
/// 空环境路径表示从根块序列查找。调用者仍须校验目标节点、slot 与文档代际。
pub fn block_list_by_table_cell<'a>(
    blocks: &'a [Block],
    environment_ids: &[NodeId],
    table_id: &NodeId,
    row_id: &NodeId,
    cell_id: &NodeId,
) -> Option<&'a [Block]> {
    let container = block_list_by_environment_ids(blocks, environment_ids)?;
    let table = find_table_in_blocks(container, table_id)?;
    let row = table.rows.iter().find(|row| &row.id == row_id)?;
    let cell = row.cells.iter().find(|cell| &cell.id == cell_id)?;
    Some(&cell.blocks)
}

/// 可变取得表格单元格内的块序列；不修改路径上的文档内容。
pub fn block_list_by_table_cell_mut<'a>(
    blocks: &'a mut Vec<Block>,
    environment_ids: &[NodeId],
    table_id: &NodeId,
    row_id: &NodeId,
    cell_id: &NodeId,
) -> Option<&'a mut Vec<Block>> {
    let container = block_list_by_environment_ids_mut(blocks, environment_ids)?;
    let table = find_table_in_blocks_mut(container, table_id)?;
    let row = table.rows.iter_mut().find(|row| &row.id == row_id)?;
    let cell = row.cells.iter_mut().find(|cell| &cell.id == cell_id)?;
    Some(&mut cell.blocks)
}

fn find_table_in_blocks<'a>(
    blocks: &'a [Block],
    table_id: &NodeId,
) -> Option<&'a caret_doc::Table> {
    let mut matches = blocks.iter().filter_map(|block| match block {
        Block::Table(table) if &table.id == table_id => Some(table),
        _ => None,
    });
    let table = matches.next()?;
    matches.next().is_none().then_some(table)
}

fn find_table_in_blocks_mut<'a>(
    blocks: &'a mut [Block],
    table_id: &NodeId,
) -> Option<&'a mut caret_doc::Table> {
    let mut matching_index = None;
    for (index, block) in blocks.iter().enumerate() {
        if matches!(block, Block::Table(table) if &table.id == table_id)
            && matching_index.replace(index).is_some()
        {
            return None;
        }
    }
    match blocks.get_mut(matching_index?)? {
        Block::Table(table) => Some(table),
        _ => None,
    }
}

/// 比较同一文档快照中两个节点的外侧边界。
///
/// 同一有序序列中的相邻节点共享的间隙会得到相等结果；无 ID 节点仍占据 slot。
/// 父节点的外侧边界会包围标题和后代块。位置键只在本次快照中有效，不应跨代复用。
pub fn compare_document_node_edges(
    document: &Document,
    left_id: &NodeId,
    left_edge: DocumentNodeEdge,
    right_id: &NodeId,
    right_edge: DocumentNodeEdge,
) -> Option<Ordering> {
    let left = find_document_node(document, left_id)?;
    let right = find_document_node(document, right_id)?;
    let left_key = edge_order_key(&left, left_edge)?;
    let right_key = edge_order_key(&right, right_edge)?;
    Some(left_key.cmp(&right_key))
}

fn edge_order_key(location: &DocumentNodeLocation, edge: DocumentNodeEdge) -> Option<Vec<usize>> {
    let mut key = location.order.0.clone();
    match location.kind {
        DocumentNodeKind::EnvironmentTitle => {
            key.push(match edge {
                DocumentNodeEdge::Before => 0,
                DocumentNodeEdge::After => 2,
            });
        }
        _ => {
            let slot = key.last_mut()?;
            *slot = match edge {
                DocumentNodeEdge::Before => slot.checked_sub(1)?,
                DocumentNodeEdge::After => slot.checked_add(1)?,
            };
        }
    }
    Some(key)
}

/// 物理树 DFS 的共享遍历状态：环境父链、容器路径与文档顺序前缀。
struct TraversalState<'a> {
    parents: &'a mut Vec<NodeId>,
    container: &'a mut BlockContainerPath,
    prefix: &'a mut Vec<usize>,
}

fn find_in_blocks(
    blocks: &[Block],
    id: &NodeId,
    state: &mut TraversalState<'_>,
) -> Option<DocumentNodeLocation> {
    for (slot, block) in blocks.iter().enumerate() {
        state.prefix.push(slot * 2 + 1);
        match block {
            Block::Table(table) => {
                if &table.id == id {
                    let location = location(
                        table.id.clone(),
                        DocumentNodeKind::Table,
                        None,
                        false,
                        state,
                        Some(slot),
                    );
                    state.prefix.pop();
                    return Some(location);
                }
                let mut found = None;
                'rows: for (row_index, row) in table.rows.iter().enumerate() {
                    state.prefix.push(row_index * 2 + 1);
                    for (cell_index, cell) in row.cells.iter().enumerate() {
                        state.prefix.push(cell_index * 2 + 1);
                        state
                            .container
                            .push(ContainerStep::TableCell(TableCellPath::new(
                                table.id.clone(),
                                row.id.clone(),
                                cell.id.clone(),
                                row_index,
                                cell_index,
                            )));
                        found = find_in_blocks(&cell.blocks, id, state);
                        state.container.pop();
                        state.prefix.pop();
                        if found.is_some() {
                            state.prefix.pop();
                            break 'rows;
                        }
                    }
                    state.prefix.pop();
                }
                if found.is_some() {
                    state.prefix.pop();
                    return found;
                }
            }
            Block::Paragraph(paragraph) if &paragraph.id == id => {
                let location = location(
                    paragraph.id.clone(),
                    DocumentNodeKind::Paragraph,
                    Some(paragraph.id.clone()),
                    false,
                    state,
                    Some(slot),
                );
                state.prefix.pop();
                return Some(location);
            }
            Block::Paragraph(paragraph) => {
                state.prefix.push(1);
                let found = find_inline_nodes(
                    &paragraph.inlines,
                    id,
                    state,
                    &mut Vec::new(),
                    &paragraph.id,
                    false,
                );
                state.prefix.pop();
                if found.is_some() {
                    state.prefix.pop();
                    return found;
                }
            }
            Block::Equation(equation) if &equation.id == id => {
                let location = location(
                    equation.id.clone(),
                    DocumentNodeKind::Equation,
                    None,
                    false,
                    state,
                    Some(slot),
                );
                state.prefix.pop();
                return Some(location);
            }
            Block::Align(align) if &align.id == id => {
                let location = location(
                    align.id.clone(),
                    DocumentNodeKind::Align,
                    None,
                    false,
                    state,
                    Some(slot),
                );
                state.prefix.pop();
                return Some(location);
            }
            Block::Environment(environment) => {
                if &environment.id == id {
                    let location = location(
                        environment.id.clone(),
                        DocumentNodeKind::Environment,
                        None,
                        false,
                        state,
                        Some(slot),
                    );
                    state.prefix.pop();
                    return Some(location);
                }
                state.parents.push(environment.id.clone());
                state
                    .container
                    .push(ContainerStep::Environment(environment.id.clone()));
                if let Some(title) = &environment.title {
                    if &title.id == id {
                        state.prefix.push(0);
                        let location = location(
                            title.id.clone(),
                            DocumentNodeKind::EnvironmentTitle,
                            Some(title.id.clone()),
                            true,
                            state,
                            None,
                        );
                        state.prefix.pop();
                        state.parents.pop();
                        state.container.pop();
                        state.prefix.pop();
                        return Some(location);
                    }
                    state.prefix.push(0);
                    state.prefix.push(1);
                    let found = find_inline_nodes(
                        &title.inlines,
                        id,
                        state,
                        &mut Vec::new(),
                        &title.id,
                        true,
                    );
                    state.prefix.pop();
                    state.prefix.pop();
                    if found.is_some() {
                        state.parents.pop();
                        state.container.pop();
                        state.prefix.pop();
                        return found;
                    }
                }
                state.prefix.push(1);
                let found = find_in_blocks(&environment.children, id, state);
                state.prefix.pop();
                state.parents.pop();
                state.container.pop();
                if found.is_some() {
                    state.prefix.pop();
                    return found;
                }
                state.prefix.pop();
                continue;
            }
            Block::Equation(_) | Block::Align(_) | Block::ExplicitBreak(_) | Block::Opaque(_) => {}
        }
        state.prefix.pop();
    }
    None
}

fn location(
    id: NodeId,
    kind: DocumentNodeKind,
    paragraph_id: Option<NodeId>,
    is_title: bool,
    state: &TraversalState<'_>,
    sibling_index: Option<usize>,
) -> DocumentNodeLocation {
    DocumentNodeLocation {
        id,
        kind,
        paragraph_id,
        is_title,
        parent_environment_ids: state.parents.to_vec(),
        parent_inline_environment_ids: Vec::new(),
        sibling_index,
        container: state.container.clone(),
        order: DocumentOrderKey(state.prefix.to_vec()),
    }
}

fn find_inline_nodes(
    nodes: &[ParagraphContent],
    id: &NodeId,
    state: &mut TraversalState<'_>,
    inline_parents: &mut Vec<NodeId>,
    paragraph_id: &NodeId,
    is_title: bool,
) -> Option<DocumentNodeLocation> {
    for (slot, node) in nodes.iter().enumerate() {
        state.prefix.push(slot * 2 + 1);
        match node {
            FlowContent::Math(math) if &math.id == id => {
                let mut location = location(
                    math.id.clone(),
                    DocumentNodeKind::InlineMath,
                    Some(paragraph_id.clone()),
                    is_title,
                    state,
                    Some(slot),
                );
                location.parent_inline_environment_ids = inline_parents.clone();
                state.prefix.pop();
                return Some(location);
            }
            FlowContent::Environment(environment) => {
                if &environment.id == id {
                    let mut location = location(
                        environment.id.clone(),
                        DocumentNodeKind::InlineEnvironment,
                        Some(paragraph_id.clone()),
                        is_title,
                        state,
                        Some(slot),
                    );
                    location.parent_inline_environment_ids = inline_parents.clone();
                    state.prefix.pop();
                    return Some(location);
                }
                inline_parents.push(environment.id.clone());
                state.prefix.push(1);
                let found = find_inline_nodes(
                    &environment.children,
                    id,
                    state,
                    inline_parents,
                    paragraph_id,
                    is_title,
                );
                state.prefix.pop();
                inline_parents.pop();
                if found.is_some() {
                    state.prefix.pop();
                    return found;
                }
            }
            FlowContent::Text(_)
            | FlowContent::Math(_)
            | FlowContent::SoftBreak(_)
            | FlowContent::Opaque(_) => {}
        }
        state.prefix.pop();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use caret_doc::{
        Block, BlockEnvironment, BlockEnvironmentKind, Document, NodeId, Paragraph, Table,
        TableCell, TableRow,
    };

    #[test]
    fn block_location_records_nested_environment_and_table_cell_container() {
        let paragraph = Paragraph::empty(NodeId::generate());
        let paragraph_id = paragraph.id.clone();
        let table = Table {
            id: NodeId::generate(),
            rows: vec![TableRow {
                id: NodeId::generate(),
                cells: vec![TableCell {
                    id: NodeId::generate(),
                    blocks: vec![Block::Paragraph(paragraph)],
                }],
            }],
        };
        let table_id = table.id.clone();
        let row_id = table.rows[0].id.clone();
        let cell_id = table.rows[0].cells[0].id.clone();
        let environment = BlockEnvironment {
            id: NodeId::generate(),
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: Default::default(),
            title: None,
            children: vec![Block::Table(table)],
            extra: Default::default(),
        };
        let environment_id = environment.id.clone();
        let mut document = Document::blank();
        document.body = vec![Block::Environment(environment)];

        let location = find_document_node(&document, &paragraph_id)
            .unwrap_or_else(|| panic!("cell paragraph must be locatable"));
        assert_eq!(location.parent_environment_ids, vec![environment_id]);
        let cell = location
            .container
            .innermost_cell()
            .unwrap_or_else(|| panic!("container must record the table cell"));
        assert_eq!(cell.table_id, table_id);
        assert_eq!(cell.row_id, row_id);
        assert_eq!(cell.cell_id, cell_id);
        assert_eq!(location.sibling_index, Some(0));
        let cell_blocks = crate::block_list_by_container(&document.body, &location.container)
            .unwrap_or_else(|| panic!("container path must resolve cell blocks"));
        assert_eq!(cell_blocks.len(), 1);
    }
}
