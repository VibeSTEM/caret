//! 将行内公式创建与数学树编辑纳入 Session 文档事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::paragraph_ops::paragraph_at_location_mut;
use super::super::text_map::TextMap;
use super::super::{
    Affinity, EditError, Effect, MathDirection, MathFocus, MathPos, Pos, PositionMap, Selection,
};
use super::Session;
use crate::apply_math_edit;
use crate::math_edit::{MathEdit, MathEditError};
use caret_doc::{
    InlineMath, InlineNode, MATH_TREE_SCHEMA_VERSION, MathNode, MathNodeKind, MathTree, NodeId,
};
use caret_layout::VisualAffinity;
use std::collections::BTreeMap;
use unicode_segmentation::UnicodeSegmentation;

impl Session {
    pub(super) fn insert_math_text(
        &mut self,
        text: &str,
        buffer_incomplete: bool,
    ) -> Result<Effect, EditError> {
        if self.math_focus.is_none() {
            return Err(EditError::InvalidPosition);
        }
        let pending_input = self.math_input_buffer.take();
        let prior_input = pending_input.clone();
        let previous_error = self.math_input_error;
        let had_pending_input = pending_input.is_some();
        let mut input = pending_input.unwrap_or_default();
        if input.len().saturating_add(text.len()) > caret_math::MAX_MATH_SOURCE_BYTES {
            self.math_input_buffer = prior_input;
            self.math_input_error = previous_error;
            return Err(EditError::InvalidMathText);
        }
        input.push_str(text);
        if buffer_incomplete
            && caret_math::classify_input(&input, caret_math::MathInputKind::Inline)
                == caret_math::MathInputStatus::Incomplete
        {
            self.math_input_buffer = Some(input);
            self.math_input_error = false;
            return Ok(Effect {
                math_input_changed: true,
                ..Effect::default()
            });
        }
        match self.insert_math_edit(MathEdit::InsertText(input.clone())) {
            Ok(mut effect) => {
                self.math_input_error = false;
                effect.math_input_changed = had_pending_input;
                Ok(effect)
            }
            Err(EditError::InvalidMathText) => {
                let incomplete = buffer_incomplete
                    && caret_math::classify_input(&input, caret_math::MathInputKind::Inline)
                        == caret_math::MathInputStatus::Incomplete;
                self.math_input_buffer = Some(input);
                self.math_input_error = !incomplete;
                Ok(Effect {
                    math_input_changed: true,
                    math_input_error: !incomplete,
                    ..Effect::default()
                })
            }
            Err(error) => {
                self.math_input_buffer = prior_input;
                self.math_input_error = previous_error;
                Err(error)
            }
        }
    }

    pub(super) fn backspace_math_input(&mut self) -> Result<Effect, EditError> {
        let Some(mut input) = self.math_input_buffer.take() else {
            return Err(EditError::InvalidMathText);
        };
        let previous_input = input.clone();
        let previous_error = self.math_input_error;
        if let Some((offset, _)) = input.grapheme_indices(true).next_back() {
            input.truncate(offset);
        }
        if input.is_empty() {
            self.math_input_error = false;
            return Ok(Effect {
                math_input_changed: true,
                ..Effect::default()
            });
        }
        match caret_math::classify_input(&input, caret_math::MathInputKind::Inline) {
            caret_math::MathInputStatus::Complete => {
                self.math_input_error = false;
                match self.run(super::super::Command::InsertMathText(input)) {
                    Ok(mut effect) => {
                        effect.math_input_changed = true;
                        Ok(effect)
                    }
                    Err(error) => {
                        self.math_input_buffer = Some(previous_input);
                        self.math_input_error = previous_error;
                        Err(error)
                    }
                }
            }
            caret_math::MathInputStatus::Incomplete => {
                self.math_input_buffer = Some(input);
                self.math_input_error = false;
                Ok(Effect {
                    math_input_changed: true,
                    ..Effect::default()
                })
            }
            caret_math::MathInputStatus::Invalid => {
                self.math_input_buffer = Some(input);
                self.math_input_error = true;
                Ok(Effect {
                    math_input_changed: true,
                    math_input_error: true,
                    ..Effect::default()
                })
            }
        }
    }

