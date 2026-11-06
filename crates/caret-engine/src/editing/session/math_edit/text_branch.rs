//! 数学 TextFlow 分支的源码映射、Run 操作与文本编辑事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::flow_edit::insertion_template;
use super::super::super::flow_edit::replace_text_flow_range;
use super::super::super::grapheme::GraphemeMap;
use super::super::history::{self, DocumentPatch, EditGroup};
use super::super::{Affinity, Direction, EditError, Effect};
use super::Session;
use super::navigation::{next_grapheme_end, previous_grapheme_start};
use super::{
    MathEdit, MathEditBranch, MathEditTarget, MathPositionAffinity, MathPositionHost,
    MathSequencePath, MathSequenceStep, MathTextFlowPath, MathTextPosition, MathTreePosition,
    history_position, math_text_selection_range,
};
use caret_doc::{Block, MathContent, MathNodeKind};

mod format_tree_edit;
mod split_tree_edit;
pub(super) use format_tree_edit::{
    apply_math_text_environment_tree_range, apply_math_text_font_family_tree_range,
};
pub(super) use split_tree_edit::apply_math_text_split_tree_edit;
use std::ops::Range;

#[derive(Debug, Clone)]
pub(super) struct MathTextEdit {
    pub(super) path: MathTextFlowPath,
    pub(super) range: Range<usize>,
    pub(super) split_math_contents: Vec<MathContent>,
}

#[derive(Clone, Copy)]
pub(super) enum PendingStylePolicy {
    Clear,
    Preserve,
}

pub(super) struct MathTextTreeCommit {
    pub(super) branch: MathEditBranch,
    pub(super) align_cell: Option<(usize, usize)>,
    pub(super) cursor: usize,
    pub(super) merge_group: Option<EditGroup>,
    pub(super) pending_style: PendingStylePolicy,
    pub(super) selection_changed: bool,
}

pub(super) fn text_source_boundary_from_map(
    text: &caret_doc::TextFlow,
    source_map: &caret_math::MathTextSourceMap,
    source_base: usize,
    offset: usize,
    affinity: Affinity,
) -> Option<usize> {
    let text_len = text
        .runs
        .iter()
        .try_fold(0usize, |length, run| length.checked_add(run.text.len()))?;
    if offset > text_len {
        return None;
    }
    if text_len == 0 && source_map.segments().is_empty() {
        return empty_text_source_boundary(source_map.range(), source_base);
    }
    let mut upstream = None;
    let mut downstream = None;
    for segment in source_map.segments() {
        let display = segment.display();
        let host_source = segment.source()?;
        let source_start = host_source.start.checked_sub(source_base)?;
        let source_end = host_source.end.checked_sub(source_base)?;
        if offset == display.start {
            downstream = Some(source_start);
        }
        if offset == display.end {
            upstream = Some(source_end);
        } else if display.start < offset
            && offset < display.end
            && display.len() == host_source.len()
        {
            return source_start.checked_add(offset - display.start);
        }
    }
    match affinity {
        Affinity::Upstream => upstream.or(downstream),
        Affinity::Downstream => downstream.or(upstream),
    }
}

/// 空 TextFlow 没有映射片段时，以其唯一零宽源码范围定位光标。
pub(super) fn empty_text_source_boundary(
    range: Option<Range<usize>>,
    source_base: usize,
) -> Option<usize> {
    let range = range?;
    (range.start == range.end)
        .then(|| range.start.checked_sub(source_base))
        .flatten()
}

