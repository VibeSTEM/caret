//! 管理会话的分支编辑历史及其局部文档补丁。

use super::super::paragraph_ops::{ParagraphIndex, ParagraphLocation, paragraph_at_location};
use super::super::{PositionMap, Selection, StoryId};
use caret_doc::{Block, BlockEnvironment, Document, NodeId, PageGeometry, Paragraph};
use caret_layout::VisualAffinity;
use std::mem::size_of;
use undo::{Edit, History, Merged};

const HISTORY_ACTION_LIMIT: usize = 256;
const HISTORY_MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const ESTIMATED_BRANCH_OVERHEAD_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ParagraphState {
    pub id: NodeId,
    pub paragraph: Paragraph,
    estimated_paragraph_bytes: usize,
    pub parent_environment_ids: Vec<NodeId>,
    pub sibling_index: usize,
    pub is_title: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum DocumentPatch {
    Paragraphs {
        before: Vec<ParagraphState>,
        after: Vec<ParagraphState>,
        merge_group: Option<EditGroup>,
        merge_epoch: u64,
    },
    PageGeometry {
        before: PageGeometry,
        after: PageGeometry,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditGroup {
    InsertText,
    DeleteBackward,
    DeleteForward,
    Composition,
}

#[derive(Debug, Clone)]
pub(super) struct SessionHistoryEdit {
    patch: DocumentPatch,
    before_selection: Selection,
    after_selection: Selection,
    before_visual_affinity: VisualAffinity,
    after_visual_affinity: VisualAffinity,
    before_state_id: u64,
    after_state_id: u64,
}

#[derive(Debug, Clone)]
pub(super) struct HistoryTransition {
    pub selection: Selection,
    pub visual_affinity: VisualAffinity,
    pub state_id: u64,
}

impl SessionHistoryEdit {
    pub fn new(
        patch: DocumentPatch,
        before_selection: Selection,
        after_selection: Selection,
        before_visual_affinity: VisualAffinity,
        after_visual_affinity: VisualAffinity,
        before_state_id: u64,
        after_state_id: u64,
    ) -> Self {
        Self {
            patch,
            before_selection,
            after_selection,
            before_visual_affinity,
            after_visual_affinity,
            before_state_id,
            after_state_id,
        }
    }

    pub(super) fn estimated_bytes(&self) -> usize {
        size_of::<Self>()
            + self.patch.estimated_bytes()
            + selection_id_bytes(&self.before_selection)
            + selection_id_bytes(&self.after_selection)
    }

    fn apply_before(&self, document: &mut Document) {
        self.patch.undo(document);
    }

    fn apply_after(&self, document: &mut Document) {
        self.patch.redo(document);
    }

    fn before_transition(&self) -> HistoryTransition {
        HistoryTransition {
            selection: self.before_selection.clone(),
            visual_affinity: self.before_visual_affinity,
            state_id: self.before_state_id,
        }
    }

    fn after_transition(&self) -> HistoryTransition {
        HistoryTransition {
            selection: self.after_selection.clone(),
            visual_affinity: self.after_visual_affinity,
            state_id: self.after_state_id,
        }
    }
}

impl Edit for SessionHistoryEdit {
    type Target = Document;
    type Output = HistoryTransition;

    // 首次记录时命令已修改文档；只登记历史，不能重复执行补丁。
    fn edit(&mut self, _: &mut Document) -> Self::Output {
        self.after_transition()
    }

    fn undo(&mut self, document: &mut Document) -> Self::Output {
        self.apply_before(document);
        self.before_transition()
    }

    fn redo(&mut self, document: &mut Document) -> Self::Output {
        self.apply_after(document);
        self.after_transition()
    }

    fn merge(&mut self, other: Self) -> Merged<Self> {
        let (
            DocumentPatch::Paragraphs {
                after: current_after,
                merge_group: current_group,
                merge_epoch: current_epoch,
                ..
            },
            DocumentPatch::Paragraphs {
                before: next_before,
                after: next_after,
                merge_group: next_group,
                merge_epoch: next_epoch,
            },
        ) = (&mut self.patch, &other.patch)
        else {
            return Merged::No(other);
        };
        if current_group.is_none()
            || current_group != next_group
            || current_epoch != next_epoch
            || self.after_state_id != other.before_state_id
            || self.after_selection != other.before_selection
            || self.after_visual_affinity != other.before_visual_affinity
            || self.before_selection.anchor == self.before_selection.focus
                && other.before_selection.anchor != other.before_selection.focus
            || current_after.len() != 1
            || next_before.len() != 1
            || next_after.len() != 1
            || current_after[0].id != next_before[0].id
            || current_after[0].parent_environment_ids != next_before[0].parent_environment_ids
            || current_after[0].sibling_index != next_before[0].sibling_index
        {
            return Merged::No(other);
        }
        *current_after = next_after.clone();
        self.after_selection = other.after_selection;
        self.after_visual_affinity = other.after_visual_affinity;
        self.after_state_id = other.after_state_id;
        Merged::Yes
    }
}

impl DocumentPatch {
    pub fn estimated_bytes(&self) -> usize {
        match self {
            Self::Paragraphs { before, after, .. } => before
                .iter()
                .chain(after)
                .map(ParagraphState::estimated_bytes)
                .sum(),
            Self::PageGeometry { .. } => size_of::<PageGeometry>(),
        }
    }

    fn undo(&self, document: &mut Document) {
        match self {
            Self::Paragraphs { before, after, .. } => {
                apply_paragraph_states(document, after, before)
            }
            Self::PageGeometry { before, .. } => set_page_geometry(document, *before),
        }
    }

    fn redo(&self, document: &mut Document) {
        match self {
            Self::Paragraphs { before, after, .. } => {
                apply_paragraph_states(document, before, after)
            }
            Self::PageGeometry { after, .. } => set_page_geometry(document, *after),
        }
    }
}

impl ParagraphState {
    pub fn capture(
        document: &Document,
        paragraph_index: &ParagraphIndex,
        paragraph_id: &NodeId,
    ) -> Option<Self> {
        let location = paragraph_index.get(paragraph_id)?;
        let paragraph = paragraph_at_location(&document.body, location)?.clone();
        let estimated_paragraph_bytes = paragraph_estimated_bytes(&paragraph);
        let (parent_environment_ids, sibling_index) =
            parent_environment_path(&document.body, location)?;
        Some(Self {
            id: paragraph_id.clone(),
            paragraph,
            estimated_paragraph_bytes,
            parent_environment_ids,
            sibling_index,
            is_title: location.is_title,
        })
    }

    pub(super) fn estimated_bytes(&self) -> usize {
        size_of::<Self>()
            + node_id_bytes(&self.id)
            + self
                .parent_environment_ids
                .iter()
                .map(node_id_bytes)
                .sum::<usize>()
            + self.parent_environment_ids.len() * size_of::<NodeId>()
            + self.estimated_paragraph_bytes
    }
}

pub(super) fn history() -> History<SessionHistoryEdit> {
    History::builder()
        .limit(HISTORY_ACTION_LIMIT)
        .saved(true)
        .build()
}

pub(super) fn state_ids_from_position_map(map: &PositionMap, output: &mut Vec<NodeId>) {
    match map {
        PositionMap::Composite(maps) => {
            for map in maps {
                state_ids_from_position_map(map, output);
            }
        }
        PositionMap::TextEdit { paragraph_id, .. } => output.push(paragraph_id.clone()),
        PositionMap::ParagraphSplit {
            source_id,
            right_id,
            ..
        } => {
            output.push(source_id.clone());
            output.push(right_id.clone());
        }
        PositionMap::ParagraphMerge {
            left_id, right_id, ..
        } => {
            output.push(left_id.clone());
            output.push(right_id.clone());
        }
        PositionMap::ParagraphRangeMerge {
            paragraph_id,
            end_paragraph_id,
            removed_ids,
            ..
        } => {
            output.push(paragraph_id.clone());
            output.push(end_paragraph_id.clone());
            output.extend(removed_ids.iter().cloned());
        }
    }
}

fn apply_paragraph_states(
    document: &mut Document,
    current: &[ParagraphState],
    target: &[ParagraphState],
) {
    for state in current {
        remove_paragraph(&mut document.body, &state.id);
    }
    let mut titles = target
        .iter()
        .filter(|state| state.is_title)
        .collect::<Vec<_>>();
    for state in titles.drain(..) {
        let Some(environment_id) = state.parent_environment_ids.last() else {
            continue;
        };
        if let Some(environment) = find_environment_mut(&mut document.body, environment_id) {
            environment.title = Some(state.paragraph.clone());
        }
    }
    let mut paragraphs = target
        .iter()
        .filter(|state| !state.is_title)
        .collect::<Vec<_>>();
    paragraphs.sort_by(|left, right| {
        left.parent_environment_ids
            .cmp(&right.parent_environment_ids)
            .then_with(|| left.sibling_index.cmp(&right.sibling_index))
    });
    for state in paragraphs {
        let Some(blocks) = block_list_mut(&mut document.body, &state.parent_environment_ids) else {
            continue;
        };
        let index = state.sibling_index.min(blocks.len());
        blocks.insert(index, Block::Paragraph(state.paragraph.clone()));
    }
}

fn remove_paragraph(blocks: &mut Vec<Block>, paragraph_id: &NodeId) -> bool {
    let mut index = 0;
    while index < blocks.len() {
        match &mut blocks[index] {
            Block::Paragraph(paragraph) if &paragraph.id == paragraph_id => {
                blocks.remove(index);
                return true;
            }
            Block::Environment(environment) => {
                if environment
                    .title
                    .as_ref()
                    .is_some_and(|title| &title.id == paragraph_id)
                {
                    environment.title = None;
                    return true;
                }
                if remove_paragraph(&mut environment.children, paragraph_id) {
                    return true;
                }
            }
            _ => {}
        }
        index += 1;
    }
    false
}

fn block_list_mut<'a>(
    blocks: &'a mut Vec<Block>,
    environment_ids: &[NodeId],
) -> Option<&'a mut Vec<Block>> {
    let Some((environment_id, rest)) = environment_ids.split_first() else {
        return Some(blocks);
    };
    let environment = find_environment_mut(blocks, environment_id)?;
    block_list_mut(&mut environment.children, rest)
}

fn find_environment_mut<'a>(
    blocks: &'a mut [Block],
    environment_id: &NodeId,
) -> Option<&'a mut BlockEnvironment> {
    for block in blocks {
        if let Block::Environment(environment) = block {
            if &environment.id == environment_id {
                return Some(environment);
            }
            if let Some(found) = find_environment_mut(&mut environment.children, environment_id) {
                return Some(found);
            }
        }
    }
    None
}

