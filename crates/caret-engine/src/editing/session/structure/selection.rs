//! 删除结构选区中的块环境子树并管理其恢复位置。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;
use crate::editing::paragraph_ops;

impl Session {
    /// 删除结构选区中的整个块环境子树，并将插入点移至相邻段落。
    pub(in crate::editing::session) fn delete_selected_block_environment(
        &mut self,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        let Some(environment_id) = self.selected_block_environment.clone() else {
            return Ok(Effect::default());
        };
        let Some(entry) = self.structure_index.get(&environment_id) else {
            return Err(EditError::InvalidPosition);
        };
        if !entry.kind.is_block_environment() {
            return Err(EditError::InvalidPosition);
        }

        let previous_id = entry
            .range
            .paragraph_ordinals
            .start
            .checked_sub(1)
            .and_then(|ordinal| self.structure_index.paragraph_id_at(ordinal))
            .cloned();
        let next_id = self
            .structure_index
            .paragraph_id_at(entry.range.paragraph_ordinals.end)
            .cloned();
        let target_position = previous_id
            .as_ref()
            .and_then(|paragraph_id| self.position_at_paragraph(paragraph_id, true))
            .or_else(|| {
                next_id
                    .as_ref()
                    .and_then(|paragraph_id| self.position_at_paragraph(paragraph_id, false))
            });

        let Some((parent_environment_ids, sibling_index)) =
            environment_parent_location(&self.document.body, &environment_id)
        else {
            return Err(EditError::InvalidPosition);
        };
        let siblings = block_list_by_environment_ids(&self.document.body, &parent_environment_ids)
            .ok_or(EditError::InvalidPosition)?;
        let Some(Block::Environment(environment)) = siblings.get(sibling_index) else {
            return Err(EditError::InvalidPosition);
        };
        let environment = environment.clone();
        let environment_estimated_bytes = history::environment_estimated_bytes(&environment);
        let fallback_paragraph = target_position
            .is_none()
            .then(|| Box::new(Paragraph::empty(NodeId::generate())));
        let position = target_position
            .or_else(|| {
                fallback_paragraph.as_ref().map(|paragraph| Pos {
                    story: StoryId::Body,
                    paragraph_id: paragraph.id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                })
            })
            .ok_or(EditError::InvalidPosition)?;

        {
            let siblings =
                block_list_by_environment_ids_mut(&mut self.document.body, &parent_environment_ids)
                    .ok_or(EditError::InvalidPosition)?;
            siblings.remove(sibling_index);
            if let Some(paragraph) = &fallback_paragraph {
                siblings.insert(sibling_index, Block::Paragraph((**paragraph).clone()));
            }
        }
        if let Err(error) = self.document.validate() {
            let siblings =
                block_list_by_environment_ids_mut(&mut self.document.body, &parent_environment_ids)
                    .ok_or(EditError::InvalidPosition)?;
            if fallback_paragraph.is_some() {
                siblings.remove(sibling_index);
            }
            siblings.insert(sibling_index, Block::Environment(environment));
            return Err(EditError::InvalidDocument(error.to_string()));
        }

        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        self.selection = Selection {
            anchor: position.clone(),
            focus: position,
        };
        self.selection_visual_affinity = if previous_id.is_some() {
            VisualAffinity::Upstream
        } else {
            VisualAffinity::Downstream
        };
        self.selected_block_environment = None;
        self.selected_block_environments.clear();
        self.structure_selection = None;
        self.block_environment_exit = None;
        self.finalize_structure_edit(
            super::super::super::paragraph_ops::build_paragraph_index(&self.document.body),
            DocumentPatch::EnvironmentDeletion {
                parent_environment_ids,
                sibling_index,
                environment: Box::new(environment),
                environment_estimated_bytes,
                fallback_paragraph,
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

    fn position_at_paragraph(&self, paragraph_id: &NodeId, at_end: bool) -> Option<Pos> {
        let location = self.paragraph_index.get(paragraph_id)?;
        let paragraph = paragraph_at_location(&self.document.body, location)?;
        let offset = if at_end {
            TextMap::new(paragraph).ok()?.text.len()
        } else {
            0
        };
        Some(Pos {
            story: location.story.clone(),
            paragraph_id: paragraph_id.clone(),
            offset,
            affinity: if at_end {
                Affinity::Upstream
            } else {
                Affinity::Downstream
            },
        })
    }

    /// 删除结构选区覆盖的文字与块环境，并作为一个历史操作提交。
    pub(in crate::editing::session) fn delete_selected_structure_range(
        &mut self,
    ) -> Result<Option<Effect>, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(None);
        }
        if self.selected_block_environments.is_empty() {
            return Ok(None);
        }

        let selected_environments = self.selected_block_environments.clone();
        let mut paragraph_ids = Vec::new();
        let mut text_range = None;
        let mut range_start_ordinal = None;
        if let Some(selection) = self.structure_selection.as_ref() {
            let Some(anchor_ordinal) = self
                .structure_index
                .paragraph_ordinal(&selection.anchor.paragraph_id)
            else {
                return Err(EditError::InvalidPosition);
            };
            let Some(focus_ordinal) = self
                .structure_index
                .paragraph_ordinal(&selection.focus.paragraph_id)
            else {
                return Err(EditError::InvalidPosition);
            };
            let (start, start_ordinal, end, end_ordinal) =
                if (anchor_ordinal, selection.anchor.offset)
                    <= (focus_ordinal, selection.focus.offset)
                {
                    (
                        &selection.anchor,
                        anchor_ordinal,
                        &selection.focus,
                        focus_ordinal,
                    )
                } else {
                    (
                        &selection.focus,
                        focus_ordinal,
                        &selection.anchor,
                        anchor_ordinal,
                    )
                };
            range_start_ordinal = Some(start_ordinal);
            for ordinal in start_ordinal..=end_ordinal {
                let Some(id) = self.structure_index.paragraph_id_at(ordinal) else {
                    continue;
                };
                let covered_by_selected_environment = selected_environments.iter().any(|id| {
                    self.structure_index
                        .get(id)
                        .is_some_and(|entry| entry.range.paragraph_ordinals.contains(&ordinal))
                });
                if !covered_by_selected_environment {
                    paragraph_ids.push(id.clone());
                }
            }
            text_range = Some((
                start.paragraph_id.clone(),
                start.offset,
                end.paragraph_id.clone(),
                end.offset,
            ));
        } else if let Some(ordinal) = self
            .structure_index
            .paragraph_ordinal(&self.selection.focus.paragraph_id)
        {
            range_start_ordinal = Some(ordinal);
        }

        let before_paragraphs =
            history::paragraph_states_for_ids(&self.document, &self.paragraph_index, paragraph_ids);
        let mut planned_document = self.document.clone();
        let mut after_paragraphs = Vec::with_capacity(before_paragraphs.len());
        for state in &before_paragraphs {
            let Some((start_id, start_offset, end_id, end_offset)) = &text_range else {
                after_paragraphs.push(state.clone());
                continue;
            };
            let paragraph = self
                .paragraph_for(&Pos {
                    story: self
                        .paragraph_index
                        .get(&state.id)
                        .ok_or(EditError::InvalidPosition)?
                        .story
                        .clone(),
                    paragraph_id: state.id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                })?
                .clone();
            let map = TextMap::new(&paragraph)?;
            let from = if state.id == *start_id {
                *start_offset
            } else {
                0
            };
            let to = if state.id == *end_id {
                *end_offset
            } else {
                map.text.len()
            };
            if from < to {
                let location = self
                    .paragraph_index
                    .get(&state.id)
                    .cloned()
                    .ok_or(EditError::InvalidPosition)?;
                let paragraph =
                    paragraph_ops::paragraph_at_location_mut(&mut planned_document.body, &location)
                        .ok_or(EditError::InvalidPosition)?;
                map.delete_range(paragraph, from, to)?;
            }
            after_paragraphs.push(
                history::ParagraphState::capture(
                    &planned_document,
                    &self.paragraph_index,
                    &state.id,
                )
                .ok_or(EditError::InvalidPosition)?,
            );
        }

        let mut environment_deletions = Vec::with_capacity(selected_environments.len());
        for environment_id in &selected_environments {
            let Some(entry) = self.structure_index.get(environment_id) else {
                return Err(EditError::InvalidPosition);
            };
            let Some((parent_environment_ids, sibling_index)) =
                environment_parent_location(&self.document.body, environment_id)
            else {
                return Err(EditError::InvalidPosition);
            };
            let siblings =
                block_list_by_environment_ids(&self.document.body, &parent_environment_ids)
                    .ok_or(EditError::InvalidPosition)?;
            let Some(Block::Environment(environment)) = siblings.get(sibling_index) else {
                return Err(EditError::InvalidPosition);
            };
            if !entry.kind.is_block_environment() {
                return Err(EditError::InvalidPosition);
            }
            environment_deletions.push((
                parent_environment_ids,
                sibling_index,
                environment.clone(),
            ));
        }
        environment_deletions
            .sort_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)));

        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let mut patches = Vec::with_capacity(1 + environment_deletions.len());
        if before_paragraphs != after_paragraphs {
            patches.push(DocumentPatch::Paragraphs {
                before: before_paragraphs,
                after: after_paragraphs,
                merge_group: None,
                merge_epoch: self.history_merge_epoch,
            });
        }
        for (parent_environment_ids, sibling_index, environment) in environment_deletions {
            patches.push(DocumentPatch::EnvironmentDeletion {
                parent_environment_ids,
                sibling_index,
                environment_estimated_bytes: history::environment_estimated_bytes(&environment),
                environment: Box::new(environment),
                fallback_paragraph: None,
            });
        }

        for patch in &patches {
            patch.redo(&mut planned_document);
        }

        if paragraph_ops::build_paragraph_index(&planned_document.body).is_empty() {
            let paragraph = Box::new(Paragraph::empty(NodeId::generate()));
            let sibling_index = planned_document.body.len();
            planned_document
                .body
                .push(Block::Paragraph((*paragraph).clone()));
            patches.push(DocumentPatch::ParagraphInsertion {
                parent_environment_ids: Vec::new(),
                sibling_index,
                paragraph,
            });
        }

        planned_document
            .validate()
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
        self.document = planned_document;

        let updated_index = paragraph_ops::build_paragraph_index(&self.document.body);
        let range_start = text_range
            .as_ref()
            .map(|(paragraph_id, offset, _, _)| (paragraph_id, *offset));
        let target = range_start
            .and_then(|(paragraph_id, offset)| {
                updated_index.get(paragraph_id).map(|location| Pos {
                    story: location.story.clone(),
                    paragraph_id: paragraph_id.clone(),
                    offset,
                    affinity: Affinity::Downstream,
                })
            })
            .or_else(|| {
                let start_ordinal = range_start_ordinal.unwrap_or(0);
                (start_ordinal..)
                    .map_while(|ordinal| self.structure_index.paragraph_id_at(ordinal))
                    .find_map(|id| updated_index.get(id).map(|location| (id, location)))
                    .map(|(id, location)| Pos {
                        story: location.story.clone(),
                        paragraph_id: id.clone(),
                        offset: 0,
                        affinity: Affinity::Downstream,
                    })
                    .or_else(|| {
                        (0..start_ordinal).rev().find_map(|ordinal| {
                            let id = self.structure_index.paragraph_id_at(ordinal)?;
                            updated_index.get(id).map(|location| Pos {
                                story: location.story.clone(),
                                paragraph_id: id.clone(),
                                offset: 0,
                                affinity: Affinity::Downstream,
                            })
                        })
                    })
            })
            .or_else(|| {
                updated_index.iter().next().map(|(id, location)| Pos {
                    story: location.story.clone(),
                    paragraph_id: id.clone(),
                    offset: 0,
                    affinity: Affinity::Downstream,
                })
            })
            .ok_or(EditError::InvalidPosition)?;
        self.selection = Selection {
            anchor: target.clone(),
            focus: target,
        };
        self.selected_block_environment = None;
        self.selected_block_environments.clear();
        self.structure_selection = None;
        self.block_environment_exit = None;
        self.finalize_structure_edit(
            updated_index,
            DocumentPatch::Compound(patches),
            before_selection,
            before_visual_affinity,
            before_state_id,
        );
        Ok(Some(Effect {
            document_changed: true,
            selection_changed: true,
            ..Effect::default()
        }))
    }
}
