//! 管理会话的分支编辑历史及其局部文档补丁。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::paragraph_ops::{ParagraphIndex, ParagraphLocation, paragraph_at_location};
use super::super::{PositionMap, Selection, StoryId};
use super::formatting::PendingInlineStyle;
use super::math_edit::MathEditTarget;
use caret_doc::{
    Align, Block, BlockEnvironment, BlockEnvironmentKind, Document, DocumentFontTarget,
    DocumentSettings, Equation, MathNode, MathTree, NodeId, PageGeometry, Paragraph,
};
use caret_layout::VisualAffinity;
use std::mem::size_of;
use undo::{Edit, History, Merged};

const HISTORY_ACTION_LIMIT: usize = 256;
const HISTORY_MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const ESTIMATED_BRANCH_OVERHEAD_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LatinFontFamilyState {
    default_family: Option<String>,
    legacy_script_entries: Vec<(String, String)>,
}

impl LatinFontFamilyState {
    pub(super) fn capture(settings: &DocumentSettings) -> Self {
        Self {
            default_family: settings.default_font_family.clone(),
            legacy_script_entries: settings
                .script_font_families
                .iter()
                .filter(|(script, _)| script.eq_ignore_ascii_case("Latn"))
                .map(|(script, family)| (script.clone(), family.clone()))
                .collect(),
        }
    }

    fn restore(&self, settings: &mut DocumentSettings) {
        settings
            .default_font_family
            .clone_from(&self.default_family);
        settings
            .script_font_families
            .retain(|script, _| !script.eq_ignore_ascii_case("Latn"));
        settings
            .script_font_families
            .extend(self.legacy_script_entries.iter().cloned());
    }

    fn estimated_bytes(&self) -> usize {
        self.default_family.as_ref().map_or(0, String::capacity)
            + self
                .legacy_script_entries
                .iter()
                .map(|(script, family)| script.capacity() + family.capacity())
                .sum::<usize>()
            + size_of::<Self>()
    }
}

fn restore_document_font_family(
    document: &mut Document,
    target: &DocumentFontTarget,
    family: Option<String>,
) {
    // 撤销记录只由已校验的编辑命令创建，重放时目标应始终有效。
    let restored = document.global_settings.set_font_family(target, family);
    debug_assert!(restored.is_ok(), "history contains an invalid font target");
}

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
    PendingStyle,
    Compound(Vec<DocumentPatch>),
    Paragraphs {
        before: Vec<ParagraphState>,
        after: Vec<ParagraphState>,
        merge_group: Option<EditGroup>,
        merge_epoch: u64,
    },
    ParagraphInsertion {
        parent_environment_ids: Vec<NodeId>,
        sibling_index: usize,
        paragraph: Box<Paragraph>,
    },
    PageGeometry {
        before: PageGeometry,
        after: PageGeometry,
    },
    LatinFontFamily {
        before: LatinFontFamilyState,
        after: LatinFontFamilyState,
    },
    DocumentFontFamily {
        target: DocumentFontTarget,
        before: Option<String>,
        after: Option<String>,
    },
    EnvironmentCreation {
        parent_environment_ids: Vec<NodeId>,
        sibling_index: usize,
        paragraph: Box<Paragraph>,
        paragraph_estimated_bytes: usize,
        environment_id: NodeId,
        kind: BlockEnvironmentKind,
    },
    EnvironmentInsertion {
        parent_environment_ids: Vec<NodeId>,
        sibling_index: usize,
        environment: Box<BlockEnvironment>,
        environment_estimated_bytes: usize,
    },
    EnvironmentBoundaryParagraphs {
        parent_environment_ids: Vec<NodeId>,
        sibling_index: usize,
        paragraphs: Vec<Paragraph>,
        paragraphs_estimated_bytes: usize,
    },
    EnvironmentDeletion {
        parent_environment_ids: Vec<NodeId>,
        sibling_index: usize,
        environment: Box<BlockEnvironment>,
        environment_estimated_bytes: usize,
        fallback_paragraph: Option<Box<Paragraph>>,
    },
    EnvironmentUnwrap {
        parent_environment_ids: Vec<NodeId>,
        sibling_index: usize,
        environment: Box<BlockEnvironment>,
        environment_estimated_bytes: usize,
    },
    /// 同一父级下一段连续块的整体替换；用于插入或修改显示公式、对齐公式块。
    BlockRange {
        parent_environment_ids: Vec<NodeId>,
        sibling_index: usize,
        before: Vec<Block>,
        after: Vec<Block>,
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
    before: SessionHistoryState,
    after: SessionHistoryState,
}

