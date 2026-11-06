//! 管理 IME 组字的更新、幂等提交与取消，提交正文仍复用文本编辑事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::paragraph_ops::{compare_positions, paragraph_at_location};
use super::super::text_map::{utf8_offset_to_utf16, utf16_offset_to_utf8};
use super::super::{
    Affinity, Command, EditError, Effect, InputComposition, Pos, PositionMap, Selection,
};
use super::Session;
use super::StructureSelection;
use super::formatting::PendingInlineStyle;
use super::history::{
    self, DocumentPatch, MathEditPosition, SessionHistoryEdit, SessionHistoryState,
};
use super::math_edit::{MathEditBranch, MathEditTarget, MathPositionAffinity, MathTextPosition};
use std::mem;

/// 供布局验证后安装的公式外组字候选。
#[derive(Debug, Clone)]
pub struct CompositionUpdateCandidate {
    candidate: Session,
    base: CompositionCandidateBase,
}

/// 输入法预编辑范围的宿主；正文范围使用段落位置，数学文本范围使用树端点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompositionRange {
    Paragraph(Selection),
    MathText {
        anchor: MathTextPosition,
        focus: MathTextPosition,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompositionCandidateBase {
    session_id: caret_doc::NodeId,
    document_generation: u64,
    selection: Selection,
    selection_visual_affinity: caret_layout::VisualAffinity,
    composition: Option<InputComposition>,
    pending_inline_style: PendingInlineStyle,
    command_line: Option<super::super::CommandLine>,
    math_edit: Option<super::math_edit::MathEdit>,
    block_environment_exit: Option<(caret_doc::NodeId, super::super::Direction)>,
    selected_block_environment: Option<caret_doc::NodeId>,
    selected_block_environments: Vec<caret_doc::NodeId>,
    structure_selection: Option<StructureSelection>,
    last_committed_composition: Option<u64>,
    history_state_id: u64,
    next_history_state_id: u64,
    history_merge_epoch: u64,
}

impl CompositionCandidateBase {
    fn from_session(session: &Session) -> Self {
        Self {
            session_id: session.session_id.clone(),
            document_generation: session.document_generation,
            selection: session.selection.clone(),
            selection_visual_affinity: session.selection_visual_affinity,
            composition: session.composition.clone(),
            pending_inline_style: session.pending_inline_style.clone(),
            command_line: session.command_line.clone(),
            math_edit: session.math_edit.clone(),
            block_environment_exit: session.block_environment_exit.clone(),
            selected_block_environment: session.selected_block_environment.clone(),
            selected_block_environments: session.selected_block_environments.clone(),
            structure_selection: session.structure_selection.clone(),
            last_committed_composition: session.last_committed_composition,
            history_state_id: session.history_state_id,
            next_history_state_id: session.next_history_state_id,
            history_merge_epoch: session.history_merge_epoch,
        }
    }

    fn matches(&self, session: &Session) -> bool {
        self == &Self::from_session(session)
    }
}

fn math_composition_document_patch(
    before: &Session,
    after: &Session,
    edit: &super::math_edit::MathEdit,
) -> Result<DocumentPatch, EditError> {
    match &edit.target {
        MathEditTarget::Inline { paragraph_id, .. } => {
            let merge_epoch = before.history_merge_epoch;
            let before = history::paragraph_states_for_ids(
                &before.document,
                &before.paragraph_index,
                [paragraph_id.clone()],
            );
            let after = history::paragraph_states_for_ids(
                &after.document,
                &after.paragraph_index,
                [paragraph_id.clone()],
            );
            if before.is_empty() || after.is_empty() {
                return Err(EditError::InvalidPosition);
            }
            Ok(DocumentPatch::Paragraphs {
                before,
                after,
                merge_group: None,
                merge_epoch,
            })
        }
        MathEditTarget::Block { block_id, .. } => {
            let (parent_environment_ids, sibling_index, before) =
                find_composition_math_block(&before.document.body, block_id, &[])
                    .ok_or(EditError::InvalidPosition)?;
            let (_, _, after) = find_composition_math_block(&after.document.body, block_id, &[])
                .ok_or(EditError::InvalidPosition)?;
            Ok(DocumentPatch::BlockRange {
                parent_environment_ids,
                sibling_index,
                before: vec![before],
                after: vec![after],
            })
        }
    }
}

fn find_composition_math_block(
    blocks: &[caret_doc::Block],
    target_id: &caret_doc::NodeId,
    parent_environment_ids: &[caret_doc::NodeId],
) -> Option<(Vec<caret_doc::NodeId>, usize, caret_doc::Block)> {
    for (index, block) in blocks.iter().enumerate() {
        let matches_target = match block {
            caret_doc::Block::Equation(equation) => &equation.id == target_id,
            caret_doc::Block::Align(align) => &align.id == target_id,
            _ => false,
        };
        if matches_target {
            return Some((parent_environment_ids.to_vec(), index, block.clone()));
        }
        if let caret_doc::Block::Environment(environment) = block {
            let mut child_parents = parent_environment_ids.to_vec();
            child_parents.push(environment.id.clone());
            if let Some(found) =
                find_composition_math_block(&environment.children, target_id, &child_parents)
            {
                return Some(found);
            }
        }
    }
    None
}

impl Session {
    fn math_text_edit_is_active(&self) -> bool {
        self.math_edit()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Text { .. }))
    }

    fn math_text_composition_preview(
        &self,
        id: u64,
        preedit: &str,
        cursor_position_utf16: usize,
    ) -> Result<(Self, CompositionRange), EditError> {
        let (_, anchor_offset, focus_offset) = self
            .math_text_selection_island_offsets()
            .ok_or(EditError::InvalidPosition)?;
        let replacement_start = anchor_offset.min(focus_offset);
        let mut preview = self.transaction_clone();
        preview.run(Command::CommitComposition {
            id,
            text: preedit.to_owned(),
        })?;
        let (island, _, preview_focus) = preview
            .math_text_selection_island_offsets()
            .ok_or(EditError::InvalidPosition)?;
        let preedit_cursor = utf16_offset_to_utf8(
            preedit,
            cursor_position_utf16.min(preedit.encode_utf16().count()),
        )?;
        let cursor_offset = replacement_start
            .checked_add(preedit_cursor)
            .ok_or(EditError::InvalidPosition)?
            .min(preview_focus);
        let display = island.display();
        let cursor_offset = super::super::grapheme::GraphemeMap::snap_in(
            display,
            cursor_offset,
            Affinity::Downstream,
        )
        .ok_or(EditError::InvalidPosition)?;
        let range_start = super::super::grapheme::GraphemeMap::snap_in(
            display,
            replacement_start,
            Affinity::Upstream,
        )
        .ok_or(EditError::InvalidPosition)?;
        let range_end = super::super::grapheme::GraphemeMap::snap_in(
            display,
            replacement_start
                .checked_add(preedit.len())
                .ok_or(EditError::InvalidPosition)?,
            Affinity::Downstream,
        )
        .ok_or(EditError::InvalidPosition)?;
        let anchor = island
            .position_at_offset(range_start, MathPositionAffinity::Upstream)
            .ok_or(EditError::InvalidPosition)?;
        let focus = island
            .position_at_offset(range_end, MathPositionAffinity::Downstream)
            .ok_or(EditError::InvalidPosition)?;
        let cursor = island
            .position_at_offset(cursor_offset, MathPositionAffinity::Downstream)
            .ok_or(EditError::InvalidPosition)?;
        preview.set_math_text_selection_positions(cursor.clone(), cursor)?;
        Ok((preview, CompositionRange::MathText { anchor, focus }))
    }

    fn commit_math_text_composition_replacement(
        &mut self,
        active_composition_id: Option<u64>,
        replacement_composition_id: u64,
        replacement_start: i32,
        replacement_length: i32,
        text: &str,
    ) -> Result<Effect, EditError> {
        self.ensure_math_edit_revision_current()?;
        let original_session_id = self.session_id.clone();
        let original_generation = self.document_generation;
        let original_history_state_id = self.history_state_id;
        let original_next_history_state_id = self.next_history_state_id;
        let original_history_merge_epoch = self.history_merge_epoch;
        let original_math_edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let mut candidate = self.transaction_clone();
        let result = (|| {
            let mut effects = Vec::with_capacity(2);
            if let Some(id) = active_composition_id {
                candidate.run(Command::CancelComposition { id })?;
            }
            effects.push(candidate.math_text_insert("")?);
            candidate.set_math_text_composition_replacement_range(
                replacement_start,
                replacement_length,
            )?;
            candidate.run(Command::UpdateComposition {
                id: replacement_composition_id,
                preedit: String::new(),
            })?;
            effects.push(candidate.run(Command::CommitComposition {
                id: replacement_composition_id,
                text: text.to_owned(),
            })?);
            Ok(combine_effects(effects))
        })();
        let effect = result?;
        let patch = if effect.document_changed {
            Some(math_composition_document_patch(
                self,
                &candidate,
                &original_math_edit,
            )?)
        } else {
            None
        };
        let before_math_position = MathEditPosition::from_edit(&original_math_edit);
        let after_math_position = candidate
            .math_edit
            .as_ref()
            .map(MathEditPosition::from_edit);
        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let before_pending_style = self.pending_inline_style.clone();
        candidate.edit_history = mem::replace(&mut self.edit_history, history::history());
        candidate.history_estimated_bytes = self.history_estimated_bytes;
        candidate.history_state_id = original_history_state_id;
        candidate.next_history_state_id = original_next_history_state_id;
        candidate.history_merge_epoch = original_history_merge_epoch;
        if let Some(patch) = patch {
            candidate.advance_history_merge_epoch();
            let after_state_id = candidate.allocate_history_state_id();
            let edit = SessionHistoryEdit::new(
                patch,
                SessionHistoryState::new(
                    before_selection,
                    before_visual_affinity,
                    before_pending_style.clone(),
                    original_history_state_id,
                )
                .with_math_edit(Some(before_math_position)),
                SessionHistoryState::new(
                    candidate.selection.clone(),
                    candidate.selection_visual_affinity,
                    candidate.pending_inline_style.clone(),
                    after_state_id,
                )
                .with_math_edit(after_math_position),
            );
            candidate.record_history_edit(edit);
            candidate.history_state_id = after_state_id;
            candidate.session_id = original_session_id;
            candidate.document_generation = original_generation;
            candidate.advance_document_generation();
            candidate.advance_history_merge_epoch();
        } else {
            candidate.history_state_id = original_history_state_id;
            candidate.document_generation = original_generation;
        }
        candidate.sync_math_edit_revision();
        *self = candidate;
        Ok(effect)
    }

    fn set_math_text_composition_replacement_range(
        &mut self,
        replacement_start: i32,
        replacement_length: i32,
    ) -> Result<(), EditError> {
        if replacement_start == 0 && replacement_length == 0 {
            return Ok(());
        }
        let (island, anchor_offset, focus_offset) = self
            .math_text_selection_island_offsets()
            .ok_or(EditError::InvalidPosition)?;
        let text = island.display();
        if anchor_offset != focus_offset {
            return Err(EditError::InvalidPosition);
        }
        let replacement_range =
            utf16_replacement_range(text, focus_offset, replacement_start, replacement_length)?;
        let anchor = island
            .position_at_offset(replacement_range.start, MathPositionAffinity::Upstream)
            .ok_or(EditError::InvalidPosition)?;
        let focus = island
            .position_at_offset(replacement_range.end, MathPositionAffinity::Downstream)
            .ok_or(EditError::InvalidPosition)?;
        self.set_math_text_selection_positions(anchor, focus)?;
        Ok(())
    }

    /// 按 Qt UTF-16 相对范围原子提交输入法替换，支持先取消已有预编辑再应用新范围。
    ///
    /// `active_composition_id` 指向待替换的预编辑；普通段落分别使用两个新 ID 删除原选区并提交替换文本，
    /// 数学 TextFlow 在候选会话中应用两段树编辑，再以一条历史记录原子安装最终结果。
    /// 若范围无效或事务失败，保留当前会话状态。
    pub fn commit_composition_replacement(
        &mut self,
        active_composition_id: Option<u64>,
        selection_removal_composition_id: u64,
        replacement_composition_id: u64,
        replacement_start: i32,
        replacement_length: i32,
        text: &str,
    ) -> Result<Effect, EditError> {
        if self.math_edit.is_some() {
            if !self.math_text_edit_is_active() {
                return Err(EditError::CompositionPending);
            }
            return self.commit_math_text_composition_replacement(
                active_composition_id,
                replacement_composition_id,
                replacement_start,
                replacement_length,
                text,
            );
        }
        let original_session_id = self.session_id.clone();
        let original_generation = self.document_generation;
        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let mut affected_ids =
            history::before_selection_ids(&self.paragraph_index, &self.selection);
        affected_ids.push(self.selection.focus.paragraph_id.clone());
        let before_paragraph_states =
            history::paragraph_states_for_ids(&self.document, &self.paragraph_index, affected_ids);
        let mut candidate = self.transaction_clone();
        let original_history = mem::replace(&mut self.edit_history, history::history());
        let original_history_bytes = self.history_estimated_bytes;
        let result = (|| {
            if let Some(id) = active_composition_id {
                candidate.run(Command::CancelComposition { id })?;
            }
            let mut effects = Vec::with_capacity(2);
            let selection_is_nonempty = compare_positions(
                &candidate.paragraph_index,
                &candidate.selection.anchor,
                &candidate.selection.focus,
            )?
            .is_ne();
            if selection_is_nonempty {
                candidate.run(Command::UpdateComposition {
                    id: selection_removal_composition_id,
                    preedit: String::new(),
                })?;
                effects.push(candidate.run(Command::CommitComposition {
                    id: selection_removal_composition_id,
                    text: String::new(),
                })?);
            }
            candidate.set_composition_replacement_range(replacement_start, replacement_length)?;
            candidate.run(Command::UpdateComposition {
                id: replacement_composition_id,
                preedit: String::new(),
            })?;
            effects.push(candidate.run(Command::CommitComposition {
                id: replacement_composition_id,
                text: text.to_owned(),
            })?);
            Ok(combine_effects(effects))
        })();
        let effect = match result {
            Ok(effect) => effect,
            Err(error) => {
                self.edit_history = original_history;
                return Err(error);
            }
        };
        candidate.edit_history = original_history;
        candidate.history_estimated_bytes = original_history_bytes;
        if effect.document_changed {
            candidate.document_generation = self.document_generation;
            candidate.next_history_state_id = self.next_history_state_id;
            candidate.history_state_id = self.history_state_id;
            let after_state_id = candidate.allocate_history_state_id();
            let mut changed_ids = before_paragraph_states
                .iter()
                .map(|state| state.id.clone())
                .collect::<Vec<_>>();
            if let Some(map) = &effect.position_map {
                history::state_ids_from_position_map(map, &mut changed_ids);
            }
            changed_ids.push(candidate.selection.focus.paragraph_id.clone());
            let after_paragraph_states = history::paragraph_states_for_ids(
                &candidate.document,
                &candidate.paragraph_index,
                changed_ids,
            );
            if before_paragraph_states.is_empty() || after_paragraph_states.is_empty() {
                self.edit_history = mem::replace(&mut candidate.edit_history, history::history());
                return Err(EditError::InvalidPosition);
            }
            let edit = SessionHistoryEdit::new(
                DocumentPatch::Paragraphs {
                    before: before_paragraph_states,
                    after: after_paragraph_states,
                    merge_group: None,
                    merge_epoch: candidate.history_merge_epoch,
                },
                SessionHistoryState::new(
                    before_selection,
                    before_visual_affinity,
                    self.pending_inline_style.clone(),
                    before_state_id,
                ),
                SessionHistoryState::new(
                    candidate.selection.clone(),
                    candidate.selection_visual_affinity,
                    candidate.pending_inline_style.clone(),
                    after_state_id,
                ),
            );
            candidate.record_history_edit(edit);
            candidate.session_id = original_session_id;
            candidate.document_generation = original_generation;
            candidate.advance_document_generation();
        }
        *self = candidate;
        Ok(effect)
    }

    /// 构造待安装的 composition 与对应显示副本，预览校验失败时调用方可保留原状态。
    pub fn composition_update_candidate(
        &self,
        id: u64,
        preedit: String,
        replacement_range: Option<(i32, i32)>,
        cursor_position_utf16: usize,
    ) -> Result<(CompositionUpdateCandidate, Self, CompositionRange), EditError> {
        if self.math_edit.is_some() && !self.math_text_edit_is_active() {
            return Err(EditError::CompositionPending);
        }
        let base = CompositionCandidateBase::from_session(self);
        let mut candidate = self.transaction_clone();
        if let Some((replacement_start, replacement_length)) = replacement_range {
            candidate.set_composition_replacement_range(replacement_start, replacement_length)?;
        }
        candidate.run(Command::UpdateComposition { id, preedit })?;
        let (preview, range) = candidate.composition_preview_with_range(cursor_position_utf16)?;
        Ok((
            CompositionUpdateCandidate { candidate, base },
            preview,
            range,
        ))
    }

    /// 布局验证成功后安装仍对应当前交互状态的候选，并保留原会话历史。
    /// 候选生成后会话或编辑状态改变时返回 InvalidPosition，且不修改会话。
    pub fn install_composition_candidate(
        &mut self,
        candidate: CompositionUpdateCandidate,
    ) -> Result<(), EditError> {
        if !candidate.base.matches(self) {
            return Err(EditError::InvalidPosition);
        }
        let mut candidate = candidate.candidate;
        candidate.edit_history = mem::replace(&mut self.edit_history, history::history());
        candidate.history_estimated_bytes = self.history_estimated_bytes;
        *self = candidate;
        Ok(())
    }

    fn transaction_clone(&self) -> Self {
        Self {
            document: self.document.clone(),
            selection: self.selection.clone(),
            selection_visual_affinity: self.selection_visual_affinity,
            session_id: self.session_id.clone(),
            document_generation: self.document_generation,
            composition: self.composition.clone(),
            pending_inline_style: self.pending_inline_style.clone(),
            command_line: self.command_line.clone(),
            math_edit: self.math_edit.clone(),
            block_environment_exit: self.block_environment_exit.clone(),
            selected_block_environment: self.selected_block_environment.clone(),
            selected_block_environments: self.selected_block_environments.clone(),
            structure_selection: self.structure_selection.clone(),
            last_committed_composition: self.last_committed_composition,
            text_map_cache: self.text_map_cache.clone(),
            paragraph_index: self.paragraph_index.clone(),
            structure_index: self.structure_index.clone(),
            edit_history: history::history(),
            history_state_id: self.history_state_id,
            next_history_state_id: self.next_history_state_id,
            history_estimated_bytes: self.history_estimated_bytes,
            history_merge_epoch: self.history_merge_epoch,
        }
    }

    /// 生成用于屏幕显示的组字会话副本，不修改当前文档或保存代际。
    pub fn composition_preview(&self, cursor_position_utf16: usize) -> Result<Self, EditError> {
        self.composition_preview_with_range(cursor_position_utf16)
            .map(|(preview, _)| preview)
    }

    /// 生成组字显示副本，并返回只覆盖预编辑文本的逻辑范围。
    pub fn composition_preview_with_range(
        &self,
        cursor_position_utf16: usize,
    ) -> Result<(Self, CompositionRange), EditError> {
        if self.math_edit.is_some() && !self.math_text_edit_is_active() {
            return Err(EditError::CompositionPending);
        }
        let composition = self
            .composition
            .as_ref()
            .ok_or(EditError::InvalidCompositionId)?;
        let preedit = composition.preedit.clone();
        if self.math_text_edit_is_active() {
            return self.math_text_composition_preview(
                composition.id,
                &preedit,
                cursor_position_utf16,
            );
        }
        let mut preview = self.transaction_clone();
        preview.run(Command::CommitComposition {
            id: composition.id,
            text: preedit.clone(),
        })?;
        if self.block_environment_exit.is_some() {
            return self.composition_preview_at_environment_exit(
                preview,
                &preedit,
                cursor_position_utf16,
            );
        }
        let mut composition_start = if compare_positions(
            &self.paragraph_index,
            &self.selection.anchor,
            &self.selection.focus,
        )?
        .is_gt()
        {
            self.selection.focus.clone()
        } else {
            self.selection.anchor.clone()
        };
        composition_start.affinity = Affinity::Upstream;
        let mapped_start = composition_start;
        let cursor_position_utf16 = cursor_position_utf16.min(preedit.encode_utf16().count());
        let relative_offset = utf16_offset_to_utf8(&preedit, cursor_position_utf16)?;
        let offset = mapped_start
            .offset
            .checked_add(relative_offset)
            .ok_or(EditError::InvalidPosition)?;
        let has_pending_inline_style = preview.pending_inline_style.is_active();
        let (position, start, end) = {
            let map = preview.text_map_for(&mapped_start)?;
            let position = Pos {
                offset: map.snap_to_grapheme_boundary(offset, Affinity::Downstream),
                affinity: if has_pending_inline_style && relative_offset > 0 {
                    Affinity::Upstream
                } else {
                    Affinity::Downstream
                },
                ..mapped_start.clone()
            };
            let start = map.snap_to_grapheme_boundary(mapped_start.offset, Affinity::Upstream);
            let end_offset = mapped_start
                .offset
                .checked_add(preedit.len())
                .ok_or(EditError::InvalidPosition)?;
            let end = map.snap_to_grapheme_boundary(end_offset, Affinity::Downstream);
            (position, start, end)
        };
        preview.set_selection_with_visual_affinity(
            Some(Selection {
                anchor: position.clone(),
                focus: position,
            }),
            self.selection_visual_affinity,
        )?;
        let range = Selection {
            anchor: Pos {
                offset: start,
                affinity: Affinity::Upstream,
                ..mapped_start.clone()
            },
            focus: Pos {
                offset: end,
                affinity: Affinity::Downstream,
                ..mapped_start
            },
        };
        Ok((preview, CompositionRange::Paragraph(range)))
    }

    /// 将块环境外的预编辑位置映射到新建的同级段落，而不是映射回环境内部段落。
    fn composition_preview_at_environment_exit(
        &self,
        mut preview: Self,
        preedit: &str,
        cursor_position_utf16: usize,
    ) -> Result<(Self, CompositionRange), EditError> {
        let normalized = preedit.replace("\r\n", "\n").replace('\r', "\n");
        let cursor_byte = utf16_offset_to_utf8(
            &normalized,
            cursor_position_utf16.min(normalized.encode_utf16().count()),
        )?;
        let line_start = normalized[..cursor_byte]
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        let local_offset = cursor_byte - line_start;

        // 提交后的焦点位于新建的最后一段；其余段紧邻它按文本行顺序排列。
        let last_id = &preview.selection.focus.paragraph_id;
        if self.paragraph_index.contains_key(last_id) {
            return Err(EditError::InvalidPosition);
        }
        let mut location = preview
            .paragraph_index
            .get(last_id)
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        let lines_after_cursor = normalized[line_start..]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        let sibling_index = location
            .block_path
            .last_mut()
            .ok_or(EditError::InvalidPosition)?;
        *sibling_index = sibling_index
            .checked_sub(lines_after_cursor)
            .ok_or(EditError::InvalidPosition)?;
        let paragraph_id = paragraph_at_location(&preview.document.body, &location)
            .ok_or(EditError::InvalidPosition)?
            .id
            .clone();
        if self.paragraph_index.contains_key(&paragraph_id) {
            return Err(EditError::InvalidPosition);
        }
        let map = preview.text_map_for(&Pos {
            story: location.story.clone(),
            paragraph_id: paragraph_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        })?;
        let start = map.snap_to_grapheme_boundary(0, Affinity::Upstream);
        let end = map.snap_to_grapheme_boundary(map.text.len(), Affinity::Downstream);
        let position = Pos {
            story: location.story.clone(),
            paragraph_id: paragraph_id.clone(),
            offset: map.snap_to_grapheme_boundary(local_offset, Affinity::Downstream),
            affinity: Affinity::Downstream,
        };
        preview.set_selection_with_visual_affinity(
            Some(Selection {
                anchor: position.clone(),
                focus: position,
            }),
            self.selection_visual_affinity,
        )?;
        let range = Selection {
            anchor: Pos {
                story: location.story.clone(),
                paragraph_id: paragraph_id.clone(),
                offset: start,
                affinity: Affinity::Upstream,
            },
            focus: Pos {
                story: location.story.clone(),
                paragraph_id,
                offset: end,
                affinity: Affinity::Downstream,
            },
        };
        Ok((preview, CompositionRange::Paragraph(range)))
    }

    /// 将 Qt 的 UTF-16 相对替换区间转换为同段落中按字素边界吸附的 Rust 选区。
    pub fn set_composition_replacement_range(
        &mut self,
        replacement_start: i32,
        replacement_length: i32,
    ) -> Result<(), EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if self.math_edit.is_some() {
            if !self.math_text_edit_is_active() {
                return Err(EditError::CompositionPending);
            }
            return self.set_math_text_composition_replacement_range(
                replacement_start,
                replacement_length,
            );
        }
        // 虚拟环境外位置没有相邻文本可供 IME 替换，保持外侧停靠点并将提交视为插入。
        if self.block_environment_exit.is_some() {
            return Ok(());
        }
        if replacement_start == 0 && replacement_length == 0 {
            return Ok(());
        }
        let focus = self.selection.focus.clone();
        let map = self.text_map_for(&focus)?;
        map.validate_pos(&focus)?;
        let cursor_units = i64::try_from(utf8_offset_to_utf16(&map.text, focus.offset)?)
            .map_err(|_| EditError::InvalidUtf16Offset)?;
        let start_units = cursor_units + i64::from(replacement_start);
        let end_units = start_units + i64::from(replacement_length);
        let text_units = i64::try_from(map.text.encode_utf16().count())
            .map_err(|_| EditError::InvalidUtf16Offset)?;
        if start_units < 0 || end_units < 0 || start_units > text_units || end_units > text_units {
            return Err(EditError::InvalidUtf16Offset);
        }
        let (start_units, end_units) = if start_units <= end_units {
            (start_units, end_units)
        } else {
            (end_units, start_units)
        };
        let start = map.snap_to_grapheme_boundary(
            utf16_offset_to_utf8(
                &map.text,
                usize::try_from(start_units).unwrap_or(usize::MAX),
            )?,
            Affinity::Upstream,
        );
        let end = map.snap_to_grapheme_boundary(
            utf16_offset_to_utf8(&map.text, usize::try_from(end_units).unwrap_or(usize::MAX))?,
            Affinity::Downstream,
        );
        let anchor = Pos {
            offset: start,
            affinity: Affinity::Upstream,
            ..focus.clone()
        };
        let focus = Pos {
            offset: end,
            affinity: Affinity::Downstream,
            ..focus
        };
        self.set_selection_with_visual_affinity(
            Some(Selection { anchor, focus }),
            self.selection_visual_affinity,
        )?;
        Ok(())
    }

    pub(super) fn update_composition(
        &mut self,
        id: u64,
        preedit: String,
    ) -> Result<Effect, EditError> {
        if self.last_committed_composition == Some(id) {
            return Err(EditError::InvalidCompositionId);
        }
        if self
            .composition
            .as_ref()
            .is_some_and(|current| current.id != id)
        {
            return Err(EditError::InvalidCompositionId);
        }
        let next = Some(InputComposition { id, preedit });
        let changed = self.composition != next;
        self.composition = next;
        Ok(Effect {
            composition_changed: changed,
            ..Effect::default()
        })
    }

    pub(super) fn commit_composition(&mut self, id: u64, text: &str) -> Result<Effect, EditError> {
        if self.last_committed_composition == Some(id) {
            return Ok(Effect::default());
        }
        if self
            .composition
            .as_ref()
            .is_none_or(|current| current.id != id)
        {
            return Err(EditError::InvalidCompositionId);
        }
        let prior = self.composition.take();
        let result = if self.math_text_edit_is_active() {
            self.math_text_insert(text)
        } else if self.block_environment_exit.is_some() {
            self.insert_text_at_environment_exit(text)
        } else {
            self.insert_text(text)
        };
        match result {
            Ok(mut effect) => {
                self.last_committed_composition = Some(id);
                effect.composition_changed = true;
                Ok(effect)
            }
            Err(error) => {
                self.composition = prior;
                Err(error)
            }
        }
    }

    pub(super) fn cancel_composition(&mut self, id: u64) -> Result<Effect, EditError> {
        if self
            .composition
            .as_ref()
            .is_none_or(|current| current.id != id)
        {
            return Err(EditError::InvalidCompositionId);
        }
        self.composition = None;
        Ok(Effect {
            composition_changed: true,
            ..Effect::default()
        })
    }
}