#[cfg(test)]
pub(super) fn text_source_range_from_map(
    source_map: &caret_math::MathTextSourceMap,
    source_base: usize,
    range: Range<usize>,
) -> Option<Range<usize>> {
    if range.start >= range.end {
        return None;
    }
    let mut mapped = range.start;
    let mut source_range = None;
    for segment in source_map.segments() {
        let display = segment.display();
        let overlap_start = range.start.max(display.start);
        let overlap_end = range.end.min(display.end);
        if overlap_start >= overlap_end {
            continue;
        }
        if overlap_start != mapped {
            return None;
        }
        let host_source = segment.source()?;
        let source_start = host_source.start.checked_sub(source_base)?;
        let source_end = host_source.end.checked_sub(source_base)?;
        let linear = display.len() == host_source.len();
        if !linear && (overlap_start != display.start || overlap_end != display.end) {
            return None;
        }
        let mapped_source_start = if linear {
            source_start.checked_add(overlap_start - display.start)?
        } else {
            source_start
        };
        if source_range
            .as_ref()
            .is_some_and(|previous: &Range<usize>| previous.end != mapped_source_start)
        {
            return None;
        }
        let mapped_source_end = if linear {
            source_start.checked_add(overlap_end - display.start)?
        } else {
            source_end
        };
        match &mut source_range {
            Some(current) => current.end = mapped_source_end,
            None => source_range = Some(mapped_source_start..mapped_source_end),
        }
        mapped = overlap_end;
    }
    (mapped == range.end)
        .then_some(source_range?)
        .filter(|source_range| source_range.start < source_range.end)
}

pub(super) fn is_grapheme_boundary(text: &str, offset: usize) -> bool {
    GraphemeMap::is_boundary_in(text, offset)
}

pub(super) fn snap_text_cursor(text: &str, offset: usize, forward: bool) -> Option<usize> {
    GraphemeMap::snap_in(
        text,
        offset,
        if forward {
            Affinity::Downstream
        } else {
            Affinity::Upstream
        },
    )
}