fn parent_environment_path(
    blocks: &[Block],
    location: &ParagraphLocation,
) -> Option<(Vec<NodeId>, usize)> {
    let mut environment_ids = Vec::new();
    let count = if location.is_title {
        location.block_path.len()
    } else {
        location.block_path.len().checked_sub(1)?
    };
    let mut current = blocks;
    for block_index in location.block_path.iter().take(count) {
        let Block::Environment(environment) = current.get(*block_index)? else {
            return None;
        };
        environment_ids.push(environment.id.clone());
        current = &environment.children;
    }
    let sibling_index = if location.is_title {
        0
    } else {
        *location.block_path.last()?
    };
    Some((environment_ids, sibling_index))
}

fn set_page_geometry(document: &mut Document, geometry: PageGeometry) {
    if let Some(section) = document.page_sections.first_mut() {
        let _ = section.set_geometry(geometry);
    }
}

fn paragraph_estimated_bytes(paragraph: &Paragraph) -> usize {
    size_of::<Paragraph>()
        + node_id_bytes(&paragraph.id)
        + paragraph.language.as_ref().map_or(0, String::len)
        + paragraph
            .inlines
            .iter()
            .map(inline_estimated_bytes)
            .sum::<usize>()
        + string_value_map_bytes(&paragraph.props)
        + string_value_map_bytes(&paragraph.mark_style)
        + string_value_map_bytes(&paragraph.extra)
        + paragraph.layout.as_ref().map_or(0, |layout| {
            size_of_val(layout) + string_value_map_bytes(&layout.extra)
        })
}

