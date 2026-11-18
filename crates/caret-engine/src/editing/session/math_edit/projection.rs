//! 维护数学文本岛的选区、光标位置与源码投影。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

impl Session {
    /// 将宿主测得的字节位置限制在当前 TextFlow，并拒绝半个 UTF-8 字符或 grapheme。
    pub(super) fn set_math_text_selection(
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
            current.branch = MathEditBranch::text_selection(
                anchor_position,
                focus_position,
                anchor_visual_affinity,
                visual_affinity,
            );
        }
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    pub(super) fn math_tree_for_edit<'a>(
        &'a self,
        edit: &MathEdit,
        align_cell: Option<(usize, usize)>,
    ) -> Result<&'a caret_doc::MathTree, EditError> {
        math_tree_for_target(&self.document, &edit.target, align_cell)
            .ok_or(EditError::InvalidPosition)
    }

    pub(super) fn math_tree_position_at_text_boundary<'a>(
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

    pub(super) fn align_cell_for_position(
        &self,
        edit: &MathEdit,
        position: &MathTreePosition,
    ) -> Result<Option<(usize, usize)>, EditError> {
        self.align_cell_for_target_and_host(&edit.target, &position.host)
    }

    pub(in crate::editing::session) fn align_cell_for_target_and_host(
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

    pub(in crate::editing::session) fn math_position_host_for_target(
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

    pub(super) fn apply_math_tree_caret_move(
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
                    current.branch = MathEditBranch::text(
                        path,
                        cursor,
                        MathPositionAffinity::Downstream,
                        VisualAffinity::Downstream,
                    );
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

    pub(super) fn move_to_adjacent_align_cell(
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
    pub(in crate::editing::session) fn refresh_math_text_projection(
        &mut self,
    ) -> Result<(), EditError> {
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
        edit.branch = MathEditBranch::text_selection(
            anchor,
            MathTextPosition::new(path, edit.cursor, MathPositionAffinity::Downstream),
            VisualAffinity::Downstream,
            VisualAffinity::Downstream,
        );
        self.set_math_edit(Some(edit));
        Ok(())
    }

    /// 返回当前 TextFlow 选择所在文本岛及 anchor/focus 的岛级 UTF-8 偏移。
    pub(in crate::editing::session) fn math_text_selection_island_offsets(
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

    pub(super) fn math_text_path_at_source_boundary(
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

    pub(super) fn enter_math_text_branch(&mut self, path: MathTextFlowPath, cursor: usize) {
        self.pending_inline_style.clear();
        if let Some(edit) = &mut self.math_edit {
            edit.replace_source_projection(None);
            edit.source_draft = None;
            edit.cursor = cursor;
            edit.branch = MathEditBranch::text(
                path,
                cursor,
                MathPositionAffinity::Downstream,
                VisualAffinity::Downstream,
            );
        }
        self.advance_history_merge_epoch();
    }

    /// 移动公式内光标；光标移动切断连续输入的撤销合并。
    pub(super) fn set_math_cursor(&mut self, cursor: usize) {
        if let Some(edit) = &mut self.math_edit
            && edit.cursor != cursor
        {
            edit.cursor = cursor;
            self.advance_history_merge_epoch();
        }
    }
}
