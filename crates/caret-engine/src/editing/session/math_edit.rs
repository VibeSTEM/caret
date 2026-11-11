//! 公式编辑状态：维护已提交树投影，并隔离可恢复的源码草稿。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::{collections::BTreeMap, ops::Range, sync::Arc};

use super::super::grapheme::GraphemeMap;
use super::super::paragraph_ops::build_paragraph_index;
#[cfg(test)]
use metadata::{preserve_align_metadata, preserve_math_tree_metadata, unchanged_source_boundaries};
use metadata::{preserve_display_math_metadata, preserve_math_tree_metadata_for_edit};
use navigation::{
    align_cell_at, math_input_body_range, next_grapheme_end, next_math_caret_stop,
    previous_grapheme_start, previous_math_caret_stop,
};

mod demotion;
pub(super) mod inline_root_split;
mod metadata;
mod multiline;
mod navigation;
mod path;
mod selection_extension;
mod source_draft;
mod text_branch;
mod text_inline_environment;
mod text_run_property;
mod text_run_split;
mod tree_position;
use super::super::paragraph_ops::paragraph_parent_path;
use super::super::text_map::{OBJECT_REPLACEMENT, TextMap, inline_length, split_inline_nodes};
use super::super::{
    Affinity, Command, CommandLine, Direction, EditError, Effect, HostCaretKind, HostCaretPosition,
    HostCaretTransition, Pos, PositionMap, Selection, StoryId,
};
use super::Session;
use super::history::{
    self, DocumentPatch, EditGroup, MathEditPosition, SessionHistoryEdit, SessionHistoryState,
};
use crate::{StructureIndex, VisualAffinity};
#[cfg(test)]
use caret_doc::TextFlow;
use caret_doc::{
    Align, Block, Equation, InlineMath, MathAlignRow, MathContent, MathFlow, MathNode,
    MathNodeKind, MathTree, NodeId, Paragraph, ParagraphContent,
};
use inline_root_split::{
    InlineRootFocus, replace_inline_math_with_split, split_inline_math_root_with_map,
};
pub(super) use path::TextIslandSnapshot;
pub use path::{MathTextFlowPath, MathTextFlowPathStep, MathTextPosition};
use source_draft::MathSourceDraft;
pub(super) use text_branch::math_text_split_is_available;
use text_branch::{MathTextEdit, apply_math_text_split_tree_edit, is_grapheme_boundary};
pub use tree_position::{
    MathPositionAffinity, MathPositionHost, MathSequencePath, MathSequenceStep, MathTreePosition,
    MathTreePositionError,
};
pub(super) use tree_position::{MathTextFlowBoundary, compare_math_position_affinity};
use tree_position::{MathTreeCaretMove, advance_math_tree_position, child_sequence_path};

enum MathTextCaretStep {
    Local(usize),
    CrossFlow(MathTextFlowPath, usize),
    Boundary,
}

type MathTreeTextBoundary<'a> = (&'a MathTree, MathTreePosition, Option<(usize, usize)>);

pub(super) fn math_text_flow_display_len(
    tree: &MathTree,
    path: &MathTextFlowPath,
) -> Option<usize> {
    path.resolve_structurally(tree)?
        .runs
        .iter()
        .try_fold(0usize, |length, run| length.checked_add(run.text.len()))
}

pub(super) fn math_text_flow_boundary_is_shared(
    tree: &MathTree,
    path: &MathTextFlowPath,
    boundary: MathTextFlowBoundary,
) -> Option<bool> {
    let island = path::contiguous_text_flow_text(tree, path)?;
    let (sequence_path, text_index) =
        MathTreePosition::sequence_boundary_from_text_path(path.steps()).ok()?;
    let boundary_index = match boundary {
        MathTextFlowBoundary::Start => text_index,
        MathTextFlowBoundary::End => text_index.checked_add(1)?,
    };
    let mut matches = 0;
    for candidate in &island.paths {
        let (candidate_path, candidate_index) =
            MathTreePosition::sequence_boundary_from_text_path(candidate.steps()).ok()?;
        if candidate_path == sequence_path && candidate_index == boundary_index {
            matches += 1;
        }
        if candidate_path == sequence_path && candidate_index.checked_add(1)? == boundary_index {
            matches += 1;
        }
        if matches > 1 {
            return Some(true);
        }
    }
    Some(false)
}

struct DisplayMathInsertPlan {
    parent_environment_ids: Vec<NodeId>,
    sibling_index: usize,
    before: Block,
    after: Vec<Block>,
    next_position: Pos,
    target: MathEditTarget,
    cursor: usize,
    branch: MathEditBranch,
}

fn empty_math_tree() -> MathTree {
    MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: MathFlow {
            id: NodeId::generate(),
            content: Vec::new(),
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }
}

fn math_sequence_is_empty(contents: &[MathContent]) -> bool {
    contents.is_empty()
        || contents.iter().all(|content| {
            matches!(content, MathContent::Math(node)
                if node.kind == MathNodeKind::Placeholder
                    && node.text.is_none()
                    && node.attributes.is_empty()
                    && node.children.is_empty()
                    && node.extra.is_empty())
        })
}

fn math_tree_edge_position(
    tree: &MathTree,
    host: MathPositionHost,
    direction: Direction,
    align_cell: Option<(usize, usize)>,
) -> Result<(usize, MathEditBranch), EditError> {
    if let Some(path) = path::root_text_flow_path(tree, direction == Direction::Forward, align_cell)
    {
        let text = path
            .resolve_structurally(tree)
            .ok_or(EditError::InvalidPosition)?;
        let length = text
            .runs
            .iter()
            .try_fold(0usize, |length, run| length.checked_add(run.text.len()))
            .ok_or(EditError::InvalidPosition)?;
        let cursor = match direction {
            Direction::Forward => 0,
            Direction::Backward => length,
        };
        let anchor = MathTextPosition::new(path.clone(), cursor, MathPositionAffinity::Downstream);
        return Ok((
            cursor,
            MathEditBranch::Text {
                path,
                anchor,
                focus_affinity: MathPositionAffinity::Downstream,
                anchor_visual_affinity: VisualAffinity::Downstream,
                visual_affinity: VisualAffinity::Downstream,
            },
        ));
    }
    let position = MathTreePosition {
        host,
        path: MathSequencePath::root(),
        offset: match direction {
            Direction::Forward => 0,
            Direction::Backward => {
                tree_position::placeholder_run_start(&tree.root.content, tree.root.content.len())
                    .unwrap_or(tree.root.content.len())
            }
        },
        affinity: match direction {
            Direction::Forward => MathPositionAffinity::Downstream,
            Direction::Backward => MathPositionAffinity::Upstream,
        },
    };
    Ok((
        position.offset,
        MathEditBranch::Tree {
            path: position.path.clone(),
            anchor: position,
        },
    ))
}

fn first_placeholder_position(
    contents: &[MathContent],
    path: &MathSequencePath,
) -> Option<(MathSequencePath, usize)> {
    for (content_index, content) in contents.iter().enumerate() {
        match content {
            MathContent::Math(node) if node.kind == MathNodeKind::Placeholder => {
                return Some((path.clone(), content_index));
            }
            MathContent::Math(node) => {
                for (operand_index, flow) in node.children.iter().enumerate() {
                    if optional_placeholder_operand(node, operand_index) {
                        continue;
                    }
                    let mut steps = path.steps().to_vec();
                    steps.push(MathSequenceStep::Operand {
                        content_index,
                        operand_index,
                    });
                    if let Some(found) =
                        first_placeholder_position(&flow.content, &MathSequencePath::new(steps))
                    {
                        return Some(found);
                    }
                }
            }
            MathContent::Environment(environment) => {
                let mut steps = path.steps().to_vec();
                steps.push(MathSequenceStep::EnvironmentChildren { content_index });
                if let Some(found) =
                    first_placeholder_position(&environment.children, &MathSequencePath::new(steps))
                {
                    return Some(found);
                }
            }
            MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
        }
    }
    None
}

pub(super) fn optional_placeholder_operand(
    node: &caret_doc::MathNode,
    operand_index: usize,
) -> bool {
    node.kind
        .optional_placeholder_operands()
        .contains(&operand_index)
        && node.children.get(operand_index).is_some_and(|flow| {
            matches!(flow.content.as_slice(), [MathContent::Math(child)] if child.kind == MathNodeKind::Placeholder)
        })
}

fn math_content_has_unknown_metadata(content: &MathContent) -> bool {
    match content {
        MathContent::Text(text) => {
            !text.extra.is_empty() || text.runs.iter().any(|run| !run.extra.is_empty())
        }
        MathContent::Math(node) => {
            !node.extra.is_empty()
                || node.children.iter().any(|flow| {
                    !flow.extra.is_empty()
                        || flow.content.iter().any(math_content_has_unknown_metadata)
                })
        }
        MathContent::Environment(environment) => {
            !environment.extra.is_empty()
                || environment
                    .children
                    .iter()
                    .any(math_content_has_unknown_metadata)
        }
        MathContent::SoftBreak(value) => !value.extra.is_empty(),
        MathContent::Opaque(_) => true,
    }
}

fn empty_math_operand_parent(
    tree: &MathTree,
    path: &MathSequencePath,
) -> Result<Option<(MathSequencePath, usize, bool)>, EditError> {
    let Some((MathSequenceStep::Operand { content_index, .. }, parent_steps)) =
        path.steps().split_last()
    else {
        return Ok(None);
    };
    let parent_path = MathSequencePath::new(parent_steps.to_vec());
    let parent = MathTreePosition {
        host: MathPositionHost::Equation {
            owner_id: tree.root.id.clone(),
        },
        path: parent_path.clone(),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    }
    .resolve(tree)
    .map_err(|_| EditError::InvalidPosition)?;
    let Some(MathContent::Math(node)) = parent.get(*content_index) else {
        return Err(EditError::InvalidPosition);
    };
    let empty_structure = node.children.iter().all(|flow| {
        flow.content.iter().all(|content| {
            matches!(content, MathContent::Math(child) if child.kind == MathNodeKind::Placeholder)
        })
    });
    Ok(Some((parent_path, *content_index, empty_structure)))
}

fn math_text_caret_step(
    tree: &caret_doc::MathTree,
    path: &MathTextFlowPath,
    cursor: usize,
    direction: Direction,
) -> Result<MathTextCaretStep, EditError> {
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    let current_range = island
        .ranges
        .get(island.current_index)
        .ok_or(EditError::InvalidPosition)?;
    let current_display = island
        .display
        .get(current_range.clone())
        .ok_or(EditError::InvalidPosition)?;
    if cursor > current_display.len() || !current_display.is_char_boundary(cursor) {
        return Err(EditError::InvalidPosition);
    }
    let local_step = match direction {
        Direction::Forward => next_grapheme_end(current_display, cursor),
        Direction::Backward => previous_grapheme_start(current_display, cursor),
    };
    let global_cursor = current_range
        .start
        .checked_add(cursor)
        .ok_or(EditError::InvalidPosition)?;
    if global_cursor > current_range.end {
        return Err(EditError::InvalidPosition);
    }

    let local_target = local_step
        .map(|offset| {
            current_range
                .start
                .checked_add(offset)
                .ok_or(EditError::InvalidPosition)
        })
        .transpose()?;
    let current_is_boundary = is_grapheme_boundary(&island.display, global_cursor);
    let crosses_a_logical_grapheme_boundary = !current_is_boundary
        || local_target.is_some_and(|offset| !is_grapheme_boundary(&island.display, offset));
    if !crosses_a_logical_grapheme_boundary {
        return Ok(local_step.map_or(MathTextCaretStep::Boundary, MathTextCaretStep::Local));
    }

    let next = match direction {
        Direction::Forward => next_grapheme_end(&island.display, global_cursor),
        Direction::Backward => previous_grapheme_start(&island.display, global_cursor),
    };
    let Some(next) = next else {
        return Ok(MathTextCaretStep::Boundary);
    };
    let target_index = match direction {
        Direction::Forward => island
            .ranges
            .iter()
            .position(|range| range.start <= next && next < range.end)
            .or_else(|| island.ranges.iter().rposition(|range| next == range.end)),
        Direction::Backward => island
            .ranges
            .iter()
            .rposition(|range| range.start < next && next <= range.end)
            .or_else(|| island.ranges.iter().position(|range| next == range.start)),
    }
    .ok_or(EditError::InvalidPosition)?;
    let local = next
        .checked_sub(island.ranges[target_index].start)
        .ok_or(EditError::InvalidPosition)?;
    Ok(MathTextCaretStep::CrossFlow(
        island.paths[target_index].clone(),
        local,
    ))
}

fn math_text_offsets_are_boundaries(
    tree: &caret_doc::MathTree,
    path: &MathTextFlowPath,
    offsets: &[usize],
) -> bool {
    let Some(text) = path.resolve_structurally(tree) else {
        return false;
    };
    let display = text
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    let Some(island) = path::contiguous_text_flow_text(tree, path) else {
        return false;
    };
    let Some(base) = island
        .ranges
        .get(island.current_index)
        .map(|range| range.start)
    else {
        return false;
    };
    offsets.iter().all(|offset| {
        *offset <= display.len()
            && display.is_char_boundary(*offset)
            && base
                .checked_add(*offset)
                .is_some_and(|global| is_grapheme_boundary(&island.display, global))
    })
}

/// 将 TextFlow 分支两端转换为同一文本岛内的 UTF-8 字节范围。
fn math_text_selection_range(
    tree: &caret_doc::MathTree,
    focus_path: &MathTextFlowPath,
    anchor: &MathTextPosition,
    focus_offset: usize,
) -> Option<(path::TextIslandSnapshot, Range<usize>)> {
    let island = path::contiguous_text_flow_text(tree, focus_path)?;
    let anchor_offset = island.global_offset(anchor)?;
    let focus_offset = island.global_offset(&MathTextPosition::new(
        focus_path.clone(),
        focus_offset,
        MathPositionAffinity::Downstream,
    ))?;
    if !is_grapheme_boundary(&island.display, anchor_offset)
        || !is_grapheme_boundary(&island.display, focus_offset)
    {
        return None;
    }
    Some((
        island,
        anchor_offset.min(focus_offset)..anchor_offset.max(focus_offset),
    ))
}

pub(super) const ALIGN_BEGIN: &str = "\\begin{aligned}";
const ALIGN_END: &str = "\\end{aligned}";
/// 对齐公式里 Enter 插入的换行分隔符。
pub(super) const ALIGN_ROW_BREAK: &str = " \\\\ ";

/// 正在编辑的公式对象。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathEditTarget {
    /// 段落中的行内公式。
    Inline {
        paragraph_id: NodeId,
        math_id: NodeId,
    },
    /// 显示公式块；`align` 为真时是多行对齐公式，源码只含 `aligned` 环境内部。
    Block { block_id: NodeId, align: bool },
}

/// 数学编辑器当前操作的文本载体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathEditBranch {
    /// 编辑 LaTeX 源码；`MathEdit.cursor` 是源码 UTF-8 字节偏移。
    Source,
    /// 编辑公式树内的 TextFlow；焦点使用当前流本地坐标，锚点记录自己的宿主流和坐标。
    Text {
        path: MathTextFlowPath,
        anchor: MathTextPosition,
        focus_affinity: MathPositionAffinity,
        /// Parley 的锚点视觉侧，用于双向文本中的选区折叠与撤销恢复。
        anchor_visual_affinity: VisualAffinity,
        /// Parley 的视觉侧；同一双向文本字节边界可对应两个不同插入点。
        visual_affinity: VisualAffinity,
    },
    /// 以结构路径定位数学内容序列；`MathEdit.cursor` 是该序列中的边界索引。
    Tree {
        path: MathSequencePath,
        anchor: MathTreePosition,
    },
}

struct MathSourceWriteOptions {
    text_edit: Option<MathTextEdit>,
    before_math_position_override: Option<MathEditPosition>,
    reject_unmapped_metadata: bool,
}

/// 标识源码投影对应的会话与文档 generation。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MathEditRevision {
    pub(super) session_id: NodeId,
    pub(super) document_generation: u64,
    pub(super) projection_id: NodeId,
}

/// 公式编辑状态：目标、源码投影、当前编辑载体和载体定义的位置坐标。不属于文档修订。
#[derive(Debug, Clone)]
pub struct MathEdit {
    pub target: MathEditTarget,
    pub source_projection: Option<Arc<caret_math::MathSourceMap<'static>>>,
    pub(super) source_draft: Option<MathSourceDraft>,
    /// 绑定投影的会话与文档 generation；不一致时拒绝向该投影应用编辑。
    pub(super) revision: MathEditRevision,
    pub cursor: usize,
    /// 光标所属的数学树分支；源码和 TextFlow 分别使用各自的字节坐标。
    pub branch: MathEditBranch,
}

impl PartialEq for MathEdit {
    // Revision 用于拒绝过期编辑，不属于用户可见的公式编辑状态。
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target
            && self.source_projection == other.source_projection
            && self.source_draft == other.source_draft
            && self.cursor == other.cursor
            && self.branch == other.branch
    }
}

impl Eq for MathEdit {}

impl MathEdit {
    pub(super) fn replace_source_projection(
        &mut self,
        projection: Option<Arc<caret_math::MathSourceMap<'static>>>,
    ) {
        self.source_projection = projection;
        self.revision.projection_id = NodeId::generate();
    }

    fn required_source_projection(&self) -> Result<&caret_math::MathSourceMap<'static>, EditError> {
        self.source_projection
            .as_deref()
            .ok_or(EditError::InvalidPosition)
    }

    fn command_snapshot(&self) -> Self {
        Self {
            target: self.target.clone(),
            source_projection: self.source_projection.clone(),
            source_draft: self
                .source_draft
                .as_ref()
                .map(MathSourceDraft::command_snapshot),
            revision: self.revision.clone(),
            cursor: self.cursor,
            branch: self.branch.clone(),
        }
    }

    /// 当前编辑分支对应的源码视图；Align 不包含环境包围命令。
    pub fn source(&self) -> Option<&str> {
        self.source_draft
            .as_ref()
            .map(MathSourceDraft::source)
            .or_else(|| {
                self.source_projection
                    .as_ref()
                    .map(|projection| projection.edit_source())
            })
    }

    fn record_source_draft(
        &mut self,
        next_source: String,
        next_cursor: usize,
    ) -> Result<(), EditError> {
        if self.source_draft.is_none() {
            let source = self.source().ok_or(EditError::InvalidPosition)?;
            self.source_draft = Some(MathSourceDraft::new(source.to_owned(), self.cursor));
        }
        let draft = self
            .source_draft
            .as_mut()
            .ok_or(EditError::InvalidPosition)?;
        draft.record(next_source, self.cursor, next_cursor)?;
        self.cursor = next_cursor;
        self.branch = MathEditBranch::Source;
        Ok(())
    }

    pub(super) fn undo_source_draft(&mut self) -> Result<bool, EditError> {
        let Some(draft) = self.source_draft.as_mut() else {
            return Ok(false);
        };
        let Some(cursor) = draft.undo()? else {
            return Ok(false);
        };
        self.cursor = cursor;
        Ok(true)
    }

    pub(super) fn redo_source_draft(&mut self) -> Result<bool, EditError> {
        let Some(draft) = self.source_draft.as_mut() else {
            return Ok(false);
        };
        let Some(cursor) = draft.redo()? else {
            return Ok(false);
        };
        self.cursor = cursor;
        Ok(true)
    }

    pub(super) fn source_draft_can_undo(&self) -> bool {
        self.source_draft
            .as_ref()
            .is_some_and(MathSourceDraft::can_undo)
    }

    pub(super) fn source_draft_can_redo(&self) -> bool {
        self.source_draft
            .as_ref()
            .is_some_and(MathSourceDraft::can_redo)
    }

    pub(super) fn source_draft_is_pending(&self) -> bool {
        self.source_draft.as_ref().is_some_and(|draft| {
            self.source_projection
                .as_ref()
                .is_none_or(|projection| draft.source() != projection.edit_source())
        })
    }

    /// 对齐公式中光标所在的行、单元格及单元格内偏移；单元格源码已去掉首尾空白。
    pub fn align_cell_at_cursor(&self) -> Option<(usize, usize, usize)> {
        if self.source_draft_is_pending()
            || !matches!(self.target, MathEditTarget::Block { align: true, .. })
            || !matches!(self.branch, MathEditBranch::Source)
        {
            return None;
        }
        align_cell_at(self.source_projection.as_deref()?, self.cursor)
    }
}

/// 历史位置记录的源码投影身份是否与当前投影一致。
fn projection_identity_matches(
    identity: Option<usize>,
    projection: &Arc<caret_math::MathSourceMap<'static>>,
) -> bool {
    identity == Some(history::projection_identity(projection))
}

pub(super) fn restore_math_edit(
    document: &caret_doc::Document,
    position: MathEditPosition,
    revision: MathEditRevision,
) -> Option<MathEdit> {
    if let MathEditBranch::Tree { path, anchor } = &position.branch {
        if position.projection_identity.is_some()
            || position.source_projection.is_some()
            || path != &anchor.path
        {
            return None;
        }
        let tree = match (&position.target, &anchor.host) {
            (
                MathEditTarget::Inline {
                    paragraph_id,
                    math_id,
                },
                MathPositionHost::InlineMath { owner_id },
            ) if math_id == owner_id => {
                &find_inline_math_in_blocks(&document.body, paragraph_id, math_id)?.math_tree
            }
            (
                MathEditTarget::Block {
                    block_id,
                    align: false,
                },
                MathPositionHost::Equation { owner_id },
            ) if block_id == owner_id => {
                let Block::Equation(equation) = find_block_by_id(&document.body, block_id)? else {
                    return None;
                };
                &equation.math_tree
            }
            (
                MathEditTarget::Block {
                    block_id,
                    align: true,
                },
                MathPositionHost::AlignCell { row_id, cell_id },
            ) => {
                let Block::Align(align) = find_block_by_id(&document.body, block_id)? else {
                    return None;
                };
                align.rows.iter().find_map(|row| {
                    (row.id == *row_id)
                        .then(|| row.cells.iter().find(|cell| cell.root.id == *cell_id))?
                })?
            }
            _ => return None,
        };
        let mut focus = anchor.clone();
        focus.offset = position.cursor;
        if focus.resolve(tree).is_err() || anchor.resolve(tree).is_err() {
            return None;
        }
        return Some(MathEdit {
            target: position.target,
            source_projection: None,
            source_draft: None,
            revision,
            cursor: position.cursor,
            branch: position.branch,
        });
    }

    if let MathEditBranch::Text { path, anchor, .. } = &position.branch {
        if position.projection_identity.is_some() || position.source_projection.is_some() {
            return None;
        }
        let tree = match (&position.target, path.align_cell()) {
            (
                MathEditTarget::Inline {
                    paragraph_id,
                    math_id,
                },
                None,
            ) => &find_inline_math_in_blocks(&document.body, paragraph_id, math_id)?.math_tree,
            (
                MathEditTarget::Block {
                    block_id,
                    align: false,
                },
                None,
            ) => {
                let Block::Equation(equation) = find_block_by_id(&document.body, block_id)? else {
                    return None;
                };
                &equation.math_tree
            }
            (
                MathEditTarget::Block {
                    block_id,
                    align: true,
                },
                Some((row, cell)),
            ) => {
                let Block::Align(align) = find_block_by_id(&document.body, block_id)? else {
                    return None;
                };
                align.rows.get(row)?.cells.get(cell)?
            }
            _ => return None,
        };
        math_text_selection_range(tree, path, anchor, position.cursor)?;
        return Some(MathEdit {
            target: position.target,
            source_projection: None,
            source_draft: None,
            revision,
            cursor: position.cursor,
            branch: position.branch,
        });
    }

    let MathEditBranch::Source = &position.branch else {
        return None;
    };
    let stored_projection = position.source_projection.as_ref()?;
    if !projection_identity_matches(position.projection_identity, stored_projection) {
        return None;
    }
    let (kind, projected_target) =
        history_projection_for_target(document, &position.target, stored_projection)?;
    let source_projection = stored_projection;
    if source_projection.kind() != kind
        || !projection_identity_matches(position.projection_identity, source_projection)
    {
        return None;
    }
    let requires_rebinding = match &projected_target {
        HistoryProjectionTarget::Tree { source_map, .. } => {
            let stored = source_projection.tree()?;
            if !stored.same_locations(source_map) {
                return None;
            }
            stored != source_map
        }
        HistoryProjectionTarget::Align { source_map, .. } => {
            let stored = source_projection.align()?;
            if !stored.same_locations(source_map) {
                return None;
            }
            stored != source_map
        }
    };
    let source_projection = if requires_rebinding {
        let mut projection = (**source_projection).clone();
        match &projected_target {
            HistoryProjectionTarget::Tree { tree, source_map } => {
                if !projection.bind_tree_sidecar_after_validated_edit(tree, source_map.clone()) {
                    return None;
                }
            }
            HistoryProjectionTarget::Align { align, source_map } => {
                let mapped = align
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(row_index, row)| {
                        (0..row.cells.len())
                            .map(|cell_index| source_map.cell_tree(row_index, cell_index).cloned())
                            .collect::<Option<Vec<_>>>()
                    })
                    .collect::<Option<Vec<_>>>()?;
                if !projection.bind_align_sidecars_after_validated_edit(align, mapped) {
                    return None;
                }
            }
        }
        Arc::new(projection)
    } else {
        Arc::clone(source_projection)
    };
    let edit_source = source_projection.edit_source();
    if position.cursor > edit_source.len() || !edit_source.is_char_boundary(position.cursor) {
        return None;
    }
    Some(MathEdit {
        target: position.target,
        source_projection: Some(source_projection),
        source_draft: None,
        revision,
        cursor: position.cursor,
        branch: MathEditBranch::Source,
    })
}