fn inline_estimated_bytes(node: &caret_doc::InlineNode) -> usize {
    use caret_doc::InlineNode;
    size_of::<InlineNode>()
        + match node {
            InlineNode::Run(run) => {
                size_of_val(run)
                    + run.language.as_ref().map_or(0, String::len)
                    + run.text.len()
                    + string_value_map_bytes(&run.props)
                    + string_value_map_bytes(&run.extra)
            }
            InlineNode::Environment(environment) => {
                size_of_val(environment)
                    + node_id_bytes(&environment.id)
                    + environment.definition_id.as_ref().map_or(0, String::len)
                    + string_value_map_bytes(&environment.props)
                    + string_value_map_bytes(&environment.extra)
                    + environment
                        .children
                        .iter()
                        .map(inline_estimated_bytes)
                        .sum::<usize>()
            }
            InlineNode::SoftBreak(value) => {
                size_of_val(value) + string_value_map_bytes(&value.extra)
            }
            InlineNode::Opaque(value) => size_of_val(value) + value.raw.to_string().len(),
        }
}

pub(super) fn history_memory_limit() -> usize {
    HISTORY_MEMORY_LIMIT
}

pub(super) fn estimated_history_bytes(history: &History<SessionHistoryEdit>) -> usize {
    let current_entries = history
        .entries()
        .map(estimated_entry_bytes)
        .fold(0usize, usize::saturating_add);
    let branch_bytes = history
        .branches()
        .map(|(_, branch)| {
            branch
                .entries()
                .map(estimated_entry_bytes)
                .fold(ESTIMATED_BRANCH_OVERHEAD_BYTES, usize::saturating_add)
        })
        .fold(0usize, usize::saturating_add);
    // 额外系数覆盖分配器节点、容器容量和非 payload 的树形索引开销。
    size_of::<History<SessionHistoryEdit>>()
        .saturating_add(current_entries)
        .saturating_add(branch_bytes)
        .saturating_mul(2)
}