    pub(super) fn move_math_caret(
        &mut self,
        direction: MathDirection,
    ) -> Result<Effect, EditError> {
        if self.math_input_buffer.is_some() {
            return Err(EditError::InvalidMathText);
        }
        let focus = self.math_focus.clone().ok_or(EditError::InvalidPosition)?;
        let story = self
            .paragraph_index
            .get(&focus.paragraph_id)
            .map(|location| location.story.clone())
            .ok_or_else(|| EditError::UnknownParagraph(focus.paragraph_id.to_string()))?;
        let paragraph_pos = Pos {
            story,
            paragraph_id: focus.paragraph_id.clone(),
            offset: self.selection.focus.offset,
            affinity: self.selection.focus.affinity,
        };
        let paragraph = self.paragraph_for(&paragraph_pos)?;
        let math = find_inline_math(&paragraph.inlines, &focus.inline_math_id)
            .ok_or(EditError::InvalidPosition)?;
        if focus.position.resolve(&math.math_tree).is_none() {
            return Err(EditError::InvalidPosition);
        }
        let next_position = match direction {
            MathDirection::Left | MathDirection::Right => {
                let mut positions = Vec::new();
                collect_horizontal_positions(
                    &math.math_tree.root,
                    vec![math.math_tree.root.id.clone()],
                    &mut positions,
                );
                let current = positions
                    .iter()
                    .position(|position| position == &focus.position);
                match (direction, current) {
                    (MathDirection::Left, Some(index)) if index > 0 => {
                        positions.get(index - 1).cloned()
                    }
                    (MathDirection::Right, Some(index)) => positions.get(index + 1).cloned(),
                    (MathDirection::Right, None) => positions.first().cloned(),
                    (MathDirection::Left, None) => None,
                    _ => None,
                }
            }
            MathDirection::Up | MathDirection::Down => {
                vertical_math_target(&math.math_tree, &focus.position, direction)
            }
        };
        if let Some(position) = next_position {
            if position == focus.position {
                return Ok(Effect::default());
            }
            self.advance_history_merge_epoch();
            self.math_focus
                .as_mut()
                .ok_or(EditError::InvalidPosition)?
                .position = position;
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        match direction {
            MathDirection::Left => self.exit_math_toward(false),
            MathDirection::Right => self.exit_math_toward(true),
            MathDirection::Up | MathDirection::Down => Ok(Effect::default()),
        }
    }

    pub(super) fn move_math_caret_to_next_placeholder(&mut self) -> Result<Effect, EditError> {
        if self.math_input_buffer.is_some() {
            return Err(EditError::InvalidMathText);
        }
        let focus = self.math_focus.clone().ok_or(EditError::InvalidPosition)?;
        let story = self
            .paragraph_index
            .get(&focus.paragraph_id)
            .map(|location| location.story.clone())
            .ok_or_else(|| EditError::UnknownParagraph(focus.paragraph_id.to_string()))?;
        let paragraph_pos = Pos {
            story,
            paragraph_id: focus.paragraph_id.clone(),
            offset: self.selection.focus.offset,
            affinity: self.selection.focus.affinity,
        };
        let paragraph = self.paragraph_for(&paragraph_pos)?;
        let math = find_inline_math(&paragraph.inlines, &focus.inline_math_id)
            .ok_or(EditError::InvalidPosition)?;
        if focus.position.resolve(&math.math_tree).is_none() {
            return Err(EditError::InvalidPosition);
        }

        let Some(current_indices) = math_path_indices(&math.math_tree.root, &focus.position.path)
        else {
            return Err(EditError::InvalidPosition);
        };
        let mut path = vec![&math.math_tree.root.id];
        let mut indices = Vec::with_capacity(current_indices.len());
        let next_position = next_placeholder_after(
            &math.math_tree.root,
            &current_indices,
            focus.position.offset,
            &mut path,
            &mut indices,
        );
        let Some(position) = next_position else {
            return Ok(Effect::default());
        };
        self.advance_history_merge_epoch();
        self.math_focus
            .as_mut()
            .ok_or(EditError::InvalidPosition)?
            .position = position;
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    fn exit_math_toward(&mut self, after_object: bool) -> Result<Effect, EditError> {
        let Some(focus) = self.math_focus.clone() else {
            return Ok(Effect::default());
        };
        let mut position = self.selection.focus.clone();
        if !after_object {
            position.offset = position
                .offset
                .checked_sub("\u{fffc}".len())
                .ok_or(EditError::InvalidPosition)?;
            position.affinity = Affinity::Upstream;
        } else {
            position.affinity = Affinity::Downstream;
        }
        self.text_map_for(&position)?.validate_pos(&position)?;
        self.advance_history_merge_epoch();
        self.math_focus = None;
        self.selection = Selection {
            anchor: position.clone(),
            focus: position,
        };
        self.selection_visual_affinity = match self.selection.focus.affinity {
            Affinity::Upstream => VisualAffinity::Upstream,
            Affinity::Downstream => VisualAffinity::Downstream,
        };
        debug_assert_eq!(focus.paragraph_id, self.selection.focus.paragraph_id);
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    pub(super) fn insert_inline_math(&mut self) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let (anchor, focus) = self.edit_range()?;
        if anchor.paragraph_id != focus.paragraph_id {
            return Err(EditError::CrossParagraphSelection);
        }
        let start = if anchor.offset <= focus.offset {
            &anchor
        } else {
            &focus
        };
        let start_offset = anchor.offset.min(focus.offset);
        let end_offset = anchor.offset.max(focus.offset);
        let mut paragraph = self.paragraph_for(&anchor)?.clone();
        let map = TextMap::new(&paragraph)?;
        map.validate_pos(&anchor)?;
        map.validate_pos(&focus)?;
        if start_offset != end_offset {
            map.delete_range(&mut paragraph, start_offset, end_offset)?;
        }
        let math_id = NodeId::generate();
        let root_id = NodeId::generate();
        let math_tree = MathTree {
            schema_version: MATH_TREE_SCHEMA_VERSION,
            source: String::new(),
            root: MathNode {
                id: root_id.clone(),
                kind: MathNodeKind::Row,
                text: None,
                attributes: BTreeMap::new(),
                source_range: None,
                children: Vec::new(),
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        };
        let inline_math = InlineMath {
            id: math_id.clone(),
            math_tree,
            extra: BTreeMap::new(),
        };
        let updated_map = TextMap::new(&paragraph)?;
        updated_map.insert_math(&mut paragraph, start_offset, start.affinity, inline_math)?;
        self.validate_paragraph_candidate(&anchor, &paragraph)?;
        self.replace_paragraph(&anchor, paragraph)?;

        let inserted_len = "\u{fffc}".len();
        let result_offset = start_offset + inserted_len;
        self.text_map_cache.clear();
        let result_boundaries = self
            .text_map_for(&Pos {
                offset: result_offset,
                ..start.clone()
            })?
            .grapheme_boundaries
            .clone();
        let snapped_offset = result_boundaries
            .iter()
            .copied()
            .find(|boundary| *boundary >= result_offset)
            .unwrap_or(result_offset);
        let body_position = Pos {
            offset: snapped_offset,
            affinity: Affinity::Downstream,
            ..anchor.clone()
        };
        self.selection = Selection {
            anchor: body_position.clone(),
            focus: body_position,
        };
        self.selection_visual_affinity = VisualAffinity::Downstream;
        self.math_focus = Some(MathFocus {
            paragraph_id: anchor.paragraph_id.clone(),
            inline_math_id: math_id,
            position: MathPos {
                path: vec![root_id],
                offset: 0,
            },
        });
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            position_map: Some(PositionMap::TextEdit {
                paragraph_id: anchor.paragraph_id,
                start: start_offset,
                end: end_offset,
                inserted_len,
                snap_graphemes: true,
                result_boundaries,
            }),
            ..Effect::default()
        })
    }

    pub(super) fn insert_math_edit(&mut self, edit: MathEdit) -> Result<Effect, EditError> {
        if self.math_input_buffer.is_some() {
            return Err(EditError::InvalidMathText);
        }
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let mut focus = self.math_focus.clone().ok_or(EditError::InvalidPosition)?;
        let story = self
            .paragraph_index
            .get(&focus.paragraph_id)
            .map(|location| location.story.clone())
            .ok_or_else(|| EditError::UnknownParagraph(focus.paragraph_id.to_string()))?;
        let paragraph_pos = Pos {
            story,
            paragraph_id: focus.paragraph_id.clone(),
            offset: self.selection.focus.offset,
            affinity: self.selection.focus.affinity,
        };
        let mut paragraph = self.paragraph_for(&paragraph_pos)?.clone();
        let inline_math = find_inline_math_mut(&mut paragraph.inlines, &focus.inline_math_id)
            .ok_or(EditError::InvalidPosition)?;
        let before_math_node_count = math_node_count(&inline_math.math_tree.root);
        let result = apply_math_edit(&inline_math.math_tree, &focus.position, edit)
            .map_err(map_math_edit_error)?;
        let increases_document_node_count =
            math_node_count(&result.tree.root) > before_math_node_count;
        inline_math.math_tree = result.tree;
        focus.position = result.position;
        if increases_document_node_count {
            self.validate_paragraph_candidate(&paragraph_pos, &paragraph)?;
        }
        self.replace_paragraph(&paragraph_pos, paragraph)?;
        self.math_focus = Some(focus);
        self.text_map_cache.clear();
        Ok(Effect {
            document_changed: true,
            ..Effect::default()
        })
    }

    pub(super) fn exit_math(&mut self) -> Result<Effect, EditError> {
        if self.math_input_buffer.is_some() {
            return Err(EditError::InvalidMathText);
        }
        if self.math_focus.take().is_none() {
            return Ok(Effect::default());
        }
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    fn validate_paragraph_candidate(
        &self,
        pos: &Pos,
        paragraph: &caret_doc::Paragraph,
    ) -> Result<(), EditError> {
        let location = self
            .paragraph_index
            .get(&pos.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))?;
        if location.story != pos.story {
            return Err(EditError::WrongStory);
        }
        let mut candidate = self.document.clone();
        *paragraph_at_location_mut(&mut candidate.body, location)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))? =
            paragraph.clone();
        candidate
            .validate()
            .map_err(|error| EditError::InvalidDocument(error.to_string()))
    }
}

