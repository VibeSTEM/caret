//! 实现段落定位、拆分、合并及块环境标题操作。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::text_map::{TextMap, inline_length};
use super::{Affinity, Direction, EditError, Pos, StoryId};
use caret_doc::{Block, InlineNode, NodeId, Paragraph};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(super) struct ParagraphLocation {
    pub(super) story: StoryId,
    pub(super) block_path: Vec<usize>,
    pub(super) is_title: bool,
}

impl ParagraphLocation {
    pub(super) fn body(block_path: Vec<usize>) -> Self {
        Self {
            story: StoryId::Body,
            block_path,
            is_title: false,
        }
    }
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
    collect_locations(blocks, &mut Vec::new(), &mut index);
    index
}

fn collect_locations(blocks: &[Block], parent_path: &mut Vec<usize>, index: &mut ParagraphIndex) {
    for (block_index, block) in blocks.iter().enumerate() {
        parent_path.push(block_index);
        match block {
            Block::Paragraph(paragraph) => {
                index.insert(
                    paragraph.id.clone(),
                    ParagraphLocation::body(parent_path.clone()),
                );
            }
            Block::Environment(environment) => {
                if let Some(title) = &environment.title {
                    index.insert(
                        title.id.clone(),
                        ParagraphLocation {
                            story: StoryId::EnvironmentTitle(environment.id.clone()),
                            block_path: parent_path.clone(),
                            is_title: true,
                        },
                    );
                }
                collect_locations(&environment.children, parent_path, index);
            }
            Block::ExplicitBreak(_) | Block::Opaque(_) | Block::Equation(_) | Block::Align(_) => {}
        }
        parent_path.pop();
    }
}

pub(super) fn update_index_after_split(
    blocks: &[Block],
    index: &mut ParagraphIndex,
    right_id: &NodeId,
    location: &ParagraphLocation,
) {
    if location.is_title {
        let Some(Block::Environment(environment)) = block_at_path(blocks, &location.block_path)
        else {
            return;
        };
        collect_locations_from(&environment.children, &location.block_path, 1, index);
        let mut right_path = location.block_path.clone();
        right_path.push(0);
        index.insert(right_id.clone(), ParagraphLocation::body(right_path));
    } else {
        let Some((&source_index, parent_path)) = location.block_path.split_last() else {
            return;
        };
        let Some(siblings) = block_list_at_path(blocks, parent_path) else {
            return;
        };
        collect_locations_from(siblings, parent_path, source_index + 2, index);
        let mut right_path = parent_path.to_vec();
        right_path.push(source_index + 1);
        index.insert(right_id.clone(), ParagraphLocation::body(right_path));
    }
}

pub(super) fn update_index_after_merge(
    blocks: &[Block],
    index: &mut ParagraphIndex,
    left_id: &NodeId,
    right_id: &NodeId,
) {
    let Some((left_index, parent_path)) = index
        .get(left_id)
        .and_then(|location| location.block_path.split_last())
        .map(|(&left_index, parent_path)| (left_index, parent_path.to_vec()))
    else {
        return;
    };
    let Some(siblings) = block_list_at_path(blocks, &parent_path) else {
        return;
    };
    collect_locations_from(siblings, &parent_path, left_index + 1, index);
    index.remove(right_id);
}

fn collect_locations_from(
    blocks: &[Block],
    parent_path: &[usize],
    start_index: usize,
    index: &mut ParagraphIndex,
) {
    for (block_index, block) in blocks.iter().enumerate().skip(start_index) {
        let mut path = parent_path.to_vec();
        path.push(block_index);
        collect_block_locations(block, &path, index);
    }
}

fn collect_block_locations(block: &Block, path: &[usize], index: &mut ParagraphIndex) {
    match block {
        Block::Paragraph(paragraph) => {
            index.insert(paragraph.id.clone(), ParagraphLocation::body(path.to_vec()));
        }
        Block::Environment(environment) => {
            if let Some(title) = &environment.title {
                index.insert(
                    title.id.clone(),
                    ParagraphLocation {
                        story: StoryId::EnvironmentTitle(environment.id.clone()),
                        block_path: path.to_vec(),
                        is_title: true,
                    },
                );
            }
            let mut child_path = path.to_vec();
            collect_locations(&environment.children, &mut child_path, index);
        }
        Block::ExplicitBreak(_) | Block::Opaque(_) | Block::Equation(_) | Block::Align(_) => {}
    }
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

fn block_at_path<'a>(blocks: &'a [Block], path: &[usize]) -> Option<&'a Block> {
    let (block_index, parent_path) = path.split_last()?;
    block_list_at_path(blocks, parent_path)?.get(*block_index)
}

pub(super) fn paragraph_at_location<'a>(
    blocks: &'a [Block],
    location: &ParagraphLocation,
) -> Option<&'a Paragraph> {
    let mut current = blocks;
    for (depth, block_index) in location.block_path.iter().enumerate() {
        let block = current.get(*block_index)?;
        if depth + 1 == location.block_path.len() {
            return match (block, location.is_title) {
                (Block::Paragraph(paragraph), false) => Some(paragraph),
                (Block::Environment(environment), true) => environment.title.as_ref(),
                _ => None,
            };
        }
        current = match block {
            Block::Environment(environment) => &environment.children,
            _ => return None,
        };
    }
    None
}

