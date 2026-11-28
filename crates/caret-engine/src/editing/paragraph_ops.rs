//! 实现段落定位、拆分、合并及块环境标题操作。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::position::compare_affinity;
use super::text_map::{TextMap, inline_length};
use super::{Direction, EditError, Pos, StoryId};
use caret_doc::{Block, FlowContent, NodeId, Paragraph, ParagraphContent};
use std::collections::HashMap;

pub(super) use caret_structure::{BlockContainerPath, ContainerStep, TableCellPath};

fn cell_story(cell: &TableCellPath) -> StoryId {
    StoryId::TableCell {
        table_id: cell.table_id.clone(),
        row_id: cell.row_id.clone(),
        cell_id: cell.cell_id.clone(),
    }
}

fn container_cell_story(container: &BlockContainerPath) -> Option<StoryId> {
    container.innermost_cell().map(cell_story)
}

#[derive(Debug, Clone)]
pub(super) struct ParagraphLocation {
    pub(super) paragraph_id: NodeId,
    pub(super) story: StoryId,
    /// 相对所属块容器的块下标路径；块环境本身由容器路径表达。
    pub(super) block_path: Vec<usize>,
    /// 兼容投影：容器路径中的环境 ID 链。
    pub(super) parent_environment_ids: Vec<NodeId>,
    pub(super) is_title: bool,
    pub(super) container: BlockContainerPath,
    /// 从根块序列推导的文档顺序键；容器相对路径不能跨容器比较，顺序统一用它。
    pub(super) document_path: Vec<usize>,
}

impl ParagraphLocation {
    fn in_container(
        paragraph_id: NodeId,
        block_path: Vec<usize>,
        container: BlockContainerPath,
        document_path: Vec<usize>,
    ) -> Self {
        let story = match container_cell_story(&container) {
            Some(story) if container_is_inside_cell(&container) => story,
            _ => StoryId::Body,
        };
        Self {
            paragraph_id,
            story,
            block_path,
            parent_environment_ids: container.environment_ids(),
            is_title: false,
            container,
            document_path,
        }
    }

    fn title(
        paragraph_id: NodeId,
        environment_id: NodeId,
        container: BlockContainerPath,
        document_path: Vec<usize>,
    ) -> Self {
        Self {
            paragraph_id,
            story: StoryId::EnvironmentTitle(environment_id),
            block_path: Vec::new(),
            parent_environment_ids: container.environment_ids(),
            is_title: true,
            container,
            document_path,
        }
    }
}

fn container_is_inside_cell(container: &BlockContainerPath) -> bool {
    container.innermost_cell().is_some()
}

pub(super) type ParagraphIndex = HashMap<NodeId, ParagraphLocation>;

pub(super) struct ParagraphMergeResult {
    pub(super) left_id: NodeId,
    pub(super) right_id: NodeId,
    pub(super) left_len: usize,
    pub(super) result_boundaries: Vec<usize>,
}

pub(super) struct ParagraphRangeMergeResult {
    pub(super) paragraph_id: NodeId,
    pub(super) removed_ids: Vec<NodeId>,
    pub(super) start_offset: usize,
    pub(super) end_offset: usize,
    pub(super) result_boundaries: Vec<usize>,
}

pub(super) struct SectionTitleMergeResult {
    pub(super) merge: ParagraphMergeResult,
    pub(super) environment_id: NodeId,
}

pub(super) fn build_paragraph_index(blocks: &[Block]) -> ParagraphIndex {
    let mut index = ParagraphIndex::new();
    collect_locations(
        blocks,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut BlockContainerPath::default(),
        &mut index,
    );
    index
}

