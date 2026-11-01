//! 公式编辑状态：在行内公式或显示公式块内部按 LaTeX 源码编辑，并在每次修改后重新解析。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::ops::Range;

use super::super::paragraph_ops::build_paragraph_index;
#[cfg(test)]
use metadata::{preserve_align_metadata, preserve_math_tree_metadata, unchanged_source_boundaries};
use metadata::{preserve_display_math_metadata, preserve_math_tree_metadata_for_edit};
use navigation::{
    align_cell_at, math_input_body_range, next_grapheme_end, next_math_caret_stop,
    previous_grapheme_start, previous_math_caret_stop,
};

mod align_source;
mod metadata;
mod navigation;
mod path;
mod text_branch;
use super::super::text_map::{OBJECT_REPLACEMENT, TextMap, inline_length, split_inline_nodes};
use super::super::{
    Affinity, Command, CommandLine, Direction, EditError, EditorKey, Effect, Pos, Selection,
    StoryId,
};
use super::Session;
use super::history::{
    self, DocumentPatch, EditGroup, MathEditPosition, SessionHistoryEdit, SessionHistoryState,
};
use crate::{StructureIndex, VisualAffinity};
#[cfg(test)]
use caret_doc::MathTree;
use caret_doc::{
    Align, Block, Equation, InlineMath, MathContent, MathFlow, MathNode, NodeId, Paragraph,
    ParagraphContent,
};
pub use path::{MathTextFlowPath, MathTextFlowPathStep};
use std::sync::Arc;
use text_branch::{
    MathTextEdit, is_grapheme_boundary, replace_text_flow_range, snap_text_cursor,
    validate_text_flow_edit,
};

pub(super) const ALIGN_BEGIN: &str = "\\begin{aligned}";
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

/// 数学编辑器当前操作的文本载体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathEditBranch {
    /// 编辑 LaTeX 源码；`MathEdit.cursor` 是源码 UTF-8 字节偏移。
    Source,
    /// 编辑公式树内的 TextFlow；光标和锚点按拼接文本的 UTF-8 字节计数。
    Text {
        path: MathTextFlowPath,
        anchor: usize,
    },
}

/// 公式编辑状态：源码树、当前编辑载体和载体内 UTF-8 光标。不属于文档修订。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathEdit {
    pub target: MathEditTarget,
    pub source: String,
    pub cursor: usize,
    /// 光标所属的数学树分支；源码和 TextFlow 分别使用各自的字节坐标。
    pub branch: MathEditBranch,
}

impl MathEdit {
    /// 对齐公式中光标所在的行、单元格及单元格内偏移；单元格源码已去掉首尾空白。
    pub fn align_cell_at_cursor(&self) -> Option<(usize, usize, usize)> {
        if !matches!(self.target, MathEditTarget::Block { align: true, .. })
            || !matches!(self.branch, MathEditBranch::Source)
        {
            return None;
        }
        align_cell_at(&self.source, self.cursor)
    }
}

pub(super) fn restore_math_edit(
    document: &caret_doc::Document,
    position: MathEditPosition,
) -> Option<MathEdit> {
    let source = match &position.target {
        MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } => find_inline_math_in_blocks(&document.body, paragraph_id, math_id)?
            .math_tree
            .source
            .clone(),
        MathEditTarget::Block { block_id, align } => {
            match (find_block_by_id(&document.body, block_id)?, align) {
                (Block::Equation(equation), false) => equation.math_tree.source.clone(),
                (Block::Align(value), true) => align_body(&value.source),
                _ => return None,
            }
        }
    };
    match &position.branch {
        MathEditBranch::Source => {
            if position.cursor > source.len() || !source.is_char_boundary(position.cursor) {
                return None;
            }
        }
        MathEditBranch::Text { path, anchor } => {
            let tree = math_tree_for_target(document, &position.target, path.align_cell())?;
            let text = path.resolve(tree)?;
            if host_source_range_for_path(document, &position.target, path)?
                != path.host_source_range()
            {
                return None;
            }
            let display_text = text
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>();
            if position.cursor > display_text.len()
                || *anchor > display_text.len()
                || !display_text.is_char_boundary(position.cursor)
                || !display_text.is_char_boundary(*anchor)
                || !is_grapheme_boundary(&display_text, position.cursor)
                || !is_grapheme_boundary(&display_text, *anchor)
            {
                return None;
            }
        }
    }
    Some(MathEdit {
        target: position.target,
        source,
        cursor: position.cursor,
        branch: position.branch,
    })
}

fn host_source_range_for_path(
    document: &caret_doc::Document,
    target: &MathEditTarget,
    path: &MathTextFlowPath,
) -> Option<(usize, usize)> {
    let (start, end) = path.source_range();
    let base = match (target, path.align_cell()) {
        (MathEditTarget::Inline { .. }, None)
        | (MathEditTarget::Block { align: false, .. }, None) => 0,
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
            align_source::cell_source_ranges(align)?
                .get(row)?
                .get(cell)?
                .start
                .checked_sub(align_source::align_body_range(&align.source)?.start)?
        }
        _ => return None,
    };
    Some((base + start as usize, base + end as usize))
}

