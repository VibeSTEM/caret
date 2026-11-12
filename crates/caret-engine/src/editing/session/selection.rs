//! 校验选区位置、维护逻辑/视觉亲和性，并折叠结构范围到文档端点。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::paragraph_ops::{compare_positions, paragraph_at_location};
use super::super::text_map::{OBJECT_REPLACEMENT, TextMap};
use super::super::{
    Affinity, Command, Direction, DocumentRangeEndpoint, EditError, Effect, NavigationIntent, Pos,
    Selection, StoryId, StructureSelection,
};
use super::Session;
use super::document_position::{visual_affinity_for, visual_affinity_for_block_edge};
use caret_doc::NodeId;
use caret_layout::VisualAffinity;

impl Session {
    /// 按文档顺序提取当前正文选区的纯文本，段落之间以换行连接。
    ///
    /// 结构选区按同一文档范围遍历物理块序列：覆盖环境的全部段落完整复制，
    /// Equation/Align 与行内公式一致输出 U+FFFC，范围两端的普通文字只取切口。
    pub fn selected_text(&self) -> Result<String, EditError> {
        if self.has_structure_selection() {
            return self.structure_selection_text();
        }
        let (start, end) = self.edit_range()?;
        if start.story != end.story {
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
        if start.story != StoryId::Body || end.story != StoryId::Body {
            return Err(EditError::CrossParagraphSelection);
        }
        let end_map = TextMap::new(self.paragraph_for(&end)?)?;
        end_map.validate_pos(&end)?;

        let mut paragraphs: Vec<_> = self
            .paragraph_index
            .iter()
            .filter(|(_, location)| location.story == StoryId::Body)
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
        let (start_index, start_offset, end_index, end_offset) =
            if compare_positions(&self.document.body, &self.paragraph_index, &start, &end)?.is_gt()
            {
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

    /// 结构选区的纯文本投影；块公式与行内公式统一输出 U+FFFC。
    fn structure_selection_text(&self) -> Result<String, EditError> {
        // 端点必须可在当前文档排序；投影本身只用覆盖列表与文本边界。
        self.ordered_structure_selection()?;
        let (environments, math_blocks) = self.structure_range_block_nodes()?;
        // 只覆盖块对象时没有可复制文字，两端最近段落会交错，此时不按段落切口复制。
        let bounds = self.usable_structure_text_bounds()?.map(
            |(start_position, end_position, start_ordinal, end_ordinal)| {
                (
                    start_ordinal,
                    end_ordinal,
                    start_position.offset,
                    end_position.offset,
                )
            },
        );
        let mut text = String::new();
        let mut first = true;
        self.collect_structure_selection_text(
            &self.document.body,
            bounds,
            &environments,
            &math_blocks,
            false,
            &mut text,
            &mut first,
        )?;
        Ok(text)
    }

    /// 按物理块顺序汇集结构选区文本；`whole` 表示当前位于被覆盖环境内部。
    #[allow(clippy::too_many_arguments)]
    fn collect_structure_selection_text(
        &self,
        blocks: &[caret_doc::Block],
        bounds: Option<(usize, usize, usize, usize)>,
        environments: &[NodeId],
        math_blocks: &[NodeId],
        whole: bool,
        text: &mut String,
        first: &mut bool,
    ) -> Result<(), EditError> {
        for block in blocks {
            match block {
                caret_doc::Block::Paragraph(paragraph) => {
                    self.push_structure_paragraph(paragraph, bounds, whole, text, first)?;
                }
                caret_doc::Block::Environment(environment) => {
                    let covered = whole || environments.contains(&environment.id);
                    if let Some(title) = &environment.title {
                        self.push_structure_paragraph(title, bounds, covered, text, first)?;
                    }
                    self.collect_structure_selection_text(
                        &environment.children,
                        bounds,
                        environments,
                        math_blocks,
                        covered,
                        text,
                        first,
                    )?;
                }
                caret_doc::Block::Equation(equation)
                    if whole || math_blocks.contains(&equation.id) =>
                {
                    push_structure_selection_unit(text, first, None);
                }
                caret_doc::Block::Align(align) if whole || math_blocks.contains(&align.id) => {
                    push_structure_selection_unit(text, first, None);
                }
                caret_doc::Block::Equation(_)
                | caret_doc::Block::Align(_)
                | caret_doc::Block::ExplicitBreak(_)
                | caret_doc::Block::Opaque(_) => {}
            }
        }
        Ok(())
    }

    /// 汇集单个段落片段；整段复制时忽略范围切口。
    fn push_structure_paragraph(
        &self,
        paragraph: &caret_doc::Paragraph,
        bounds: Option<(usize, usize, usize, usize)>,
        whole: bool,
        text: &mut String,
        first: &mut bool,
    ) -> Result<(), EditError> {
        let map = TextMap::new(paragraph)?;
        let (from, to, emit) = if whole {
            (0, map.text.len(), true)
        } else {
            let Some((start_ordinal, end_ordinal, start_offset, end_offset)) = bounds else {
                return Ok(());
            };
            let Some(ordinal) = self.structure_index.paragraph_ordinal(&paragraph.id) else {
                return Ok(());
            };
            if ordinal < start_ordinal || ordinal > end_ordinal {
                return Ok(());
            }
            let text_len = map.text.len();
            let from = if ordinal == start_ordinal {
                start_offset.min(text_len)
            } else {
                0
            };
            let to = if ordinal == end_ordinal {
                end_offset.min(text_len)
            } else {
                text_len
            };
            let to = to.max(from);
            // 范围边界上的空片段不产生空行；范围内整段落即使为空也保留段落分隔。
            let boundary = ordinal == start_ordinal || ordinal == end_ordinal;
            (from, to, !boundary || to > from)
        };
        if !emit {
            return Ok(());
        }
        let fragment = map.text.get(from..to).ok_or(EditError::InvalidPosition)?;
        push_structure_selection_unit(text, first, Some(fragment));
        Ok(())
    }

    /// 将布局命中得到的视觉侧与同一 Rust 选区一起提交。
    pub fn set_selection_with_visual_affinity(
        &mut self,
        selection: Option<Selection>,
        visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        let anchor_visual_affinity = selection
            .as_ref()
            .filter(|selection| selection.anchor != selection.focus)
            .map(|_| self.selection_anchor_visual_affinity)
            .unwrap_or(visual_affinity);
        self.set_selection_with_visual_affinities(
            selection,
            anchor_visual_affinity,
            visual_affinity,
        )
    }

    /// 将正文选区两端的视觉侧与逻辑位置一并提交。
    pub fn set_selection_with_visual_affinities(
        &mut self,
        selection: Option<Selection>,
        anchor_visual_affinity: VisualAffinity,
        focus_visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        let previous_anchor_visual_affinity = self.selection_anchor_visual_affinity;
        let previous_visual_affinity = self.selection_visual_affinity;
        let mut effect = self.run(Command::SetSelection(selection))?;
        effect.selection_changed |= previous_anchor_visual_affinity != anchor_visual_affinity
            || previous_visual_affinity != focus_visual_affinity;
        self.selection_anchor_visual_affinity = anchor_visual_affinity;
        self.selection_visual_affinity = focus_visual_affinity;
        Ok(effect)
    }

    /// 统一提交鼠标产生的文本范围，并在覆盖或跨越块宿主时提升为结构选区。
    ///
    /// 提升判定与键盘导航共用同一覆盖投影，避免鼠标拖选跨过公式块时留下无法
    /// 删除、复制也丢失对象的普通文本范围。
    pub fn set_pointer_selection_range(
        &mut self,
        anchor: Pos,
        focus: Pos,
        anchor_visual_affinity: VisualAffinity,
        visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        if self.range_requires_structure_selection(&anchor, &focus)? {
            return self.promote_text_range_to_structure(
                anchor,
                focus,
                anchor_visual_affinity,
                visual_affinity,
            );
        }
        self.set_selection_with_visual_affinities(
            Some(Selection { anchor, focus }),
            anchor_visual_affinity,
            visual_affinity,
        )
    }

    /// 判定文本范围是否需要提升为结构选区。
    ///
    /// 范围完整覆盖块环境或 Equation/Align 时提升；只跨越环境边界但未完整包含时
    /// 沿用既有的环境提升规则，保证选中块环境仍以整块环境为目标。
    fn range_requires_structure_selection(
        &self,
        anchor: &Pos,
        focus: &Pos,
    ) -> Result<bool, EditError> {
        if anchor == focus {
            return Ok(false);
        }
        let (environments, math_blocks) = self.covered_block_nodes_for_text_pair(anchor, focus)?;
        if !environments.is_empty() || !math_blocks.is_empty() {
            return Ok(true);
        }
        Ok(self.range_requires_block_environment_selection(anchor, focus))
    }

    /// 提升文本范围为结构选区；环境范围沿用环境投影，其余由覆盖投影派生。
    fn promote_text_range_to_structure(
        &mut self,
        anchor: Pos,
        focus: Pos,
        anchor_visual_affinity: VisualAffinity,
        focus_visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        if self.range_requires_block_environment_selection(&anchor, &focus) {
            return self.select_block_environments_from_range_with_anchor_affinity(
                anchor,
                focus,
                anchor_visual_affinity,
                focus_visual_affinity,
            );
        }
        self.install_block_node_range_selection(
            anchor,
            focus,
            anchor_visual_affinity,
            focus_visual_affinity,
        )
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

    pub(super) fn block_environment_at(&self, position: &Pos) -> Option<NodeId> {
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

    /// 把结构选区折叠到方向对应的一侧并消费按键：Forward 到范围终点、Backward 到起点。
    ///
    /// 结构范围的逻辑 Selection 始终折叠在焦点端，若直接沿用普通文本候选会从焦点再
    /// 移动一个视觉步长，导致落点漂移或越出范围。这里先按文档顺序取端点并复用共享
    /// 停靠策略，再清除结构状态；任何端点校验失败都在写入前返回。
    pub(super) fn collapse_structure_selection(
        &mut self,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        let (start, end) = {
            let (start, end) = self.ordered_structure_selection()?;
            (start.clone(), end.clone())
        };
        let visual_affinity = |endpoint: &DocumentRangeEndpoint| match endpoint {
            DocumentRangeEndpoint::Text(position) => visual_affinity_for(position.affinity),
            DocumentRangeEndpoint::BlockEdge { side, .. } => visual_affinity_for_block_edge(*side),
        };
        let (endpoint, visual_affinity) = match direction {
            Direction::Forward => {
                let visual_affinity = visual_affinity(&end);
                (end, visual_affinity)
            }
            Direction::Backward => {
                let visual_affinity = visual_affinity(&start);
                (start, visual_affinity)
            }
        };
        let (caret, host_caret_position) = self.park_at_range_endpoint(&endpoint)?;
        self.clear_structure_selection_state();
        self.selection = Selection {
            anchor: caret.clone(),
            focus: caret,
        };
        self.selection_visual_affinity = visual_affinity;
        self.selection_anchor_visual_affinity = visual_affinity;
        self.host_caret_position = host_caret_position;
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    /// 提交键盘导航产生的位置，并按逻辑移动方向更新环境边界亲和性。
    pub fn set_navigation_selection(
        &mut self,
        selection: Selection,
        visual_affinity: VisualAffinity,
        intent: NavigationIntent,
    ) -> Result<Effect, EditError> {
        self.set_navigation_selection_with_direction(selection, visual_affinity, intent, None)
    }

    /// 提交键盘导航候选；空段落没有可从位置推导的方向，因此由实体方向键显式提供。
    pub(super) fn set_navigation_selection_with_direction(
        &mut self,
        mut selection: Selection,
        visual_affinity: VisualAffinity,
        intent: NavigationIntent,
        logical_direction: Option<Direction>,
    ) -> Result<Effect, EditError> {
        self.ensure_math_edit_revision_current()?;
        self.validate_navigation_position(&selection.anchor)?;
        self.validate_navigation_position(&selection.focus)?;
        // 候选可以落在不同 Story（例如从正文进入环境标题），因此用共享结构端点
        // 比较而不是限制同 Story；无法比较时保持无方向，避免误判逐块移动。
        let candidate_direction = if self.selection.focus == selection.focus {
            None
        } else {
            match self.compare_range_endpoints(
                &DocumentRangeEndpoint::Text(self.selection.focus.clone()),
                &DocumentRangeEndpoint::Text(selection.focus.clone()),
            ) {
                Ok(std::cmp::Ordering::Less) => Some(Direction::Forward),
                Ok(std::cmp::Ordering::Greater) => Some(Direction::Backward),
                _ => None,
            }
        };
        let was_collapsed = selection.anchor == selection.focus;
        if intent == NavigationIntent::Horizontal
            && was_collapsed
            && let Some(direction) = logical_direction
            && let Some(effect) = self.move_from_math_block_boundary_stop(direction)?
        {
            return Ok(effect);
        }
        let anchor_visual_affinity = if self.selection.anchor == self.selection.focus
            && self.structure_selection.is_none()
        {
            self.selection_visual_affinity
        } else {
            self.selection_anchor_visual_affinity
        };
        // 每个复杂块是一个文档单元：布局候选若一次跨过一个单元以上，不能把整段
        // 文本范围直接提升为同时覆盖多个单元的结构选区。
        if intent == NavigationIntent::Horizontal
            && selection.anchor != selection.focus
            && let Some(direction) = logical_direction
        {
            let (environments, math_blocks) =
                self.covered_block_nodes_for_text_pair(&selection.anchor, &selection.focus)?;
            if (environments.is_empty() && math_blocks.len() > 1)
                || self.candidate_skips_next_complex_unit(&selection.focus, direction)?
            {
                return self.extend_structure_selection(direction);
            }
        }
        if self.range_requires_structure_selection(&selection.anchor, &selection.focus)? {
            return self.promote_text_range_to_structure(
                selection.anchor,
                selection.focus,
                anchor_visual_affinity,
                visual_affinity,
            );
        }
        self.validate_selection(&selection)?;
        if intent == NavigationIntent::Horizontal
            && was_collapsed
            && self.selection.anchor == self.selection.focus
            && self.math_edit.is_none()
            && self.selection.focus.story == selection.focus.story
            && self.selection.focus.paragraph_id == selection.focus.paragraph_id
            && self.selection.focus.offset == selection.focus.offset
            && let Some(direction) = logical_direction
            && self.enter_adjacent_math_if_present(direction)?
        {
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        if intent == NavigationIntent::Horizontal
            && was_collapsed
            && self.selection.anchor == self.selection.focus
            && self.math_edit.is_none()
            && let Some(direction) =
                self.navigation_math_crossing_direction(&self.selection.focus, &selection.focus)?
            && self.enter_adjacent_math_if_present(direction)?
        {
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        // 任意复杂块链：布局候选可能一次跨过紧邻的公式对象，仍按块外侧停靠/进入
        // 逐个消费，不能跳过第一个公式直接落到后面的块环境或段落。
        if intent == NavigationIntent::Horizontal
            && was_collapsed
            && self.selection.anchor == self.selection.focus
            && self.math_edit.is_none()
            && let Some(direction) = candidate_direction
            && self.paragraph_beside_block_math(&self.selection.focus, direction)?
            && self.candidate_skips_next_complex_unit(&selection.focus, direction)?
            && self.enter_adjacent_math_if_present(direction)?
        {
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        if intent == NavigationIntent::Horizontal
            && was_collapsed
            && self.selection.anchor == self.selection.focus
            && let Some(direction) =
                self.navigation_block_boundary_stop_direction(&selection.focus, logical_direction)?
        {
            return self.run(Command::ExitBlockEnvironment(direction));
        }
        let ordering = compare_positions(
            &self.document.body,
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
        self.validate_selection(&selection)?;
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
        self.select_block_environments_from_range_with_anchor_affinity(
            anchor,
            focus,
            visual_affinity,
            visual_affinity,
        )
    }

    /// 保存结构选区时分别保留锚点和焦点的排版视觉侧。
    pub(in crate::editing::session) fn select_block_environments_from_range_with_anchor_affinity(
        &mut self,
        anchor: Pos,
        focus: Pos,
        anchor_visual_affinity: VisualAffinity,
        focus_visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        if self.has_uncommitted_math_source_draft() {
            return Err(EditError::InvalidMath(
                "resolve or cancel the incomplete math source draft before changing selection"
                    .to_owned(),
            ));
        }
        self.ensure_math_edit_revision_current()?;
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
        // 环境范围投影必须至少覆盖一个块环境；公式块由同一范围按覆盖派生。
        let environments = self.block_environments_in_range(&anchor, &focus);
        if environments.is_empty() {
            return Err(EditError::InvalidPosition);
        }
        // 端点扩展到整个环境外侧边界，使结构范围与派生列表描述同一范围。
        let structure_selection =
            self.normalized_environment_range(&anchor, &focus, &environments)?;
        let previous_selection = self.structure_selection.clone();
        let previous_environments = self.selected_block_environments.clone();
        let previous_math_blocks = self.selected_math_blocks.clone();
        // 目标与范围均有效后，才切换离开公式编辑状态。
        self.install_structure_selection(structure_selection)?;
        let cancels_command_line_preedit = self
            .command_line
            .as_ref()
            .is_some_and(|line| !line.preedit.is_empty());
        let changed = self.structure_selection != previous_selection
            || self.selected_block_environments != previous_environments
            || self.selected_math_blocks != previous_math_blocks
            || self.selection.focus != focus
            || self.selection_anchor_visual_affinity != anchor_visual_affinity
            || self.selection_visual_affinity != focus_visual_affinity;
        self.math_edit = None;
        self.selection = Selection {
            anchor: focus.clone(),
            focus: focus.clone(),
        };
        self.selection_anchor_visual_affinity = anchor_visual_affinity;
        self.selection_visual_affinity = focus_visual_affinity;
        self.host_caret_position = None;
        if cancels_command_line_preedit {
            self.command_line = None;
        }
        Ok(Effect {
            selection_changed: changed,
            composition_changed: cancels_command_line_preedit,
            ..Effect::default()
        })
    }

    /// 候选范围完整跨越块环境或 Equation/Align 时记录结构范围并折叠光标。
    ///
    /// 覆盖列表由 `install_structure_selection` 统一派生；调用方已通过共享提升判定，
    /// 因此这里只负责安装与提交光标/视觉状态。
    fn install_block_node_range_selection(
        &mut self,
        anchor: Pos,
        focus: Pos,
        anchor_visual_affinity: VisualAffinity,
        focus_visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        self.validate_selection(&Selection {
            anchor: anchor.clone(),
            focus: focus.clone(),
        })?;
        let structure_selection = StructureSelection {
            anchor: DocumentRangeEndpoint::Text(anchor),
            focus: DocumentRangeEndpoint::Text(focus.clone()),
        };
        let previous_selection = self.structure_selection.clone();
        let previous_environments = self.selected_block_environments.clone();
        let previous_math_blocks = self.selected_math_blocks.clone();
        self.install_structure_selection(structure_selection)?;
        let changed = self.structure_selection != previous_selection
            || self.selected_block_environments != previous_environments
            || self.selected_math_blocks != previous_math_blocks;
        self.math_edit = None;
        self.selection = Selection {
            anchor: focus.clone(),
            focus,
        };
        self.selection_anchor_visual_affinity = anchor_visual_affinity;
        self.selection_visual_affinity = focus_visual_affinity;
        self.host_caret_position = None;
        Ok(Effect {
            selection_changed: changed,
            ..Effect::default()
        })
    }

    fn validate_navigation_position(&mut self, position: &Pos) -> Result<(), EditError> {
        self.text_map_for(position)?.validate_pos(position)
    }

    /// 仅在经过位置校验和相邻公式仲裁后，判断候选是否跨出当前块环境。
    fn navigation_block_boundary_stop_direction(
        &mut self,
        target: &Pos,
        visual_direction: Option<Direction>,
    ) -> Result<Option<Direction>, EditError> {
        let current = self.selection.focus.clone();
        if current.paragraph_id == target.paragraph_id {
            return Ok(None);
        }
        let current_environments = self
            .structure_index
            .at(&caret_structure::StructurePosition {
                paragraph_id: current.paragraph_id.clone(),
                offset_utf8: current.offset,
            })
            .into_iter()
            .filter(|entry| entry.kind.is_block_environment())
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        let target_environments = self
            .structure_index
            .at(&caret_structure::StructurePosition {
                paragraph_id: target.paragraph_id.clone(),
                offset_utf8: target.offset,
            })
            .into_iter()
            .filter(|entry| entry.kind.is_block_environment())
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        if current_environments.is_empty()
            || !current_environments
                .iter()
                .any(|id| !target_environments.contains(id))
        {
            return Ok(None);
        }
        let text_len = self.text_map_for(&current)?.text.len();
        Ok(if text_len == 0 {
            visual_direction
        } else if current.offset == text_len {
            Some(Direction::Forward)
        } else if current.offset == 0 {
            Some(Direction::Backward)
        } else {
            None
        })
    }

    /// 判断水平候选是否跳过紧邻的下一个复杂块单元。
    ///
    /// 以结构范围扩展的同一单元规则为准：从当前焦点（或已有结构范围焦点）按一个
    /// 文档单元前进，只有该单元是公式对象/整棵块环境且候选越过它时才需要先消费
    /// 该单元；纯文本单元不在此处拦截，保持原有字素/视觉移动。
    fn candidate_skips_next_complex_unit(
        &mut self,
        target: &Pos,
        direction: Direction,
    ) -> Result<bool, EditError> {
        let current = match self.ordered_structure_selection() {
            Ok((_, focus)) => focus.clone(),
            Err(_) => DocumentRangeEndpoint::Text(self.selection.focus.clone()),
        };
        let Some(unit) = self.next_range_focus_step(&current, direction)? else {
            return Ok(false);
        };
        if !matches!(unit, DocumentRangeEndpoint::BlockEdge { .. }) {
            return Ok(false);
        }
        let ordering =
            self.compare_range_endpoints(&DocumentRangeEndpoint::Text(target.clone()), &unit)?;
        let beyond_unit = match direction {
            Direction::Forward => ordering.is_gt(),
            Direction::Backward => ordering.is_lt(),
        };
        if !beyond_unit {
            return Ok(false);
        }
        // 第一个复杂单元之后还有另一个复杂单元时才需要逐单元消费；只跨单个公式
        // 对象、随后就是普通文本的候选仍保持精确文本端点提升（既有契约）。
        let Some(following) = self.next_range_focus_step(&unit, direction)? else {
            return Ok(false);
        };
        Ok(matches!(following, DocumentRangeEndpoint::BlockEdge { .. }))
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
        let next = selection.unwrap_or_else(|| Selection {
            anchor: self.selection.focus.clone(),
            focus: self.selection.focus.clone(),
        });
        self.validate_selection(&next)?;
        let changed = self.selection != next
            || self.host_caret_position.is_some()
            || self.has_structure_selection()
            || !self.selected_block_environments.is_empty()
            || !self.selected_math_blocks.is_empty();
        self.host_caret_position = None;
        self.clear_structure_selection_state();
        self.selection = next;
        Ok(Effect {
            selection_changed: changed,
            ..Effect::default()
        })
    }

    pub(super) fn validate_selection(&mut self, next: &Selection) -> Result<(), EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if next.anchor.story != next.focus.story {
            return Err(EditError::CrossParagraphSelection);
        }
        self.text_map_for(&next.anchor)?
            .validate_pos(&next.anchor)?;
        self.text_map_for(&next.focus)?.validate_pos(&next.focus)?;
        Ok(())
    }
}

/// 追加一个结构选区文本单元；`None` 表示块级公式对象，与行内公式一样输出 U+FFFC。
fn push_structure_selection_unit(text: &mut String, first: &mut bool, fragment: Option<&str>) {
    if !*first {
        text.push('\n');
    }
    match fragment {
        Some(fragment) => text.push_str(fragment),
        None => text.push(OBJECT_REPLACEMENT),
    }
    *first = false;
}

#[cfg(test)]
#[path = "selection/tests.rs"]
mod tests;