fn collect_locations(
    blocks: &[Block],
    block_path: &mut Vec<usize>,
    document_path: &mut Vec<usize>,
    container: &mut BlockContainerPath,
    index: &mut ParagraphIndex,
) {
    for (block_index, block) in blocks.iter().enumerate() {
        block_path.push(block_index);
        document_path.push(block_index);
        match block {
            Block::Paragraph(paragraph) => {
                index.insert(
                    paragraph.id.clone(),
                    ParagraphLocation::in_container(
                        paragraph.id.clone(),
                        block_path.clone(),
                        container.clone(),
                        document_path.clone(),
                    ),
                );
            }
            Block::Environment(environment) => {
                if let Some(title) = &environment.title {
                    let mut title_container = container.clone();
                    title_container.push(ContainerStep::Environment(environment.id.clone()));
                    index.insert(
                        title.id.clone(),
                        ParagraphLocation::title(
                            title.id.clone(),
                            environment.id.clone(),
                            title_container,
                            document_path.clone(),
                        ),
                    );
                }
                container.push(ContainerStep::Environment(environment.id.clone()));
                collect_locations(
                    &environment.children,
                    &mut Vec::new(),
                    document_path,
                    container,
                    index,
                );
                container.pop();
            }
            Block::Table(table) => {
                for (row_index, row) in table.rows.iter().enumerate() {
                    for (cell_index, cell) in row.cells.iter().enumerate() {
                        container.push(ContainerStep::TableCell(TableCellPath {
                            table_id: table.id.clone(),
                            row_id: row.id.clone(),
                            cell_id: cell.id.clone(),
                            row_index,
                            cell_index,
                        }));
                        document_path.push(row_index + 1);
                        document_path.push(cell_index + 1);
                        collect_locations(
                            &cell.blocks,
                            &mut Vec::new(),
                            document_path,
                            container,
                            index,
                        );
                        document_path.pop();
                        document_path.pop();
                        container.pop();
                    }
                }
            }
            Block::ExplicitBreak(_) | Block::Opaque(_) | Block::Equation(_) | Block::Align(_) => {}
        }
        document_path.pop();
        block_path.pop();
    }
}

/// 容器内的文档顺序前缀长度 = 文档路径去掉容器相对块路径。
fn container_document_prefix(location: &ParagraphLocation) -> Vec<usize> {
    let prefix_len = location
        .document_path
        .len()
        .saturating_sub(location.block_path.len());
    location.document_path[..prefix_len].to_vec()
}

pub(super) fn update_index_after_split(
    blocks: &[Block],
    index: &mut ParagraphIndex,
    right_id: &NodeId,
    location: &ParagraphLocation,
) {
    let document_prefix = container_document_prefix(location);
    if location.is_title {
        let Some(environment) = title_environment_in_container(blocks, &location.container) else {
            return;
        };
        collect_locations_from(
            &environment.children,
            &[],
            &document_prefix,
            &location.container.clone(),
            0,
            index,
        );
    } else {
        let Some((&source_index, parent_path)) = location.block_path.split_last() else {
            return;
        };
        let Some(siblings) = block_list_at_location(blocks, location, parent_path) else {
            return;
        };
        let mut split_prefix = document_prefix.clone();
        split_prefix.extend_from_slice(parent_path);
        collect_locations_from(
            siblings,
            parent_path,
            &split_prefix,
            &location.container,
            source_index + 2,
            index,
        );
        let mut right_path = parent_path.to_vec();
        right_path.push(source_index + 1);
        let mut right_document_path = split_prefix;
        right_document_path.push(source_index + 1);
        index.insert(
            right_id.clone(),
            ParagraphLocation::in_container(
                right_id.clone(),
                right_path,
                location.container.clone(),
                right_document_path,
            ),
        );
    }
}

pub(super) fn update_index_after_merge(
    blocks: &[Block],
    index: &mut ParagraphIndex,
    left_id: &NodeId,
    right_id: &NodeId,
) {
    let Some(location) = index.get(left_id).cloned() else {
        return;
    };
    if location.is_title {
        let Some(environment) = title_environment_in_container(blocks, &location.container) else {
            return;
        };
        collect_locations_from(
            &environment.children,
            &[],
            &location.document_path,
            &location.container,
            0,
            index,
        );
        index.remove(right_id);
        return;
    }
    let Some((&left_index, parent_path)) = location.block_path.split_last() else {
        return;
    };
    let parent_path = parent_path.to_vec();
    let Some(siblings) = block_list_at_location(blocks, &location, &parent_path) else {
        return;
    };
    let document_prefix = container_document_prefix(&location);
    let mut prefix = document_prefix;
    prefix.extend_from_slice(&parent_path);
    collect_locations_from(
        siblings,
        &parent_path,
        &prefix,
        &location.container,
        left_index + 1,
        index,
    );
    index.remove(right_id);
}

fn collect_locations_from(
    blocks: &[Block],
    parent_path: &[usize],
    document_prefix: &[usize],
    container: &BlockContainerPath,
    start_index: usize,
    index: &mut ParagraphIndex,
) {
    for (block_index, block) in blocks.iter().enumerate().skip(start_index) {
        let mut path = parent_path.to_vec();
        path.push(block_index);
        let mut document_path = document_prefix.to_vec();
        document_path.push(block_index);
        collect_block_locations(block, &path, &document_path, container, index);
    }
}