enum HistoryProjectionTarget {
    Tree {
        tree: caret_doc::MathTree,
        source_map: caret_math::MathTreeSourceMap,
    },
    Align {
        align: Align,
        source_map: caret_math::AlignSourceMap,
    },
}

fn history_projection_for_target(
    document: &caret_doc::Document,
    target: &MathEditTarget,
    stored_projection: &caret_math::MathSourceMap<'_>,
) -> Option<(caret_math::MathInputKind, HistoryProjectionTarget)> {
    match target {
        MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } => {
            let tree =
                &find_inline_math_in_blocks(&document.body, paragraph_id, math_id)?.math_tree;
            let (tree, source_map) =
                crate::math_validation::tree_for_source_history_projection_with_map(
                    tree,
                    stored_projection,
                    caret_math::MathInputKind::Inline,
                )?;
            Some((
                caret_math::MathInputKind::Inline,
                HistoryProjectionTarget::Tree { tree, source_map },
            ))
        }
        MathEditTarget::Block { block_id, align } => {
            match (find_block_by_id(&document.body, block_id)?, align) {
                (Block::Equation(equation), false) => {
                    let (tree, source_map) =
                        crate::math_validation::tree_for_source_history_projection_with_map(
                            &equation.math_tree,
                            stored_projection,
                            caret_math::MathInputKind::Equation,
                        )?;
                    Some((
                        caret_math::MathInputKind::Equation,
                        HistoryProjectionTarget::Tree { tree, source_map },
                    ))
                }
                (Block::Align(align), true) => {
                    let (align, source_map) =
                        crate::math_validation::align_for_source_history_projection_with_map(
                            align,
                            stored_projection,
                        )?;
                    Some((
                        caret_math::MathInputKind::Align,
                        HistoryProjectionTarget::Align { align, source_map },
                    ))
                }
                _ => None,
            }
        }
    }
}

pub(super) fn math_tree_for_target<'a>(
    document: &'a caret_doc::Document,
    target: &MathEditTarget,
    align_cell: Option<(usize, usize)>,
) -> Option<&'a caret_doc::MathTree> {
    match target {
        MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } => {
            if align_cell.is_some() {
                return None;
            }
            find_inline_math_in_blocks(&document.body, paragraph_id, math_id)
                .map(|math| &math.math_tree)
        }
        MathEditTarget::Block { block_id, align } => {
            let block = find_block_by_id(&document.body, block_id)?;
            match (block, align, align_cell) {
                (Block::Equation(equation), false, None) => Some(&equation.math_tree),
                (Block::Align(value), true, Some((row, cell))) => {
                    value.rows.get(row)?.cells.get(cell)
                }
                _ => None,
            }
        }
    }
}

pub(super) fn math_source_body_range(edit: &MathEdit) -> Option<Range<usize>> {
    navigation::math_input_body_range(edit)
}

pub(super) fn history_position(edit: &MathEdit) -> MathEditPosition {
    MathEditPosition::from_edit(edit)
}

fn find_inline_math_in_blocks<'a>(
    blocks: &'a [Block],
    paragraph_id: &NodeId,
    math_id: &NodeId,
) -> Option<&'a InlineMath> {
    blocks.iter().find_map(|block| match block {
        Block::Paragraph(paragraph) if &paragraph.id == paragraph_id => {
            find_inline_math(&paragraph.inlines, math_id)
        }
        Block::Environment(environment) => environment
            .title
            .as_ref()
            .filter(|title| &title.id == paragraph_id)
            .and_then(|title| find_inline_math(&title.inlines, math_id))
            .or_else(|| find_inline_math_in_blocks(&environment.children, paragraph_id, math_id)),
        _ => None,
    })
}

pub(super) fn find_block_by_id<'a>(blocks: &'a [Block], id: &NodeId) -> Option<&'a Block> {
    let (parents, index) = find_block_path(blocks, id)?;
    block_list_ref(blocks, &parents).ok()?.get(index)
}

fn parked_position_for_math_target(
    session: &Session,
    target: &MathEditTarget,
) -> Result<Option<Pos>, EditError> {
    match target {
        MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } => {
            let story = session
                .story_for_paragraph(paragraph_id)
                .ok_or(EditError::InvalidPosition)?;
            let probe = Pos {
                story,
                paragraph_id: paragraph_id.clone(),
                offset: 0,
                affinity: Affinity::Downstream,
            };
            let start = inline_math_slots(session.paragraph_for(&probe)?)?
                .into_iter()
                .find(|(_, id)| id == math_id)
                .map(|(start, _)| start)
                .ok_or(EditError::InvalidPosition)?;
            Ok(Some(Pos {
                offset: start + OBJECT_REPLACEMENT.len_utf8(),
                affinity: Affinity::Upstream,
                ..probe
            }))
        }
        MathEditTarget::Block { block_id, .. } => {
            let forward = session.paragraph_beside_math_chain(block_id, Direction::Forward)?;
            match forward {
                Some(position) => Ok(Some(position)),
                None => session.paragraph_beside_math_chain(block_id, Direction::Backward),
            }
        }
    }
}

impl Session {
    /// 当前公式编辑状态；为 `None` 时光标在正文中。
    pub fn math_edit(&self) -> Option<&MathEdit> {
        self.math_edit.as_ref()
    }

    /// 从同一布局代际的数学结构命中进入树原生编辑。
    ///
    /// 命中须属于当前会话、文档和字体代际，并解析到有效的 MathFlow 插入位置；拒绝时
    /// 不改变文档、选择或编辑状态。
    pub fn focus_math_structure_hit(
        &mut self,
        hit: &crate::PageMathStructureHit,
        current_font_generation: u64,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        self.ensure_math_edit_revision_current()?;
        if self.command_line.is_some() {
            return Err(EditError::UnknownCommand);
        }
        if hit.session_id.as_ref() != Some(&self.session_id)
            || hit.document_generation != self.document_generation
            || hit.font_generation != current_font_generation
            || self.has_uncommitted_math_source_draft()
        {
            return Err(EditError::InvalidPosition);
        }
        let align_cell = self.align_cell_for_target_and_host(&hit.target, &hit.position.host)?;
        let tree = math_tree_for_target(&self.document, &hit.target, align_cell)
            .ok_or(EditError::InvalidPosition)?;
        let contents = hit
            .position
            .resolve(tree)
            .map_err(|_| EditError::InvalidPosition)?;
        if hit.position.offset > contents.len() {
            return Err(EditError::InvalidPosition);
        }
        // 必填空槽的 Placeholder 后紧邻边界不可编辑；紧邻它的后续内容仍保留合法边界。
        let mut focused_position = hit.position.clone();
        if let Some(placeholder_index) =
            tree_position::placeholder_run_start(contents, focused_position.offset)
        {
            focused_position.offset = placeholder_index;
            focused_position.affinity = if focused_position.offset == 0 {
                MathPositionAffinity::Downstream
            } else {
                MathPositionAffinity::Upstream
            };
        }
        if self.math_edit.as_ref().is_some_and(|edit| {
            edit.target == hit.target
                && matches!(&edit.branch, MathEditBranch::Tree { anchor, .. } if anchor == &focused_position)
                && self.selection.anchor == self.selection.focus
        }) {
            return Ok(Effect::default());
        }

        let parked = parked_position_for_math_target(self, &hit.target)?;
        let position = parked.unwrap_or_else(|| self.selection.focus.clone());
        self.selection = Selection {
            anchor: position.clone(),
            focus: position.clone(),
        };
        self.selection_visual_affinity = match position.affinity {
            Affinity::Upstream => VisualAffinity::Upstream,
            Affinity::Downstream => VisualAffinity::Downstream,
        };
        self.math_edit = Some(MathEdit {
            target: hit.target.clone(),
            source_projection: None,
            source_draft: None,
            revision: self.current_math_edit_revision(),
            cursor: focused_position.offset,
            branch: MathEditBranch::Tree {
                path: focused_position.path.clone(),
                anchor: focused_position,
            },
        });
        self.pending_inline_style.clear();
        self.clear_structure_selection_state();
        self.host_caret_position = None;
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    /// 从同一当前布局代际的公式文本命中进入树原生编辑。
    ///
    /// 调用方须先确认快照的字体代际仍有效；此方法校验命中会话、文档代际、宿主身份、
    /// TextFlow 路径及字素边界。任何校验失败都不改变选区或编辑状态。
    pub fn focus_math_text_hit(
        &mut self,
        hit: &crate::PageMathTextHit,
        current_font_generation: u64,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        self.ensure_math_edit_revision_current()?;
        if self.command_line.is_some() {
            return Err(EditError::UnknownCommand);
        }
        if hit.session_id.as_ref() != Some(&self.session_id)
            || hit.document_generation != self.document_generation
            || hit.font_generation != current_font_generation
            || self.has_uncommitted_math_source_draft()
        {
            return Err(EditError::InvalidPosition);
        }

        let align_cell = hit.position.path.align_cell();
        let expected_host = match (&hit.target, align_cell) {
            (MathEditTarget::Inline { math_id, .. }, None) => MathPositionHost::InlineMath {
                owner_id: math_id.clone(),
            },
            (
                MathEditTarget::Block {
                    block_id,
                    align: false,
                },
                None,
            ) => MathPositionHost::Equation {
                owner_id: block_id.clone(),
            },
            (
                MathEditTarget::Block {
                    block_id,
                    align: true,
                },
                Some((row, cell)),
            ) => {
                let Block::Align(value) = find_block_by_id(&self.document.body, block_id)
                    .ok_or(EditError::InvalidPosition)?
                else {
                    return Err(EditError::InvalidPosition);
                };
                let row = value.rows.get(row).ok_or(EditError::InvalidPosition)?;
                let cell = row.cells.get(cell).ok_or(EditError::InvalidPosition)?;
                MathPositionHost::AlignCell {
                    row_id: row.id.clone(),
                    cell_id: cell.root.id.clone(),
                }
            }
            _ => return Err(EditError::InvalidPosition),
        };
        if expected_host != hit.host {
            return Err(EditError::InvalidPosition);
        }
        let tree = math_tree_for_target(&self.document, &hit.target, align_cell)
            .ok_or(EditError::InvalidPosition)?;
        let island = path::contiguous_text_flow_text(tree, &hit.position.path)
            .ok_or(EditError::InvalidPosition)?;
        let island_offset = island
            .global_offset(&hit.position)
            .ok_or(EditError::InvalidPosition)?;
        if !is_grapheme_boundary(&island.display, island_offset) {
            return Err(EditError::InvalidPosition);
        }

        if let Some(edit) = &mut self.math_edit
            && edit.target == hit.target
            && let MathEditBranch::Text {
                path,
                anchor,
                focus_affinity,
                anchor_visual_affinity,
                visual_affinity,
            } = &mut edit.branch
            && *path == hit.position.path
            && *anchor == hit.position
            && edit.cursor == hit.position.offset
            && *focus_affinity == hit.position.affinity
            && *anchor_visual_affinity == hit.visual_affinity
            && *visual_affinity == hit.visual_affinity
            && self.selection.anchor == self.selection.focus
        {
            return Ok(Effect::default());
        }

        if let Some(edit) = &mut self.math_edit
            && edit.target == hit.target
            && let MathEditBranch::Text {
                path,
                anchor,
                focus_affinity,
                anchor_visual_affinity,
                visual_affinity,
            } = &mut edit.branch
            && *path == hit.position.path
            && *anchor == hit.position
            && edit.cursor == hit.position.offset
            && *focus_affinity == hit.position.affinity
        {
            *anchor_visual_affinity = hit.visual_affinity;
            *visual_affinity = hit.visual_affinity;
            self.advance_history_merge_epoch();
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }

        let position = parked_position_for_math_target(self, &hit.target)?
            .unwrap_or_else(|| self.selection.focus.clone());
        self.selection = Selection {
            anchor: position.clone(),
            focus: position.clone(),
        };
        self.selection_visual_affinity = match position.affinity {
            Affinity::Upstream => VisualAffinity::Upstream,
            Affinity::Downstream => VisualAffinity::Downstream,
        };
        self.math_edit = Some(MathEdit {
            target: hit.target.clone(),
            source_projection: None,
            source_draft: None,
            revision: self.current_math_edit_revision(),
            cursor: hit.position.offset,
            branch: MathEditBranch::Text {
                path: hit.position.path.clone(),
                anchor: hit.position.clone(),
                focus_affinity: hit.position.affinity,
                anchor_visual_affinity: hit.visual_affinity,
                visual_affinity: hit.visual_affinity,
            },
        });
        self.pending_inline_style.clear();
        self.clear_structure_selection_state();
        self.host_caret_position = None;
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    /// 原子应用公式文本拖选端点，保留当前会话的身份、代际与历史。
    ///
    /// 两个端点先在同一候选状态中完成命中和字素边界校验；任一端点无效时，
    /// 原会话完全不变。候选副本保留身份，避免纯选区操作伪装成会话替换。
    pub fn set_math_text_selection_from_hits(
        &mut self,
        anchor_hit: &crate::PageMathTextHit,
        focus_hit: &crate::PageMathTextHit,
        current_font_generation: u64,
    ) -> Result<Effect, EditError> {
        if anchor_hit.target != focus_hit.target {
            return Err(EditError::InvalidPosition);
        }
        let mut candidate = self.clone_preserving_identity();
        let composition_effect =
            if let Some(composition_id) = candidate.composition.as_ref().map(|c| c.id) {
                candidate.cancel_composition(composition_id)?
            } else {
                Effect::default()
            };
        let anchor_effect = candidate.focus_math_text_hit(anchor_hit, current_font_generation)?;
        let focus_effect = candidate.focus_math_text_hit(focus_hit, current_font_generation)?;
        let mut effect = candidate.set_math_text_selection_positions_with_visual_affinities(
            anchor_hit.position.clone(),
            focus_hit.position.clone(),
            anchor_hit.visual_affinity,
            focus_hit.visual_affinity,
        )?;
        effect.composition_changed = composition_effect.composition_changed;
        effect.selection_changed |= composition_effect.selection_changed
            || anchor_effect.selection_changed
            || focus_effect.selection_changed;
        *self = candidate;
        Ok(effect)
    }

    /// 公式编辑期间截获输入；`None` 表示命令由 Session 的常规分派继续处理。
    pub(super) fn run_math_edit(&mut self, command: &Command) -> Result<Option<Effect>, EditError> {
        if self.math_edit.is_none() {
            return Ok(None);
        }
        let math_text_active = self.math_edit_is_text_branch();
        let composition_command = matches!(
            command,
            Command::UpdateComposition { .. }
                | Command::CommitComposition { .. }
                | Command::CancelComposition { .. }
        );
        if (self.composition.is_some() && !composition_command)
            || (composition_command && !math_text_active)
        {
            return Err(EditError::CompositionPending);
        }
        if composition_command && math_text_active {
            return match command {
                Command::UpdateComposition { id, preedit } => {
                    self.update_composition(*id, preedit.clone()).map(Some)
                }
                Command::CommitComposition { id, text } => {
                    self.commit_composition(*id, text).map(Some)
                }
                Command::CancelComposition { id } => self.cancel_composition(*id).map(Some),
                _ => unreachable!("composition_command only matches composition commands"),
            };
        }
        if self.command_line.is_some() {
            if matches!(
                command,
                Command::ToggleInlineFormat(_)
                    | Command::ToggleRunUnderline
                    | Command::SetInlineFontFamily(_)
                    | Command::RemoveInlineEnvironmentPreservingContent(_)
                    | Command::RemoveRunPropertyPreservingContent(_)
            ) {
                return Err(EditError::UnsupportedInlineFormat);
            }
            return if matches!(
                command,
                Command::BeginCommandLine
                    | Command::TypeCommandLine(_)
                    | Command::BackspaceCommandLine
                    | Command::MoveCommandLineCursor(_)
                    | Command::MoveCommandLineSelection(_)
                    | Command::SelectCommandLineCandidate(_)
                    | Command::TabCommandLine
                    | Command::AcceptCommandLine
                    | Command::CancelCommandLine
                    | Command::Undo
                    | Command::Redo
                    | Command::Delete(Direction::Backward)
            ) {
                Ok(None)
            } else {
                Err(EditError::UnknownCommand)
            };
        }
        if self
            .math_edit
            .as_ref()
            .is_some_and(MathEdit::source_draft_is_pending)
            && matches!(
                command,
                Command::SetPageGeometry(_)
                    | Command::SetDefaultFontFamily(_)
                    | Command::SetDocumentFontFamily { .. }
            )
        {
            return Err(EditError::InvalidMath(
                "resolve or cancel the incomplete math source draft before changing document settings"
                    .to_owned(),
            ));
        }
        let effect = match command {
            Command::InsertInlineMath(source) if self.math_edit_is_text_branch() => {
                self.math_text_enter_math(source)?
            }
            Command::InsertText(text) if self.math_edit_is_text_branch() => {
                self.math_text_insert(text)?
            }
            Command::PasteText(text) if self.math_edit_is_text_branch() => {
                self.math_text_insert(text)?
            }
            Command::InsertText(text) if self.math_edit_is_tree_branch() => {
                self.math_tree_insert_text(text)?
            }
            Command::PasteText(text) if self.math_edit_is_tree_branch() => {
                self.math_tree_insert_text(text)?
            }
            Command::SetMathTextSelection { anchor, focus } => {
                self.set_math_text_selection(*anchor, *focus)?
            }
            Command::InsertText(text) => self.math_insert_text(text)?,
            Command::PasteText(text) => self.math_insert_raw(text, text.len())?,
            Command::InsertSoftBreak | Command::InsertParagraphBreak => {
                return Err(EditError::UnsupportedNewline);
            }
            Command::Delete(Direction::Backward) if self.math_edit_is_text_branch() => {
                let edit = self
                    .math_edit
                    .as_ref()
                    .map(MathEdit::command_snapshot)
                    .ok_or(EditError::InvalidPosition)?;
                if let Some(effect) = multiline::handle_text_backspace(self, &edit)? {
                    return Ok(Some(effect));
                }
                self.math_text_delete(Direction::Backward)?
            }
            Command::Delete(Direction::Forward) if self.math_edit_is_text_branch() => {
                self.math_text_delete(Direction::Forward)?
            }
            Command::Delete(Direction::Backward) if self.math_edit_is_tree_branch() => {
                self.math_tree_delete(Direction::Backward)?
            }
            Command::Delete(Direction::Forward) if self.math_edit_is_tree_branch() => {
                self.math_tree_delete(Direction::Forward)?
            }
            Command::Delete(Direction::Backward) => self.math_delete_backward()?,
            Command::Delete(Direction::Forward) => self.math_delete_forward()?,
            Command::MoveCaret(direction) => self.math_move(*direction)?,
            Command::EnterMath(_) => Effect::default(),
            Command::ToggleRunUnderline if self.math_edit_is_text_branch() => {
                self.toggle_math_text_run_property(crate::editing::EnvironmentKind::Underline)?
            }
            Command::ToggleInlineFormat(kind) if self.math_edit_is_text_branch() => {
                self.toggle_math_text_inline_environment(*kind)?
            }
            Command::SetInlineFontFamily(family) if self.math_edit_is_text_branch() => {
                self.set_math_text_font_family(family)?
            }
            Command::ToggleInlineFormat(_)
            | Command::ToggleRunUnderline
            | Command::SetInlineFontFamily(_)
            | Command::RemoveInlineEnvironmentPreservingContent(_)
            | Command::RemoveRunPropertyPreservingContent(_) => {
                return Err(EditError::UnsupportedInlineFormat);
            }
            Command::InsertOneCharacterWidthSpace | Command::InsertTwoCharacterWidthSpace => {
                // 数学槽位导航尚未定义；Tab 保留公式焦点，不回退为正文空格输入。
                Effect::default()
            }
            Command::SetSelection(None) => {
                if self.math_edit_is_text_branch() {
                    let cursor = self
                        .math_edit
                        .as_ref()
                        .ok_or(EditError::InvalidPosition)?
                        .cursor;
                    self.set_math_text_selection(cursor, cursor)?
                } else {
                    Effect::default()
                }
            }
            Command::SetSelection(Some(selection)) => {
                if self
                    .math_edit
                    .as_ref()
                    .is_some_and(MathEdit::source_draft_is_pending)
                {
                    return Err(EditError::InvalidMath(
                        "resolve or cancel the incomplete math source draft before leaving"
                            .to_owned(),
                    ));
                }
                // 鼠标选区先按正文位置规则验证；失败时保留原公式编辑状态。
                let active_edit = self.math_edit.take();
                match self.set_selection(Some(selection.clone())) {
                    Ok(mut effect) => {
                        self.command_line = None;
                        self.pending_inline_style.clear();
                        self.selection_visual_affinity = match self.selection.focus.affinity {
                            Affinity::Upstream => VisualAffinity::Upstream,
                            Affinity::Downstream => VisualAffinity::Downstream,
                        };
                        self.advance_history_merge_epoch();
                        effect.selection_changed |= active_edit.is_some();
                        return Ok(Some(effect));
                    }
                    Err(error) => {
                        self.math_edit = active_edit;
                        return Err(error);
                    }
                }
            }
            Command::SetPageGeometry(_)
            | Command::SetDefaultFontFamily(_)
            | Command::SetDocumentFontFamily { .. } => return Ok(None),
            Command::BeginCommandLine => {
                self.command_line = Some(CommandLine::math(self.selection.focus.clone()));
                Effect::default()
            }
            // 编辑目标仍是公式时，拒绝尚未适配的命令，避免它们落到公式宿主正文。
            _ => return Err(EditError::UnknownCommand),
        };
        Ok(Some(effect))
    }

    /// `\math`：插入空行内公式并进入编辑。
    pub(super) fn begin_inline_math(&mut self) -> Result<Effect, EditError> {
        self.run_with_inline_math_focus(Command::InsertInlineMath(String::new()), true)
    }

    /// `\equation` / `\align`：在当前正文段落之后插入空显示公式块并进入编辑。
    /// 当前段落为空时公式插在它之前，空段落留作公式后的续写位置。
    pub(super) fn insert_display_math(&mut self, align: bool) -> Result<Effect, EditError> {
        let plan = self.prepare_display_math_insert(align)?;
        let before_selection = self.selection.clone();
        let before_anchor_visual_affinity = self.selection_anchor_visual_affinity;
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let blocks = block_list(&mut self.document.body, &plan.parent_environment_ids)?;
        blocks.splice(
            plan.sibling_index..=plan.sibling_index,
            plan.after.iter().cloned(),
        );
        self.selection = Selection {
            anchor: plan.next_position.clone(),
            focus: plan.next_position,
        };
        self.math_edit = Some(MathEdit {
            target: plan.target,
            source_projection: None,
            source_draft: None,
            revision: self.current_math_edit_revision(),
            cursor: plan.cursor,
            branch: plan.branch,
        });
        let index = build_paragraph_index(&self.document.body);
        self.finalize_structure_edit(
            index,
            DocumentPatch::BlockRange {
                parent_environment_ids: plan.parent_environment_ids,
                sibling_index: plan.sibling_index,
                before: vec![plan.before],
                after: plan.after,
            },
            before_selection,
            before_anchor_visual_affinity,
            before_visual_affinity,
            before_state_id,
        );
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            ..Effect::default()
        })
    }

    pub(super) fn can_insert_display_math(&self, align: bool) -> bool {
        self.prepare_display_math_insert(align).is_ok()
    }

