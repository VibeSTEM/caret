//! 管理 IME 组字的更新、幂等提交与取消，提交正文仍复用文本编辑事务。

use super::super::paragraph_ops::{compare_positions, paragraph_at_location};
use super::super::text_map::{utf8_offset_to_utf16, utf16_offset_to_utf8};
use super::super::{
    Affinity, Command, EditError, Effect, InputComposition, Pos, PositionMap, Selection,
};
use super::Session;
use super::history::{self, DocumentPatch, SessionHistoryEdit, SessionHistoryState};
use std::mem;

impl Session {
    /// 按 Qt UTF-16 相对范围原子提交输入法替换，支持先取消已有预编辑再应用新范围。
    ///
    /// `active_composition_id` 指向待替换的预编辑；两个传入的新 ID 分别用于先删除原选区和提交替换文本。
    /// 若范围无效、文本编辑不兼容或任一事务命令失败，返回对应错误且保留当前会话状态。
    pub fn commit_composition_replacement(
        &mut self,
        active_composition_id: Option<u64>,
        selection_removal_composition_id: u64,
        replacement_composition_id: u64,
        replacement_start: i32,
        replacement_length: i32,
        text: &str,
    ) -> Result<Effect, EditError> {
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
            candidate.document_generation = self
                .document_generation
                .checked_add(1)
                .unwrap_or(candidate.document_generation);
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
    ) -> Result<(Self, Self, Selection), EditError> {
        let mut candidate = self.transaction_clone();
        if let Some((replacement_start, replacement_length)) = replacement_range {
            candidate.set_composition_replacement_range(replacement_start, replacement_length)?;
        }
        candidate.run(Command::UpdateComposition { id, preedit })?;
        let (preview, range) = candidate.composition_preview_with_range(cursor_position_utf16)?;
        Ok((candidate, preview, range))
    }

    /// 布局验证成功后安装候选，并移动原会话历史，避免失败时丢失撤销分支。
    pub fn install_composition_candidate(&mut self, mut candidate: Self) {
        candidate.edit_history = mem::replace(&mut self.edit_history, history::history());
        candidate.history_estimated_bytes = self.history_estimated_bytes;
        *self = candidate;
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
    ) -> Result<(Self, Selection), EditError> {
        let composition = self
            .composition
            .as_ref()
            .ok_or(EditError::InvalidCompositionId)?;
        let preedit = composition.preedit.clone();
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
        let has_pending_inline_style = preview
            .pending_inline_style
            .active_formats()
            .next()
            .is_some();
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
        Ok((preview, range))
    }

    /// 将块环境外的预编辑位置映射到新建的同级段落，而不是映射回环境内部段落。
    fn composition_preview_at_environment_exit(
        &self,
        mut preview: Self,
        preedit: &str,
        cursor_position_utf16: usize,
    ) -> Result<(Self, Selection), EditError> {
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
        Ok((preview, range))
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
        if replacement_start == 0 && replacement_length == 0 {
            return Ok(());
        }
        // 虚拟环境外位置没有相邻文本可供 IME 替换，保持外侧停靠点并将提交视为插入。
        if self.block_environment_exit.is_some() {
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
        let result = if self.block_environment_exit.is_some() {
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