fn collect_block_locations(
    block: &Block,
    path: &[usize],
    document_path: &[usize],
    container: &BlockContainerPath,
    index: &mut ParagraphIndex,
) {
    match block {
        Block::Paragraph(paragraph) => {
            index.insert(
                paragraph.id.clone(),
                ParagraphLocation::in_container(
                    paragraph.id.clone(),
                    path.to_vec(),
                    container.clone(),
                    document_path.to_vec(),
                ),
            );
        }
        Block::Environment(environment) => {
            if let Some(title) = &environment.title {
                let mut title_container = container.clone();
                title_container.push(ContainerStep::Environment(environment.id.clone()));
                index.insert(
                    title.id.clone(),
                    ParagraphLocation::title(
                        title.id.clone(),
                        environment.id.clone(),
                        title_container,
                        document_path.to_vec(),
                    ),
                );
            }
            let mut child_container = container.clone();
            child_container.push(ContainerStep::Environment(environment.id.clone()));
            collect_locations(
                &environment.children,
                &mut Vec::new(),
                &mut document_path.to_vec(),
                &mut child_container,
                index,
            );
        }
        Block::Table(table) => {
            for (row_index, row) in table.rows.iter().enumerate() {
                for (cell_index, cell) in row.cells.iter().enumerate() {
                    let mut child_container = container.clone();
                    child_container.push(ContainerStep::TableCell(TableCellPath {
                        table_id: table.id.clone(),
                        row_id: row.id.clone(),
                        cell_id: cell.id.clone(),
                        row_index,
                        cell_index,
                    }));
                    let mut child_document_path = document_path.to_vec();
                    child_document_path.push(row_index + 1);
                    child_document_path.push(cell_index + 1);
                    collect_locations(
                        &cell.blocks,
                        &mut Vec::new(),
                        &mut child_document_path,
                        &mut child_container,
                        index,
                    );
                }
            }
        }
        Block::ExplicitBreak(_) | Block::Opaque(_) | Block::Equation(_) | Block::Align(_) => {}
    }
}

/// 标题所属环境的可变引用：标题容器路径的最后一步是该环境。
fn title_environment_mut<'a>(
    blocks: &'a mut Vec<Block>,
    location: &ParagraphLocation,
) -> Option<&'a mut caret_doc::BlockEnvironment> {
    let environment_id = location.container.title_environment_id()?.clone();
    let parent = location.container.without_last()?;
    let parent_blocks = container_blocks_mut(blocks, &parent)?;
    parent_blocks.iter_mut().find_map(|block| match block {
        Block::Environment(environment) if environment.id == environment_id => Some(environment),
        _ => None,
    })
}

/// 普通段落的直接环境容器（块环境子项）；根容器或单元格直接子项返回 `None`。
fn paragraph_environment_mut<'a>(
    blocks: &'a mut Vec<Block>,
    location: &ParagraphLocation,
) -> Option<&'a mut caret_doc::BlockEnvironment> {
    let environment_id = location.container.last_step().and_then(|step| match step {
        ContainerStep::Environment(id) => Some(id.clone()),
        ContainerStep::TableCell(_) => None,
    })?;
    let parent = location.container.without_last()?;
    let parent_blocks = container_blocks_mut(blocks, &parent)?;
    parent_blocks.iter_mut().find_map(|block| match block {
        Block::Environment(environment) if environment.id == environment_id => Some(environment),
        _ => None,
    })
}

/// 用替换块覆盖位置所指的块；单元格与环境标题都按容器路径解析。
pub(super) fn replace_block_at_location(
    blocks: &mut Vec<Block>,
    location: &ParagraphLocation,
    replacement: Block,
) -> bool {
    let Some(block) = block_at_location_mut(blocks, location, &location.block_path) else {
        return false;
    };
    *block = replacement;
    true
}

/// 位置所在容器内的块序列（相对路径的父级）。
pub(super) fn block_list_for_location<'a>(
    blocks: &'a [Block],
    location: &ParagraphLocation,
    parent_path: &[usize],
) -> Option<&'a [Block]> {
    block_list_at_location(blocks, location, parent_path)
}

pub(super) fn block_list_in_container<'a>(
    blocks: &'a [Block],
    container: &BlockContainerPath,
) -> Option<&'a [Block]> {
    container_blocks(blocks, container)
}

pub(super) fn block_list_in_container_mut<'a>(
    blocks: &'a mut Vec<Block>,
    container: &BlockContainerPath,
) -> Option<&'a mut Vec<Block>> {
    container_blocks_mut(blocks, container)
}

/// 标题所属环境的只读引用（按容器路径）。
pub(super) fn title_environment_in_container<'a>(
    blocks: &'a [Block],
    container: &BlockContainerPath,
) -> Option<&'a caret_doc::BlockEnvironment> {
    let environment_id = container.title_environment_id()?;
    let parent = container.without_last()?;
    let parent_blocks = container_blocks(blocks, &parent)?;
    parent_blocks.iter().find_map(|block| match block {
        Block::Environment(environment) if &environment.id == environment_id => Some(environment),
        _ => None,
    })
}

