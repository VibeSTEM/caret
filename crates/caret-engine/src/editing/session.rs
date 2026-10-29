//! 管理编辑 Session 状态并执行输入、选区和 IME 命令。

mod clipboard;
mod command_line;
mod composition;
mod history;
mod navigation;
mod paragraph_edit;
mod range_edit;
mod selection;
mod text_edit;

use super::paragraph_ops::{
    ParagraphIndex, build_paragraph_index, first_paragraph, paragraph_at_location,
    paragraph_at_location_mut,
};
use super::text_map::TextMap;
use super::{
    Affinity, Command, CommandLine, Direction, EditError, Effect, InputComposition, Pos, Selection,
    StoryId,
};
use caret_doc::{Document, NodeId, PageGeometry, Paragraph};
use caret_layout::VisualAffinity;
use std::collections::VecDeque;
use undo::History;

use self::history::{DocumentPatch, SessionHistoryEdit};

const TEXT_MAP_CACHE_CAPACITY: usize = 2;

#[derive(Debug)]
pub struct Session {
    pub(super) document: Document,
    pub(super) selection: Selection,
    selection_visual_affinity: VisualAffinity,
    session_id: NodeId,
    document_generation: u64,
    composition: Option<InputComposition>,
    command_line: Option<CommandLine>,
    last_committed_composition: Option<u64>,
    text_map_cache: VecDeque<(NodeId, StoryId, TextMap)>,
    paragraph_index: ParagraphIndex,
    edit_history: History<SessionHistoryEdit>,
    history_state_id: u64,
    next_history_state_id: u64,
    history_estimated_bytes: usize,
    history_merge_epoch: u64,
}

