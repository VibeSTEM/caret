//! 校验选区位置并维护逻辑亲和性与视觉亲和性。

use super::super::paragraph_ops::{compare_positions, paragraph_at_location};
use super::super::text_map::TextMap;
use super::super::{Affinity, Command, Direction, EditError, Effect, Pos, Selection, StoryId};
use super::Session;
use caret_layout::VisualAffinity;

impl Session {
    /// 按文档顺序提取当前正文选区的纯文本，段落之间以换行连接。
    pub fn selected_text(&self) -> Result<String, EditError> {
        let (start, end) = self.edit_range()?;
        if start.story != StoryId::Body || end.story != StoryId::Body {
            return Err(EditError::CrossParagraphSelection);
        }
        if start == end {
            return Ok(String::new());
        }
        let start_map = TextMap::new(self.paragraph_for(&start)?)?;
        start_map.validate_pos(&start)?;
        if start.paragraph_id != end.paragraph_id {
            let end_map = TextMap::new(self.paragraph_for(&end)?)?;
            end_map.validate_pos(&end)?;
        }

        let mut paragraphs: Vec<_> = self
            .paragraph_index
            .iter()
            .filter(|(_, location)| location.story == StoryId::Body)
            .collect();
        paragraphs.sort_by(|(_, left), (_, right)| left.block_path.cmp(&right.block_path));
        let start_index = paragraphs
            .iter()
            .position(|(id, _)| *id == &start.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(start.paragraph_id.to_string()))?;
        let end_index = paragraphs
            .iter()
            .position(|(id, _)| *id == &end.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(end.paragraph_id.to_string()))?;
        let (start_index, start_offset, end_index, end_offset) =
            if compare_positions(&self.paragraph_index, &start, &end)?.is_gt() {
                (end_index, end.offset, start_index, start.offset)
            } else {
                (start_index, start.offset, end_index, end.offset)
            };
        if start_index == end_index && start_offset == end_offset {
            return Ok(String::new());
        }

        let mut text = String::new();
        for (index, (paragraph_id, location)) in paragraphs
            .iter()
            .enumerate()
            .take(end_index + 1)
            .skip(start_index)
        {
            let paragraph = paragraph_at_location(&self.document.body, location)
                .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
            let paragraph_text = TextMap::new(paragraph)?.text;
            let fragment_start = if index == start_index {
                start_offset
            } else {
                0
            };
            let fragment_end = if index == end_index {
                end_offset
            } else {
                paragraph_text.len()
            };
            let fragment = paragraph_text
                .get(fragment_start..fragment_end)
                .ok_or(EditError::InvalidPosition)?;
            if index != start_index {
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

    /// 提交键盘导航产生的位置，并按逻辑移动方向更新环境边界亲和性。
    pub fn set_navigation_selection(
        &mut self,
        mut selection: Selection,
        visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        let was_collapsed = selection.anchor == selection.focus;
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
                .environment_spans
                .iter()
                .any(|(start, end)| {
                    *start == selection.focus.offset || *end == selection.focus.offset
                });
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
        self.set_selection_with_visual_affinity(Some(selection), visual_affinity)
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
}