/// 标题所属环境的可变引用（按容器路径）；仅供历史补丁恢复标题槽。
pub(super) fn title_environment_in_container_mut<'a>(
    blocks: &'a mut Vec<Block>,
    container: &BlockContainerPath,
) -> Option<&'a mut caret_doc::BlockEnvironment> {
    let environment_id = container.title_environment_id()?.clone();
    let parent = container.without_last()?;
    let parent_blocks = container_blocks_mut(blocks, &parent)?;
    parent_blocks.iter_mut().find_map(|block| match block {
        Block::Environment(environment) if environment.id == environment_id => Some(environment),
        _ => None,
    })
}

fn container_blocks<'a>(
    blocks: &'a [Block],
    container: &BlockContainerPath,
) -> Option<&'a [Block]> {
    caret_structure::block_list_by_container(blocks, container)
}

fn container_blocks_mut<'a>(
    blocks: &'a mut Vec<Block>,
    container: &BlockContainerPath,
) -> Option<&'a mut Vec<Block>> {
    caret_structure::block_list_by_container_mut(blocks, container)
}

fn block_list_at_path<'a>(blocks: &'a [Block], parent_path: &[usize]) -> Option<&'a [Block]> {
    let mut current = blocks;
    for block_index in parent_path {
        let Block::Environment(environment) = current.get(*block_index)? else {
            return None;
        };
        current = &environment.children;
    }
    Some(current)
}

fn block_list_at_location<'a>(
    blocks: &'a [Block],
    location: &ParagraphLocation,
    parent_path: &[usize],
) -> Option<&'a [Block]> {
    let container = container_blocks(blocks, &location.container)?;
    block_list_at_path(container, parent_path)
}

fn block_list_at_location_mut<'a>(
    blocks: &'a mut Vec<Block>,
    location: &ParagraphLocation,
    parent_path: &[usize],
) -> Option<&'a mut Vec<Block>> {
    let mut current = container_blocks_mut(blocks, &location.container)?;
    for block_index in parent_path {
        let Block::Environment(environment) = current.get_mut(*block_index)? else {
            return None;
        };
        current = &mut environment.children;
    }
    Some(current)
}

fn block_at_location_mut<'a>(
    blocks: &'a mut Vec<Block>,
    location: &ParagraphLocation,
    path: &[usize],
) -> Option<&'a mut Block> {
    let (block_index, parent_path) = path.split_last()?;
    block_list_at_location_mut(blocks, location, parent_path)?.get_mut(*block_index)
}

pub(super) fn paragraph_at_location<'a>(
    blocks: &'a [Block],
    location: &ParagraphLocation,
) -> Option<&'a Paragraph> {
    paragraph_at_location_any(blocks, location)
        .filter(|paragraph| paragraph.id == location.paragraph_id)
}

/// 校验缓存段落位置并返回段落、真实父链和同级位置；标题父链包含所属环境，标题槽返回 `0`。
pub(super) fn validated_paragraph_location<'a>(
    blocks: &'a [Block],
    location: &ParagraphLocation,
) -> Option<(&'a Paragraph, Vec<NodeId>, usize)> {
    let paragraph = paragraph_at_location(blocks, location)?;
    let physical = caret_structure::find_block_node(blocks, &location.paragraph_id)?;
    if physical.parent_environment_ids != location.parent_environment_ids {
        return None;
    }
    if location.is_title {
        let owner_id = physical.parent_environment_ids.last()?;
        if physical.kind != caret_structure::DocumentNodeKind::EnvironmentTitle
            || location.story != StoryId::EnvironmentTitle(owner_id.clone())
        {
            return None;
        }
        Some((paragraph, physical.parent_environment_ids, 0))
    } else {
        let expected_story = container_cell_story(&location.container).unwrap_or(StoryId::Body);
        if physical.kind != caret_structure::DocumentNodeKind::Paragraph
            || location.story != expected_story
        {
            return None;
        }
        Some((
            paragraph,
            physical.parent_environment_ids,
            physical.sibling_index?,
        ))
    }
}

/// 返回已校验段落的父链与定位 slot；标题的 `0` 表示标题槽，不是正文 children 下标。
pub(super) fn paragraph_parent_path(
    blocks: &[Block],
    location: &ParagraphLocation,
) -> Option<(Vec<NodeId>, usize)> {
    validated_paragraph_location(blocks, location)
        .map(|(_, parent_environment_ids, sibling_index)| (parent_environment_ids, sibling_index))
}