impl Session {
    pub fn new(document: Document) -> Result<Self, EditError> {
        document
            .validate()
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
        let (paragraph, story) = first_paragraph(&document.body)
            .ok_or_else(|| EditError::InvalidDocument("document has no paragraph".to_owned()))?;
        let pos = Pos {
            story,
            paragraph_id: paragraph.id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        Ok(Self {
            paragraph_index: build_paragraph_index(&document.body),
            document,
            selection: Selection {
                anchor: pos.clone(),
                focus: pos,
            },
            selection_visual_affinity: VisualAffinity::Downstream,
            session_id: NodeId::generate(),
            document_generation: 0,
            composition: None,
            command_line: None,
            last_committed_composition: None,
            text_map_cache: VecDeque::new(),
            edit_history: history::history(),
            history_state_id: 0,
            next_history_state_id: 1,
            history_estimated_bytes: 0,
            history_merge_epoch: 0,
        })
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    /// 当前打开会话的唯一标识，用于区分被新 Session 替换的同代文档快照。
    pub fn session_id(&self) -> &NodeId {
        &self.session_id
    }

    /// 文档内容或页面几何每成功变更一次便递增；异步布局结果用它拒绝过时代际。
    pub fn document_generation(&self) -> u64 {
        self.document_generation
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// 当前焦点的视觉侧；不属于 UTF-8 逻辑位置，也不改变文档 generation。
    pub fn selection_visual_affinity(&self) -> VisualAffinity {
        self.selection_visual_affinity
    }

    pub fn composition(&self) -> Option<&InputComposition> {
        self.composition.as_ref()
    }

    /// 当前临时命令查询；此状态不属于文档修订或布局缓存。
    pub fn command_line(&self) -> Option<&CommandLine> {
        self.command_line.as_ref()
    }

    /// 当前文档历史状态标识，用于把异步保存结果关联到实际保存点。
    pub fn history_state_id(&self) -> u64 {
        self.history_state_id
    }

    pub fn can_undo(&self) -> bool {
        self.edit_history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.edit_history.can_redo()
    }

    /// 在异步保存捕获修订前切断输入合并，保留可精确返回的保存位置。
    pub fn begin_save_checkpoint(&mut self) {
        self.advance_history_merge_epoch();
    }

    /// 执行唯一命令入口；可恢复校验在替换文档数据前完成。
    pub fn run(&mut self, command: Command) -> Result<Effect, EditError> {
        if matches!(
            &command,
            Command::BeginCommandLine
                | Command::TypeCommandLine(_)
                | Command::BackspaceCommandLine
                | Command::MoveCommandLineCursor(_)
                | Command::MoveCommandLineSelection(_)
                | Command::SelectCommandLineCandidate(_)
                | Command::TabCommandLine
                | Command::AcceptCommandLine
                | Command::CancelCommandLine
        ) {
            return self.run_command_line(command);
        }
        if command == Command::Undo {
            let effect = self.restore_history(false)?;
            self.command_line = None;
            return Ok(effect);
        }
        if command == Command::Redo {
            let effect = self.restore_history(true)?;
            self.command_line = None;
            return Ok(effect);
        }
        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let edit_group = history::edit_group(&command);
        let breaks_history_merge = edit_group.is_none();
        let mut before_paragraph_states = if is_paragraph_edit(&command) {
            let mut ids = history::before_selection_ids(&self.paragraph_index, &self.selection);
            if let Command::Delete(direction) = &command
                && self.selection.anchor == self.selection.focus
                && let Some(adjacent_id) = self.adjacent_paragraph_for_delete(*direction)?
            {
                ids.push(adjacent_id);
            }
            history::paragraph_states_for_ids(&self.document, &self.paragraph_index, ids)
        } else {
            Vec::new()
        };
        let before_page_geometry = matches!(&command, Command::SetPageGeometry(_))
            .then(|| {
                self.document
                    .page_sections
                    .first()
                    .and_then(|section| section.geometry().ok())
            })
            .flatten();
        let mut effect = match command {
            Command::InsertText(text) => self.insert_text(&text),
            Command::PasteText(text) => self.paste_text(&text),
            Command::InsertSoftBreak => self.insert_soft_break(),
            Command::SplitParagraph => self.split_paragraph(),
            Command::Delete(direction) => self.delete(direction),
            Command::MoveCaret(direction) => self.move_caret(direction),
            Command::SetSelection(selection) => self.set_selection(selection),
            Command::UpdateComposition { id, preedit } => self.update_composition(id, preedit),
            Command::CommitComposition { id, text } => self.commit_composition(id, &text),
            Command::CancelComposition { id } => self.cancel_composition(id),
            Command::SetPageGeometry(geometry) => self.set_page_geometry(geometry),
            Command::BeginCommandLine
            | Command::TypeCommandLine(_)
            | Command::BackspaceCommandLine
            | Command::MoveCommandLineCursor(_)
            | Command::MoveCommandLineSelection(_)
            | Command::SelectCommandLineCandidate(_)
            | Command::TabCommandLine
            | Command::AcceptCommandLine
            | Command::CancelCommandLine => unreachable!("command-line commands returned above"),
            Command::Undo | Command::Redo => unreachable!("history commands returned above"),
        }?;
        self.command_line = None;
        if breaks_history_merge {
            self.advance_history_merge_epoch();
        }
        if effect.selection_changed {
            self.selection_visual_affinity = match self.selection.focus.affinity {
                Affinity::Upstream => VisualAffinity::Upstream,
                Affinity::Downstream => VisualAffinity::Downstream,
            };
        }
        if effect.document_changed {
            let after_state_id = self.allocate_history_state_id();
            let patch = if let Some(before) = before_page_geometry {
                let after = self
                    .document
                    .page_sections
                    .first()
                    .and_then(|section| section.geometry().ok())
                    .ok_or_else(|| {
                        EditError::InvalidDocument("document has no PageSection".to_owned())
                    })?;
                DocumentPatch::PageGeometry { before, after }
            } else {
                let mut ids = before_paragraph_states
                    .iter()
                    .map(|state| state.id.clone())
                    .collect::<Vec<_>>();
                if let Some(map) = &effect.position_map {
                    history::state_ids_from_position_map(map, &mut ids);
                }
                let after_paragraph_states =
                    history::paragraph_states_for_ids(&self.document, &self.paragraph_index, ids);
                if after_paragraph_states.is_empty() {
                    return Err(EditError::InvalidDocument(
                        "changed document command did not produce a history patch".to_owned(),
                    ));
                }
                DocumentPatch::Paragraphs {
                    before: std::mem::take(&mut before_paragraph_states),
                    after: after_paragraph_states,
                    merge_group: edit_group,
                    merge_epoch: self.history_merge_epoch,
                }
            };
            effect.changed_paragraph_id = patch.single_changed_paragraph_id();
            let edit = SessionHistoryEdit::new(
                patch,
                before_selection,
                self.selection.clone(),
                before_visual_affinity,
                self.selection_visual_affinity,
                before_state_id,
                after_state_id,
            );
            self.record_history_edit(edit);
            self.history_state_id = after_state_id;
            if let Some(next) = self.document_generation.checked_add(1) {
                self.document_generation = next;
            } else {
                self.session_id = NodeId::generate();
                self.document_generation = 0;
            }
        }
        Ok(effect)
    }

    fn record_history_edit(&mut self, edit: SessionHistoryEdit) {
        let latest_edit_bytes = edit.estimated_bytes();
        let transition = self.edit_history.edit(&mut self.document, edit.clone());
        self.history_state_id = transition.state_id;
        self.history_estimated_bytes = history::estimated_history_bytes(&self.edit_history);
        if self.history_estimated_bytes > history::history_memory_limit() {
            self.edit_history.clear();
            self.history_estimated_bytes = 0;
            if latest_edit_bytes <= history::history_memory_limit() {
                let transition = self.edit_history.edit(&mut self.document, edit);
                self.history_state_id = transition.state_id;
                self.history_estimated_bytes = history::estimated_history_bytes(&self.edit_history);
                if self.history_estimated_bytes > history::history_memory_limit() {
                    self.edit_history.clear();
                    self.history_estimated_bytes = 0;
                }
            }
        }
    }

    fn advance_history_merge_epoch(&mut self) {
        if let Some(next) = self.history_merge_epoch.checked_add(1) {
            self.history_merge_epoch = next;
        } else {
            self.edit_history.clear();
            self.history_estimated_bytes = 0;
            self.history_merge_epoch = 0;
        }
    }

    pub(super) fn allocate_history_state_id(&mut self) -> u64 {
        let state_id = self.next_history_state_id;
        if let Some(next) = state_id.checked_add(1) {
            self.next_history_state_id = next;
        } else {
            self.edit_history.clear();
            self.history_estimated_bytes = 0;
            self.next_history_state_id = 1;
        }
        state_id
    }

    fn restore_history(&mut self, redo: bool) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let transition = if redo {
            self.edit_history.redo(&mut self.document)
        } else {
            self.edit_history.undo(&mut self.document)
        };
        let Some(transition) = transition else {
            return Ok(Effect::default());
        };
        let (selection, visual_affinity, state_id, changed_paragraph_id) =
            history::restore_history_transition(transition);
        self.selection = selection;
        self.selection_visual_affinity = visual_affinity;
        self.history_state_id = state_id;
        self.advance_history_merge_epoch();
        self.paragraph_index = build_paragraph_index(&self.document.body);
        self.text_map_cache.clear();
        if let Some(next) = self.document_generation.checked_add(1) {
            self.document_generation = next;
        } else {
            self.session_id = NodeId::generate();
            self.document_generation = 0;
        }
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            changed_paragraph_id,
            ..Effect::default()
        })
    }

