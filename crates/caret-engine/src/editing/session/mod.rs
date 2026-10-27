//! 管理编辑 Session 状态并执行输入、选区和 IME 命令。

mod clipboard;
mod composition;
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
use super::{Affinity, Command, EditError, Effect, InputComposition, Pos, Selection, StoryId};
use caret_doc::{Document, NodeId, PageGeometry, Paragraph};
use caret_layout::VisualAffinity;
use std::collections::VecDeque;

const TEXT_MAP_CACHE_CAPACITY: usize = 2;

#[derive(Debug)]
pub struct Session {
    pub(super) document: Document,
    pub(super) selection: Selection,
    selection_visual_affinity: VisualAffinity,
    session_id: NodeId,
    document_generation: u64,
    composition: Option<InputComposition>,
    last_committed_composition: Option<u64>,
    text_map_cache: VecDeque<(NodeId, StoryId, TextMap)>,
    paragraph_index: ParagraphIndex,
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
            last_committed_composition: None,
            text_map_cache: VecDeque::new(),
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

    /// 执行唯一命令入口；可恢复校验在替换文档数据前完成。
    pub fn run(&mut self, command: Command) -> Result<Effect, EditError> {
        let effect = match command {
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
        }?;
        if effect.selection_changed {
            self.selection_visual_affinity = match self.selection.focus.affinity {
                Affinity::Upstream => VisualAffinity::Upstream,
                Affinity::Downstream => VisualAffinity::Downstream,
            };
        }
        if effect.document_changed {
            if let Some(next) = self.document_generation.checked_add(1) {
                self.document_generation = next;
            } else {
                self.session_id = NodeId::generate();
                self.document_generation = 0;
            }
        }
        Ok(effect)
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
            last_committed_composition: self.last_committed_composition,
            text_map_cache: self.text_map_cache.clone(),
            paragraph_index: self.paragraph_index.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caret_doc::Block;

    #[test]
    fn text_map_cache_keeps_both_paragraphs_of_a_cross_paragraph_selection() {
        let mut session = Session::new(Document::blank())
            .unwrap_or_else(|error| panic!("session creation failed: {error}"));
        session
            .run(Command::SplitParagraph)
            .unwrap_or_else(|error| panic!("paragraph split failed: {error}"));
        let Block::Paragraph(first) = &session.document.body[0] else {
            panic!("first block should be a paragraph");
        };
        let Block::Paragraph(second) = &session.document.body[1] else {
            panic!("second block should be a paragraph");
        };
        let first_id = first.id.clone();
        let second_id = second.id.clone();
        let first_position = Pos {
            story: StoryId::Body,
            paragraph_id: first_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        let second_position = Pos {
            paragraph_id: second_id.clone(),
            ..first_position.clone()
        };

        session
            .text_map_for(&first_position)
            .unwrap_or_else(|error| panic!("first text map failed: {error}"));
        session
            .text_map_for(&second_position)
            .unwrap_or_else(|error| panic!("second text map failed: {error}"));
        session
            .text_map_for(&first_position)
            .unwrap_or_else(|error| panic!("cached first text map failed: {error}"));

        assert_eq!(session.text_map_cache.len(), TEXT_MAP_CACHE_CAPACITY);
        assert!(
            session
                .text_map_cache
                .iter()
                .any(|(id, _, _)| id == &first_id)
        );
        assert!(
            session
                .text_map_cache
                .iter()
                .any(|(id, _, _)| id == &second_id)
        );
    }

    #[test]
    fn revision_overflow_rotates_session_identity_before_reusing_generation_zero() {
        let mut session = Session::new(Document::blank())
            .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
        let old_session_id = session.session_id().clone();
        session.document_generation = u64::MAX;

        let effect = session
            .run(Command::InsertText("new revision".to_owned()))
            .unwrap_or_else(|error| panic!("text insertion failed: {error}"));

        assert!(effect.document_changed);
        assert_eq!(session.document_generation(), 0);
        assert_ne!(session.session_id(), &old_session_id);
        assert_eq!(session.selection().focus.offset, "new revision".len());
    }
}