fn math_tree_for_target<'a>(
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

pub(super) fn history_position(edit: &MathEdit) -> MathEditPosition {
    MathEditPosition::with_branch(edit.target.clone(), edit.cursor, edit.branch.clone())
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
        _ => None,
    })
}

fn find_block_by_id<'a>(blocks: &'a [Block], id: &NodeId) -> Option<&'a Block> {
    let (parents, index) = find_block_path(blocks, id)?;
    block_list_ref(blocks, &parents).ok()?.get(index)
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
        if self.composition.is_some()
            || matches!(
                command,
                Command::UpdateComposition { .. }
                    | Command::CommitComposition { .. }
                    | Command::CancelComposition { .. }
            )
        {
            return Err(EditError::CompositionPending);
        }
        if self.math_edit_is_text_branch() && matches!(command, Command::BeginCommandLine) {
            return Err(EditError::UnknownCommand);
        }
        if self.command_line.is_some() {
            if matches!(
                command,
                Command::ToggleInlineFormat(_)
                    | Command::ToggleRunUnderline
                    | Command::SetInlineFontFamily(_)
                    | Command::RemoveInlineEnvironmentPreservingContent(_)
                    | Command::RemoveRunPropertyPreservingContent(_)
            ) {
                return Err(EditError::UnsupportedInlineFormat);
            }
            return if matches!(
                command,
                Command::BeginCommandLine
                    | Command::TypeCommandLine(_)
                    | Command::BackspaceCommandLine
                    | Command::MoveCommandLineCursor(_)
                    | Command::MoveCommandLineSelection(_)
                    | Command::SelectCommandLineCandidate(_)
                    | Command::TabCommandLine
                    | Command::AcceptCommandLine
                    | Command::CancelCommandLine
                    | Command::Undo
                    | Command::Redo
                    | Command::Delete(Direction::Backward)
                    | Command::EditorKey(
                        EditorKey::Escape
                            | EditorKey::Enter
                            | EditorKey::Tab
                            | EditorKey::ShiftEnter
                    )
            ) {
                Ok(None)
            } else if matches!(command, Command::EditorKey(EditorKey::ModBackspace)) {
                Ok(Some(Effect::default()))
            } else {
                Err(EditError::UnknownCommand)
            };
        }
        let effect = match command {
            Command::InsertText(text) if self.math_edit_is_text_branch() => {
                self.math_text_insert(text)?
            }
            Command::PasteText(text) if self.math_edit_is_text_branch() => {
                self.math_text_insert(text)?
            }
            Command::InsertText(text) => self.math_insert_text(text)?,
            Command::PasteText(text) => self.math_insert_raw(text, text.len())?,
            Command::Delete(Direction::Backward) if self.math_edit_is_text_branch() => {
                self.math_text_delete(Direction::Backward)?
            }
            Command::Delete(Direction::Forward) if self.math_edit_is_text_branch() => {
                self.math_text_delete(Direction::Forward)?
            }
            Command::Delete(Direction::Backward) => self.math_delete_backward()?,
            Command::Delete(Direction::Forward) => self.math_delete_forward()?,
            Command::MoveCaret(direction) => self.math_move(*direction)?,
            Command::EnterMath(_) => Effect::default(),
            Command::ToggleInlineFormat(_)
            | Command::ToggleRunUnderline
            | Command::SetInlineFontFamily(_)
            | Command::RemoveInlineEnvironmentPreservingContent(_)
            | Command::RemoveRunPropertyPreservingContent(_) => {
                return Err(EditError::UnsupportedInlineFormat);
            }
            Command::EditorKey(EditorKey::Enter)
                if matches!(
                    self.math_edit.as_ref().map(|edit| &edit.target),
                    Some(MathEditTarget::Block { align: true, .. })
                ) && !self.math_edit_is_text_branch() =>
            {
                self.math_insert_raw(ALIGN_ROW_BREAK, ALIGN_ROW_BREAK.len())?
            }
            Command::EditorKey(EditorKey::Enter) if self.math_edit_is_text_branch() => {
                return Err(EditError::UnsupportedNewline);
            }
            Command::InsertOneCharacterWidthSpace | Command::InsertTwoCharacterWidthSpace => {
                // 数学槽位导航尚未定义；Tab 保留公式焦点，不回退为正文空格输入。
                Effect::default()
            }
            Command::EditorKey(EditorKey::Tab) => Effect::default(),
            Command::EditorKey(EditorKey::ShiftEnter) => Effect::default(),
            Command::EditorKey(EditorKey::Escape | EditorKey::Enter) => {
                self.exit_math_edit(Direction::Forward, true)?
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
        self.run_with_inline_math_focus(Command::InsertInlineMath(String::new()), true)
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
        let paragraph_map = TextMap::new(&paragraph)?;
        paragraph_map.validate_pos(&focus)?;
        let paragraph_empty = paragraph_map.text.is_empty();
        let block = display_math_block(align, "")?;
        let block_id = block_id(&block)
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        let current = Block::Paragraph(paragraph.clone());
        let (after, next_position) = if paragraph_empty {
            (vec![block, current.clone()], focus.clone())
        } else {
            if !paragraph.extra.is_empty() {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            let (left_inlines, right_inlines) =
                split_inline_nodes(paragraph.inlines.clone(), focus.offset)?;
            let mut left = paragraph.clone();
            left.inlines = left_inlines;
            let mut next = paragraph;
            next.id = NodeId::generate();
            next.inlines = right_inlines;
            next.extra.clear();
            let position = Pos {
                story: StoryId::Body,
                paragraph_id: next.id.clone(),
                offset: 0,
                affinity: Affinity::Downstream,
            };
            (
                vec![Block::Paragraph(left), block, Block::Paragraph(next)],
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
        self.math_edit = Some(MathEdit {
            target: MathEditTarget::Block { block_id, align },
            source: String::new(),
            cursor: 0,
            branch: MathEditBranch::Source,
        });
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
            || self.structure_selection.is_some()
            || self.selected_block_environment.is_some()
            || !self.selected_block_environments.is_empty()
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
            self.selection_visual_affinity = VisualAffinity::Upstream;
            self.math_edit = Some(MathEdit {
                target: MathEditTarget::Inline {
                    paragraph_id: paragraph.id.clone(),
                    math_id,
                },
                source,
                cursor,
                branch: MathEditBranch::Source,
            });
            self.advance_history_merge_epoch();
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
            branch: MathEditBranch::Source,
        });
        self.advance_history_merge_epoch();
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
        let source_edit = edit.cursor..edit.cursor;
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
        self.write_math_source(source, cursor, Some(EditGroup::InsertText), source_edit)
    }

    fn math_insert_raw(&mut self, text: &str, cursor_in_text: usize) -> Result<Effect, EditError> {
        let edit = self.math_edit.as_ref().ok_or(EditError::InvalidPosition)?;
        let mut source = edit.source.clone();
        let source_edit = edit.cursor..edit.cursor;
        source.insert_str(source_edit.start, text);
        let cursor = source_edit.start + cursor_in_text;
        self.write_math_source(source, cursor, None, source_edit)
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
            return self.exit_math_edit(Direction::Backward, false);
        }
        if let Some(construct_range) = self.empty_construct_range(&edit) {
            let mut next = source.to_owned();
            next.replace_range(construct_range.clone(), "");
            return self.write_math_source(
                next,
                construct_range.start,
                Some(EditGroup::DeleteBackward),
                construct_range,
            );
        }
        let local_start =
            previous_math_caret_stop(body_source, cursor).ok_or(EditError::InvalidPosition)?;
        let start = body.start + local_start;
        let end = body.start + cursor;
        let mut next = source.to_owned();
        next.replace_range(start..end, "");
        self.write_math_source(next, start, Some(EditGroup::DeleteBackward), start..end)
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
            return self.exit_math_edit(Direction::Forward, false);
        };
        let start = body.start + cursor;
        let end = body.start + next_end;
        let mut next = source.to_owned();
        next.replace_range(start..end, "");
        self.write_math_source(next, start, Some(EditGroup::DeleteForward), start..end)
    }

    fn math_move(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        if let MathEditBranch::Text { path, .. } = &edit.branch {
            let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
            let text = path.resolve(tree).ok_or(EditError::InvalidPosition)?;
            let display_text = text
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>();
            let stop = match direction {
                Direction::Forward => next_grapheme_end(&display_text, edit.cursor),
                Direction::Backward => previous_grapheme_start(&display_text, edit.cursor),
            };
            if let Some(cursor) = stop {
                if let Some(current) = &mut self.math_edit {
                    current.cursor = cursor;
                    if let MathEditBranch::Text { anchor, .. } = &mut current.branch {
                        *anchor = cursor;
                    }
                }
            } else {
                let (start, end) = path.host_source_range();
                let exit_cursor = if display_text.is_empty() {
                    let Some(exit_cursor) = (match direction {
                        Direction::Backward => previous_math_caret_stop(&edit.source, start),
                        Direction::Forward => next_math_caret_stop(&edit.source, end),
                    }) else {
                        // 空文本岛若紧贴公式边缘且同向没有源码停靠点，继续退出公式以避免边界重入陷阱。
                        return self.exit_math_edit(direction, false);
                    };
                    exit_cursor
                } else {
                    match direction {
                        Direction::Backward => start,
                        Direction::Forward => end,
                    }
                };
                if let Some(current) = &mut self.math_edit {
                    current.branch = MathEditBranch::Source;
                    current.cursor = exit_cursor;
                }
            }
            self.advance_history_merge_epoch();
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        let body = math_input_body_range(&edit).ok_or(EditError::InvalidPosition)?;
        let source = edit
            .source
            .get(body.clone())
            .ok_or(EditError::InvalidPosition)?;
        let cursor = edit
            .cursor
            .checked_sub(body.start)
            .ok_or(EditError::InvalidPosition)?;
        let adjacent_text_path = match direction {
            Direction::Backward => {
                self.math_text_path_at_source_boundary(&edit, edit.cursor, false)
            }
            Direction::Forward => self.math_text_path_at_source_boundary(&edit, edit.cursor, true),
        };
        if let Some(path) = adjacent_text_path {
            let text = path
                .resolve(self.math_tree_for_edit(&edit, path.align_cell())?)
                .ok_or(EditError::InvalidPosition)?;
            let text_cursor = match direction {
                Direction::Backward => text.runs.iter().map(|run| run.text.len()).sum(),
                Direction::Forward => 0,
            };
            self.enter_math_text_branch(path, text_cursor);
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        let stop = match direction {
            Direction::Forward => next_math_caret_stop(source, cursor),
            Direction::Backward => previous_math_caret_stop(source, cursor),
        };
        match stop {
            Some(stop) => {
                let source_cursor = body.start + stop;
                if direction == Direction::Backward
                    && let Some(path) =
                        self.math_text_path_at_source_boundary(&edit, source_cursor, false)
                {
                    let text = path
                        .resolve(self.math_tree_for_edit(&edit, path.align_cell())?)
                        .ok_or(EditError::InvalidPosition)?;
                    let cursor = text.runs.iter().map(|run| run.text.len()).sum();
                    self.enter_math_text_branch(path, cursor);
                    return Ok(Effect {
                        selection_changed: true,
                        ..Effect::default()
                    });
                }
                if direction == Direction::Forward
                    && let Some(path) =
                        self.math_text_path_at_source_boundary(&edit, source_cursor, true)
                {
                    self.enter_math_text_branch(path, 0);
                    return Ok(Effect {
                        selection_changed: true,
                        ..Effect::default()
                    });
                }
                self.set_math_cursor(source_cursor);
                Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                })
            }
            None => self.exit_math_edit(direction, false),
        }
    }

    fn math_edit_is_text_branch(&self) -> bool {
        self.math_edit
            .as_ref()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Text { .. }))
    }

    fn math_tree_for_edit<'a>(
        &'a self,
        edit: &MathEdit,
        align_cell: Option<(usize, usize)>,
    ) -> Result<&'a caret_doc::MathTree, EditError> {
        math_tree_for_target(&self.document, &edit.target, align_cell)
            .ok_or(EditError::InvalidPosition)
    }

    fn math_text_path_at_source_boundary(
        &self,
        edit: &MathEdit,
        source_offset: usize,
        forward: bool,
    ) -> Option<MathTextFlowPath> {
        if let MathEditTarget::Block {
            block_id,
            align: true,
        } = &edit.target
        {
            let Block::Align(align) = find_block_by_id(&self.document.body, block_id)? else {
                return None;
            };
            let body_start = align_source::align_body_range(&align.source)?.start;
            let cell_ranges = align_source::cell_source_ranges(align)?;
            let (row, cell, cell_range) =
                cell_ranges.iter().enumerate().find_map(|(row, cells)| {
                    cells.iter().enumerate().find_map(|(cell, range)| {
                        let local_start = range.start.checked_sub(body_start)?;
                        let local_end = range.end.checked_sub(body_start)?;
                        (local_start <= source_offset && source_offset <= local_end)
                            .then_some((row, cell, range))
                    })
                })?;
            let tree = align.rows.get(row)?.cells.get(cell)?;
            let cell_start = cell_range.start.checked_sub(body_start)?;
            let cell_offset = source_offset.checked_sub(cell_start)?;
            if cell_offset > tree.source.len() {
                return None;
            }
            let path =
                path::path_at_source_boundary(tree, cell_offset, forward, Some((row, cell)))?;
            return Some(path.with_host_source_base(cell_start));
        }
        let tree = self.math_tree_for_edit(edit, None).ok()?;
        path::path_at_source_boundary(tree, source_offset, forward, None)
    }

    fn enter_math_text_branch(&mut self, path: MathTextFlowPath, cursor: usize) {
        if let Some(edit) = &mut self.math_edit {
            edit.cursor = cursor;
            edit.branch = MathEditBranch::Text {
                path,
                anchor: cursor,
            };
        }
        self.advance_history_merge_epoch();
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
    pub(super) fn exit_math_edit(
        &mut self,
        direction: Direction,
        allow_fallback: bool,
    ) -> Result<Effect, EditError> {
        let Some(edit) = self.math_edit.clone() else {
            return Ok(Effect::default());
        };
        match &edit.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let story = self
                    .story_for_paragraph(paragraph_id)
                    .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
                let probe = Pos {
                    story,
                    paragraph_id: paragraph_id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                };
                let start = inline_math_slots(self.paragraph_for(&probe)?)?
                    .into_iter()
                    .find(|(_, id)| id == math_id)
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
                self.set_math_exit_selection(position);
            }
            MathEditTarget::Block { block_id, .. } => {
                if allow_fallback {
                    let other_direction = match direction {
                        Direction::Backward => Direction::Forward,
                        Direction::Forward => Direction::Backward,
                    };
                    let position = match self.paragraph_beside_math_chain(block_id, direction)? {
                        Some(position) => Some(position),
                        None => self.paragraph_beside_math_chain(block_id, other_direction)?,
                    };
                    let Some(position) = position else {
                        return Ok(Effect::default());
                    };
                    self.set_math_exit_selection(position);
                } else if let Some(position) = self.paragraph_beside_block(block_id, direction)? {
                    self.set_math_exit_selection(position);
                } else if let Some(next_edit) = self.math_edit_beside_block(block_id, direction)? {
                    self.math_edit = Some(next_edit);
                    self.advance_history_merge_epoch();
                    return Ok(Effect {
                        selection_changed: true,
                        ..Effect::default()
                    });
                } else {
                    return Ok(Effect::default());
                }
            }
        }
        self.math_edit = None;
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    fn set_math_exit_selection(&mut self, position: Pos) {
        self.selection = Selection {
            anchor: position.clone(),
            focus: position.clone(),
        };
        self.selection_visual_affinity = match position.affinity {
            Affinity::Upstream => VisualAffinity::Upstream,
            Affinity::Downstream => VisualAffinity::Downstream,
        };
    }

    /// 显式退出时跳过连续公式块，寻找最近的同级正文段落。
    fn paragraph_beside_math_chain(
        &self,
        block_id: &NodeId,
        direction: Direction,
    ) -> Result<Option<Pos>, EditError> {
        let (parent_ids, mut index) =
            find_block_path(&self.document.body, block_id).ok_or(EditError::InvalidPosition)?;
        let siblings = block_list_ref(&self.document.body, &parent_ids)?;
        loop {
            let Some(next_index) = (match direction {
                Direction::Forward => index.checked_add(1),
                Direction::Backward => index.checked_sub(1),
            }) else {
                return Ok(None);
            };
            index = next_index;
            let Some(block) = siblings.get(index) else {
                return Ok(None);
            };
            match block {
                Block::Paragraph(paragraph) => {
                    let offset = match direction {
                        Direction::Forward => 0,
                        Direction::Backward => TextMap::new(paragraph)?.text.len(),
                    };
                    return Ok(Some(Pos {
                        story: StoryId::Body,
                        paragraph_id: paragraph.id.clone(),
                        offset,
                        affinity: match direction {
                            Direction::Forward => Affinity::Upstream,
                            Direction::Backward => Affinity::Downstream,
                        },
                    }));
                }
                Block::Equation(_) | Block::Align(_) => {}
                _ => return Ok(None),
            }
        }
    }

    /// 连续显示公式之间按块顺序移动公式焦点，不经过旧正文选区。
    fn math_edit_beside_block(
        &self,
        block_id: &NodeId,
        direction: Direction,
    ) -> Result<Option<MathEdit>, EditError> {
        let Some((parent_ids, index)) = find_block_path(&self.document.body, block_id) else {
            return Err(EditError::InvalidPosition);
        };
        let siblings = block_list_ref(&self.document.body, &parent_ids)?;
        let neighbour = match direction {
            Direction::Forward => index.checked_add(1),
            Direction::Backward => index.checked_sub(1),
        };
        let Some(block) = neighbour.and_then(|index| siblings.get(index)) else {
            return Ok(None);
        };
        let (block_id, align, source) = match block {
            Block::Equation(equation) => (
                equation.id.clone(),
                false,
                equation.math_tree.source.clone(),
            ),
            Block::Align(align) => (align.id.clone(), true, align_body(&align.source)),
            _ => return Ok(None),
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
        Ok(Some(MathEdit {
            target: MathEditTarget::Block { block_id, align },
            source,
            cursor,
            branch: MathEditBranch::Source,
        }))
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
            affinity: match direction {
                Direction::Forward => Affinity::Upstream,
                Direction::Backward => Affinity::Downstream,
            },
        }))
    }

    /// 空公式中退格：删除整个公式对象并退出编辑。
    fn delete_math_object(&mut self) -> Result<Effect, EditError> {
        let Some(edit) = self.math_edit.clone() else {
            return Ok(Effect::default());
        };
        let before_math_position =
            MathEditPosition::with_branch(edit.target.clone(), edit.cursor, edit.branch.clone());
        match edit.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let story = self
                    .story_for_paragraph(&paragraph_id)
                    .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
                let position = Pos {
                    story,
                    paragraph_id: paragraph_id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                };
                let mut paragraph = self.paragraph_for(&position)?.clone();
                let start = inline_math_slots(&paragraph)?
                    .into_iter()
                    .find(|(_, id)| *id == math_id)
                    .map(|(start, _)| start)
                    .ok_or(EditError::InvalidPosition)?;
                let before_selection = self.selection.clone();
                let before_visual_affinity = self.selection_visual_affinity;
                let before_states = history::paragraph_states_for_ids(
                    &self.document,
                    &self.paragraph_index,
                    [paragraph_id.clone()],
                );
                TextMap::new(&paragraph)?.delete_range(
                    &mut paragraph,
                    start,
                    start + OBJECT_REPLACEMENT.len_utf8(),
                )?;
                self.replace_paragraph(&position, paragraph)?;
                self.text_map_cache.clear();
                let caret = Pos {
                    offset: start,
                    affinity: Affinity::Downstream,
                    ..position
                };
                self.selection = Selection {
                    anchor: caret.clone(),
                    focus: caret.clone(),
                };
                self.selection_visual_affinity = match caret.affinity {
                    Affinity::Upstream => VisualAffinity::Upstream,
                    Affinity::Downstream => VisualAffinity::Downstream,
                };
                let after_visual_affinity = self.selection_visual_affinity;
                self.math_edit = None;
                let after_states = history::paragraph_states_for_ids(
                    &self.document,
                    &self.paragraph_index,
                    [paragraph_id.clone()],
                );
                self.record_math_history(
                    DocumentPatch::Paragraphs {
                        before: before_states,
                        after: after_states,
                        merge_group: None,
                        merge_epoch: self.history_merge_epoch,
                    },
                    (before_selection, before_visual_affinity),
                    (self.selection.clone(), after_visual_affinity),
                    Some(before_math_position),
                    None,
                );
                Ok(Effect {
                    document_changed: true,
                    selection_changed: true,
                    changed_paragraph_id: Some(paragraph_id),
                    ..Effect::default()
                })
            }
            MathEditTarget::Block { block_id, .. } => {
                let target =
                    match self.paragraph_beside_math_chain(&block_id, Direction::Backward)? {
                        Some(position) => position,
                        None => self
                            .paragraph_beside_math_chain(&block_id, Direction::Forward)?
                            .ok_or(EditError::InvalidPosition)?,
                    };
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
                self.selection_visual_affinity = match self.selection.focus.affinity {
                    Affinity::Upstream => VisualAffinity::Upstream,
                    Affinity::Downstream => VisualAffinity::Downstream,
                };
                let paragraph_index = build_paragraph_index(&self.document.body);
                self.paragraph_index = paragraph_index;
                self.structure_index = Arc::new(StructureIndex::build(&self.document));
                self.text_map_cache.clear();
                self.advance_history_merge_epoch();
                let after_state_id = self.allocate_history_state_id();
                let before_state = SessionHistoryState::new(
                    before_selection,
                    before_visual_affinity,
                    self.pending_inline_style.clone(),
                    before_state_id,
                )
                .with_math_edit(Some(before_math_position));
                let after_state = SessionHistoryState::new(
                    self.selection.clone(),
                    self.selection_visual_affinity,
                    self.pending_inline_style.clone(),
                    after_state_id,
                );
                self.record_history_edit(SessionHistoryEdit::new(
                    DocumentPatch::BlockRange {
                        parent_environment_ids: parent_ids,
                        sibling_index: index,
                        before: vec![removed],
                        after: Vec::new(),
                    },
                    before_state,
                    after_state,
                ));
                self.history_state_id = after_state_id;
                self.advance_document_generation();
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
        old_edit: std::ops::Range<usize>,
    ) -> Result<Effect, EditError> {
        self.write_math_source_in_branch(source, cursor, merge_group, old_edit, None)
    }

    fn write_math_source_in_branch(
        &mut self,
        source: String,
        cursor: usize,
        merge_group: Option<EditGroup>,
        old_edit: std::ops::Range<usize>,
        text_branch: Option<(MathTextFlowPath, usize)>,
    ) -> Result<Effect, EditError> {
        self.write_math_source_with_metadata(
            source,
            cursor,
            merge_group,
            old_edit,
            text_branch,
            None,
        )
    }

    fn write_math_source_with_metadata(
        &mut self,
        source: String,
        cursor: usize,
        merge_group: Option<EditGroup>,
        old_edit: std::ops::Range<usize>,
        text_branch: Option<(MathTextFlowPath, usize)>,
        text_edit: Option<MathTextEdit>,
    ) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        if source == edit.source {
            self.set_math_cursor(cursor);
            return Ok(Effect::default());
        }
        if old_edit.start > old_edit.end
            || old_edit.end > edit.source.len()
            || !edit.source.is_char_boundary(old_edit.start)
            || !edit.source.is_char_boundary(old_edit.end)
        {
            return Err(EditError::InvalidPosition);
        }
        let inserted_len = source
            .len()
            .checked_sub(edit.source.len() - old_edit.len())
            .ok_or(EditError::InvalidPosition)?;
        let new_edit = old_edit.start..old_edit.start + inserted_len;
        let path_old_edit = text_edit
            .as_ref()
            .map(|text_edit| text_edit.source_edit.clone())
            .unwrap_or_else(|| old_edit.clone());
        let path_new_edit = text_edit.as_ref().map_or_else(
            || new_edit.clone(),
            |text_edit| {
                text_edit.source_edit.start
                    ..text_edit.source_edit.start + text_edit.source_replacement_len
            },
        );
        let mut next_branch = MathEditBranch::Source;
        let mut next_cursor = cursor;
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
                if let Some(text_edit) = &text_edit {
                    let text = text_edit
                        .path
                        .resolve_mut(&mut math.math_tree)
                        .ok_or(EditError::InvalidPosition)?;
                    validate_text_flow_edit(
                        text,
                        text_edit.range.clone(),
                        !text_edit.inserted.is_empty(),
                    )?;
                    replace_text_flow_range(text, text_edit.range.clone(), &text_edit.inserted)?;
                }
                if !preserve_math_tree_metadata_for_edit(
                    &math.math_tree,
                    &mut math_tree,
                    &old_edit,
                    &new_edit,
                ) {
                    return Err(EditError::InvalidMath(
                        "editing would discard unmapped math metadata".to_owned(),
                    ));
                }
                if let Some((path, text_cursor)) = &text_branch {
                    match path.rebase(
                        &math_tree,
                        path_old_edit.clone(),
                        path_new_edit.clone(),
                        old_edit.clone(),
                        new_edit.clone(),
                    ) {
                        Some(rebased) => {
                            let display = rebased
                                .resolve(&math_tree)
                                .ok_or(EditError::InvalidPosition)?
                                .runs
                                .iter()
                                .map(|run| run.text.as_str())
                                .collect::<String>();
                            let snap_forward = text_edit
                                .as_ref()
                                .is_none_or(|text_edit| text_edit.snap_forward);
                            next_cursor = snap_text_cursor(&display, *text_cursor, snap_forward)
                                .ok_or(EditError::InvalidPosition)?;
                            next_branch = MathEditBranch::Text {
                                path: rebased,
                                anchor: next_cursor,
                            };
                        }
                        None if text_edit
                            .as_ref()
                            .is_some_and(|text_edit| text_edit.removes_entire_flow) =>
                        {
                            // 分组边界可能让相邻 TextFlow 合并；退回确定的源码删除点，不猜新节点索引。
                            next_cursor = old_edit.start;
                        }
                        None => return Err(EditError::InvalidPosition),
                    }
                }
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
                self.record_math_history(
                    patch,
                    (self.selection.clone(), self.selection_visual_affinity),
                    (self.selection.clone(), self.selection_visual_affinity),
                    Some(MathEditPosition::with_branch(
                        edit.target.clone(),
                        edit.cursor,
                        edit.branch.clone(),
                    )),
                    Some(MathEditPosition::with_branch(
                        edit.target.clone(),
                        next_cursor,
                        next_branch.clone(),
                    )),
                );
            }
            MathEditTarget::Block { block_id, align } => {
                let mut block = display_math_block_with_id(*align, &source, block_id.clone())?;
                if let Some((path, text_cursor)) = &text_branch {
                    let tree = match (&block, path.align_cell()) {
                        (Block::Equation(equation), None) => &equation.math_tree,
                        (Block::Align(align), Some((row, cell))) => align
                            .rows
                            .get(row)
                            .and_then(|row| row.cells.get(cell))
                            .ok_or(EditError::InvalidPosition)?,
                        _ => return Err(EditError::InvalidPosition),
                    };
                    match path.rebase(
                        tree,
                        path_old_edit.clone(),
                        path_new_edit.clone(),
                        old_edit.clone(),
                        new_edit.clone(),
                    ) {
                        Some(rebased) => {
                            let display = rebased
                                .resolve(tree)
                                .ok_or(EditError::InvalidPosition)?
                                .runs
                                .iter()
                                .map(|run| run.text.as_str())
                                .collect::<String>();
                            let text_cursor = snap_text_cursor(
                                &display,
                                *text_cursor,
                                text_edit
                                    .as_ref()
                                    .is_none_or(|text_edit| text_edit.snap_forward),
                            )
                            .ok_or(EditError::InvalidPosition)?;
                            next_branch = MathEditBranch::Text {
                                path: rebased,
                                anchor: text_cursor,
                            };
                            next_cursor = text_cursor;
                        }
                        None if text_edit
                            .as_ref()
                            .is_some_and(|text_edit| text_edit.removes_entire_flow) =>
                        {
                            // 分组边界可能让相邻 TextFlow 合并；退回确定的源码删除点，不猜新节点索引。
                            next_cursor = old_edit.start;
                        }
                        None => return Err(EditError::InvalidPosition),
                    }
                }
                let original = find_block_path(&self.document.body, block_id).and_then(
                    |(parent_ids, index)| {
                        block_list_ref(&self.document.body, &parent_ids)
                            .ok()?
                            .get(index)
                    },
                );
                if let Some(original) = original {
                    let mut metadata_block = original.clone();
                    if let Some(text_edit) = &text_edit {
                        let tree = match (&mut metadata_block, text_edit.path.align_cell()) {
                            (Block::Equation(equation), None) => &mut equation.math_tree,
                            (Block::Align(align), Some((row, cell))) => &mut align
                                .rows
                                .get_mut(row)
                                .ok_or(EditError::InvalidPosition)?
                                .cells
                                .get_mut(cell)
                                .ok_or(EditError::InvalidPosition)?,
                            _ => return Err(EditError::InvalidPosition),
                        };
                        let text = text_edit
                            .path
                            .resolve_mut(tree)
                            .ok_or(EditError::InvalidPosition)?;
                        validate_text_flow_edit(
                            text,
                            text_edit.range.clone(),
                            !text_edit.inserted.is_empty(),
                        )?;
                        replace_text_flow_range(
                            text,
                            text_edit.range.clone(),
                            &text_edit.inserted,
                        )?;
                    }
                    if !preserve_display_math_metadata(
                        &metadata_block,
                        &mut block,
                        &old_edit,
                        &new_edit,
                    ) {
                        return Err(EditError::InvalidMath(
                            "editing would discard unmapped math metadata".to_owned(),
                        ));
                    }
                }
                let (parent_ids, index) = find_block_path(&self.document.body, block_id)
                    .ok_or(EditError::InvalidPosition)?;
                let blocks = block_list(&mut self.document.body, &parent_ids)?;
                let slot = blocks.get_mut(index).ok_or(EditError::InvalidPosition)?;
                let before = std::mem::replace(slot, block.clone());
                self.text_map_cache.clear();
                self.advance_history_merge_epoch();
                self.record_math_history(
                    DocumentPatch::BlockRange {
                        parent_environment_ids: parent_ids,
                        sibling_index: index,
                        before: vec![before],
                        after: vec![block],
                    },
                    (self.selection.clone(), self.selection_visual_affinity),
                    (self.selection.clone(), self.selection_visual_affinity),
                    Some(MathEditPosition::with_branch(
                        edit.target.clone(),
                        edit.cursor,
                        edit.branch.clone(),
                    )),
                    Some(MathEditPosition::with_branch(
                        edit.target.clone(),
                        next_cursor,
                        next_branch.clone(),
                    )),
                );
            }
        }
        if let Some(current) = &mut self.math_edit {
            current.source = source;
            current.cursor = next_cursor;
            current.branch = next_branch;
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
        fn containing_node(node: &MathNode, cursor: usize) -> Option<Range<usize>> {
            let has_empty_argument = node.children.iter().flat_map(|flow| &flow.content).any(|content| {
                matches!(content, MathContent::Math(child) if child.kind == caret_doc::MathNodeKind::Placeholder
                    && child.source_range.as_ref().is_some_and(|range| {
                        range.start as usize + 1 == cursor && range.end as usize == cursor + 1
                    }))
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
                .find_map(|flow| containing_flow(flow, cursor))
        }
        fn containing_flow(flow: &MathFlow, cursor: usize) -> Option<Range<usize>> {
            flow.content
                .iter()
                .find_map(|content| containing_content(content, cursor))
        }
        fn containing_content(content: &MathContent, cursor: usize) -> Option<Range<usize>> {
            match content {
                MathContent::Math(node) => containing_node(node, cursor),
                MathContent::Environment(environment) => environment
                    .children
                    .iter()
                    .find_map(|child| containing_content(child, cursor)),
                _ => None,
            }
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
        containing_flow(&tree.root, edit.cursor)
    }

    fn record_math_history(
        &mut self,
        patch: DocumentPatch,
        before_selection: (Selection, VisualAffinity),
        after_selection: (Selection, VisualAffinity),
        before_math_edit: Option<MathEditPosition>,
        after_math_edit: Option<MathEditPosition>,
    ) {
        let after_state_id = self.allocate_history_state_id();
        let before_state = SessionHistoryState::new(
            before_selection.0,
            before_selection.1,
            self.pending_inline_style.clone(),
            self.history_state_id,
        )
        .with_math_edit(before_math_edit);
        let after_state = SessionHistoryState::new(
            after_selection.0,
            after_selection.1,
            self.pending_inline_style.clone(),
            after_state_id,
        )
        .with_math_edit(after_math_edit);
        self.record_history_edit(SessionHistoryEdit::new(patch, before_state, after_state));
        self.history_state_id = after_state_id;
        self.advance_document_generation();
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

fn find_inline_math<'a>(nodes: &'a [ParagraphContent], id: &NodeId) -> Option<&'a InlineMath> {
    nodes.iter().find_map(|node| match node {
        ParagraphContent::Math(math) if &math.id == id => Some(math),
        ParagraphContent::Environment(environment) => find_inline_math(&environment.children, id),
        _ => None,
    })
}

fn find_inline_math_mut<'a>(
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
    align_source::align_body_range(source)
        .map(|range| source[range].to_owned())
        .unwrap_or_else(|| source.trim().to_owned())
}

#[cfg(test)]
#[path = "math_edit/tests.rs"]
mod tests;
