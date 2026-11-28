//! 数学宿主的进入、退出和相邻块导航。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;
use crate::editing::paragraph_ops;

impl Session {
    /// 退出公式编辑；块公式把边界 ID 和方向交给共享外侧停靠状态。
    pub(in crate::editing::session) fn exit_math_edit(
        &mut self,
        direction: Direction,
        allow_fallback: bool,
    ) -> Result<Effect, EditError> {
        self.exit_math_edit_with_boundary(direction, allow_fallback, false)
    }

    /// 显式离开块公式时停在外侧边界，供重新进入或继续移动到相邻宿主。
    pub(in crate::editing::session) fn exit_math_edit_for_navigation(
        &mut self,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        self.exit_math_edit_with_boundary(direction, false, true)
    }

    fn exit_math_edit_with_boundary(
        &mut self,
        direction: Direction,
        allow_fallback: bool,
        keep_block_boundary_stop: bool,
    ) -> Result<Effect, EditError> {
        if self
            .math_edit
            .as_ref()
            .is_some_and(MathEdit::source_draft_is_pending)
        {
            return Err(EditError::InvalidMath(
                "resolve or cancel the incomplete math source draft before leaving".to_owned(),
            ));
        }
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
                } else {
                    if let Some(position) = self.paragraph_beside_block(block_id, direction)? {
                        self.set_math_exit_selection(position);
                    }
                    if keep_block_boundary_stop {
                        self.set_host_caret_position(Some(HostCaretPosition::outside(
                            block_id.clone(),
                            direction,
                        )));
                    }
                }
            }
        }
        self.clear_math_edit();
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

    /// 查找同级正文段落；定位、Enter 回退和对象删除恢复使用此策略，Escape 不跨块跳转。
    pub(in crate::editing::session) fn paragraph_beside_math_chain(
        &self,
        block_id: &NodeId,
        direction: Direction,
    ) -> Result<Option<Pos>, EditError> {
        let (container, mut index) = find_block_container_path(&self.document.body, block_id)
            .ok_or(EditError::InvalidPosition)?;
        let siblings = caret_structure::block_list_by_container(&self.document.body, &container)
            .ok_or(EditError::InvalidPosition)?;
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
                        story: paragraph_ops::story_for_container(&container),
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

    pub(in crate::editing::session) fn math_edit_at_block(
        &self,
        block_id: &NodeId,
        entry_direction: Direction,
    ) -> Result<Option<MathEdit>, EditError> {
        let Some((container, index)) = find_block_container_path(&self.document.body, block_id)
        else {
            return Err(EditError::InvalidPosition);
        };
        let Some(block) = caret_structure::block_list_by_container(&self.document.body, &container)
            .ok_or(EditError::InvalidPosition)?
            .get(index)
        else {
            return Err(EditError::InvalidPosition);
        };
        self.math_edit_for_block(block, entry_direction)
    }

    fn math_edit_for_block(
        &self,
        block: &Block,
        direction: Direction,
    ) -> Result<Option<MathEdit>, EditError> {
        let (block_id, align, cursor, branch) = match block {
            Block::Equation(equation) => {
                let (cursor, branch) = math_tree_edge_position(
                    &equation.math_tree,
                    MathPositionHost::Equation {
                        owner_id: equation.id.clone(),
                    },
                    direction,
                    None,
                )?;
                (equation.id.clone(), false, cursor, branch)
            }
            Block::Align(value) => {
                let cell = match direction {
                    Direction::Forward => {
                        value.rows.iter().enumerate().find_map(|(row_index, row)| {
                            row.cells.first().map(|cell| (row_index, 0, &row.id, cell))
                        })
                    }
                    Direction::Backward => {
                        value
                            .rows
                            .iter()
                            .enumerate()
                            .rev()
                            .find_map(|(row_index, row)| {
                                let cell_index = row.cells.len().checked_sub(1)?;
                                Some((row_index, cell_index, &row.id, row.cells.get(cell_index)?))
                            })
                    }
                }
                .ok_or(EditError::InvalidPosition)?;
                let (row_index, cell_index, row_id, cell) = cell;
                let (cursor, branch) = math_tree_edge_position(
                    cell,
                    MathPositionHost::AlignCell {
                        row_id: row_id.clone(),
                        cell_id: cell.root.id.clone(),
                    },
                    direction,
                    Some((row_index, cell_index)),
                )?;
                (value.id.clone(), true, cursor, branch)
            }
            _ => return Ok(None),
        };
        let target = MathEditTarget::Block { block_id, align };
        Ok(Some(MathEdit::entering(
            target.clone(),
            cursor,
            branch,
            self.current_math_edit_revision(),
        )))
    }
}