/// 容器对应的可编辑 Story：最内层单元格优先，否则正文 Story。
pub(super) fn story_for_container(container: &BlockContainerPath) -> StoryId {
    container
        .innermost_cell()
        .map_or(StoryId::Body, |cell| StoryId::TableCell {
            table_id: cell.table_id.clone(),
            row_id: cell.row_id.clone(),
            cell_id: cell.cell_id.clone(),
        })
}

/// 段落所在块序列的容器路径与同级下标；标题返回其环境父容器的标题槽。
pub(super) fn paragraph_parent_container_path(
    blocks: &[Block],
    location: &ParagraphLocation,
) -> Option<(BlockContainerPath, usize)> {
    validated_paragraph_location(blocks, location)?;
    if location.is_title {
        return Some((location.container.without_last()?, 0));
    }
    Some((location.container.clone(), *location.block_path.last()?))
}

pub(super) fn paragraph_at_location_any<'a>(
    blocks: &'a [Block],
    location: &ParagraphLocation,
) -> Option<&'a Paragraph> {
    if location.is_title {
        let environment_id = location.container.title_environment_id()?;
        let parent = location.container.without_last()?;
        let parent_blocks = container_blocks(blocks, &parent)?;
        let environment = parent_blocks.iter().find_map(|block| match block {
            Block::Environment(environment) if &environment.id == environment_id => {
                Some(environment)
            }
            _ => None,
        })?;
        return environment.title.as_ref();
    }
    let container = container_blocks(blocks, &location.container)?;
    let (block_index, parent_path) = location.block_path.split_last()?;
    let siblings = block_list_at_path(container, parent_path)?;
    match siblings.get(*block_index) {
        Some(Block::Paragraph(paragraph)) => Some(paragraph),
        _ => None,
    }
}

pub(super) fn paragraph_at_location_mut<'a>(
    blocks: &'a mut Vec<Block>,
    location: &ParagraphLocation,
) -> Option<&'a mut Paragraph> {
    if location.is_title {
        let environment_id = location.container.title_environment_id()?.clone();
        let parent = location.container.without_last()?;
        let parent_blocks = container_blocks_mut(blocks, &parent)?;
        let environment = parent_blocks.iter_mut().find_map(|block| match block {
            Block::Environment(environment) if environment.id == environment_id => {
                Some(environment)
            }
            _ => None,
        })?;
        return environment.title.as_mut();
    }
    let container = container_blocks_mut(blocks, &location.container)?;
    let (block_index, parent_path) = location.block_path.split_last()?;
    let mut siblings: &mut Vec<Block> = container;
    for path_index in parent_path {
        let Block::Environment(environment) = siblings.get_mut(*path_index)? else {
            return None;
        };
        siblings = &mut environment.children;
    }
    let paragraph = siblings.get_mut(*block_index)?;
    match paragraph {
        Block::Paragraph(paragraph) => Some(paragraph),
        _ => None,
    }
}

pub(super) fn first_paragraph(blocks: &[Block]) -> Option<(&Paragraph, StoryId)> {
    first_paragraph_with_cell(blocks, None)
}

/// 在块序列中找首个可编辑段落；单元格内段落使用 TableCell Story。
fn first_paragraph_with_cell<'a>(
    blocks: &'a [Block],
    cell_story: Option<&StoryId>,
) -> Option<(&'a Paragraph, StoryId)> {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                return Some((paragraph, cell_story.cloned().unwrap_or(StoryId::Body)));
            }
            Block::Environment(environment) => {
                if let Some(title) = &environment.title {
                    return Some((title, StoryId::EnvironmentTitle(environment.id.clone())));
                }
                if let Some(paragraph) =
                    first_paragraph_with_cell(&environment.children, cell_story)
                {
                    return Some(paragraph);
                }
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        let story = StoryId::TableCell {
                            table_id: table.id.clone(),
                            row_id: row.id.clone(),
                            cell_id: cell.id.clone(),
                        };
                        if let Some(paragraph) =
                            first_paragraph_with_cell(&cell.blocks, Some(&story))
                        {
                            return Some(paragraph);
                        }
                    }
                }
            }
            Block::ExplicitBreak(_) | Block::Opaque(_) | Block::Equation(_) | Block::Align(_) => {}
        }
    }
    None
}

/// 校验段落身份、Story、物理路径和 TextMap 边界，再按文档顺序比较位置。
pub(super) fn compare_positions(
    blocks: &[Block],
    index: &ParagraphIndex,
    left: &Pos,
    right: &Pos,
) -> Result<std::cmp::Ordering, EditError> {
    if (matches!(&left.story, StoryId::TableCell { .. })
        || matches!(&right.story, StoryId::TableCell { .. }))
        && left.story != right.story
    {
        return Err(EditError::WrongStory);
    }
    compare_document_positions(blocks, index, left, right)
}

