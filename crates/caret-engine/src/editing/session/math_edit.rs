//! 公式编辑状态：维护已提交树投影，并隔离可恢复的源码草稿。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::paragraph_ops::BlockContainerPath;
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
mod entry_exit;
pub(super) mod inline_root_split;
pub(super) mod inline_selection_step;
mod metadata;
mod multiline;
mod navigation;
mod path;
mod projection;
pub(super) mod selection_extension;
mod source_commit;
mod source_draft;
mod symbol_input;
mod text_branch;
mod text_inline_environment;
mod text_run_property;
mod text_run_split;
mod tree_edit;
mod tree_position;
use super::super::text_map::{OBJECT_REPLACEMENT, TextMap, inline_length, split_inline_nodes};
use super::super::{
    Affinity, Command, CommandLine, Direction, EditError, Effect, HostCaretKind, HostCaretPosition,
    HostCaretTransition, Pos, PositionMap, Selection, StoryId,
};
use super::Session;
use super::document_position::visual_affinity_for_block_edge;
use super::history::{self, DocumentPatch, EditGroup, MathEditPosition, SessionHistoryState};
use super::range_extension::AdjacentComplexUnit;
use crate::{StructureIndex, VisualAffinity};
#[cfg(test)]
use caret_doc::TextFlow;
use caret_doc::{
    Align, Block, BlockEnvironment, Equation, InlineMath, MathAlignRow, MathContent, MathFlow,
    MathNode, MathNodeKind, MathTree, NodeId, Paragraph, ParagraphContent,
};
use inline_root_split::{
    InlineRootFocus, replace_inline_math_with_split, split_inline_math_root_with_map,
};
pub(super) use path::TextIslandSnapshot;
pub use path::{MathTextFlowPath, MathTextFlowPathStep, MathTextPosition};
use source_draft::MathSourceDraft;
use text_branch::is_grapheme_boundary;
pub(super) use text_branch::math_text_split_is_available;
pub use tree_position::{
    MathPositionAffinity, MathPositionHost, MathSequencePath, MathSequenceStep, MathTreePosition,
    MathTreePositionError,
};
pub(super) use tree_position::{MathTextFlowBoundary, compare_math_position_affinity};
use tree_position::{MathTreeCaretMove, advance_math_tree_position, child_sequence_path};
pub(super) use tree_position::{math_tree_order_key, resolve_sequence_mut};

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
    container: BlockContainerPath,
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
        return Ok((
            cursor,
            MathEditBranch::text(
                path,
                cursor,
                MathPositionAffinity::Downstream,
                VisualAffinity::Downstream,
            ),
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

/// 数学位置亲和性映射为文本位置的逻辑亲和性。
fn math_position_affinity_to_affinity(affinity: MathPositionAffinity) -> Affinity {
    match affinity {
        MathPositionAffinity::Upstream => Affinity::Upstream,
        MathPositionAffinity::Downstream => Affinity::Downstream,
    }
}

/// 在数学树指定路径的内容区间内定位第一个可编辑空槽。
///
/// 优先返回直接位于当前序列中的占位节点；否则按结构顺序下钻数学节点的非可选
/// 占位操作数与行内环境 children。`range` 只限定起始序列的扫描内容索引范围，
/// 递归下钻始终扫描子序列的完整范围；Text/SoftBreak/Opaque 不参与定位。
pub(super) fn first_placeholder_position(
    tree: &MathTree,
    path: &MathSequencePath,
    range: Range<usize>,
) -> Option<(MathSequencePath, usize)> {
    let contents = sequence_contents(tree, path)?;
    first_placeholder_in_contents(contents, path, range)
}

fn first_placeholder_in_contents(
    contents: &[MathContent],
    path: &MathSequencePath,
    range: Range<usize>,
) -> Option<(MathSequencePath, usize)> {
    for content_index in range {
        match contents.get(content_index)? {
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
                    if let Some(found) = first_placeholder_in_contents(
                        &flow.content,
                        &MathSequencePath::new(steps),
                        0..flow.content.len(),
                    ) {
                        return Some(found);
                    }
                }
            }
            MathContent::Environment(environment) => {
                let mut steps = path.steps().to_vec();
                steps.push(MathSequenceStep::EnvironmentChildren { content_index });
                if let Some(found) = first_placeholder_in_contents(
                    &environment.children,
                    &MathSequencePath::new(steps),
                    0..environment.children.len(),
                ) {
                    return Some(found);
                }
            }
            MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
        }
    }
    None
}

/// 解析结构路径对应的内容序列；路径末端不要求是 TextFlow。
fn sequence_contents<'a>(tree: &'a MathTree, path: &MathSequencePath) -> Option<&'a [MathContent]> {
    let mut contents = tree.root.content.as_slice();
    for step in path.steps() {
        contents = match *step {
            MathSequenceStep::Operand {
                content_index,
                operand_index,
            } => match contents.get(content_index)? {
                MathContent::Math(node) => node.children.get(operand_index)?.content.as_slice(),
                _ => return None,
            },
            MathSequenceStep::EnvironmentChildren { content_index } => {
                match contents.get(content_index)? {
                    MathContent::Environment(environment) => environment.children.as_slice(),
                    _ => return None,
                }
            }
        };
    }
    Some(contents)
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