fn estimated_entry_bytes(entry: &undo::Entry<SessionHistoryEdit>) -> usize {
    size_of::<undo::Entry<SessionHistoryEdit>>().saturating_add(entry.as_ref().estimated_bytes())
}

pub(super) fn paragraph_states_for_ids(
    document: &Document,
    paragraph_index: &ParagraphIndex,
    ids: impl IntoIterator<Item = NodeId>,
) -> Vec<ParagraphState> {
    let mut seen = std::collections::HashSet::new();
    let mut states = ids
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .filter_map(|id| ParagraphState::capture(document, paragraph_index, &id))
        .collect::<Vec<_>>();
    states.sort_by(|left, right| {
        left.parent_environment_ids
            .cmp(&right.parent_environment_ids)
            .then_with(|| left.sibling_index.cmp(&right.sibling_index))
    });
    states
}

pub(super) fn edit_group(command: &super::super::Command) -> Option<EditGroup> {
    match command {
        super::super::Command::InsertText(text)
            if !text.is_empty() && !text.contains(['\n', '\r']) =>
        {
            Some(EditGroup::InsertText)
        }
        super::super::Command::Delete(super::super::Direction::Backward) => {
            Some(EditGroup::DeleteBackward)
        }
        super::super::Command::Delete(super::super::Direction::Forward) => {
            Some(EditGroup::DeleteForward)
        }
        super::super::Command::CommitComposition { .. } => Some(EditGroup::Composition),
        _ => None,
    }
}

