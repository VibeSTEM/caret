//! 公式编辑状态：在行内公式或显示公式块内部按 LaTeX 源码编辑，并在每次修改后重新解析。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::{
    collections::{HashMap, HashSet, VecDeque},
    ops::Range,
};
use unicode_segmentation::UnicodeSegmentation;

use super::super::paragraph_ops::build_paragraph_index;
use super::super::text_map::{OBJECT_REPLACEMENT, TextMap, inline_length};
use super::super::{
    Affinity, Command, CommandLine, Direction, EditError, EditorKey, Effect, Pos, Selection,
    StoryId,
};
use super::Session;
use super::history::{self, DocumentPatch, EditGroup, SessionHistoryEdit, SessionHistoryState};
use caret_doc::{
    Align, Block, Equation, InlineMath, InlineNode, MathNode, MathTree, NodeId, Paragraph,
};

const ALIGN_BEGIN: &str = "\\begin{aligned}";
const ALIGN_END: &str = "\\end{aligned}";
/// 对齐公式里 Enter 插入的换行分隔符。
const ALIGN_ROW_BREAK: &str = " \\\\ ";

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

/// 公式编辑状态：源码与源码内的 UTF-8 光标。不属于文档修订。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathEdit {
    pub target: MathEditTarget,
    pub source: String,
    pub cursor: usize,
}

impl MathEdit {
    /// 对齐公式中光标所在的行、单元格及单元格内偏移；单元格源码已去掉首尾空白。
    pub fn align_cell_at_cursor(&self) -> Option<(usize, usize, usize)> {
        if !matches!(self.target, MathEditTarget::Block { align: true, .. }) {
            return None;
        }
        align_cell_at(&self.source, self.cursor)
    }
}

impl Session {
    /// 当前公式编辑状态；为 `None` 时光标在正文中。
    pub fn math_edit(&self) -> Option<&MathEdit> {
        self.math_edit.as_ref()
    }

    /// 公式编辑期间截获输入；返回 `None` 表示命令照常执行（并已退出公式编辑）。
    pub(super) fn run_math_edit(&mut self, command: &Command) -> Result<Option<Effect>, EditError> {
        if self.math_edit.is_none() {
            return Ok(None);
        }
        if self.command_line.is_some() {
            // 子命令行打开时，命令行命令照常执行；其余命令先关闭命令行。
            return Ok(None);
        }
        let effect = match command {
            Command::InsertText(text) | Command::PasteText(text) => self.math_insert_text(text)?,
            Command::CommitComposition { text, .. } => {
                let mut effect = self.math_insert_text(text)?;
                effect.composition_changed = true;
                effect
            }
            Command::UpdateComposition { .. } | Command::CancelComposition { .. } => {
                Effect::default()
            }
            Command::Delete(Direction::Backward) => self.math_delete_backward()?,
            Command::Delete(Direction::Forward) => self.math_delete_forward()?,
            Command::MoveCaret(direction) => self.math_move(*direction)?,
            Command::EnterMath(_) => Effect::default(),
            Command::RemoveRunPropertyPreservingContent(_) => Effect::default(),
            Command::EditorKey(EditorKey::Enter)
                if matches!(
                    self.math_edit.as_ref().map(|edit| &edit.target),
                    Some(MathEditTarget::Block { align: true, .. })
                ) =>
            {
                self.math_insert_raw(ALIGN_ROW_BREAK, ALIGN_ROW_BREAK.len())?
            }
            Command::EditorKey(EditorKey::Escape | EditorKey::Enter) => {
                self.exit_math_edit(Direction::Forward)?
            }
            Command::BeginCommandLine => {
                self.command_line = Some(CommandLine::math(self.selection.focus.clone()));
                Effect::default()
            }
            _ => {
                self.math_edit = None;
                return Ok(None);
            }
        };
        Ok(Some(effect))
    }

    /// `\math`：插入空行内公式并进入编辑。
    pub(super) fn begin_inline_math(&mut self) -> Result<Effect, EditError> {
        let effect = self.run(Command::InsertInlineMath(String::new()))?;
        let focus = self.selection.focus.clone();
        let end = focus
            .offset
            .checked_sub(OBJECT_REPLACEMENT.len_utf8())
            .ok_or(EditError::InvalidPosition)?;
        let paragraph = self.paragraph_for(&focus)?;
        let math_id = inline_math_slots(paragraph)?
            .into_iter()
            .find(|(start, _)| *start == end)
            .map(|(_, id)| id)
            .ok_or(EditError::InvalidPosition)?;
        self.math_edit = Some(MathEdit {
            target: MathEditTarget::Inline {
                paragraph_id: focus.paragraph_id,
                math_id,
            },
            source: String::new(),
            cursor: 0,
        });
        Ok(effect)
    }