fn math_node_count(root: &MathNode) -> usize {
    let mut count = 0usize;
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        count = count.saturating_add(1);
        pending.extend(&node.children);
    }
    count
}

fn find_inline_math<'a>(nodes: &'a [InlineNode], id: &NodeId) -> Option<&'a InlineMath> {
    for node in nodes {
        match node {
            InlineNode::Math(math) if &math.id == id => return Some(math),
            InlineNode::Environment(environment) => {
                if let Some(math) = find_inline_math(&environment.children, id) {
                    return Some(math);
                }
            }
            InlineNode::Math(_)
            | InlineNode::Run(_)
            | InlineNode::SoftBreak(_)
            | InlineNode::Opaque(_) => {}
        }
    }
    None
}

fn find_inline_math_mut<'a>(
    nodes: &'a mut [InlineNode],
    id: &NodeId,
) -> Option<&'a mut InlineMath> {
    for node in nodes {
        match node {
            InlineNode::Math(math) if &math.id == id => return Some(math),
            InlineNode::Environment(environment) => {
                if let Some(math) = find_inline_math_mut(&mut environment.children, id) {
                    return Some(math);
                }
            }
            InlineNode::Math(_)
            | InlineNode::Run(_)
            | InlineNode::SoftBreak(_)
            | InlineNode::Opaque(_) => {}
        }
    }
    None
}

