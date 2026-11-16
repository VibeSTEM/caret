//! 管理 Align 逻辑行的退格语义、结构更新与焦点恢复。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::block_rows::{self, BackspaceAction};
use super::*;

/// Align 逻辑行删除采用的事务。
#[derive(Clone, Copy)]
enum AlignRowTransaction {
    /// 树与文本分支：提交 Align 树事务。
    Tree,
    /// 源码分支：按源码投影改写源码。
    Source,
}

pub(super) fn handle_tree_backspace(
    session: &mut Session,
    edit: &MathEdit,
    align_cell: Option<(usize, usize)>,
    start: usize,
    end: usize,
) -> Result<Option<Effect>, EditError> {
    let MathEditTarget::Block { align: true, .. } = &edit.target else {
        return Ok(None);
    };
    let Some((row_index, cell_index)) = align_cell else {
        return Ok(None);
    };
    let at_row_start = cell_index == 0 && start == 0 && end == 0;
    align_row_backspace(
        session,
        edit,
        row_index,
        at_row_start,
        AlignRowTransaction::Tree,
    )
}

pub(super) fn handle_text_backspace(
    session: &mut Session,
    edit: &MathEdit,
) -> Result<Option<Effect>, EditError> {
    let MathEditTarget::Block { align: true, .. } = &edit.target else {
        return Ok(None);
    };
    let MathEditBranch::Text { path, anchor, .. } = &edit.branch else {
        return Ok(None);
    };
    if path != &anchor.path || anchor.offset != 0 || edit.cursor != 0 {
        return Ok(None);
    }
    let Some((row_index, _cell_index)) = path.align_cell() else {
        return Ok(None);
    };
    // 非空 TextFlow 的退格范围由文本岛仲裁；局部 offset 0 不一定是逻辑行首，
    // 因此这里只按整行为空处理，其余情况交回文本分支。
    align_row_backspace(session, edit, row_index, false, AlignRowTransaction::Tree)
}

pub(super) fn handle_source_backspace(
    session: &mut Session,
    edit: &MathEdit,
) -> Result<Option<Effect>, EditError> {
    if edit.source_draft_is_pending() || !matches!(edit.branch, MathEditBranch::Source) {
        return Ok(None);
    }
    let MathEditTarget::Block { align: true, .. } = &edit.target else {
        return Ok(None);
    };
    let Some((row_index, cell_index, offset)) = edit.align_cell_at_cursor() else {
        return Ok(None);
    };
    let at_row_start = cell_index == 0 && offset == 0;
    align_row_backspace(
        session,
        edit,
        row_index,
        at_row_start,
        AlignRowTransaction::Source,
    )
}