#[derive(Debug, Clone)]
pub(super) struct SessionHistoryState {
    selection: Selection,
    visual_affinity: VisualAffinity,
    pending_style: PendingInlineStyle,
    math_edit: Option<MathEditPosition>,
    state_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MathEditPosition {
    pub(super) target: MathEditTarget,
    pub(super) cursor: usize,
}

impl MathEditPosition {
    pub(super) fn new(target: MathEditTarget, cursor: usize) -> Self {
        Self { target, cursor }
    }

    fn estimated_bytes(&self) -> usize {
        size_of::<Self>()
            + match &self.target {
                MathEditTarget::Inline {
                    paragraph_id,
                    math_id,
                } => node_id_bytes(paragraph_id) + node_id_bytes(math_id),
                MathEditTarget::Block { block_id, .. } => node_id_bytes(block_id),
            }
    }
}

impl SessionHistoryState {
    pub(super) fn new(
        selection: Selection,
        visual_affinity: VisualAffinity,
        pending_style: PendingInlineStyle,
        state_id: u64,
    ) -> Self {
        Self {
            selection,
            visual_affinity,
            pending_style,
            math_edit: None,
            state_id,
        }
    }

    pub(super) fn with_math_edit(mut self, math_edit: Option<MathEditPosition>) -> Self {
        self.math_edit = math_edit;
        self
    }
}

#[derive(Debug, Clone)]
pub(super) struct HistoryTransition {
    pub selection: Selection,
    pub visual_affinity: VisualAffinity,
    pub pending_style: PendingInlineStyle,
    pub math_edit: Option<MathEditPosition>,
    pub state_id: u64,
    pub changed_paragraph_id: Option<NodeId>,
    pub document_changed: bool,
}

impl SessionHistoryEdit {
    pub fn new(
        patch: DocumentPatch,
        before: SessionHistoryState,
        after: SessionHistoryState,
    ) -> Self {
        Self {
            patch,
            before,
            after,
        }
    }

    pub(super) fn estimated_bytes(&self) -> usize {
        size_of::<Self>()
            + self.patch.estimated_bytes()
            + selection_id_bytes(&self.before.selection)
            + selection_id_bytes(&self.after.selection)
            + self
                .before
                .math_edit
                .as_ref()
                .map_or(0, MathEditPosition::estimated_bytes)
            + self
                .after
                .math_edit
                .as_ref()
                .map_or(0, MathEditPosition::estimated_bytes)
    }

    fn apply_before(&self, document: &mut Document) {
        self.patch.undo(document);
    }

    fn apply_after(&self, document: &mut Document) {
        self.patch.redo(document);
    }

    fn before_transition(&self) -> HistoryTransition {
        HistoryTransition {
            selection: self.before.selection.clone(),
            visual_affinity: self.before.visual_affinity,
            pending_style: self.before.pending_style.clone(),
            math_edit: self.before.math_edit.clone(),
            state_id: self.before.state_id,
            changed_paragraph_id: self.patch.single_changed_paragraph_id(),
            document_changed: self.patch.affects_document(),
        }
    }