/// 比较已校验的文档位置，包含不同单元格及正文之间的物理顺序。
pub(super) fn compare_document_positions(
    blocks: &[Block],
    index: &ParagraphIndex,
    left: &Pos,
    right: &Pos,
) -> Result<std::cmp::Ordering, EditError> {
    let left_location = index
        .get(&left.paragraph_id)
        .ok_or_else(|| EditError::UnknownParagraph(left.paragraph_id.to_string()))?;
    let right_location = index
        .get(&right.paragraph_id)
        .ok_or_else(|| EditError::UnknownParagraph(right.paragraph_id.to_string()))?;
    if left_location.paragraph_id != left.paragraph_id
        || right_location.paragraph_id != right.paragraph_id
    {
        return Err(EditError::InvalidPosition);
    }
    if left_location.story != left.story || right_location.story != right.story {
        return Err(EditError::WrongStory);
    }
    let left_paragraph =
        paragraph_at_location(blocks, left_location).ok_or(EditError::InvalidPosition)?;
    if left.paragraph_id == right.paragraph_id {
        let map = TextMap::new(left_paragraph)?;
        map.validate_pos(left)?;
        map.validate_pos(right)?;
    } else {
        let right_paragraph =
            paragraph_at_location(blocks, right_location).ok_or(EditError::InvalidPosition)?;
        TextMap::new(left_paragraph)?.validate_pos(left)?;
        TextMap::new(right_paragraph)?.validate_pos(right)?;
    }
    Ok(left_location
        .document_path
        .cmp(&right_location.document_path)
        .then_with(|| left.offset.cmp(&right.offset))
        .then_with(|| compare_affinity(left.affinity, right.affinity)))
}

pub(super) fn insert_paragraph_after(
    blocks: &mut Vec<Block>,
    location: &ParagraphLocation,
    left: &mut Option<Paragraph>,
    right: &mut Option<Paragraph>,
) -> bool {
    if location.is_title {
        return false;
    }
    let Some((&target_index, parent_path)) = location.block_path.split_last() else {
        return false;
    };
    let Some(parent) = block_list_at_location_mut(blocks, location, parent_path) else {
        return false;
    };
    if !matches!(parent.get(target_index), Some(Block::Paragraph(_))) {
        return false;
    }
    let (Some(left), Some(right)) = (left.take(), right.take()) else {
        return false;
    };
    parent[target_index] = Block::Paragraph(left);
    parent.insert(target_index + 1, Block::Paragraph(right));
    true
}

pub(super) fn replace_paragraph_with_many(
    blocks: &mut Vec<Block>,
    location: &ParagraphLocation,
    paragraphs: &mut Vec<Paragraph>,
) -> bool {
    if location.is_title || paragraphs.is_empty() {
        return false;
    }
    let Some((&target_index, parent_path)) = location.block_path.split_last() else {
        return false;
    };
    let Some(parent) = block_list_at_location_mut(blocks, location, parent_path) else {
        return false;
    };
    if !matches!(parent.get(target_index), Some(Block::Paragraph(_))) {
        return false;
    }
    let replacements = std::mem::take(paragraphs)
        .into_iter()
        .map(Block::Paragraph)
        .collect::<Vec<_>>();
    parent.splice(target_index..=target_index, replacements);
    true
}

pub(super) fn split_environment_title(
    blocks: &mut Vec<Block>,
    location: &ParagraphLocation,
    left: &mut Option<Paragraph>,
    right: &mut Option<Paragraph>,
) -> bool {
    let StoryId::EnvironmentTitle(environment_id) = &location.story else {
        return false;
    };
    if !location.is_title {
        return false;
    }
    let Some(environment) = title_environment_mut(blocks, location) else {
        return false;
    };
    if &environment.id != environment_id {
        return false;
    }
    if environment.title.is_none() {
        return false;
    }
    let (Some(left), Some(right)) = (left.take(), right.take()) else {
        return false;
    };
    environment.title = Some(left);
    environment.children.insert(0, Block::Paragraph(right));
    true
}