fn collect_horizontal_positions(node: &MathNode, path: Vec<NodeId>, positions: &mut Vec<MathPos>) {
    match node.kind {
        MathNodeKind::Row => {
            if node.children.is_empty() {
                push_math_position(positions, MathPos { path, offset: 0 });
                return;
            }
            for (index, child) in node.children.iter().enumerate() {
                if is_math_structure(child.kind) {
                    push_math_position(
                        positions,
                        MathPos {
                            path: path.clone(),
                            offset: index,
                        },
                    );
                }
                let mut child_path = path.clone();
                child_path.push(child.id.clone());
                collect_horizontal_positions(child, child_path, positions);
                if is_math_structure(child.kind) {
                    push_math_position(
                        positions,
                        MathPos {
                            path: path.clone(),
                            offset: index + 1,
                        },
                    );
                }
            }
        }
        MathNodeKind::Symbol
            if node.attributes.get("atomClass").map(String::as_str) == Some("ord") =>
        {
            let text = node.text.as_deref().unwrap_or_default();
            let mut offsets = text
                .grapheme_indices(true)
                .map(|(offset, _)| offset)
                .collect::<Vec<_>>();
            offsets.push(text.len());
            for offset in offsets {
                push_math_position(
                    positions,
                    MathPos {
                        path: path.clone(),
                        offset,
                    },
                );
            }
        }
        MathNodeKind::Symbol => {
            push_math_position(
                positions,
                MathPos {
                    path: path.clone(),
                    offset: 0,
                },
            );
            push_math_position(positions, MathPos { path, offset: 1 });
        }
        MathNodeKind::TextContent => {
            let text = node.text.as_deref().unwrap_or_default();
            let mut offsets = text
                .grapheme_indices(true)
                .map(|(offset, _)| offset)
                .collect::<Vec<_>>();
            offsets.push(text.len());
            for offset in offsets {
                push_math_position(
                    positions,
                    MathPos {
                        path: path.clone(),
                        offset,
                    },
                );
            }
        }
        MathNodeKind::TextSymbol
        | MathNodeKind::Operator
        | MathNodeKind::SizedDelimiter
        | MathNodeKind::TextSpace
        | MathNodeKind::Space
        | MathNodeKind::Strut => {
            push_math_position(
                positions,
                MathPos {
                    path: path.clone(),
                    offset: 0,
                },
            );
            push_math_position(positions, MathPos { path, offset: 1 });
        }
        MathNodeKind::Placeholder => push_math_position(positions, MathPos { path, offset: 0 }),
        _ if node.children.is_empty() => push_math_position(positions, MathPos { path, offset: 0 }),
        _ => {
            for child in &node.children {
                let mut child_path = path.clone();
                child_path.push(child.id.clone());
                collect_horizontal_positions(child, child_path, positions);
            }
        }
    }
}