    fn prepare_display_math_insert(&self, align: bool) -> Result<DisplayMathInsertPlan, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let focus = self.selection.focus.clone();
        if focus.story != StoryId::Body || self.selection.anchor != focus {
            return Err(EditError::InvalidPosition);
        }
        let location = self
            .paragraph_index
            .get(&focus.paragraph_id)
            .cloned()
            .ok_or_else(|| EditError::UnknownParagraph(focus.paragraph_id.to_string()))?;
        if location.is_title {
            return Err(EditError::InvalidPosition);
        }
        let (parent_ids, sibling_index) = paragraph_parent_path(&self.document.body, &location)
            .ok_or(EditError::InvalidPosition)?;
        let paragraph = self.paragraph_for(&focus)?.clone();
        let paragraph_map = TextMap::new(&paragraph)?;
        paragraph_map.validate_pos(&focus)?;
        let paragraph_empty = paragraph_map.text.is_empty();
        let block_id = NodeId::generate();
        let block = if align {
            Block::Align(Align {
                id: block_id.clone(),
                rows: vec![MathAlignRow {
                    id: NodeId::generate(),
                    cells: vec![empty_math_tree()],
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            })
        } else {
            Block::Equation(Equation {
                id: block_id.clone(),
                math_tree: empty_math_tree(),
                extra: BTreeMap::new(),
            })
        };
        let (cursor, branch) = match &block {
            Block::Equation(equation) => math_tree_edge_position(
                &equation.math_tree,
                MathPositionHost::Equation {
                    owner_id: block_id.clone(),
                },
                Direction::Forward,
                None,
            )?,
            Block::Align(align) => {
                let cell = align
                    .rows
                    .first()
                    .and_then(|row| row.cells.first())
                    .ok_or(EditError::InvalidPosition)?;
                math_tree_edge_position(
                    cell,
                    MathPositionHost::AlignCell {
                        row_id: align.rows[0].id.clone(),
                        cell_id: cell.root.id.clone(),
                    },
                    Direction::Forward,
                    Some((0, 0)),
                )?
            }
            _ => return Err(EditError::InvalidPosition),
        };
        let current = Block::Paragraph(paragraph.clone());
        let (after, next_position) = if paragraph_empty {
            (vec![block, current.clone()], focus.clone())
        } else {
            if !paragraph.extra.is_empty() {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            let (left_inlines, right_inlines) =
                split_inline_nodes(paragraph.inlines.clone(), focus.offset)?;
            let mut left = paragraph.clone();
            left.inlines = left_inlines;
            let mut next = paragraph;
            next.id = NodeId::generate();
            next.inlines = right_inlines;
            next.extra.clear();
            let position = Pos {
                story: StoryId::Body,
                paragraph_id: next.id.clone(),
                offset: 0,
                affinity: Affinity::Downstream,
            };
            (
                vec![Block::Paragraph(left), block, Block::Paragraph(next)],
                position,
            )
        };
        let mut candidate_document = self.document.clone();
        let blocks = block_list(&mut candidate_document.body, &parent_ids)?;
        blocks.splice(sibling_index..=sibling_index, after.iter().cloned());
        caret_doc::CdocFile::new(candidate_document)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
        Ok(DisplayMathInsertPlan {
            parent_environment_ids: parent_ids,
            sibling_index,
            before: current,
            after,
            next_position,
            target: MathEditTarget::Block { block_id, align },
            cursor,
            branch,
        })
    }

    /// 从正文进入相邻公式：候选必须跨过单个公式，且正文两端必须落在对应段首/段尾。
    pub(super) fn navigation_math_crossing_direction(
        &self,
        current: &Pos,
        target: &Pos,
    ) -> Result<Option<Direction>, EditError> {
        if current.story != target.story {
            return Ok(None);
        }
        if current.paragraph_id == target.paragraph_id {
            let object_len = OBJECT_REPLACEMENT.len_utf8();
            for (start, _) in inline_math_slots(self.paragraph_for(current)?)? {
                let Some(end) = start.checked_add(object_len) else {
                    return Err(EditError::InvalidPosition);
                };
                if current.offset == start && target.offset == end {
                    return Ok(Some(Direction::Forward));
                }
                if current.offset == end && target.offset == start {
                    return Ok(Some(Direction::Backward));
                }
            }
            return Ok(None);
        }
        if current.story != StoryId::Body || target.story != StoryId::Body {
            return Ok(None);
        }

        let Some(current_location) = self.paragraph_index.get(&current.paragraph_id) else {
            return Ok(None);
        };
        let Some(target_location) = self.paragraph_index.get(&target.paragraph_id) else {
            return Ok(None);
        };
        let Some((current_parents, current_index)) =
            paragraph_parent_path(&self.document.body, current_location)
        else {
            return Ok(None);
        };
        let Some((target_parents, target_index)) =
            paragraph_parent_path(&self.document.body, target_location)
        else {
            return Ok(None);
        };
        if current_parents != target_parents {
            return Ok(None);
        }
        let (math_index, direction) = if target_index == current_index.saturating_add(2) {
            (current_index + 1, Direction::Forward)
        } else if current_index == target_index.saturating_add(2) {
            (target_index + 1, Direction::Backward)
        } else {
            return Ok(None);
        };
        let blocks = block_list_ref(&self.document.body, &current_parents)?;
        if !matches!(
            blocks.get(math_index),
            Some(Block::Equation(_) | Block::Align(_))
        ) {
            return Ok(None);
        }
        let current_len = TextMap::new(self.paragraph_for(current)?)?.text.len();
        let target_len = TextMap::new(self.paragraph_for(target)?)?.text.len();
        let crosses_at_host_edges = match direction {
            Direction::Forward => current.offset == current_len && target.offset == 0,
            Direction::Backward => current.offset == 0 && target.offset == target_len,
        };
        if !crosses_at_host_edges {
            return Err(EditError::InvalidPosition);
        }
        Ok(Some(direction))
    }

    /// 返回当前停靠点关联的公式块，供 app 在生成视觉导航候选前委托 Session 仲裁。
    pub fn math_block_boundary_exit(&self) -> Option<(NodeId, Direction)> {
        let position = self.host_caret_position.as_ref()?;
        if position.kind() != HostCaretKind::Block {
            return None;
        }
        let (block_id, direction) = position.as_outside()?;
        let (parent_ids, index) = find_block_path(&self.document.body, block_id)?;
        matches!(
            block_list_ref(&self.document.body, &parent_ids)
                .ok()?
                .get(index),
            Some(Block::Equation(_) | Block::Align(_))
        )
        .then(|| (block_id.clone(), direction))
    }

    /// 仅当当前正文焦点紧邻受支持的公式对象时进入该公式。
    pub(super) fn enter_adjacent_math_if_present(
        &mut self,
        direction: Direction,
    ) -> Result<bool, EditError> {
        self.enter_adjacent_math_with_inline(direction, true, true, true)
    }

    /// 仅当正文焦点位于文档块边界且相邻块是显示公式时进入该公式。
    pub(super) fn enter_adjacent_block_math_if_present(
        &mut self,
        direction: Direction,
    ) -> Result<bool, EditError> {
        self.enter_adjacent_math_with_inline(direction, false, false, true)
    }

    fn enter_adjacent_math_with_inline(
        &mut self,
        direction: Direction,
        include_inline: bool,
        require_text_edge: bool,
        stop_before_entering: bool,
    ) -> Result<bool, EditError> {
        if self.selection.anchor != self.selection.focus
            || self.math_edit.is_some()
            || self.has_structure_selection()
            || !self.selected_block_environments.is_empty()
            || !self.selected_math_blocks.is_empty()
            || self.composition.is_some()
            || self.command_line.is_some()
        {
            return Ok(false);
        }
        let focus = self.selection.focus.clone();
        let paragraph = self.paragraph_for(&focus)?;
        let object_len = OBJECT_REPLACEMENT.len_utf8();
        if self.block_boundary_exit().is_none()
            && include_inline
            && inline_math_slots(paragraph)?
                .iter()
                .any(|(start, _)| match direction {
                    Direction::Forward => *start == focus.offset,
                    Direction::Backward => start + object_len == focus.offset,
                })
        {
            self.enter_math_with_constraints(direction, true, true)?;
            return Ok(self.math_edit.is_some());
        }
        let at_edge = !require_text_edge
            || match direction {
                Direction::Forward => focus.offset == TextMap::new(paragraph)?.text.len(),
                Direction::Backward => focus.offset == 0,
            };
        if !at_edge || focus.story != StoryId::Body {
            return Ok(false);
        }
        let Some(location) = self.paragraph_index.get(&focus.paragraph_id) else {
            return Ok(false);
        };
        if location.is_title {
            return Ok(false);
        }
        let Some((parent_ids, sibling_index)) =
            paragraph_parent_path(&self.document.body, location)
        else {
            return Ok(false);
        };
        let neighbour = match direction {
            Direction::Forward => sibling_index.checked_add(1),
            Direction::Backward => sibling_index.checked_sub(1),
        };
        let Some(neighbour) = neighbour else {
            return Ok(false);
        };
        let blocks = block_list_ref(&self.document.body, &parent_ids)?;
        if !matches!(
            blocks.get(neighbour),
            Some(Block::Equation(_) | Block::Align(_))
        ) {
            return Ok(false);
        }
        let target_id = match blocks.get(neighbour) {
            Some(Block::Equation(equation)) => equation.id.clone(),
            Some(Block::Align(align)) => align.id.clone(),
            _ => unreachable!("the adjacent math block was checked above"),
        };
        let opposite = match direction {
            Direction::Backward => Direction::Forward,
            Direction::Forward => Direction::Backward,
        };
        match self.host_caret_position.clone() {
            Some(position) if position.kind() == HostCaretKind::Inline => {
                if position.outside_direction() != Some(direction) {
                    return Ok(false);
                }
                if stop_before_entering {
                    self.host_caret_position =
                        Some(HostCaretPosition::outside(target_id, opposite));
                    self.advance_history_merge_epoch();
                    return Ok(true);
                }
                if let Some(edit) = self.math_edit_at_block(&target_id, direction)? {
                    self.math_edit = Some(edit);
                    self.host_caret_position = None;
                    self.advance_history_merge_epoch();
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Some(position)
                if position.kind() == HostCaretKind::Block && position.host_id() == &target_id =>
            {
                if let Some((_, exit_direction)) = position.as_outside() {
                    if direction == exit_direction {
                        Ok(true)
                    } else if let Some(edit) = self.math_edit_at_block(&target_id, direction)? {
                        self.host_caret_position = None;
                        self.math_edit = Some(edit);
                        self.advance_history_merge_epoch();
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                } else {
                    Ok(false)
                }
            }
            Some(_) => Ok(false),
            None if stop_before_entering => {
                self.host_caret_position = Some(HostCaretPosition::outside(target_id, opposite));
                self.advance_history_merge_epoch();
                Ok(true)
            }
            None => {
                let edit = self
                    .math_edit_at_block(&target_id, direction)?
                    .ok_or(EditError::InvalidPosition)?;
                self.math_edit = Some(edit);
                self.advance_history_merge_epoch();
                Ok(true)
            }
        }
    }

    pub(super) fn enter_math(&mut self, direction: Direction) -> Result<Effect, EditError> {
        if let Some(position) = self.host_caret_position.clone()
            && position.kind() == HostCaretKind::Block
            && let Some((block_id, _)) = position.as_outside()
            && let Some(edit) = self.math_edit_at_block(block_id, direction)?
        {
            self.host_caret_position = None;
            self.math_edit = Some(edit);
            self.advance_history_merge_epoch();
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        if self.enter_adjacent_math_with_inline(direction, true, true, false)? {
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        self.enter_math_with_constraints(direction, true, true)
    }

    fn enter_math_with_constraints(
        &mut self,
        direction: Direction,
        include_inline: bool,
        require_text_edge: bool,
    ) -> Result<Effect, EditError> {
        let focus = self.selection.focus.clone();
        if self.selection.anchor != focus
            || self.has_structure_selection()
            || !self.selected_block_environments.is_empty()
            || !self.selected_math_blocks.is_empty()
            || self.composition.is_some()
            || self.command_line.is_some()
        {
            return Ok(Effect::default());
        }
        let paragraph = self.paragraph_for(&focus)?.clone();
        let object_len = OBJECT_REPLACEMENT.len_utf8();
        let slot = if include_inline {
            inline_math_slots(&paragraph)?
                .into_iter()
                .find(|(start, _)| match direction {
                    Direction::Forward => *start == focus.offset,
                    Direction::Backward => start + object_len == focus.offset,
                })
        } else {
            None
        };
        if let Some(position) = self.host_caret_position.clone() {
            let can_enter_adjacent_inline_math = slot.is_some()
                && position.kind() == HostCaretKind::Inline
                && position.outside_direction() == Some(direction)
                && self
                    .text_map_for(&focus)?
                    .is_empty_inline_environment(focus.offset, position.host_id());
            if !can_enter_adjacent_inline_math {
                return Ok(Effect::default());
            }
        }
        if let Some((start, math_id)) = slot {
            let tree = find_inline_math(&paragraph.inlines, &math_id)
                .ok_or(EditError::InvalidPosition)?
                .math_tree
                .clone();
            let (cursor, branch) = math_tree_edge_position(
                &tree,
                MathPositionHost::InlineMath {
                    owner_id: math_id.clone(),
                },
                direction,
                None,
            )?;
            // 编辑期间选区停在公式之后，退出时再按方向放回公式前或后。
            let position = Pos {
                offset: start + object_len,
                affinity: Affinity::Upstream,
                ..focus
            };
            self.selection = Selection {
                anchor: position.clone(),
                focus: position,
            };
            self.selection_visual_affinity = VisualAffinity::Upstream;
            self.host_caret_position = None;
            let target = MathEditTarget::Inline {
                paragraph_id: paragraph.id.clone(),
                math_id,
            };
            self.math_edit = Some(MathEdit {
                target: target.clone(),
                source_projection: None,
                source_draft: None,
                revision: self.current_math_edit_revision(),
                cursor,
                branch,
            });
            self.advance_history_merge_epoch();
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        let text_len = TextMap::new(&paragraph)?.text.len();
        let at_edge = !require_text_edge
            || match direction {
                Direction::Forward => focus.offset == text_len,
                Direction::Backward => focus.offset == 0,
            };
        if !at_edge || focus.story != StoryId::Body {
            return Ok(Effect::default());
        }
        let Some(location) = self.paragraph_index.get(&focus.paragraph_id).cloned() else {
            return Ok(Effect::default());
        };
        if location.is_title {
            return Ok(Effect::default());
        }
        let Some((parent_ids, sibling_index)) =
            paragraph_parent_path(&self.document.body, &location)
        else {
            return Ok(Effect::default());
        };
        let neighbour = match direction {
            Direction::Forward => sibling_index.checked_add(1),
            Direction::Backward => sibling_index.checked_sub(1),
        };
        let Some(neighbour) = neighbour else {
            return Ok(Effect::default());
        };
        let (block_id, align, cursor, branch) =
            {
                let block = block_list(&mut self.document.body, &parent_ids)?
                    .get_mut(neighbour)
                    .ok_or(EditError::InvalidPosition)?;
                match block {
                    Block::Equation(equation) => {
                        let (cursor, branch) = math_tree_edge_position(
                            &equation.math_tree,
                            MathPositionHost::Equation {
                                owner_id: equation.id.clone(),
                            },
                            direction,
                            None,
                        )?;
                        (equation.id.clone(), false, cursor, branch)
                    }
                    Block::Align(value) => {
                        let cell =
                            match direction {
                                Direction::Forward => {
                                    value.rows.iter().enumerate().find_map(|(row_index, row)| {
                                        row.cells.first().map(|cell| (row_index, 0, &row.id, cell))
                                    })
                                }
                                Direction::Backward => {
                                    value.rows.iter().enumerate().rev().find_map(
                                        |(row_index, row)| {
                                            let cell_index = row.cells.len().checked_sub(1)?;
                                            Some((
                                                row_index,
                                                cell_index,
                                                &row.id,
                                                row.cells.get(cell_index)?,
                                            ))
                                        },
                                    )
                                }
                            };
                        let (row_index, cell_index, row_id, cell) =
                            cell.ok_or(EditError::InvalidPosition)?;
                        let (cursor, branch) = math_tree_edge_position(
                            cell,
                            MathPositionHost::AlignCell {
                                row_id: row_id.clone(),
                                cell_id: cell.root.id.clone(),
                            },
                            direction,
                            Some((row_index, cell_index)),
                        )?;
                        (value.id.clone(), true, cursor, branch)
                    }
                    _ => return Ok(Effect::default()),
                }
            };
        let target = MathEditTarget::Block { block_id, align };
        self.math_edit = Some(MathEdit {
            target: target.clone(),
            source_projection: None,
            source_draft: None,
            revision: self.current_math_edit_revision(),
            cursor,
            branch,
        });
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    pub(super) fn focus_merged_inline_math_at(
        &mut self,
        paragraph_id: &NodeId,
        math_id: &NodeId,
        root_offset: usize,
    ) -> Result<(), EditError> {
        let story = self
            .story_for_paragraph(paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
        let lookup = Pos {
            story: story.clone(),
            paragraph_id: paragraph_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        let paragraph = self.paragraph_for(&lookup)?;
        let math =
            find_inline_math(&paragraph.inlines, math_id).ok_or(EditError::InvalidPosition)?;
        if root_offset > math.math_tree.root.content.len() {
            return Err(EditError::InvalidPosition);
        }
        let start = inline_math_slots(paragraph)?
            .into_iter()
            .find(|(_, id)| id == math_id)
            .map(|(start, _)| start)
            .ok_or(EditError::InvalidPosition)?;
        let position = Pos {
            story,
            paragraph_id: paragraph_id.clone(),
            offset: start + OBJECT_REPLACEMENT.len_utf8(),
            affinity: Affinity::Upstream,
        };
        self.selection = Selection {
            anchor: position.clone(),
            focus: position,
        };
        self.selection_visual_affinity = VisualAffinity::Upstream;
        let host = MathPositionHost::InlineMath {
            owner_id: math_id.clone(),
        };
        let path = MathSequencePath::root();
        let anchor = MathTreePosition {
            host,
            path: path.clone(),
            offset: root_offset,
            affinity: MathPositionAffinity::Downstream,
        };
        self.math_edit = Some(MathEdit {
            target: MathEditTarget::Inline {
                paragraph_id: paragraph_id.clone(),
                math_id: math_id.clone(),
            },
            source_projection: None,
            source_draft: None,
            revision: self.current_math_edit_revision(),
            cursor: root_offset,
            branch: MathEditBranch::Tree { path, anchor },
        });
        self.advance_history_merge_epoch();
        Ok(())
    }

    /// 公式子命令：在光标处插入模板，光标进入模板的第一个空槽 `{}`。
    pub(super) fn math_insert_template(&mut self, template: &str) -> Result<Effect, EditError> {
        let (template, cursor) = if is_bare_math_control_word(template) {
            let mut delimited = String::with_capacity(template.len() + 1);
            delimited.push_str(template);
            delimited.push(' ');
            (delimited, template.len() + 1)
        } else {
            let cursor = template
                .find("{}")
                .map_or(template.len(), |index| index + 1);
            (template.to_owned(), cursor)
        };
        if template_has_root_text_flow(&template)?
            && let Some(edit) = self.math_edit.clone()
            && matches!(edit.target, MathEditTarget::Inline { .. })
            && matches!(
                &edit.branch,
                MathEditBranch::Tree { path, anchor }
                    if path.steps().is_empty() && anchor.path.steps().is_empty()
            )
        {
            // InlineMath 根流不能保存 TextFlow；通过源码事务触发同一根层提升与焦点映射。
            let previous_edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
            let tree = self.math_tree_for_edit(&edit, None)?.clone();
            let (projection, _) = crate::math_validation::project_tree_for_edit(
                &tree,
                caret_math::MathInputKind::Inline,
            )
            .map_err(EditError::InvalidMath)?;
            let source = projection.edit_source();
            let source_map = projection.tree().ok_or(EditError::InvalidPosition)?;
            let MathEditBranch::Tree { anchor, .. } = &edit.branch else {
                return Err(EditError::InvalidPosition);
            };
            let source_anchor = inline_root_source_cursor(&tree, source_map, anchor.offset, source)
                .ok_or(EditError::InvalidPosition)?;
            let source_cursor = inline_root_source_cursor(&tree, source_map, edit.cursor, source)
                .ok_or(EditError::InvalidPosition)?;
            let source_edit = self.math_edit.as_mut().ok_or(EditError::InvalidPosition)?;
            source_edit.replace_source_projection(Some(Arc::new(projection)));
            source_edit.cursor = source_cursor;
            source_edit.branch = MathEditBranch::Source;
            let result = self.math_insert_raw_with_range(
                &template,
                cursor,
                source_anchor.min(source_cursor)..source_anchor.max(source_cursor),
                Some(MathEditPosition::from_edit(&previous_edit)),
            );
            if result.is_err() {
                self.math_edit = Some(previous_edit);
            }
            return result;
        }
        if self.math_edit_is_text_branch() {
            return self.math_text_enter_math_at(&template, cursor);
        }
        if self.math_edit_is_tree_branch() {
            return self.math_tree_insert_template(&template, cursor);
        }
        self.math_insert_raw(&template, cursor)
    }

    fn math_insert_text(&mut self, text: &str) -> Result<Effect, EditError> {
        let edit = self.math_edit.as_ref().ok_or(EditError::InvalidPosition)?;
        let source_edit = edit.cursor..edit.cursor;
        let mut source = edit.source().ok_or(EditError::InvalidPosition)?.to_owned();
        let mut cursor = edit.cursor;
        for character in text.chars() {
            match character {
                '\n' | '\r' => return Err(EditError::UnsupportedNewline),
                '^' | '_' => {
                    source.insert(cursor, character);
                    source.insert_str(cursor + 1, "{}");
                    cursor += 2;
                }
                '{' => {
                    source.insert_str(cursor, "{}");
                    cursor += 1;
                }
                // 右花括号由结构自动补齐；在空槽末尾输入时跳出该槽。
                '}' => {
                    if source[cursor..].starts_with('}') {
                        cursor += 1;
                    } else {
                        return Err(EditError::InvalidMath("unmatched closing brace".to_owned()));
                    }
                }
                _ => {
                    source.insert(cursor, character);
                    cursor += character.len_utf8();
                }
            }
        }
        self.write_math_source(source, cursor, Some(EditGroup::InsertText), source_edit)
    }

    pub(super) fn math_insert_raw(
        &mut self,
        text: &str,
        cursor_in_text: usize,
    ) -> Result<Effect, EditError> {
        let edit = self.math_edit.as_ref().ok_or(EditError::InvalidPosition)?;
        let source_edit = edit.cursor..edit.cursor;
        self.math_insert_raw_with_range(text, cursor_in_text, source_edit, None)
    }

    fn math_insert_raw_with_range(
        &mut self,
        text: &str,
        cursor_in_text: usize,
        source_edit: Range<usize>,
        before_math_position: Option<MathEditPosition>,
    ) -> Result<Effect, EditError> {
        let edit = self.math_edit.as_ref().ok_or(EditError::InvalidPosition)?;
        let mut source = edit.source().ok_or(EditError::InvalidPosition)?.to_owned();
        if source_edit.start > source_edit.end
            || source_edit.end > source.len()
            || !source.is_char_boundary(source_edit.start)
            || !source.is_char_boundary(source_edit.end)
        {
            return Err(EditError::InvalidPosition);
        }
        source.replace_range(source_edit.clone(), text);
        let cursor = source_edit.start + cursor_in_text;
        self.write_math_source_with_metadata(
            source,
            cursor,
            None,
            source_edit,
            MathSourceWriteOptions {
                text_edit: None,
                before_math_position_override: before_math_position,
                reject_unmapped_metadata: false,
            },
        )
    }

    fn math_delete_backward(&mut self) -> Result<Effect, EditError> {
        let edit = self
            .math_edit
            .as_ref()
            .map(MathEdit::command_snapshot)
            .ok_or(EditError::InvalidPosition)?;
        if let Some(effect) = multiline::handle_source_backspace(self, &edit)? {
            return Ok(effect);
        }
        let body = math_input_body_range(&edit).ok_or(EditError::InvalidPosition)?;
        let current_source = edit.source().ok_or(EditError::InvalidPosition)?;
        let body_source = current_source
            .get(body.clone())
            .ok_or(EditError::InvalidPosition)?;
        if body_source.trim().is_empty() && !edit.source_draft_is_pending() {
            return self.delete_math_object();
        }
        let source = current_source;
        let Some(cursor) = edit.cursor.checked_sub(body.start) else {
            return Err(EditError::InvalidPosition);
        };
        if cursor == 0 {
            return self.exit_math_edit(Direction::Backward, false);
        }
        if !edit.source_draft_is_pending()
            && let Some(construct_range) = self.empty_construct_range(&edit)
        {
            let mut next = source.to_owned();
            next.replace_range(construct_range.clone(), "");
            return self.write_math_source(
                next,
                construct_range.start,
                Some(EditGroup::DeleteBackward),
                construct_range,
            );
        }
        let local_start =
            previous_math_caret_stop(body_source, cursor).ok_or(EditError::InvalidPosition)?;
        let start = body.start + local_start;
        let end = body.start + cursor;
        let mut next = source.to_owned();
        next.replace_range(start..end, "");
        self.write_math_source(next, start, Some(EditGroup::DeleteBackward), start..end)
    }

    fn math_delete_forward(&mut self) -> Result<Effect, EditError> {
        let edit = self
            .math_edit
            .as_ref()
            .map(MathEdit::command_snapshot)
            .ok_or(EditError::InvalidPosition)?;
        let body = math_input_body_range(&edit).ok_or(EditError::InvalidPosition)?;
        let source = edit.source().ok_or(EditError::InvalidPosition)?;
        let cursor = edit
            .cursor
            .checked_sub(body.start)
            .ok_or(EditError::InvalidPosition)?;
        let body_source = source.get(body.clone()).ok_or(EditError::InvalidPosition)?;
        let Some(next_end) = next_math_caret_stop(body_source, cursor) else {
            return self.exit_math_edit(Direction::Forward, false);
        };
        let start = body.start + cursor;
        let end = body.start + next_end;
        let mut next = source.to_owned();
        next.replace_range(start..end, "");
        self.write_math_source(next, start, Some(EditGroup::DeleteForward), start..end)
    }

    fn math_move(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let effect = self.math_move_inner(direction)?;
        if effect.selection_changed {
            self.pending_inline_style.clear();
        }
        Ok(effect)
    }

    fn math_move_inner(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let edit = self
            .math_edit
            .as_ref()
            .map(MathEdit::command_snapshot)
            .ok_or(EditError::InvalidPosition)?;
        if let MathEditBranch::Tree { path, anchor } = &edit.branch {
            if anchor.path != *path {
                return Err(EditError::InvalidPosition);
            }
            if anchor.offset != edit.cursor {
                let cursor = match direction {
                    Direction::Backward => anchor.offset.min(edit.cursor),
                    Direction::Forward => anchor.offset.max(edit.cursor),
                };
                let mut position = anchor.clone();
                position.offset = cursor;
                if let Some(current) = &mut self.math_edit {
                    current.cursor = cursor;
                    current.branch = MathEditBranch::Tree {
                        path: position.path.clone(),
                        anchor: position,
                    };
                }
                self.advance_history_merge_epoch();
                return Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                });
            }
            let align_cell = self.align_cell_for_position(&edit, anchor)?;
            let tree = self.math_tree_for_edit(&edit, align_cell)?;
            let mut position = anchor.clone();
            position.offset = edit.cursor;
            let movement = advance_math_tree_position(
                tree,
                &position,
                direction == Direction::Forward,
                align_cell,
            )
            .map_err(|_| EditError::InvalidPosition)?;
            return self.apply_math_tree_caret_move(&edit, movement, direction);
        }
        if let MathEditBranch::Text { path, anchor, .. } = &edit.branch {
            let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
            let (island, range) = math_text_selection_range(tree, path, anchor, edit.cursor)
                .ok_or(EditError::InvalidPosition)?;
            if !range.is_empty() {
                let cursor = match direction {
                    Direction::Backward => range.start,
                    Direction::Forward => range.end,
                };
                let affinity = match direction {
                    Direction::Backward => MathPositionAffinity::Upstream,
                    Direction::Forward => MathPositionAffinity::Downstream,
                };
                let position = island
                    .position_at_offset(cursor, affinity)
                    .ok_or(EditError::InvalidPosition)?;
                let focus_affinity = position.affinity;
                if let Some(current) = &mut self.math_edit {
                    current.cursor = position.offset;
                    let visual_affinity = match direction {
                        Direction::Backward => VisualAffinity::Upstream,
                        Direction::Forward => VisualAffinity::Downstream,
                    };
                    current.branch = MathEditBranch::Text {
                        path: position.path.clone(),
                        anchor: position,
                        focus_affinity,
                        anchor_visual_affinity: visual_affinity,
                        visual_affinity,
                    };
                }
                self.advance_history_merge_epoch();
                return Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                });
            }
            let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
            match math_text_caret_step(tree, path, edit.cursor, direction)? {
                MathTextCaretStep::Local(cursor) => {
                    if let Some(current) = &mut self.math_edit {
                        current.cursor = cursor;
                        if let MathEditBranch::Text {
                            path: current_path,
                            anchor,
                            focus_affinity,
                            anchor_visual_affinity,
                            visual_affinity,
                            ..
                        } = &mut current.branch
                        {
                            *anchor = MathTextPosition::new(
                                current_path.clone(),
                                cursor,
                                MathPositionAffinity::Downstream,
                            );
                            *focus_affinity = MathPositionAffinity::Downstream;
                            let next_visual_affinity = match direction {
                                Direction::Backward => VisualAffinity::Upstream,
                                Direction::Forward => VisualAffinity::Downstream,
                            };
                            *anchor_visual_affinity = next_visual_affinity;
                            *visual_affinity = next_visual_affinity;
                        }
                    }
                }
                MathTextCaretStep::CrossFlow(next_path, cursor) => {
                    if let Some(current) = &mut self.math_edit {
                        current.branch = MathEditBranch::Text {
                            path: next_path.clone(),
                            anchor: MathTextPosition::new(
                                next_path,
                                cursor,
                                MathPositionAffinity::Downstream,
                            ),
                            focus_affinity: MathPositionAffinity::Downstream,
                            anchor_visual_affinity: match direction {
                                Direction::Backward => VisualAffinity::Upstream,
                                Direction::Forward => VisualAffinity::Downstream,
                            },
                            visual_affinity: match direction {
                                Direction::Backward => VisualAffinity::Upstream,
                                Direction::Forward => VisualAffinity::Downstream,
                            },
                        };
                        current.cursor = cursor;
                    }
                }
                MathTextCaretStep::Boundary => {
                    if let Some(adjacent_path) =
                        path::adjacent_text_flow_path(tree, path, direction == Direction::Forward)
                    {
                        let adjacent_text = adjacent_path
                            .resolve_structurally(tree)
                            .ok_or(EditError::InvalidPosition)?;
                        let adjacent_display = adjacent_text
                            .runs
                            .iter()
                            .map(|run| run.text.as_str())
                            .collect::<String>();
                        let cursor = if direction == Direction::Forward {
                            0
                        } else {
                            adjacent_display.len()
                        };
                        if let Some(current) = &mut self.math_edit {
                            current.branch = MathEditBranch::Text {
                                path: adjacent_path.clone(),
                                anchor: MathTextPosition::new(
                                    adjacent_path,
                                    cursor,
                                    MathPositionAffinity::Downstream,
                                ),
                                focus_affinity: MathPositionAffinity::Downstream,
                                anchor_visual_affinity: match direction {
                                    Direction::Backward => VisualAffinity::Upstream,
                                    Direction::Forward => VisualAffinity::Downstream,
                                },
                                visual_affinity: match direction {
                                    Direction::Backward => VisualAffinity::Upstream,
                                    Direction::Forward => VisualAffinity::Downstream,
                                },
                            };
                            current.cursor = cursor;
                        }
                        self.advance_history_merge_epoch();
                        return Ok(Effect {
                            selection_changed: true,
                            ..Effect::default()
                        });
                    }
                    let tree_root_edge = edit.source_projection.is_none()
                        && path.align_cell().is_none()
                        && !path
                            .steps()
                            .iter()
                            .any(|step| matches!(step, MathTextFlowPathStep::Operand(_)))
                        && path::root_text_flow_path(tree, direction == Direction::Backward, None)
                            .is_some_and(|edge| edge.steps() == path.steps());
                    if tree_root_edge {
                        return self.exit_math_edit_for_navigation(direction);
                    }
                    let (tree, position, align_cell) =
                        self.math_tree_position_at_text_boundary(&edit, path, direction)?;
                    let movement = advance_math_tree_position(
                        tree,
                        &position,
                        direction == Direction::Forward,
                        align_cell,
                    )
                    .map_err(|_| EditError::InvalidPosition)?;
                    return self.apply_math_tree_caret_move(&edit, movement, direction);
                }
            }
            self.advance_history_merge_epoch();
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        let body = math_input_body_range(&edit).ok_or(EditError::InvalidPosition)?;
        let source = edit
            .source()
            .ok_or(EditError::InvalidPosition)?
            .get(body.clone())
            .ok_or(EditError::InvalidPosition)?;
        let cursor = edit
            .cursor
            .checked_sub(body.start)
            .ok_or(EditError::InvalidPosition)?;
        let adjacent_text_path = match direction {
            Direction::Backward => {
                self.math_text_path_at_source_boundary(&edit, edit.cursor, false)
            }
            Direction::Forward => self.math_text_path_at_source_boundary(&edit, edit.cursor, true),
        };
        if let Some(path) = adjacent_text_path {
            let text = path
                .resolve(self.math_tree_for_edit(&edit, path.align_cell())?)
                .ok_or(EditError::InvalidPosition)?;
            let text_cursor = match direction {
                Direction::Backward => text.runs.iter().map(|run| run.text.len()).sum(),
                Direction::Forward => 0,
            };
            self.enter_math_text_branch(path, text_cursor);
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        let stop = match direction {
            Direction::Forward => next_math_caret_stop(source, cursor),
            Direction::Backward => previous_math_caret_stop(source, cursor),
        };
        match stop {
            Some(stop) => {
                let source_cursor = body.start + stop;
                if direction == Direction::Backward
                    && let Some(path) =
                        self.math_text_path_at_source_boundary(&edit, source_cursor, false)
                {
                    let text = path
                        .resolve(self.math_tree_for_edit(&edit, path.align_cell())?)
                        .ok_or(EditError::InvalidPosition)?;
                    let cursor = text.runs.iter().map(|run| run.text.len()).sum();
                    self.enter_math_text_branch(path, cursor);
                    return Ok(Effect {
                        selection_changed: true,
                        ..Effect::default()
                    });
                }
                if direction == Direction::Forward
                    && let Some(path) =
                        self.math_text_path_at_source_boundary(&edit, source_cursor, true)
                {
                    self.enter_math_text_branch(path, 0);
                    return Ok(Effect {
                        selection_changed: true,
                        ..Effect::default()
                    });
                }
                self.set_math_cursor(source_cursor);
                Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                })
            }
            None => self.exit_math_edit_for_navigation(direction),
        }
    }

    pub(super) fn math_edit_is_text_branch(&self) -> bool {
        self.math_edit
            .as_ref()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Text { .. }))
    }

    pub(super) fn math_edit_is_tree_branch(&self) -> bool {
        self.math_edit
            .as_ref()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Tree { .. }))
    }

    pub(super) fn cancel_math_source_draft(&mut self) {
        if let Some(edit) = &mut self.math_edit {
            if let Some(draft) = &edit.source_draft {
                edit.cursor = draft.base_cursor();
            }
            edit.source_draft = None;
        }
    }

    fn math_tree_insert_text(&mut self, text: &str) -> Result<Effect, EditError> {
        if text == "&"
            && self.math_edit.as_ref().is_some_and(|edit| {
                matches!(edit.target, MathEditTarget::Block { align: true, .. })
            })
        {
            return self.math_tree_align_break(false);
        }
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree {
            mut path,
            mut anchor,
        } = edit.branch.clone()
        else {
            return Err(EditError::InvalidPosition);
        };
        if anchor.path != path {
            return Err(EditError::InvalidPosition);
        }
        if text.contains('\n') || text.contains('\r') {
            return Err(EditError::UnsupportedNewline);
        }
        let align_cell = self.align_cell_for_position(&edit, &anchor)?;
        let mut tree = self.math_tree_for_edit(&edit, align_cell)?.clone();
        let original_tree = tree.clone();
        let mut start = anchor.offset.min(edit.cursor);
        let mut end = anchor.offset.max(edit.cursor);
        let boundaries = GraphemeMap::new_with_fences(text, &[]).into_boundaries();
        let graphemes = boundaries
            .windows(2)
            .map(|pair| text.get(pair[0]..pair[1]).ok_or(EditError::InvalidPosition))
            .collect::<Result<Vec<_>, _>>()?;
        let has_insertable_grapheme = graphemes.iter().any(|grapheme| {
            grapheme
                .chars()
                .next()
                .is_some_and(|character| !character.is_whitespace())
        });
        let starts_with_plain_text = graphemes
            .iter()
            .find(|grapheme| {
                grapheme
                    .chars()
                    .next()
                    .is_some_and(|character| !character.is_whitespace())
            })
            .is_some_and(|grapheme| is_literal_math_input(grapheme));
        if start == 0
            && start == end
            && starts_with_plain_text
            && let Some((parent_path, script_index)) =
                script_base_parent_for_start_insertion(&tree, &anchor)
        {
            // 非空脚标基底的起点与脚标节点外边界重合；普通文本应插入在整个脚标之前。
            path = parent_path;
            anchor.path = path.clone();
            anchor.offset = script_index;
            start = script_index;
            end = script_index;
        }
        let contents = tree_position::resolve_sequence_mut(&mut tree, &path)
            .ok_or(EditError::InvalidPosition)?;
        if end > contents.len() {
            return Err(EditError::InvalidPosition);
        }
        if start == end
            && has_insertable_grapheme
            && let Some(placeholder_index) = tree_position::placeholder_run_start(contents, start)
        {
            // 防止旧导航状态把输入插到必填空槽占位符之后。
            start = placeholder_index;
            end = placeholder_index;
        }
        let replaces_placeholder = start == end
            && has_insertable_grapheme
            && matches!(contents.get(start), Some(MathContent::Math(node)) if node.kind == MathNodeKind::Placeholder);
        if replaces_placeholder {
            let placeholder_end = (start..contents.len())
                .take_while(|index| {
                    matches!(
                        contents.get(*index),
                        Some(MathContent::Math(node))
                            if node.kind == MathNodeKind::Placeholder
                    )
                })
                .last()
                .map_or(start, |index| index + 1);
            if contents[start..placeholder_end]
                .iter()
                .any(math_content_has_unknown_metadata)
            {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            contents.drain(start..placeholder_end);
        }
        let selected = contents.drain(start..end).collect::<Vec<_>>();
        let mut selected_base = Some(selected);
        let initial_position = anchor.clone();
        let mut position = anchor;
        position.offset = start;
        let mut literal_batch = String::new();
        for grapheme in graphemes {
            let Some(character) = grapheme.chars().next() else {
                continue;
            };
            if character.is_whitespace() {
                continue;
            }
            match grapheme {
                "^" | "_" => {
                    flush_math_literal_batch(&mut tree, &mut position, &mut literal_batch)?;
                    insert_math_script(&mut tree, &mut position, grapheme, selected_base.take())?;
                }
                "{" => {
                    flush_math_literal_batch(&mut tree, &mut position, &mut literal_batch)?;
                    open_math_group(&mut tree, &mut position)?;
                }
                "}" => {
                    flush_math_literal_batch(&mut tree, &mut position, &mut literal_batch)?;
                    close_math_group(&tree, &mut position)?;
                }
                _ if is_literal_math_input(grapheme) => {
                    selected_base = None;
                    literal_batch.push_str(grapheme);
                }
                _ => {
                    flush_math_literal_batch(&mut tree, &mut position, &mut literal_batch)?;
                    selected_base = None;
                    insert_math_symbol(&mut tree, &mut position, grapheme)?;
                }
            }
        }
        flush_math_literal_batch(&mut tree, &mut position, &mut literal_batch)?;
        if start == end && position == initial_position && tree == original_tree {
            return Ok(Effect::default());
        }
        tree.validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let cursor = position.offset;
        self.commit_math_text_tree(
            edit,
            tree,
            text_branch::MathTextTreeCommit {
                branch: MathEditBranch::Tree {
                    path: position.path.clone(),
                    anchor: position,
                },
                align_cell,
                cursor,
                merge_group: Some(EditGroup::InsertText),
                pending_style: text_branch::PendingStylePolicy::Clear,
                selection_changed: true,
            },
        )
    }

    pub(super) fn math_tree_align_break(&mut self, new_row: bool) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Block {
            block_id,
            align: true,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        let MathEditBranch::Tree { path, anchor } = edit.branch.clone() else {
            return Err(EditError::InvalidPosition);
        };
        if !path.steps().is_empty() || anchor.path != path {
            return Err(EditError::InvalidPosition);
        }
        let (row_index, cell_index) = self
            .align_cell_for_position(&edit, &anchor)?
            .ok_or(EditError::InvalidPosition)?;
        let Block::Align(before_align) =
            find_block_by_id(&self.document.body, block_id).ok_or(EditError::InvalidPosition)?
        else {
            return Err(EditError::InvalidPosition);
        };
        let before_align = before_align.clone();
        let mut after_align = before_align.clone();
        let row = after_align
            .rows
            .get_mut(row_index)
            .ok_or(EditError::InvalidPosition)?;
        let cell = row
            .cells
            .get_mut(cell_index)
            .ok_or(EditError::InvalidPosition)?;
        let start = anchor.offset.min(edit.cursor);
        let end = anchor.offset.max(edit.cursor);
        let content = &cell.root.content;
        if end > content.len() {
            return Err(EditError::InvalidPosition);
        }
        let left = content[..start].to_vec();
        let right = content[end..].to_vec();
        cell.root.content = left;
        let mut next_tree = empty_math_tree();
        next_tree.root.content = right;
        let next_row_id = if new_row {
            let cell_count = row.cells.len();
            let mut cells = (0..cell_count)
                .map(|_| empty_math_tree())
                .collect::<Vec<_>>();
            let next = cells
                .get_mut(cell_index)
                .ok_or(EditError::InvalidPosition)?;
            next.root.content = std::mem::take(&mut next_tree.root.content);
            let id = NodeId::generate();
            after_align.rows.insert(
                row_index + 1,
                MathAlignRow {
                    id: id.clone(),
                    cells,
                    extra: BTreeMap::new(),
                },
            );
            Some(id)
        } else {
            row.cells.insert(cell_index + 1, next_tree);
            None
        };
        let next_row_index = if new_row { row_index + 1 } else { row_index };
        let next_cell_index = if new_row { cell_index } else { cell_index + 1 };
        let next_align_row = after_align
            .rows
            .get(next_row_index)
            .ok_or(EditError::InvalidPosition)?;
        let next_cell = next_align_row
            .cells
            .get(next_cell_index)
            .ok_or(EditError::InvalidPosition)?;
        let position = MathTreePosition {
            host: MathPositionHost::AlignCell {
                row_id: next_row_id.unwrap_or_else(|| next_align_row.id.clone()),
                cell_id: next_cell.root.id.clone(),
            },
            path: MathSequencePath::root(),
            offset: 0,
            affinity: MathPositionAffinity::Downstream,
        };
        multiline::commit_align_tree_edit(
            self,
            edit,
            before_align,
            after_align,
            position.offset,
            MathEditBranch::Tree {
                path: position.path.clone(),
                anchor: position,
            },
        )
    }

    fn math_tree_insert_template(
        &mut self,
        template: &str,
        cursor_in_template: usize,
    ) -> Result<Effect, EditError> {
        if cursor_in_template > template.len() || !template.is_char_boundary(cursor_in_template) {
            return Err(EditError::InvalidPosition);
        }
        let parsed = caret_math::parse_projection(template, caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let (parsed, _) = parsed.into_parts();
        let caret_math::ProjectedMath::Tree(mut template_tree) = parsed else {
            return Err(EditError::InvalidMath(
                "expected inline math template".to_owned(),
            ));
        };
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree { path, anchor } = edit.branch.clone() else {
            return Err(EditError::InvalidPosition);
        };
        if anchor.path != path {
            return Err(EditError::InvalidPosition);
        }
        let align_cell = self.align_cell_for_position(&edit, &anchor)?;
        let mut tree = self.math_tree_for_edit(&edit, align_cell)?.clone();
        if matches!(template, "^{}" | "_{}") {
            let contents = tree_position::resolve_sequence_mut(&mut tree, &path)
                .ok_or(EditError::InvalidPosition)?;
            let start = anchor.offset.min(edit.cursor);
            let end = anchor.offset.max(edit.cursor);
            if end > contents.len() {
                return Err(EditError::InvalidPosition);
            }
            let selected = contents.drain(start..end).collect();
            let mut position = anchor;
            position.offset = start;
            let script = if template == "^{}" { "^" } else { "_" };
            insert_math_script(&mut tree, &mut position, script, Some(selected))?;
            tree.validate()
                .map_err(|error| EditError::InvalidMath(error.to_string()))?;
            let cursor = position.offset;
            return self.commit_math_text_tree(
                edit,
                tree,
                text_branch::MathTextTreeCommit {
                    branch: MathEditBranch::Tree {
                        path: position.path.clone(),
                        anchor: position,
                    },
                    align_cell,
                    cursor,
                    merge_group: None,
                    pending_style: text_branch::PendingStylePolicy::Clear,
                    selection_changed: true,
                },
            );
        }
        let contents = tree_position::resolve_sequence_mut(&mut tree, &path)
            .ok_or(EditError::InvalidPosition)?;
        let start = anchor.offset.min(edit.cursor);
        let end = anchor.offset.max(edit.cursor);
        if end > contents.len() {
            return Err(EditError::InvalidPosition);
        }

        let placeholder = if template
            .get(cursor_in_template..)
            .is_some_and(|remaining| remaining.starts_with('}'))
        {
            first_placeholder_position(&template_tree.root.content, &MathSequencePath::root())
        } else {
            None
        };
        if let Some((placeholder_path, placeholder_index)) = &placeholder {
            let sequence =
                tree_position::resolve_sequence_mut(&mut template_tree, placeholder_path)
                    .ok_or(EditError::InvalidPosition)?;
            if !matches!(sequence.get(*placeholder_index), Some(MathContent::Math(node)) if node.kind == MathNodeKind::Placeholder)
            {
                return Err(EditError::InvalidPosition);
            }
            sequence.remove(*placeholder_index);
        }

        let text_content_index = template_tree
            .root
            .content
            .iter()
            .position(|content| matches!(content, MathContent::Text(_)));
        let inserted_len = template_tree.root.content.len();
        if start == end && inserted_len == 0 {
            return Ok(Effect::default());
        }
        contents.splice(start..end, template_tree.root.content);
        tree.validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;

        if let Some(content_index) = text_content_index {
            let content_index = start
                .checked_add(content_index)
                .ok_or(EditError::InvalidPosition)?;
            let text_path =
                tree_position::text_path_for_content(&anchor, content_index, align_cell);
            let text_anchor =
                MathTextPosition::new(text_path.clone(), 0, MathPositionAffinity::Downstream);
            return self.commit_math_text_tree(
                edit,
                tree,
                text_branch::MathTextTreeCommit {
                    branch: MathEditBranch::Text {
                        path: text_path,
                        anchor: text_anchor,
                        focus_affinity: MathPositionAffinity::Downstream,
                        anchor_visual_affinity: VisualAffinity::Downstream,
                        visual_affinity: VisualAffinity::Downstream,
                    },
                    align_cell,
                    cursor: 0,
                    merge_group: None,
                    pending_style: text_branch::PendingStylePolicy::Clear,
                    selection_changed: true,
                },
            );
        }

        let position = if let Some((relative_path, offset)) = placeholder {
            let mut steps = path.steps().to_vec();
            let mut relative = relative_path.steps().to_vec();
            if let Some(first) = relative.first_mut() {
                match first {
                    MathSequenceStep::Operand { content_index, .. }
                    | MathSequenceStep::EnvironmentChildren { content_index } => {
                        *content_index = content_index
                            .checked_add(start)
                            .ok_or(EditError::InvalidPosition)?;
                    }
                }
            }
            steps.extend(relative);
            MathTreePosition {
                host: anchor.host,
                path: MathSequencePath::new(steps),
                offset,
                affinity: anchor.affinity,
            }
        } else {
            let mut position = anchor;
            position.offset = start
                .checked_add(inserted_len)
                .ok_or(EditError::InvalidPosition)?;
            position
        };
        let cursor = position.offset;
        self.commit_math_text_tree(
            edit,
            tree,
            text_branch::MathTextTreeCommit {
                branch: MathEditBranch::Tree {
                    path: position.path.clone(),
                    anchor: position,
                },
                align_cell,
                cursor,
                merge_group: None,
                pending_style: text_branch::PendingStylePolicy::Clear,
                selection_changed: true,
            },
        )
    }

    fn math_tree_delete(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree { path, anchor } = edit.branch.clone() else {
            return Err(EditError::InvalidPosition);
        };
        if anchor.path != path {
            return Err(EditError::InvalidPosition);
        }
        let align_cell = self.align_cell_for_position(&edit, &anchor)?;
        let mut tree = self.math_tree_for_edit(&edit, align_cell)?.clone();
        let contents = tree_position::resolve_sequence_mut(&mut tree, &path)
            .ok_or(EditError::InvalidPosition)?;
        let start = anchor.offset.min(edit.cursor);
        let end = anchor.offset.max(edit.cursor);
        if end > contents.len() {
            return Err(EditError::InvalidPosition);
        }
        if matches!(direction, Direction::Backward)
            && start == end
            && path.steps().is_empty()
            && let Some(effect) =
                multiline::handle_tree_backspace(self, &edit, align_cell, start, end)?
        {
            return Ok(effect);
        }
        let deletion = if start < end {
            start..end
        } else {
            match direction {
                Direction::Backward => start.checked_sub(1).map(|index| index..start),
                Direction::Forward => (start < contents.len()).then_some(start..start + 1),
            }
            .unwrap_or_default()
        };
        if deletion.is_empty() {
            if matches!(direction, Direction::Backward) {
                if path.steps().is_empty() && math_sequence_is_empty(contents) {
                    if let MathEditTarget::Block {
                        block_id,
                        align: true,
                    } = &edit.target
                    {
                        let Block::Align(align) = find_block_by_id(&self.document.body, block_id)
                            .ok_or(EditError::InvalidPosition)?
                        else {
                            return Err(EditError::InvalidPosition);
                        };
                        let occupied = align.rows.iter().any(|row| {
                            row.cells
                                .iter()
                                .any(|cell| !math_sequence_is_empty(&cell.root.content))
                        });
                        if occupied {
                            let cells = align
                                .rows
                                .iter()
                                .enumerate()
                                .flat_map(|(row, value)| {
                                    value
                                        .cells
                                        .iter()
                                        .enumerate()
                                        .map(move |(cell, _)| (row, cell))
                                })
                                .collect::<Vec<_>>();
                            let current = align_cell.ok_or(EditError::InvalidPosition)?;
                            let current_index = cells
                                .iter()
                                .position(|cell| *cell == current)
                                .ok_or(EditError::InvalidPosition)?;
                            let neighbor = current_index
                                .checked_sub(1)
                                .and_then(|index| cells.get(index).copied())
                                .or_else(|| cells.get(current_index + 1).copied())
                                .ok_or(EditError::InvalidPosition)?;
                            let (cursor, branch) = math_tree_edge_position(
                                self.math_tree_for_edit(&edit, Some(neighbor))?,
                                MathPositionHost::AlignCell {
                                    row_id: align.rows[neighbor.0].id.clone(),
                                    cell_id: align.rows[neighbor.0].cells[neighbor.1]
                                        .root
                                        .id
                                        .clone(),
                                },
                                if current_index > 0 {
                                    Direction::Backward
                                } else {
                                    Direction::Forward
                                },
                                Some(neighbor),
                            )?;
                            if let Some(active) = &mut self.math_edit {
                                active.cursor = cursor;
                                active.branch = branch;
                            }
                            self.advance_history_merge_epoch();
                            return Ok(Effect {
                                selection_changed: true,
                                ..Effect::default()
                            });
                        }
                    }
                    return self.delete_math_object();
                }

                if let Some((parent_path, content_index, empty_structure)) =
                    empty_math_operand_parent(&tree, &path)?
                {
                    if empty_structure {
                        let parent = tree_position::resolve_sequence_mut(&mut tree, &parent_path)
                            .ok_or(EditError::InvalidPosition)?;
                        let content = parent
                            .get(content_index)
                            .ok_or(EditError::InvalidPosition)?;
                        if math_content_has_unknown_metadata(content) {
                            return Err(EditError::UnsupportedOpaqueContent);
                        }
                        parent.remove(content_index);
                        let mut next_position = anchor;
                        next_position.path = parent_path;
                        next_position.offset = content_index;
                        tree.validate()
                            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
                        return self.commit_math_text_tree(
                            edit,
                            tree,
                            text_branch::MathTextTreeCommit {
                                branch: MathEditBranch::Tree {
                                    path: next_position.path.clone(),
                                    anchor: next_position,
                                },
                                align_cell,
                                cursor: content_index,
                                merge_group: Some(EditGroup::DeleteBackward),
                                pending_style: text_branch::PendingStylePolicy::Clear,
                                selection_changed: true,
                            },
                        );
                    }
                    let position = MathTreePosition {
                        host: anchor.host,
                        path: parent_path,
                        offset: content_index,
                        affinity: anchor.affinity,
                    };
                    if let Some(active) = &mut self.math_edit {
                        active.cursor = content_index;
                        active.branch = MathEditBranch::Tree {
                            path: position.path.clone(),
                            anchor: position,
                        };
                    }
                    self.advance_history_merge_epoch();
                    return Ok(Effect {
                        selection_changed: true,
                        ..Effect::default()
                    });
                }
            }
            return Ok(Effect::default());
        }
        if start == end
            && let Some(MathContent::Text(text)) = contents.get(deletion.start)
        {
            let text_path =
                tree_position::text_path_for_content(&anchor, deletion.start, align_cell);
            let cursor = match direction {
                Direction::Backward => text.runs.iter().map(|run| run.text.len()).sum(),
                Direction::Forward => 0,
            };
            let original_edit = edit.clone();
            let mut text_edit = edit;
            text_edit.cursor = cursor;
            let text_anchor =
                MathTextPosition::new(text_path.clone(), cursor, MathPositionAffinity::Downstream);
            text_edit.branch = MathEditBranch::Text {
                path: text_path,
                anchor: text_anchor,
                focus_affinity: MathPositionAffinity::Downstream,
                anchor_visual_affinity: VisualAffinity::Downstream,
                visual_affinity: VisualAffinity::Downstream,
            };
            self.math_edit = Some(text_edit.clone());
            return match self.math_text_delete(direction) {
                Ok(effect) => Ok(effect),
                Err(error) => {
                    self.math_edit = Some(original_edit);
                    Err(error)
                }
            };
        }
        if contents[deletion.clone()]
            .iter()
            .any(math_content_has_unknown_metadata)
        {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        contents.drain(deletion.clone());
        let resegmented_affinity = match direction {
            Direction::Backward => MathPositionAffinity::Downstream,
            Direction::Forward => MathPositionAffinity::Upstream,
        };
        let cursor =
            resegment_literal_symbol_run(contents, deletion.start, None, resegmented_affinity)?
                .unwrap_or(deletion.start);
        tree.validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let mut next_position = anchor;
        next_position.offset = cursor;
        self.commit_math_text_tree(
            edit,
            tree,
            text_branch::MathTextTreeCommit {
                branch: MathEditBranch::Tree {
                    path,
                    anchor: next_position,
                },
                align_cell,
                cursor,
                merge_group: Some(match direction {
                    Direction::Backward => EditGroup::DeleteBackward,
                    Direction::Forward => EditGroup::DeleteForward,
                }),
                pending_style: text_branch::PendingStylePolicy::Clear,
                selection_changed: true,
            },
        )
    }

    /// 将宿主测得的字节位置限制在当前 TextFlow，并拒绝半个 UTF-8 字符或 grapheme。
    fn set_math_text_selection(
        &mut self,
        anchor: usize,
        focus: usize,
    ) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text { path, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let text = path
            .resolve_structurally(tree)
            .ok_or(EditError::InvalidPosition)?;
        let display = text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        if [anchor, focus]
            .into_iter()
            .any(|offset| offset > display.len() || !display.is_char_boundary(offset))
            || !math_text_offsets_are_boundaries(tree, path, &[anchor, focus])
        {
            return Err(EditError::InvalidPosition);
        }
        let (anchor_affinity, focus_affinity) = if anchor < focus {
            (
                MathPositionAffinity::Upstream,
                MathPositionAffinity::Downstream,
            )
        } else if anchor > focus {
            (
                MathPositionAffinity::Downstream,
                MathPositionAffinity::Upstream,
            )
        } else {
            (
                MathPositionAffinity::Downstream,
                MathPositionAffinity::Downstream,
            )
        };
        self.set_math_text_selection_positions(
            MathTextPosition::new(path.clone(), anchor, anchor_affinity),
            MathTextPosition::new(path.clone(), focus, focus_affinity),
        )
    }

    /// 设置同一数学文本岛中可跨 TextFlow 边界的 UTF-8 选区。
    pub fn set_math_text_selection_positions(
        &mut self,
        anchor_position: MathTextPosition,
        focus_position: MathTextPosition,
    ) -> Result<Effect, EditError> {
        self.set_math_text_selection_positions_with_visual_affinity(
            anchor_position,
            focus_position,
            VisualAffinity::Downstream,
        )
    }

    /// 设置数学文本选区并保留 Parley 的视觉 caret 侧。
    pub fn set_math_text_selection_positions_with_visual_affinity(
        &mut self,
        anchor_position: MathTextPosition,
        focus_position: MathTextPosition,
        visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        self.set_math_text_selection_positions_with_visual_affinities(
            anchor_position,
            focus_position,
            VisualAffinity::Downstream,
            visual_affinity,
        )
    }

    /// 设置数学文本选区并保留锚点与焦点各自的 Parley 视觉侧。
    pub fn set_math_text_selection_positions_with_visual_affinities(
        &mut self,
        anchor_position: MathTextPosition,
        focus_position: MathTextPosition,
        anchor_visual_affinity: VisualAffinity,
        visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        self.ensure_math_edit_revision_current()?;
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text {
            path,
            anchor,
            focus_affinity: old_focus_affinity,
            anchor_visual_affinity: old_anchor_visual_affinity,
            visual_affinity: old_visual_affinity,
            ..
        } = &edit.branch
        else {
            return Err(EditError::InvalidPosition);
        };
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let island = path::contiguous_text_flow_text(tree, &focus_position.path)
            .ok_or(EditError::InvalidPosition)?;
        let anchor_offset = island
            .global_offset(&anchor_position)
            .ok_or(EditError::InvalidPosition)?;
        let focus_offset = island
            .global_offset(&focus_position)
            .ok_or(EditError::InvalidPosition)?;
        if !is_grapheme_boundary(&island.display, anchor_offset)
            || !is_grapheme_boundary(&island.display, focus_offset)
        {
            return Err(EditError::InvalidPosition);
        }
        let anchor_visual_affinity = if anchor_offset == focus_offset {
            visual_affinity
        } else {
            anchor_visual_affinity
        };
        let old_anchor_offset = island
            .global_offset(anchor)
            .ok_or(EditError::InvalidPosition)?;
        let old_focus_offset = island
            .global_offset(&MathTextPosition::new(
                path.clone(),
                edit.cursor,
                *old_focus_affinity,
            ))
            .ok_or(EditError::InvalidPosition)?;
        if old_anchor_offset == anchor_offset
            && old_focus_offset == focus_offset
            && anchor == &anchor_position
            && path == &focus_position.path
            && *old_focus_affinity == focus_position.affinity
            && *old_anchor_visual_affinity == anchor_visual_affinity
            && *old_visual_affinity == visual_affinity
        {
            return Ok(Effect::default());
        }
        self.pending_inline_style.clear();
        if let Some(current) = &mut self.math_edit {
            current.cursor = focus_position.offset;
            current.branch = MathEditBranch::Text {
                path: focus_position.path.clone(),
                anchor: anchor_position,
                focus_affinity: focus_position.affinity,
                anchor_visual_affinity,
                visual_affinity,
            };
        }
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    fn math_tree_for_edit<'a>(
        &'a self,
        edit: &MathEdit,
        align_cell: Option<(usize, usize)>,
    ) -> Result<&'a caret_doc::MathTree, EditError> {
        math_tree_for_target(&self.document, &edit.target, align_cell)
            .ok_or(EditError::InvalidPosition)
    }

    fn math_tree_position_at_text_boundary<'a>(
        &'a self,
        edit: &MathEdit,
        path: &MathTextFlowPath,
        direction: Direction,
    ) -> Result<MathTreeTextBoundary<'a>, EditError> {
        let align_cell = path.align_cell();
        let tree = self.math_tree_for_edit(edit, align_cell)?;
        let host = self.math_position_host_for_target(&edit.target, align_cell)?;
        let boundary = match direction {
            Direction::Backward => tree_position::MathTextFlowBoundary::Start,
            Direction::Forward => tree_position::MathTextFlowBoundary::End,
        };
        let affinity = match direction {
            Direction::Backward => MathPositionAffinity::Upstream,
            Direction::Forward => MathPositionAffinity::Downstream,
        };
        let position = MathTreePosition::from_text_flow_boundary(
            tree,
            host,
            path.steps(),
            align_cell,
            boundary,
            affinity,
        )
        .map_err(|_| EditError::InvalidPosition)?;
        Ok((tree, position, align_cell))
    }

    fn align_cell_for_position(
        &self,
        edit: &MathEdit,
        position: &MathTreePosition,
    ) -> Result<Option<(usize, usize)>, EditError> {
        self.align_cell_for_target_and_host(&edit.target, &position.host)
    }

    pub(super) fn align_cell_for_target_and_host(
        &self,
        target: &MathEditTarget,
        host: &MathPositionHost,
    ) -> Result<Option<(usize, usize)>, EditError> {
        match (target, host) {
            (MathEditTarget::Inline { math_id, .. }, MathPositionHost::InlineMath { owner_id })
                if math_id == owner_id =>
            {
                Ok(None)
            }
            (
                MathEditTarget::Block {
                    block_id,
                    align: false,
                },
                MathPositionHost::Equation { owner_id },
            ) if block_id == owner_id => Ok(None),
            (
                MathEditTarget::Block {
                    block_id,
                    align: true,
                },
                MathPositionHost::AlignCell { row_id, cell_id },
            ) => {
                let Block::Align(align) = find_block_by_id(&self.document.body, block_id)
                    .ok_or(EditError::InvalidPosition)?
                else {
                    return Err(EditError::InvalidPosition);
                };
                align
                    .rows
                    .iter()
                    .enumerate()
                    .find_map(|(row_index, row)| {
                        (row.id == *row_id).then(|| {
                            row.cells.iter().enumerate().find_map(|(cell_index, cell)| {
                                (cell.root.id == *cell_id).then_some((row_index, cell_index))
                            })
                        })?
                    })
                    .map(Some)
                    .ok_or(EditError::InvalidPosition)
            }
            _ => Err(EditError::InvalidPosition),
        }
    }

    pub(super) fn math_position_host_for_target(
        &self,
        target: &MathEditTarget,
        align_cell: Option<(usize, usize)>,
    ) -> Result<MathPositionHost, EditError> {
        match (target, align_cell) {
            (MathEditTarget::Inline { math_id, .. }, None) => Ok(MathPositionHost::InlineMath {
                owner_id: math_id.clone(),
            }),
            (
                MathEditTarget::Block {
                    block_id,
                    align: false,
                },
                None,
            ) => Ok(MathPositionHost::Equation {
                owner_id: block_id.clone(),
            }),
            (
                MathEditTarget::Block {
                    block_id,
                    align: true,
                },
                Some((row_index, cell_index)),
            ) => {
                let Block::Align(align) = find_block_by_id(&self.document.body, block_id)
                    .ok_or(EditError::InvalidPosition)?
                else {
                    return Err(EditError::InvalidPosition);
                };
                let row = align
                    .rows
                    .get(row_index)
                    .ok_or(EditError::InvalidPosition)?;
                let cell = row
                    .cells
                    .get(cell_index)
                    .ok_or(EditError::InvalidPosition)?;
                Ok(MathPositionHost::AlignCell {
                    row_id: row.id.clone(),
                    cell_id: cell.root.id.clone(),
                })
            }
            _ => Err(EditError::InvalidPosition),
        }
    }

    fn apply_math_tree_caret_move(
        &mut self,
        edit: &MathEdit,
        movement: MathTreeCaretMove,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        match movement {
            MathTreeCaretMove::Position(position) => {
                let align_cell = self.align_cell_for_position(edit, &position)?;
                position
                    .resolve(self.math_tree_for_edit(edit, align_cell)?)
                    .map_err(|_| EditError::InvalidPosition)?;
                if let Some(current) = &mut self.math_edit {
                    current.replace_source_projection(None);
                    current.source_draft = None;
                    current.cursor = position.offset;
                    current.branch = MathEditBranch::Tree {
                        path: position.path.clone(),
                        anchor: position,
                    };
                }
            }
            MathTreeCaretMove::TextFlow(path, cursor) => {
                let tree = self.math_tree_for_edit(edit, path.align_cell())?;
                let text = path
                    .resolve_structurally(tree)
                    .ok_or(EditError::InvalidPosition)?;
                let length = text
                    .runs
                    .iter()
                    .try_fold(0usize, |length, run| length.checked_add(run.text.len()));
                if length.is_none_or(|length| cursor > length)
                    || !math_text_offsets_are_boundaries(tree, &path, &[cursor])
                {
                    return Err(EditError::InvalidPosition);
                }
                if let Some(current) = &mut self.math_edit {
                    current.replace_source_projection(None);
                    current.source_draft = None;
                    current.cursor = cursor;
                    let anchor = MathTextPosition::new(
                        path.clone(),
                        cursor,
                        MathPositionAffinity::Downstream,
                    );
                    current.branch = MathEditBranch::Text {
                        path,
                        anchor,
                        focus_affinity: MathPositionAffinity::Downstream,
                        anchor_visual_affinity: VisualAffinity::Downstream,
                        visual_affinity: VisualAffinity::Downstream,
                    };
                }
            }
            MathTreeCaretMove::Boundary => {
                if self.move_to_adjacent_align_cell(edit, direction)? {
                    return Ok(Effect {
                        selection_changed: true,
                        ..Effect::default()
                    });
                }
                return self.exit_math_edit_for_navigation(direction);
            }
        }
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    fn move_to_adjacent_align_cell(
        &mut self,
        edit: &MathEdit,
        direction: Direction,
    ) -> Result<bool, EditError> {
        let MathEditTarget::Block {
            block_id,
            align: true,
        } = &edit.target
        else {
            return Ok(false);
        };
        let current = match &edit.branch {
            MathEditBranch::Tree { anchor, .. } => self
                .align_cell_for_position(edit, anchor)?
                .ok_or(EditError::InvalidPosition)?,
            MathEditBranch::Text { path, .. } => {
                let Some(cell) = path.align_cell() else {
                    return Ok(false);
                };
                cell
            }
            MathEditBranch::Source => return Ok(false),
        };
        let Block::Align(align) =
            find_block_by_id(&self.document.body, block_id).ok_or(EditError::InvalidPosition)?
        else {
            return Err(EditError::InvalidPosition);
        };
        let cells = align
            .rows
            .iter()
            .enumerate()
            .flat_map(|(row_index, row)| {
                (0..row.cells.len()).map(move |cell_index| (row_index, cell_index))
            })
            .collect::<Vec<_>>();
        let current_index = cells
            .iter()
            .position(|cell| *cell == current)
            .ok_or(EditError::InvalidPosition)?;
        let next_index = match direction {
            Direction::Forward => current_index.checked_add(1).filter(|i| *i < cells.len()),
            Direction::Backward => current_index.checked_sub(1),
        };
        let Some((row_index, cell_index)) = next_index.and_then(|index| cells.get(index).copied())
        else {
            return Ok(false);
        };
        let row = align
            .rows
            .get(row_index)
            .ok_or(EditError::InvalidPosition)?;
        let cell = row
            .cells
            .get(cell_index)
            .ok_or(EditError::InvalidPosition)?;
        let host = MathPositionHost::AlignCell {
            row_id: row.id.clone(),
            cell_id: cell.root.id.clone(),
        };
        let (cursor, branch) =
            math_tree_edge_position(cell, host, direction, Some((row_index, cell_index)))?;
        let next = self.math_edit.as_mut().ok_or(EditError::InvalidPosition)?;
        next.replace_source_projection(None);
        next.source_draft = None;
        next.cursor = cursor;
        next.branch = branch;
        self.advance_history_merge_epoch();
        Ok(true)
    }

    /// 在兼容的源码分支按当前 MathTree 懒生成投影；树路径本身不携带源码范围。
    /// 投影只保存在 MathEdit 状态中，不写入 Document。
    pub(super) fn refresh_math_text_projection(&mut self) -> Result<(), EditError> {
        let mut edit = self
            .math_edit
            .as_ref()
            .map(MathEdit::command_snapshot)
            .ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text { path, anchor, .. } = edit.branch.clone() else {
            return Ok(());
        };
        let (projection, projected_target) = match (&edit.target, path.align_cell()) {
            (MathEditTarget::Inline { .. }, None) => {
                let tree = self.math_tree_for_edit(&edit, None)?.clone();
                let (projection, tree) = crate::math_validation::project_tree_for_edit(
                    &tree,
                    caret_math::MathInputKind::Inline,
                )
                .map_err(EditError::InvalidMath)?;
                let source_map = projection
                    .tree()
                    .cloned()
                    .ok_or(EditError::InvalidPosition)?;
                (
                    projection,
                    HistoryProjectionTarget::Tree { tree, source_map },
                )
            }
            (MathEditTarget::Block { align: false, .. }, None) => {
                let tree = self.math_tree_for_edit(&edit, None)?.clone();
                let (projection, tree) = crate::math_validation::project_tree_for_edit(
                    &tree,
                    caret_math::MathInputKind::Equation,
                )
                .map_err(EditError::InvalidMath)?;
                let source_map = projection
                    .tree()
                    .cloned()
                    .ok_or(EditError::InvalidPosition)?;
                (
                    projection,
                    HistoryProjectionTarget::Tree { tree, source_map },
                )
            }
            (
                MathEditTarget::Block {
                    align: true,
                    block_id,
                },
                Some(_),
            ) => {
                let Block::Align(align) = find_block_by_id(&self.document.body, block_id)
                    .ok_or(EditError::InvalidPosition)?
                else {
                    return Err(EditError::InvalidPosition);
                };
                let (projection, align) = crate::math_validation::project_align_for_edit(align)
                    .map_err(EditError::InvalidMath)?;
                let source_map = projection
                    .align()
                    .cloned()
                    .ok_or(EditError::InvalidPosition)?;
                (
                    projection,
                    HistoryProjectionTarget::Align { align, source_map },
                )
            }
            _ => return Err(EditError::InvalidPosition),
        };
        let tree = match (&projected_target, path.align_cell()) {
            (HistoryProjectionTarget::Tree { tree, .. }, None) => tree,
            (HistoryProjectionTarget::Align { align, .. }, Some((row, cell))) => align
                .rows
                .get(row)
                .and_then(|row| row.cells.get(cell))
                .ok_or(EditError::InvalidPosition)?,
            _ => return Err(EditError::InvalidPosition),
        };
        let (text, _) = path
            .resolve_in_projection(tree, &projection)
            .ok_or(EditError::InvalidPosition)?;
        let display = text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        let (anchor_text, _) = anchor
            .path
            .resolve_in_projection(tree, &projection)
            .ok_or(EditError::InvalidPosition)?;
        let anchor_display = anchor_text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        if edit.cursor > display.len()
            || !display.is_char_boundary(edit.cursor)
            || anchor.offset > anchor_display.len()
            || !anchor_display.is_char_boundary(anchor.offset)
            || math_text_selection_range(tree, &path, &anchor, edit.cursor).is_none()
        {
            return Err(EditError::InvalidPosition);
        }
        let _ = projected_target;
        edit.replace_source_projection(Some(Arc::new(projection)));
        edit.branch = MathEditBranch::Text {
            path,
            anchor,
            focus_affinity: MathPositionAffinity::Downstream,
            anchor_visual_affinity: VisualAffinity::Downstream,
            visual_affinity: VisualAffinity::Downstream,
        };
        self.math_edit = Some(edit);
        Ok(())
    }

    /// 返回当前 TextFlow 选择所在文本岛及 anchor/focus 的岛级 UTF-8 偏移。
    pub(super) fn math_text_selection_island_offsets(
        &self,
    ) -> Option<(TextIslandSnapshot, usize, usize)> {
        let edit = self.math_edit.as_ref()?;
        let MathEditBranch::Text {
            path,
            anchor,
            focus_affinity,
            ..
        } = &edit.branch
        else {
            return None;
        };
        let tree = self.math_tree_for_edit(edit, path.align_cell()).ok()?;
        let island = path::contiguous_text_flow_text(tree, path)?;
        let anchor_offset = island.global_offset(anchor)?;
        let focus_offset = island.global_offset(&MathTextPosition::new(
            path.clone(),
            edit.cursor,
            *focus_affinity,
        ))?;
        Some((island, anchor_offset, focus_offset))
    }

    fn math_text_path_at_source_boundary(
        &self,
        edit: &MathEdit,
        source_offset: usize,
        forward: bool,
    ) -> Option<MathTextFlowPath> {
        if edit.source_draft_is_pending() {
            return None;
        }
        if let MathEditTarget::Block {
            block_id,
            align: true,
        } = &edit.target
        {
            let Block::Align(align) = find_block_by_id(&self.document.body, block_id)? else {
                return None;
            };
            let edit_source = edit.source_projection.as_deref()?;
            let body_start = edit_source.edit_range().start;
            let cell_ranges = edit_source.align()?.cell_navigation_ranges();
            let (row, cell) = cell_ranges.iter().enumerate().find_map(|(row, cells)| {
                cells.iter().enumerate().find_map(|(cell, range)| {
                    let local_start = range.start.checked_sub(body_start)?;
                    let local_end = range.end.checked_sub(body_start)?;
                    (local_start <= source_offset && source_offset <= local_end)
                        .then_some((row, cell))
                })
            })?;
            let tree = align.rows.get(row)?.cells.get(cell)?;
            let semantic_range = edit_source.align()?.cell_range(row, cell)?;
            let source_offset = body_start
                .checked_add(source_offset)?
                .clamp(semantic_range.start, semantic_range.end);
            let source_map = edit_source.align()?.cell_tree(row, cell)?;
            let path = path::path_at_source_boundary(
                source_map,
                source_offset,
                forward,
                Some((row, cell)),
            )?;
            path.resolve_in_projection(tree, edit_source)?;
            return Some(path);
        }
        let tree = self.math_tree_for_edit(edit, None).ok()?;
        let edit_source = edit.source_projection.as_deref()?;
        let body_start = edit_source.edit_range().start;
        let source_map = edit_source.tree()?;
        let path = path::path_at_source_boundary(
            source_map,
            body_start.checked_add(source_offset)?,
            forward,
            None,
        )?;
        path.resolve_in_projection(tree, edit_source)?;
        Some(path)
    }

    fn enter_math_text_branch(&mut self, path: MathTextFlowPath, cursor: usize) {
        self.pending_inline_style.clear();
        if let Some(edit) = &mut self.math_edit {
            edit.replace_source_projection(None);
            edit.source_draft = None;
            edit.cursor = cursor;
            let anchor =
                MathTextPosition::new(path.clone(), cursor, MathPositionAffinity::Downstream);
            edit.branch = MathEditBranch::Text {
                path,
                anchor,
                focus_affinity: MathPositionAffinity::Downstream,
                anchor_visual_affinity: VisualAffinity::Downstream,
                visual_affinity: VisualAffinity::Downstream,
            };
        }
        self.advance_history_merge_epoch();
    }

    /// 移动公式内光标；光标移动切断连续输入的撤销合并。
    fn set_math_cursor(&mut self, cursor: usize) {
        if let Some(edit) = &mut self.math_edit
            && edit.cursor != cursor
        {
            edit.cursor = cursor;
            self.advance_history_merge_epoch();
        }
    }

    /// 退出公式编辑；块公式把边界 ID 和方向交给共享外侧停靠状态。
    pub(super) fn exit_math_edit(
        &mut self,
        direction: Direction,
        allow_fallback: bool,
    ) -> Result<Effect, EditError> {
        self.exit_math_edit_with_boundary(direction, allow_fallback, false)
    }

    /// 显式离开块公式时停在外侧边界，供重新进入或继续移动到相邻宿主。
    pub(super) fn exit_math_edit_for_navigation(
        &mut self,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        self.exit_math_edit_with_boundary(direction, false, true)
    }

    fn exit_math_edit_with_boundary(
        &mut self,
        direction: Direction,
        allow_fallback: bool,
        keep_block_boundary_stop: bool,
    ) -> Result<Effect, EditError> {
        if self
            .math_edit
            .as_ref()
            .is_some_and(MathEdit::source_draft_is_pending)
        {
            return Err(EditError::InvalidMath(
                "resolve or cancel the incomplete math source draft before leaving".to_owned(),
            ));
        }
        let Some(edit) = self.math_edit.clone() else {
            return Ok(Effect::default());
        };
        match &edit.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let story = self
                    .story_for_paragraph(paragraph_id)
                    .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
                let probe = Pos {
                    story,
                    paragraph_id: paragraph_id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                };
                let start = inline_math_slots(self.paragraph_for(&probe)?)?
                    .into_iter()
                    .find(|(_, id)| id == math_id)
                    .map(|(start, _)| start)
                    .ok_or(EditError::InvalidPosition)?;
                let position = match direction {
                    Direction::Backward => Pos {
                        offset: start,
                        affinity: Affinity::Downstream,
                        ..probe
                    },
                    Direction::Forward => Pos {
                        offset: start + OBJECT_REPLACEMENT.len_utf8(),
                        affinity: Affinity::Upstream,
                        ..probe
                    },
                };
                self.set_math_exit_selection(position);
            }
            MathEditTarget::Block { block_id, .. } => {
                if allow_fallback {
                    let other_direction = match direction {
                        Direction::Backward => Direction::Forward,
                        Direction::Forward => Direction::Backward,
                    };
                    let position = match self.paragraph_beside_math_chain(block_id, direction)? {
                        Some(position) => Some(position),
                        None => self.paragraph_beside_math_chain(block_id, other_direction)?,
                    };
                    let Some(position) = position else {
                        return Ok(Effect::default());
                    };
                    self.set_math_exit_selection(position);
                } else {
                    if let Some(position) = self.paragraph_beside_block(block_id, direction)? {
                        self.set_math_exit_selection(position);
                    }
                    if keep_block_boundary_stop {
                        self.host_caret_position =
                            Some(HostCaretPosition::outside(block_id.clone(), direction));
                    }
                }
            }
        }
        self.math_edit = None;
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    fn set_math_exit_selection(&mut self, position: Pos) {
        self.selection = Selection {
            anchor: position.clone(),
            focus: position.clone(),
        };
        self.selection_visual_affinity = match position.affinity {
            Affinity::Upstream => VisualAffinity::Upstream,
            Affinity::Downstream => VisualAffinity::Downstream,
        };
    }

    /// 查找同级正文段落；定位、Enter 回退和对象删除恢复使用此策略，Escape 不跨块跳转。
    fn paragraph_beside_math_chain(
        &self,
        block_id: &NodeId,
        direction: Direction,
    ) -> Result<Option<Pos>, EditError> {
        let (parent_ids, mut index) =
            find_block_path(&self.document.body, block_id).ok_or(EditError::InvalidPosition)?;
        let siblings = block_list_ref(&self.document.body, &parent_ids)?;
        loop {
            let Some(next_index) = (match direction {
                Direction::Forward => index.checked_add(1),
                Direction::Backward => index.checked_sub(1),
            }) else {
                return Ok(None);
            };
            index = next_index;
            let Some(block) = siblings.get(index) else {
                return Ok(None);
            };
            match block {
                Block::Paragraph(paragraph) => {
                    let offset = match direction {
                        Direction::Forward => 0,
                        Direction::Backward => TextMap::new(paragraph)?.text.len(),
                    };
                    return Ok(Some(Pos {
                        story: StoryId::Body,
                        paragraph_id: paragraph.id.clone(),
                        offset,
                        affinity: match direction {
                            Direction::Forward => Affinity::Upstream,
                            Direction::Backward => Affinity::Downstream,
                        },
                    }));
                }
                Block::Equation(_) | Block::Align(_) => {}
                _ => return Ok(None),
            }
        }
    }

    /// 连续显示公式之间按块顺序移动公式焦点，不经过旧正文选区。
    fn math_edit_beside_block(
        &mut self,
        block_id: &NodeId,
        direction: Direction,
    ) -> Result<Option<MathEdit>, EditError> {
        let Some((parent_ids, index)) = find_block_path(&self.document.body, block_id) else {
            return Err(EditError::InvalidPosition);
        };
        let neighbour = match direction {
            Direction::Forward => index.checked_add(1),
            Direction::Backward => index.checked_sub(1),
        };
        let Some(neighbour) = neighbour else {
            return Ok(None);
        };
        let Some(block) = block_list_ref(&self.document.body, &parent_ids)?.get(neighbour) else {
            return Ok(None);
        };
        self.math_edit_for_block(block, direction)
    }

    pub(super) fn math_edit_at_block(
        &self,
        block_id: &NodeId,
        entry_direction: Direction,
    ) -> Result<Option<MathEdit>, EditError> {
        let Some((parent_ids, index)) = find_block_path(&self.document.body, block_id) else {
            return Err(EditError::InvalidPosition);
        };
        let Some(block) = block_list_ref(&self.document.body, &parent_ids)?.get(index) else {
            return Err(EditError::InvalidPosition);
        };
        self.math_edit_for_block(block, entry_direction)
    }

    /// 统一仲裁数学块外侧停靠点的反向重入与同向越过。
    pub(super) fn move_from_math_block_boundary_stop(
        &mut self,
        direction: Direction,
    ) -> Result<Option<Effect>, EditError> {
        let Some(position) = self.host_caret_position.clone() else {
            return Ok(None);
        };
        if position.kind() != HostCaretKind::Block {
            return Ok(None);
        }
        let Some((block_id, exit_direction)) = position.as_outside() else {
            return Ok(None);
        };
        let entry_direction = match exit_direction {
            Direction::Backward => Direction::Forward,
            Direction::Forward => Direction::Backward,
        };
        let transition =
            position.horizontal_transition(direction == Direction::Forward, false, false);
        let Some(target_edit) = self.math_edit_at_block(block_id, entry_direction)? else {
            return Ok(None);
        };
        if matches!(
            transition,
            HostCaretTransition::EnterStart | HostCaretTransition::EnterEnd
        ) {
            self.host_caret_position = None;
            self.math_edit = Some(target_edit);
            self.advance_history_merge_epoch();
            return Ok(Some(Effect {
                selection_changed: true,
                ..Effect::default()
            }));
        }

        let next_edit = self.math_edit_beside_block(block_id, direction)?;
        if let Some(next_edit) = next_edit {
            self.math_edit = Some(next_edit);
            self.host_caret_position = None;
            self.advance_history_merge_epoch();
            return Ok(Some(Effect {
                selection_changed: true,
                ..Effect::default()
            }));
        }
        if let Some(position) = self.paragraph_beside_block(block_id, direction)? {
            let empty_inline_host = self
                .text_map_for(&position)?
                .empty_inline_environment_stack_at(position.offset, position.affinity)
                .last()
                .and_then(|span| span.id.clone());
            let visual_affinity = match position.affinity {
                Affinity::Upstream => VisualAffinity::Upstream,
                Affinity::Downstream => VisualAffinity::Downstream,
            };
            self.selection = Selection {
                anchor: position.clone(),
                focus: position.clone(),
            };
            self.selection_visual_affinity = visual_affinity;
            self.host_caret_position = empty_inline_host.map(|host_id| match position.affinity {
                Affinity::Upstream => HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Inline,
                },
                Affinity::Downstream => HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Inline,
                },
            });
            self.advance_history_merge_epoch();
            return Ok(Some(Effect {
                selection_changed: true,
                ..Effect::default()
            }));
        }
        Ok(Some(Effect::default()))
    }

    fn math_edit_for_block(
        &self,
        block: &Block,
        direction: Direction,
    ) -> Result<Option<MathEdit>, EditError> {
        let (block_id, align, cursor, branch) = match block {
            Block::Equation(equation) => {
                let (cursor, branch) = math_tree_edge_position(
                    &equation.math_tree,
                    MathPositionHost::Equation {
                        owner_id: equation.id.clone(),
                    },
                    direction,
                    None,
                )?;
                (equation.id.clone(), false, cursor, branch)
            }
            Block::Align(value) => {
                let cell = match direction {
                    Direction::Forward => {
                        value.rows.iter().enumerate().find_map(|(row_index, row)| {
                            row.cells.first().map(|cell| (row_index, 0, &row.id, cell))
                        })
                    }
                    Direction::Backward => {
                        value
                            .rows
                            .iter()
                            .enumerate()
                            .rev()
                            .find_map(|(row_index, row)| {
                                let cell_index = row.cells.len().checked_sub(1)?;
                                Some((row_index, cell_index, &row.id, row.cells.get(cell_index)?))
                            })
                    }
                }
                .ok_or(EditError::InvalidPosition)?;
                let (row_index, cell_index, row_id, cell) = cell;
                let (cursor, branch) = math_tree_edge_position(
                    cell,
                    MathPositionHost::AlignCell {
                        row_id: row_id.clone(),
                        cell_id: cell.root.id.clone(),
                    },
                    direction,
                    Some((row_index, cell_index)),
                )?;
                (value.id.clone(), true, cursor, branch)
            }
            _ => return Ok(None),
        };
        let target = MathEditTarget::Block { block_id, align };
        Ok(Some(MathEdit {
            target: target.clone(),
            source_projection: None,
            source_draft: None,
            revision: self.current_math_edit_revision(),
            cursor,
            branch,
        }))
    }

    /// 块公式前（向后）或后（向前）的同级正文段落边缘。
    pub(super) fn paragraph_beside_block(
        &self,
        block_id: &NodeId,
        direction: Direction,
    ) -> Result<Option<Pos>, EditError> {
        let (parent_ids, index) =
            find_block_path(&self.document.body, block_id).ok_or(EditError::InvalidPosition)?;
        let siblings = block_list_ref(&self.document.body, &parent_ids)?;
        let neighbour = match direction {
            Direction::Forward => index.checked_add(1),
            Direction::Backward => index.checked_sub(1),
        };
        let Some(Block::Paragraph(paragraph)) = neighbour.and_then(|index| siblings.get(index))
        else {
            return Ok(None);
        };
        let offset = match direction {
            Direction::Forward => 0,
            Direction::Backward => TextMap::new(paragraph)?.text.len(),
        };
        Ok(Some(Pos {
            story: StoryId::Body,
            paragraph_id: paragraph.id.clone(),
            offset,
            affinity: match direction {
                Direction::Forward => Affinity::Upstream,
                Direction::Backward => Affinity::Downstream,
            },
        }))
    }

    /// 空公式中退格：删除整个公式对象并退出编辑。
    fn delete_math_object(&mut self) -> Result<Effect, EditError> {
        let Some(edit) = self.math_edit.clone() else {
            return Ok(Effect::default());
        };
        let before_math_position = MathEditPosition::from_edit(&edit);
        match edit.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let story = self
                    .story_for_paragraph(&paragraph_id)
                    .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
                let position = Pos {
                    story,
                    paragraph_id: paragraph_id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                };
                let mut paragraph = self.paragraph_for(&position)?.clone();
                let start = inline_math_slots(&paragraph)?
                    .into_iter()
                    .find(|(_, id)| *id == math_id)
                    .map(|(start, _)| start)
                    .ok_or(EditError::InvalidPosition)?;
                let before_selection = self.selection.clone();
                let before_visual_affinity = self.selection_visual_affinity;
                let before_states = history::paragraph_states_for_ids(
                    &self.document,
                    &self.paragraph_index,
                    [paragraph_id.clone()],
                );
                TextMap::new(&paragraph)?.delete_range(
                    &mut paragraph,
                    start,
                    start + OBJECT_REPLACEMENT.len_utf8(),
                )?;
                let result_map = TextMap::new(&paragraph)?;
                let caret_affinity = if result_map.grapheme_boundaries.binary_search(&start).is_ok()
                {
                    Affinity::Downstream
                } else {
                    Affinity::Upstream
                };
                let caret_offset = result_map.snap_to_grapheme_boundary(start, caret_affinity);
                let result_boundaries = result_map.grapheme_boundaries.clone();
                self.replace_paragraph(&position, paragraph)?;
                self.text_map_cache.clear();
                let caret = Pos {
                    offset: caret_offset,
                    affinity: caret_affinity,
                    ..position
                };
                self.selection = Selection {
                    anchor: caret.clone(),
                    focus: caret.clone(),
                };
                self.selection_visual_affinity = match caret.affinity {
                    Affinity::Upstream => VisualAffinity::Upstream,
                    Affinity::Downstream => VisualAffinity::Downstream,
                };
                let after_visual_affinity = self.selection_visual_affinity;
                self.math_edit = None;
                let after_states = history::paragraph_states_for_ids(
                    &self.document,
                    &self.paragraph_index,
                    [paragraph_id.clone()],
                );
                self.record_math_history(
                    DocumentPatch::Paragraphs {
                        before: before_states,
                        after: after_states,
                        merge_group: None,
                        merge_epoch: self.history_merge_epoch,
                    },
                    (before_selection, before_visual_affinity),
                    (self.selection.clone(), after_visual_affinity),
                    Some(before_math_position),
                    None,
                );
                Ok(Effect {
                    document_changed: true,
                    selection_changed: true,
                    changed_paragraph_id: Some(paragraph_id.clone()),
                    position_map: Some(PositionMap::TextEdit {
                        paragraph_id,
                        start,
                        end: start + OBJECT_REPLACEMENT.len_utf8(),
                        inserted_len: 0,
                        snap_graphemes: true,
                        result_boundaries,
                    }),
                    ..Effect::default()
                })
            }
            MathEditTarget::Block { block_id, .. } => {
                let target =
                    match self.paragraph_beside_math_chain(&block_id, Direction::Backward)? {
                        Some(position) => position,
                        None => self
                            .paragraph_beside_math_chain(&block_id, Direction::Forward)?
                            .ok_or(EditError::InvalidPosition)?,
                    };
                let (parent_ids, index) = find_block_path(&self.document.body, &block_id)
                    .ok_or(EditError::InvalidPosition)?;
                let before_selection = self.selection.clone();
                let before_visual_affinity = self.selection_visual_affinity;
                let before_state_id = self.history_state_id;
                let blocks = block_list(&mut self.document.body, &parent_ids)?;
                let removed = blocks.remove(index);
                self.math_edit = None;
                self.selection = Selection {
                    anchor: target.clone(),
                    focus: target,
                };
                self.selection_visual_affinity = match self.selection.focus.affinity {
                    Affinity::Upstream => VisualAffinity::Upstream,
                    Affinity::Downstream => VisualAffinity::Downstream,
                };
                let paragraph_index = build_paragraph_index(&self.document.body);
                self.paragraph_index = paragraph_index;
                self.structure_index = Arc::new(StructureIndex::build(&self.document));
                self.text_map_cache.clear();
                self.advance_history_merge_epoch();
                let after_state_id = self.allocate_history_state_id();
                let before_state = SessionHistoryState::new(
                    before_selection,
                    before_visual_affinity,
                    self.pending_inline_style.clone(),
                    before_state_id,
                )
                .with_math_edit(Some(before_math_position));
                let after_state = SessionHistoryState::new(
                    self.selection.clone(),
                    self.selection_visual_affinity,
                    self.pending_inline_style.clone(),
                    after_state_id,
                );
                self.record_history_edit(SessionHistoryEdit::new(
                    DocumentPatch::BlockRange {
                        parent_environment_ids: parent_ids,
                        sibling_index: index,
                        before: vec![removed],
                        after: Vec::new(),
                    },
                    before_state,
                    after_state,
                ));
                self.history_state_id = after_state_id;
                self.advance_document_generation();
                Ok(Effect {
                    document_changed: true,
                    selection_changed: true,
                    ..Effect::default()
                })
            }
        }
    }

    /// 解析候选源码并提交结构树；可继续输入的失败保留为独立草稿。
    fn write_math_source(
        &mut self,
        source: String,
        cursor: usize,
        merge_group: Option<EditGroup>,
        old_edit: std::ops::Range<usize>,
    ) -> Result<Effect, EditError> {
        self.write_math_source_with_metadata(
            source,
            cursor,
            merge_group,
            old_edit,
            MathSourceWriteOptions {
                text_edit: None,
                before_math_position_override: None,
                reject_unmapped_metadata: false,
            },
        )
    }

    /// 提交必须保留既有结构元数据的源码变更；无法映射元数据时拒绝而不进入草稿。
    fn write_math_source_strict(
        &mut self,
        source: String,
        cursor: usize,
        merge_group: Option<EditGroup>,
        old_edit: std::ops::Range<usize>,
    ) -> Result<Effect, EditError> {
        self.write_math_source_with_metadata(
            source,
            cursor,
            merge_group,
            old_edit,
            MathSourceWriteOptions {
                text_edit: None,
                before_math_position_override: None,
                reject_unmapped_metadata: true,
            },
        )
    }

    fn write_math_source_with_metadata(
        &mut self,
        source: String,
        cursor: usize,
        merge_group: Option<EditGroup>,
        old_edit: std::ops::Range<usize>,
        options: MathSourceWriteOptions,
    ) -> Result<Effect, EditError> {
        let MathSourceWriteOptions {
            text_edit,
            before_math_position_override,
            reject_unmapped_metadata,
        } = options;
        let edit = self
            .math_edit
            .as_ref()
            .map(MathEdit::command_snapshot)
            .ok_or(EditError::InvalidPosition)?;
        let current_source = edit.source().ok_or(EditError::InvalidPosition)?;
        let current_projection = edit.required_source_projection()?;
        if source == current_source && text_edit.is_none() {
            self.set_math_cursor(cursor);
            return Ok(Effect::default());
        }
        if old_edit.start > old_edit.end
            || old_edit.end > current_source.len()
            || !current_source.is_char_boundary(old_edit.start)
            || !current_source.is_char_boundary(old_edit.end)
        {
            return Err(EditError::InvalidPosition);
        }
        let mut source_projection_candidate =
            if text_edit.is_some() || matches!(edit.branch, MathEditBranch::Source) {
                let (kind, candidate_source) = match &edit.target {
                    MathEditTarget::Inline { .. } => {
                        (caret_math::MathInputKind::Inline, source.clone())
                    }
                    MathEditTarget::Block { align: false, .. } => {
                        (caret_math::MathInputKind::Equation, source.clone())
                    }
                    MathEditTarget::Block { align: true, .. } => (
                        caret_math::MathInputKind::Align,
                        format!("{ALIGN_BEGIN}{source}{ALIGN_END}"),
                    ),
                };
                match caret_math::parse_projection(&candidate_source, kind) {
                    Ok(projection) => Some(projection.into_parts()),
                    Err(caret_math::MathParseError::InputTooLong { .. }) => {
                        return Err(EditError::InvalidMath(
                            "math source exceeds the editing limit".to_owned(),
                        ));
                    }
                    Err(error)
                        if text_edit.is_none() && error.is_incomplete()
                            || (matches!(&error, caret_math::MathParseError::Parse { .. })
                                && caret_math::classify_input(&candidate_source, kind)
                                    == caret_math::MathInputStatus::Incomplete) =>
                    {
                        if reject_unmapped_metadata {
                            return Err(EditError::InvalidMath(error.to_string()));
                        }
                        self.math_edit
                            .as_mut()
                            .ok_or(EditError::InvalidPosition)?
                            .record_source_draft(source, cursor)?;
                        self.advance_history_merge_epoch();
                        return Ok(Effect {
                            selection_changed: true,
                            ..Effect::default()
                        });
                    }
                    Err(error) => return Err(EditError::InvalidMath(error.to_string())),
                }
            } else {
                None
            };
        let (transaction_old_edit, new_edit) = if edit.source_draft_is_pending() {
            changed_source_range(current_projection.edit_source(), &source)
                .ok_or(EditError::InvalidPosition)?
        } else {
            let inserted_len = source
                .len()
                .checked_sub(current_source.len() - old_edit.len())
                .ok_or(EditError::InvalidPosition)?;
            (
                old_edit.clone(),
                old_edit.start..old_edit.start + inserted_len,
            )
        };
        let before_math_position = if let Some(position) = before_math_position_override {
            position
        } else if let Some(draft) = &edit.source_draft {
            let mut baseline = edit.clone();
            baseline.source_draft = None;
            baseline.cursor = draft.base_cursor();
            MathEditPosition::from_edit(&baseline)
        } else {
            MathEditPosition::from_edit(&edit)
        };
        let mut next_branch = MathEditBranch::Source;
        let mut next_cursor = cursor;
        let mut next_edit = MathEdit {
            target: edit.target.clone(),
            source_projection: edit.source_projection.clone(),
            source_draft: None,
            revision: edit.revision.clone(),
            cursor,
            branch: MathEditBranch::Source,
        };
        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let mut position_map = None;
        let mut selection_changed = false;
        match &edit.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let story = self
                    .story_for_paragraph(paragraph_id)
                    .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
                let position = Pos {
                    story,
                    paragraph_id: paragraph_id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                };
                let mut paragraph = self.paragraph_for(&position)?.clone();
                let math = find_inline_math_mut(&mut paragraph.inlines, math_id)
                    .ok_or(EditError::InvalidPosition)?;
                let (parsed, mut source_projection) = source_projection_candidate
                    .take()
                    .ok_or(EditError::InvalidPosition)?;
                let caret_math::ProjectedMath::Tree(parsed_tree) = parsed else {
                    return Err(EditError::InvalidMath("expected inline math".to_owned()));
                };
                let mut math_tree = if let Some(text_edit) = &text_edit {
                    let replacement = source
                        .get(old_edit.start..new_edit.end)
                        .ok_or(EditError::InvalidPosition)?;
                    let mut math_tree = math.math_tree.clone();
                    apply_math_text_split_tree_edit(
                        &mut math_tree,
                        &text_edit.path,
                        text_edit.range.clone(),
                        replacement,
                        &text_edit.split_math_contents,
                    )?;
                    math_tree
                } else {
                    parsed_tree.clone()
                };
                let previous_projection = edit.required_source_projection()?;
                let previous_map = previous_projection
                    .tree()
                    .ok_or(EditError::InvalidPosition)?;
                let parsed_map = source_projection
                    .tree()
                    .cloned()
                    .ok_or(EditError::InvalidPosition)?;
                let metadata_preserved = crate::math_validation::bind_tree_sidecar(
                    &mut source_projection,
                    &parsed_tree,
                    &math_tree,
                    Some(previous_map),
                ) && preserve_math_tree_metadata_for_edit(
                    &math.math_tree,
                    &mut math_tree,
                    &absolute_source_edit(previous_projection, &transaction_old_edit)
                        .ok_or(EditError::InvalidPosition)?,
                    &absolute_source_edit(&source_projection, &new_edit)
                        .ok_or(EditError::InvalidPosition)?,
                    previous_map,
                    source_projection.tree().ok_or(EditError::InvalidPosition)?,
                );
                if !metadata_preserved {
                    if text_edit.is_none() && matches!(edit.branch, MathEditBranch::Source) {
                        self.math_edit
                            .as_mut()
                            .ok_or(EditError::InvalidPosition)?
                            .record_source_draft(source, cursor)?;
                        self.advance_history_merge_epoch();
                        return Ok(Effect {
                            selection_changed: true,
                            ..Effect::default()
                        });
                    }
                    return Err(EditError::InvalidMath(
                        "editing would discard unmapped math metadata".to_owned(),
                    ));
                }
                math_tree
                    .validate()
                    .map_err(|error| EditError::InvalidMath(error.to_string()))?;
                let mapped = parsed_map
                    .reconcile_for_tree(
                        &parsed_tree,
                        &math_tree,
                        Some(previous_map),
                        source_projection.source(),
                        None,
                    )
                    .ok_or_else(|| {
                        EditError::InvalidMath(
                            "edited formula cannot preserve source sidecar".to_owned(),
                        )
                    })?;
                if !source_projection.bind_tree_sidecar_after_validated_edit(&math_tree, mapped) {
                    return Err(EditError::InvalidMath(
                        "edited formula cannot be rebound to its source sidecar".to_owned(),
                    ));
                }
                let (source_anchor, source_cursor) = (next_cursor, next_cursor);
                math.math_tree = math_tree;
                let source_map = source_projection.tree().ok_or(EditError::InvalidPosition)?;
                let split = split_inline_math_root_with_map(
                    math,
                    source_map,
                    source_anchor,
                    source_cursor,
                    source_projection.source(),
                )?;
                next_edit.replace_source_projection(Some(Arc::new(source_projection)));
                if let Some(split) = split {
                    let inline_start = inline_math_slots(&paragraph)?
                        .into_iter()
                        .find(|(_, id)| id == math_id)
                        .map(|(start, _)| start)
                        .ok_or(EditError::InvalidPosition)?;
                    let split_focus = split.focus.clone();
                    let split_len = split.inserted_len;
                    let replacement = split.contents;
                    if !replace_inline_math_with_split(&mut paragraph.inlines, math_id, replacement)
                    {
                        return Err(EditError::InvalidPosition);
                    }
                    let map = TextMap::new(&paragraph)?;
                    let (after_selection, affinity, after_math) = match split_focus {
                        InlineRootFocus::Text {
                            anchor_offset,
                            cursor_offset,
                        } => {
                            let anchor_offset = inline_start
                                .checked_add(anchor_offset)
                                .ok_or(EditError::InvalidPosition)?;
                            let cursor_offset = inline_start
                                .checked_add(cursor_offset)
                                .ok_or(EditError::InvalidPosition)?;
                            let anchor_affinity = if anchor_offset == cursor_offset {
                                Affinity::Downstream
                            } else if anchor_offset < cursor_offset {
                                Affinity::Upstream
                            } else {
                                Affinity::Downstream
                            };
                            let cursor_affinity = if anchor_offset <= cursor_offset {
                                Affinity::Downstream
                            } else {
                                Affinity::Upstream
                            };
                            let anchor = Pos {
                                offset: anchor_offset,
                                affinity: anchor_affinity,
                                ..position.clone()
                            };
                            let cursor = Pos {
                                offset: cursor_offset,
                                affinity: cursor_affinity,
                                ..position.clone()
                            };
                            map.validate_pos(&anchor)?;
                            map.validate_pos(&cursor)?;
                            (
                                Selection {
                                    anchor,
                                    focus: cursor,
                                },
                                cursor_affinity,
                                None,
                            )
                        }
                        InlineRootFocus::Math {
                            math_id,
                            source,
                            cursor,
                            paragraph_offset,
                        } => {
                            let offset = inline_start
                                .checked_add(paragraph_offset)
                                .and_then(|start| start.checked_add(OBJECT_REPLACEMENT.len_utf8()))
                                .ok_or(EditError::InvalidPosition)?;
                            next_cursor = cursor;
                            next_branch = MathEditBranch::Source;
                            let target = MathEditTarget::Inline {
                                paragraph_id: paragraph_id.clone(),
                                math_id: math_id.clone(),
                            };
                            let projection = caret_math::parse_projection(
                                &source,
                                caret_math::MathInputKind::Inline,
                            )
                            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
                            let (content, mut source_projection) = projection.into_parts();
                            let caret_math::ProjectedMath::Tree(parsed_tree) = content else {
                                return Err(EditError::InvalidPosition);
                            };
                            let split_tree = find_inline_math(&paragraph.inlines, &math_id)
                                .map(|inline| &inline.math_tree)
                                .ok_or(EditError::InvalidPosition)?;
                            if !crate::math_validation::bind_tree_sidecar(
                                &mut source_projection,
                                &parsed_tree,
                                split_tree,
                                None,
                            ) {
                                return Err(EditError::InvalidMath(
                                    "split formula cannot be bound to its source map".to_owned(),
                                ));
                            }
                            next_edit = MathEdit {
                                target: target.clone(),
                                source_projection: Some(Arc::new(source_projection)),
                                source_draft: None,
                                revision: edit.revision.clone(),
                                cursor,
                                branch: MathEditBranch::Source,
                            };
                            let position = Pos {
                                offset,
                                affinity: Affinity::Upstream,
                                ..position.clone()
                            };
                            map.validate_pos(&position)?;
                            (
                                Selection {
                                    anchor: position.clone(),
                                    focus: position,
                                },
                                Affinity::Upstream,
                                Some(target),
                            )
                        }
                    };
                    let after_visual_affinity = match affinity {
                        Affinity::Upstream => VisualAffinity::Upstream,
                        Affinity::Downstream => VisualAffinity::Downstream,
                    };
                    selection_changed = after_selection != before_selection
                        || after_visual_affinity != before_visual_affinity;
                    position_map = Some(PositionMap::TextEdit {
                        paragraph_id: paragraph_id.clone(),
                        start: inline_start,
                        end: inline_start + OBJECT_REPLACEMENT.len_utf8(),
                        inserted_len: split_len,
                        snap_graphemes: true,
                        result_boundaries: map.grapheme_boundaries.clone(),
                    });
                    let before_states = history::paragraph_states_for_ids(
                        &self.document,
                        &self.paragraph_index,
                        [paragraph_id.clone()],
                    );
                    self.replace_paragraph(&position, paragraph)?;
                    self.text_map_cache.clear();
                    let after_math_position = after_math.map(|target| {
                        debug_assert_eq!(target, next_edit.target);
                        MathEditPosition::with_edit_state(
                            &next_edit,
                            next_cursor,
                            next_branch.clone(),
                        )
                    });
                    self.selection = after_selection.clone();
                    self.selection_visual_affinity = after_visual_affinity;
                    let after_states = history::paragraph_states_for_ids(
                        &self.document,
                        &self.paragraph_index,
                        [paragraph_id.clone()],
                    );
                    self.advance_history_merge_epoch();
                    let patch = DocumentPatch::Paragraphs {
                        before: before_states,
                        after: after_states,
                        merge_group: None,
                        merge_epoch: self.history_merge_epoch,
                    };
                    self.record_math_history_with_pending_style(
                        patch,
                        (before_selection, before_visual_affinity),
                        (after_selection, self.selection_visual_affinity),
                        Some(before_math_position.clone()),
                        after_math_position.clone(),
                        self.pending_inline_style.clone(),
                    );
                    self.math_edit = after_math_position.map(|_| next_edit.clone());
                    self.sync_math_edit_revision();
                    return Ok(Effect {
                        document_changed: true,
                        selection_changed,
                        changed_paragraph_id: Some(paragraph_id.clone()),
                        position_map,
                        ..Effect::default()
                    });
                }
                let before_states = history::paragraph_states_for_ids(
                    &self.document,
                    &self.paragraph_index,
                    [paragraph_id.clone()],
                );
                self.replace_paragraph(&position, paragraph)?;
                self.text_map_cache.clear();
                let after_states = history::paragraph_states_for_ids(
                    &self.document,
                    &self.paragraph_index,
                    [paragraph_id.clone()],
                );
                let patch = DocumentPatch::Paragraphs {
                    before: before_states,
                    after: after_states,
                    merge_group,
                    merge_epoch: self.history_merge_epoch,
                };
                self.record_math_history(
                    patch,
                    (self.selection.clone(), self.selection_visual_affinity),
                    (self.selection.clone(), self.selection_visual_affinity),
                    Some(before_math_position.clone()),
                    Some(MathEditPosition::with_edit_state(
                        &next_edit,
                        next_cursor,
                        next_branch.clone(),
                    )),
                );
            }
            MathEditTarget::Block { block_id, align } => {
                let original = find_block_path(&self.document.body, block_id).and_then(
                    |(parent_ids, index)| {
                        block_list_ref(&self.document.body, &parent_ids)
                            .ok()?
                            .get(index)
                            .cloned()
                    },
                );
                let (content, mut source_projection) = source_projection_candidate
                    .take()
                    .ok_or(EditError::InvalidPosition)?;
                let parsed_for_binding = text_edit.as_ref().map(|_| content.clone());
                let mut block = if let Some(text_edit) = &text_edit {
                    let replacement = source
                        .get(old_edit.start..new_edit.end)
                        .ok_or(EditError::InvalidPosition)?;
                    let mut block = original.clone().ok_or(EditError::InvalidPosition)?;
                    let tree = text_run_property::block_math_tree_mut(
                        &mut block,
                        text_edit.path.align_cell(),
                    )
                    .ok_or(EditError::InvalidPosition)?;
                    apply_math_text_split_tree_edit(
                        tree,
                        &text_edit.path,
                        text_edit.range.clone(),
                        replacement,
                        &text_edit.split_math_contents,
                    )?;
                    block
                } else {
                    match (align, content) {
                        (false, caret_math::ProjectedMath::Tree(math_tree)) => {
                            Block::Equation(Equation {
                                id: block_id.clone(),
                                math_tree,
                                extra: Default::default(),
                            })
                        }
                        (true, caret_math::ProjectedMath::Align(parsed)) => Block::Align(Align {
                            id: block_id.clone(),
                            ..parsed
                        }),
                        _ => return Err(EditError::InvalidPosition),
                    }
                };
                match &block {
                    Block::Equation(equation) => equation
                        .math_tree
                        .validate()
                        .map_err(|error| EditError::InvalidMath(error.to_string()))?,
                    Block::Align(align) => align
                        .validate()
                        .map_err(|error| EditError::InvalidMath(error.to_string()))?,
                    _ => return Err(EditError::InvalidPosition),
                }
                let previous_projection = edit.required_source_projection()?;
                let rebound = match (&block, align) {
                    (Block::Equation(equation), false) => {
                        let parsed = parsed_for_binding
                            .as_ref()
                            .and_then(|content| match content {
                                caret_math::ProjectedMath::Tree(tree) => Some(tree),
                                caret_math::ProjectedMath::Align(_) => None,
                            })
                            .unwrap_or(&equation.math_tree);
                        crate::math_validation::bind_tree_sidecar(
                            &mut source_projection,
                            parsed,
                            &equation.math_tree,
                            previous_projection.tree(),
                        )
                    }
                    (Block::Align(align), true) => {
                        let parsed = parsed_for_binding
                            .as_ref()
                            .and_then(|content| match content {
                                caret_math::ProjectedMath::Align(align) => Some(align),
                                caret_math::ProjectedMath::Tree(_) => None,
                            })
                            .unwrap_or(align);
                        crate::math_validation::bind_align_sidecars(
                            &mut source_projection,
                            parsed,
                            align,
                            previous_projection.align(),
                        )
                    }
                    _ => false,
                };
                if !rebound {
                    return Err(EditError::InvalidMath(
                        "edited formula cannot be rebound to its source map".to_owned(),
                    ));
                }
                if text_edit.is_none()
                    && let Some(original) = original.as_ref()
                {
                    let metadata_preserved = preserve_display_math_metadata(
                        original,
                        &mut block,
                        &transaction_old_edit,
                        &new_edit,
                        edit.required_source_projection()?,
                        &source_projection,
                    );
                    if !metadata_preserved {
                        if reject_unmapped_metadata {
                            return Err(EditError::InvalidMath(
                                "editing would discard unmapped math metadata".to_owned(),
                            ));
                        }
                        if text_edit.is_none() && matches!(edit.branch, MathEditBranch::Source) {
                            self.math_edit
                                .as_mut()
                                .ok_or(EditError::InvalidPosition)?
                                .record_source_draft(source, cursor)?;
                            self.advance_history_merge_epoch();
                            return Ok(Effect {
                                selection_changed: true,
                                ..Effect::default()
                            });
                        }
                        return Err(EditError::InvalidMath(
                            "editing would discard unmapped math metadata".to_owned(),
                        ));
                    }
                }
                match &block {
                    Block::Equation(equation) => equation
                        .math_tree
                        .validate()
                        .map_err(|error| EditError::InvalidMath(error.to_string()))?,
                    Block::Align(align) => align
                        .validate()
                        .map_err(|error| EditError::InvalidMath(error.to_string()))?,
                    _ => return Err(EditError::InvalidPosition),
                }
                let rebound = text_edit.is_some()
                    || match (&block, align) {
                        (Block::Equation(equation), false) => {
                            crate::math_validation::bind_tree_sidecar(
                                &mut source_projection,
                                &equation.math_tree,
                                &equation.math_tree,
                                if text_edit.is_none() {
                                    previous_projection.tree()
                                } else {
                                    None
                                },
                            )
                        }
                        (Block::Align(align), true) => crate::math_validation::bind_align_sidecars(
                            &mut source_projection,
                            align,
                            align,
                            if text_edit.is_none() {
                                previous_projection.align()
                            } else {
                                None
                            },
                        ),
                        _ => false,
                    };
                if !rebound {
                    return Err(EditError::InvalidMath(
                        "edited formula cannot be rebound to its source map".to_owned(),
                    ));
                }
                next_edit.replace_source_projection(Some(Arc::new(source_projection)));
                let (parent_ids, index) = find_block_path(&self.document.body, block_id)
                    .ok_or(EditError::InvalidPosition)?;
                let blocks = block_list(&mut self.document.body, &parent_ids)?;
                let slot = blocks.get_mut(index).ok_or(EditError::InvalidPosition)?;
                let before = std::mem::replace(slot, block.clone());
                self.text_map_cache.clear();
                self.advance_history_merge_epoch();
                self.record_math_history(
                    DocumentPatch::BlockRange {
                        parent_environment_ids: parent_ids,
                        sibling_index: index,
                        before: vec![before],
                        after: vec![block],
                    },
                    (self.selection.clone(), self.selection_visual_affinity),
                    (self.selection.clone(), self.selection_visual_affinity),
                    Some(before_math_position),
                    Some(MathEditPosition::with_edit_state(
                        &next_edit,
                        next_cursor,
                        next_branch.clone(),
                    )),
                );
            }
        }
        next_edit.cursor = next_cursor;
        next_edit.branch = next_branch;
        self.math_edit = Some(next_edit);
        self.sync_math_edit_revision();
        let changed_paragraph_id = match &edit.target {
            MathEditTarget::Inline { paragraph_id, .. } => Some(paragraph_id.clone()),
            MathEditTarget::Block { .. } => None,
        };
        Ok(Effect {
            document_changed: true,
            selection_changed,
            changed_paragraph_id,
            position_map,
            ..Effect::default()
        })
    }

    fn empty_construct_range(&self, edit: &MathEdit) -> Option<Range<usize>> {
        fn containing_flow(
            flow: &MathFlow,
            source_map: &caret_math::MathFlowSourceMap,
            cursor: usize,
        ) -> Option<Range<usize>> {
            flow.content
                .iter()
                .zip(source_map.content())
                .find_map(|(content, map)| containing_content(content, map, cursor))
        }

        fn containing_content(
            content: &MathContent,
            source_map: &caret_math::MathContentSourceMap,
            cursor: usize,
        ) -> Option<Range<usize>> {
            match (content, source_map) {
                (
                    MathContent::Math(node),
                    caret_math::MathContentSourceMap::Math { operands, .. },
                ) => {
                    let has_empty_argument =
                        node.children.iter().zip(operands).any(|(flow, flow_map)| {
                            flow.content
                                .iter()
                                .zip(flow_map.content())
                                .any(|(content, map)| {
                                    matches!(
                                        (content, map),
                                        (
                                            MathContent::Math(child),
                                            caret_math::MathContentSourceMap::Math { .. }
                                        ) if child.kind == caret_doc::MathNodeKind::Placeholder
                                            && map.range().is_some_and(|range| {
                                                range.start + 1 == cursor
                                                    && range.end == cursor + 1
                                            })
                                    )
                                })
                        });
                    let all_arguments_empty = !node.children.is_empty()
                        && node.children.iter().all(flow_contains_no_content);
                    // 删除整个构造前确认没有其他有效操作数，否则退格只能按普通源码停靠点尝试。
                    if has_empty_argument
                        && all_arguments_empty
                        && matches!(
                            node.kind,
                            caret_doc::MathNodeKind::Fraction
                                | caret_doc::MathNodeKind::Radical
                                | caret_doc::MathNodeKind::Superscript
                                | caret_doc::MathNodeKind::Subscript
                                | caret_doc::MathNodeKind::SubSup
                        )
                    {
                        return source_map.range();
                    }
                    node.children
                        .iter()
                        .zip(operands)
                        .find_map(|(flow, map)| containing_flow(flow, map, cursor))
                }
                (
                    MathContent::Environment(environment),
                    caret_math::MathContentSourceMap::Environment { children, .. },
                ) => environment
                    .children
                    .iter()
                    .zip(children)
                    .find_map(|(child, map)| containing_content(child, map, cursor)),
                _ => None,
            }
        }

        fn flow_contains_no_content(flow: &MathFlow) -> bool {
            flow.content.iter().all(content_contains_no_content)
        }
        fn content_contains_no_content(content: &MathContent) -> bool {
            match content {
                MathContent::Math(node) => {
                    node.kind == caret_doc::MathNodeKind::Placeholder
                        || (node.kind == caret_doc::MathNodeKind::Group
                            && node.text.as_deref().is_none_or(str::is_empty)
                            && node.children.iter().all(flow_contains_no_content))
                }
                MathContent::Text(text) => text.runs.iter().all(|run| run.text.is_empty()),
                MathContent::Environment(environment) => {
                    environment.children.iter().all(content_contains_no_content)
                }
                MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
            }
        }

        let projection = edit.source_projection.as_deref()?;
        let (tree, source_map) = match &edit.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let story = self.story_for_paragraph(paragraph_id)?;
                let position = Pos {
                    story,
                    paragraph_id: paragraph_id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                };
                let tree = find_inline_math(&self.paragraph_for(&position).ok()?.inlines, math_id)
                    .map(|math| &math.math_tree)?;
                (tree, projection.tree()?)
            }
            MathEditTarget::Block { block_id, align } => {
                let block = find_block_path(&self.document.body, block_id).and_then(
                    |(parents, index)| {
                        block_list_ref(&self.document.body, &parents)
                            .ok()?
                            .get(index)
                    },
                )?;
                if *align {
                    let Block::Align(align) = block else {
                        return None;
                    };
                    let (row, cell, _) = edit.align_cell_at_cursor()?;
                    let tree = align.rows.get(row)?.cells.get(cell)?;
                    let source_map = projection.align()?.cell_tree(row, cell)?;
                    (tree, source_map)
                } else {
                    let Block::Equation(equation) = block else {
                        return None;
                    };
                    (&equation.math_tree, projection.tree()?)
                }
            }
        };
        let cursor = projection.edit_range().start.checked_add(edit.cursor)?;
        let range = containing_flow(&tree.root, source_map.root(), cursor)?;
        Some(
            range.start.checked_sub(projection.edit_range().start)?
                ..range.end.checked_sub(projection.edit_range().start)?,
        )
    }

    fn record_math_history(
        &mut self,
        patch: DocumentPatch,
        before_selection: (Selection, VisualAffinity),
        after_selection: (Selection, VisualAffinity),
        before_math_edit: Option<MathEditPosition>,
        after_math_edit: Option<MathEditPosition>,
    ) {
        let before_pending_style = self.pending_inline_style.clone();
        self.record_math_history_with_pending_style(
            patch,
            before_selection,
            after_selection,
            before_math_edit,
            after_math_edit,
            before_pending_style,
        );
    }

    fn record_math_history_with_pending_style(
        &mut self,
        patch: DocumentPatch,
        before_selection: (Selection, VisualAffinity),
        after_selection: (Selection, VisualAffinity),
        before_math_edit: Option<MathEditPosition>,
        after_math_edit: Option<MathEditPosition>,
        before_pending_style: super::formatting::PendingInlineStyle,
    ) {
        let after_state_id = self.allocate_history_state_id();
        let before_state = SessionHistoryState::new(
            before_selection.0,
            before_selection.1,
            before_pending_style,
            self.history_state_id,
        )
        .with_math_edit(before_math_edit);
        let after_state = SessionHistoryState::new(
            after_selection.0,
            after_selection.1,
            self.pending_inline_style.clone(),
            after_state_id,
        )
        .with_math_edit(after_math_edit);
        self.record_history_edit(SessionHistoryEdit::new(patch, before_state, after_state));
        self.history_state_id = after_state_id;
        self.advance_document_generation();
    }
}

