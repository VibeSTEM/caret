//! 管理编辑 Session 状态并执行输入、选区和 IME 命令。

use super::paragraph_ops::{
    ParagraphIndex, ParagraphMergeResult, build_paragraph_index, compare_positions,
    first_paragraph, insert_paragraph_after, merge_adjacent_paragraph, paragraph_at_location,
    paragraph_at_location_mut, split_environment_title, update_index_after_merge,
    update_index_after_split,
};
use super::text_map::{TextMap, split_inline_nodes};
use super::{
    Affinity, Command, Direction, EditError, Effect, InputComposition, Pos, PositionMap, Selection,
    StoryId,
};
use caret_doc::{Document, NodeId, PageGeometry, Paragraph};

#[derive(Debug)]
pub struct Session {
    pub(super) document: Document,
    pub(super) selection: Selection,
    session_id: NodeId,
    document_generation: u64,
    composition: Option<InputComposition>,
    last_committed_composition: Option<u64>,
    text_map_cache: Option<(NodeId, StoryId, TextMap)>,
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
            session_id: NodeId::generate(),
            document_generation: 0,
            composition: None,
            last_committed_composition: None,
            text_map_cache: None,
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

    pub fn composition(&self) -> Option<&InputComposition> {
        self.composition.as_ref()
    }