fn math_path_indices(root: &MathNode, path: &[NodeId]) -> Option<Vec<usize>> {
    let (root_id, descendants) = path.split_first()?;
    if root_id != &root.id {
        return None;
    }
    let mut node = root;
    let mut indices = Vec::with_capacity(descendants.len());
    for id in descendants {
        let index = node.children.iter().position(|child| &child.id == id)?;
        node = node.children.get(index)?;
        indices.push(index);
    }
    Some(indices)
}

fn next_placeholder_after<'tree>(
    node: &'tree MathNode,
    current_indices: &[usize],
    current_offset: usize,
    path: &mut Vec<&'tree NodeId>,
    indices: &mut Vec<usize>,
) -> Option<MathPos> {
    if node.kind == MathNodeKind::Placeholder
        && math_position_precedes_child_path(current_indices, current_offset, indices)
    {
        return Some(MathPos {
            path: path.iter().map(|id| (*id).clone()).collect(),
            offset: 0,
        });
    }
    for (index, child) in node.children.iter().enumerate() {
        path.push(&child.id);
        indices.push(index);
        let result = next_placeholder_after(child, current_indices, current_offset, path, indices);
        indices.pop();
        path.pop();
        if result.is_some() {
            return result;
        }
    }
    None
}

fn math_position_precedes_child_path(
    current_indices: &[usize],
    current_offset: usize,
    child_indices: &[usize],
) -> bool {
    let common_depth = current_indices
        .iter()
        .zip(child_indices)
        .take_while(|(current, child)| current == child)
        .count();
    if common_depth == current_indices.len() {
        return child_indices
            .get(common_depth)
            .is_some_and(|child_index| current_offset <= *child_index);
    }
    child_indices
        .get(common_depth)
        .zip(current_indices.get(common_depth))
        .is_some_and(|(child_index, current_index)| child_index > current_index)
}

fn push_math_position(positions: &mut Vec<MathPos>, position: MathPos) {
    if positions.last() != Some(&position) {
        positions.push(position);
    }
}

fn is_math_structure(kind: MathNodeKind) -> bool {
    !matches!(
        kind,
        MathNodeKind::Symbol
            | MathNodeKind::TextContent
            | MathNodeKind::Placeholder
            | MathNodeKind::TextSymbol
            | MathNodeKind::Operator
            | MathNodeKind::SizedDelimiter
            | MathNodeKind::TextSpace
            | MathNodeKind::Space
            | MathNodeKind::Strut
            | MathNodeKind::Hline
    )
}

