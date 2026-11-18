//! 承载数学树分支的文本、结构与删除操作。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

impl Session {
    pub(in crate::editing::session) fn math_edit_is_text_branch(&self) -> bool {
        self.math_edit
            .as_ref()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Text { .. }))
    }

    pub(in crate::editing::session) fn math_edit_is_tree_branch(&self) -> bool {
        self.math_edit
            .as_ref()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Tree { .. }))
    }

    pub(in crate::editing::session) fn cancel_math_source_draft(&mut self) {
        if let Some(edit) = &mut self.math_edit {
            if let Some(draft) = &edit.source_draft {
                edit.cursor = draft.base_cursor();
            }
            edit.source_draft = None;
        }
    }

    pub(super) fn math_tree_insert_text(&mut self, text: &str) -> Result<Effect, EditError> {
        if text == "&"
            && self.math_edit.as_ref().is_some_and(|edit| {
                matches!(edit.target, MathEditTarget::Block { align: true, .. })
            })
        {
            return self.math_tree_align_break(false);
        }
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree {
            mut path,
            mut anchor,
        } = edit.branch.clone()
        else {
            return Err(EditError::InvalidPosition);
        };
        if anchor.path != path {
            return Err(EditError::InvalidPosition);
        }
        if text.contains('\n') || text.contains('\r') {
            return Err(EditError::UnsupportedNewline);
        }
        let align_cell = self.align_cell_for_position(&edit, &anchor)?;
        let mut tree = self.math_tree_for_edit(&edit, align_cell)?.clone();
        let original_tree = tree.clone();
        let mut start = anchor.offset.min(edit.cursor);
        let mut end = anchor.offset.max(edit.cursor);
        let boundaries = GraphemeMap::new_with_fences(text, &[]).into_boundaries();
        let graphemes = boundaries
            .windows(2)
            .map(|pair| text.get(pair[0]..pair[1]).ok_or(EditError::InvalidPosition))
            .collect::<Result<Vec<_>, _>>()?;
        let has_insertable_grapheme = graphemes.iter().any(|grapheme| {
            grapheme
                .chars()
                .next()
                .is_some_and(|character| !character.is_whitespace())
        });
        let starts_with_plain_text = graphemes
            .iter()
            .find(|grapheme| {
                grapheme
                    .chars()
                    .next()
                    .is_some_and(|character| !character.is_whitespace())
            })
            .is_some_and(|grapheme| is_literal_math_input(grapheme));
        if start == 0
            && start == end
            && starts_with_plain_text
            && let Some((parent_path, script_index)) =
                script_base_parent_for_start_insertion(&tree, &anchor)
        {
            // 非空脚标基底的起点与脚标节点外边界重合；普通文本应插入在整个脚标之前。
            path = parent_path;
            anchor.path = path.clone();
            anchor.offset = script_index;
            start = script_index;
            end = script_index;
        }
        let contents = tree_position::resolve_sequence_mut(&mut tree, &path)
            .ok_or(EditError::InvalidPosition)?;
        if end > contents.len() {
            return Err(EditError::InvalidPosition);
        }
        if start == end
            && has_insertable_grapheme
            && let Some(placeholder_index) = tree_position::placeholder_run_start(contents, start)
        {
            // 防止旧导航状态把输入插到必填空槽占位符之后。
            start = placeholder_index;
            end = placeholder_index;
        }
        let replaces_placeholder = start == end
            && has_insertable_grapheme
            && matches!(contents.get(start), Some(MathContent::Math(node)) if node.kind == MathNodeKind::Placeholder);
        if replaces_placeholder {
            let placeholder_end = (start..contents.len())
                .take_while(|index| {
                    matches!(
                        contents.get(*index),
                        Some(MathContent::Math(node))
                            if node.kind == MathNodeKind::Placeholder
                    )
                })
                .last()
                .map_or(start, |index| index + 1);
            if contents[start..placeholder_end]
                .iter()
                .any(math_content_has_unknown_metadata)
            {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            contents.drain(start..placeholder_end);
        }
        let selected = contents.drain(start..end).collect::<Vec<_>>();
        let mut selected_base = Some(selected);
        let initial_position = anchor.clone();
        let mut position = anchor;
        position.offset = start;
        let mut literal_batch = String::new();
        for grapheme in graphemes {
            let Some(character) = grapheme.chars().next() else {
                continue;
            };
            if character.is_whitespace() {
                continue;
            }
            match grapheme {
                "^" | "_" => {
                    flush_math_literal_batch(&mut tree, &mut position, &mut literal_batch)?;
                    insert_math_script(&mut tree, &mut position, grapheme, selected_base.take())?;
                }
                "{" => {
                    flush_math_literal_batch(&mut tree, &mut position, &mut literal_batch)?;
                    open_math_group(&mut tree, &mut position)?;
                }
                "}" => {
                    flush_math_literal_batch(&mut tree, &mut position, &mut literal_batch)?;
                    close_math_group(&tree, &mut position)?;
                }
                _ if is_literal_math_input(grapheme) => {
                    selected_base = None;
                    literal_batch.push_str(grapheme);
                }
                _ => {
                    flush_math_literal_batch(&mut tree, &mut position, &mut literal_batch)?;
                    selected_base = None;
                    insert_math_symbol(&mut tree, &mut position, grapheme)?;
                }
            }
        }
        flush_math_literal_batch(&mut tree, &mut position, &mut literal_batch)?;
        if start == end && position == initial_position && tree == original_tree {
            return Ok(Effect::default());
        }
        tree.validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let cursor = position.offset;
        self.commit_math_text_tree(
            edit,
            tree,
            text_branch::MathTextTreeCommit {
                branch: MathEditBranch::Tree {
                    path: position.path.clone(),
                    anchor: position,
                },
                align_cell,
                cursor,
                merge_group: Some(EditGroup::InsertText),
                pending_style: text_branch::PendingStylePolicy::Clear,
                selection_changed: true,
            },
        )
    }

    pub(in crate::editing::session) fn math_tree_align_break(
        &mut self,
        new_row: bool,
    ) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Block {
            block_id,
            align: true,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        let MathEditBranch::Tree { path, anchor } = edit.branch.clone() else {
            return Err(EditError::InvalidPosition);
        };
        if !path.steps().is_empty() || anchor.path != path {
            return Err(EditError::InvalidPosition);
        }
        let (row_index, cell_index) = self
            .align_cell_for_position(&edit, &anchor)?
            .ok_or(EditError::InvalidPosition)?;
        let Block::Align(before_align) =
            find_block_by_id(&self.document.body, block_id).ok_or(EditError::InvalidPosition)?
        else {
            return Err(EditError::InvalidPosition);
        };
        let before_align = before_align.clone();
        let mut after_align = before_align.clone();
        let row = after_align
            .rows
            .get_mut(row_index)
            .ok_or(EditError::InvalidPosition)?;
        let cell = row
            .cells
            .get_mut(cell_index)
            .ok_or(EditError::InvalidPosition)?;
        let start = anchor.offset.min(edit.cursor);
        let end = anchor.offset.max(edit.cursor);
        let content = &cell.root.content;
        if end > content.len() {
            return Err(EditError::InvalidPosition);
        }
        let left = content[..start].to_vec();
        let right = content[end..].to_vec();
        cell.root.content = left;
        let mut next_tree = empty_math_tree();
        next_tree.root.content = right;
        let next_row_id = if new_row {
            let cell_count = row.cells.len();
            let mut cells = (0..cell_count)
                .map(|_| empty_math_tree())
                .collect::<Vec<_>>();
            let next = cells
                .get_mut(cell_index)
                .ok_or(EditError::InvalidPosition)?;
            next.root.content = std::mem::take(&mut next_tree.root.content);
            let id = NodeId::generate();
            after_align.rows.insert(
                row_index + 1,
                MathAlignRow {
                    id: id.clone(),
                    cells,
                    extra: BTreeMap::new(),
                },
            );
            Some(id)
        } else {
            row.cells.insert(cell_index + 1, next_tree);
            None
        };
        let next_row_index = if new_row { row_index + 1 } else { row_index };
        let next_cell_index = if new_row { cell_index } else { cell_index + 1 };
        let next_align_row = after_align
            .rows
            .get(next_row_index)
            .ok_or(EditError::InvalidPosition)?;
        let next_cell = next_align_row
            .cells
            .get(next_cell_index)
            .ok_or(EditError::InvalidPosition)?;
        let position = MathTreePosition {
            host: MathPositionHost::AlignCell {
                row_id: next_row_id.unwrap_or_else(|| next_align_row.id.clone()),
                cell_id: next_cell.root.id.clone(),
            },
            path: MathSequencePath::root(),
            offset: 0,
            affinity: MathPositionAffinity::Downstream,
        };
        multiline::commit_align_tree_edit(
            self,
            edit,
            before_align,
            after_align,
            position.offset,
            MathEditBranch::Tree {
                path: position.path.clone(),
                anchor: position,
            },
        )
    }

    pub(super) fn math_tree_insert_template(
        &mut self,
        template: &str,
        cursor_in_template: usize,
    ) -> Result<Effect, EditError> {
        if cursor_in_template > template.len() || !template.is_char_boundary(cursor_in_template) {
            return Err(EditError::InvalidPosition);
        }
        let parsed = caret_math::parse_projection(template, caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let (parsed, _) = parsed.into_parts();
        let caret_math::ProjectedMath::Tree(mut template_tree) = parsed else {
            return Err(EditError::InvalidMath(
                "expected inline math template".to_owned(),
            ));
        };
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree { path, anchor } = edit.branch.clone() else {
            return Err(EditError::InvalidPosition);
        };
        if anchor.path != path {
            return Err(EditError::InvalidPosition);
        }
        let align_cell = self.align_cell_for_position(&edit, &anchor)?;
        let mut tree = self.math_tree_for_edit(&edit, align_cell)?.clone();
        if matches!(template, "^{}" | "_{}") {
            let contents = tree_position::resolve_sequence_mut(&mut tree, &path)
                .ok_or(EditError::InvalidPosition)?;
            let start = anchor.offset.min(edit.cursor);
            let end = anchor.offset.max(edit.cursor);
            if end > contents.len() {
                return Err(EditError::InvalidPosition);
            }
            let selected = contents.drain(start..end).collect();
            let mut position = anchor;
            position.offset = start;
            let script = if template == "^{}" { "^" } else { "_" };
            insert_math_script(&mut tree, &mut position, script, Some(selected))?;
            tree.validate()
                .map_err(|error| EditError::InvalidMath(error.to_string()))?;
            let cursor = position.offset;
            return self.commit_math_text_tree(
                edit,
                tree,
                text_branch::MathTextTreeCommit {
                    branch: MathEditBranch::Tree {
                        path: position.path.clone(),
                        anchor: position,
                    },
                    align_cell,
                    cursor,
                    merge_group: None,
                    pending_style: text_branch::PendingStylePolicy::Clear,
                    selection_changed: true,
                },
            );
        }
        let contents = tree_position::resolve_sequence_mut(&mut tree, &path)
            .ok_or(EditError::InvalidPosition)?;
        let start = anchor.offset.min(edit.cursor);
        let end = anchor.offset.max(edit.cursor);
        if end > contents.len() {
            return Err(EditError::InvalidPosition);
        }

        let placeholder = if template
            .get(cursor_in_template..)
            .is_some_and(|remaining| remaining.starts_with('}'))
        {
            first_placeholder_position(
                &template_tree,
                &MathSequencePath::root(),
                0..template_tree.root.content.len(),
            )
        } else {
            None
        };
        if let Some((placeholder_path, placeholder_index)) = &placeholder {
            let sequence =
                tree_position::resolve_sequence_mut(&mut template_tree, placeholder_path)
                    .ok_or(EditError::InvalidPosition)?;
            if !matches!(sequence.get(*placeholder_index), Some(MathContent::Math(node)) if node.kind == MathNodeKind::Placeholder)
            {
                return Err(EditError::InvalidPosition);
            }
            sequence.remove(*placeholder_index);
        }

        let text_content_index = template_tree
            .root
            .content
            .iter()
            .position(|content| matches!(content, MathContent::Text(_)));
        let inserted_len = template_tree.root.content.len();
        if start == end && inserted_len == 0 {
            return Ok(Effect::default());
        }
        contents.splice(start..end, template_tree.root.content);
        tree.validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;

        if let Some(content_index) = text_content_index {
            let content_index = start
                .checked_add(content_index)
                .ok_or(EditError::InvalidPosition)?;
            let text_path =
                tree_position::text_path_for_content(&anchor, content_index, align_cell);
            return self.commit_math_text_tree(
                edit,
                tree,
                text_branch::MathTextTreeCommit {
                    branch: MathEditBranch::text(
                        text_path,
                        0,
                        MathPositionAffinity::Downstream,
                        VisualAffinity::Downstream,
                    ),
                    align_cell,
                    cursor: 0,
                    merge_group: None,
                    pending_style: text_branch::PendingStylePolicy::Clear,
                    selection_changed: true,
                },
            );
        }

        let position = if let Some((relative_path, offset)) = placeholder {
            let mut steps = path.steps().to_vec();
            let mut relative = relative_path.steps().to_vec();
            if let Some(first) = relative.first_mut() {
                match first {
                    MathSequenceStep::Operand { content_index, .. }
                    | MathSequenceStep::EnvironmentChildren { content_index } => {
                        *content_index = content_index
                            .checked_add(start)
                            .ok_or(EditError::InvalidPosition)?;
                    }
                }
            }
            steps.extend(relative);
            MathTreePosition {
                host: anchor.host,
                path: MathSequencePath::new(steps),
                offset,
                affinity: anchor.affinity,
            }
        } else {
            let mut position = anchor;
            position.offset = start
                .checked_add(inserted_len)
                .ok_or(EditError::InvalidPosition)?;
            position
        };
        let cursor = position.offset;
        self.commit_math_text_tree(
            edit,
            tree,
            text_branch::MathTextTreeCommit {
                branch: MathEditBranch::Tree {
                    path: position.path.clone(),
                    anchor: position,
                },
                align_cell,
                cursor,
                merge_group: None,
                pending_style: text_branch::PendingStylePolicy::Clear,
                selection_changed: true,
            },
        )
    }

    pub(super) fn math_tree_delete(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree { path, anchor } = edit.branch.clone() else {
            return Err(EditError::InvalidPosition);
        };
        if anchor.path != path {
            return Err(EditError::InvalidPosition);
        }
        let align_cell = self.align_cell_for_position(&edit, &anchor)?;
        let mut tree = self.math_tree_for_edit(&edit, align_cell)?.clone();
        let contents = tree_position::resolve_sequence_mut(&mut tree, &path)
            .ok_or(EditError::InvalidPosition)?;
        let start = anchor.offset.min(edit.cursor);
        let end = anchor.offset.max(edit.cursor);
        if end > contents.len() {
            return Err(EditError::InvalidPosition);
        }
        if matches!(direction, Direction::Backward)
            && start == end
            && path.steps().is_empty()
            && let Some(effect) =
                multiline::handle_tree_backspace(self, &edit, align_cell, start, end)?
        {
            return Ok(effect);
        }
        let deletion = if start < end {
            start..end
        } else {
            match direction {
                Direction::Backward => start.checked_sub(1).map(|index| index..start),
                Direction::Forward => (start < contents.len()).then_some(start..start + 1),
            }
            .unwrap_or_default()
        };
        if deletion.is_empty() {
            if matches!(direction, Direction::Backward) {
                if path.steps().is_empty() && math_sequence_is_empty(contents) {
                    if let MathEditTarget::Block {
                        block_id,
                        align: true,
                    } = &edit.target
                    {
                        let Block::Align(align) = find_block_by_id(&self.document.body, block_id)
                            .ok_or(EditError::InvalidPosition)?
                        else {
                            return Err(EditError::InvalidPosition);
                        };
                        let occupied = align.rows.iter().any(|row| {
                            row.cells
                                .iter()
                                .any(|cell| !math_sequence_is_empty(&cell.root.content))
                        });
                        if occupied {
                            let cells = align
                                .rows
                                .iter()
                                .enumerate()
                                .flat_map(|(row, value)| {
                                    value
                                        .cells
                                        .iter()
                                        .enumerate()
                                        .map(move |(cell, _)| (row, cell))
                                })
                                .collect::<Vec<_>>();
                            let current = align_cell.ok_or(EditError::InvalidPosition)?;
                            let current_index = cells
                                .iter()
                                .position(|cell| *cell == current)
                                .ok_or(EditError::InvalidPosition)?;
                            let neighbor = current_index
                                .checked_sub(1)
                                .and_then(|index| cells.get(index).copied())
                                .or_else(|| cells.get(current_index + 1).copied())
                                .ok_or(EditError::InvalidPosition)?;
                            let (cursor, branch) = math_tree_edge_position(
                                self.math_tree_for_edit(&edit, Some(neighbor))?,
                                MathPositionHost::AlignCell {
                                    row_id: align.rows[neighbor.0].id.clone(),
                                    cell_id: align.rows[neighbor.0].cells[neighbor.1]
                                        .root
                                        .id
                                        .clone(),
                                },
                                if current_index > 0 {
                                    Direction::Backward
                                } else {
                                    Direction::Forward
                                },
                                Some(neighbor),
                            )?;
                            if let Some(active) = &mut self.math_edit {
                                active.cursor = cursor;
                                active.branch = branch;
                            }
                            self.advance_history_merge_epoch();
                            return Ok(Effect {
                                selection_changed: true,
                                ..Effect::default()
                            });
                        }
                    }
                    return self.delete_math_object();
                }

                if let Some((parent_path, content_index, empty_structure)) =
                    empty_math_operand_parent(&tree, &path)?
                {
                    if empty_structure {
                        let parent = tree_position::resolve_sequence_mut(&mut tree, &parent_path)
                            .ok_or(EditError::InvalidPosition)?;
                        let content = parent
                            .get(content_index)
                            .ok_or(EditError::InvalidPosition)?;
                        if math_content_has_unknown_metadata(content) {
                            return Err(EditError::UnsupportedOpaqueContent);
                        }
                        parent.remove(content_index);
                        let mut next_position = anchor;
                        next_position.path = parent_path;
                        next_position.offset = content_index;
                        tree.validate()
                            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
                        return self.commit_math_text_tree(
                            edit,
                            tree,
                            text_branch::MathTextTreeCommit {
                                branch: MathEditBranch::Tree {
                                    path: next_position.path.clone(),
                                    anchor: next_position,
                                },
                                align_cell,
                                cursor: content_index,
                                merge_group: Some(EditGroup::DeleteBackward),
                                pending_style: text_branch::PendingStylePolicy::Clear,
                                selection_changed: true,
                            },
                        );
                    }
                    let position = MathTreePosition {
                        host: anchor.host,
                        path: parent_path,
                        offset: content_index,
                        affinity: anchor.affinity,
                    };
                    if let Some(active) = &mut self.math_edit {
                        active.cursor = content_index;
                        active.branch = MathEditBranch::Tree {
                            path: position.path.clone(),
                            anchor: position,
                        };
                    }
                    self.advance_history_merge_epoch();
                    return Ok(Effect {
                        selection_changed: true,
                        ..Effect::default()
                    });
                }
            }
            return Ok(Effect::default());
        }
        if start == end
            && let Some(MathContent::Text(text)) = contents.get(deletion.start)
        {
            let text_path =
                tree_position::text_path_for_content(&anchor, deletion.start, align_cell);
            let cursor = match direction {
                Direction::Backward => text.runs.iter().map(|run| run.text.len()).sum(),
                Direction::Forward => 0,
            };
            let original_edit = edit.clone();
            let mut text_edit = edit;
            text_edit.cursor = cursor;
            text_edit.branch = MathEditBranch::text(
                text_path,
                cursor,
                MathPositionAffinity::Downstream,
                VisualAffinity::Downstream,
            );
            self.set_math_edit(Some(text_edit.clone()));
            return match self.math_text_delete(direction) {
                Ok(effect) => Ok(effect),
                Err(error) => {
                    self.set_math_edit(Some(original_edit));
                    Err(error)
                }
            };
        }
        if contents[deletion.clone()]
            .iter()
            .any(math_content_has_unknown_metadata)
        {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        contents.drain(deletion.clone());
        let resegmented_affinity = match direction {
            Direction::Backward => MathPositionAffinity::Downstream,
            Direction::Forward => MathPositionAffinity::Upstream,
        };
        let cursor =
            resegment_literal_symbol_run(contents, deletion.start, None, resegmented_affinity)?
                .unwrap_or(deletion.start);
        tree.validate()
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let mut next_position = anchor;
        next_position.offset = cursor;
        self.commit_math_text_tree(
            edit,
            tree,
            text_branch::MathTextTreeCommit {
                branch: MathEditBranch::Tree {
                    path,
                    anchor: next_position,
                },
                align_cell,
                cursor,
                merge_group: Some(match direction {
                    Direction::Backward => EditGroup::DeleteBackward,
                    Direction::Forward => EditGroup::DeleteForward,
                }),
                pending_style: text_branch::PendingStylePolicy::Clear,
                selection_changed: true,
            },
        )
    }
}

impl Session {
    /// 公式子命令：在光标处插入模板，光标进入模板的第一个空槽 `{}`。
    pub(in crate::editing::session) fn math_insert_template(
        &mut self,
        template: &str,
    ) -> Result<Effect, EditError> {
        let (template, cursor) = if is_bare_math_control_word(template) {
            let mut delimited = String::with_capacity(template.len() + 1);
            delimited.push_str(template);
            delimited.push(' ');
            (delimited, template.len() + 1)
        } else {
            let cursor = template
                .find("{}")
                .map_or(template.len(), |index| index + 1);
            (template.to_owned(), cursor)
        };
        if template_has_root_text_flow(&template)?
            && let Some(edit) = self.math_edit.clone()
            && matches!(edit.target, MathEditTarget::Inline { .. })
            && matches!(
                &edit.branch,
                MathEditBranch::Tree { path, anchor }
                    if path.steps().is_empty() && anchor.path.steps().is_empty()
            )
        {
            // InlineMath 根流不能保存 TextFlow；通过源码事务触发同一根层提升与焦点映射。
            let previous_edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
            let tree = self.math_tree_for_edit(&edit, None)?.clone();
            let (projection, _) = crate::math_validation::project_tree_for_edit(
                &tree,
                caret_math::MathInputKind::Inline,
            )
            .map_err(EditError::InvalidMath)?;
            let source = projection.edit_source();
            let source_map = projection.tree().ok_or(EditError::InvalidPosition)?;
            let MathEditBranch::Tree { anchor, .. } = &edit.branch else {
                return Err(EditError::InvalidPosition);
            };
            let source_anchor = inline_root_source_cursor(&tree, source_map, anchor.offset, source)
                .ok_or(EditError::InvalidPosition)?;
            let source_cursor = inline_root_source_cursor(&tree, source_map, edit.cursor, source)
                .ok_or(EditError::InvalidPosition)?;
            let source_edit = self.math_edit.as_mut().ok_or(EditError::InvalidPosition)?;
            source_edit.replace_source_projection(Some(Arc::new(projection)));
            source_edit.cursor = source_cursor;
            source_edit.branch = MathEditBranch::Source;
            let result = self.math_insert_raw_with_range(
                &template,
                cursor,
                source_anchor.min(source_cursor)..source_anchor.max(source_cursor),
                Some(MathEditPosition::from_edit(&previous_edit)),
            );
            if result.is_err() {
                self.set_math_edit(Some(previous_edit));
            }
            return result;
        }
        if self.math_edit_is_text_branch() {
            return self.math_text_enter_math_at(&template, cursor);
        }
        if self.math_edit_is_tree_branch() {
            return self.math_tree_insert_template(&template, cursor);
        }
        self.math_insert_raw(&template, cursor)
    }

    pub(super) fn math_insert_text(&mut self, text: &str) -> Result<Effect, EditError> {
        let edit = self.math_edit.as_ref().ok_or(EditError::InvalidPosition)?;
        let source_edit = edit.cursor..edit.cursor;
        let mut source = edit.source().ok_or(EditError::InvalidPosition)?.to_owned();
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

    pub(in crate::editing::session) fn math_insert_raw(
        &mut self,
        text: &str,
        cursor_in_text: usize,
    ) -> Result<Effect, EditError> {
        let edit = self.math_edit.as_ref().ok_or(EditError::InvalidPosition)?;
        let source_edit = edit.cursor..edit.cursor;
        self.math_insert_raw_with_range(text, cursor_in_text, source_edit, None)
    }

    pub(super) fn math_insert_raw_with_range(
        &mut self,
        text: &str,
        cursor_in_text: usize,
        source_edit: Range<usize>,
        before_math_position: Option<MathEditPosition>,
    ) -> Result<Effect, EditError> {
        let edit = self.math_edit.as_ref().ok_or(EditError::InvalidPosition)?;
        let mut source = edit.source().ok_or(EditError::InvalidPosition)?.to_owned();
        if source_edit.start > source_edit.end
            || source_edit.end > source.len()
            || !source.is_char_boundary(source_edit.start)
            || !source.is_char_boundary(source_edit.end)
        {
            return Err(EditError::InvalidPosition);
        }
        source.replace_range(source_edit.clone(), text);
        let cursor = source_edit.start + cursor_in_text;
        self.write_math_source_with_metadata(
            source,
            cursor,
            None,
            source_edit,
            MathSourceWriteOptions {
                before_math_position_override: before_math_position,
                reject_unmapped_metadata: false,
            },
        )
    }

    pub(super) fn math_delete_backward(&mut self) -> Result<Effect, EditError> {
        let edit = self
            .math_edit
            .as_ref()
            .map(MathEdit::command_snapshot)
            .ok_or(EditError::InvalidPosition)?;
        if let Some(effect) = multiline::handle_source_backspace(self, &edit)? {
            return Ok(effect);
        }
        let body = math_input_body_range(&edit).ok_or(EditError::InvalidPosition)?;
        let current_source = edit.source().ok_or(EditError::InvalidPosition)?;
        let body_source = current_source
            .get(body.clone())
            .ok_or(EditError::InvalidPosition)?;
        if body_source.trim().is_empty() && !edit.source_draft_is_pending() {
            return self.delete_math_object();
        }
        let source = current_source;
        let Some(cursor) = edit.cursor.checked_sub(body.start) else {
            return Err(EditError::InvalidPosition);
        };
        if cursor == 0 {
            return self.exit_math_edit(Direction::Backward, false);
        }
        if !edit.source_draft_is_pending()
            && let Some(construct_range) = self.empty_construct_range(&edit)
        {
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

    pub(super) fn math_delete_forward(&mut self) -> Result<Effect, EditError> {
        let edit = self
            .math_edit
            .as_ref()
            .map(MathEdit::command_snapshot)
            .ok_or(EditError::InvalidPosition)?;
        let body = math_input_body_range(&edit).ok_or(EditError::InvalidPosition)?;
        let source = edit.source().ok_or(EditError::InvalidPosition)?;
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

    pub(super) fn math_move(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let effect = self.math_move_inner(direction)?;
        if effect.selection_changed {
            self.pending_inline_style.clear();
        }
        Ok(effect)
    }

    fn math_move_inner(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let edit = self
            .math_edit
            .as_ref()
            .map(MathEdit::command_snapshot)
            .ok_or(EditError::InvalidPosition)?;
        if let MathEditBranch::Tree { path, anchor } = &edit.branch {
            if anchor.path != *path {
                return Err(EditError::InvalidPosition);
            }
            if anchor.offset != edit.cursor {
                let cursor = match direction {
                    Direction::Backward => anchor.offset.min(edit.cursor),
                    Direction::Forward => anchor.offset.max(edit.cursor),
                };
                let mut position = anchor.clone();
                position.offset = cursor;
                if let Some(current) = &mut self.math_edit {
                    current.cursor = cursor;
                    current.branch = MathEditBranch::Tree {
                        path: position.path.clone(),
                        anchor: position,
                    };
                }
                self.advance_history_merge_epoch();
                return Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                });
            }
            let align_cell = self.align_cell_for_position(&edit, anchor)?;
            let tree = self.math_tree_for_edit(&edit, align_cell)?;
            let mut position = anchor.clone();
            position.offset = edit.cursor;
            let movement = advance_math_tree_position(
                tree,
                &position,
                direction == Direction::Forward,
                align_cell,
            )
            .map_err(|_| EditError::InvalidPosition)?;
            return self.apply_math_tree_caret_move(&edit, movement, direction);
        }
        if let MathEditBranch::Text { path, anchor, .. } = &edit.branch {
            let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
            let (island, range) = math_text_selection_range(tree, path, anchor, edit.cursor)
                .ok_or(EditError::InvalidPosition)?;
            if !range.is_empty() {
                let cursor = match direction {
                    Direction::Backward => range.start,
                    Direction::Forward => range.end,
                };
                let affinity = match direction {
                    Direction::Backward => MathPositionAffinity::Upstream,
                    Direction::Forward => MathPositionAffinity::Downstream,
                };
                let position = island
                    .position_at_offset(cursor, affinity)
                    .ok_or(EditError::InvalidPosition)?;
                if let Some(current) = &mut self.math_edit {
                    current.cursor = position.offset;
                    // 文本 caret 的视觉侧与块外侧停靠共用同一方向投影：向前落在下游。
                    current.branch = MathEditBranch::text(
                        position.path,
                        position.offset,
                        position.affinity,
                        visual_affinity_for_block_edge(direction),
                    );
                }
                self.advance_history_merge_epoch();
                return Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                });
            }
            let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
            match math_text_caret_step(tree, path, edit.cursor, direction)? {
                MathTextCaretStep::Local(cursor) => {
                    if let Some(current) = &mut self.math_edit {
                        current.cursor = cursor;
                        if let MathEditBranch::Text {
                            path: current_path,
                            anchor,
                            focus_affinity,
                            anchor_visual_affinity,
                            visual_affinity,
                            ..
                        } = &mut current.branch
                        {
                            *anchor = MathTextPosition::new(
                                current_path.clone(),
                                cursor,
                                MathPositionAffinity::Downstream,
                            );
                            *focus_affinity = MathPositionAffinity::Downstream;
                            let next_visual_affinity = visual_affinity_for_block_edge(direction);
                            *anchor_visual_affinity = next_visual_affinity;
                            *visual_affinity = next_visual_affinity;
                        }
                    }
                }
                MathTextCaretStep::CrossFlow(next_path, cursor) => {
                    if let Some(current) = &mut self.math_edit {
                        current.branch = MathEditBranch::text(
                            next_path,
                            cursor,
                            MathPositionAffinity::Downstream,
                            visual_affinity_for_block_edge(direction),
                        );
                        current.cursor = cursor;
                    }
                }
                MathTextCaretStep::Boundary => {
                    if let Some(adjacent_path) =
                        path::adjacent_text_flow_path(tree, path, direction == Direction::Forward)
                    {
                        let adjacent_text = adjacent_path
                            .resolve_structurally(tree)
                            .ok_or(EditError::InvalidPosition)?;
                        let adjacent_display = adjacent_text
                            .runs
                            .iter()
                            .map(|run| run.text.as_str())
                            .collect::<String>();
                        let cursor = if direction == Direction::Forward {
                            0
                        } else {
                            adjacent_display.len()
                        };
                        if let Some(current) = &mut self.math_edit {
                            current.branch = MathEditBranch::text(
                                adjacent_path,
                                cursor,
                                MathPositionAffinity::Downstream,
                                visual_affinity_for_block_edge(direction),
                            );
                            current.cursor = cursor;
                        }
                        self.advance_history_merge_epoch();
                        return Ok(Effect {
                            selection_changed: true,
                            ..Effect::default()
                        });
                    }
                    let tree_root_edge = edit.source_projection.is_none()
                        && path.align_cell().is_none()
                        && !path
                            .steps()
                            .iter()
                            .any(|step| matches!(step, MathTextFlowPathStep::Operand(_)))
                        && path::root_text_flow_path(tree, direction == Direction::Backward, None)
                            .is_some_and(|edge| edge.steps() == path.steps());
                    if tree_root_edge {
                        return self.exit_math_edit_for_navigation(direction);
                    }
                    let (tree, position, align_cell) =
                        self.math_tree_position_at_text_boundary(&edit, path, direction)?;
                    let movement = advance_math_tree_position(
                        tree,
                        &position,
                        direction == Direction::Forward,
                        align_cell,
                    )
                    .map_err(|_| EditError::InvalidPosition)?;
                    return self.apply_math_tree_caret_move(&edit, movement, direction);
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
            .source()
            .ok_or(EditError::InvalidPosition)?
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
            None => self.exit_math_edit_for_navigation(direction),
        }
    }
}
