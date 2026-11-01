//! 校验选区位置并维护逻辑亲和性与视觉亲和性。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::paragraph_ops::{compare_positions, paragraph_at_location};
use super::super::text_map::TextMap;
use super::super::{Affinity, Command, Direction, EditError, Effect, Pos, Selection, StoryId};
use super::Session;
use caret_doc::NodeId;
use caret_layout::VisualAffinity;

impl Session {
    /// 按文档顺序提取当前正文选区的纯文本，段落之间以换行连接。
    pub fn selected_text(&self) -> Result<String, EditError> {
        let structural_range = self
            .structure_selection
            .as_ref()
            .map(|selection| (&selection.anchor, &selection.focus));
        let (start, end) = match structural_range {
            Some((anchor, focus)) => (anchor.clone(), focus.clone()),
            None => self.edit_range()?,
        };
        if start == end
            && structural_range.is_some()
            && !self.selected_block_environments.is_empty()
        {
            let mut paragraphs: Vec<_> = self.paragraph_index.iter().collect();
            paragraphs.sort_by(|(_, left), (_, right)| {
                left.block_path
                    .cmp(&right.block_path)
                    .then_with(|| left.is_title.cmp(&right.is_title).reverse())
            });
            let selected_ranges: Vec<_> = self
                .selected_block_environments
                .iter()
                .filter_map(|id| self.structure_index.get(id))
                .map(|entry| entry.range.paragraph_ordinals.clone())
                .collect();
            let mut text = String::new();
            let mut first = true;
            for (ordinal, (paragraph_id, location)) in paragraphs.iter().enumerate() {
                if !selected_ranges.iter().any(|range| range.contains(&ordinal)) {
                    continue;
                }
                let paragraph = paragraph_at_location(&self.document.body, location)
                    .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
                if !first {
                    text.push('\n');
                }
                text.push_str(&TextMap::new(paragraph)?.text);
                first = false;
            }
            return Ok(text);
        }
        if structural_range.is_none() && start.story != end.story {
            return Err(EditError::CrossParagraphSelection);
        }
        if start == end {
            return Ok(String::new());
        }
        let start_map = TextMap::new(self.paragraph_for(&start)?)?;
        start_map.validate_pos(&start)?;
        if start.paragraph_id == end.paragraph_id {
            start_map.validate_pos(&end)?;
            let (from, to) = if start.offset <= end.offset {
                (start.offset, end.offset)
            } else {
                (end.offset, start.offset)
            };
            return start_map
                .text
                .get(from..to)
                .map(str::to_owned)
                .ok_or(EditError::InvalidPosition);
        }
        if structural_range.is_none()
            && (start.story != StoryId::Body || end.story != StoryId::Body)
        {
            return Err(EditError::CrossParagraphSelection);
        }
        let end_map = TextMap::new(self.paragraph_for(&end)?)?;
        end_map.validate_pos(&end)?;

        let mut paragraphs: Vec<_> = self
            .paragraph_index
            .iter()
            .filter(|(_, location)| structural_range.is_some() || location.story == StoryId::Body)
            .collect();
        paragraphs.sort_by(|(_, left), (_, right)| {
            left.block_path
                .cmp(&right.block_path)
                .then_with(|| left.is_title.cmp(&right.is_title).reverse())
        });
        let start_index = paragraphs
            .iter()
            .position(|(id, _)| *id == &start.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(start.paragraph_id.to_string()))?;
        let end_index = paragraphs
            .iter()
            .position(|(id, _)| *id == &end.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(end.paragraph_id.to_string()))?;
        let (start_index, start_offset, end_index, end_offset) = if structural_range.is_some() {
            if (start_index, start.offset) <= (end_index, end.offset) {
                (start_index, start.offset, end_index, end.offset)
            } else {
                (end_index, end.offset, start_index, start.offset)
            }
        } else if compare_positions(&self.paragraph_index, &start, &end)?.is_gt() {
            (end_index, end.offset, start_index, start.offset)
        } else {
            (start_index, start.offset, end_index, end.offset)
        };
        if start_index == end_index && start_offset == end_offset {
            return Ok(String::new());
        }

        let selected_environment_ranges: Vec<_> = self
            .selected_block_environments
            .iter()
            .filter_map(|id| self.structure_index.get(id))
            .map(|entry| entry.range.paragraph_ordinals.clone())
            .collect();
        let (included_start_index, included_end_index) = selected_environment_ranges
            .iter()
            .filter(|range| range.start <= end_index && range.end > start_index)
            .fold((start_index, end_index), |(start, end), range| {
                (start.min(range.start), end.max(range.end.saturating_sub(1)))
            });

        let mut text = String::new();
        for (index, (paragraph_id, location)) in paragraphs
            .iter()
            .enumerate()
            .take(included_end_index + 1)
            .skip(included_start_index)
        {
            let paragraph = paragraph_at_location(&self.document.body, location)
                .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
            let paragraph_text = TextMap::new(paragraph)?.text;
            let selected_as_whole = selected_environment_ranges
                .iter()
                .any(|range| range.contains(&index));
            let fragment_start = if selected_as_whole {
                0
            } else if index == start_index {
                start_offset
            } else {
                0
            };
            let fragment_end = if selected_as_whole {
                paragraph_text.len()
            } else if index == end_index {
                end_offset
            } else {
                paragraph_text.len()
            };
            let fragment = paragraph_text
                .get(fragment_start..fragment_end)
                .ok_or(EditError::InvalidPosition)?;
            if index != included_start_index {
                text.push('\n');
            }
            text.push_str(fragment);
        }
        Ok(text)
    }