    fn after_transition(&self) -> HistoryTransition {
        HistoryTransition {
            selection: self.after.selection.clone(),
            visual_affinity: self.after.visual_affinity,
            pending_style: self.after.pending_style.clone(),
            math_edit: self.after.math_edit.clone(),
            state_id: self.after.state_id,
            changed_paragraph_id: self.patch.single_changed_paragraph_id(),
            document_changed: self.patch.affects_document(),
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
            || self.after.state_id != other.before.state_id
            || self.after.selection != other.before.selection
            || self.after.visual_affinity != other.before.visual_affinity
            || self.after.math_edit != other.before.math_edit
            || self.before.selection.anchor == self.before.selection.focus
                && other.before.selection.anchor != other.before.selection.focus
            || current_after.len() != 1
            || next_before.len() != 1
            || next_after.len() != 1
            || current_after[0].id != next_before[0].id
            || current_after[0].parent_environment_ids != next_before[0].parent_environment_ids
            || current_after[0].sibling_index != next_before[0].sibling_index
        {
            return Merged::No(other);
        }
        let DocumentPatch::Paragraphs {
            after: next_after, ..
        } = other.patch
        else {
            unreachable!("merge compatibility was checked above");
        };
        *current_after = next_after;
        self.after = other.after;
        Merged::Yes
    }
}

impl DocumentPatch {
    fn affects_document(&self) -> bool {
        !matches!(self, Self::PendingStyle)
    }

    pub(super) fn single_changed_paragraph_id(&self) -> Option<NodeId> {
        let Self::Paragraphs { before, after, .. } = self else {
            return None;
        };
        if before.len() != 1 || after.len() != 1 || before[0].id != after[0].id {
            return None;
        }
        Some(before[0].id.clone())
    }

    pub fn estimated_bytes(&self) -> usize {
        match self {
            Self::PendingStyle => size_of::<Self>(),
            Self::Compound(patches) => {
                patches.capacity() * size_of::<DocumentPatch>()
                    + patches.iter().map(Self::estimated_bytes).sum::<usize>()
            }
            Self::Paragraphs { before, after, .. } => before
                .iter()
                .chain(after)
                .map(ParagraphState::estimated_bytes)
                .sum(),
            Self::ParagraphInsertion {
                parent_environment_ids,
                paragraph,
                ..
            } => {
                parent_environment_ids
                    .iter()
                    .map(node_id_bytes)
                    .sum::<usize>()
                    + paragraph_estimated_bytes(paragraph)
                    + size_of::<Box<Paragraph>>()
            }
            Self::PageGeometry { .. } => size_of::<PageGeometry>(),
            Self::LatinFontFamily { before, after } => {
                before.estimated_bytes() + after.estimated_bytes()
            }
            Self::DocumentFontFamily { before, after, .. } => {
                before.as_ref().map_or(0, String::capacity)
                    + after.as_ref().map_or(0, String::capacity)
                    + size_of::<Option<String>>() * 2
            }
            Self::EnvironmentCreation {
                parent_environment_ids,
                paragraph_estimated_bytes,
                environment_id,
                ..
            } => {
                parent_environment_ids
                    .iter()
                    .map(node_id_bytes)
                    .sum::<usize>()
                    + paragraph_estimated_bytes
                    + size_of::<Box<Paragraph>>()
                    + node_id_bytes(environment_id)
            }
            Self::BlockRange {
                parent_environment_ids,
                before,
                after,
                ..
            } => {
                parent_environment_ids
                    .iter()
                    .map(node_id_bytes)
                    .sum::<usize>()
                    + before
                        .iter()
                        .chain(after)
                        .map(block_estimated_bytes)
                        .sum::<usize>()
            }
            Self::EnvironmentInsertion {
                parent_environment_ids,
                environment_estimated_bytes,
                ..
            } => {
                parent_environment_ids
                    .iter()
                    .map(node_id_bytes)
                    .sum::<usize>()
                    + environment_estimated_bytes
            }
            Self::EnvironmentBoundaryParagraphs {
                parent_environment_ids,
                paragraphs,
                paragraphs_estimated_bytes,
                ..
            } => {
                parent_environment_ids
                    .iter()
                    .map(node_id_bytes)
                    .sum::<usize>()
                    + paragraphs_estimated_bytes
                    + paragraphs.len() * size_of::<Paragraph>()
            }
            Self::EnvironmentDeletion {
                parent_environment_ids,
                environment_estimated_bytes,
                fallback_paragraph,
                ..
            } => {
                parent_environment_ids
                    .iter()
                    .map(node_id_bytes)
                    .sum::<usize>()
                    + environment_estimated_bytes
                    + fallback_paragraph.as_ref().map_or(0, |paragraph| {
                        paragraph_estimated_bytes(paragraph) + size_of::<Box<Paragraph>>()
                    })
            }
            Self::EnvironmentUnwrap {
                parent_environment_ids,
                environment_estimated_bytes,
                ..
            } => {
                parent_environment_ids
                    .iter()
                    .map(node_id_bytes)
                    .sum::<usize>()
                    + environment_estimated_bytes
            }
        }
    }