/// 段落内每个行内公式的起始 UTF-8 偏移与节点 ID，按文档顺序。
pub(crate) fn inline_math_slots(paragraph: &Paragraph) -> Result<Vec<(usize, NodeId)>, EditError> {
    fn visit(
        nodes: &[ParagraphContent],
        offset: &mut usize,
        output: &mut Vec<(usize, NodeId)>,
    ) -> Result<(), EditError> {
        for node in nodes {
            match node {
                ParagraphContent::Environment(environment) => {
                    visit(&environment.children, offset, output)?
                }
                ParagraphContent::Math(math) => {
                    output.push((*offset, math.id.clone()));
                    *offset += OBJECT_REPLACEMENT.len_utf8();
                }
                other => *offset += inline_length(other)?,
            }
        }
        Ok(())
    }
    let mut output = Vec::new();
    visit(&paragraph.inlines, &mut 0, &mut output)?;
    Ok(output)
}

pub(super) fn find_inline_math<'a>(
    nodes: &'a [ParagraphContent],
    id: &NodeId,
) -> Option<&'a InlineMath> {
    nodes.iter().find_map(|node| match node {
        ParagraphContent::Math(math) if &math.id == id => Some(math),
        ParagraphContent::Environment(environment) => find_inline_math(&environment.children, id),
        _ => None,
    })
}

