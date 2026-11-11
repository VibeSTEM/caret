//! 公式内部扩选与跨宿主结构范围转换。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::document_position::{visual_affinity_for, visual_affinity_for_block_edge};
use super::super::{
    Affinity, Direction, DocumentRangeEndpoint, EditError, Effect, Pos, Selection, Session,
    StructureSelection,
};
use super::tree_position::{MathTreeCaretMove, advance_math_tree_selection_position};
use super::{MathEdit, MathEditBranch, MathEditTarget, inline_math_slots};
use crate::editing::paragraph_ops::paragraph_at_location;
use caret_doc::{Block, NodeId};
use caret_structure::find_document_node;

impl Session {
    /// 在 MathTree 分支按结构路径扩展焦点；到达宿主边界时转换为文档结构范围。
    ///
    /// 坍缩状态下锚点固定在当前位置后移动光标；已有范围时只移动光标并保留锚点。
    /// 扩选不进入嵌套 Math/Environment：嵌套结构在当前序列层作为一个原子步进，
    /// 保证焦点与锚点始终同路径，范围可被历史恢复和后续按键消费。焦点一旦离开
    /// 锚点所在序列（climb 到父路径或到达 root/Align cell 边界），就把整个公式宿主
    /// 转换为文档结构范围；跨越 MathTree/TextFlow 边界时沿用本地移动语义。
    pub(in crate::editing::session) fn extend_math_tree_selection(
        &mut self,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        self.ensure_math_edit_revision_current()?;
        let Some(edit) = self.math_edit.as_ref().map(MathEdit::command_snapshot) else {
            return Err(EditError::InvalidPosition);
        };
        let MathEditBranch::Tree { path, anchor } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        // 锚点已不在当前序列（防御历史恢复后的异常组合）：改用整公式宿主范围，
        // 不写入焦点与锚点路径不一致、无法恢复的 Tree 分支。
        if anchor.path != *path {
            return self.convert_math_host_to_document_selection(direction);
        }
        let align_cell = self.align_cell_for_position(&edit, anchor)?;
        let tree = self.math_tree_for_edit(&edit, align_cell)?;
        let mut position = anchor.clone();
        position.path = path.clone();
        position.offset = edit.cursor;
        position
            .validate_reachable(tree)
            .map_err(|_| EditError::InvalidPosition)?;
        let movement = advance_math_tree_selection_position(
            tree,
            &position,
            direction == Direction::Forward,
            align_cell,
        )
        .map_err(|_| EditError::InvalidPosition)?;
        match movement {
            MathTreeCaretMove::Boundary => {
                // root 边界与 Align cell 边界都转换为整个公式宿主，保持 caret 可操作。
                self.convert_math_host_to_document_selection(direction)
            }
            MathTreeCaretMove::Position(next) => {
                // climb 到父序列后焦点已离开锚点所在序列，同一 anchor 无法表达范围。
                if next.path != anchor.path {
                    return self.convert_math_host_to_document_selection(direction);
                }
                let next_align_cell = self.align_cell_for_position(&edit, &next)?;
                next.resolve(self.math_tree_for_edit(&edit, next_align_cell)?)
                    .map_err(|_| EditError::InvalidPosition)?;
                if let Some(current) = &mut self.math_edit {
                    current.replace_source_projection(None);
                    current.source_draft = None;
                    current.cursor = next.offset;
                    current.branch = MathEditBranch::Tree {
                        path: next.path.clone(),
                        anchor: anchor.clone(),
                    };
                }
                self.advance_history_merge_epoch();
                Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                })
            }
            MathTreeCaretMove::TextFlow(..) => {
                self.apply_math_tree_caret_move(&edit, movement, direction)
            }
        }
    }

    /// 当前 MathText 分支的焦点是否已到达整个公式宿主的文本岛边缘。
    ///
    /// Align 的每个 cell 有自己的文本岛，只有首个/末个 cell 的边缘才等于宿主边缘。
    pub(in crate::editing::session) fn math_text_at_host_edge(
        &self,
        direction: Direction,
    ) -> Result<bool, EditError> {
        let Some(edit) = self.math_edit.as_ref() else {
            return Ok(false);
        };
        let MathEditBranch::Text { path, .. } = &edit.branch else {
            return Ok(false);
        };
        // Align 的每个 cell 有自己的树：先要求当前 cell 是该块的首/末 cell。
        if let MathEditTarget::Block {
            block_id,
            align: true,
        } = &edit.target
        {
            let Some((row, cell)) = path.align_cell() else {
                return Ok(false);
            };
            let Some(Block::Align(align)) = super::find_block_by_id(&self.document.body, block_id)
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
            let current = cells
                .iter()
                .position(|position| *position == (row, cell))
                .ok_or(EditError::InvalidPosition)?;
            let at_cell_edge = match direction {
                Direction::Forward => current + 1 == cells.len(),
                Direction::Backward => current == 0,
            };
            if !at_cell_edge {
                return Ok(false);
            }
        }
        // 只有当前文本岛是同方向最后一个文本岛时才是宿主边界；多个 `\text{}`
        // 岛或操作数内部文本岛不会提前转换为整个公式对象。
        let align_cell = path.align_cell();
        let Ok(tree) = self.math_tree_for_edit(edit, align_cell) else {
            return Ok(false);
        };
        Ok(super::path::text_island_is_host_edge(
            tree,
            path,
            direction == Direction::Forward,
        ))
    }

    /// 把当前公式宿主的本地焦点转换为文档级选区，并把 caret 停靠到宿主外侧边界。
    ///
    /// 块级公式选择整个 Equation/Align（BlockEdge 两端）；InlineMath 只是段落文本中
    /// 的一个 U+FFFC 对象字符，越界时写普通段落文本选区覆盖该字符，不伪造空覆盖的
    /// 结构选区，之后的方向键继续按文档文本扩选。
    /// 所有校验在写入任何状态前完成。
    pub(in crate::editing::session) fn convert_math_host_to_document_selection(
        &mut self,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        self.ensure_math_edit_revision_current()?;
        if self.has_uncommitted_math_source_draft() {
            return Err(EditError::InvalidMath(
                "resolve or cancel the incomplete math source draft before extending the selection"
                    .to_owned(),
            ));
        }
        let Some(edit) = self.math_edit.as_ref() else {
            return Ok(Effect::default());
        };
        match edit.target.clone() {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let (before, after) = self.inline_math_slot_bounds(&paragraph_id, &math_id)?;
                self.text_map_for(&before)?.validate_pos(&before)?;
                self.text_map_for(&after)?.validate_pos(&after)?;
                let (anchor, focus) = match direction {
                    Direction::Backward => (after, before),
                    Direction::Forward => (before, after),
                };
                let anchor_visual_affinity = visual_affinity_for(anchor.affinity);
                let focus_visual_affinity = visual_affinity_for(focus.affinity);
                // InlineMath 的行内范围覆盖不到任何块环境或公式块；空覆盖会被
                // install_structure_selection 拒绝，这里不调用安装入口，直接写覆盖
                // U+FFFC 的普通文本选区。
                self.clear_structure_selection_state();
                self.math_edit = None;
                self.selection = Selection { anchor, focus };
                self.selection_visual_affinity = focus_visual_affinity;
                self.selection_anchor_visual_affinity = anchor_visual_affinity;
                self.host_caret_position = None;
            }
            MathEditTarget::Block { block_id, .. } => {
                let location = find_document_node(&self.document, &block_id)
                    .ok_or(EditError::InvalidPosition)?;
                if !location.kind.is_math_block() {
                    return Err(EditError::InvalidPosition);
                }
                let (anchor_side, focus_side) = match direction {
                    Direction::Forward => (Direction::Backward, Direction::Forward),
                    Direction::Backward => (Direction::Forward, Direction::Backward),
                };
                let structure_selection = StructureSelection {
                    anchor: DocumentRangeEndpoint::BlockEdge {
                        block_id: block_id.clone(),
                        side: anchor_side,
                    },
                    focus: DocumentRangeEndpoint::BlockEdge {
                        block_id: block_id.clone(),
                        side: focus_side,
                    },
                };
                let (caret, host_caret_position) =
                    self.park_beside_block_host(&block_id, direction)?;
                self.install_structure_selection(structure_selection)?;
                self.math_edit = None;
                self.selection = Selection {
                    anchor: caret.clone(),
                    focus: caret,
                };
                self.selection_visual_affinity = visual_affinity_for_block_edge(direction);
                self.selection_anchor_visual_affinity =
                    visual_affinity_for_block_edge(match direction {
                        Direction::Forward => Direction::Backward,
                        Direction::Backward => Direction::Forward,
                    });
                self.host_caret_position = Some(host_caret_position);
            }
        }
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    /// 返回 InlineMath 在所属段落 U+FFFC 两侧的外部位置。
    fn inline_math_slot_bounds(
        &mut self,
        paragraph_id: &NodeId,
        math_id: &NodeId,
    ) -> Result<(Pos, Pos), EditError> {
        let story = self
            .story_for_paragraph(paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
        let probe = Pos {
            story,
            paragraph_id: paragraph_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        let start = {
            let location = self
                .paragraph_index
                .get(paragraph_id)
                .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
            let paragraph = paragraph_at_location(&self.document.body, location)
                .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
            inline_math_slots(paragraph)?
                .into_iter()
                .find(|(_, id)| id == math_id)
                .map(|(start, _)| start)
                .ok_or(EditError::InvalidPosition)?
        };
        let end = start
            .checked_add(crate::editing::text_map::OBJECT_REPLACEMENT.len_utf8())
            .ok_or(EditError::InvalidPosition)?;
        Ok((
            Pos {
                offset: start,
                affinity: Affinity::Upstream,
                ..probe.clone()
            },
            Pos {
                offset: end,
                affinity: Affinity::Downstream,
                ..probe
            },
        ))
    }
}