    fn adjacent_paragraph_for_delete(
        &mut self,
        direction: Direction,
    ) -> Result<Option<NodeId>, EditError> {
        let position = self.selection.focus.clone();
        let at_boundary = match direction {
            Direction::Backward => position.offset == 0,
            Direction::Forward => position.offset == self.text_map_for(&position)?.text.len(),
        };
        Ok(at_boundary
            .then(|| {
                history::include_adjacent_paragraph_id(
                    &self.paragraph_index,
                    &position.paragraph_id,
                    direction,
                )
            })
            .flatten())
    }

    fn set_page_geometry(&mut self, geometry: PageGeometry) -> Result<Effect, EditError> {
        geometry
            .validate()
            .map_err(|error| EditError::InvalidPageGeometry(error.to_string()))?;
        let section =
            self.document.page_sections.first_mut().ok_or_else(|| {
                EditError::InvalidDocument("document has no PageSection".to_owned())
            })?;
        let current = section
            .geometry()
            .map_err(|error| EditError::InvalidPageGeometry(error.to_string()))?;
        if current == geometry {
            return Ok(Effect::default());
        }
        section
            .set_geometry(geometry)
            .map_err(|error| EditError::InvalidPageGeometry(error.to_string()))?;
        Ok(Effect {
            document_changed: true,
            ..Effect::default()
        })
    }