fn find_inline_math_mut<'a>(
    nodes: &'a mut [ParagraphContent],
    id: &NodeId,
) -> Option<&'a mut InlineMath> {
    nodes.iter_mut().find_map(|node| match node {
        ParagraphContent::Math(math) if &math.id == id => Some(math),
        ParagraphContent::Environment(environment) => {
            find_inline_math_mut(&mut environment.children, id)
        }
        _ => None,
    })
}

/// 在块树中查找块 ID，返回父环境 ID 路径与同级下标。
pub(crate) fn find_block_path(blocks: &[Block], id: &NodeId) -> Option<(Vec<NodeId>, usize)> {
    let location = caret_structure::find_block_node(blocks, id)?;
    if location.kind != caret_structure::DocumentNodeKind::Paragraph
        && !location.kind.is_range_block()
    {
        return None;
    }
    Some((location.parent_environment_ids, location.sibling_index?))
}

fn block_list<'a>(
    blocks: &'a mut Vec<Block>,
    parent_ids: &[NodeId],
) -> Result<&'a mut Vec<Block>, EditError> {
    caret_structure::block_list_by_environment_ids_mut(blocks, parent_ids)
        .ok_or(EditError::InvalidPosition)
}

fn block_list_ref<'a>(
    blocks: &'a [Block],
    parent_ids: &[NodeId],
) -> Result<&'a [Block], EditError> {
    caret_structure::block_list_by_environment_ids(blocks, parent_ids)
        .ok_or(EditError::InvalidPosition)
}