/// 拒绝无法安全迁移到新源码位置的 Run 扩展字段。
pub(super) fn validate_text_flow_edit(
    text: &caret_doc::TextFlow,
    source_map: Option<&caret_math::MathTextSourceMap>,
    range: Range<usize>,
    inserts: bool,
) -> Result<(), EditError> {
    if source_map.is_some_and(|map| {
        !map.range_extra().is_empty()
            || map
                .segments()
                .iter()
                .any(|segment| !segment.extra().is_empty() || !segment.source_extra().is_empty())
    }) {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    let mut offset = 0;
    for run in &text.runs {
        let end = offset + run.text.len();
        let overlaps = range.start < range.end && offset < range.end && range.start < end;
        if overlaps && !run.extra.is_empty() {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        if run.text.is_empty()
            && range.start <= offset
            && offset <= range.end
            && !run.extra.is_empty()
        {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        offset = end;
    }
    if inserts
        && insertion_template(&text.runs, range.start).is_some_and(|run| !run.extra.is_empty())
    {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    Ok(())
}

impl Session {
    pub(super) fn commit_math_text_tree(
        &mut self,
        edit: MathEdit,
        tree: caret_doc::MathTree,
        commit: MathTextTreeCommit,
    ) -> Result<Effect, EditError> {
        let MathTextTreeCommit {
            branch,
            align_cell,
            cursor,
            merge_group,
            pending_style,
            selection_changed,
        } = commit;
        let before_math = history_position(&edit);
        let before_selection = (self.selection.clone(), self.selection_visual_affinity);
        let before_pending_style = self.pending_inline_style.clone();
        let mut after_edit = edit.clone();
        after_edit.source_projection = None;
        after_edit.source_draft = None;
        after_edit.cursor = cursor;
        after_edit.branch = branch;
        match &edit.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let story = self
                    .story_for_paragraph(paragraph_id)
                    .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
                let position = super::super::Pos {
                    story,
                    paragraph_id: paragraph_id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                };
                let mut paragraph = self.paragraph_for(&position)?.clone();
                let math = super::find_inline_math_mut(&mut paragraph.inlines, math_id)
                    .ok_or(EditError::InvalidPosition)?;
                math.math_tree = tree;
                let before = history::paragraph_states_for_ids(
                    &self.document,
                    &self.paragraph_index,
                    [paragraph_id.clone()],
                );
                let document_before = self.document.clone();
                let paragraph_index_before = self.paragraph_index.clone();
                self.replace_paragraph(&position, paragraph)?;
                if let Err(error) = self.document.validate() {
                    self.document = document_before;
                    self.paragraph_index = paragraph_index_before;
                    self.text_map_cache.clear();
                    return Err(EditError::InvalidDocument(error.to_string()));
                }
                self.text_map_cache.clear();
                let after = history::paragraph_states_for_ids(
                    &self.document,
                    &self.paragraph_index,
                    [paragraph_id.clone()],
                );
                if matches!(pending_style, PendingStylePolicy::Clear) {
                    self.pending_inline_style.clear();
                }
                self.record_math_history_with_pending_style(
                    DocumentPatch::Paragraphs {
                        before,
                        after,
                        merge_group,
                        merge_epoch: self.history_merge_epoch,
                    },
                    before_selection.clone(),
                    before_selection,
                    Some(before_math),
                    Some(history_position(&after_edit)),
                    before_pending_style,
                );
            }
            MathEditTarget::Block { block_id, .. } => {
                let document_before = self.document.clone();
                let (parent_environment_ids, sibling_index) =
                    super::find_block_path(&self.document.body, block_id)
                        .ok_or(EditError::InvalidPosition)?;
                let blocks = super::block_list(&mut self.document.body, &parent_environment_ids)?;
                let slot = blocks
                    .get_mut(sibling_index)
                    .ok_or(EditError::InvalidPosition)?;
                let before = slot.clone();
                let mut after = before.clone();
                match (&mut after, align_cell) {
                    (Block::Equation(equation), None) => equation.math_tree = tree,
                    (Block::Align(align), Some((row, cell))) => {
                        let target = align
                            .rows
                            .get_mut(row)
                            .and_then(|row| row.cells.get_mut(cell))
                            .ok_or(EditError::InvalidPosition)?;
                        *target = tree;
                    }
                    _ => return Err(EditError::InvalidPosition),
                }
                *slot = after.clone();
                if let Err(error) = self.document.validate() {
                    self.document = document_before;
                    return Err(EditError::InvalidDocument(error.to_string()));
                }
                self.text_map_cache.clear();
                if matches!(pending_style, PendingStylePolicy::Clear) {
                    self.pending_inline_style.clear();
                }
                self.record_math_history_with_pending_style(
                    DocumentPatch::BlockRange {
                        parent_environment_ids,
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
            }
        }
        after_edit.revision = self.current_math_edit_revision();
        self.math_edit = Some(after_edit);
        Ok(Effect {
            document_changed: true,
            selection_changed,
            ..Effect::default()
        })
    }

    /// 将文本岛选区解析并提升为 MathTree 分支，同时保留选区前后的文本岛。
    pub(in crate::editing::session) fn math_text_enter_math(
        &mut self,
        initial_source: &str,
    ) -> Result<Effect, EditError> {
        let body = caret_math::input_body_range(initial_source, caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        self.math_text_enter_math_at(initial_source, body.len())
    }

    /// 将编辑焦点放在初始公式源码正文中的指定 UTF-8 字节位置。
    pub(in crate::editing::session) fn math_text_enter_math_at(
        &mut self,
        initial_source: &str,
        cursor_in_body: usize,
    ) -> Result<Effect, EditError> {
        let before_document = self.document.clone();
        let before_edit = self.math_edit.clone();
        let result = self.math_text_enter_math_at_transaction(initial_source, cursor_in_body);
        if result.is_err() {
            self.document = before_document;
            self.math_edit = before_edit;
        }
        result
    }

    fn math_text_enter_math_at_transaction(
        &mut self,
        initial_source: &str,
        cursor_in_body: usize,
    ) -> Result<Effect, EditError> {
        let body = caret_math::input_body_range(initial_source, caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        if cursor_in_body > body.len()
            || !initial_source.is_char_boundary(body.start + cursor_in_body)
        {
            return Err(EditError::InvalidPosition);
        }
        let initial_source = initial_source.get(body).ok_or(EditError::InvalidPosition)?;
        let split_math_contents =
            match caret_math::parse_projection(initial_source, caret_math::MathInputKind::Inline)
                .map_err(|error| EditError::InvalidMath(error.to_string()))?
                .into_parts()
                .0
            {
                caret_math::ProjectedMath::Tree(tree) => tree.root.content,
                caret_math::ProjectedMath::Align(_) => return Err(EditError::InvalidPosition),
            };
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text { path, anchor, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        if anchor.path != *path {
            return Err(EditError::UnsupportedInlineFormat);
        }
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let text = path.resolve(tree).ok_or(EditError::InvalidPosition)?;
        let range = anchor.offset.min(edit.cursor)..anchor.offset.max(edit.cursor);
        let display_len = text
            .runs
            .iter()
            .try_fold(0usize, |length, run| length.checked_add(run.text.len()))
            .ok_or(EditError::InvalidPosition)?;
        if range.end > display_len
            || !super::math_text_offsets_are_boundaries(tree, path, &[range.start, range.end])
        {
            return Err(EditError::InvalidPosition);
        }
        let host = math_position_host(self, &edit, path.align_cell())?;
        let insertion = MathTreePosition::from_text_flow_boundary(
            tree,
            host.clone(),
            path.steps(),
            path.align_cell(),
            super::tree_position::MathTextFlowBoundary::Start,
            MathPositionAffinity::Downstream,
        )
        .map_err(|_| EditError::InvalidPosition)?;
        let insertion_index = insertion.offset;
        let mut target_tree = tree.clone();
        let replacement = if initial_source.is_empty() { "$$" } else { "" };
        apply_math_text_split_tree_edit(
            &mut target_tree,
            path,
            range.clone(),
            replacement,
            &split_math_contents,
        )?;
        target_tree
            .validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;

        // 拆分始终保留前置 TextFlow，即使其内容为空。
        let inserted_start = insertion_index
            .checked_add(1)
            .ok_or(EditError::InvalidPosition)?;
        let inserted_len = split_math_contents
            .len()
            .checked_add(usize::from(
                split_math_contents.is_empty() && initial_source.is_empty(),
            ))
            .ok_or(EditError::InvalidPosition)?;
        let inserted_end = inserted_start
            .checked_add(inserted_len)
            .ok_or(EditError::InvalidPosition)?;
        let (position_path, position_offset) =
            first_placeholder_position(&target_tree, &insertion.path, inserted_start..inserted_end)
                .unwrap_or_else(|| (insertion.path.clone(), inserted_end));
        let position = MathTreePosition {
            host,
            path: position_path.clone(),
            offset: position_offset,
            affinity: MathPositionAffinity::Downstream,
        };
        position
            .resolve(&target_tree)
            .map_err(|_| EditError::InvalidPosition)?;
        let align_cell = path.align_cell();
        self.commit_math_text_tree(
            edit,
            target_tree,
            MathTextTreeCommit {
                branch: MathEditBranch::Tree {
                    path: position_path,
                    anchor: position.clone(),
                },
                align_cell,
                cursor: position_offset,
                merge_group: None,
                pending_style: PendingStylePolicy::Clear,
                selection_changed: true,
            },
        )
    }

    /// 在当前 TextFlow 上直接提交树事务，不经源码编码或重新解析。
    pub(in crate::editing::session) fn math_text_insert(
        &mut self,
        inserted: &str,
    ) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text { path, anchor, .. } = edit.branch.clone() else {
            return Err(EditError::InvalidPosition);
        };
        if inserted.is_empty() && anchor.path == path && anchor.offset == edit.cursor {
            return Ok(Effect::default());
        }
        if inserted
            .chars()
            .any(|character| matches!(character, '\n' | '\r'))
        {
            return Err(EditError::UnsupportedNewline);
        }
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let (island_before, range) = math_text_selection_range(tree, &path, &anchor, edit.cursor)
            .ok_or(EditError::InvalidPosition)?;
        let focus_position =
            MathTextPosition::new(path.clone(), edit.cursor, MathPositionAffinity::Downstream);
        let anchor_global = island_before
            .global_offset(&anchor)
            .ok_or(EditError::InvalidPosition)?;
        let focus_global = island_before
            .global_offset(&focus_position)
            .ok_or(EditError::InvalidPosition)?;
        let insertion_position = if anchor_global <= focus_global {
            anchor
        } else {
            focus_position
        };
        let insertion_global = range.start;
        let requested_global_cursor = insertion_global
            .checked_add(inserted.len())
            .ok_or(EditError::InvalidPosition)?;
        let mut target_tree = tree.clone();
        for (candidate, flow_range) in island_before.paths.iter().zip(&island_before.ranges) {
            let overlap_start = range.start.max(flow_range.start);
            let overlap_end = range.end.min(flow_range.end);
            let candidate_is_insertion = candidate == &insertion_position.path;
            if overlap_start > overlap_end
                || (overlap_start == overlap_end
                    && !(candidate_is_insertion && !inserted.is_empty()))
            {
                continue;
            }
            let local_start = overlap_start - flow_range.start;
            let local_end = overlap_end - flow_range.start;
            let local_range = local_start..local_end;
            let text = candidate
                .resolve_structurally(&target_tree)
                .cloned()
                .ok_or(EditError::InvalidPosition)?;
            let source_map = edit.source_projection.as_deref().and_then(|projection| {
                candidate
                    .resolve_in_projection(tree, projection)
                    .map(|(_, source_map)| source_map)
            });
            validate_text_flow_edit(
                &text,
                source_map,
                local_range.clone(),
                candidate_is_insertion && !inserted.is_empty(),
            )?;
            let mut updated = text;
            replace_text_flow_range(
                &mut updated,
                local_range,
                if candidate_is_insertion { inserted } else { "" },
            )?;
            candidate
                .resolve_structurally_mut(&mut target_tree)
                .ok_or(EditError::InvalidPosition)?
                .clone_from(&updated);
        }

        // 区域指示符配对与组合字素会跨 TextFlow 边界变化；插入光标和待格式范围均按文本岛计算。
        let island = super::path::contiguous_text_flow_text(&target_tree, &insertion_position.path)
            .ok_or(EditError::InvalidPosition)?;
        let global_insert_start = insertion_global;
        let global_insert_end = requested_global_cursor;
        let format_global_start = snap_text_cursor(&island.display, global_insert_start, false)
            .ok_or(EditError::InvalidPosition)?;
        let format_global_end = snap_text_cursor(&island.display, global_insert_end, true)
            .ok_or(EditError::InvalidPosition)?;
        let format_range = format_global_start..format_global_end;
        let pending_properties = self
            .pending_inline_style
            .active_run_properties()
            .collect::<Vec<_>>();
        let affected_ranges = island
            .paths
            .iter()
            .zip(&island.ranges)
            .filter_map(|(candidate, flow_range)| {
                let start = format_range.start.max(flow_range.start);
                let end = format_range.end.min(flow_range.end);
                (start < end).then(|| (candidate.clone(), flow_range.clone(), start..end))
            })
            .collect::<Vec<_>>();
        for (candidate, flow_range, global_range) in affected_ranges.iter().rev() {
            let local_range =
                global_range.start - flow_range.start..global_range.end - flow_range.start;
            let text = candidate
                .resolve_structurally(&target_tree)
                .cloned()
                .ok_or(EditError::InvalidPosition)?;
            let source_map = edit.source_projection.as_deref().and_then(|projection| {
                candidate
                    .resolve_in_projection(tree, projection)
                    .map(|(_, source_map)| source_map)
            });
            validate_text_flow_edit(&text, source_map, local_range.clone(), true)?;
            let mut text = text;
            super::text_run_property::apply_pending_run_properties(
                &mut text,
                local_range.start,
                local_range.end - local_range.start,
                &pending_properties,
            )?;
            candidate
                .resolve_structurally_mut(&mut target_tree)
                .ok_or(EditError::InvalidPosition)?
                .clone_from(&text);
        }
        let mut tracking_path = insertion_position.path.clone();
        let mut preferred_cursor_path = insertion_position.path.clone();
        for (kind, enabled) in self
            .pending_inline_style
            .active_formats()
            .collect::<Vec<_>>()
        {
            for (_, _, global_range) in &affected_ranges {
                let island = super::path::contiguous_text_flow_text(&target_tree, &tracking_path)
                    .ok_or(EditError::InvalidPosition)?;
                let (candidate, local_start) = island
                    .path_at_offset(global_range.start, true)
                    .ok_or(EditError::InvalidPosition)?;
                let flow_range = island
                    .paths
                    .iter()
                    .position(|path| path.steps() == candidate.steps())
                    .and_then(|index| island.ranges.get(index))
                    .ok_or(EditError::InvalidPosition)?;
                let local_range =
                    local_start..global_range.end.min(flow_range.end) - flow_range.start;
                if super::text_inline_environment::inherited_environment_value(
                    &target_tree,
                    &candidate,
                    kind,
                ) == Some(enabled)
                {
                    if global_range.start <= requested_global_cursor
                        && requested_global_cursor <= global_range.end
                    {
                        preferred_cursor_path = candidate;
                    }
                    continue;
                }
                tracking_path = apply_math_text_environment_tree_range(
                    &mut target_tree,
                    &candidate,
                    local_range,
                    kind,
                    enabled,
                )?;
                if global_range.start <= requested_global_cursor
                    && requested_global_cursor <= global_range.end
                {
                    preferred_cursor_path = tracking_path.clone();
                }
            }
        }
        if let Some(family) = self.pending_inline_style.active_font_family() {
            for (_, _, global_range) in &affected_ranges {
                let island = super::path::contiguous_text_flow_text(&target_tree, &tracking_path)
                    .ok_or(EditError::InvalidPosition)?;
                let (candidate, local_start) = island
                    .path_at_offset(global_range.start, true)
                    .ok_or(EditError::InvalidPosition)?;
                let flow_range = island
                    .paths
                    .iter()
                    .position(|path| path.steps() == candidate.steps())
                    .and_then(|index| island.ranges.get(index))
                    .ok_or(EditError::InvalidPosition)?;
                let local_range =
                    local_start..global_range.end.min(flow_range.end) - flow_range.start;
                if super::text_inline_environment::inherited_math_text_font_family(
                    &target_tree,
                    &candidate,
                )
                .is_some_and(|inherited| inherited.as_deref() == Some(family))
                {
                    if global_range.start <= requested_global_cursor
                        && requested_global_cursor <= global_range.end
                    {
                        preferred_cursor_path = candidate;
                    }
                    continue;
                }
                tracking_path = apply_math_text_font_family_tree_range(
                    &mut target_tree,
                    &candidate,
                    local_range,
                    family,
                )?;
                if global_range.start <= requested_global_cursor
                    && requested_global_cursor <= global_range.end
                {
                    preferred_cursor_path = tracking_path.clone();
                }
            }
        }
        let island = super::path::contiguous_text_flow_text(&target_tree, &tracking_path)
            .ok_or(EditError::InvalidPosition)?;
        let global_cursor = snap_text_cursor(&island.display, requested_global_cursor, true)
            .ok_or(EditError::InvalidPosition)?;
        let preferred_cursor = island
            .paths
            .iter()
            .position(|candidate| candidate.steps() == preferred_cursor_path.steps())
            .and_then(|index| {
                let range = island.ranges.get(index)?;
                (range.start <= global_cursor && global_cursor <= range.end).then(|| {
                    (
                        island.paths[index].clone(),
                        global_cursor.checked_sub(range.start),
                    )
                })
            })
            .and_then(|(path, cursor)| cursor.map(|cursor| (path, cursor)));
        let (next_path, local_cursor) = preferred_cursor
            .or_else(|| island.path_at_offset(global_cursor, true))
            .ok_or(EditError::InvalidPosition)?;
        let next_position = MathTextPosition::new(
            next_path.clone(),
            local_cursor,
            MathPositionAffinity::Downstream,
        );
        target_tree
            .validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        self.commit_math_text_tree(
            edit,
            target_tree,
            MathTextTreeCommit {
                branch: MathEditBranch::Text {
                    path: next_path.clone(),
                    anchor: next_position,
                    focus_affinity: MathPositionAffinity::Downstream,
                    anchor_visual_affinity: crate::VisualAffinity::Downstream,
                    visual_affinity: crate::VisualAffinity::Downstream,
                },
                align_cell: next_path.align_cell(),
                cursor: local_cursor,
                merge_group: Some(EditGroup::InsertText),
                pending_style: PendingStylePolicy::Preserve,
                selection_changed: true,
            },
        )
    }

    /// 跨相邻 TextFlow 删除时，按同一文本岛的结构锚点重定位光标。
    pub(super) fn math_text_delete(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text { path, anchor, .. } = edit.branch.clone() else {
            return Err(EditError::InvalidPosition);
        };
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let (island, selected_range) = math_text_selection_range(tree, &path, &anchor, edit.cursor)
            .ok_or(EditError::InvalidPosition)?;
        let cursor = island
            .global_offset(&MathTextPosition::new(
                path.clone(),
                edit.cursor,
                MathPositionAffinity::Downstream,
            ))
            .ok_or(EditError::InvalidPosition)?;
        let selected = if !selected_range.is_empty() {
            Some(selected_range)
        } else {
            let cursor = GraphemeMap::snap_in(
                &island.display,
                cursor,
                if direction == Direction::Backward {
                    Affinity::Downstream
                } else {
                    Affinity::Upstream
                },
            )
            .ok_or(EditError::InvalidPosition)?;
            match direction {
                Direction::Backward => {
                    previous_grapheme_start(&island.display, cursor).map(|start| start..cursor)
                }
                Direction::Forward => {
                    next_grapheme_end(&island.display, cursor).map(|end| cursor..end)
                }
            }
        };
        let Some(range) = selected else {
            return Ok(Effect::default());
        };
        let mut target_tree = tree.clone();
        for (candidate, flow_range) in island.paths.iter().zip(&island.ranges) {
            let start = range.start.max(flow_range.start);
            let end = range.end.min(flow_range.end);
            if start >= end {
                continue;
            }
            let local_range = start - flow_range.start..end - flow_range.start;
            let text = candidate
                .resolve_structurally(&target_tree)
                .cloned()
                .ok_or(EditError::InvalidPosition)?;
            let source_map = edit.source_projection.as_deref().and_then(|projection| {
                candidate
                    .resolve_in_projection(tree, projection)
                    .map(|(_, source_map)| source_map)
            });
            validate_text_flow_edit(&text, source_map, local_range.clone(), false)?;
            let mut updated = text;
            replace_text_flow_range(&mut updated, local_range, "")?;
            candidate
                .resolve_structurally_mut(&mut target_tree)
                .ok_or(EditError::InvalidPosition)?
                .clone_from(&updated);
        }
        let mut updated_display = String::new();
        let mut updated_flow_ranges = Vec::with_capacity(island.paths.len());
        for candidate in &island.paths {
            let text = candidate
                .resolve_structurally(&target_tree)
                .ok_or(EditError::InvalidPosition)?;
            let start = updated_display.len();
            for run in &text.runs {
                updated_display.push_str(&run.text);
            }
            updated_flow_ranges.push(start..updated_display.len());
        }
        let caret = GraphemeMap::snap_in(&updated_display, range.start, Affinity::Upstream)
            .ok_or(EditError::InvalidPosition)?;
        let focus_index = if direction == Direction::Backward {
            updated_flow_ranges
                .iter()
                .rposition(|flow_range| flow_range.start <= caret && caret <= flow_range.end)
        } else {
            updated_flow_ranges
                .iter()
                .position(|flow_range| flow_range.start <= caret && caret <= flow_range.end)
        }
        .ok_or(EditError::InvalidPosition)?;
        let edit_path = &island.paths[focus_index];
        let next_cursor = caret
            .checked_sub(updated_flow_ranges[focus_index].start)
            .ok_or(EditError::InvalidPosition)?;
        let next_path = edit_path.clone();
        let next_position = MathTextPosition::new(
            next_path.clone(),
            next_cursor,
            MathPositionAffinity::Downstream,
        );
        target_tree
            .validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        self.commit_math_text_tree(
            edit,
            target_tree,
            MathTextTreeCommit {
                branch: MathEditBranch::Text {
                    path: next_path.clone(),
                    anchor: next_position,
                    focus_affinity: MathPositionAffinity::Downstream,
                    anchor_visual_affinity: crate::VisualAffinity::Downstream,
                    visual_affinity: crate::VisualAffinity::Downstream,
                },
                align_cell: edit_path.align_cell(),
                cursor: next_cursor,
                merge_group: Some(match direction {
                    Direction::Backward => EditGroup::DeleteBackward,
                    Direction::Forward => EditGroup::DeleteForward,
                }),
                pending_style: PendingStylePolicy::Clear,
                selection_changed: true,
            },
        )
    }
}

fn math_position_host(
    session: &Session,
    edit: &MathEdit,
    align_cell: Option<(usize, usize)>,
) -> Result<MathPositionHost, EditError> {
    match (&edit.target, align_cell) {
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
            let Block::Align(align) = super::find_block_by_id(&session.document.body, block_id)
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

fn first_placeholder_position(
    tree: &caret_doc::MathTree,
    path: &MathSequencePath,
    range: Range<usize>,
) -> Option<(MathSequencePath, usize)> {
    let mut contents = tree.root.content.as_slice();
    for step in path.steps() {
        contents = match *step {
            MathSequenceStep::Operand {
                content_index,
                operand_index,
            } => match contents.get(content_index)? {
                MathContent::Math(node) => &node.children.get(operand_index)?.content,
                _ => return None,
            },
            MathSequenceStep::EnvironmentChildren { content_index } => {
                match contents.get(content_index)? {
                    MathContent::Environment(environment) => &environment.children,
                    _ => return None,
                }
            }
        };
    }
    for content_index in range {
        match contents.get(content_index)? {
            MathContent::Math(node) if node.kind == MathNodeKind::Placeholder => {
                return Some((path.clone(), content_index));
            }
            MathContent::Math(node) => {
                for (operand_index, flow) in node.children.iter().enumerate() {
                    let mut steps = path.steps().to_vec();
                    steps.push(MathSequenceStep::Operand {
                        content_index,
                        operand_index,
                    });
                    if let Some(position) = first_placeholder_position(
                        tree,
                        &MathSequencePath::new(steps),
                        0..flow.content.len(),
                    ) {
                        return Some(position);
                    }
                }
            }
            MathContent::Environment(environment) => {
                let mut steps = path.steps().to_vec();
                steps.push(MathSequenceStep::EnvironmentChildren { content_index });
                if let Some(position) = first_placeholder_position(
                    tree,
                    &MathSequencePath::new(steps),
                    0..environment.children.len(),
                ) {
                    return Some(position);
                }
            }
            MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
        }
    }
    None
}