pub(super) fn paragraph_at_location_mut<'a>(
    blocks: &'a mut [Block],
    location: &ParagraphLocation,
) -> Option<&'a mut Paragraph> {
    fn visit<'a>(
        blocks: &'a mut [Block],
        path: &[usize],
        is_title: bool,
    ) -> Option<&'a mut Paragraph> {
        let (block_index, rest) = path.split_first()?;
        let block = blocks.get_mut(*block_index)?;
        if rest.is_empty() {
            return match (block, is_title) {
                (Block::Paragraph(paragraph), false) => Some(paragraph),
                (Block::Environment(environment), true) => environment.title.as_mut(),
                _ => None,
            };
        }
        match block {
            Block::Environment(environment) => visit(&mut environment.children, rest, is_title),
            _ => None,
        }
    }

    visit(blocks, &location.block_path, location.is_title)
}

pub(super) fn first_paragraph(blocks: &[Block]) -> Option<(&Paragraph, StoryId)> {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => return Some((paragraph, StoryId::Body)),
            Block::Environment(environment) => {
                if let Some(title) = &environment.title {
                    return Some((title, StoryId::EnvironmentTitle(environment.id.clone())));
                }
                if let Some(paragraph) = first_paragraph(&environment.children) {
                    return Some(paragraph);
                }
            }
            Block::ExplicitBreak(_) | Block::Opaque(_) | Block::Equation(_) | Block::Align(_) => {}
        }
    }
    None
}

pub(super) fn compare_positions(
    index: &ParagraphIndex,
    left: &Pos,
    right: &Pos,
) -> Result<std::cmp::Ordering, EditError> {
    let left_path = &index
        .get(&left.paragraph_id)
        .ok_or_else(|| EditError::UnknownParagraph(left.paragraph_id.to_string()))?
        .block_path;
    let right_path = &index
        .get(&right.paragraph_id)
        .ok_or_else(|| EditError::UnknownParagraph(right.paragraph_id.to_string()))?
        .block_path;
    Ok(left_path
        .cmp(right_path)
        .then_with(|| left.offset.cmp(&right.offset))
        .then_with(|| match (left.affinity, right.affinity) {
            (Affinity::Upstream, Affinity::Downstream) => std::cmp::Ordering::Less,
            (Affinity::Downstream, Affinity::Upstream) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        }))
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
    let Some(parent) = block_list_at_path_mut(blocks, parent_path) else {
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
    let Some(parent) = block_list_at_path_mut(blocks, parent_path) else {
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
    blocks: &mut [Block],
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
    let Some(environment) =
        block_at_path_mut(blocks, &location.block_path).and_then(|block| match block {
            Block::Environment(environment) if &environment.id == environment_id => {
                Some(environment)
            }
            _ => None,
        })
    else {
        return false;
    };
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
    let Some(blocks) = block_list_at_path_mut(blocks, parent_path) else {
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
    blocks: &mut [Block],
    location: &ParagraphLocation,
) -> Result<Option<SectionTitleMergeResult>, EditError> {
    if location.is_title || location.story != StoryId::Body {
        return Ok(None);
    }
    let Some((&index, parent_path)) = location.block_path.split_last() else {
        return Ok(None);
    };
    if index != 0 {
        return Ok(None);
    }
    let Some(Block::Environment(environment)) = block_at_path_mut(blocks, parent_path) else {
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
    let Some(siblings) = block_list_at_path_mut(blocks, parent_path) else {
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
    let siblings =
        block_list_at_path(blocks, start_parent).ok_or(EditError::CrossParagraphSelection)?;
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

    let siblings =
        block_list_at_path_mut(blocks, start_parent).ok_or(EditError::CrossParagraphSelection)?;
    siblings.splice(start_index..=end_index, [Block::Paragraph(merged)]);
    Ok(ParagraphRangeMergeResult {
        paragraph_id,
        removed_ids,
        start_offset,
        end_offset,
        result_boundaries,
    })
}

fn inline_tree_has_unknown_fields(nodes: &[InlineNode]) -> bool {
    nodes.iter().any(|node| match node {
        InlineNode::Run(run) => !run.extra.is_empty(),
        InlineNode::Environment(environment) => {
            !environment.extra.is_empty() || inline_tree_has_unknown_fields(&environment.children)
        }
        InlineNode::SoftBreak(value) => !value.extra.is_empty(),
        InlineNode::Math(value) => !value.extra.is_empty(),
        InlineNode::Opaque(_) => true,
    })
}

fn block_list_at_path_mut<'a>(
    mut blocks: &'a mut Vec<Block>,
    parent_path: &[usize],
) -> Option<&'a mut Vec<Block>> {
    for block_index in parent_path {
        let Block::Environment(environment) = blocks.get_mut(*block_index)? else {
            return None;
        };
        blocks = &mut environment.children;
    }
    Some(blocks)
}

fn block_at_path_mut<'a>(mut blocks: &'a mut [Block], path: &[usize]) -> Option<&'a mut Block> {
    for (depth, block_index) in path.iter().enumerate() {
        let block = blocks.get_mut(*block_index)?;
        if depth + 1 == path.len() {
            return Some(block);
        }
        let Block::Environment(environment) = block else {
            return None;
        };
        blocks = &mut environment.children;
    }
    None
}