pub(super) fn math_content_has_unknown_metadata(content: &MathContent) -> bool {
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

pub(super) fn math_text_offsets_are_boundaries(
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

impl MathEditBranch {
    /// 进入单一 TextFlow 的文本分支：锚点与焦点重合，两侧视觉侧相同。
    pub(super) fn text(
        path: MathTextFlowPath,
        offset: usize,
        affinity: MathPositionAffinity,
        visual_affinity: VisualAffinity,
    ) -> Self {
        Self::text_selection(
            MathTextPosition::new(path.clone(), offset, affinity),
            MathTextPosition::new(path, offset, affinity),
            visual_affinity,
            visual_affinity,
        )
    }

    /// 进入文本分支并保留两端各自的位置；两端可位于同一文本岛的不同 TextFlow。
    pub(super) fn text_selection(
        anchor: MathTextPosition,
        focus: MathTextPosition,
        anchor_visual_affinity: VisualAffinity,
        visual_affinity: VisualAffinity,
    ) -> Self {
        Self::Text {
            path: focus.path,
            anchor,
            focus_affinity: focus.affinity,
            anchor_visual_affinity,
            visual_affinity,
        }
    }
}

struct MathSourceWriteOptions {
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
    pub(super) source_projection: Option<Arc<caret_math::MathSourceMap<'static>>>,
    pub(super) source_draft: Option<MathSourceDraft>,
    /// 绑定投影的会话与文档 generation；不一致时拒绝向该投影应用编辑。
    pub(super) revision: MathEditRevision,
    pub(super) cursor: usize,
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
    /// 进入公式编辑的初始状态：没有源码投影，也没有可恢复的源码草稿。
    ///
    /// `revision` 由调用方在同一时点从会话与文档代际取得，构造器不自行生成身份。
    pub(super) fn entering(
        target: MathEditTarget,
        cursor: usize,
        branch: MathEditBranch,
        revision: MathEditRevision,
    ) -> Self {
        Self {
            target,
            source_projection: None,
            source_draft: None,
            revision,
            cursor,
            branch,
        }
    }

    /// 进入源码分支并绑定既有源码投影；`None` 表示该编辑尚无投影。
    pub(super) fn editing_source(
        target: MathEditTarget,
        source_projection: Option<Arc<caret_math::MathSourceMap<'static>>>,
        cursor: usize,
        revision: MathEditRevision,
    ) -> Self {
        Self {
            source_projection,
            ..Self::entering(target, cursor, MathEditBranch::Source, revision)
        }
    }

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
        let mut snapshot = Self::entering(
            self.target.clone(),
            self.cursor,
            self.branch.clone(),
            self.revision.clone(),
        );
        snapshot
            .source_projection
            .clone_from(&self.source_projection);
        snapshot.source_draft = self
            .source_draft
            .as_ref()
            .map(MathSourceDraft::command_snapshot);
        snapshot
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
        return Some(MathEdit::entering(
            position.target,
            position.cursor,
            position.branch,
            revision,
        ));
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
        return Some(MathEdit::entering(
            position.target,
            position.cursor,
            position.branch,
            revision,
        ));
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
    Some(MathEdit::editing_source(
        position.target,
        Some(source_projection),
        position.cursor,
        revision,
    ))
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
        Block::Table(table) => table.rows.iter().find_map(|row| {
            row.cells
                .iter()
                .find_map(|cell| find_inline_math_in_blocks(&cell.blocks, paragraph_id, math_id))
        }),
        _ => None,
    })
}