#[cfg(test)]
fn display_math_block(align: bool, source: &str) -> Result<Block, EditError> {
    display_math_block_with_projection(align, source, NodeId::generate()).map(|(block, _)| block)
}

fn is_bare_math_control_word(template: &str) -> bool {
    template
        .strip_prefix('\\')
        .is_some_and(|name| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphabetic()))
}

fn template_has_root_text_flow(template: &str) -> Result<bool, EditError> {
    let projection = caret_math::parse_projection(template, caret_math::MathInputKind::Inline)
        .map_err(|error| EditError::InvalidMath(error.to_string()))?;
    let caret_math::ProjectedMath::Tree(tree) = projection.content() else {
        return Ok(false);
    };
    Ok(tree
        .root
        .content
        .iter()
        .any(|content| matches!(content, MathContent::Text(_))))
}

/// 将 InlineMath 根序列边界映射到已投影源码；不按序列索引猜测 UTF-8 偏移。
fn inline_root_source_cursor(
    tree: &MathTree,
    source_map: &caret_math::MathTreeSourceMap,
    offset: usize,
    source: &str,
) -> Option<usize> {
    let content = &tree.root.content;
    let maps = source_map.root().content();
    if offset > content.len() || maps.len() != content.len() {
        return None;
    }
    if content.is_empty() {
        return (offset == 0).then_some(0);
    }
    let cursor = if offset < content.len() {
        maps.get(offset)?.range()?.start
    } else if let Some(previous) = offset.checked_sub(1) {
        maps.get(previous)?.range()?.end
    } else {
        let root = source_map.root().range()?;
        let body = source.get(root.clone())?;
        root.start + body.find('{')?.checked_add(1)?
    };
    (cursor <= source.len() && source.is_char_boundary(cursor)).then_some(cursor)
}