/// Align 行首 Backspace 的统一骨架。
///
/// 查找目标块并按整行内容与行首位置分类：空行校验元数据可安全移除后，仅剩一行时
/// 删除整个数学对象，多行时按调用方选择的 `transaction` 删除该行并迁移焦点；
/// 非空行首返回空效果，行内退格返回 `None` 交给行内编辑器。调用方只负责确定
/// `row_index`、`at_row_start` 与删除事务。
fn align_row_backspace(
    session: &mut Session,
    edit: &MathEdit,
    row_index: usize,
    at_row_start: bool,
    transaction: AlignRowTransaction,
) -> Result<Option<Effect>, EditError> {
    let MathEditTarget::Block {
        block_id,
        align: true,
    } = &edit.target
    else {
        return Ok(None);
    };
    let Block::Align(align) =
        find_block_by_id(&session.document.body, block_id).ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition);
    };
    let row = align
        .rows
        .get(row_index)
        .ok_or(EditError::InvalidPosition)?;
    match block_rows::backspace_action(align_row_content_is_empty(row), at_row_start) {
        BackspaceAction::RemoveEmptyRow => {}
        BackspaceAction::IgnoreNonEmptyRowStart => return Ok(Some(Effect::default())),
        BackspaceAction::DelegateToCell => return Ok(None),
    }
    if !align_row_removal_is_safe(row) {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    let only_row = align.rows.len() == 1;
    // 源码事务在删行前还要确认投影范围可改写（并拒绝含注释 % 的行）；该守卫先于
    // 唯一行删除对象执行，以保持原有的错误优先级。
    let source_removal = match transaction {
        AlignRowTransaction::Tree => None,
        AlignRowTransaction::Source => source_row_removal(edit, align, row_index, only_row)?,
    };
    if only_row {
        if !align.extra.is_empty() {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        return session.delete_math_object().map(Some);
    }
    match transaction {
        AlignRowTransaction::Tree => {
            remove_empty_tree_row(session, edit, align.clone(), row_index).map(Some)
        }
        AlignRowTransaction::Source => {
            let removal = source_removal.ok_or(EditError::InvalidPosition)?;
            session
                .write_math_source_strict(
                    removal.updated_source,
                    removal.cursor,
                    Some(EditGroup::DeleteBackward),
                    removal.start..removal.end,
                )
                .map(Some)
        }
    }
}

/// 源码事务删除空逻辑行所需的源码范围、焦点与改写结果。
struct SourceRowRemoval {
    start: usize,
    end: usize,
    cursor: usize,
    updated_source: String,
}

/// 计算源码事务删除空逻辑行所需的投影范围与焦点。
///
/// `only_row` 为真时只执行投影与注释守卫并返回 `None`：唯一行随后走统一的
/// “删除整个数学对象”路径，但守卫必须先运行，以保持原有的错误优先级。
fn source_row_removal(
    edit: &MathEdit,
    align: &Align,
    row_index: usize,
    only_row: bool,
) -> Result<Option<SourceRowRemoval>, EditError> {
    let projection = edit
        .source_projection
        .as_deref()
        .ok_or(EditError::InvalidPosition)?;
    let align_map = projection.align().ok_or(EditError::InvalidPosition)?;
    let edit_base = projection.edit_range().start;
    let row_range = align_map
        .row_navigation_range(row_index)
        .ok_or(EditError::UnsupportedOpaqueContent)?;
    let source = edit.source().ok_or(EditError::InvalidPosition)?;
    let row_source_start = row_range
        .start
        .checked_sub(edit_base)
        .ok_or(EditError::InvalidPosition)?;
    let row_source_end = row_range
        .end
        .checked_sub(edit_base)
        .ok_or(EditError::InvalidPosition)?;
    if source
        .get(row_source_start..row_source_end)
        .is_none_or(|row_source| row_source.contains('%'))
    {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    if only_row {
        return Ok(None);
    }
    let (target_row, target_cell) = if row_index > 0 {
        let target_row = row_index - 1;
        let target_cell = align.rows[target_row]
            .cells
            .len()
            .checked_sub(1)
            .ok_or(EditError::InvalidPosition)?;
        (target_row, target_cell)
    } else {
        (1, 0)
    };
    let removed_range = if row_index > 0 {
        let preceding_break = align_map
            .row_break_range_after(row_index - 1)
            .ok_or(EditError::UnsupportedOpaqueContent)?;
        preceding_break.start..row_range.end
    } else {
        let following_break = align_map
            .row_break_range_after(row_index)
            .ok_or(EditError::UnsupportedOpaqueContent)?;
        row_range.start..following_break.end
    };
    let start = removed_range
        .start
        .checked_sub(edit_base)
        .ok_or(EditError::InvalidPosition)?;
    let end = removed_range
        .end
        .checked_sub(edit_base)
        .ok_or(EditError::InvalidPosition)?;
    if source.get(start..end).is_none() {
        return Err(EditError::InvalidPosition);
    }
    let target_range = align_map
        .cell_range(target_row, target_cell)
        .ok_or(EditError::InvalidPosition)?;
    let cursor = if row_index > 0 {
        target_range.end.checked_sub(edit_base)
    } else {
        target_range
            .start
            .checked_sub(edit_base)
            .and_then(|target_start| target_start.checked_sub(end - start))
    }
    .ok_or(EditError::InvalidPosition)?;
    let mut updated_source = source.to_owned();
    updated_source.replace_range(start..end, "");
    Ok(Some(SourceRowRemoval {
        start,
        end,
        cursor,
        updated_source,
    }))
}

pub(super) fn commit_align_tree_edit(
    session: &mut Session,
    edit: MathEdit,
    before_align: Align,
    after_align: Align,
    cursor: usize,
    branch: MathEditBranch,
) -> Result<Effect, EditError> {
    after_align
        .validate()
        .map_err(|error| EditError::InvalidMath(error.to_string()))?;
    let MathEditTarget::Block {
        block_id,
        align: true,
    } = &edit.target
    else {
        return Err(EditError::InvalidPosition);
    };
    let before = Block::Align(before_align);
    let after = Block::Align(after_align);
    let (parents, sibling_index) =
        find_block_path(&session.document.body, block_id).ok_or(EditError::InvalidPosition)?;
    let before_math = history_position(&edit);
    let before_selection = (session.selection.clone(), session.selection_visual_affinity);
    let before_pending_style = session.pending_inline_style.clone();
    let document_before = session.document.clone();
    let blocks = block_list(&mut session.document.body, &parents)?;
    let slot = blocks
        .get_mut(sibling_index)
        .ok_or(EditError::InvalidPosition)?;
    *slot = after.clone();
    if let Err(error) = session.document.validate() {
        session.document = document_before;
        return Err(EditError::InvalidDocument(error.to_string()));
    }
    session.text_map_cache.clear();
    session.pending_inline_style.clear();
    let mut after_edit = edit;
    after_edit.replace_source_projection(None);
    after_edit.source_draft = None;
    after_edit.cursor = cursor;
    after_edit.branch = branch;
    session.record_math_history_with_pending_style(
        DocumentPatch::BlockRange {
            parent_environment_ids: parents,
            sibling_index,
            before: vec![before],
            after: vec![after],
        },
        before_selection.clone(),
        before_selection,
        Some(before_math),
        Some(history_position(&after_edit)),
        before_pending_style,
    );
    session.refresh_math_edit_revision(&mut after_edit);
    session.math_edit = Some(after_edit);
    Ok(Effect {
        document_changed: true,
        selection_changed: true,
        ..Effect::default()
    })
}

fn align_row_content_is_empty(row: &MathAlignRow) -> bool {
    !row.cells.is_empty()
        && row
            .cells
            .iter()
            .all(|cell| math_flow_content_is_empty(&cell.root))
}

fn align_row_removal_is_safe(row: &MathAlignRow) -> bool {
    row.extra.is_empty()
        && row
            .cells
            .iter()
            .all(|cell| cell.extra.is_empty() && math_flow_removal_is_safe(&cell.root))
}

fn math_flow_content_is_empty(flow: &MathFlow) -> bool {
    flow.content.iter().all(math_content_is_empty)
}

fn math_content_is_empty(content: &MathContent) -> bool {
    match content {
        MathContent::Math(node) => {
            node.kind == MathNodeKind::Placeholder
                && node.text.is_none()
                && node.children.is_empty()
        }
        MathContent::Text(text) => text.runs.iter().all(|run| run.text.is_empty()),
        MathContent::Environment(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
    }
}

fn math_flow_removal_is_safe(flow: &MathFlow) -> bool {
    flow.extra.is_empty() && flow.content.iter().all(math_content_removal_is_safe)
}

fn math_content_removal_is_safe(content: &MathContent) -> bool {
    match content {
        MathContent::Math(node) => {
            node.extra.is_empty()
                && node.attributes.is_empty()
                && node.kind == MathNodeKind::Placeholder
                && node.text.is_none()
                && node.children.is_empty()
        }
        MathContent::Text(text) => {
            text.extra.is_empty()
                && text.runs.iter().all(|run| {
                    run.text.is_empty()
                        && run.language.is_none()
                        && run.props.is_empty()
                        && run.extra.is_empty()
                })
        }
        MathContent::Environment(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
    }
}

fn remove_empty_tree_row(
    session: &mut Session,
    edit: &MathEdit,
    before_align: Align,
    row_index: usize,
) -> Result<Effect, EditError> {
    if before_align.rows.len() <= 1 {
        return Err(EditError::InvalidPosition);
    }
    let mut after_align = before_align.clone();
    after_align.rows.remove(row_index);
    let (target_row_index, target_cell_index, direction) = if row_index > 0 {
        let previous_row_index = row_index - 1;
        let previous_cell_index = after_align.rows[previous_row_index]
            .cells
            .len()
            .checked_sub(1)
            .ok_or(EditError::InvalidPosition)?;
        (previous_row_index, previous_cell_index, Direction::Backward)
    } else {
        (0, 0, Direction::Forward)
    };
    let target_row = after_align
        .rows
        .get(target_row_index)
        .ok_or(EditError::InvalidPosition)?;
    let target_tree = target_row
        .cells
        .get(target_cell_index)
        .ok_or(EditError::InvalidPosition)?;
    let target_host = MathPositionHost::AlignCell {
        row_id: target_row.id.clone(),
        cell_id: target_tree.root.id.clone(),
    };
    let (cursor, branch) = math_tree_edge_position(
        target_tree,
        target_host,
        direction,
        Some((target_row_index, target_cell_index)),
    )?;
    commit_align_tree_edit(
        session,
        edit.clone(),
        before_align,
        after_align,
        cursor,
        branch,
    )
}