    /// Executes one command. All validation that may fail is done before replacing document data.
    pub fn run(&mut self, command: Command) -> Result<Effect, EditError> {
        let effect = match command {
            Command::InsertText(text) => self.insert_text(&text),
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

    fn insert_text(&mut self, text: &str) -> Result<Effect, EditError> {
        if text.contains(['\n', '\r']) {
            return Err(EditError::UnsupportedNewline);
        }
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let (anchor, focus) = self.edit_range()?;
        if text.is_empty() && anchor.offset == focus.offset {
            return Ok(Effect::default());
        }
        let start = if anchor.offset <= focus.offset {
            &anchor
        } else {
            &focus
        };
        let insert_offset = anchor.offset.min(focus.offset);
        if anchor.offset == focus.offset {
            let map = TextMap::new(self.paragraph_for(&anchor)?)?;
            map.validate_pos(&anchor)?;
            map.insert_text(
                self.paragraph_for_mut(&anchor)?,
                insert_offset,
                start.affinity,
                text,
            )?;
        } else {
            let mut paragraph = self.paragraph_for(&anchor)?.clone();
            let map = TextMap::new(&paragraph)?;
            map.validate_pos(&anchor)?;
            map.validate_pos(&focus)?;
            map.delete_range(
                &mut paragraph,
                anchor.offset.min(focus.offset),
                anchor.offset.max(focus.offset),
            )?;
            let updated_map = TextMap::new(&paragraph)?;
            updated_map.insert_text(&mut paragraph, insert_offset, start.affinity, text)?;
            self.replace_paragraph(&anchor, paragraph)?;
        }
        let paragraph_id = anchor.paragraph_id.clone();
        let replaced_start = anchor.offset.min(focus.offset);
        let replaced_end = anchor.offset.max(focus.offset);
        self.text_map_cache = None;
        let result_map = self.text_map_for(&Pos {
            offset: insert_offset + text.len(),
            ..start.clone()
        })?;
        let position_offset =
            result_map.snap_to_grapheme_boundary(insert_offset + text.len(), Affinity::Downstream);
        let result_boundaries = result_map.grapheme_boundaries.clone();
        let new_pos = Pos {
            offset: position_offset,
            affinity: start.affinity,
            ..anchor
        };
        self.selection = Selection {
            anchor: new_pos.clone(),
            focus: new_pos,
        };
        Ok(Effect {
            document_changed: !text.is_empty() || anchor.offset != focus.offset,
            selection_changed: true,
            composition_changed: false,
            position_map: Some(PositionMap::TextEdit {
                paragraph_id,
                start: replaced_start,
                end: replaced_end,
                inserted_len: text.len(),
                snap_graphemes: true,
                result_boundaries,
            }),
        })
    }

    fn delete(&mut self, direction: Direction) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let (anchor, focus) = self.edit_range()?;
        let paragraph = self.paragraph_for(&anchor)?;
        let map = TextMap::new(paragraph)?;
        map.validate_pos(&anchor)?;
        map.validate_pos(&focus)?;
        let (from, to) = if anchor.offset != focus.offset {
            (
                anchor.offset.min(focus.offset),
                anchor.offset.max(focus.offset),
            )
        } else {
            match direction {
                Direction::Backward => (map.previous_boundary(anchor.offset), anchor.offset),
                Direction::Forward => (anchor.offset, map.next_boundary(anchor.offset)),
            }
        };
        if from == to {
            let location = self
                .paragraph_index
                .get(&anchor.paragraph_id)
                .cloned()
                .ok_or_else(|| EditError::UnknownParagraph(anchor.paragraph_id.to_string()))?;
            if anchor.offset == 0
                && direction == Direction::Backward
                && matches!(&anchor.story, StoryId::Body)
                && let Some(ParagraphMergeResult {
                    left_id,
                    right_id,
                    left_len,
                    result_boundaries,
                }) = merge_adjacent_paragraph(
                    &mut self.document.body,
                    &location,
                    Direction::Backward,
                )?
            {
                update_index_after_merge(
                    &self.document.body,
                    &mut self.paragraph_index,
                    &left_id,
                    &right_id,
                );
                let position_map = PositionMap::ParagraphMerge {
                    left_id,
                    right_id,
                    left_len,
                    result_boundaries,
                };
                let pos = position_map.map(&anchor);
                self.selection = Selection {
                    anchor: pos.clone(),
                    focus: pos,
                };
                self.text_map_cache = None;
                return Ok(Effect {
                    document_changed: true,
                    selection_changed: true,
                    composition_changed: false,
                    position_map: Some(position_map),
                });
            }
            if anchor.offset == map.text.len()
                && direction == Direction::Forward
                && matches!(&anchor.story, StoryId::Body)
                && let Some(ParagraphMergeResult {
                    left_id,
                    right_id,
                    left_len,
                    result_boundaries,
                }) = merge_adjacent_paragraph(
                    &mut self.document.body,
                    &location,
                    Direction::Forward,
                )?
            {
                update_index_after_merge(
                    &self.document.body,
                    &mut self.paragraph_index,
                    &left_id,
                    &right_id,
                );
                let position_map = PositionMap::ParagraphMerge {
                    left_id,
                    right_id,
                    left_len,
                    result_boundaries,
                };
                let pos = position_map.map(&anchor);
                self.selection = Selection {
                    anchor: pos.clone(),
                    focus: pos,
                };
                self.text_map_cache = None;
                return Ok(Effect {
                    document_changed: true,
                    selection_changed: true,
                    composition_changed: false,
                    position_map: Some(position_map),
                });
            }
            return Ok(Effect::default());
        }
        if anchor.offset == focus.offset {
            map.delete_range(self.paragraph_for_mut(&anchor)?, from, to)?;
        } else {
            let mut updated = paragraph.clone();
            map.delete_range(&mut updated, from, to)?;
            self.replace_paragraph(&anchor, updated)?;
        }
        self.text_map_cache = None;
        let result_map = self.text_map_for(&Pos {
            offset: from,
            ..anchor.clone()
        })?;
        let caret_offset = result_map.snap_to_grapheme_boundary(from, Affinity::Upstream);
        let result_boundaries = result_map.grapheme_boundaries.clone();
        let collapse_affinity = if anchor.offset <= focus.offset {
            anchor.affinity
        } else {
            focus.affinity
        };
        let paragraph_id = anchor.paragraph_id.clone();
        let pos = Pos {
            offset: caret_offset,
            affinity: collapse_affinity,
            ..anchor
        };
        self.selection = Selection {
            anchor: pos.clone(),
            focus: pos,
        };
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            composition_changed: false,
            position_map: Some(PositionMap::TextEdit {
                paragraph_id,
                start: from,
                end: to,
                inserted_len: 0,
                snap_graphemes: true,
                result_boundaries,
            }),
        })
    }

    fn insert_soft_break(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let (anchor, focus) = self.edit_range()?;
        let mut paragraph = self.paragraph_for(&anchor)?.clone();
        let map = TextMap::new(&paragraph)?;
        map.validate_pos(&anchor)?;
        map.validate_pos(&focus)?;
        let offset = anchor.offset.min(focus.offset);
        if anchor.offset != focus.offset {
            map.delete_range(
                &mut paragraph,
                anchor.offset.min(focus.offset),
                anchor.offset.max(focus.offset),
            )?;
        }
        let affinity = if anchor.offset <= focus.offset {
            anchor.affinity
        } else {
            focus.affinity
        };
        let paragraph_id = anchor.paragraph_id.clone();
        let replaced_start = anchor.offset.min(focus.offset);
        let replaced_end = anchor.offset.max(focus.offset);
        TextMap::new(&paragraph)?.insert_soft_break(&mut paragraph, offset, affinity)?;
        self.replace_paragraph(&anchor, paragraph)?;
        self.text_map_cache = None;
        let result_boundaries = TextMap::new(self.paragraph_for(&anchor)?)?.grapheme_boundaries;
        let pos = Pos {
            offset: offset + 1,
            affinity: anchor.affinity,
            ..anchor
        };
        self.selection = Selection {
            anchor: pos.clone(),
            focus: pos,
        };
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            composition_changed: false,
            position_map: Some(PositionMap::TextEdit {
                paragraph_id,
                start: replaced_start,
                end: replaced_end,
                inserted_len: 1,
                snap_graphemes: true,
                result_boundaries,
            }),
        })
    }

    fn split_paragraph(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let (anchor, focus) = self.edit_range()?;
        let mut source = self.paragraph_for(&anchor)?.clone();
        let map = TextMap::new(&source)?;
        map.validate_pos(&anchor)?;
        map.validate_pos(&focus)?;
        let split_offset = anchor.offset.min(focus.offset);
        if anchor.offset != focus.offset {
            map.delete_range(
                &mut source,
                anchor.offset.min(focus.offset),
                anchor.offset.max(focus.offset),
            )?;
        }
        let result_map = TextMap::new(&source)?;
        let result_boundaries = result_map.grapheme_boundaries;
        if !source.extra.is_empty() {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let (left, right) = split_inline_nodes(std::mem::take(&mut source.inlines), split_offset)?;
        let mut left_paragraph = source.clone();
        left_paragraph.inlines = left;
        let mut right_paragraph = source;
        right_paragraph.id = NodeId::generate();
        right_paragraph.inlines = right;
        right_paragraph.extra.clear();
        let right_id = right_paragraph.id.clone();
        let source_id = anchor.paragraph_id.clone();
        let source_story = anchor.story.clone();
        let replaced_start = anchor.offset.min(focus.offset);
        let replaced_end = anchor.offset.max(focus.offset);
        let mut left_paragraph = Some(left_paragraph);
        let mut right_paragraph = Some(right_paragraph);
        let location = self
            .paragraph_index
            .get(&anchor.paragraph_id)
            .cloned()
            .ok_or_else(|| EditError::UnknownParagraph(anchor.paragraph_id.to_string()))?;

        match &anchor.story {
            StoryId::Body => {
                if !insert_paragraph_after(
                    &mut self.document.body,
                    &location,
                    &mut left_paragraph,
                    &mut right_paragraph,
                ) {
                    return Err(EditError::UnknownParagraph(anchor.paragraph_id.to_string()));
                }
            }
            StoryId::EnvironmentTitle(_) => {
                if !split_environment_title(
                    &mut self.document.body,
                    &location,
                    &mut left_paragraph,
                    &mut right_paragraph,
                ) {
                    return Err(EditError::UnknownParagraph(anchor.paragraph_id.to_string()));
                }
            }
        }
        self.text_map_cache = None;
        update_index_after_split(
            &self.document.body,
            &mut self.paragraph_index,
            &right_id,
            &location,
        );
        let pos = Pos {
            story: StoryId::Body,
            paragraph_id: right_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        self.selection = Selection {
            anchor: pos.clone(),
            focus: pos,
        };
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            composition_changed: false,
            position_map: Some(if replaced_start == replaced_end {
                PositionMap::ParagraphSplit {
                    source_id,
                    right_id,
                    split_offset,
                    source_story,
                    right_story: StoryId::Body,
                }
            } else {
                PositionMap::Composite(vec![
                    PositionMap::TextEdit {
                        paragraph_id: source_id.clone(),
                        start: replaced_start,
                        end: replaced_end,
                        inserted_len: 0,
                        snap_graphemes: false,
                        result_boundaries,
                    },
                    PositionMap::ParagraphSplit {
                        source_id,
                        right_id,
                        split_offset,
                        source_story,
                        right_story: StoryId::Body,
                    },
                ])
            }),
        })
    }

    fn move_caret(&mut self, direction: Direction) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if self.selection.anchor != self.selection.focus {
            let ordering = compare_positions(
                &self.paragraph_index,
                &self.selection.anchor,
                &self.selection.focus,
            )?;
            let (left, right) = if ordering.is_le() {
                (&self.selection.anchor, &self.selection.focus)
            } else {
                (&self.selection.focus, &self.selection.anchor)
            };
            let pos = match direction {
                Direction::Backward => left.clone(),
                Direction::Forward => right.clone(),
            };
            self.selection = Selection {
                anchor: pos.clone(),
                focus: pos,
            };
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        let pos = self.selection.focus.clone();
        let map = self.text_map_for(&pos)?;
        map.validate_pos(&pos)?;
        if direction == Direction::Forward
            && pos.affinity == Affinity::Upstream
            && map
                .environment_spans
                .iter()
                .any(|(_, end)| *end == pos.offset)
        {
            let target = Pos {
                affinity: Affinity::Downstream,
                ..pos
            };
            self.selection = Selection {
                anchor: target.clone(),
                focus: target,
            };
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        if direction == Direction::Backward
            && pos.affinity == Affinity::Downstream
            && map
                .environment_spans
                .iter()
                .any(|(start, _)| *start == pos.offset)
        {
            let target = Pos {
                affinity: Affinity::Upstream,
                ..pos
            };
            self.selection = Selection {
                anchor: target.clone(),
                focus: target,
            };
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        let offset = match direction {
            Direction::Backward => map.previous_boundary(pos.offset),
            Direction::Forward => map.next_boundary(pos.offset),
        };
        let affinity = if map
            .environment_spans
            .iter()
            .any(|(start, end)| *start == offset || *end == offset)
        {
            match direction {
                Direction::Backward => Affinity::Downstream,
                Direction::Forward => Affinity::Upstream,
            }
        } else {
            pos.affinity
        };
        let target = Pos {
            offset,
            affinity,
            ..pos
        };
        let changed = target != self.selection.focus;
        self.selection = Selection {
            anchor: target.clone(),
            focus: target,
        };
        Ok(Effect {
            selection_changed: changed,
            ..Effect::default()
        })
    }

    fn set_selection(&mut self, selection: Option<Selection>) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let next = selection.unwrap_or_else(|| Selection {
            anchor: self.selection.focus.clone(),
            focus: self.selection.focus.clone(),
        });
        self.text_map_for(&next.anchor)?
            .validate_pos(&next.anchor)?;
        self.text_map_for(&next.focus)?.validate_pos(&next.focus)?;
        let changed = self.selection != next;
        self.selection = next;
        Ok(Effect {
            selection_changed: changed,
            ..Effect::default()
        })
    }

    fn update_composition(&mut self, id: u64, preedit: String) -> Result<Effect, EditError> {
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

    fn commit_composition(&mut self, id: u64, text: &str) -> Result<Effect, EditError> {
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
        match self.insert_text(text) {
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

    fn cancel_composition(&mut self, id: u64) -> Result<Effect, EditError> {
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

    fn edit_range(&self) -> Result<(Pos, Pos), EditError> {
        let selection = &self.selection;
        if selection.anchor.paragraph_id != selection.focus.paragraph_id
            || selection.anchor.story != selection.focus.story
        {
            return Err(EditError::CrossParagraphSelection);
        }
        Ok((selection.anchor.clone(), selection.focus.clone()))
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

    fn text_map_for(&mut self, pos: &Pos) -> Result<&TextMap, EditError> {
        let cache_miss = self
            .text_map_cache
            .as_ref()
            .is_none_or(|(paragraph_id, story, _)| {
                paragraph_id != &pos.paragraph_id || story != &pos.story
            });
        if cache_miss {
            let paragraph = self.paragraph_for(pos)?;
            let map = TextMap::new(paragraph)?;
            self.text_map_cache = Some((pos.paragraph_id.clone(), pos.story.clone(), map));
        }
        self.text_map_cache
            .as_ref()
            .map(|(_, _, map)| map)
            .ok_or(EditError::InvalidPosition)
    }

    fn replace_paragraph(&mut self, pos: &Pos, paragraph: Paragraph) -> Result<(), EditError> {
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

/// Clone forks an independent editor session, so its async-layout identity and revision restart.
impl Clone for Session {
    fn clone(&self) -> Self {
        Self {
            document: self.document.clone(),
            selection: self.selection.clone(),
            session_id: NodeId::generate(),
            document_generation: 0,
            composition: self.composition.clone(),
            last_committed_composition: self.last_committed_composition,
            text_map_cache: self.text_map_cache.clone(),
            paragraph_index: self.paragraph_index.clone(),
        }
    }
}
