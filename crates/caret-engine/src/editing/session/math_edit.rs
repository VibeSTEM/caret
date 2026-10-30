//! 将行内公式创建与数学树编辑纳入 Session 文档事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::paragraph_ops::paragraph_at_location_mut;
use super::super::text_map::TextMap;
use super::super::{Affinity, EditError, Effect, MathFocus, MathPos, Pos, PositionMap, Selection};
use super::Session;
use crate::apply_math_edit;
use crate::math_edit::{MathEdit, MathEditError};
use caret_doc::{
    InlineMath, InlineNode, MATH_TREE_SCHEMA_VERSION, MathNode, MathNodeKind, MathTree, NodeId,
};
use caret_layout::VisualAffinity;
use std::collections::BTreeMap;

impl Session {
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
        let inline_math = find_inline_math(&mut paragraph.inlines, &focus.inline_math_id)
            .ok_or(EditError::InvalidPosition)?;
        let target_kind = focus
            .position
            .resolve(&inline_math.math_tree)
            .map(|node| node.kind)
            .ok_or(EditError::InvalidPosition)?;
        let increases_document_node_count = match &edit {
            MathEdit::InsertText(_) => matches!(
                target_kind,
                MathNodeKind::Row | MathNodeKind::Text | MathNodeKind::TextStyle
            ),
            MathEdit::InsertFraction | MathEdit::InsertRadical => true,
        };
        let result = apply_math_edit(&inline_math.math_tree, &focus.position, edit)
            .map_err(map_math_edit_error)?;
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
        if self.math_focus.take().is_none() {
            return Ok(Effect::default());
        }
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

fn find_inline_math<'a>(nodes: &'a mut [InlineNode], id: &NodeId) -> Option<&'a mut InlineMath> {
    for node in nodes {
        match node {
            InlineNode::Math(math) if &math.id == id => return Some(math),
            InlineNode::Environment(environment) => {
                if let Some(math) = find_inline_math(&mut environment.children, id) {
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

fn map_math_edit_error(error: MathEditError) -> EditError {
    match error {
        MathEditError::InvalidPosition => EditError::InvalidPosition,
        MathEditError::UnsupportedTarget => EditError::UnsupportedMathTarget,
        MathEditError::InvalidText => EditError::InvalidMathText,
        MathEditError::InvalidTree => EditError::InvalidMathTree,
    }
}