    pub(super) fn paragraph_for(&self, pos: &Pos) -> Result<&Paragraph, EditError> {
        let location = self
            .paragraph_index
            .get(&pos.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))?;
        if location.story != pos.story {
            return Err(EditError::WrongStory);
        }
        paragraph_at_location(&self.document.body, location)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))
    }

    fn paragraph_for_mut(&mut self, pos: &Pos) -> Result<&mut Paragraph, EditError> {
        let location = self
            .paragraph_index
            .get(&pos.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))?;
        if location.story != pos.story {
            return Err(EditError::WrongStory);
        }
        paragraph_at_location_mut(&mut self.document.body, location)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))
    }

    pub(super) fn text_map_for(&mut self, pos: &Pos) -> Result<&TextMap, EditError> {
        if let Some(index) = self
            .text_map_cache
            .iter()
            .position(|(paragraph_id, story, _)| {
                paragraph_id == &pos.paragraph_id && story == &pos.story
            })
        {
            if index > 0 {
                let entry = self
                    .text_map_cache
                    .remove(index)
                    .ok_or(EditError::InvalidPosition)?;
                self.text_map_cache.push_front(entry);
            }
        } else {
            let paragraph = self.paragraph_for(pos)?;
            let map = TextMap::new(paragraph)?;
            self.text_map_cache
                .push_front((pos.paragraph_id.clone(), pos.story.clone(), map));
            if self.text_map_cache.len() > TEXT_MAP_CACHE_CAPACITY {
                self.text_map_cache.pop_back();
            }
        }
        self.text_map_cache
            .front()
            .map(|(_, _, map)| map)
            .ok_or(EditError::InvalidPosition)
    }

    pub(super) fn replace_paragraph(
        &mut self,
        pos: &Pos,
        paragraph: Paragraph,
    ) -> Result<(), EditError> {
        let location = self
            .paragraph_index
            .get(&pos.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))?;
        if location.story != pos.story {
            return Err(EditError::WrongStory);
        }
        *paragraph_at_location_mut(&mut self.document.body, location)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))? = paragraph;
        Ok(())
    }
}

/// 克隆创建独立编辑会话，异步布局身份与文档代际从新会话重新开始。
impl Clone for Session {
    fn clone(&self) -> Self {
        Self {
            document: self.document.clone(),
            selection: self.selection.clone(),
            selection_visual_affinity: self.selection_visual_affinity,
            session_id: NodeId::generate(),
            document_generation: 0,
            composition: self.composition.clone(),
            command_line: self.command_line.clone(),
            last_committed_composition: self.last_committed_composition,
            text_map_cache: self.text_map_cache.clone(),
            paragraph_index: self.paragraph_index.clone(),
            edit_history: self.edit_history.clone(),
            history_state_id: self.history_state_id,
            next_history_state_id: self.next_history_state_id,
            history_estimated_bytes: self.history_estimated_bytes,
            history_merge_epoch: self.history_merge_epoch,
        }
    }
}

fn is_paragraph_edit(command: &Command) -> bool {
    matches!(
        command,
        Command::InsertText(_)
            | Command::PasteText(_)
            | Command::InsertSoftBreak
            | Command::SplitParagraph
            | Command::Delete(_)
            | Command::CommitComposition { .. }
    )
}

#[cfg(test)]
mod tests;