    /// 将布局命中得到的视觉侧与同一 Rust 选区一起提交。
    pub fn set_selection_with_visual_affinity(
        &mut self,
        selection: Option<Selection>,
        visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        let previous_visual_affinity = self.selection_visual_affinity;
        let mut effect = self.run(Command::SetSelection(selection))?;
        effect.selection_changed |= previous_visual_affinity != visual_affinity;
        self.selection_visual_affinity = visual_affinity;
        Ok(effect)
    }

    /// 统一提交鼠标产生的文本范围，并在跨越块环境时提升为结构选区。
    pub fn set_pointer_selection_range(
        &mut self,
        anchor: Pos,
        focus: Pos,
        visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        if self.range_requires_block_environment_selection(&anchor, &focus) {
            self.select_block_environments_from_range(anchor, focus, visual_affinity)
        } else {
            self.set_selection_with_visual_affinity(
                Some(Selection { anchor, focus }),
                visual_affinity,
            )
        }
    }

    fn range_requires_block_environment_selection(&self, anchor: &Pos, focus: &Pos) -> bool {
        if anchor == focus {
            return false;
        }
        let crossed_environment_range = !self.block_environments_in_range(anchor, focus).is_empty();
        if !crossed_environment_range {
            return false;
        }
        let anchor_environment = self.block_environment_at(anchor);
        let focus_environment = self.block_environment_at(focus);
        anchor.story != focus.story
            || anchor_environment != focus_environment
            || anchor_environment.is_none()
    }

    fn block_environment_at(&self, position: &Pos) -> Option<NodeId> {
        let ordinal = self
            .structure_index
            .paragraph_ordinal(&position.paragraph_id)?;
        self.structure_index
            .at(&caret_structure::StructurePosition {
                paragraph_id: position.paragraph_id.clone(),
                offset_utf8: position.offset,
            })
            .into_iter()
            .rev()
            .find(|entry| {
                entry.kind.is_block_environment()
                    && entry.range.paragraph_ordinals.contains(&ordinal)
            })
            .map(|entry| entry.id.clone())
    }

    /// 提交键盘导航产生的位置，并按逻辑移动方向更新环境边界亲和性。
    pub fn set_navigation_selection(
        &mut self,
        mut selection: Selection,
        visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        let was_collapsed = selection.anchor == selection.focus;
        if self.range_requires_block_environment_selection(&selection.anchor, &selection.focus) {
            return self.select_block_environments_from_range(
                selection.anchor,
                selection.focus,
                visual_affinity,
            );
        }
        let ordering = compare_positions(
            &self.paragraph_index,
            &self.selection.focus,
            &selection.focus,
        )?;
        let direction = match ordering {
            std::cmp::Ordering::Less => Some(Direction::Forward),
            std::cmp::Ordering::Greater => Some(Direction::Backward),
            std::cmp::Ordering::Equal => {
                match (self.selection.focus.affinity, selection.focus.affinity) {
                    (Affinity::Upstream, Affinity::Downstream) => Some(Direction::Forward),
                    (Affinity::Downstream, Affinity::Upstream) => Some(Direction::Backward),
                    _ => None,
                }
            }
        };
        if let Some(direction) = direction {
            let at_environment_boundary = self
                .text_map_for(&selection.focus)?
                .has_environment_boundary(selection.focus.offset);
            if at_environment_boundary {
                selection.focus.affinity = match direction {
                    Direction::Backward => Affinity::Downstream,
                    Direction::Forward => Affinity::Upstream,
                };
            }
        }
        if was_collapsed {
            selection.anchor = selection.focus.clone();
        }
        self.block_environment_exit = None;
        self.selected_block_environment = None;
        self.selected_block_environments.clear();
        self.structure_selection = None;
        self.set_selection_with_visual_affinity(Some(selection), visual_affinity)
    }