pub(super) fn merge_adjacent_paragraph(
    blocks: &mut Vec<Block>,
    location: &ParagraphLocation,
    direction: Direction,
) -> Result<Option<ParagraphMergeResult>, EditError> {
    if location.is_title {
        return Ok(None);
    }
    let Some((&index, parent_path)) = location.block_path.split_last() else {
        return Ok(None);
    };
    let Some(blocks) = block_list_at_location_mut(blocks, location, parent_path) else {
        return Ok(None);
    };
    let (left_index, right_index) = match direction {
        Direction::Backward if index > 0 => (index - 1, index),
        Direction::Forward if index + 1 < blocks.len() => (index, index + 1),
        _ => return Ok(None),
    };
    let (Block::Paragraph(left), Block::Paragraph(right)) =
        (&blocks[left_index], &blocks[right_index])
    else {
        return Ok(None);
    };
    if left.language != right.language
        || left.layout != right.layout
        || left.mark_style != right.mark_style
        || left.props != right.props
        || !left.extra.is_empty()
        || !right.extra.is_empty()
    {
        return Err(EditError::IncompatibleParagraphMerge);
    }
    let mut merged = left.clone();
    let right = right.clone();
    let caret_offset = merged.inlines.iter().try_fold(0usize, |length, node| {
        length
            .checked_add(inline_length(node)?)
            .ok_or(EditError::InvalidPosition)
    })?;
    merged.inlines.extend(right.inlines);
    let merged_id = merged.id.clone();
    let removed_id = right.id.clone();
    let boundaries = TextMap::new(&merged)?.grapheme_boundaries;
    blocks[left_index] = Block::Paragraph(merged);
    blocks.remove(right_index);
    Ok(Some(ParagraphMergeResult {
        left_id: merged_id,
        right_id: removed_id,
        left_len: caret_offset,
        result_boundaries: boundaries,
    }))
}

/// 将 Section 首个正文段落合并回独立标题，供段首退格删除标题边界。
pub(super) fn merge_section_body_into_title(
    blocks: &mut Vec<Block>,
    location: &ParagraphLocation,
) -> Result<Option<SectionTitleMergeResult>, EditError> {
    if location.is_title || location.story != StoryId::Body {
        return Ok(None);
    }
    let Some((&index, _parent_path)) = location.block_path.split_last() else {
        return Ok(None);
    };
    if index != 0 {
        return Ok(None);
    }
    let Some(environment) = paragraph_environment_mut(blocks, location) else {
        return Ok(None);
    };
    if environment.kind != caret_doc::BlockEnvironmentKind::Section {
        return Ok(None);
    }
    let Some(title) = environment.title.as_mut() else {
        return Ok(None);
    };
    let Some(Block::Paragraph(body)) = environment.children.first() else {
        return Ok(None);
    };
    if title.language != body.language
        || title.layout != body.layout
        || title.mark_style != body.mark_style
        || title.props != body.props
        || !title.extra.is_empty()
        || !body.extra.is_empty()
    {
        return Err(EditError::IncompatibleParagraphMerge);
    }

    let left_len = title.inlines.iter().try_fold(0usize, |length, node| {
        length
            .checked_add(inline_length(node)?)
            .ok_or(EditError::InvalidPosition)
    })?;
    let body = match environment.children.remove(0) {
        Block::Paragraph(body) => body,
        _ => return Err(EditError::InvalidPosition),
    };
    title.inlines.extend(body.inlines);
    let title_id = title.id.clone();
    let environment_id = environment.id.clone();
    let boundaries = TextMap::new(title)?.grapheme_boundaries;
    Ok(Some(SectionTitleMergeResult {
        merge: ParagraphMergeResult {
            left_id: title_id,
            right_id: body.id,
            left_len,
            result_boundaries: boundaries,
        },
        environment_id,
    }))
}

/// 将空 Section 后紧邻段落并入标题，供该段段首退格回到节标题末尾。
pub(super) fn merge_paragraph_after_empty_section_into_title(
    blocks: &mut Vec<Block>,
    location: &ParagraphLocation,
) -> Result<Option<SectionTitleMergeResult>, EditError> {
    if location.is_title || location.story != StoryId::Body {
        return Ok(None);
    }
    let Some((&paragraph_index, parent_path)) = location.block_path.split_last() else {
        return Ok(None);
    };
    let Some(section_index) = paragraph_index.checked_sub(1) else {
        return Ok(None);
    };
    let Some(siblings) = block_list_at_location_mut(blocks, location, parent_path) else {
        return Ok(None);
    };
    let Some(Block::Paragraph(paragraph)) = siblings.get(paragraph_index) else {
        return Ok(None);
    };
    let paragraph = paragraph.clone();
    let Some(Block::Environment(environment)) = siblings.get_mut(section_index) else {
        return Ok(None);
    };
    if environment.kind != caret_doc::BlockEnvironmentKind::Section
        || !environment.children.is_empty()
    {
        return Ok(None);
    }
    let Some(title) = environment.title.as_mut() else {
        return Ok(None);
    };
    if title.language != paragraph.language
        || title.layout != paragraph.layout
        || title.mark_style != paragraph.mark_style
        || title.props != paragraph.props
        || !title.extra.is_empty()
        || !paragraph.extra.is_empty()
    {
        return Err(EditError::IncompatibleParagraphMerge);
    }

    let left_len = title.inlines.iter().try_fold(0usize, |length, node| {
        length
            .checked_add(inline_length(node)?)
            .ok_or(EditError::InvalidPosition)
    })?;
    title.inlines.extend(paragraph.inlines.clone());
    let title_id = title.id.clone();
    let environment_id = environment.id.clone();
    let boundaries = TextMap::new(title)?.grapheme_boundaries;
    siblings.remove(paragraph_index);
    Ok(Some(SectionTitleMergeResult {
        merge: ParagraphMergeResult {
            left_id: title_id,
            right_id: paragraph.id,
            left_len,
            result_boundaries: boundaries,
        },
        environment_id,
    }))
}