    pub(super) fn undo(&self, document: &mut Document) {
        match self {
            Self::PendingStyle => {}
            Self::Compound(patches) => {
                for patch in patches.iter().rev() {
                    patch.undo(document);
                }
            }
            Self::Paragraphs { before, after, .. } => {
                apply_paragraph_states(document, after, before)
            }
            Self::ParagraphInsertion {
                parent_environment_ids,
                sibling_index,
                paragraph,
            } => apply_paragraph_insertion(
                document,
                parent_environment_ids,
                *sibling_index,
                paragraph,
                false,
            ),
            Self::PageGeometry { before, .. } => set_page_geometry(document, *before),
            Self::LatinFontFamily { before, .. } => {
                before.restore(&mut document.global_settings);
            }
            Self::DocumentFontFamily { target, before, .. } => {
                restore_document_font_family(document, target, before.clone());
            }
            Self::EnvironmentCreation {
                parent_environment_ids,
                sibling_index,
                paragraph,
                environment_id,
                kind,
                ..
            } => apply_environment_creation(
                document,
                parent_environment_ids,
                *sibling_index,
                paragraph,
                environment_id,
                *kind,
                false,
            ),
            Self::BlockRange {
                parent_environment_ids,
                sibling_index,
                before,
                after,
            } => apply_block_range(
                document,
                parent_environment_ids,
                *sibling_index,
                after,
                before,
            ),
            Self::EnvironmentInsertion {
                parent_environment_ids,
                sibling_index,
                environment,
                ..
            } => apply_environment_insertion(
                document,
                parent_environment_ids,
                *sibling_index,
                environment,
                false,
            ),
            Self::EnvironmentBoundaryParagraphs {
                parent_environment_ids,
                sibling_index,
                paragraphs,
                ..
            } => apply_environment_boundary_paragraphs(
                document,
                parent_environment_ids,
                *sibling_index,
                paragraphs,
                false,
            ),
            Self::EnvironmentDeletion {
                parent_environment_ids,
                sibling_index,
                environment,
                fallback_paragraph,
                ..
            } => apply_environment_deletion(
                document,
                parent_environment_ids,
                *sibling_index,
                environment,
                fallback_paragraph.as_deref(),
                true,
            ),
            Self::EnvironmentUnwrap {
                parent_environment_ids,
                sibling_index,
                environment,
                ..
            } => apply_environment_unwrap(
                document,
                parent_environment_ids,
                *sibling_index,
                environment,
                true,
            ),
        }
    }