    /// 将跨越环境边界的指针范围提升为整块结构选区，并把焦点落在拖动方向的边界。
    pub fn select_block_environment_from_range(
        &mut self,
        environment_id: NodeId,
        anchor: Pos,
        focus: Pos,
        visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        if !self
            .structure_index
            .structures()
            .iter()
            .any(|entry| entry.id == environment_id && entry.kind.is_block_environment())
        {
            return Err(EditError::InvalidPosition);
        }
        self.select_block_environments_from_range(anchor, focus, visual_affinity)
    }

    /// 返回端点范围覆盖的最外层块环境，按文档顺序排列。
    pub fn block_environments_in_range(&self, anchor: &Pos, focus: &Pos) -> Vec<NodeId> {
        let Some(anchor_ordinal) = self.structure_index.paragraph_ordinal(&anchor.paragraph_id)
        else {
            return Vec::new();
        };
        let Some(focus_ordinal) = self.structure_index.paragraph_ordinal(&focus.paragraph_id)
        else {
            return Vec::new();
        };
        let start = anchor_ordinal.min(focus_ordinal);
        let end = anchor_ordinal.max(focus_ordinal).saturating_add(1);
        let intersecting = self
            .structure_index
            .structures()
            .iter()
            .filter(|entry| {
                entry.kind.is_block_environment()
                    && entry.range.paragraph_ordinals.start < end
                    && start < entry.range.paragraph_ordinals.end
            })
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        intersecting
            .iter()
            .filter(|candidate_id| {
                !intersecting.iter().any(|other_id| {
                    other_id != *candidate_id
                        && self.structure_index.is_ancestor(other_id, candidate_id)
                })
            })
            .cloned()
            .collect()
    }

    /// 保存跨段、跨 Story 的结构范围，并选中范围覆盖的全部块环境。
    pub fn select_block_environments_from_range(
        &mut self,
        anchor: Pos,
        focus: Pos,
        visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        // 结构选区与公式内编辑互斥。
        self.math_edit = None;
        if self.composition.is_some()
            || self
                .command_line
                .as_ref()
                .is_some_and(|line| line.preedit.is_empty())
        {
            return Err(EditError::InvalidPosition);
        }
        self.text_map_for(&anchor)?.validate_pos(&anchor)?;
        self.text_map_for(&focus)?.validate_pos(&focus)?;
        let selected = self.block_environments_in_range(&anchor, &focus);
        if selected.is_empty() {
            return Err(EditError::InvalidPosition);
        }
        let cancels_command_line_preedit = self
            .command_line
            .as_ref()
            .is_some_and(|line| !line.preedit.is_empty());
        let changed = self.selected_block_environments != selected
            || self
                .structure_selection
                .as_ref()
                .is_none_or(|selection| selection.anchor != anchor || selection.focus != focus)
            || self.selection.focus != focus
            || self.selection_visual_affinity != visual_affinity;
        self.selection = Selection {
            anchor: focus.clone(),
            focus: focus.clone(),
        };
        self.selection_visual_affinity = visual_affinity;
        self.selected_block_environment = selected.first().cloned();
        self.selected_block_environments = selected;
        self.structure_selection = Some(super::StructureSelection { anchor, focus });
        self.block_environment_exit = None;
        if cancels_command_line_preedit {
            self.command_line = None;
        }
        Ok(Effect {
            selection_changed: changed,
            composition_changed: cancels_command_line_preedit,
            ..Effect::default()
        })
    }

    /// 将输入命令使用的选区规整为文档顺序，并拒绝不同文本 Story 的范围。
    pub(super) fn edit_range(&self) -> Result<(Pos, Pos), EditError> {
        let selection = &self.selection;
        if selection.anchor.story != selection.focus.story {
            return Err(EditError::CrossParagraphSelection);
        }
        Ok((selection.anchor.clone(), selection.focus.clone()))
    }

    /// 校验并安装 Rust Session 中的逻辑选区。
    pub(super) fn set_selection(
        &mut self,
        selection: Option<Selection>,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let next = selection.unwrap_or_else(|| Selection {
            anchor: self.selection.focus.clone(),
            focus: self.selection.focus.clone(),
        });
        if next.anchor.story != next.focus.story {
            return Err(EditError::CrossParagraphSelection);
        }
        self.text_map_for(&next.anchor)?
            .validate_pos(&next.anchor)?;
        self.text_map_for(&next.focus)?.validate_pos(&next.focus)?;
        let changed = self.selection != next;
        self.block_environment_exit = None;
        self.selected_block_environment = None;
        self.selected_block_environments.clear();
        self.structure_selection = None;
        self.selection = next;
        Ok(Effect {
            selection_changed: changed,
            ..Effect::default()
        })
    }
}