#[cfg(test)]
#[path = "composition/tests.rs"]
mod tests;

fn utf16_replacement_range(
    text: &str,
    cursor_offset: usize,
    replacement_start: i32,
    replacement_length: i32,
) -> Result<std::ops::Range<usize>, EditError> {
    if cursor_offset > text.len() || !text.is_char_boundary(cursor_offset) {
        return Err(EditError::InvalidPosition);
    }
    let cursor_units = i64::try_from(utf8_offset_to_utf16(text, cursor_offset)?)
        .map_err(|_| EditError::InvalidUtf16Offset)?;
    let start_units = cursor_units + i64::from(replacement_start);
    let end_units = start_units + i64::from(replacement_length);
    let text_units =
        i64::try_from(text.encode_utf16().count()).map_err(|_| EditError::InvalidUtf16Offset)?;
    if start_units < 0 || end_units < 0 || start_units > text_units || end_units > text_units {
        return Err(EditError::InvalidUtf16Offset);
    }
    let (start_units, end_units) = if start_units <= end_units {
        (start_units, end_units)
    } else {
        (end_units, start_units)
    };
    let start = utf16_offset_to_utf8(
        text,
        usize::try_from(start_units).map_err(|_| EditError::InvalidUtf16Offset)?,
    )?;
    let end = utf16_offset_to_utf8(
        text,
        usize::try_from(end_units).map_err(|_| EditError::InvalidUtf16Offset)?,
    )?;
    Ok(
        super::super::grapheme::GraphemeMap::snap_in(text, start, Affinity::Upstream)
            .ok_or(EditError::InvalidPosition)?
            ..super::super::grapheme::GraphemeMap::snap_in(text, end, Affinity::Downstream)
                .ok_or(EditError::InvalidPosition)?,
    )
}

fn combine_effects(effects: Vec<Effect>) -> Effect {
    let mut combined = Effect::default();
    let mut position_maps = Vec::new();
    for effect in effects {
        combined.document_changed |= effect.document_changed;
        combined.selection_changed |= effect.selection_changed;
        combined.composition_changed |= effect.composition_changed;
        if let Some(position_map) = effect.position_map {
            position_maps.push(position_map);
        }
    }
    combined.position_map = match position_maps.len() {
        0 => None,
        1 => position_maps.pop(),
        _ => Some(PositionMap::Composite(position_maps)),
    };
    combined
}