impl Session {
    /// 公式编辑期间截获输入；`None` 表示命令由 Session 的常规分派继续处理。
    pub(in crate::editing::session) fn run_math_edit(
        &mut self,
        command: &Command,
    ) -> Result<Option<Effect>, EditError> {
        if self.math_edit.is_none() {
            return Ok(None);
        }
        let math_text_active = self.math_edit_is_text_branch();
        let composition_command = matches!(
            command,
            Command::UpdateComposition { .. }
                | Command::CommitComposition { .. }
                | Command::CancelComposition { .. }
        );
        if (self.composition.is_some() && !composition_command)
            || (composition_command && !math_text_active)
        {
            return Err(EditError::CompositionPending);
        }
        if composition_command && math_text_active {
            return match command {
                Command::UpdateComposition { id, preedit } => {
                    self.update_composition(*id, preedit.clone()).map(Some)
                }
                Command::CommitComposition { id, text } => {
                    self.commit_composition(*id, text).map(Some)
                }
                Command::CancelComposition { id } => self.cancel_composition(*id).map(Some),
                _ => unreachable!("composition_command only matches composition commands"),
            };
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
            ) {
                Ok(None)
            } else {
                Err(EditError::UnknownCommand)
            };
        }
        if self
            .math_edit
            .as_ref()
            .is_some_and(MathEdit::source_draft_is_pending)
            && matches!(
                command,
                Command::SetPageGeometry(_)
                    | Command::SetDefaultFontFamily(_)
                    | Command::SetDocumentFontFamily { .. }
            )
        {
            return Err(EditError::InvalidMath(
                "resolve or cancel the incomplete math source draft before changing document settings"
                    .to_owned(),
            ));
        }
        let effect = match command {
            Command::InsertInlineMath(source) if self.math_edit_is_text_branch() => {
                self.math_text_enter_math(source)?
            }
            Command::InsertText(text) if self.math_edit_is_text_branch() => {
                self.math_text_insert(text)?
            }
            Command::PasteText(text) if self.math_edit_is_text_branch() => {
                self.math_text_insert(text)?
            }
            Command::InsertText(text) if self.math_edit_is_tree_branch() => {
                self.math_tree_insert_text(text)?
            }
            Command::PasteText(text) if self.math_edit_is_tree_branch() => {
                self.math_tree_insert_text(text)?
            }
            Command::SetMathTextSelection { anchor, focus } => {
                self.set_math_text_selection(*anchor, *focus)?
            }
            Command::InsertText(text) => self.math_insert_text(text)?,
            Command::PasteText(text) => self.math_insert_raw(text, text.len())?,
            Command::InsertSoftBreak | Command::InsertParagraphBreak => {
                return Err(EditError::UnsupportedNewline);
            }
            Command::Delete(Direction::Backward) if self.math_edit_is_text_branch() => {
                let edit = self
                    .math_edit
                    .as_ref()
                    .map(MathEdit::command_snapshot)
                    .ok_or(EditError::InvalidPosition)?;
                if let Some(effect) = multiline::handle_text_backspace(self, &edit)? {
                    return Ok(Some(effect));
                }
                self.math_text_delete(Direction::Backward)?
            }
            Command::Delete(Direction::Forward) if self.math_edit_is_text_branch() => {
                self.math_text_delete(Direction::Forward)?
            }
            Command::Delete(Direction::Backward) if self.math_edit_is_tree_branch() => {
                self.math_tree_delete(Direction::Backward)?
            }
            Command::Delete(Direction::Forward) if self.math_edit_is_tree_branch() => {
                self.math_tree_delete(Direction::Forward)?
            }
            Command::Delete(Direction::Backward) => self.math_delete_backward()?,
            Command::Delete(Direction::Forward) => self.math_delete_forward()?,
            Command::MoveCaret(direction) => self.math_move(*direction)?,
            Command::EnterMath(_) => Effect::default(),
            Command::ToggleRunUnderline if self.math_edit_is_text_branch() => {
                self.toggle_math_text_run_property(crate::editing::EnvironmentKind::Underline)?
            }
            Command::ToggleInlineFormat(kind) if self.math_edit_is_text_branch() => {
                self.toggle_math_text_inline_environment(*kind)?
            }
            Command::SetInlineFontFamily(family) if self.math_edit_is_text_branch() => {
                self.set_math_text_font_family(family)?
            }
            Command::ToggleInlineFormat(_)
            | Command::ToggleRunUnderline
            | Command::SetInlineFontFamily(_)
            | Command::RemoveInlineEnvironmentPreservingContent(_)
            | Command::RemoveRunPropertyPreservingContent(_) => {
                return Err(EditError::UnsupportedInlineFormat);
            }
            Command::InsertOneCharacterWidthSpace | Command::InsertTwoCharacterWidthSpace => {
                // 数学槽位导航尚未定义；Tab 保留公式焦点，不回退为正文空格输入。
                Effect::default()
            }
            Command::SetSelection(None) => {
                if self.math_edit_is_text_branch() {
                    let cursor = self
                        .math_edit
                        .as_ref()
                        .ok_or(EditError::InvalidPosition)?
                        .cursor;
                    self.set_math_text_selection(cursor, cursor)?
                } else {
                    Effect::default()
                }
            }
            Command::SetSelection(Some(selection)) => {
                if self
                    .math_edit
                    .as_ref()
                    .is_some_and(MathEdit::source_draft_is_pending)
                {
                    return Err(EditError::InvalidMath(
                        "resolve or cancel the incomplete math source draft before leaving"
                            .to_owned(),
                    ));
                }
                // 鼠标选区先按正文位置规则验证；失败时保留原公式编辑状态。
                let active_edit = self.take_math_edit();
                match self.set_selection(Some(selection.clone())) {
                    Ok(mut effect) => {
                        self.clear_command_line();
                        self.pending_inline_style.clear();
                        self.selection_visual_affinity = match self.selection.focus.affinity {
                            Affinity::Upstream => VisualAffinity::Upstream,
                            Affinity::Downstream => VisualAffinity::Downstream,
                        };
                        self.advance_history_merge_epoch();
                        effect.selection_changed |= active_edit.is_some();
                        return Ok(Some(effect));
                    }
                    Err(error) => {
                        self.set_math_edit(active_edit);
                        return Err(error);
                    }
                }
            }
            Command::SetPageGeometry(_)
            | Command::SetDefaultFontFamily(_)
            | Command::SetDocumentFontFamily { .. } => return Ok(None),
            Command::BeginCommandLine => {
                self.set_command_line(Some(CommandLine::math(self.selection.focus.clone())));
                Effect::default()
            }
            // 编辑目标仍是公式时，拒绝尚未适配的命令，避免它们落到公式宿主正文。
            _ => return Err(EditError::UnknownCommand),
        };
        Ok(Some(effect))
    }

    /// `\math`：插入空行内公式并进入编辑。
    pub(in crate::editing::session) fn begin_inline_math(&mut self) -> Result<Effect, EditError> {
        self.run_with_inline_math_focus(Command::InsertInlineMath(String::new()), true)
    }

    /// `\equation` / `\align`：在当前正文段落之后插入空显示公式块并进入编辑。
    /// 当前段落为空时公式插在它之前，空段落留作公式后的续写位置。
    pub(in crate::editing::session) fn insert_display_math(
        &mut self,
        align: bool,
    ) -> Result<Effect, EditError> {
        let plan = self.prepare_display_math_insert(align)?;
        let before_selection = self.selection.clone();
        let before_anchor_visual_affinity = self.selection_anchor_visual_affinity;
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let blocks =
            paragraph_ops::block_list_in_container_mut(&mut self.document.body, &plan.container)
                .ok_or(EditError::InvalidPosition)?;
        blocks.splice(
            plan.sibling_index..=plan.sibling_index,
            plan.after.iter().cloned(),
        );
        self.selection = Selection {
            anchor: plan.next_position.clone(),
            focus: plan.next_position,
        };
        self.set_math_edit(Some(MathEdit::entering(
            plan.target,
            plan.cursor,
            plan.branch,
            self.current_math_edit_revision(),
        )));
        let index = build_paragraph_index(&self.document.body);
        self.finalize_structure_edit(
            index,
            DocumentPatch::BlockRange {
                container: plan.container.clone(),
                sibling_index: plan.sibling_index,
                before: vec![plan.before],
                after: plan.after,
            },
            before_selection,
            before_anchor_visual_affinity,
            before_visual_affinity,
            before_state_id,
        );
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            ..Effect::default()
        })
    }

    pub(in crate::editing::session) fn can_insert_display_math(&self, align: bool) -> bool {
        self.prepare_display_math_insert(align).is_ok()
    }

    fn prepare_display_math_insert(&self, align: bool) -> Result<DisplayMathInsertPlan, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let focus = self.selection.focus.clone();
        if self.selection.anchor != focus {
            return Err(EditError::InvalidPosition);
        }
        let location = self
            .paragraph_index
            .get(&focus.paragraph_id)
            .cloned()
            .ok_or_else(|| EditError::UnknownParagraph(focus.paragraph_id.to_string()))?;
        if location.is_title || location.story != focus.story {
            return Err(EditError::InvalidPosition);
        }
        let container = location.container.clone();
        let Some(&sibling_index) = location.block_path.last() else {
            return Err(EditError::InvalidPosition);
        };
        let paragraph = self.paragraph_for(&focus)?.clone();
        let paragraph_map = TextMap::new(&paragraph)?;
        paragraph_map.validate_pos(&focus)?;
        let paragraph_empty = paragraph_map.text.is_empty();
        let block_id = NodeId::generate();
        let block = if align {
            Block::Align(Align {
                id: block_id.clone(),
                rows: vec![MathAlignRow {
                    id: NodeId::generate(),
                    cells: vec![empty_math_tree()],
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            })
        } else {
            Block::Equation(Equation {
                id: block_id.clone(),
                math_tree: empty_math_tree(),
                extra: BTreeMap::new(),
            })
        };
        let (cursor, branch) = match &block {
            Block::Equation(equation) => math_tree_edge_position(
                &equation.math_tree,
                MathPositionHost::Equation {
                    owner_id: block_id.clone(),
                },
                Direction::Forward,
                None,
            )?,
            Block::Align(align) => {
                let cell = align
                    .rows
                    .first()
                    .and_then(|row| row.cells.first())
                    .ok_or(EditError::InvalidPosition)?;
                math_tree_edge_position(
                    cell,
                    MathPositionHost::AlignCell {
                        row_id: align.rows[0].id.clone(),
                        cell_id: cell.root.id.clone(),
                    },
                    Direction::Forward,
                    Some((0, 0)),
                )?
            }
            _ => return Err(EditError::InvalidPosition),
        };
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
                story: focus.story.clone(),
                paragraph_id: next.id.clone(),
                offset: 0,
                affinity: Affinity::Downstream,
            };
            (
                vec![Block::Paragraph(left), block, Block::Paragraph(next)],
                position,
            )
        };
        let mut candidate_document = self.document.clone();
        let blocks =
            paragraph_ops::block_list_in_container_mut(&mut candidate_document.body, &container)
                .ok_or(EditError::InvalidPosition)?;
        blocks.splice(sibling_index..=sibling_index, after.iter().cloned());
        caret_doc::CdocFile::new(candidate_document)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
        Ok(DisplayMathInsertPlan {
            container,
            sibling_index,
            before: current,
            after,
            next_position,
            target: MathEditTarget::Block { block_id, align },
            cursor,
            branch,
        })
    }

    /// 从正文进入相邻公式：候选必须跨过单个公式，且正文两端必须落在对应段首/段尾。
    pub(in crate::editing::session) fn navigation_math_crossing_direction(
        &self,
        current: &Pos,
        target: &Pos,
    ) -> Result<Option<Direction>, EditError> {
        if current.story != target.story {
            return Ok(None);
        }
        if current.paragraph_id == target.paragraph_id {
            let object_len = OBJECT_REPLACEMENT.len_utf8();
            for (start, _) in inline_math_slots(self.paragraph_for(current)?)? {
                let Some(end) = start.checked_add(object_len) else {
                    return Err(EditError::InvalidPosition);
                };
                if current.offset == start && target.offset == end {
                    return Ok(Some(Direction::Forward));
                }
                if current.offset == end && target.offset == start {
                    return Ok(Some(Direction::Backward));
                }
            }
            return Ok(None);
        }
        if current.story != target.story {
            return Ok(None);
        }

        let Some(current_location) = self.paragraph_index.get(&current.paragraph_id) else {
            return Ok(None);
        };
        let Some(target_location) = self.paragraph_index.get(&target.paragraph_id) else {
            return Ok(None);
        };
        let Some((current_container, current_index)) =
            paragraph_ops::paragraph_parent_container_path(&self.document.body, current_location)
        else {
            return Ok(None);
        };
        let Some((target_container, target_index)) =
            paragraph_ops::paragraph_parent_container_path(&self.document.body, target_location)
        else {
            return Ok(None);
        };
        if current_container != target_container {
            return Ok(None);
        }
        let (math_index, direction) = if target_index == current_index.saturating_add(2) {
            (current_index + 1, Direction::Forward)
        } else if current_index == target_index.saturating_add(2) {
            (target_index + 1, Direction::Backward)
        } else {
            return Ok(None);
        };
        let blocks =
            caret_structure::block_list_by_container(&self.document.body, &current_container)
                .ok_or(EditError::InvalidPosition)?;
        if !matches!(
            blocks.get(math_index),
            Some(Block::Equation(_) | Block::Align(_))
        ) {
            return Ok(None);
        }
        let current_len = TextMap::new(self.paragraph_for(current)?)?.text.len();
        let target_len = TextMap::new(self.paragraph_for(target)?)?.text.len();
        let crosses_at_host_edges = match direction {
            Direction::Forward => current.offset == current_len && target.offset == 0,
            Direction::Backward => current.offset == 0 && target.offset == target_len,
        };
        if !crosses_at_host_edges {
            return Err(EditError::InvalidPosition);
        }
        Ok(Some(direction))
    }

    /// 仅当当前正文焦点紧邻受支持的公式对象时进入该公式。
    pub(in crate::editing::session) fn enter_adjacent_math_if_present(
        &mut self,
        direction: Direction,
    ) -> Result<bool, EditError> {
        self.enter_adjacent_math(direction, true)
    }

    fn enter_adjacent_math(
        &mut self,
        direction: Direction,
        stop_before_entering: bool,
    ) -> Result<bool, EditError> {
        if self.selection.anchor != self.selection.focus
            || self.math_edit.is_some()
            || self.has_structure_selection()
            || self.composition.is_some()
            || self.command_line.is_some()
        {
            return Ok(false);
        }
        let focus = self.selection.focus.clone();
        let paragraph = self.paragraph_for(&focus)?;
        let object_len = OBJECT_REPLACEMENT.len_utf8();
        if self.block_boundary_exit().is_none()
            && inline_math_slots(paragraph)?
                .iter()
                .any(|(start, _)| match direction {
                    Direction::Forward => *start == focus.offset,
                    Direction::Backward => start + object_len == focus.offset,
                })
        {
            self.enter_math_at_text_edge(direction)?;
            return Ok(self.math_edit.is_some());
        }
        let at_edge = match direction {
            Direction::Forward => focus.offset == TextMap::new(paragraph)?.text.len(),
            Direction::Backward => focus.offset == 0,
        };
        if !at_edge {
            return Ok(false);
        }
        let Some(location) = self.paragraph_index.get(&focus.paragraph_id) else {
            return Ok(false);
        };
        if location.is_title {
            return Ok(false);
        }
        let Some((container, sibling_index)) =
            paragraph_ops::paragraph_parent_container_path(&self.document.body, location)
        else {
            return Ok(false);
        };
        let neighbour = match direction {
            Direction::Forward => sibling_index.checked_add(1),
            Direction::Backward => sibling_index.checked_sub(1),
        };
        let Some(neighbour) = neighbour else {
            return Ok(false);
        };
        let blocks = caret_structure::block_list_by_container(&self.document.body, &container)
            .ok_or(EditError::InvalidPosition)?;
        if !matches!(
            blocks.get(neighbour),
            Some(Block::Equation(_) | Block::Align(_))
        ) {
            return Ok(false);
        }
        let target_id = match blocks.get(neighbour) {
            Some(Block::Equation(equation)) => equation.id.clone(),
            Some(Block::Align(align)) => align.id.clone(),
            _ => unreachable!("the adjacent math block was checked above"),
        };
        let opposite = match direction {
            Direction::Backward => Direction::Forward,
            Direction::Forward => Direction::Backward,
        };
        match self.host_caret_position.clone() {
            Some(position) if position.kind() == HostCaretKind::Inline => {
                if position.outside_direction() != Some(direction) {
                    return Ok(false);
                }
                if stop_before_entering {
                    self.set_host_caret_position(Some(HostCaretPosition::outside(
                        target_id, opposite,
                    )));
                    self.advance_history_merge_epoch();
                    return Ok(true);
                }
                if let Some(edit) = self.math_edit_at_block(&target_id, direction)? {
                    self.set_math_edit(Some(edit));
                    self.clear_host_caret_position();
                    self.advance_history_merge_epoch();
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Some(position)
                if position.kind() == HostCaretKind::Block && position.host_id() == &target_id =>
            {
                if let Some((_, exit_direction)) = position.as_outside() {
                    if direction == exit_direction {
                        Ok(true)
                    } else if let Some(edit) = self.math_edit_at_block(&target_id, direction)? {
                        self.clear_host_caret_position();
                        self.set_math_edit(Some(edit));
                        self.advance_history_merge_epoch();
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                } else {
                    Ok(false)
                }
            }
            Some(_) => Ok(false),
            None if stop_before_entering => {
                self.set_host_caret_position(Some(HostCaretPosition::outside(target_id, opposite)));
                self.advance_history_merge_epoch();
                Ok(true)
            }
            None => {
                let edit = self
                    .math_edit_at_block(&target_id, direction)?
                    .ok_or(EditError::InvalidPosition)?;
                self.set_math_edit(Some(edit));
                self.advance_history_merge_epoch();
                Ok(true)
            }
        }
    }

    pub(in crate::editing::session) fn enter_math(
        &mut self,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        if let Some(position) = self.host_caret_position.clone()
            && position.kind() == HostCaretKind::Block
            && let Some((block_id, _)) = position.as_outside()
            && let Some(edit) = self.math_edit_at_block(block_id, direction)?
        {
            self.clear_host_caret_position();
            self.set_math_edit(Some(edit));
            self.advance_history_merge_epoch();
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        if self.enter_adjacent_math(direction, false)? {
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        self.enter_math_at_text_edge(direction)
    }

    fn enter_math_at_text_edge(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let focus = self.selection.focus.clone();
        if self.selection.anchor != focus
            || self.has_structure_selection()
            || self.composition.is_some()
            || self.command_line.is_some()
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
        if let Some(position) = self.host_caret_position.clone() {
            let can_enter_adjacent_inline_math = slot.is_some()
                && position.kind() == HostCaretKind::Inline
                && position.outside_direction() == Some(direction)
                && self
                    .text_map_for(&focus)?
                    .is_empty_inline_environment(focus.offset, position.host_id());
            if !can_enter_adjacent_inline_math {
                return Ok(Effect::default());
            }
        }
        if let Some((start, math_id)) = slot {
            let tree = find_inline_math(&paragraph.inlines, &math_id)
                .ok_or(EditError::InvalidPosition)?
                .math_tree
                .clone();
            let (cursor, branch) = math_tree_edge_position(
                &tree,
                MathPositionHost::InlineMath {
                    owner_id: math_id.clone(),
                },
                direction,
                None,
            )?;
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
            self.clear_host_caret_position();
            let target = MathEditTarget::Inline {
                paragraph_id: paragraph.id.clone(),
                math_id,
            };
            self.set_math_edit(Some(MathEdit::entering(
                target.clone(),
                cursor,
                branch,
                self.current_math_edit_revision(),
            )));
            self.advance_history_merge_epoch();
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        let at_edge = match direction {
            Direction::Forward => focus.offset == TextMap::new(&paragraph)?.text.len(),
            Direction::Backward => focus.offset == 0,
        };
        if !at_edge {
            return Ok(Effect::default());
        }
        let Some(location) = self.paragraph_index.get(&focus.paragraph_id).cloned() else {
            return Ok(Effect::default());
        };
        if location.is_title {
            return Ok(Effect::default());
        }
        let Some((container, sibling_index)) =
            paragraph_ops::paragraph_parent_container_path(&self.document.body, &location)
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
        let block = caret_structure::block_list_by_container(&self.document.body, &container)
            .ok_or(EditError::InvalidPosition)?
            .get(neighbour)
            .ok_or(EditError::InvalidPosition)?;
        let Some(edit) = self.math_edit_for_block(block, direction)? else {
            return Ok(Effect::default());
        };
        self.set_math_edit(Some(edit));
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    pub(in crate::editing::session) fn focus_merged_inline_math_at(
        &mut self,
        paragraph_id: &NodeId,
        math_id: &NodeId,
        root_offset: usize,
        affinity: MathPositionAffinity,
    ) -> Result<(), EditError> {
        let story = self
            .story_for_paragraph(paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
        let lookup = Pos {
            story: story.clone(),
            paragraph_id: paragraph_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        let paragraph = self.paragraph_for(&lookup)?;
        let math =
            find_inline_math(&paragraph.inlines, math_id).ok_or(EditError::InvalidPosition)?;
        if root_offset > math.math_tree.root.content.len() {
            return Err(EditError::InvalidPosition);
        }
        let start = inline_math_slots(paragraph)?
            .into_iter()
            .find(|(_, id)| id == math_id)
            .map(|(start, _)| start)
            .ok_or(EditError::InvalidPosition)?;
        let position = Pos {
            story,
            paragraph_id: paragraph_id.clone(),
            offset: start + OBJECT_REPLACEMENT.len_utf8(),
            affinity: Affinity::Upstream,
        };
        self.selection = Selection {
            anchor: position.clone(),
            focus: position,
        };
        // 兼容的正文停靠点固定在 U+FFFC 后侧；公式内部 caret affinity
        // 独立保存在 MathTreePosition，不能用正文视觉侧替代。
        self.selection_visual_affinity = VisualAffinity::Upstream;
        let host = MathPositionHost::InlineMath {
            owner_id: math_id.clone(),
        };
        let path = MathSequencePath::root();
        let anchor = MathTreePosition {
            host,
            path: path.clone(),
            offset: root_offset,
            affinity,
        };
        self.set_math_edit(Some(MathEdit::entering(
            MathEditTarget::Inline {
                paragraph_id: paragraph_id.clone(),
                math_id: math_id.clone(),
            },
            root_offset,
            MathEditBranch::Tree { path, anchor },
            self.current_math_edit_revision(),
        )));
        self.advance_history_merge_epoch();
        Ok(())
    }
}
