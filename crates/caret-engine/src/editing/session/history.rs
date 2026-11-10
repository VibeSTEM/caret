//! 管理会话的分支编辑历史及其局部文档补丁。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::paragraph_ops::{ParagraphIndex, validated_paragraph_location};
use super::super::{HostCaretPosition, Pos, PositionMap, Selection, StoryId};
use super::formatting::PendingInlineStyle;
use super::math_edit::{MathEdit, MathEditBranch, MathEditTarget};
use caret_doc::{
    Align, Block, BlockEnvironment, BlockEnvironmentKind, Document, DocumentFontTarget,
    DocumentSettings, Equation, MathNode, MathTree, NodeId, PageGeometry, Paragraph,
};
use caret_layout::VisualAffinity;
use std::{mem::size_of, sync::Arc};
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
) -> bool {
    // 撤销记录只由已校验的编辑命令创建，重放时目标应始终有效。
    let restored = document.global_settings.set_font_family(target, family);
    debug_assert!(restored.is_ok(), "history contains an invalid font target");
    restored.is_ok()
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
    BlockBoundaryParagraphs {
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
    anchor_visual_affinity: VisualAffinity,
    visual_affinity: VisualAffinity,
    pending_style: PendingInlineStyle,
    math_edit: Option<MathEditPosition>,
    host_caret_position: Option<HostCaretPosition<Pos>>,
    state_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MathEditPosition {
    pub(super) target: MathEditTarget,
    pub(super) projection_identity: Option<usize>,
    pub(super) cursor: usize,
    pub(super) branch: MathEditBranch,
    pub(super) source_projection: Option<Arc<caret_math::MathSourceMap<'static>>>,
}

impl MathEditPosition {
    pub(super) fn from_edit(edit: &MathEdit) -> Self {
        let source_branch = matches!(&edit.branch, MathEditBranch::Source);
        Self {
            target: edit.target.clone(),
            projection_identity: source_branch
                .then(|| {
                    edit.source_projection
                        .as_ref()
                        .map(|projection| Arc::as_ptr(projection) as usize)
                })
                .flatten(),
            cursor: edit.cursor,
            branch: edit.branch.clone(),
            source_projection: source_branch
                .then(|| edit.source_projection.clone())
                .flatten(),
        }
    }

    pub(super) fn with_edit_state(edit: &MathEdit, cursor: usize, branch: MathEditBranch) -> Self {
        let source_branch = matches!(&branch, MathEditBranch::Source);
        Self {
            target: edit.target.clone(),
            projection_identity: source_branch
                .then(|| {
                    edit.source_projection
                        .as_ref()
                        .map(|projection| Arc::as_ptr(projection) as usize)
                })
                .flatten(),
            cursor,
            branch,
            source_projection: source_branch
                .then(|| edit.source_projection.clone())
                .flatten(),
        }
    }

    fn estimated_bytes(&self, seen_projections: &mut std::collections::HashSet<usize>) -> usize {
        size_of::<Self>()
            + self.source_projection.as_ref().map_or(0, |projection| {
                if seen_projections.insert(Arc::as_ptr(projection) as usize) {
                    projection.estimated_bytes()
                } else {
                    0
                }
            })
            + match &self.branch {
                MathEditBranch::Source => 0,
                MathEditBranch::Text { path, anchor, .. } => {
                    path.estimated_bytes() + anchor.path.estimated_bytes()
                }
                MathEditBranch::Tree { path, anchor } => {
                    path.estimated_bytes() + anchor.estimated_bytes()
                }
            }
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
            anchor_visual_affinity: visual_affinity,
            visual_affinity,
            pending_style,
            math_edit: None,
            host_caret_position: None,
            state_id,
        }
    }

    pub(super) fn with_host_caret_position(
        mut self,
        host_caret_position: Option<HostCaretPosition<Pos>>,
    ) -> Self {
        self.host_caret_position = host_caret_position;
        self
    }

    pub(super) fn with_math_edit(mut self, math_edit: Option<MathEditPosition>) -> Self {
        self.math_edit = math_edit;
        self
    }

    pub(super) fn with_anchor_visual_affinity(
        mut self,
        anchor_visual_affinity: VisualAffinity,
    ) -> Self {
        self.anchor_visual_affinity = anchor_visual_affinity;
        self
    }
}

#[derive(Debug, Clone)]
pub(super) struct HistoryTransition {
    pub selection: Selection,
    pub anchor_visual_affinity: VisualAffinity,
    pub visual_affinity: VisualAffinity,
    pub pending_style: PendingInlineStyle,
    pub math_edit: Option<MathEditPosition>,
    pub host_caret_position: Option<HostCaretPosition<Pos>>,
    pub state_id: u64,
    pub changed_paragraph_id: Option<NodeId>,
    pub document_changed: bool,
    pub valid: bool,
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
        self.estimated_bytes_with_seen(&mut std::collections::HashSet::new())
    }

    fn estimated_bytes_with_seen(
        &self,
        seen_projections: &mut std::collections::HashSet<usize>,
    ) -> usize {
        size_of::<Self>()
            + self.patch.estimated_bytes()
            + selection_id_bytes(&self.before.selection)
            + selection_id_bytes(&self.after.selection)
            + self
                .before
                .math_edit
                .as_ref()
                .map_or(0, |position| position.estimated_bytes(seen_projections))
            + self
                .after
                .math_edit
                .as_ref()
                .map_or(0, |position| position.estimated_bytes(seen_projections))
            + self
                .before
                .host_caret_position
                .as_ref()
                .map_or(0, |position| node_id_bytes(position.host_id()))
            + self
                .after
                .host_caret_position
                .as_ref()
                .map_or(0, |position| node_id_bytes(position.host_id()))
    }

    fn apply_before(&self, document: &mut Document) -> bool {
        self.patch.undo(document)
    }

    fn apply_after(&self, document: &mut Document) -> bool {
        self.patch.redo(document)
    }

    fn before_transition(&self) -> HistoryTransition {
        HistoryTransition {
            selection: self.before.selection.clone(),
            anchor_visual_affinity: self.before.anchor_visual_affinity,
            visual_affinity: self.before.visual_affinity,
            pending_style: self.before.pending_style.clone(),
            math_edit: self.before.math_edit.clone(),
            host_caret_position: self.before.host_caret_position.clone(),
            state_id: self.before.state_id,
            changed_paragraph_id: self.patch.single_changed_paragraph_id(),
            document_changed: self.patch.affects_document(),
            valid: true,
        }
    }

    fn after_transition(&self) -> HistoryTransition {
        HistoryTransition {
            selection: self.after.selection.clone(),
            anchor_visual_affinity: self.after.anchor_visual_affinity,
            visual_affinity: self.after.visual_affinity,
            pending_style: self.after.pending_style.clone(),
            math_edit: self.after.math_edit.clone(),
            host_caret_position: self.after.host_caret_position.clone(),
            state_id: self.after.state_id,
            changed_paragraph_id: self.patch.single_changed_paragraph_id(),
            document_changed: self.patch.affects_document(),
            valid: true,
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
        let mut transition = self.before_transition();
        transition.valid = self.apply_before(document);
        transition
    }

    fn redo(&mut self, document: &mut Document) -> Self::Output {
        let mut transition = self.after_transition();
        transition.valid = self.apply_after(document);
        transition
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
            Self::BlockBoundaryParagraphs {
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

    pub(super) fn undo(&self, document: &mut Document) -> bool {
        self.apply_transactionally(document, true)
    }

    pub(super) fn redo(&self, document: &mut Document) -> bool {
        self.apply_transactionally(document, false)
    }

    fn apply_transactionally(&self, document: &mut Document, undo: bool) -> bool {
        if matches!(self, Self::PendingStyle) {
            return true;
        }
        let mut candidate = document.clone();
        let changed = if let Self::Compound(patches) = self {
            if undo {
                for patch in patches.iter().rev() {
                    let mut next = candidate.clone();
                    if !patch.apply_unchecked(&mut next, true) || next == candidate {
                        return false;
                    }
                    candidate = next;
                }
            } else {
                for patch in patches {
                    let mut next = candidate.clone();
                    if !patch.apply_unchecked(&mut next, false) || next == candidate {
                        return false;
                    }
                    candidate = next;
                }
            }
            candidate != *document
        } else {
            self.apply_unchecked(&mut candidate, undo) && candidate != *document
        };
        if changed {
            *document = candidate;
        }
        changed
    }

    fn apply_unchecked(&self, document: &mut Document, undo: bool) -> bool {
        if let Self::Compound(patches) = self {
            if undo {
                for patch in patches.iter().rev() {
                    if !patch.apply_transactionally(document, true) {
                        return false;
                    }
                }
            } else {
                for patch in patches {
                    if !patch.apply_transactionally(document, false) {
                        return false;
                    }
                }
            }
            return true;
        }
        if undo {
            self.undo_in_place(document)
        } else {
            self.redo_in_place(document)
        }
    }

    fn undo_in_place(&self, document: &mut Document) -> bool {
        match self {
            Self::PendingStyle => true,
            Self::Compound(_) => false,
            Self::Paragraphs { before, after, .. } => {
                apply_paragraph_states_in_place(document, after, before)
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
                true
            }
            Self::DocumentFontFamily { target, before, .. } => {
                restore_document_font_family(document, target, before.clone())
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
            Self::BlockBoundaryParagraphs {
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

    fn redo_in_place(&self, document: &mut Document) -> bool {
        match self {
            Self::PendingStyle => true,
            Self::Compound(_) => false,
            Self::Paragraphs { before, after, .. } => {
                apply_paragraph_states_in_place(document, before, after)
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
                true
            }
            Self::DocumentFontFamily { target, after, .. } => {
                restore_document_font_family(document, target, after.clone())
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
            Self::BlockBoundaryParagraphs {
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
        if location.paragraph_id != *paragraph_id {
            return None;
        }
        let (paragraph, parent_environment_ids, sibling_index) =
            validated_paragraph_location(&document.body, location)?;
        let paragraph = paragraph.clone();
        let estimated_paragraph_bytes = paragraph_estimated_bytes(&paragraph);
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

fn apply_paragraph_states_in_place(
    document: &mut Document,
    current: &[ParagraphState],
    target: &[ParagraphState],
) -> bool {
    let current_ids = current
        .iter()
        .map(|state| state.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let target_ids = target
        .iter()
        .map(|state| state.id.clone())
        .collect::<std::collections::HashSet<_>>();
    if current_ids.len() != current.len() || target_ids.len() != target.len() {
        return false;
    }
    let mut removals = current.iter().collect::<Vec<_>>();
    removals.sort_by(|left, right| {
        left.parent_environment_ids
            .cmp(&right.parent_environment_ids)
            .then_with(|| right.is_title.cmp(&left.is_title))
            .then_with(|| right.sibling_index.cmp(&left.sibling_index))
    });
    for state in removals {
        if !remove_paragraph_state(document, state) {
            return false;
        }
    }
    for state in target.iter().filter(|state| state.is_title) {
        let Some(environment_id) = state.parent_environment_ids.last() else {
            return false;
        };
        let Some(parents) = state
            .parent_environment_ids
            .get(..state.parent_environment_ids.len() - 1)
        else {
            return false;
        };
        let Some(siblings) = block_list_mut(&mut document.body, parents) else {
            return false;
        };
        let Some(environment) = siblings.iter_mut().find_map(|block| match block {
            Block::Environment(environment) if &environment.id == environment_id => {
                Some(environment)
            }
            _ => None,
        }) else {
            return false;
        };
        if environment.title.is_some() {
            return false;
        }
        environment.title = Some(state.paragraph.clone());
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
            return false;
        };
        if state.sibling_index > blocks.len() {
            return false;
        }
        blocks.insert(
            state.sibling_index,
            Block::Paragraph(state.paragraph.clone()),
        );
    }
    true
}

fn remove_paragraph_state(document: &mut Document, state: &ParagraphState) -> bool {
    if state.is_title {
        let Some((environment_id, parent_ids)) = state.parent_environment_ids.split_last() else {
            return false;
        };
        let Some(siblings) = block_list_mut(&mut document.body, parent_ids) else {
            return false;
        };
        let Some(environment) = siblings.iter_mut().find_map(|block| match block {
            Block::Environment(environment) if &environment.id == environment_id => {
                Some(environment)
            }
            _ => None,
        }) else {
            return false;
        };
        if !environment
            .title
            .as_ref()
            .is_some_and(|title| title.id == state.id && title == &state.paragraph)
        {
            return false;
        }
        environment.title = None;
        return true;
    }

    let Some(siblings) = block_list_mut(&mut document.body, &state.parent_environment_ids) else {
        return false;
    };
    if !matches!(
        siblings.get(state.sibling_index),
        Some(Block::Paragraph(paragraph)) if paragraph.id == state.id && paragraph == &state.paragraph
    ) {
        return false;
    }
    siblings.remove(state.sibling_index);
    true
}

fn block_list_mut<'a>(
    blocks: &'a mut Vec<Block>,
    environment_ids: &[NodeId],
) -> Option<&'a mut Vec<Block>> {
    let Some((environment_id, rest)) = environment_ids.split_first() else {
        return Some(blocks);
    };
    let environment = blocks.iter_mut().find_map(|block| match block {
        Block::Environment(environment) if &environment.id == environment_id => Some(environment),
        _ => None,
    })?;
    block_list_mut(&mut environment.children, rest)
}

fn apply_paragraph_insertion(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    paragraph: &Paragraph,
    insert: bool,
) -> bool {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return false;
    };
    if insert {
        if blocks
            .iter()
            .any(|block| matches!(block, Block::Paragraph(existing) if existing.id == paragraph.id))
        {
            return false;
        }
        if sibling_index <= blocks.len() {
            blocks.insert(sibling_index, Block::Paragraph(paragraph.clone()));
            true
        } else {
            false
        }
    } else if matches!(blocks.get(sibling_index), Some(Block::Paragraph(current)) if current == paragraph)
    {
        let index = sibling_index;
        blocks.remove(index);
        true
    } else {
        false
    }
}

fn apply_environment_creation(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    paragraph: &Paragraph,
    environment_id: &NodeId,
    kind: BlockEnvironmentKind,
    create: bool,
) -> bool {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return false;
    };
    let Some(current) = blocks.get(sibling_index) else {
        return false;
    };
    let expected_environment = BlockEnvironment {
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
    };
    let expected = if create {
        matches!(current, Block::Paragraph(value) if value == paragraph)
    } else {
        matches!(current, Block::Environment(value) if value == &expected_environment)
    };
    if !expected {
        return false;
    }
    blocks[sibling_index] = if create {
        Block::Environment(expected_environment)
    } else {
        Block::Paragraph(paragraph.clone())
    };
    true
}

fn apply_environment_insertion(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    environment: &BlockEnvironment,
    insert: bool,
) -> bool {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return false;
    };
    if insert {
        if sibling_index <= blocks.len() {
            blocks.insert(sibling_index, Block::Environment(environment.clone()));
            true
        } else {
            false
        }
    } else if matches!(blocks.get(sibling_index), Some(Block::Environment(value)) if value == environment)
    {
        blocks.remove(sibling_index);
        true
    } else {
        false
    }
}

/// 把 `current` 所在的连续块替换为 `replacement`；位置或内容不符时不改文档。
fn apply_block_range(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    current: &[Block],
    replacement: &[Block],
) -> bool {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return false;
    };
    let Some(end) = sibling_index.checked_add(current.len()) else {
        return false;
    };
    if !blocks
        .get(sibling_index..end)
        .is_some_and(|actual| persisted_blocks_equal(actual, current))
    {
        return false;
    }
    blocks.splice(sibling_index..end, replacement.iter().cloned());
    true
}

fn persisted_blocks_equal(left: &[Block], right: &[Block]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left == right
                || match (serde_json::to_value(left), serde_json::to_value(right)) {
                    (Ok(left), Ok(right)) => left == right,
                    _ => false,
                }
        })
}

fn apply_environment_boundary_paragraphs(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    paragraphs: &[Paragraph],
    insert: bool,
) -> bool {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return false;
    };
    if insert {
        if sibling_index <= blocks.len() {
            blocks.splice(
                sibling_index..sibling_index,
                paragraphs.iter().cloned().map(Block::Paragraph),
            );
            true
        } else {
            false
        }
    } else {
        let end = sibling_index.saturating_add(paragraphs.len());
        let matches_inserted = blocks.get(sibling_index..end).is_some_and(|actual| {
            match actual {
                [] => false,
                actual => actual.iter().zip(paragraphs).all(|(block, expected)| {
                    matches!(block, Block::Paragraph(paragraph) if paragraph == expected)
                }),
            }
        });
        if matches_inserted {
            blocks.drain(sibling_index..end);
            true
        } else {
            false
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
) -> bool {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return false;
    };
    if restore {
        if let Some(paragraph) = fallback_paragraph {
            if !matches!(blocks.get(sibling_index), Some(Block::Paragraph(value)) if value == paragraph)
            {
                return false;
            }
            blocks.remove(sibling_index);
        } else if sibling_index > blocks.len() {
            return false;
        }
        if blocks
            .iter()
            .any(|block| matches!(block, Block::Environment(value) if value.id == environment.id))
        {
            return false;
        }
        if sibling_index <= blocks.len() {
            blocks.insert(sibling_index, Block::Environment(environment.clone()));
            true
        } else {
            false
        }
    } else if matches!(blocks.get(sibling_index), Some(Block::Environment(value)) if value == environment)
    {
        blocks.remove(sibling_index);
        if let Some(paragraph) = fallback_paragraph
            && sibling_index <= blocks.len()
        {
            blocks.insert(sibling_index, Block::Paragraph(paragraph.clone()));
        }
        true
    } else {
        false
    }
}

fn apply_environment_unwrap(
    document: &mut Document,
    parent_environment_ids: &[NodeId],
    sibling_index: usize,
    environment: &BlockEnvironment,
    restore: bool,
) -> bool {
    let Some(blocks) = block_list_mut(&mut document.body, parent_environment_ids) else {
        return false;
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
            return true;
        }
        return false;
    }
    if !matches!(blocks.get(sibling_index), Some(Block::Environment(value)) if value == environment)
    {
        return false;
    }
    let mut flattened = Vec::with_capacity(environment.children.len() + 1);
    if let Some(title) = &environment.title {
        flattened.push(Block::Paragraph(title.clone()));
    }
    flattened.extend(environment.children.iter().cloned());
    blocks.splice(sibling_index..=sibling_index, flattened);
    true
}

fn set_page_geometry(document: &mut Document, geometry: PageGeometry) -> bool {
    if let Some(section) = document.page_sections.first_mut() {
        return section.set_geometry(geometry).is_ok();
    }
    false
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
    let mut seen_projections = std::collections::HashSet::new();
    let current_entries = history
        .entries()
        .map(|entry| estimated_entry_bytes(entry, &mut seen_projections))
        .fold(0usize, usize::saturating_add);
    let branch_bytes = history
        .branches()
        .map(|(_, branch)| {
            branch
                .entries()
                .map(|entry| estimated_entry_bytes(entry, &mut seen_projections))
                .fold(ESTIMATED_BRANCH_OVERHEAD_BYTES, usize::saturating_add)
        })
        .fold(0usize, usize::saturating_add);
    // 额外系数覆盖分配器节点、容器容量和非 payload 的树形索引开销。
    size_of::<History<SessionHistoryEdit>>()
        .saturating_add(current_entries)
        .saturating_add(branch_bytes)
        .saturating_mul(2)
}

fn estimated_entry_bytes(
    entry: &undo::Entry<SessionHistoryEdit>,
    seen_projections: &mut std::collections::HashSet<usize>,
) -> usize {
    size_of::<undo::Entry<SessionHistoryEdit>>()
        .saturating_add(entry.as_ref().estimated_bytes_with_seen(seen_projections))
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

pub(super) struct RestoredHistoryTransition {
    pub(super) selection: Selection,
    pub(super) anchor_visual_affinity: VisualAffinity,
    pub(super) visual_affinity: VisualAffinity,
    pub(super) pending_style: PendingInlineStyle,
    pub(super) state_id: u64,
    pub(super) changed_paragraph_id: Option<NodeId>,
    pub(super) document_changed: bool,
    pub(super) math_edit: Option<MathEditPosition>,
    pub(super) host_caret_position: Option<HostCaretPosition<Pos>>,
    pub(super) valid: bool,
}

pub(super) fn restore_history_transition(
    transition: HistoryTransition,
) -> RestoredHistoryTransition {
    RestoredHistoryTransition {
        selection: transition.selection,
        anchor_visual_affinity: transition.anchor_visual_affinity,
        visual_affinity: transition.visual_affinity,
        pending_style: transition.pending_style,
        state_id: transition.state_id,
        changed_paragraph_id: transition.changed_paragraph_id,
        document_changed: transition.document_changed,
        math_edit: transition.math_edit,
        host_caret_position: transition.host_caret_position,
        valid: transition.valid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editing::paragraph_ops::{build_paragraph_index, paragraph_parent_path};
    use caret_doc::{BlockEnvironment, MathContent, ParagraphContent, Run};
    use caret_math::MathInputKind;
    use serde_json::json;

    fn parsed_tree(source: &str) -> MathTree {
        match caret_math::parse_projection(source, MathInputKind::Inline) {
            Ok(projection) => match projection.into_parts().0 {
                caret_math::ProjectedMath::Tree(tree) => tree,
                caret_math::ProjectedMath::Align(_) => panic!("expected a formula tree"),
            },
            Err(error) => panic!("test formula should parse: {error}"),
        }
    }

    fn paragraph_state(paragraph: Paragraph, sibling_index: usize) -> ParagraphState {
        ParagraphState {
            id: paragraph.id.clone(),
            estimated_paragraph_bytes: paragraph_estimated_bytes(&paragraph),
            paragraph,
            parent_environment_ids: Vec::new(),
            sibling_index,
            is_title: false,
        }
    }

    #[test]
    fn history_parent_locator_uses_physical_parent_ids_and_rejects_moved_paragraphs() {
        let environment_id = NodeId::generate();
        let title = Paragraph::empty(NodeId::generate());
        let title_id = title.id.clone();
        let child = Paragraph::empty(NodeId::generate());
        let child_id = child.id.clone();
        let nested_sibling = Paragraph::empty(NodeId::generate());
        let trailing = Paragraph::empty(NodeId::generate());
        let trailing_id = trailing.id.clone();
        let mut document = Document {
            body: vec![
                Block::Environment(BlockEnvironment {
                    id: environment_id.clone(),
                    kind: BlockEnvironmentKind::Section,
                    definition_id: None,
                    props: Default::default(),
                    title: Some(title),
                    children: vec![Block::Paragraph(child), Block::Paragraph(nested_sibling)],
                    extra: Default::default(),
                }),
                Block::Paragraph(trailing),
            ],
            ..Document::blank()
        };
        let index = build_paragraph_index(&document.body);
        let title_location = index
            .get(&title_id)
            .unwrap_or_else(|| panic!("title location should exist"));
        let child_location = index
            .get(&child_id)
            .unwrap_or_else(|| panic!("child location should exist"));
        let trailing_location = index
            .get(&trailing_id)
            .unwrap_or_else(|| panic!("trailing location should exist"));

        let mut mismatched_index = ParagraphIndex::new();
        mismatched_index.insert(child_id.clone(), trailing_location.clone());
        assert!(ParagraphState::capture(&document, &mismatched_index, &child_id).is_none());

        assert_eq!(
            validated_paragraph_location(&document.body, title_location)
                .map(|(_, parents, slot)| (parents, slot)),
            Some((vec![environment_id.clone()], 0))
        );
        assert_eq!(
            validated_paragraph_location(&document.body, child_location)
                .map(|(_, parents, slot)| (parents, slot)),
            Some((vec![environment_id.clone()], 0))
        );
        assert_eq!(
            validated_paragraph_location(&document.body, trailing_location)
                .map(|(_, parents, slot)| (parents, slot)),
            Some((Vec::new(), 1))
        );

        {
            let Block::Environment(environment) = &mut document.body[0] else {
                panic!("first block should remain the Section");
            };
            environment.children.swap(0, 1);
        }
        assert!(ParagraphState::capture(&document, &index, &child_id).is_none());
        assert!(validated_paragraph_location(&document.body, child_location).is_none());
        assert!(paragraph_parent_path(&document.body, child_location).is_none());
        if let Block::Environment(environment) = &mut document.body[0] {
            environment.children.swap(0, 1);
        }

        let Block::Environment(environment) = &mut document.body[0] else {
            panic!("first block should remain the Section");
        };
        let Block::Paragraph(moved_child) = environment.children.remove(0) else {
            panic!("Section child should be a paragraph");
        };
        document.body.push(Block::Paragraph(moved_child));
        assert_eq!(
            validated_paragraph_location(&document.body, child_location),
            None
        );
    }

    #[test]
    fn nested_section_title_patch_restores_through_undo_and_redo() {
        fn title_with_text(id: NodeId, value: &str) -> Paragraph {
            let mut paragraph = Paragraph::empty(id);
            paragraph
                .inlines
                .push(ParagraphContent::Text(caret_doc::TextFlow {
                    runs: vec![Run {
                        text: value.to_owned(),
                        language: None,
                        props: Default::default(),
                        extra: Default::default(),
                    }],
                    extra: Default::default(),
                }));
            paragraph
        }

        let outer_id = NodeId::generate();
        let inner_id = NodeId::generate();
        let title_id = NodeId::generate();
        let expected_parent_ids = vec![outer_id.clone(), inner_id.clone()];
        let mut document = Document {
            body: vec![Block::Environment(BlockEnvironment {
                id: outer_id.clone(),
                kind: BlockEnvironmentKind::Section,
                definition_id: None,
                props: Default::default(),
                title: Some(Paragraph::empty(NodeId::generate())),
                children: vec![Block::Environment(BlockEnvironment {
                    id: inner_id.clone(),
                    kind: BlockEnvironmentKind::Section,
                    definition_id: None,
                    props: Default::default(),
                    title: Some(title_with_text(title_id.clone(), "before")),
                    children: Vec::new(),
                    extra: Default::default(),
                })],
                extra: Default::default(),
            })],
            ..Document::blank()
        };
        let before_index = build_paragraph_index(&document.body);
        let before = ParagraphState::capture(&document, &before_index, &title_id)
            .unwrap_or_else(|| panic!("nested title state should be captured"));

        {
            let Block::Environment(outer) = &mut document.body[0] else {
                panic!("first block should be the outer Section");
            };
            let Block::Environment(inner) = &mut outer.children[0] else {
                panic!("outer Section child should be the inner Section");
            };
            inner.title = Some(title_with_text(title_id.clone(), "after"));
        }

        let after_index = build_paragraph_index(&document.body);
        let after = ParagraphState::capture(&document, &after_index, &title_id)
            .unwrap_or_else(|| panic!("updated nested title state should be captured"));
        assert_eq!(before.parent_environment_ids, expected_parent_ids);

        let patch = DocumentPatch::Paragraphs {
            before: vec![before.clone()],
            after: vec![after.clone()],
            merge_group: None,
            merge_epoch: 0,
        };
        assert!(patch.undo(&mut document));
        assert!(matches!(
            &document.body[0],
            Block::Environment(outer)
                if matches!(&outer.children[0], Block::Environment(inner)
                    if matches!(inner.title.as_ref().and_then(|title| title.inlines.first()),
                        Some(ParagraphContent::Text(flow)) if flow.runs[0].text == "before"))
        ));

        assert!(patch.redo(&mut document));
        assert!(matches!(
            &document.body[0],
            Block::Environment(outer)
                if matches!(&outer.children[0], Block::Environment(inner)
                    if matches!(inner.title.as_ref().and_then(|title| title.inlines.first()),
                        Some(ParagraphContent::Text(flow)) if flow.runs[0].text == "after"))
        ));
    }

    #[test]
    fn paragraph_history_restores_shared_id_and_rejects_invalid_slot_atomically() {
        let mut before_paragraph = Paragraph::empty(NodeId::generate());
        before_paragraph
            .inlines
            .push(ParagraphContent::Text(caret_doc::TextFlow {
                runs: vec![Run {
                    text: "before".to_owned(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                }],
                extra: Default::default(),
            }));
        let mut after_paragraph = before_paragraph.clone();
        if let Some(ParagraphContent::Text(flow)) = after_paragraph.inlines.first_mut() {
            flow.runs[0].text = "after".to_owned();
        }
        let current = paragraph_state(after_paragraph, 0);
        let target = paragraph_state(before_paragraph.clone(), 0);
        let mut document = Document {
            body: vec![Block::Paragraph(current.paragraph.clone())],
            ..Document::blank()
        };

        assert!(apply_paragraph_states_in_place(
            &mut document,
            std::slice::from_ref(&current),
            std::slice::from_ref(&target)
        ));
        assert!(matches!(
            &document.body[0],
            Block::Paragraph(paragraph)
                if paragraph.id == target.id
                    && matches!(paragraph.inlines.first(), Some(ParagraphContent::Text(flow)) if flow.runs[0].text == "before")
        ));

        let invalid_current = paragraph_state(before_paragraph, 1);
        let unchanged = document.clone();
        assert!(!apply_paragraph_states_in_place(
            &mut document,
            std::slice::from_ref(&invalid_current),
            &[]
        ));
        assert_eq!(document, unchanged);
    }

    #[test]
    fn compound_history_rejects_partial_application_atomically() {
        let mut before = Paragraph::empty(NodeId::generate());
        before
            .inlines
            .push(ParagraphContent::Text(caret_doc::TextFlow {
                runs: vec![Run {
                    text: "before".to_owned(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                }],
                extra: Default::default(),
            }));
        let mut after = before.clone();
        let Some(ParagraphContent::Text(flow)) = after.inlines.first_mut() else {
            panic!("paragraph should contain text");
        };
        flow.runs[0].text = "after".to_owned();
        let mut document = Document {
            body: vec![Block::Paragraph(before.clone())],
            ..Document::blank()
        };
        let patch = DocumentPatch::Compound(vec![
            DocumentPatch::Paragraphs {
                before: vec![paragraph_state(before, 0)],
                after: vec![paragraph_state(after, 0)],
                merge_group: None,
                merge_epoch: 0,
            },
            DocumentPatch::ParagraphInsertion {
                parent_environment_ids: vec![NodeId::generate()],
                sibling_index: 0,
                paragraph: Box::new(Paragraph::empty(NodeId::generate())),
            },
        ]);
        let unchanged = document.clone();

        assert!(!patch.redo(&mut document));
        assert_eq!(document, unchanged);
    }

    #[test]
    fn history_removals_reject_same_id_changed_snapshots() {
        let paragraph = Paragraph::empty(NodeId::generate());
        let boundary_patch = DocumentPatch::BlockBoundaryParagraphs {
            parent_environment_ids: Vec::new(),
            sibling_index: 1,
            paragraphs: vec![paragraph.clone()],
            paragraphs_estimated_bytes: paragraph_estimated_bytes(&paragraph),
        };
        let mut document = Document {
            body: vec![Block::Paragraph(Paragraph::empty(NodeId::generate()))],
            ..Document::blank()
        };
        assert!(boundary_patch.redo(&mut document));
        let Some(Block::Paragraph(changed)) = document.body.get_mut(1) else {
            panic!("inserted boundary paragraph should exist");
        };
        changed.props.insert("changed".to_owned(), json!(true));
        let unchanged = document.clone();
        assert!(!boundary_patch.undo(&mut document));
        assert_eq!(document, unchanged);

        let source_paragraph = Paragraph::empty(NodeId::generate());
        let environment_id = NodeId::generate();
        let creation_patch = DocumentPatch::EnvironmentCreation {
            parent_environment_ids: Vec::new(),
            sibling_index: 0,
            paragraph: Box::new(source_paragraph.clone()),
            paragraph_estimated_bytes: paragraph_estimated_bytes(&source_paragraph),
            environment_id: environment_id.clone(),
            kind: BlockEnvironmentKind::Custom,
        };
        let mut changed_paragraph = source_paragraph.clone();
        changed_paragraph
            .props
            .insert("changed".to_owned(), json!(true));
        let mut document = Document {
            body: vec![Block::Paragraph(changed_paragraph)],
            ..Document::blank()
        };
        let unchanged = document.clone();
        assert!(!creation_patch.redo(&mut document));
        assert_eq!(document, unchanged);

        let mut changed_environment = BlockEnvironment {
            id: environment_id,
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: Default::default(),
            title: None,
            children: vec![Block::Paragraph(source_paragraph)],
            extra: Default::default(),
        };
        changed_environment
            .props
            .insert("changed".to_owned(), json!(true));
        let mut document = Document {
            body: vec![Block::Environment(changed_environment)],
            ..Document::blank()
        };
        let unchanged = document.clone();
        assert!(!creation_patch.undo(&mut document));
        assert_eq!(document, unchanged);

        let environment = BlockEnvironment {
            id: NodeId::generate(),
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: Default::default(),
            title: None,
            children: vec![Block::Paragraph(Paragraph::empty(NodeId::generate()))],
            extra: Default::default(),
        };
        let mut changed_environment = environment.clone();
        changed_environment
            .props
            .insert("changed".to_owned(), json!(true));
        let environment_patch = DocumentPatch::EnvironmentInsertion {
            parent_environment_ids: Vec::new(),
            sibling_index: 0,
            environment: Box::new(environment.clone()),
            environment_estimated_bytes: 0,
        };
        for (patch, undo) in [
            (environment_patch, true),
            (
                DocumentPatch::EnvironmentDeletion {
                    parent_environment_ids: Vec::new(),
                    sibling_index: 0,
                    environment: Box::new(environment.clone()),
                    environment_estimated_bytes: 0,
                    fallback_paragraph: None,
                },
                false,
            ),
            (
                DocumentPatch::EnvironmentUnwrap {
                    parent_environment_ids: Vec::new(),
                    sibling_index: 0,
                    environment: Box::new(environment),
                    environment_estimated_bytes: 0,
                },
                false,
            ),
        ] {
            let mut document = Document {
                body: vec![Block::Environment(changed_environment.clone())],
                ..Document::blank()
            };
            let unchanged = document.clone();
            let applied = if undo {
                patch.undo(&mut document)
            } else {
                patch.redo(&mut document)
            };
            assert!(!applied);
            assert_eq!(document, unchanged);
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

        let short_text = parsed_tree(r"\text{x}");
        let long_text = parsed_tree(&format!(r"\text{{{}}}", "x".repeat(256)));
        let Some(MathContent::Text(flow)) = long_text.root.content.first() else {
            panic!("text command should produce a text flow");
        };
        assert_eq!(flow.runs[0].text.len(), 256);
        assert!(math_tree_estimated_bytes(&long_text) > math_tree_estimated_bytes(&short_text));
    }

    #[test]
    fn shared_math_projection_is_counted_once_per_history_estimate() {
        let projection = Arc::new(
            caret_math::parse_projection(&"x".repeat(4096), MathInputKind::Inline)
                .unwrap_or_else(|error| panic!("test source should parse: {error}"))
                .into_parts()
                .1,
        );
        let position = MathEditPosition {
            target: MathEditTarget::Inline {
                paragraph_id: NodeId::generate(),
                math_id: NodeId::generate(),
            },
            projection_identity: Some(Arc::as_ptr(&projection) as usize),
            cursor: 0,
            branch: MathEditBranch::Source,
            source_projection: Some(Arc::clone(&projection)),
        };

        let mut seen = std::collections::HashSet::new();
        let first = position.estimated_bytes(&mut seen);
        let second = position.clone().estimated_bytes(&mut seen);

        assert!(first >= projection.estimated_bytes());
        assert!(first - second >= projection.estimated_bytes());
    }
}