pub(super) fn find_block_by_id<'a>(blocks: &'a [Block], id: &NodeId) -> Option<&'a Block> {
    let (container, index) = find_block_container_path(blocks, id)?;
    caret_structure::block_list_by_container(blocks, &container)?.get(index)
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
        self.set_math_edit(Some(MathEdit::entering(
            hit.target.clone(),
            focused_position.offset,
            MathEditBranch::Tree {
                path: focused_position.path.clone(),
                anchor: focused_position,
            },
            self.current_math_edit_revision(),
        )));
        self.pending_inline_style.clear();
        self.clear_structure_selection_state();
        self.clear_host_caret_position();
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
        self.set_math_edit(Some(MathEdit::entering(
            hit.target.clone(),
            hit.position.offset,
            MathEditBranch::text(
                hit.position.path.clone(),
                hit.position.offset,
                hit.position.affinity,
                hit.visual_affinity,
            ),
            self.current_math_edit_revision(),
        )));
        self.pending_inline_style.clear();
        self.clear_structure_selection_state();
        self.clear_host_caret_position();
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    /// 原子应用公式文本拖选端点，保留当前会话的身份、代际与历史。
    ///
    /// 同一宿主与文本岛 token 内可跨 TextFlow 路径；两个端点在候选状态中校验字素边界。任一端点无效时，
    /// 原会话完全不变。候选副本保留身份，避免纯选区操作伪装成会话替换。
    pub fn set_math_text_selection_from_hits(
        &mut self,
        anchor_hit: &crate::PageMathTextHit,
        focus_hit: &crate::PageMathTextHit,
        current_font_generation: u64,
    ) -> Result<Effect, EditError> {
        if anchor_hit.target != focus_hit.target || anchor_hit.token != focus_hit.token {
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

pub(super) fn find_inline_math_mut<'a>(
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

/// 在块树中查找块 ID，返回真实容器路径与同级下标；环境与表格单元格统一寻址。
pub(crate) fn find_block_container_path(
    blocks: &[Block],
    id: &NodeId,
) -> Option<(BlockContainerPath, usize)> {
    let location = caret_structure::find_block_node(blocks, id)?;
    if location.kind != caret_structure::DocumentNodeKind::Paragraph
        && !location.kind.is_range_block()
    {
        return None;
    }
    Some((location.container, location.sibling_index?))
}

/// 兼容投影：只返回父环境 ID 链；新代码应使用 [`find_block_container_path`]。
pub(crate) fn find_block_path(blocks: &[Block], id: &NodeId) -> Option<(Vec<NodeId>, usize)> {
    find_block_container_path(blocks, id)
        .map(|(container, index)| (container.environment_ids(), index))
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

use symbol_input::{
    absolute_source_edit, changed_source_range, close_math_group, flush_math_literal_batch,
    insert_math_script, insert_math_symbol, is_literal_math_input, open_math_group,
    resegment_literal_symbol_run, script_base_parent_for_start_insertion,
};
#[cfg(test)]
use symbol_input::{
    display_math_block_with_id, display_math_block_with_projection, literal_symbol_text,
    symbol_text,
};
/// 返回块环境在方向上的第一个可编辑入口，供逐块水平导航和公式外侧停靠使用。
pub(super) fn block_environment_entry(
    environment: &BlockEnvironment,
    direction: Direction,
) -> Option<Pos> {
    match direction {
        Direction::Forward => {
            if let Some(title) = &environment.title {
                return Some(environment_title_position(
                    environment,
                    title,
                    Affinity::Downstream,
                ));
            }
            first_entry_in_blocks(&environment.children)
        }
        Direction::Backward => {
            if let Some(position) = last_entry_in_blocks(&environment.children) {
                return Some(position);
            }
            let title = environment.title.as_ref()?;
            let offset = TextMap::new(title).ok()?.text.len();
            Some(Pos {
                offset,
                affinity: Affinity::Upstream,
                ..environment_title_position(environment, title, Affinity::Upstream)
            })
        }
    }
}

fn environment_title_position(
    environment: &BlockEnvironment,
    title: &Paragraph,
    affinity: Affinity,
) -> Pos {
    Pos {
        story: StoryId::EnvironmentTitle(environment.id.clone()),
        paragraph_id: title.id.clone(),
        offset: 0,
        affinity,
    }
}

/// 环境入口的既定契约是“标题起点 / 首个可编辑文本段落”，不直接停靠到首个子公式。
///
/// 公式外停靠进入环境时复用该入口（见 `block_environment_entry`）；环境内部以公式
/// 开头/结尾时，随后按方向键由共享块顺序仲裁逐个进入该公式，不在入口一步伪造
/// 块外侧停靠。空环境或全是不可编辑块时返回 `None`，由调用方保持 no-op。
fn first_entry_in_blocks(blocks: &[Block]) -> Option<Pos> {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                return Some(Pos {
                    story: StoryId::Body,
                    paragraph_id: paragraph.id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                });
            }
            Block::Environment(environment) => {
                if let Some(position) = block_environment_entry(environment, Direction::Forward) {
                    return Some(position);
                }
            }
            // 表格内部首个/末个位置随 046 的单元格导航接入。
            Block::Table(_)
            | Block::Equation(_)
            | Block::Align(_)
            | Block::ExplicitBreak(_)
            | Block::Opaque(_) => {}
        }
    }
    None
}

fn last_entry_in_blocks(blocks: &[Block]) -> Option<Pos> {
    for block in blocks.iter().rev() {
        match block {
            Block::Paragraph(paragraph) => {
                let offset = TextMap::new(paragraph).ok()?.text.len();
                return Some(Pos {
                    story: StoryId::Body,
                    paragraph_id: paragraph.id.clone(),
                    offset,
                    affinity: Affinity::Upstream,
                });
            }
            Block::Environment(environment) => {
                if let Some(position) = block_environment_entry(environment, Direction::Backward) {
                    return Some(position);
                }
            }
            Block::Table(_)
            | Block::Equation(_)
            | Block::Align(_)
            | Block::ExplicitBreak(_)
            | Block::Opaque(_) => {}
        }
    }
    None
}

#[cfg(test)]
#[path = "math_edit/tests.rs"]
mod tests;