fn empty_math_flow(content: Vec<MathContent>) -> MathFlow {
    MathFlow {
        id: NodeId::generate(),
        content,
        extra: BTreeMap::new(),
    }
}

fn placeholder_math_content() -> MathContent {
    MathContent::Math(MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Placeholder,
        text: None,
        attributes: BTreeMap::new(),
        children: Vec::new(),
        extra: BTreeMap::new(),
    })
}

fn insert_math_script(
    tree: &mut MathTree,
    position: &mut MathTreePosition,
    script: &str,
    selected_base: Option<Vec<MathContent>>,
) -> Result<(), EditError> {
    let contents = tree_position::resolve_sequence_mut(tree, &position.path)
        .ok_or(EditError::InvalidPosition)?;
    if position.offset > contents.len() {
        return Err(EditError::InvalidPosition);
    }
    let selected_base = selected_base.filter(|items| !items.is_empty());
    if selected_base.is_none()
        && let Some(base_index) = position.offset.checked_sub(1)
        && let Some(MathContent::Math(node)) = contents.get_mut(base_index)
        && matches!(
            node.kind,
            MathNodeKind::Superscript | MathNodeKind::Subscript
        )
    {
        let (operand_index, insert_at) = match (script, node.kind) {
            ("^", MathNodeKind::Subscript) if node.children.len() == 2 => (2, None),
            ("_", MathNodeKind::Superscript) if node.children.len() == 2 => (1, Some(1)),
            _ => {
                return Err(EditError::InvalidMath(
                    "a script of the same kind already exists".to_owned(),
                ));
            }
        };
        let operand = empty_math_flow(Vec::new());
        if let Some(index) = insert_at {
            node.children.insert(index, operand);
        } else {
            node.children.push(operand);
        }
        node.kind = MathNodeKind::SubSup;
        position.path = child_sequence_path(
            &MathTreePosition {
                host: position.host.clone(),
                path: position.path.clone(),
                offset: position.offset,
                affinity: position.affinity,
            },
            MathSequenceStep::Operand {
                content_index: base_index,
                operand_index,
            },
        );
        position.offset = 0;
        return Ok(());
    }

    let base_index = if selected_base.is_some() {
        position.offset
    } else {
        position.offset.saturating_sub(1)
    };
    let base_contents = if let Some(selected) = selected_base {
        selected
    } else if base_index < position.offset {
        vec![contents.remove(base_index)]
    } else {
        vec![placeholder_math_content()]
    };
    let mut children = vec![empty_math_flow(base_contents), empty_math_flow(Vec::new())];
    let kind = if script == "^" {
        MathNodeKind::Superscript
    } else {
        MathNodeKind::Subscript
    };
    let operand_index = 1;
    contents.insert(
        base_index,
        MathContent::Math(MathNode {
            id: NodeId::generate(),
            kind,
            text: None,
            attributes: BTreeMap::new(),
            children: std::mem::take(&mut children),
            extra: BTreeMap::new(),
        }),
    );
    position.path = child_sequence_path(
        &MathTreePosition {
            host: position.host.clone(),
            path: position.path.clone(),
            offset: position.offset,
            affinity: position.affinity,
        },
        MathSequenceStep::Operand {
            content_index: base_index,
            operand_index,
        },
    );
    position.offset = 0;
    Ok(())
}

