//! 将数学树与源码投影的提交、删除和历史记录集中在一个职责模块。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::history::DocumentEditSnapshot;
use super::*;

impl Session {
    /// 空公式中退格：删除整个公式对象并退出编辑。
    pub(super) fn delete_math_object(&mut self) -> Result<Effect, EditError> {
        let Some(edit) = self.math_edit.clone() else {
            return Ok(Effect::default());
        };
        let before_math_position = MathEditPosition::from_edit(&edit);
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
                let result_map = TextMap::new(&paragraph)?;
                let caret_affinity = if result_map.grapheme_boundaries.binary_search(&start).is_ok()
                {
                    Affinity::Downstream
                } else {
                    Affinity::Upstream
                };
                let caret_offset = result_map.snap_to_grapheme_boundary(start, caret_affinity);
                let result_boundaries = result_map.grapheme_boundaries.clone();
                self.replace_paragraph(&position, paragraph)?;
                self.text_map_cache.clear();
                let caret = Pos {
                    offset: caret_offset,
                    affinity: caret_affinity,
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
                self.clear_math_edit();
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
                    changed_paragraph_id: Some(paragraph_id.clone()),
                    position_map: Some(PositionMap::TextEdit {
                        paragraph_id,
                        start,
                        end: start + OBJECT_REPLACEMENT.len_utf8(),
                        inserted_len: 0,
                        snap_graphemes: true,
                        result_boundaries,
                    }),
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
                self.clear_math_edit();
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
                    before_state_id,
                );
                self.commit_document_edit(
                    DocumentPatch::BlockRange {
                        parent_environment_ids: parent_ids,
                        sibling_index: index,
                        before: vec![removed],
                        after: Vec::new(),
                    },
                    before_state,
                    DocumentEditSnapshot::Explicit(Box::new(after_state)),
                );
                Ok(Effect {
                    document_changed: true,
                    selection_changed: true,
                    ..Effect::default()
                })
            }
        }
    }

    /// 解析候选源码并提交结构树；可继续输入的失败保留为独立草稿。
    pub(super) fn write_math_source(
        &mut self,
        source: String,
        cursor: usize,
        merge_group: Option<EditGroup>,
        old_edit: std::ops::Range<usize>,
    ) -> Result<Effect, EditError> {
        self.write_math_source_with_metadata(
            source,
            cursor,
            merge_group,
            old_edit,
            MathSourceWriteOptions {
                before_math_position_override: None,
                reject_unmapped_metadata: false,
            },
        )
    }

    /// 提交必须保留既有结构元数据的源码变更；无法映射元数据时拒绝而不进入草稿。
    pub(super) fn write_math_source_strict(
        &mut self,
        source: String,
        cursor: usize,
        merge_group: Option<EditGroup>,
        old_edit: std::ops::Range<usize>,
    ) -> Result<Effect, EditError> {
        self.write_math_source_with_metadata(
            source,
            cursor,
            merge_group,
            old_edit,
            MathSourceWriteOptions {
                before_math_position_override: None,
                reject_unmapped_metadata: true,
            },
        )
    }

    pub(super) fn write_math_source_with_metadata(
        &mut self,
        source: String,
        cursor: usize,
        merge_group: Option<EditGroup>,
        old_edit: std::ops::Range<usize>,
        options: MathSourceWriteOptions,
    ) -> Result<Effect, EditError> {
        let MathSourceWriteOptions {
            before_math_position_override,
            reject_unmapped_metadata,
        } = options;
        let edit = self
            .math_edit
            .as_ref()
            .map(MathEdit::command_snapshot)
            .ok_or(EditError::InvalidPosition)?;
        let current_source = edit.source().ok_or(EditError::InvalidPosition)?;
        let current_projection = edit.required_source_projection()?;
        if source == current_source {
            self.set_math_cursor(cursor);
            return Ok(Effect::default());
        }
        if old_edit.start > old_edit.end
            || old_edit.end > current_source.len()
            || !current_source.is_char_boundary(old_edit.start)
            || !current_source.is_char_boundary(old_edit.end)
        {
            return Err(EditError::InvalidPosition);
        }
        let mut source_projection_candidate = if matches!(edit.branch, MathEditBranch::Source) {
            let (kind, candidate_source) = match &edit.target {
                MathEditTarget::Inline { .. } => {
                    (caret_math::MathInputKind::Inline, source.clone())
                }
                MathEditTarget::Block { align: false, .. } => {
                    (caret_math::MathInputKind::Equation, source.clone())
                }
                MathEditTarget::Block { align: true, .. } => (
                    caret_math::MathInputKind::Align,
                    format!("{ALIGN_BEGIN}{source}{ALIGN_END}"),
                ),
            };
            match caret_math::parse_projection(&candidate_source, kind) {
                Ok(projection) => Some(projection.into_parts()),
                Err(caret_math::MathParseError::InputTooLong { .. }) => {
                    return Err(EditError::InvalidMath(
                        "math source exceeds the editing limit".to_owned(),
                    ));
                }
                Err(error)
                    if error.is_incomplete()
                        || (matches!(&error, caret_math::MathParseError::Parse { .. })
                            && caret_math::classify_input(&candidate_source, kind)
                                == caret_math::MathInputStatus::Incomplete) =>
                {
                    if reject_unmapped_metadata {
                        return Err(EditError::InvalidMath(error.to_string()));
                    }
                    self.math_edit
                        .as_mut()
                        .ok_or(EditError::InvalidPosition)?
                        .record_source_draft(source, cursor)?;
                    self.advance_history_merge_epoch();
                    return Ok(Effect {
                        selection_changed: true,
                        ..Effect::default()
                    });
                }
                Err(error) => return Err(EditError::InvalidMath(error.to_string())),
            }
        } else {
            None
        };
        let (transaction_old_edit, new_edit) = if edit.source_draft_is_pending() {
            changed_source_range(current_projection.edit_source(), &source)
                .ok_or(EditError::InvalidPosition)?
        } else {
            let inserted_len = source
                .len()
                .checked_sub(current_source.len() - old_edit.len())
                .ok_or(EditError::InvalidPosition)?;
            (
                old_edit.clone(),
                old_edit.start..old_edit.start + inserted_len,
            )
        };
        let before_math_position = if let Some(position) = before_math_position_override {
            position
        } else if let Some(draft) = &edit.source_draft {
            let mut baseline = edit.clone();
            baseline.source_draft = None;
            baseline.cursor = draft.base_cursor();
            MathEditPosition::from_edit(&baseline)
        } else {
            MathEditPosition::from_edit(&edit)
        };
        let mut next_branch = MathEditBranch::Source;
        let mut next_cursor = cursor;
        let mut next_edit = MathEdit::editing_source(
            edit.target.clone(),
            edit.source_projection.clone(),
            cursor,
            edit.revision.clone(),
        );
        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let mut position_map = None;
        let mut selection_changed = false;
        match &edit.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
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
                let (parsed, mut source_projection) = source_projection_candidate
                    .take()
                    .ok_or(EditError::InvalidPosition)?;
                let caret_math::ProjectedMath::Tree(parsed_tree) = parsed else {
                    return Err(EditError::InvalidMath("expected inline math".to_owned()));
                };
                let mut math_tree = parsed_tree.clone();
                let previous_projection = edit.required_source_projection()?;
                let previous_map = previous_projection
                    .tree()
                    .ok_or(EditError::InvalidPosition)?;
                let parsed_map = source_projection
                    .tree()
                    .cloned()
                    .ok_or(EditError::InvalidPosition)?;
                let metadata_preserved = crate::math_validation::bind_tree_sidecar(
                    &mut source_projection,
                    &parsed_tree,
                    &math_tree,
                    Some(previous_map),
                ) && preserve_math_tree_metadata_for_edit(
                    &math.math_tree,
                    &mut math_tree,
                    &absolute_source_edit(previous_projection, &transaction_old_edit)
                        .ok_or(EditError::InvalidPosition)?,
                    &absolute_source_edit(&source_projection, &new_edit)
                        .ok_or(EditError::InvalidPosition)?,
                    previous_map,
                    source_projection.tree().ok_or(EditError::InvalidPosition)?,
                );
                if !metadata_preserved {
                    if matches!(edit.branch, MathEditBranch::Source) {
                        self.math_edit
                            .as_mut()
                            .ok_or(EditError::InvalidPosition)?
                            .record_source_draft(source, cursor)?;
                        self.advance_history_merge_epoch();
                        return Ok(Effect {
                            selection_changed: true,
                            ..Effect::default()
                        });
                    }
                    return Err(EditError::InvalidMath(
                        "editing would discard unmapped math metadata".to_owned(),
                    ));
                }
                math_tree
                    .validate()
                    .map_err(|error| EditError::InvalidMath(error.to_string()))?;
                let mapped = parsed_map
                    .reconcile_for_tree(
                        &parsed_tree,
                        &math_tree,
                        Some(previous_map),
                        source_projection.source(),
                        None,
                    )
                    .ok_or_else(|| {
                        EditError::InvalidMath(
                            "edited formula cannot preserve source sidecar".to_owned(),
                        )
                    })?;
                if !source_projection.bind_tree_sidecar_after_validated_edit(&math_tree, mapped) {
                    return Err(EditError::InvalidMath(
                        "edited formula cannot be rebound to its source sidecar".to_owned(),
                    ));
                }
                let (source_anchor, source_cursor) = (next_cursor, next_cursor);
                math.math_tree = math_tree;
                let source_map = source_projection.tree().ok_or(EditError::InvalidPosition)?;
                let split = split_inline_math_root_with_map(
                    math,
                    source_map,
                    source_anchor,
                    source_cursor,
                    source_projection.source(),
                )?;
                next_edit.replace_source_projection(Some(Arc::new(source_projection)));
                if let Some(split) = split {
                    let inline_start = inline_math_slots(&paragraph)?
                        .into_iter()
                        .find(|(_, id)| id == math_id)
                        .map(|(start, _)| start)
                        .ok_or(EditError::InvalidPosition)?;
                    let split_focus = split.focus.clone();
                    let split_len = split.inserted_len;
                    let replacement = split.contents;
                    if !replace_inline_math_with_split(&mut paragraph.inlines, math_id, replacement)
                    {
                        return Err(EditError::InvalidPosition);
                    }
                    let map = TextMap::new(&paragraph)?;
                    let (after_selection, affinity, after_math) = match split_focus {
                        InlineRootFocus::Text {
                            anchor_offset,
                            cursor_offset,
                        } => {
                            let anchor_offset = inline_start
                                .checked_add(anchor_offset)
                                .ok_or(EditError::InvalidPosition)?;
                            let cursor_offset = inline_start
                                .checked_add(cursor_offset)
                                .ok_or(EditError::InvalidPosition)?;
                            let anchor_affinity = if anchor_offset == cursor_offset {
                                Affinity::Downstream
                            } else if anchor_offset < cursor_offset {
                                Affinity::Upstream
                            } else {
                                Affinity::Downstream
                            };
                            let cursor_affinity = if anchor_offset <= cursor_offset {
                                Affinity::Downstream
                            } else {
                                Affinity::Upstream
                            };
                            let anchor = Pos {
                                offset: anchor_offset,
                                affinity: anchor_affinity,
                                ..position.clone()
                            };
                            let cursor = Pos {
                                offset: cursor_offset,
                                affinity: cursor_affinity,
                                ..position.clone()
                            };
                            map.validate_pos(&anchor)?;
                            map.validate_pos(&cursor)?;
                            (
                                Selection {
                                    anchor,
                                    focus: cursor,
                                },
                                cursor_affinity,
                                None,
                            )
                        }
                        InlineRootFocus::Math {
                            math_id,
                            source,
                            cursor,
                            paragraph_offset,
                        } => {
                            let offset = inline_start
                                .checked_add(paragraph_offset)
                                .and_then(|start| start.checked_add(OBJECT_REPLACEMENT.len_utf8()))
                                .ok_or(EditError::InvalidPosition)?;
                            next_cursor = cursor;
                            next_branch = MathEditBranch::Source;
                            let target = MathEditTarget::Inline {
                                paragraph_id: paragraph_id.clone(),
                                math_id: math_id.clone(),
                            };
                            let projection = caret_math::parse_projection(
                                &source,
                                caret_math::MathInputKind::Inline,
                            )
                            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
                            let (content, mut source_projection) = projection.into_parts();
                            let caret_math::ProjectedMath::Tree(parsed_tree) = content else {
                                return Err(EditError::InvalidPosition);
                            };
                            let split_tree = find_inline_math(&paragraph.inlines, &math_id)
                                .map(|inline| &inline.math_tree)
                                .ok_or(EditError::InvalidPosition)?;
                            if !crate::math_validation::bind_tree_sidecar(
                                &mut source_projection,
                                &parsed_tree,
                                split_tree,
                                None,
                            ) {
                                return Err(EditError::InvalidMath(
                                    "split formula cannot be bound to its source map".to_owned(),
                                ));
                            }
                            next_edit = MathEdit::editing_source(
                                target.clone(),
                                Some(Arc::new(source_projection)),
                                cursor,
                                edit.revision.clone(),
                            );
                            let position = Pos {
                                offset,
                                affinity: Affinity::Upstream,
                                ..position.clone()
                            };
                            map.validate_pos(&position)?;
                            (
                                Selection {
                                    anchor: position.clone(),
                                    focus: position,
                                },
                                Affinity::Upstream,
                                Some(target),
                            )
                        }
                    };
                    let after_visual_affinity = match affinity {
                        Affinity::Upstream => VisualAffinity::Upstream,
                        Affinity::Downstream => VisualAffinity::Downstream,
                    };
                    selection_changed = after_selection != before_selection
                        || after_visual_affinity != before_visual_affinity;
                    position_map = Some(PositionMap::TextEdit {
                        paragraph_id: paragraph_id.clone(),
                        start: inline_start,
                        end: inline_start + OBJECT_REPLACEMENT.len_utf8(),
                        inserted_len: split_len,
                        snap_graphemes: true,
                        result_boundaries: map.grapheme_boundaries.clone(),
                    });
                    let before_states = history::paragraph_states_for_ids(
                        &self.document,
                        &self.paragraph_index,
                        [paragraph_id.clone()],
                    );
                    self.replace_paragraph(&position, paragraph)?;
                    self.text_map_cache.clear();
                    let after_math_position = after_math.map(|target| {
                        debug_assert_eq!(target, next_edit.target);
                        MathEditPosition::with_edit_state(
                            &next_edit,
                            next_cursor,
                            next_branch.clone(),
                        )
                    });
                    self.selection = after_selection.clone();
                    self.selection_visual_affinity = after_visual_affinity;
                    let after_states = history::paragraph_states_for_ids(
                        &self.document,
                        &self.paragraph_index,
                        [paragraph_id.clone()],
                    );
                    self.advance_history_merge_epoch();
                    let patch = DocumentPatch::Paragraphs {
                        before: before_states,
                        after: after_states,
                        merge_group: None,
                        merge_epoch: self.history_merge_epoch,
                    };
                    self.record_math_history_with_pending_style(
                        patch,
                        (before_selection, before_visual_affinity),
                        (after_selection, self.selection_visual_affinity),
                        Some(before_math_position.clone()),
                        after_math_position.clone(),
                        self.pending_inline_style.clone(),
                    );
                    self.set_math_edit(after_math_position.map(|_| next_edit.clone()));
                    self.sync_math_edit_revision();
                    return Ok(Effect {
                        document_changed: true,
                        selection_changed,
                        changed_paragraph_id: Some(paragraph_id.clone()),
                        position_map,
                        ..Effect::default()
                    });
                }
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
                    Some(before_math_position.clone()),
                    Some(MathEditPosition::with_edit_state(
                        &next_edit,
                        next_cursor,
                        next_branch.clone(),
                    )),
                );
            }
            MathEditTarget::Block { block_id, align } => {
                let original = find_block_path(&self.document.body, block_id).and_then(
                    |(parent_ids, index)| {
                        block_list_ref(&self.document.body, &parent_ids)
                            .ok()?
                            .get(index)
                            .cloned()
                    },
                );
                let (content, mut source_projection) = source_projection_candidate
                    .take()
                    .ok_or(EditError::InvalidPosition)?;
                let mut block = match (align, content) {
                    (false, caret_math::ProjectedMath::Tree(math_tree)) => {
                        Block::Equation(Equation {
                            id: block_id.clone(),
                            math_tree,
                            extra: Default::default(),
                        })
                    }
                    (true, caret_math::ProjectedMath::Align(parsed)) => Block::Align(Align {
                        id: block_id.clone(),
                        ..parsed
                    }),
                    _ => return Err(EditError::InvalidPosition),
                };
                match &block {
                    Block::Equation(equation) => equation
                        .math_tree
                        .validate()
                        .map_err(|error| EditError::InvalidMath(error.to_string()))?,
                    Block::Align(align) => align
                        .validate()
                        .map_err(|error| EditError::InvalidMath(error.to_string()))?,
                    _ => return Err(EditError::InvalidPosition),
                }
                let previous_projection = edit.required_source_projection()?;
                let rebound = match (&block, align) {
                    (Block::Equation(equation), false) => {
                        crate::math_validation::bind_tree_sidecar(
                            &mut source_projection,
                            &equation.math_tree,
                            &equation.math_tree,
                            previous_projection.tree(),
                        )
                    }
                    (Block::Align(align), true) => crate::math_validation::bind_align_sidecars(
                        &mut source_projection,
                        align,
                        align,
                        previous_projection.align(),
                    ),
                    _ => false,
                };
                if !rebound {
                    return Err(EditError::InvalidMath(
                        "edited formula cannot be rebound to its source map".to_owned(),
                    ));
                }
                if let Some(original) = original.as_ref() {
                    let metadata_preserved = preserve_display_math_metadata(
                        original,
                        &mut block,
                        &transaction_old_edit,
                        &new_edit,
                        edit.required_source_projection()?,
                        &source_projection,
                    );
                    if !metadata_preserved {
                        if reject_unmapped_metadata {
                            return Err(EditError::InvalidMath(
                                "editing would discard unmapped math metadata".to_owned(),
                            ));
                        }
                        if matches!(edit.branch, MathEditBranch::Source) {
                            self.math_edit
                                .as_mut()
                                .ok_or(EditError::InvalidPosition)?
                                .record_source_draft(source, cursor)?;
                            self.advance_history_merge_epoch();
                            return Ok(Effect {
                                selection_changed: true,
                                ..Effect::default()
                            });
                        }
                        return Err(EditError::InvalidMath(
                            "editing would discard unmapped math metadata".to_owned(),
                        ));
                    }
                }
                match &block {
                    Block::Equation(equation) => equation
                        .math_tree
                        .validate()
                        .map_err(|error| EditError::InvalidMath(error.to_string()))?,
                    Block::Align(align) => align
                        .validate()
                        .map_err(|error| EditError::InvalidMath(error.to_string()))?,
                    _ => return Err(EditError::InvalidPosition),
                }
                let rebound = match (&block, align) {
                    (Block::Equation(equation), false) => {
                        crate::math_validation::bind_tree_sidecar(
                            &mut source_projection,
                            &equation.math_tree,
                            &equation.math_tree,
                            previous_projection.tree(),
                        )
                    }
                    (Block::Align(align), true) => crate::math_validation::bind_align_sidecars(
                        &mut source_projection,
                        align,
                        align,
                        previous_projection.align(),
                    ),
                    _ => false,
                };
                if !rebound {
                    return Err(EditError::InvalidMath(
                        "edited formula cannot be rebound to its source map".to_owned(),
                    ));
                }
                next_edit.replace_source_projection(Some(Arc::new(source_projection)));
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
                    Some(before_math_position),
                    Some(MathEditPosition::with_edit_state(
                        &next_edit,
                        next_cursor,
                        next_branch.clone(),
                    )),
                );
            }
        }
        next_edit.cursor = next_cursor;
        next_edit.branch = next_branch;
        self.set_math_edit(Some(next_edit));
        self.sync_math_edit_revision();
        let changed_paragraph_id = match &edit.target {
            MathEditTarget::Inline { paragraph_id, .. } => Some(paragraph_id.clone()),
            MathEditTarget::Block { .. } => None,
        };
        Ok(Effect {
            document_changed: true,
            selection_changed,
            changed_paragraph_id,
            position_map,
            ..Effect::default()
        })
    }

    pub(super) fn empty_construct_range(&self, edit: &MathEdit) -> Option<Range<usize>> {
        fn containing_flow(
            flow: &MathFlow,
            source_map: &caret_math::MathFlowSourceMap,
            cursor: usize,
        ) -> Option<Range<usize>> {
            flow.content
                .iter()
                .zip(source_map.content())
                .find_map(|(content, map)| containing_content(content, map, cursor))
        }

        fn containing_content(
            content: &MathContent,
            source_map: &caret_math::MathContentSourceMap,
            cursor: usize,
        ) -> Option<Range<usize>> {
            match (content, source_map) {
                (
                    MathContent::Math(node),
                    caret_math::MathContentSourceMap::Math { operands, .. },
                ) => {
                    let has_empty_argument =
                        node.children.iter().zip(operands).any(|(flow, flow_map)| {
                            flow.content
                                .iter()
                                .zip(flow_map.content())
                                .any(|(content, map)| {
                                    matches!(
                                        (content, map),
                                        (
                                            MathContent::Math(child),
                                            caret_math::MathContentSourceMap::Math { .. }
                                        ) if child.kind == caret_doc::MathNodeKind::Placeholder
                                            && map.range().is_some_and(|range| {
                                                range.start + 1 == cursor
                                                    && range.end == cursor + 1
                                            })
                                    )
                                })
                        });
                    let all_arguments_empty = !node.children.is_empty()
                        && node.children.iter().all(flow_contains_no_content);
                    // 删除整个构造前确认没有其他有效操作数，否则退格只能按普通源码停靠点尝试。
                    if has_empty_argument
                        && all_arguments_empty
                        && matches!(
                            node.kind,
                            caret_doc::MathNodeKind::Fraction
                                | caret_doc::MathNodeKind::Radical
                                | caret_doc::MathNodeKind::Superscript
                                | caret_doc::MathNodeKind::Subscript
                                | caret_doc::MathNodeKind::SubSup
                        )
                    {
                        return source_map.range();
                    }
                    node.children
                        .iter()
                        .zip(operands)
                        .find_map(|(flow, map)| containing_flow(flow, map, cursor))
                }
                (
                    MathContent::Environment(environment),
                    caret_math::MathContentSourceMap::Environment { children, .. },
                ) => environment
                    .children
                    .iter()
                    .zip(children)
                    .find_map(|(child, map)| containing_content(child, map, cursor)),
                _ => None,
            }
        }

        fn flow_contains_no_content(flow: &MathFlow) -> bool {
            flow.content.iter().all(content_contains_no_content)
        }
        fn content_contains_no_content(content: &MathContent) -> bool {
            match content {
                MathContent::Math(node) => {
                    node.kind == caret_doc::MathNodeKind::Placeholder
                        || (node.kind == caret_doc::MathNodeKind::Group
                            && node.text.as_deref().is_none_or(str::is_empty)
                            && node.children.iter().all(flow_contains_no_content))
                }
                MathContent::Text(text) => text.runs.iter().all(|run| run.text.is_empty()),
                MathContent::Environment(environment) => {
                    environment.children.iter().all(content_contains_no_content)
                }
                MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
            }
        }

        let projection = edit.source_projection.as_deref()?;
        let (tree, source_map) = match &edit.target {
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
                let tree = find_inline_math(&self.paragraph_for(&position).ok()?.inlines, math_id)
                    .map(|math| &math.math_tree)?;
                (tree, projection.tree()?)
            }
            MathEditTarget::Block { block_id, align } => {
                let block = find_block_path(&self.document.body, block_id).and_then(
                    |(parents, index)| {
                        block_list_ref(&self.document.body, &parents)
                            .ok()?
                            .get(index)
                    },
                )?;
                if *align {
                    let Block::Align(align) = block else {
                        return None;
                    };
                    let (row, cell, _) = edit.align_cell_at_cursor()?;
                    let tree = align.rows.get(row)?.cells.get(cell)?;
                    let source_map = projection.align()?.cell_tree(row, cell)?;
                    (tree, source_map)
                } else {
                    let Block::Equation(equation) = block else {
                        return None;
                    };
                    (&equation.math_tree, projection.tree()?)
                }
            }
        };
        let cursor = projection.edit_range().start.checked_add(edit.cursor)?;
        let range = containing_flow(&tree.root, source_map.root(), cursor)?;
        Some(
            range.start.checked_sub(projection.edit_range().start)?
                ..range.end.checked_sub(projection.edit_range().start)?,
        )
    }

    pub(super) fn record_math_history(
        &mut self,
        patch: DocumentPatch,
        before_selection: (Selection, VisualAffinity),
        after_selection: (Selection, VisualAffinity),
        before_math_edit: Option<MathEditPosition>,
        after_math_edit: Option<MathEditPosition>,
    ) {
        let before_pending_style = self.pending_inline_style.clone();
        self.record_math_history_with_pending_style(
            patch,
            before_selection,
            after_selection,
            before_math_edit,
            after_math_edit,
            before_pending_style,
        );
    }

    pub(super) fn record_math_history_with_pending_style(
        &mut self,
        patch: DocumentPatch,
        before_selection: (Selection, VisualAffinity),
        after_selection: (Selection, VisualAffinity),
        before_math_edit: Option<MathEditPosition>,
        after_math_edit: Option<MathEditPosition>,
        before_pending_style: super::super::formatting::PendingInlineStyle,
    ) {
        let before_state = SessionHistoryState::new(
            before_selection.0,
            before_selection.1,
            before_pending_style,
            self.history_state_id,
        )
        .with_math_edit(before_math_edit);
        let after_state = SessionHistoryState::new(
            after_selection.0,
            after_selection.1,
            self.pending_inline_style.clone(),
            self.history_state_id,
        )
        .with_math_edit(after_math_edit);
        self.commit_document_edit(
            patch,
            before_state,
            DocumentEditSnapshot::Explicit(Box::new(after_state)),
        );
    }
}