    pub(super) fn redo(&self, document: &mut Document) {
        match self {
            Self::PendingStyle => {}
            Self::Compound(patches) => {
                for patch in patches {
                    patch.redo(document);
                }
            }
            Self::Paragraphs { before, after, .. } => {
                apply_paragraph_states(document, before, after)
            }
            Self::ParagraphInsertion {
                parent_environment_ids,
                sibling_index,
                paragraph,
            } => apply_paragraph_insertion(
                document,
                parent_environment_ids,
                *sibling_index,
                paragraph,
                true,
            ),
            Self::PageGeometry { after, .. } => set_page_geometry(document, *after),
            Self::LatinFontFamily { after, .. } => {
                after.restore(&mut document.global_settings);
            }
            Self::DocumentFontFamily { target, after, .. } => {
                restore_document_font_family(document, target, after.clone());
            }
            Self::EnvironmentCreation {
                parent_environment_ids,
                sibling_index,
                paragraph,
                environment_id,
                kind,
                ..
            } => apply_environment_creation(
                document,
                parent_environment_ids,
                *sibling_index,
                paragraph,
                environment_id,
                *kind,
                true,
            ),
            Self::BlockRange {
                parent_environment_ids,
                sibling_index,
                before,
                after,
            } => apply_block_range(
                document,
                parent_environment_ids,
                *sibling_index,
                before,
                after,
            ),
            Self::EnvironmentInsertion {
                parent_environment_ids,
                sibling_index,
                environment,
                ..
            } => apply_environment_insertion(
                document,
                parent_environment_ids,
                *sibling_index,
                environment,
                true,
            ),
            Self::EnvironmentBoundaryParagraphs {
                parent_environment_ids,
                sibling_index,
                paragraphs,
                ..
            } => apply_environment_boundary_paragraphs(
                document,
                parent_environment_ids,
                *sibling_index,
                paragraphs,
                true,
            ),
            Self::EnvironmentDeletion {
                parent_environment_ids,
                sibling_index,
                environment,
                fallback_paragraph,
                ..
            } => apply_environment_deletion(
                document,
                parent_environment_ids,
                *sibling_index,
                environment,
                fallback_paragraph.as_deref(),
                false,
            ),
            Self::EnvironmentUnwrap {
                parent_environment_ids,
                sibling_index,
                environment,
                ..
            } => apply_environment_unwrap(
                document,
                parent_environment_ids,
                *sibling_index,
                environment,
                false,
            ),
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
        PositionMap::StoryChange { paragraph_id, .. } => output.push(paragraph_id.clone()),
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
    for state in target.iter().filter(|state| state.is_title) {
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

fn apply_paragraph_insertion(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    paragraph: &Paragraph,
    insert: bool,
) {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return;
    };
    if insert {
        if blocks
            .iter()
            .any(|block| matches!(block, Block::Paragraph(existing) if existing.id == paragraph.id))
        {
            return;
        }
        if sibling_index <= blocks.len() {
            blocks.insert(sibling_index, Block::Paragraph(paragraph.clone()));
        }
    } else if let Some(index) = blocks.iter().position(
        |block| matches!(block, Block::Paragraph(existing) if existing.id == paragraph.id),
    ) {
        blocks.remove(index);
    }
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

pub(super) fn parent_environment_path(
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

fn apply_environment_creation(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    paragraph: &Paragraph,
    environment_id: &NodeId,
    kind: BlockEnvironmentKind,
    create: bool,
) {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return;
    };
    let Some(current) = blocks.get(sibling_index) else {
        return;
    };
    let expected = if create {
        matches!(current, Block::Paragraph(value) if value.id == paragraph.id)
    } else {
        matches!(current, Block::Environment(value) if &value.id == environment_id)
    };
    if !expected {
        return;
    }
    blocks[sibling_index] = if create {
        Block::Environment(BlockEnvironment {
            id: environment_id.clone(),
            kind,
            definition_id: None,
            props: Default::default(),
            title: (kind == BlockEnvironmentKind::Section).then(|| paragraph.clone()),
            children: if kind == BlockEnvironmentKind::Custom {
                vec![Block::Paragraph(paragraph.clone())]
            } else {
                Vec::new()
            },
            extra: Default::default(),
        })
    } else {
        Block::Paragraph(paragraph.clone())
    };
}

fn apply_environment_insertion(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    environment: &BlockEnvironment,
    insert: bool,
) {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return;
    };
    if insert {
        if sibling_index <= blocks.len() {
            blocks.insert(sibling_index, Block::Environment(environment.clone()));
        }
    } else if matches!(blocks.get(sibling_index), Some(Block::Environment(value)) if value.id == environment.id)
    {
        blocks.remove(sibling_index);
    }
}

/// 把 `current` 所在的连续块替换为 `replacement`；位置或内容不符时不改文档。
fn apply_block_range(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    current: &[Block],
    replacement: &[Block],
) {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return;
    };
    let Some(end) = sibling_index.checked_add(current.len()) else {
        return;
    };
    if blocks.get(sibling_index..end) != Some(current) {
        return;
    }
    blocks.splice(sibling_index..end, replacement.iter().cloned());
}

fn apply_environment_boundary_paragraphs(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    paragraphs: &[Paragraph],
    insert: bool,
) {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return;
    };
    if insert {
        if sibling_index <= blocks.len() {
            blocks.splice(
                sibling_index..sibling_index,
                paragraphs.iter().cloned().map(Block::Paragraph),
            );
        }
    } else {
        let end = sibling_index.saturating_add(paragraphs.len());
        let matches_inserted = blocks.get(sibling_index..end).is_some_and(|actual| {
            match actual {
                [] => false,
                actual => actual.iter().zip(paragraphs).all(|(block, expected)| {
                    matches!(block, Block::Paragraph(paragraph) if paragraph.id == expected.id)
                }),
            }
        });
        if matches_inserted {
            blocks.drain(sibling_index..end);
        }
    }
}

fn apply_environment_deletion(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    environment: &BlockEnvironment,
    fallback_paragraph: Option<&Paragraph>,
    restore: bool,
) {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return;
    };
    if restore {
        if fallback_paragraph.is_some_and(|paragraph| {
            matches!(blocks.get(sibling_index), Some(Block::Paragraph(value)) if value.id == paragraph.id)
        }) {
            blocks.remove(sibling_index);
        }
        if sibling_index <= blocks.len() {
            blocks.insert(sibling_index, Block::Environment(environment.clone()));
        }
    } else if matches!(blocks.get(sibling_index), Some(Block::Environment(value)) if value.id == environment.id)
    {
        blocks.remove(sibling_index);
        if let Some(paragraph) = fallback_paragraph
            && sibling_index <= blocks.len()
        {
            blocks.insert(sibling_index, Block::Paragraph(paragraph.clone()));
        }
    }
}

fn apply_environment_unwrap(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    environment: &BlockEnvironment,
    restore: bool,
) {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return;
    };
    if restore {
        let mut flattened = Vec::with_capacity(environment.children.len() + 1);
        if let Some(title) = &environment.title {
            flattened.push(Block::Paragraph(title.clone()));
        }
        flattened.extend(environment.children.iter().cloned());
        let end = sibling_index.saturating_add(flattened.len());
        if end <= blocks.len() && blocks[sibling_index..end] == flattened {
            blocks.splice(
                sibling_index..end,
                [Block::Environment(environment.clone())],
            );
        }
        return;
    }
    if !matches!(blocks.get(sibling_index), Some(Block::Environment(value)) if value.id == environment.id)
    {
        return;
    }
    let mut flattened = Vec::with_capacity(environment.children.len() + 1);
    if let Some(title) = &environment.title {
        flattened.push(Block::Paragraph(title.clone()));
    }
    flattened.extend(environment.children.iter().cloned());
    blocks.splice(sibling_index..=sibling_index, flattened);
}

fn set_page_geometry(document: &mut Document, geometry: PageGeometry) {
    if let Some(section) = document.page_sections.first_mut() {
        let _ = section.set_geometry(geometry);
    }
}

pub(super) fn paragraph_estimated_bytes(paragraph: &Paragraph) -> usize {
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

pub(super) fn environment_estimated_bytes(environment: &BlockEnvironment) -> usize {
    size_of::<BlockEnvironment>()
        + node_id_bytes(&environment.id)
        + environment.definition_id.as_ref().map_or(0, String::len)
        + string_value_map_bytes(&environment.props)
        + string_value_map_bytes(&environment.extra)
        + environment
            .title
            .as_ref()
            .map_or(0, paragraph_estimated_bytes)
        + environment
            .children
            .iter()
            .map(block_estimated_bytes)
            .sum::<usize>()
}

fn block_estimated_bytes(block: &Block) -> usize {
    size_of::<Block>()
        + match block {
            Block::Paragraph(paragraph) => paragraph_estimated_bytes(paragraph),
            Block::Environment(environment) => environment_estimated_bytes(environment),
            Block::ExplicitBreak(value) => {
                size_of_val(value) + string_value_map_bytes(&value.extra)
            }
            Block::Opaque(value) => size_of_val(value) + value.raw.to_string().len(),
            Block::Equation(value) => equation_estimated_bytes(value),
            Block::Align(value) => align_estimated_bytes(value),
        }
}

fn inline_estimated_bytes(node: &caret_doc::ParagraphContent) -> usize {
    use caret_doc::ParagraphContent;
    size_of::<ParagraphContent>()
        + match node {
            ParagraphContent::Text(text) => {
                size_of_val(text)
                    + text
                        .runs
                        .iter()
                        .map(|run| {
                            size_of_val(run)
                                + run.language.as_ref().map_or(0, String::len)
                                + run.text.len()
                                + string_value_map_bytes(&run.props)
                                + string_value_map_bytes(&run.extra)
                        })
                        .sum::<usize>()
                    + string_value_map_bytes(&text.extra)
            }
            ParagraphContent::Environment(environment) => {
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
            ParagraphContent::SoftBreak(value) => {
                size_of_val(value) + string_value_map_bytes(&value.extra)
            }
            ParagraphContent::Opaque(value) => size_of_val(value) + value.raw.to_string().len(),
            ParagraphContent::Math(value) => {
                size_of_val(value) + math_tree_estimated_bytes(&value.math_tree)
            }
        }
}

fn equation_estimated_bytes(equation: &Equation) -> usize {
    size_of_val(equation)
        + node_id_bytes(&equation.id)
        + math_tree_estimated_bytes(&equation.math_tree)
        + string_value_map_bytes(&equation.extra)
}

fn align_estimated_bytes(align: &Align) -> usize {
    size_of_val(align)
        + node_id_bytes(&align.id)
        + align.source.capacity()
        + align.rows.capacity() * size_of::<caret_doc::MathAlignRow>()
        + align
            .rows
            .iter()
            .map(|row| {
                size_of::<caret_doc::MathAlignRow>()
                    + node_id_bytes(&row.id)
                    + row.cells.capacity() * size_of::<MathTree>()
                    + row
                        .cells
                        .iter()
                        .map(math_tree_estimated_bytes)
                        .sum::<usize>()
                    + string_value_map_bytes(&row.extra)
            })
            .sum::<usize>()
        + string_value_map_bytes(&align.extra)
}

fn math_tree_estimated_bytes(tree: &MathTree) -> usize {
    size_of::<MathTree>()
        + tree.source.capacity()
        + math_flow_estimated_bytes(&tree.root)
        + string_value_map_bytes(&tree.extra)
}

fn math_flow_estimated_bytes(node: &caret_doc::MathFlow) -> usize {
    size_of::<caret_doc::MathFlow>()
        + node.id.as_str().len()
        + node
            .content
            .iter()
            .map(math_content_estimated_bytes)
            .sum::<usize>()
        + string_value_map_bytes(&node.extra)
}

fn math_content_estimated_bytes(content: &caret_doc::MathContent) -> usize {
    use caret_doc::MathContent;
    match content {
        MathContent::Text(text) => {
            size_of_val(text)
                + text
                    .runs
                    .iter()
                    .map(|run| {
                        size_of_val(run)
                            + run.text.len()
                            + string_value_map_bytes(&run.props)
                            + string_value_map_bytes(&run.extra)
                    })
                    .sum::<usize>()
                + text.source_map.capacity() * size_of::<caret_doc::TextFlowSourceMapEntry>()
                + text
                    .source_map
                    .iter()
                    .map(|entry| {
                        string_value_map_bytes(&entry.source_range.extra)
                            + string_value_map_bytes(&entry.extra)
                    })
                    .sum::<usize>()
                + string_value_map_bytes(&text.extra)
        }
        MathContent::Math(node) => math_node_estimated_bytes(node),
        MathContent::Environment(environment) => {
            size_of_val(environment)
                + node_id_bytes(&environment.id)
                + string_value_map_bytes(&environment.props)
                + environment
                    .children
                    .iter()
                    .map(math_content_estimated_bytes)
                    .sum::<usize>()
                + string_value_map_bytes(&environment.extra)
        }
        MathContent::SoftBreak(value) => size_of_val(value) + string_value_map_bytes(&value.extra),
        MathContent::Opaque(value) => size_of_val(value) + value.raw.to_string().len(),
    }
}

fn math_node_estimated_bytes(node: &MathNode) -> usize {
    size_of::<MathNode>()
        + node_id_bytes(&node.id)
        + node.text.as_ref().map_or(0, String::capacity)
        + node
            .attributes
            .iter()
            .map(|(key, value)| size_of::<String>() * 2 + key.capacity() + value.capacity())
            .sum::<usize>()
        + node.source_range.as_ref().map_or(0, |range| {
            size_of_val(range) + string_value_map_bytes(&range.extra)
        })
        + node.children.capacity() * size_of::<caret_doc::MathFlow>()
        + node
            .children
            .iter()
            .map(math_flow_estimated_bytes)
            .sum::<usize>()
        + string_value_map_bytes(&node.extra)
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
        super::super::Command::InsertOneCharacterWidthSpace
        | super::super::Command::InsertTwoCharacterWidthSpace => Some(EditGroup::InsertText),
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
    paragraph_index
        .iter()
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
    document: &Document,
    paragraph_index: &ParagraphIndex,
    current_id: &NodeId,
    direction: super::super::Direction,
) -> Option<NodeId> {
    let current = paragraph_index.get(current_id)?;
    if current.is_title {
        return None;
    }
    let (&index, parent_path) = current.block_path.split_last()?;
    if direction == super::super::Direction::Backward
        && let Some(previous_index) = index.checked_sub(1)
        && let Some(Block::Environment(environment)) =
            block_list_at_path(&document.body, parent_path)
                .and_then(|blocks| blocks.get(previous_index))
        && environment.kind == BlockEnvironmentKind::Section
        && environment.children.is_empty()
        && let Some(title) = &environment.title
    {
        return Some(title.id.clone());
    }
    if direction == super::super::Direction::Backward && index == 0 {
        return paragraph_index.iter().find_map(|(id, location)| {
            (location.is_title && location.block_path == parent_path).then(|| id.clone())
        });
    }
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

pub(super) fn restore_history_transition(
    transition: HistoryTransition,
) -> (
    Selection,
    VisualAffinity,
    PendingInlineStyle,
    u64,
    Option<NodeId>,
    bool,
    Option<MathEditPosition>,
) {
    (
        transition.selection,
        transition.visual_affinity,
        transition.pending_style,
        transition.state_id,
        transition.changed_paragraph_id,
        transition.document_changed,
        transition.math_edit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use caret_doc::MathContent;
    use caret_math::{MathInputKind, ParsedMath};
    use serde_json::json;

    fn parsed_tree(source: &str) -> MathTree {
        match caret_math::parse(source, MathInputKind::Inline) {
            Ok(ParsedMath::Tree(tree)) => tree,
            Ok(ParsedMath::Align(_)) => panic!("expected a formula tree"),
            Err(error) => panic!("test formula should parse: {error}"),
        }
    }

    #[test]
    fn math_history_estimate_counts_nodes_and_extension_payloads() {
        let leaf = parsed_tree("x");
        let mut fraction = parsed_tree(r"\frac{a}{b}");
        assert!(math_tree_estimated_bytes(&fraction) > math_tree_estimated_bytes(&leaf));

        let before_extra = math_tree_estimated_bytes(&fraction);
        fraction.root.extra.insert(
            "futurePayload".to_owned(),
            json!({"description": "retained extension data".repeat(8)}),
        );
        assert!(math_tree_estimated_bytes(&fraction) > before_extra);

        let mapped = parsed_tree(r"\text{x}");
        let mut unmapped = mapped.clone();
        let Some(MathContent::Text(flow)) = unmapped.root.content.first_mut() else {
            panic!("text command should produce a text flow");
        };
        flow.source_map = Vec::new();
        assert!(math_tree_estimated_bytes(&mapped) > math_tree_estimated_bytes(&unmapped));
    }
}