fn vertical_math_target(
    tree: &MathTree,
    position: &MathPos,
    direction: MathDirection,
) -> Option<MathPos> {
    let mut parent = &tree.root;
    let mut ancestry = Vec::new();
    let mut parent_path = vec![tree.root.id.clone()];
    for child_id in position.path.iter().skip(1) {
        let (index, child) = parent
            .children
            .iter()
            .enumerate()
            .find(|(_, child)| &child.id == child_id)?;
        ancestry.push((parent, index, parent_path.clone()));
        parent_path.push(child.id.clone());
        parent = child;
    }
    for (parent, child_index, parent_path) in ancestry.into_iter().rev() {
        let target_index = match (parent.kind, child_index, direction) {
            (MathNodeKind::Fraction, 0, MathDirection::Down) => 1,
            (MathNodeKind::Fraction, 1, MathDirection::Up) => 0,
            (MathNodeKind::Radical, 1, MathDirection::Up) => 0,
            (MathNodeKind::Radical, 0, MathDirection::Down) => 1,
            _ => continue,
        };
        let target = parent.children.get(target_index)?;
        let mut target_path = parent_path;
        target_path.push(target.id.clone());
        return first_editable_position(
            tree,
            target,
            target_path,
            position.offset,
            direction == MathDirection::Down,
        );
    }
    None
}

fn first_editable_position(
    tree: &MathTree,
    node: &MathNode,
    path: Vec<NodeId>,
    preferred_offset: usize,
    last: bool,
) -> Option<MathPos> {
    match node.kind {
        MathNodeKind::Symbol
            if node.attributes.get("atomClass").map(String::as_str) == Some("ord") =>
        {
            Some(MathPos {
                path,
                offset: nearest_grapheme_offset(
                    node.text.as_deref().unwrap_or_default(),
                    preferred_offset,
                    last,
                ),
            })
        }
        MathNodeKind::TextContent => Some(MathPos {
            path,
            offset: nearest_grapheme_offset(
                node.text.as_deref().unwrap_or_default(),
                preferred_offset,
                last,
            ),
        }),
        MathNodeKind::Placeholder => Some(MathPos { path, offset: 0 }),
        MathNodeKind::Row if node.children.is_empty() => {
            (!is_matrix_row_path(tree, &path)).then_some(MathPos { path, offset: 0 })
        }
        MathNodeKind::Symbol
        | MathNodeKind::Operator
        | MathNodeKind::TextSymbol
        | MathNodeKind::SizedDelimiter
        | MathNodeKind::TextSpace
        | MathNodeKind::Space
        | MathNodeKind::Strut
        | MathNodeKind::Hline => None,
        MathNodeKind::Text | MathNodeKind::TextStyle if node.children.is_empty() => {
            Some(MathPos { path, offset: 0 })
        }
        _ => {
            for step in 0..node.children.len() {
                let index = if last {
                    node.children.len() - step - 1
                } else {
                    step
                };
                let child = &node.children[index];
                let mut child_path = path.clone();
                child_path.push(child.id.clone());
                if let Some(position) =
                    first_editable_position(tree, child, child_path, preferred_offset, last)
                {
                    return Some(position);
                }
            }
            None
        }
    }
}

fn is_matrix_row_path(tree: &MathTree, path: &[NodeId]) -> bool {
    let Some((root_id, descendants)) = path.split_first() else {
        return false;
    };
    if root_id != &tree.root.id {
        return false;
    }
    let mut parent = &tree.root;
    for id in descendants {
        if parent.kind == MathNodeKind::Matrix
            && parent
                .children
                .iter()
                .any(|child| &child.id == id && child.kind == MathNodeKind::Row)
        {
            return true;
        }
        let Some(child) = parent.children.iter().find(|child| &child.id == id) else {
            return false;
        };
        parent = child;
    }
    false
}

fn nearest_grapheme_offset(text: &str, preferred_offset: usize, last: bool) -> usize {
    let mut boundaries = text
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    boundaries.push(text.len());
    if last && preferred_offset > text.len() {
        return text.len();
    }
    boundaries
        .iter()
        .copied()
        .min_by_key(|offset| offset.abs_diff(preferred_offset))
        .unwrap_or(0)
}

fn map_math_edit_error(error: MathEditError) -> EditError {
    match error {
        MathEditError::InvalidPosition => EditError::InvalidPosition,
        MathEditError::UnsupportedTarget => EditError::UnsupportedMathTarget,
        MathEditError::InvalidText => EditError::InvalidMathText,
        MathEditError::InvalidTree => EditError::InvalidMathTree,
    }
}