    /// `\equation` / `\align`：在当前正文段落之后插入空显示公式块并进入编辑。
    /// 当前段落为空时公式插在它之前，空段落留作公式后的续写位置。
    pub(super) fn insert_display_math(&mut self, align: bool) -> Result<Effect, EditError> {
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
        let (parent_ids, sibling_index) =
            history::parent_environment_path(&self.document.body, &location)
                .ok_or(EditError::InvalidPosition)?;
        let paragraph = self.paragraph_for(&focus)?.clone();
        let paragraph_empty = TextMap::new(&paragraph)?.text.is_empty();
        let block = display_math_block(align, "")?;
        let block_id = block_id(&block)
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        let current = Block::Paragraph(paragraph.clone());
        let (after, next_position) = if paragraph_empty {
            (vec![block, current.clone()], focus.clone())
        } else {
            let next = Paragraph {
                id: NodeId::generate(),
                inlines: Vec::new(),
                ..paragraph.clone()
            };
            let position = Pos {
                story: StoryId::Body,
                paragraph_id: next.id.clone(),
                offset: 0,
                affinity: Affinity::Downstream,
            };
            (
                vec![current.clone(), block, Block::Paragraph(next)],
                position,
            )
        };
        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let blocks = block_list(&mut self.document.body, &parent_ids)?;
        blocks.splice(sibling_index..=sibling_index, after.iter().cloned());
        self.selection = Selection {
            anchor: next_position.clone(),
            focus: next_position,
        };
        let index = build_paragraph_index(&self.document.body);
        self.finalize_structure_edit(
            index,
            DocumentPatch::BlockRange {
                parent_environment_ids: parent_ids,
                sibling_index,
                before: vec![current],
                after,
            },
            before_selection,
            before_visual_affinity,
            before_state_id,
        );
        self.math_edit = Some(MathEdit {
            target: MathEditTarget::Block { block_id, align },
            source: String::new(),
            cursor: 0,
        });
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            ..Effect::default()
        })
    }

    /// 从正文进入相邻公式：向前进入光标后的公式（光标置于开头），向后进入光标前的公式（光标置于末尾）。
    pub(super) fn enter_math(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let focus = self.selection.focus.clone();
        if self.selection.anchor != focus
            || self.composition.is_some()
            || self.command_line.is_some()
            || self.block_environment_exit.is_some()
        {
            return Ok(Effect::default());
        }
        let paragraph = self.paragraph_for(&focus)?.clone();
        let object_len = OBJECT_REPLACEMENT.len_utf8();
        let slot = inline_math_slots(&paragraph)?
            .into_iter()
            .find(|(start, _)| match direction {
                Direction::Forward => *start == focus.offset,
                Direction::Backward => start + object_len == focus.offset,
            });
        if let Some((start, math_id)) = slot {
            let source = find_inline_math(&paragraph.inlines, &math_id)
                .map(|math| math.math_tree.source.clone())
                .ok_or(EditError::InvalidPosition)?;
            let body = caret_math::input_body_range(&source, caret_math::MathInputKind::Inline)
                .map_err(|error| EditError::InvalidMath(error.to_string()))?;
            let cursor = match direction {
                Direction::Forward => body.start,
                Direction::Backward => body.end,
            };
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
            self.math_edit = Some(MathEdit {
                target: MathEditTarget::Inline {
                    paragraph_id: paragraph.id.clone(),
                    math_id,
                },
                source,
                cursor,
            });
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        let text_len = TextMap::new(&paragraph)?.text.len();
        let at_edge = match direction {
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
            history::parent_environment_path(&self.document.body, &location)
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
        let siblings = block_list_ref(&self.document.body, &parent_ids)?;
        let (block_id, align, source) = match siblings.get(neighbour) {
            Some(Block::Equation(equation)) => (
                equation.id.clone(),
                false,
                equation.math_tree.source.clone(),
            ),
            Some(Block::Align(value)) => (value.id.clone(), true, align_body(&value.source)),
            _ => return Ok(Effect::default()),
        };
        let body = if align {
            0..source.len()
        } else {
            caret_math::input_body_range(&source, caret_math::MathInputKind::Equation)
                .map_err(|error| EditError::InvalidMath(error.to_string()))?
        };
        let cursor = match direction {
            Direction::Forward => body.start,
            Direction::Backward => body.end,
        };
        self.math_edit = Some(MathEdit {
            target: MathEditTarget::Block { block_id, align },
            source,
            cursor,
        });
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    /// 公式子命令：在光标处插入模板，光标进入模板的第一个空槽 `{}`。
    pub(super) fn math_insert_template(&mut self, template: &str) -> Result<Effect, EditError> {
        let cursor = template
            .find("{}")
            .map_or(template.len(), |index| index + 1);
        self.math_insert_raw(template, cursor)
    }

    fn math_insert_text(&mut self, text: &str) -> Result<Effect, EditError> {
        let edit = self.math_edit.as_ref().ok_or(EditError::InvalidPosition)?;
        let mut source = edit.source.clone();
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
        self.write_math_source(source, cursor, Some(EditGroup::InsertText))
    }

    fn math_insert_raw(&mut self, text: &str, cursor_in_text: usize) -> Result<Effect, EditError> {
        let edit = self.math_edit.as_ref().ok_or(EditError::InvalidPosition)?;
        let mut source = edit.source.clone();
        source.insert_str(edit.cursor, text);
        let cursor = edit.cursor + cursor_in_text;
        self.write_math_source(source, cursor, None)
    }

    fn math_delete_backward(&mut self) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let body = math_input_body_range(&edit).ok_or(EditError::InvalidPosition)?;
        let body_source = edit
            .source
            .get(body.clone())
            .ok_or(EditError::InvalidPosition)?;
        if body_source.trim().is_empty() {
            return self.delete_math_object();
        }
        let source = edit.source.as_str();
        let Some(cursor) = edit.cursor.checked_sub(body.start) else {
            return Err(EditError::InvalidPosition);
        };
        if cursor == 0 {
            return self.exit_math_edit(Direction::Backward);
        }
        if let Some(construct_range) = self.empty_construct_range(&edit) {
            let mut next = source.to_owned();
            next.replace_range(construct_range.clone(), "");
            return self.write_math_source(
                next,
                construct_range.start,
                Some(EditGroup::DeleteBackward),
            );
        }
        let local_start =
            previous_math_caret_stop(body_source, cursor).ok_or(EditError::InvalidPosition)?;
        let start = body.start + local_start;
        let end = body.start + cursor;
        let mut next = source.to_owned();
        next.replace_range(start..end, "");
        self.write_math_source(next, start, Some(EditGroup::DeleteBackward))
    }

    fn math_delete_forward(&mut self) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let body = math_input_body_range(&edit).ok_or(EditError::InvalidPosition)?;
        let source = edit.source.as_str();
        let cursor = edit
            .cursor
            .checked_sub(body.start)
            .ok_or(EditError::InvalidPosition)?;
        let body_source = source.get(body.clone()).ok_or(EditError::InvalidPosition)?;
        let Some(next_end) = next_math_caret_stop(body_source, cursor) else {
            return self.exit_math_edit(Direction::Forward);
        };
        let start = body.start + cursor;
        let end = body.start + next_end;
        let mut next = source.to_owned();
        next.replace_range(start..end, "");
        self.write_math_source(next, start, Some(EditGroup::DeleteForward))
    }

    fn math_move(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let edit = self.math_edit.as_ref().ok_or(EditError::InvalidPosition)?;
        let body = math_input_body_range(edit).ok_or(EditError::InvalidPosition)?;
        let source = edit
            .source
            .get(body.clone())
            .ok_or(EditError::InvalidPosition)?;
        let cursor = edit
            .cursor
            .checked_sub(body.start)
            .ok_or(EditError::InvalidPosition)?;
        let stop = match direction {
            Direction::Forward => next_math_caret_stop(source, cursor),
            Direction::Backward => previous_math_caret_stop(source, cursor),
        };
        match stop {
            Some(stop) => {
                self.set_math_cursor(body.start + stop);
                Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                })
            }
            None => self.exit_math_edit(direction),
        }
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

    /// 退出公式编辑，把正文光标放到公式之前（向后）或之后（向前）。
    pub(super) fn exit_math_edit(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let Some(edit) = self.math_edit.take() else {
            return Ok(Effect::default());
        };
        match edit.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let story = self
                    .story_for_paragraph(&paragraph_id)
                    .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
                let probe = Pos {
                    story,
                    paragraph_id,
                    offset: 0,
                    affinity: Affinity::Downstream,
                };
                let start = inline_math_slots(self.paragraph_for(&probe)?)?
                    .into_iter()
                    .find(|(_, id)| *id == math_id)
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
                self.selection = Selection {
                    anchor: position.clone(),
                    focus: position,
                };
            }
            MathEditTarget::Block { block_id, .. } => {
                if let Some(position) = self.paragraph_beside_block(&block_id, direction)? {
                    self.selection = Selection {
                        anchor: position.clone(),
                        focus: position,
                    };
                }
            }
        }
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    /// 块公式前（向后）或后（向前）的同级正文段落边缘。
    fn paragraph_beside_block(
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
            affinity: Affinity::Downstream,
        }))
    }

    /// 空公式中退格：删除整个公式对象并退出编辑。
    fn delete_math_object(&mut self) -> Result<Effect, EditError> {
        let Some(edit) = self.math_edit.clone() else {
            return Ok(Effect::default());
        };
        match edit.target {
            MathEditTarget::Inline { .. } => {
                self.exit_math_edit(Direction::Forward)?;
                self.run(Command::Delete(Direction::Backward))
            }
            MathEditTarget::Block { block_id, .. } => {
                let target = self
                    .paragraph_beside_block(&block_id, Direction::Backward)?
                    .or(self.paragraph_beside_block(&block_id, Direction::Forward)?)
                    .ok_or(EditError::InvalidPosition)?;
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
                let paragraph_index = build_paragraph_index(&self.document.body);
                self.finalize_structure_edit(
                    paragraph_index,
                    DocumentPatch::BlockRange {
                        parent_environment_ids: parent_ids,
                        sibling_index: index,
                        before: vec![removed],
                        after: Vec::new(),
                    },
                    before_selection,
                    before_visual_affinity,
                    before_state_id,
                );
                Ok(Effect {
                    document_changed: true,
                    selection_changed: true,
                    ..Effect::default()
                })
            }
        }
    }

    /// 解析新源码并写回公式对象；解析失败时文档与编辑状态都不变。
    fn write_math_source(
        &mut self,
        source: String,
        cursor: usize,
        merge_group: Option<EditGroup>,
    ) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        if source == edit.source {
            self.set_math_cursor(cursor);
            return Ok(Effect::default());
        }
        match &edit.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let caret_math::ParsedMath::Tree(mut math_tree) =
                    caret_math::parse(&source, caret_math::MathInputKind::Inline)
                        .map_err(|error| EditError::InvalidMath(error.to_string()))?
                else {
                    return Err(EditError::InvalidMath("expected inline math".to_owned()));
                };
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
                preserve_math_tree_metadata(&math.math_tree, &mut math_tree);
                math.math_tree = math_tree;
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
                self.record_math_history(patch);
            }
            MathEditTarget::Block { block_id, align } => {
                let mut block = display_math_block_with_id(*align, &source, block_id.clone())?;
                let original = find_block_path(&self.document.body, block_id).and_then(
                    |(parent_ids, index)| {
                        block_list_ref(&self.document.body, &parent_ids)
                            .ok()?
                            .get(index)
                    },
                );
                if let Some(original) = original {
                    preserve_display_math_metadata(original, &mut block);
                }
                let (parent_ids, index) = find_block_path(&self.document.body, block_id)
                    .ok_or(EditError::InvalidPosition)?;
                let blocks = block_list(&mut self.document.body, &parent_ids)?;
                let slot = blocks.get_mut(index).ok_or(EditError::InvalidPosition)?;
                let before = std::mem::replace(slot, block.clone());
                self.text_map_cache.clear();
                self.advance_history_merge_epoch();
                self.record_math_history(DocumentPatch::BlockRange {
                    parent_environment_ids: parent_ids,
                    sibling_index: index,
                    before: vec![before],
                    after: vec![block],
                });
            }
        }
        if let Some(current) = &mut self.math_edit {
            current.source = source;
            current.cursor = cursor;
        }
        let changed_paragraph_id = match &edit.target {
            MathEditTarget::Inline { paragraph_id, .. } => Some(paragraph_id.clone()),
            MathEditTarget::Block { .. } => None,
        };
        Ok(Effect {
            document_changed: true,
            changed_paragraph_id,
            ..Effect::default()
        })
    }

    fn empty_construct_range(&self, edit: &MathEdit) -> Option<Range<usize>> {
        fn containing_construct(node: &MathNode, cursor: usize) -> Option<Range<usize>> {
            let has_empty_argument = node.children.iter().any(|child| {
                child.kind == caret_doc::MathNodeKind::Placeholder
                    && child.source_range.as_ref().is_some_and(|range| {
                        range.start as usize + 1 == cursor && range.end as usize == cursor + 1
                    })
            });
            if has_empty_argument
                && matches!(
                    node.kind,
                    caret_doc::MathNodeKind::Fraction
                        | caret_doc::MathNodeKind::Radical
                        | caret_doc::MathNodeKind::Superscript
                        | caret_doc::MathNodeKind::Subscript
                        | caret_doc::MathNodeKind::SubSup
                )
            {
                return node
                    .source_range
                    .as_ref()
                    .map(|range| range.start as usize..range.end as usize);
            }
            node.children
                .iter()
                .find_map(|child| containing_construct(child, cursor))
        }

        let tree = match &edit.target {
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
                find_inline_math(&self.paragraph_for(&position).ok()?.inlines, math_id)
                    .map(|math| &math.math_tree)
            }
            MathEditTarget::Block { block_id, .. } => {
                find_block_path(&self.document.body, block_id).and_then(|(parents, index)| {
                    block_list_ref(&self.document.body, &parents)
                        .ok()?
                        .get(index)
                        .and_then(|block| match block {
                            Block::Equation(value) => Some(&value.math_tree),
                            Block::Align(_) => None,
                            _ => None,
                        })
                })
            }
        }?;
        containing_construct(&tree.root, edit.cursor)
    }

    fn record_math_history(&mut self, patch: DocumentPatch) {
        let after_state_id = self.allocate_history_state_id();
        let before_state = SessionHistoryState::new(
            self.selection.clone(),
            self.selection_visual_affinity,
            self.pending_inline_style.clone(),
            self.history_state_id,
        );
        let after_state = SessionHistoryState::new(
            self.selection.clone(),
            self.selection_visual_affinity,
            self.pending_inline_style.clone(),
            after_state_id,
        );
        self.record_history_edit(SessionHistoryEdit::new(patch, before_state, after_state));
        self.history_state_id = after_state_id;
        self.advance_document_generation();
    }
}