pub(super) fn before_selection_ids(
    paragraph_index: &ParagraphIndex,
    selection: &Selection,
) -> Vec<NodeId> {
    if selection.anchor.paragraph_id == selection.focus.paragraph_id {
        return vec![selection.focus.paragraph_id.clone()];
    }
    let Some(anchor) = paragraph_index.get(&selection.anchor.paragraph_id) else {
        return Vec::new();
    };
    let Some(focus) = paragraph_index.get(&selection.focus.paragraph_id) else {
        return Vec::new();
    };
    let (start, end) = if anchor.block_path <= focus.block_path {
        (&anchor.block_path, &focus.block_path)
    } else {
        (&focus.block_path, &anchor.block_path)
    };
    let mut ordered = paragraph_index.iter().collect::<Vec<_>>();
    ordered.sort_by(|(_, left), (_, right)| left.block_path.cmp(&right.block_path));
    ordered
        .into_iter()
        .filter(|(_, location)| location.block_path >= *start && location.block_path <= *end)
        .map(|(id, _)| id.clone())
        .collect()
}

fn node_id_bytes(id: &NodeId) -> usize {
    id.as_str().len()
}

fn string_value_map_bytes(values: &std::collections::BTreeMap<String, serde_json::Value>) -> usize {
    values
        .iter()
        .map(|(key, value)| size_of::<String>() + key.len() + json_value_bytes(value))
        .sum()
}

fn json_value_bytes(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            size_of::<serde_json::Value>()
        }
        serde_json::Value::String(text) => size_of::<String>() + text.len(),
        serde_json::Value::Array(values) => {
            size_of::<Vec<serde_json::Value>>()
                + values.len() * size_of::<serde_json::Value>()
                + values.iter().map(json_value_bytes).sum::<usize>()
        }
        serde_json::Value::Object(values) => {
            size_of::<serde_json::Map<String, serde_json::Value>>()
                + values
                    .iter()
                    .map(|(key, value)| size_of::<String>() + key.len() + json_value_bytes(value))
                    .sum::<usize>()
        }
    }
}

fn selection_id_bytes(selection: &Selection) -> usize {
    node_id_bytes(&selection.anchor.paragraph_id)
        + node_id_bytes(&selection.focus.paragraph_id)
        + story_id_bytes(&selection.anchor.story)
        + story_id_bytes(&selection.focus.story)
}

fn story_id_bytes(story: &StoryId) -> usize {
    match story {
        StoryId::Body => 0,
        StoryId::EnvironmentTitle(id) => node_id_bytes(id),
    }
}

pub(super) fn include_adjacent_paragraph_id(
    paragraph_index: &ParagraphIndex,
    current_id: &NodeId,
    direction: super::super::Direction,
) -> Option<NodeId> {
    let current = paragraph_index.get(current_id)?;
    if current.is_title {
        return None;
    }
    let (&index, parent_path) = current.block_path.split_last()?;
    let target_index = match direction {
        super::super::Direction::Backward => index.checked_sub(1)?,
        super::super::Direction::Forward => index.checked_add(1)?,
    };
    paragraph_index.iter().find_map(|(id, location)| {
        (location.story == StoryId::Body
            && location.block_path.len() == parent_path.len() + 1
            && location.block_path[..parent_path.len()] == *parent_path
            && location.block_path.last() == Some(&target_index))
        .then(|| id.clone())
    })
}

pub(super) fn restore_history_transition(
    transition: HistoryTransition,
) -> (Selection, VisualAffinity, u64) {
    (
        transition.selection,
        transition.visual_affinity,
        transition.state_id,
    )
}