fn open_math_group(tree: &mut MathTree, position: &mut MathTreePosition) -> Result<(), EditError> {
    let contents = tree_position::resolve_sequence_mut(tree, &position.path)
        .ok_or(EditError::InvalidPosition)?;
    if position.offset > contents.len() {
        return Err(EditError::InvalidPosition);
    }
    let content_index = position.offset;
    contents.insert(
        content_index,
        MathContent::Math(MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Group,
            text: None,
            attributes: BTreeMap::new(),
            children: vec![empty_math_flow(Vec::new())],
            extra: BTreeMap::new(),
        }),
    );
    position.path = child_sequence_path(
        &MathTreePosition {
            host: position.host.clone(),
            path: position.path.clone(),
            offset: position.offset,
            affinity: position.affinity,
        },
        MathSequenceStep::Operand {
            content_index,
            operand_index: 0,
        },
    );
    position.offset = 0;
    Ok(())
}

fn close_math_group(tree: &MathTree, position: &mut MathTreePosition) -> Result<(), EditError> {
    let Some((
        MathSequenceStep::Operand {
            content_index,
            operand_index,
        },
        parent_steps,
    )) = position.path.steps().split_last()
    else {
        return Err(EditError::InvalidMath("unmatched closing brace".to_owned()));
    };
    let content_index = *content_index;
    let parent_path = MathSequencePath::new(parent_steps.to_vec());
    let parent_position = MathTreePosition {
        host: position.host.clone(),
        path: parent_path.clone(),
        offset: content_index,
        affinity: position.affinity,
    };
    let parent = parent_position
        .resolve(tree)
        .map_err(|_| EditError::InvalidPosition)?;
    let Some(MathContent::Math(node)) = parent.get(content_index) else {
        return Err(EditError::InvalidPosition);
    };
    let closes_group =
        node.kind == MathNodeKind::Group && *operand_index == 0 && node.children.len() == 1;
    let closes_script = matches!(
        node.kind,
        MathNodeKind::Superscript | MathNodeKind::Subscript | MathNodeKind::SubSup
    ) && *operand_index > 0
        && *operand_index < node.children.len();
    if !closes_group && !closes_script {
        return Err(EditError::InvalidMath("unmatched closing brace".to_owned()));
    }
    position.path = parent_path;
    position.offset = content_index
        .checked_add(1)
        .ok_or(EditError::InvalidPosition)?;
    Ok(())
}

fn is_literal_math_input(grapheme: &str) -> bool {
    !matches!(
        grapheme.chars().next(),
        Some('\\' | '{' | '}' | '$' | '%' | '&' | '#' | '_' | '^' | '~')
    )
}

fn script_base_parent_for_start_insertion(
    tree: &MathTree,
    position: &MathTreePosition,
) -> Option<(MathSequencePath, usize)> {
    if position.offset != 0 {
        return None;
    }
    let steps = position.path.steps();
    for (depth, step) in steps.iter().enumerate() {
        let MathSequenceStep::Operand {
            content_index,
            operand_index: 0,
        } = *step
        else {
            continue;
        };
        let parent_path = MathSequencePath::new(steps[..depth].to_vec());
        let parent_position = MathTreePosition {
            host: position.host.clone(),
            path: parent_path.clone(),
            offset: content_index,
            affinity: position.affinity,
        };
        let contents = parent_position.resolve(tree).ok()?;
        let Some(MathContent::Math(node)) = contents.get(content_index) else {
            continue;
        };
        if !matches!(
            node.kind,
            MathNodeKind::Superscript | MathNodeKind::Subscript | MathNodeKind::SubSup
        ) {
            continue;
        }
        if node.children.is_empty() {
            continue;
        }
        let mut base_path = steps[..=depth].to_vec();
        let mut base_position = MathTreePosition {
            host: position.host.clone(),
            path: MathSequencePath::new(base_path.clone()),
            offset: 0,
            affinity: position.affinity,
        };
        let mut base_contents = base_position.resolve(tree).ok()?;
        if base_contents.is_empty() || is_placeholder_at(base_contents, 0) {
            return None;
        }
        let mut nested_path_is_group_start = true;
        for nested_step in &steps[depth + 1..] {
            if *nested_step
                != (MathSequenceStep::Operand {
                    content_index: 0,
                    operand_index: 0,
                })
            {
                nested_path_is_group_start = false;
                break;
            }
            let Some(MathContent::Math(group)) = base_contents.first() else {
                nested_path_is_group_start = false;
                break;
            };
            if group.kind != MathNodeKind::Group || group.children.len() != 1 {
                nested_path_is_group_start = false;
                break;
            }
            base_path.push(*nested_step);
            base_position.path = MathSequencePath::new(base_path.clone());
            base_contents = base_position.resolve(tree).ok()?;
            if base_contents.is_empty() || is_placeholder_at(base_contents, 0) {
                return None;
            }
        }
        if nested_path_is_group_start {
            return Some((parent_path, content_index));
        }
    }
    None
}

fn is_placeholder_at(contents: &[MathContent], index: usize) -> bool {
    matches!(contents.get(index), Some(MathContent::Math(node)) if node.kind == MathNodeKind::Placeholder)
}

fn flush_math_literal_batch(
    tree: &mut MathTree,
    position: &mut MathTreePosition,
    batch: &mut String,
) -> Result<(), EditError> {
    if batch.is_empty() {
        return Ok(());
    }
    insert_math_symbol_run(tree, position, batch)?;
    batch.clear();
    Ok(())
}

fn insert_math_symbol_run(
    tree: &mut MathTree,
    position: &mut MathTreePosition,
    text: &str,
) -> Result<(), EditError> {
    let contents = tree_position::resolve_sequence_mut(tree, &position.path)
        .ok_or(EditError::InvalidPosition)?;
    let cursor = position.offset;
    let next_cursor = resegment_literal_symbol_run(
        contents,
        cursor,
        Some(text),
        MathPositionAffinity::Downstream,
    )?
    .ok_or(EditError::InvalidPosition)?;
    position.offset = next_cursor;
    Ok(())
}

fn insert_math_symbol(
    tree: &mut MathTree,
    position: &mut MathTreePosition,
    grapheme: &str,
) -> Result<(), EditError> {
    let character = grapheme.chars().next().ok_or(EditError::InvalidPosition)?;
    let contents = tree_position::resolve_sequence_mut(tree, &position.path)
        .ok_or(EditError::InvalidPosition)?;
    if position.offset > contents.len() {
        return Err(EditError::InvalidPosition);
    }
    let index = position.offset;
    for (neighbor, inserted_first) in [(index.checked_sub(1), false), (Some(index), true)] {
        let Some(neighbor) = neighbor.filter(|neighbor| *neighbor < contents.len()) else {
            continue;
        };
        let Some(neighbor_text) = contents.get(neighbor).and_then(symbol_text) else {
            continue;
        };
        let joined = if inserted_first {
            format!("{grapheme}{neighbor_text}")
        } else {
            format!("{neighbor_text}{grapheme}")
        };
        if one_grapheme(&joined)
            && contents
                .get(neighbor)
                .is_some_and(symbol_has_opaque_metadata)
        {
            return Err(EditError::UnsupportedOpaqueContent);
        }
    }
    let left_index = index.checked_sub(1);
    let left_text = left_index
        .and_then(|left| contents.get(left))
        .and_then(symbol_text);
    let right_text = contents.get(index).and_then(symbol_text);
    let joined_left = left_text.map(|left| format!("{left}{grapheme}"));
    let merge_left = joined_left.as_deref().is_some_and(one_grapheme)
        && left_index
            .and_then(|left| contents.get(left))
            .is_some_and(|content| literal_symbol_text(content).is_some());
    if joined_left.as_deref().is_some_and(one_grapheme)
        && !merge_left
        && left_index
            .and_then(|left| contents.get(left))
            .is_some_and(symbol_has_opaque_metadata)
    {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    if merge_left {
        let joined = joined_left.ok_or(EditError::InvalidPosition)?;
        let joined_with_right = right_text.map(|right| format!("{joined}{right}"));
        let right_would_join = joined_with_right.as_deref().is_some_and(one_grapheme);
        let merge_right = right_would_join
            && contents
                .get(index)
                .is_some_and(|content| literal_symbol_text(content).is_some());
        if right_would_join
            && !merge_right
            && contents.get(index).is_some_and(symbol_has_opaque_metadata)
        {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let MathContent::Math(node) = contents
            .get_mut(left_index.ok_or(EditError::InvalidPosition)?)
            .ok_or(EditError::InvalidPosition)?
        else {
            return Err(EditError::InvalidPosition);
        };
        node.text = Some(if merge_right {
            joined_with_right.ok_or(EditError::InvalidPosition)?
        } else {
            joined
        });
        if let Some(first_character) = node.text.as_deref().and_then(|text| text.chars().next()) {
            node.attributes.insert(
                "atomClass".to_owned(),
                caret_math::symbols::literal_class(first_character)
                    .as_str()
                    .to_owned(),
            );
        }
        if merge_right {
            contents.remove(index);
        }
        position.offset = index;
        return Ok(());
    }
    if let Some(right_text) = right_text {
        let joined = format!("{grapheme}{right_text}");
        if one_grapheme(&joined) {
            let merge_right = contents
                .get(index)
                .is_some_and(|content| literal_symbol_text(content).is_some());
            if !merge_right && contents.get(index).is_some_and(symbol_has_opaque_metadata) {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            if merge_right {
                let MathContent::Math(node) =
                    contents.get_mut(index).ok_or(EditError::InvalidPosition)?
                else {
                    return Err(EditError::InvalidPosition);
                };
                let first_character = joined.chars().next().ok_or(EditError::InvalidPosition)?;
                node.text = Some(joined);
                node.attributes.insert(
                    "atomClass".to_owned(),
                    caret_math::symbols::literal_class(first_character)
                        .as_str()
                        .to_owned(),
                );
                position.offset = index + 1;
                return Ok(());
            }
        }
    }
    let atom_class = caret_math::symbols::literal_class(character);
    contents.insert(
        index,
        MathContent::Math(MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Symbol,
            text: Some(grapheme.to_owned()),
            attributes: BTreeMap::from([("atomClass".to_owned(), atom_class.as_str().to_owned())]),
            children: Vec::new(),
            extra: BTreeMap::new(),
        }),
    );
    position.offset = index + 1;
    Ok(())
}

fn symbol_text(content: &MathContent) -> Option<&str> {
    match content {
        MathContent::Math(node)
            if node.kind == MathNodeKind::Symbol && node.children.is_empty() =>
        {
            node.text.as_deref()
        }
        _ => None,
    }
}

fn resegment_literal_symbol_run(
    contents: &mut Vec<MathContent>,
    index: usize,
    insertion: Option<&str>,
    affinity: MathPositionAffinity,
) -> Result<Option<usize>, EditError> {
    if index > contents.len() {
        return Err(EditError::InvalidPosition);
    }
    reject_opaque_symbol_resegmentation(contents, index, insertion)?;
    let mut start = index;
    while start > 0
        && contents
            .get(start - 1)
            .and_then(literal_symbol_text)
            .is_some()
    {
        start -= 1;
    }
    let mut end = index;
    while end < contents.len() && contents.get(end).and_then(literal_symbol_text).is_some() {
        end += 1;
    }
    if insertion.is_none() && start == end {
        return Ok(None);
    }

    let mut source = String::new();
    let mut original = Vec::with_capacity(end - start);
    for content in &contents[start..index] {
        let text = literal_symbol_text(content).ok_or(EditError::InvalidPosition)?;
        let byte_start = source.len();
        source.push_str(text);
        original.push((
            symbol_id(content).ok_or(EditError::InvalidPosition)?,
            byte_start..source.len(),
        ));
    }
    let insertion_start = source.len();
    if let Some(insertion) = insertion {
        source.push_str(insertion);
    }
    let insertion_end = source.len();
    for content in &contents[index..end] {
        let text = literal_symbol_text(content).ok_or(EditError::InvalidPosition)?;
        let byte_start = source.len();
        source.push_str(text);
        original.push((
            symbol_id(content).ok_or(EditError::InvalidPosition)?,
            byte_start..source.len(),
        ));
    }
    let boundaries = GraphemeMap::new_with_fences(&source, &[]).into_boundaries();
    let graphemes = boundaries
        .windows(2)
        .filter_map(|range| source.get(range[0]..range[1]).map(str::to_owned))
        .collect::<Vec<_>>();
    let mut assigned_ids = vec![None; graphemes.len()];
    for (original_index, (_, original_range)) in original.iter().enumerate() {
        let first_output = boundaries
            .partition_point(|boundary| *boundary <= original_range.start)
            .saturating_sub(1);
        let output_end = boundaries.partition_point(|boundary| *boundary < original_range.end);
        let best_output = (first_output..output_end)
            .filter_map(|output_index| {
                let range = &boundaries[output_index..=output_index + 1];
                let overlap = original_range
                    .end
                    .min(range[1])
                    .saturating_sub(original_range.start.max(range[0]));
                (overlap > 0).then_some((output_index, overlap))
            })
            .max_by_key(|(output_index, overlap)| {
                let range = &boundaries[*output_index..=*output_index + 1];
                let original_center = original_range.start + original_range.end;
                let output_center = range[0] + range[1];
                (
                    *overlap,
                    std::cmp::Reverse(original_center.abs_diff(output_center)),
                )
            })
            .map(|(output_index, _)| output_index);
        if let Some(output_index) = best_output
            && assigned_ids[output_index].is_none()
        {
            assigned_ids[output_index] = Some(original_index);
        }
    }
    let mut replacement = Vec::with_capacity(graphemes.len());
    for (output_index, grapheme) in graphemes.into_iter().enumerate() {
        let character = grapheme.chars().next().ok_or(EditError::InvalidPosition)?;
        let id = assigned_ids[output_index]
            .and_then(|original_index| original.get(original_index).map(|(id, _)| (*id).clone()))
            .unwrap_or_else(NodeId::generate);
        let attributes = BTreeMap::from([(
            "atomClass".to_owned(),
            caret_math::symbols::literal_class(character)
                .as_str()
                .to_owned(),
        )]);
        let node = MathNode {
            id,
            kind: MathNodeKind::Symbol,
            text: Some(grapheme),
            attributes,
            children: Vec::new(),
            extra: BTreeMap::new(),
        };
        replacement.push(MathContent::Math(node));
    }
    contents.splice(start..end, replacement);

    let target_byte = if insertion.is_some() {
        insertion_end
    } else {
        insertion_start
    };
    let caret_byte = match affinity {
        MathPositionAffinity::Upstream => boundaries
            .iter()
            .copied()
            .take_while(|boundary| *boundary <= target_byte)
            .last()
            .unwrap_or(0),
        MathPositionAffinity::Downstream => boundaries
            .iter()
            .copied()
            .find(|boundary| *boundary >= target_byte)
            .unwrap_or_else(|| boundaries.last().copied().unwrap_or(0)),
    };
    Ok(Some(
        start + boundaries.binary_search(&caret_byte).unwrap_or(0),
    ))
}

fn reject_opaque_symbol_resegmentation(
    contents: &[MathContent],
    index: usize,
    insertion: Option<&str>,
) -> Result<(), EditError> {
    let mut start = index;
    while start > 0
        && contents
            .get(start - 1)
            .and_then(symbol_text)
            .is_some_and(one_grapheme)
    {
        start -= 1;
    }
    let mut end = index;
    while end < contents.len()
        && contents
            .get(end)
            .and_then(symbol_text)
            .is_some_and(one_grapheme)
    {
        end += 1;
    }
    if start == end && insertion.is_none() {
        return Ok(());
    }

    let mut source = String::new();
    let mut protected_ranges = Vec::new();
    for content in &contents[start..index] {
        let text = symbol_text(content).ok_or(EditError::InvalidPosition)?;
        let range_start = source.len();
        source.push_str(text);
        if symbol_has_opaque_metadata(content) {
            protected_ranges.push(range_start..source.len());
        }
    }
    if let Some(insertion) = insertion {
        source.push_str(insertion);
    }
    for content in &contents[index..end] {
        let text = symbol_text(content).ok_or(EditError::InvalidPosition)?;
        let range_start = source.len();
        source.push_str(text);
        if symbol_has_opaque_metadata(content) {
            protected_ranges.push(range_start..source.len());
        }
    }

    let boundaries = GraphemeMap::new_with_fences(&source, &[]).into_boundaries();
    if protected_ranges.iter().any(|range| {
        boundaries.binary_search(&range.start).is_err()
            || boundaries.binary_search(&range.end).is_err()
    }) {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    Ok(())
}

fn symbol_id(content: &MathContent) -> Option<&NodeId> {
    match content {
        MathContent::Math(node) if node.kind == MathNodeKind::Symbol => Some(&node.id),
        _ => None,
    }
}

fn literal_symbol_text(content: &MathContent) -> Option<&str> {
    let MathContent::Math(node) = content else {
        return None;
    };
    let text = symbol_text(content)?;
    let mut characters = text.chars();
    let first = characters.next()?;
    if !one_grapheme(text)
        || matches!(
            first,
            '\\' | '{' | '}' | '$' | '%' | '&' | '#' | '_' | '^' | '~'
        )
        || !node.extra.is_empty()
        || node.attributes.len() != 1
        || node.attributes.get("atomClass").map(String::as_str)
            != Some(caret_math::symbols::literal_class(first).as_str())
    {
        return None;
    }
    Some(text)
}

fn symbol_has_opaque_metadata(content: &MathContent) -> bool {
    let MathContent::Math(node) = content else {
        return false;
    };
    node.kind == MathNodeKind::Symbol
        && (!node.extra.is_empty()
            || node.attributes.iter().any(|(key, value)| {
                key != "atomClass"
                    || !matches!(
                        value.as_str(),
                        "ord" | "op" | "bin" | "rel" | "open" | "close" | "punct" | "inner"
                    )
            }))
}

fn one_grapheme(text: &str) -> bool {
    GraphemeMap::new_with_fences(text, &[])
        .into_boundaries()
        .len()
        == 2
}

#[cfg(test)]
fn display_math_block_with_id(align: bool, source: &str, id: NodeId) -> Result<Block, EditError> {
    display_math_block_with_projection(align, source, id).map(|(block, _)| block)
}

#[cfg(test)]
fn display_math_block_with_projection(
    align: bool,
    source: &str,
    id: NodeId,
) -> Result<(Block, caret_math::MathSourceMap<'static>), EditError> {
    if align {
        let full = format!("{ALIGN_BEGIN}{source}{ALIGN_END}");
        let projection = caret_math::parse_projection(&full, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let (content, source_projection) = projection.into_parts();
        let caret_math::ProjectedMath::Align(parsed) = content else {
            return Err(EditError::InvalidMath("expected aligned math".to_owned()));
        };
        return Ok((Block::Align(Align { id, ..parsed }), source_projection));
    }
    let projection = caret_math::parse_projection(source, caret_math::MathInputKind::Equation)
        .map_err(|error| EditError::InvalidMath(error.to_string()))?;
    let (content, source_projection) = projection.into_parts();
    let caret_math::ProjectedMath::Tree(math_tree) = content else {
        return Err(EditError::InvalidMath("expected equation".to_owned()));
    };
    let block = Block::Equation(Equation {
        id,
        math_tree,
        extra: Default::default(),
    });
    Ok((block, source_projection))
}

fn changed_source_range(previous: &str, next: &str) -> Option<(Range<usize>, Range<usize>)> {
    let mut prefix = previous
        .bytes()
        .zip(next.bytes())
        .take_while(|(left, right)| left == right)
        .count();
    while !previous.is_char_boundary(prefix) || !next.is_char_boundary(prefix) {
        prefix = prefix.checked_sub(1)?;
    }
    let remaining_previous = previous.len().checked_sub(prefix)?;
    let remaining_next = next.len().checked_sub(prefix)?;
    let mut suffix = previous[previous.len() - remaining_previous..]
        .bytes()
        .rev()
        .zip(next[next.len() - remaining_next..].bytes().rev())
        .take_while(|(left, right)| left == right)
        .count();
    while suffix > 0
        && (!previous.is_char_boundary(previous.len() - suffix)
            || !next.is_char_boundary(next.len() - suffix))
    {
        suffix -= 1;
    }
    Some((
        prefix..previous.len().checked_sub(suffix)?,
        prefix..next.len().checked_sub(suffix)?,
    ))
}

fn absolute_source_edit(
    projection: &caret_math::MathSourceMap<'_>,
    edit: &Range<usize>,
) -> Option<Range<usize>> {
    if edit.start > edit.end || edit.end > projection.edit_source().len() {
        return None;
    }
    Some(
        projection.edit_range().start.checked_add(edit.start)?
            ..projection.edit_range().start.checked_add(edit.end)?,
    )
}

#[cfg(test)]
#[path = "math_edit/tests.rs"]
mod tests;