/// 段落内每个行内公式的起始 UTF-8 偏移与节点 ID，按文档顺序。
pub(crate) fn inline_math_slots(paragraph: &Paragraph) -> Result<Vec<(usize, NodeId)>, EditError> {
    fn visit(
        nodes: &[InlineNode],
        offset: &mut usize,
        output: &mut Vec<(usize, NodeId)>,
    ) -> Result<(), EditError> {
        for node in nodes {
            match node {
                InlineNode::Environment(environment) => {
                    visit(&environment.children, offset, output)?
                }
                InlineNode::Math(math) => {
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

fn find_inline_math<'a>(nodes: &'a [InlineNode], id: &NodeId) -> Option<&'a InlineMath> {
    nodes.iter().find_map(|node| match node {
        InlineNode::Math(math) if &math.id == id => Some(math),
        InlineNode::Environment(environment) => find_inline_math(&environment.children, id),
        _ => None,
    })
}

fn find_inline_math_mut<'a>(
    nodes: &'a mut [InlineNode],
    id: &NodeId,
) -> Option<&'a mut InlineMath> {
    nodes.iter_mut().find_map(|node| match node {
        InlineNode::Math(math) if &math.id == id => Some(math),
        InlineNode::Environment(environment) => find_inline_math_mut(&mut environment.children, id),
        _ => None,
    })
}

/// 在块树中查找块 ID，返回父环境 ID 路径与同级下标。
pub(crate) fn find_block_path(blocks: &[Block], id: &NodeId) -> Option<(Vec<NodeId>, usize)> {
    for (index, block) in blocks.iter().enumerate() {
        if block_id(block) == Some(id) {
            return Some((Vec::new(), index));
        }
        if let Block::Environment(environment) = block
            && let Some((mut path, child)) = find_block_path(&environment.children, id)
        {
            path.insert(0, environment.id.clone());
            return Some((path, child));
        }
    }
    None
}

fn block_id(block: &Block) -> Option<&NodeId> {
    match block {
        Block::Paragraph(value) => Some(&value.id),
        Block::Environment(value) => Some(&value.id),
        Block::Equation(value) => Some(&value.id),
        Block::Align(value) => Some(&value.id),
        Block::ExplicitBreak(_) | Block::Opaque(_) => None,
    }
}

fn block_list<'a>(
    blocks: &'a mut Vec<Block>,
    parent_ids: &[NodeId],
) -> Result<&'a mut Vec<Block>, EditError> {
    let Some((first, rest)) = parent_ids.split_first() else {
        return Ok(blocks);
    };
    let environment = blocks
        .iter_mut()
        .find_map(|block| match block {
            Block::Environment(environment) if &environment.id == first => Some(environment),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    block_list(&mut environment.children, rest)
}

fn block_list_ref<'a>(
    blocks: &'a [Block],
    parent_ids: &[NodeId],
) -> Result<&'a [Block], EditError> {
    let Some((first, rest)) = parent_ids.split_first() else {
        return Ok(blocks);
    };
    let environment = blocks
        .iter()
        .find_map(|block| match block {
            Block::Environment(environment) if &environment.id == first => Some(environment),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    block_list_ref(&environment.children, rest)
}

fn preserve_display_math_metadata(original: &Block, updated: &mut Block) {
    match (original, updated) {
        (Block::Equation(original), Block::Equation(updated)) => {
            updated.extra.clone_from(&original.extra);
            preserve_math_tree_metadata(&original.math_tree, &mut updated.math_tree);
        }
        (Block::Align(original), Block::Align(updated)) => {
            preserve_align_metadata(original, updated);
        }
        _ => {}
    }
}

fn preserve_align_metadata(previous: &Align, updated: &mut Align) {
    updated.extra.clone_from(&previous.extra);
    let same_row_count = previous.rows.len() == updated.rows.len();
    let mut previous_by_signature = HashMap::<Vec<String>, VecDeque<usize>>::new();
    for (index, row) in previous.rows.iter().enumerate() {
        previous_by_signature
            .entry(align_row_signature(row))
            .or_default()
            .push_back(index);
    }
    let mut matched_previous = HashSet::new();
    for (updated_index, new_row) in updated.rows.iter_mut().enumerate() {
        let matched_index = if same_row_count {
            Some(updated_index)
        } else {
            previous_by_signature
                .get_mut(&align_row_signature(new_row))
                .and_then(VecDeque::pop_front)
        };
        let Some(previous_index) = matched_index.filter(|index| matched_previous.insert(*index))
        else {
            continue;
        };
        let Some(old_row) = previous.rows.get(previous_index) else {
            continue;
        };
        new_row.id.clone_from(&old_row.id);
        new_row.extra.clone_from(&old_row.extra);
        if same_row_count && old_row.cells.len() == new_row.cells.len() {
            for (old_cell, new_cell) in old_row.cells.iter().zip(&mut new_row.cells) {
                preserve_math_tree_metadata(old_cell, new_cell);
            }
        } else {
            preserve_unchanged_align_cells(old_row, new_row);
        }
    }
}

fn align_row_signature(row: &caret_doc::MathAlignRow) -> Vec<String> {
    row.cells.iter().map(|cell| cell.source.clone()).collect()
}

fn preserve_unchanged_align_cells(
    previous: &caret_doc::MathAlignRow,
    updated: &mut caret_doc::MathAlignRow,
) {
    let mut previous_by_source = HashMap::<&str, VecDeque<usize>>::new();
    for (index, cell) in previous.cells.iter().enumerate() {
        previous_by_source
            .entry(&cell.source)
            .or_default()
            .push_back(index);
    }
    let mut matched_previous = HashSet::new();
    for new_cell in &mut updated.cells {
        let Some(previous_index) = previous_by_source
            .get_mut(new_cell.source.as_str())
            .and_then(VecDeque::pop_front)
            .filter(|index| matched_previous.insert(*index))
        else {
            continue;
        };
        if let Some(old_cell) = previous.cells.get(previous_index) {
            preserve_math_tree_metadata(old_cell, new_cell);
        }
    }
}

/// Reuses identity and optional data only when a node's unchanged source span maps uniquely.
/// Nodes overlapping the edited source range receive fresh metadata from the parser.
fn preserve_math_tree_metadata(previous: &MathTree, updated: &mut MathTree) {
    updated.extra.clone_from(&previous.extra);
    let (prefix_end, previous_tail, updated_tail) =
        unchanged_source_boundaries(&previous.source, &updated.source);
    preserve_math_node_metadata(
        &previous.root,
        &mut updated.root,
        prefix_end,
        previous_tail,
        updated_tail,
    );
}

fn preserve_math_node_metadata(
    previous: &MathNode,
    updated: &mut MathNode,
    prefix_end: usize,
    previous_tail: usize,
    updated_tail: usize,
) {
    if updated.kind == previous.kind {
        updated.id.clone_from(&previous.id);
        updated.extra.clone_from(&previous.extra);
        if let (Some(previous_range), Some(updated_range)) =
            (&previous.source_range, &mut updated.source_range)
        {
            updated_range.extra.clone_from(&previous_range.extra);
        }
    }
    let mut matched_previous = HashSet::new();
    for child in &mut updated.children {
        let mapped_match = child.source_range.as_ref().and_then(|range| {
            let expected = (range.start as usize, range.end as usize);
            previous
                .children
                .iter()
                .enumerate()
                .find_map(|(index, candidate)| {
                    (!matched_previous.contains(&index)
                        && candidate.kind == child.kind
                        && mapped_source_range(candidate, prefix_end, previous_tail, updated_tail)
                            == Some(expected))
                    .then_some((index, candidate))
                })
        });
        let containing_match = child.source_range.as_ref().and_then(|range| {
            let start = range.start as usize;
            let end = range.end as usize;
            if start > prefix_end || end < updated_tail {
                return None;
            }
            let mut candidates =
                previous
                    .children
                    .iter()
                    .enumerate()
                    .filter(|(index, candidate)| {
                        !matched_previous.contains(index)
                            && candidate.kind == child.kind
                            && candidate.source_range.as_ref().is_some_and(|old_range| {
                                old_range.start as usize <= prefix_end
                                    && old_range.end as usize >= previous_tail
                            })
                    });
            let candidate = candidates.next()?;
            candidates.next().is_none().then_some(candidate)
        });
        if let Some((index, matched)) = mapped_match.or(containing_match) {
            matched_previous.insert(index);
            preserve_math_node_metadata(matched, child, prefix_end, previous_tail, updated_tail);
        }
    }
}

fn mapped_source_range(
    node: &MathNode,
    prefix_end: usize,
    previous_tail: usize,
    updated_tail: usize,
) -> Option<(usize, usize)> {
    let range = node.source_range.as_ref()?;
    let start = range.start as usize;
    let end = range.end as usize;
    if end <= prefix_end {
        return Some((start, end));
    }
    if start >= previous_tail {
        let shift = updated_tail as isize - previous_tail as isize;
        return Some((
            start.checked_add_signed(shift)?,
            end.checked_add_signed(shift)?,
        ));
    }
    None
}

fn unchanged_source_boundaries(previous: &str, updated: &str) -> (usize, usize, usize) {
    let mut prefix_end = 0;
    for (old, new) in previous.chars().zip(updated.chars()) {
        if old != new {
            break;
        }
        prefix_end += old.len_utf8();
    }
    let mut previous_tail = previous.len();
    let mut updated_tail = updated.len();
    let mut old_chars = previous[prefix_end..].chars().rev();
    let mut new_chars = updated[prefix_end..].chars().rev();
    loop {
        match (old_chars.next(), new_chars.next()) {
            (Some(old), Some(new)) if old == new => {
                previous_tail -= old.len_utf8();
                updated_tail -= new.len_utf8();
            }
            _ => break,
        }
    }
    (prefix_end, previous_tail, updated_tail)
}

fn display_math_block(align: bool, source: &str) -> Result<Block, EditError> {
    display_math_block_with_id(align, source, NodeId::generate())
}

fn display_math_block_with_id(align: bool, source: &str, id: NodeId) -> Result<Block, EditError> {
    if align {
        let full = format!("{ALIGN_BEGIN}{source}{ALIGN_END}");
        let caret_math::ParsedMath::Align(parsed) =
            caret_math::parse(&full, caret_math::MathInputKind::Align)
                .map_err(|error| EditError::InvalidMath(error.to_string()))?
        else {
            return Err(EditError::InvalidMath("expected aligned math".to_owned()));
        };
        return Ok(Block::Align(Align { id, ..parsed }));
    }
    let caret_math::ParsedMath::Tree(math_tree) =
        caret_math::parse(source, caret_math::MathInputKind::Equation)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
    else {
        return Err(EditError::InvalidMath("expected equation".to_owned()));
    };
    Ok(Block::Equation(Equation {
        id,
        math_tree,
        extra: Default::default(),
    }))
}

/// 对齐公式完整源码中 `aligned` 环境的内部；不是该形式时整体返回。
fn align_body(source: &str) -> String {
    let trimmed = source.trim();
    trimmed
        .strip_prefix(ALIGN_BEGIN)
        .and_then(|rest| rest.strip_suffix(ALIGN_END))
        .unwrap_or(trimmed)
        .to_owned()
}

fn math_input_body_range(edit: &MathEdit) -> Option<Range<usize>> {
    let kind = match &edit.target {
        MathEditTarget::Inline { .. } => Some(caret_math::MathInputKind::Inline),
        MathEditTarget::Block { align: false, .. } => Some(caret_math::MathInputKind::Equation),
        MathEditTarget::Block { align: true, .. } => None,
    };
    kind.map_or_else(
        || Some(0..edit.source.len()),
        |kind| caret_math::input_body_range(&edit.source, kind).ok(),
    )
}

/// 对齐源码中顶层的 `\\` 与 `&` 切分出单元格，返回光标所在 (行, 单元格, 去首空白后的单元格内偏移)。
fn align_cell_at(source: &str, cursor: usize) -> Option<(usize, usize, usize)> {
    let mut row = 0;
    let mut cell = 0;
    let mut cell_start = 0;
    let mut depth = 0usize;
    let mut environments = Vec::<&str>::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() && index < cursor {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'%' => {
                index = source[index..]
                    .find('\n')
                    .map_or(bytes.len(), |offset| index + offset);
                continue;
            }
            b'\\' if bytes.get(index + 1) == Some(&b'\\') => {
                if depth == 0 && environments.is_empty() {
                    if index + 2 > cursor {
                        break;
                    }
                    row += 1;
                    cell = 0;
                    cell_start = index + 2;
                }
                index += 2;
                continue;
            }
            b'\\' => {
                let command_start = index + 1;
                let mut command_end = command_start;
                while bytes.get(command_end).is_some_and(u8::is_ascii_alphabetic) {
                    command_end += 1;
                }
                if command_end == command_start {
                    index = (index + 2).min(bytes.len());
                    continue;
                }
                let command = &source[command_start..command_end];
                if matches!(command, "begin" | "end") {
                    let mut name_start = command_end;
                    while bytes.get(name_start).is_some_and(u8::is_ascii_whitespace) {
                        name_start += 1;
                    }
                    if bytes.get(name_start) == Some(&b'{')
                        && let Some(relative_end) = source[name_start + 1..].find('}')
                    {
                        let name_end = name_start + 1 + relative_end;
                        let name = &source[name_start + 1..name_end];
                        if command == "begin" {
                            environments.push(name);
                        } else if environments.last().copied() == Some(name) {
                            environments.pop();
                        }
                        index = name_end + 1;
                        continue;
                    }
                }
                index = command_end;
                continue;
            }
            b'&' if depth == 0 && environments.is_empty() => {
                cell += 1;
                cell_start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    let cell_text = source.get(cell_start..cursor.max(cell_start))?;
    let leading = cell_text.len() - cell_text.trim_start().len();
    Some((row, cell, cursor.saturating_sub(cell_start + leading)))
}

fn previous_grapheme_start(source: &str, cursor: usize) -> Option<usize> {
    source
        .get(..cursor)?
        .grapheme_indices(true)
        .next_back()
        .map(|(start, _)| start)
}

fn next_grapheme_end(source: &str, cursor: usize) -> Option<usize> {
    source
        .get(cursor..)?
        .graphemes(true)
        .next()
        .map(|grapheme| cursor + grapheme.len())
}

fn previous_math_caret_stop(source: &str, cursor: usize) -> Option<usize> {
    let prefix = source.get(..cursor)?;
    if prefix.ends_with("}{") {
        return Some(cursor - 2);
    }
    let Some(command_start) = prefix.rfind('\\') else {
        return previous_grapheme_start(source, cursor);
    };
    let suffix = prefix.get(command_start + 1..)?;
    if !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Some(command_start);
    }
    previous_grapheme_start(source, cursor)
}

fn next_math_caret_stop(source: &str, cursor: usize) -> Option<usize> {
    let suffix = source.get(cursor..)?;
    if suffix.starts_with("}{") {
        return Some(cursor + 2);
    }
    if suffix.starts_with('\\') {
        let mut end = cursor + 1;
        while source
            .as_bytes()
            .get(end)
            .is_some_and(u8::is_ascii_alphabetic)
        {
            end += 1;
        }
        if end > cursor + 1 {
            return Some(end);
        }
        return next_grapheme_end(source, cursor);
    }
    if suffix.starts_with('}') {
        let mut end = cursor;
        while source.as_bytes().get(end) == Some(&b'}') {
            end += 1;
        }
        return (end < source.len()).then_some(end);
    }
    next_grapheme_end(source, cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_cursor_uses_the_same_grapheme_boundaries_as_text() {
        let source = "\\alpha a\u{301}β";
        let mut stops = vec![0];
        let mut cursor = 0;
        while let Some(next) = next_grapheme_end(source, cursor) {
            stops.push(next);
            cursor = next;
        }
        assert_eq!(stops, vec![0, 1, 2, 3, 4, 5, 6, 7, 10, 12]);
        assert_eq!(previous_grapheme_start(source, 10), Some(7));
    }

    #[test]
    fn arrow_treats_a_rendered_control_symbol_as_one_caret_stop() -> Result<(), EditError> {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.run(Command::InsertInlineMath(r"\alpha".to_owned()))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(6));

        session.run(Command::MoveCaret(Direction::Backward))?;
        assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
        Ok(())
    }

    #[test]
    fn backspace_deletes_one_grapheme_from_formula_source() -> Result<(), EditError> {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.run(Command::InsertInlineMath("abc".to_owned()))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        session.run(Command::Delete(Direction::Backward))?;

        assert_eq!(inline_source(&session).as_deref(), Some("ab"));
        assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
        Ok(())
    }

    #[test]
    fn align_cell_mapping_counts_rows_and_cells() {
        assert_eq!(align_cell_at("a&=b \\\\ c& =d", 12), Some((1, 1, 1)));
        assert_eq!(align_cell_at("a&=b", 1), Some((0, 0, 1)));
    }

    #[test]
    fn align_cell_mapping_ignores_nested_environment_separators() {
        let source = r"\begin{matrix}a&b\\c&d\end{matrix}&=z";
        let nested_cell = source
            .find("c&d")
            .unwrap_or_else(|| panic!("missing nested cell"))
            + 2;
        assert_eq!(
            align_cell_at(source, nested_cell),
            Some((0, 0, nested_cell))
        );

        let outer_cell = source
            .rfind('&')
            .unwrap_or_else(|| panic!("missing outer separator"))
            + 1;
        assert_eq!(align_cell_at(source, outer_cell), Some((0, 1, 0)));
    }

    #[test]
    fn align_cell_mapping_ignores_separators_in_comments() {
        let source = "a % & ignored \\\\ ignored\n+b&c";
        let in_comment = source
            .find("ignored")
            .unwrap_or_else(|| panic!("missing comment"))
            + 3;
        assert_eq!(align_cell_at(source, in_comment), Some((0, 0, in_comment)));
        let after_outer_separator = source
            .rfind('&')
            .unwrap_or_else(|| panic!("missing separator"))
            + 2;
        assert_eq!(
            align_cell_at(source, after_outer_separator),
            Some((0, 1, 1))
        );
    }

    #[test]
    fn preserving_math_metadata_keeps_tree_and_unchanged_node_identity() {
        let mut previous = parsed_inline_tree("a+b");
        previous
            .extra
            .insert("futureTreeValue".to_owned(), serde_json::json!(1));
        let unchanged_id = previous.root.children[0].id.clone();
        previous.root.children[0]
            .extra
            .insert("futureNodeValue".to_owned(), serde_json::json!("kept"));

        let mut updated = parsed_inline_tree("a+bc");
        preserve_math_tree_metadata(&previous, &mut updated);

        assert_eq!(updated.extra["futureTreeValue"], serde_json::json!(1));
        assert_eq!(updated.root.children[0].id, unchanged_id);
        assert_eq!(
            updated.root.children[0].extra["futureNodeValue"],
            serde_json::json!("kept")
        );
    }

    #[test]
    fn reparsing_unranged_math_nodes_never_reuses_an_identity_twice() -> Result<(), EditError> {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.run(Command::InsertInlineMath(r"\sum".to_owned()))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        session.run(Command::InsertText(" ".to_owned()))?;

        session.document().validate().map_err(|error| {
            EditError::InvalidDocument(format!("math node IDs should remain unique: {error}"))
        })?;
        crate::validate_document_math_sources(session.document())
            .map_err(EditError::InvalidDocument)
    }

    #[test]
    fn preserving_math_metadata_keeps_the_parent_of_an_edited_subtree() {
        let old_source = "\\frac{ab}{c}";
        let new_source = "\\frac{axb}{c}";
        let mut previous = parsed_inline_tree(old_source);
        let (prefix_end, previous_tail, _) = unchanged_source_boundaries(old_source, new_source);
        let Some(changed_parent) = previous.root.children.iter_mut().find(|node| {
            node.source_range.as_ref().is_some_and(|range| {
                range.start as usize <= prefix_end && range.end as usize >= previous_tail
            })
        }) else {
            panic!("fraction operand should contain the edit");
        };
        let changed_parent_id = changed_parent.id.clone();
        changed_parent
            .extra
            .insert("futureOperandValue".to_owned(), serde_json::json!(true));

        let mut updated = parsed_inline_tree(new_source);
        preserve_math_tree_metadata(&previous, &mut updated);

        let preserved = updated
            .root
            .children
            .iter()
            .find(|node| node.extra.contains_key("futureOperandValue"));
        assert!(preserved.is_some_and(|node| {
            node.id == changed_parent_id && node.extra["futureOperandValue"] == true
        }));
    }

    use crate::editing::{CommandLineMode, EditorKey};
    use caret_doc::{Document, InlineEnvironmentKind, Run};
    use std::collections::BTreeMap;

    fn parsed_inline_tree(source: &str) -> MathTree {
        match caret_math::parse(source, caret_math::MathInputKind::Inline) {
            Ok(caret_math::ParsedMath::Tree(tree)) => tree,
            Ok(caret_math::ParsedMath::Align(_)) => panic!("expected inline math tree"),
            Err(error) => panic!("test formula should parse: {error}"),
        }
    }

    fn session_with_text(text: &str) -> Result<Session, Box<dyn std::error::Error>> {
        let mut paragraph = Paragraph::empty(NodeId::generate());
        if !text.is_empty() {
            paragraph.inlines.push(InlineNode::Run(Run {
                text: text.to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }));
        }
        let mut document = Document::blank();
        document.body = vec![Block::Paragraph(paragraph)];
        let mut session = Session::new(document)?;
        if !text.is_empty() {
            for _ in 0..text.chars().count() {
                session.run(Command::MoveCaret(Direction::Forward))?;
            }
        }
        Ok(session)
    }

    fn accept(session: &mut Session, query: &str) -> Result<Effect, EditError> {
        session.run(Command::BeginCommandLine)?;
        session.run(Command::TypeCommandLine(query.to_owned()))?;
        session.run(Command::AcceptCommandLine)
    }

    fn inline_source(session: &Session) -> Option<String> {
        let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
            return None;
        };
        paragraph.inlines.iter().find_map(|node| match node {
            InlineNode::Math(math) => Some(math.math_tree.source.clone()),
            _ => None,
        })
    }

    #[test]
    fn math_command_inserts_inline_math_and_sub_commands_edit_it()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut session = session_with_text("a")?;
        accept(&mut session, "math")?;
        assert!(matches!(
            session.math_edit().map(|edit| &edit.target),
            Some(MathEditTarget::Inline { .. })
        ));
        session.run(Command::InsertText("x^2".to_owned()))?;
        assert_eq!(inline_source(&session).as_deref(), Some("x^{2}"));
        session.run(Command::InsertText("}".to_owned()))?;
        session.run(Command::BeginCommandLine)?;
        assert_eq!(
            session.command_line().map(|line| line.mode),
            Some(CommandLineMode::Math)
        );
        session.run(Command::TypeCommandLine("frac".to_owned()))?;
        session.run(Command::AcceptCommandLine)?;
        assert_eq!(inline_source(&session).as_deref(), Some("x^{2}\\frac{}{}"));
        assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(11));
        session.run(Command::InsertText("1".to_owned()))?;
        session.run(Command::MoveCaret(Direction::Forward))?;
        session.run(Command::InsertText("n".to_owned()))?;
        assert_eq!(
            inline_source(&session).as_deref(),
            Some("x^{2}\\frac{1}{n}")
        );
        session.run(Command::EditorKey(EditorKey::Escape))?;
        assert!(session.math_edit().is_none());
        assert_eq!(session.selection().focus.offset, 4);
        // 撤销是正文命令：退出公式编辑后按步恢复源码。
        session.run(Command::Undo)?;
        assert_eq!(inline_source(&session).as_deref(), Some("x^{2}\\frac{1}{}"));
        Ok(())
    }

    #[test]
    fn entering_fenced_inline_math_edits_inside_the_fences_from_both_sides()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut from_left = session_with_text("")?;
        from_left.run(Command::InsertInlineMath("$x$".to_owned()))?;
        let paragraph_id = from_left.selection().focus.paragraph_id.clone();
        let left = Pos {
            story: StoryId::Body,
            paragraph_id: paragraph_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        from_left.run(Command::SetSelection(Some(Selection {
            anchor: left.clone(),
            focus: left,
        })))?;
        from_left.run(Command::EnterMath(Direction::Forward))?;
        assert_eq!(from_left.math_edit().map(|edit| edit.cursor), Some(1));
        from_left.run(Command::InsertText("y".to_owned()))?;
        assert_eq!(inline_source(&from_left).as_deref(), Some("$yx$"));

        let mut from_right = session_with_text("")?;
        from_right.run(Command::InsertInlineMath(r"\(x\)".to_owned()))?;
        from_right.run(Command::EnterMath(Direction::Backward))?;
        assert_eq!(from_right.math_edit().map(|edit| edit.cursor), Some(3));
        from_right.run(Command::InsertText("y".to_owned()))?;
        assert_eq!(inline_source(&from_right).as_deref(), Some(r"\(xy\)"));
        from_right.run(Command::MoveCaret(Direction::Forward))?;
        assert!(from_right.math_edit().is_none());

        let mut from_left = session_with_text("")?;
        from_left.run(Command::InsertInlineMath(r"\(x\)".to_owned()))?;
        let paragraph_id = from_left.selection().focus.paragraph_id.clone();
        let left = Pos {
            story: StoryId::Body,
            paragraph_id,
            offset: 0,
            affinity: Affinity::Downstream,
        };
        from_left.run(Command::SetSelection(Some(Selection {
            anchor: left.clone(),
            focus: left,
        })))?;
        from_left.run(Command::EnterMath(Direction::Forward))?;
        assert_eq!(from_left.math_edit().map(|edit| edit.cursor), Some(2));
        from_left.run(Command::MoveCaret(Direction::Backward))?;
        assert!(from_left.math_edit().is_none());
        Ok(())
    }

    #[test]
    fn backspace_deletes_an_empty_fenced_inline_formula() -> Result<(), EditError> {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.run(Command::InsertInlineMath("$ $".to_owned()))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        session.run(Command::Delete(Direction::Backward))?;

        assert!(inline_source(&session).is_none());
        assert!(session.math_edit().is_none());
        Ok(())
    }

    #[test]
    fn rejecting_missing_block_selection_keeps_active_math_edit() -> Result<(), EditError> {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        accept(&mut session, "math")?;
        let before = session.math_edit().cloned();
        let generation = session.document_generation();

        assert_eq!(
            session.select_block_environment(NodeId::generate()),
            Err(EditError::InvalidPosition)
        );
        assert_eq!(session.math_edit(), before.as_ref());
        assert_eq!(session.document_generation(), generation);
        Ok(())
    }

    #[test]
    fn backspace_removes_empty_construct_and_then_the_object()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut session = session_with_text("")?;
        accept(&mut session, "math")?;
        session.run(Command::BeginCommandLine)?;
        session.run(Command::TypeCommandLine("sqrt".to_owned()))?;
        session.run(Command::AcceptCommandLine)?;
        assert_eq!(inline_source(&session).as_deref(), Some("\\sqrt{}"));
        session.run(Command::Delete(Direction::Backward))?;
        assert_eq!(inline_source(&session).as_deref(), Some(""));
        session.run(Command::Delete(Direction::Backward))?;
        assert!(session.math_edit().is_none());
        assert_eq!(inline_source(&session), None);
        Ok(())
    }

    #[test]
    fn greek_sub_commands_are_case_sensitive() -> Result<(), Box<dyn std::error::Error>> {
        let mut session = session_with_text("")?;
        accept(&mut session, "math")?;
        accept(&mut session, "Gamma")?;
        accept(&mut session, "gamma")?;
        assert_eq!(inline_source(&session).as_deref(), Some("\\Gamma\\gamma"));
        Ok(())
    }

    #[test]
    fn equation_command_inserts_block_after_text_and_round_trips()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut session = session_with_text("ab")?;
        accept(&mut session, "equation")?;
        assert_eq!(session.document().body.len(), 3);
        session.run(Command::InsertText("a+b".to_owned()))?;
        let Some(Block::Equation(equation)) = session.document().body.get(1) else {
            panic!("expected equation block");
        };
        assert_eq!(equation.math_tree.source, "a+b");
        session.run(Command::EditorKey(EditorKey::Enter))?;
        assert!(session.math_edit().is_none());
        session.run(Command::MoveCaret(Direction::Backward))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
        for _ in 0..4 {
            session.run(Command::Delete(Direction::Backward))?;
        }
        assert!(session.math_edit().is_none());
        assert_eq!(session.document().body.len(), 2);
        session.run(Command::Undo)?;
        assert_eq!(session.document().body.len(), 3);
        Ok(())
    }

    #[test]
    fn align_enter_inserts_row_break() -> Result<(), Box<dyn std::error::Error>> {
        let mut session = session_with_text("")?;
        accept(&mut session, "align")?;
        session.run(Command::InsertText("a&=b".to_owned()))?;
        session.run(Command::EditorKey(EditorKey::Enter))?;
        session.run(Command::InsertText("c&=d".to_owned()))?;
        let Some(Block::Align(value)) = session.document().body.first() else {
            panic!("expected align block");
        };
        assert_eq!(value.rows.len(), 2);
        assert_eq!(
            session.math_edit().and_then(MathEdit::align_cell_at_cursor),
            Some((1, 1, 2))
        );
        Ok(())
    }

    #[test]
    fn invalid_source_is_rejected_without_changes() -> Result<(), Box<dyn std::error::Error>> {
        let mut session = session_with_text("")?;
        accept(&mut session, "math")?;
        session.run(Command::InsertText("x".to_owned()))?;
        let result = session.run(Command::InsertText("\\".to_owned()));
        assert!(matches!(result, Err(EditError::InvalidMath(_))));
        assert_eq!(inline_source(&session).as_deref(), Some("x"));
        assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
        Ok(())
    }

    #[test]
    fn unmatched_closing_brace_is_not_silently_discarded() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut session = session_with_text("")?;
        accept(&mut session, "math")?;
        let generation = session.document_generation();
        let result = session.run(Command::PasteText("x}y".to_owned()));

        assert!(matches!(result, Err(EditError::InvalidMath(_))));
        assert_eq!(inline_source(&session).as_deref(), Some(""));
        assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
        assert_eq!(session.document_generation(), generation);
        Ok(())
    }

    #[test]
    fn rejected_body_command_keeps_the_active_math_edit_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut session = session_with_text("")?;
        accept(&mut session, "math")?;
        let target_before = session.math_edit().cloned();
        let result = session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Font));
        assert!(matches!(result, Err(EditError::UnsupportedInlineFormat)));
        assert_eq!(session.math_edit(), target_before.as_ref());
        Ok(())
    }

    #[test]
    fn invalid_structure_selection_keeps_the_active_math_edit_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut session = session_with_text("")?;
        accept(&mut session, "math")?;
        let target_before = session.math_edit().cloned();
        let position = session.selection().focus.clone();
        let result = session.select_block_environments_from_range(
            position.clone(),
            position,
            crate::VisualAffinity::Downstream,
        );
        assert!(matches!(result, Err(EditError::InvalidPosition)));
        assert_eq!(session.math_edit(), target_before.as_ref());
        Ok(())
    }
}