/// 在同一直接块容器内原子合并一个段落范围，并删除两端指定的文字。
pub(super) fn merge_paragraph_range(
    blocks: &mut Vec<Block>,
    start: &ParagraphLocation,
    start_offset: usize,
    end: &ParagraphLocation,
    end_offset: usize,
) -> Result<ParagraphRangeMergeResult, EditError> {
    if start.is_title || end.is_title || start.story != end.story {
        return Err(EditError::CrossParagraphSelection);
    }
    let Some((&start_index, start_parent)) = start.block_path.split_last() else {
        return Err(EditError::CrossParagraphSelection);
    };
    let Some((&end_index, end_parent)) = end.block_path.split_last() else {
        return Err(EditError::CrossParagraphSelection);
    };
    if start_parent != end_parent || start_index >= end_index {
        return Err(EditError::CrossParagraphSelection);
    }
    let siblings = block_list_at_location(blocks, start, start_parent)
        .ok_or(EditError::CrossParagraphSelection)?;
    let selected = siblings
        .get(start_index..=end_index)
        .ok_or(EditError::CrossParagraphSelection)?;
    let paragraphs = selected
        .iter()
        .map(|block| match block {
            Block::Paragraph(paragraph) => Ok(paragraph),
            _ => Err(EditError::CrossParagraphSelection),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let first = paragraphs
        .first()
        .ok_or(EditError::CrossParagraphSelection)?;
    let last = paragraphs
        .last()
        .ok_or(EditError::CrossParagraphSelection)?;
    if paragraphs.iter().any(|paragraph| {
        paragraph.language != first.language
            || paragraph.layout != first.layout
            || paragraph.mark_style != first.mark_style
            || paragraph.props != first.props
    }) {
        return Err(EditError::IncompatibleParagraphMerge);
    }
    if paragraphs.iter().any(|paragraph| {
        !paragraph.extra.is_empty() || inline_tree_has_unknown_fields(&paragraph.inlines)
    }) {
        return Err(EditError::UnsupportedOpaqueContent);
    }

    let mut merged = (*first).clone();
    let first_map = TextMap::new(&merged)?;
    if start_offset > first_map.text.len() {
        return Err(EditError::InvalidPosition);
    }
    if start_offset < first_map.text.len() {
        first_map.delete_range(&mut merged, start_offset, first_map.text.len())?;
    }
    let mut suffix = (*last).clone();
    let last_map = TextMap::new(&suffix)?;
    if end_offset > last_map.text.len() {
        return Err(EditError::InvalidPosition);
    }
    if end_offset > 0 {
        last_map.delete_range(&mut suffix, 0, end_offset)?;
    }
    merged.inlines.extend(suffix.inlines);
    let result_boundaries = TextMap::new(&merged)?.grapheme_boundaries;
    let removed_ids = paragraphs
        .iter()
        .skip(1)
        .map(|paragraph| paragraph.id.clone())
        .collect();
    let paragraph_id = merged.id.clone();

    let siblings = block_list_at_location_mut(blocks, start, start_parent)
        .ok_or(EditError::CrossParagraphSelection)?;
    siblings.splice(start_index..=end_index, [Block::Paragraph(merged)]);
    Ok(ParagraphRangeMergeResult {
        paragraph_id,
        removed_ids,
        start_offset,
        end_offset,
        result_boundaries,
    })
}

fn inline_tree_has_unknown_fields(nodes: &[ParagraphContent]) -> bool {
    nodes.iter().any(|node| match node {
        FlowContent::Text(text) => {
            !text.extra.is_empty() || text.runs.iter().any(|run| !run.extra.is_empty())
        }
        FlowContent::Environment(environment) => {
            !environment.extra.is_empty() || inline_tree_has_unknown_fields(&environment.children)
        }
        FlowContent::SoftBreak(value) => !value.extra.is_empty(),
        FlowContent::Math(value) => !value.extra.is_empty(),
        FlowContent::Opaque(_) => true,
    })
}
