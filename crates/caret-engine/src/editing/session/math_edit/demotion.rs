//! 行内公式源码降级为正文文本的事务与映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::text_map::{OBJECT_REPLACEMENT, TextMap};
use super::super::history::{self, DocumentPatch, MathEditPosition};
use super::super::{Affinity, EditError, Effect, Pos, PositionMap, Selection};
use super::metadata::can_materialize_source_as_text;
use super::text_branch::text_source_boundary;
use super::{MathEditBranch, MathEditTarget, Session, find_inline_math, inline_math_slots};
use crate::VisualAffinity;
use caret_doc::{NodeId, ParagraphContent, Run, TextFlow};
use std::collections::BTreeMap;

impl Session {
    /// 将行内公式原子降级为源码文本，并以单次历史事务保留可撤销性。
    pub(super) fn math_mod_backspace(&mut self) -> Result<Effect, EditError> {
        let Some(edit) = self.math_edit.clone() else {
            return Ok(Effect::default());
        };
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Ok(Effect::default());
        };
        let position = self.selection.focus.clone();
        if &position.paragraph_id != paragraph_id {
            return Ok(Effect::default());
        }
        let mut paragraph = self.paragraph_for(&position)?.clone();
        let math =
            find_inline_math(&paragraph.inlines, math_id).ok_or(EditError::InvalidPosition)?;
        if !math.extra.is_empty() || !can_materialize_source_as_text(&math.math_tree) {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let source = math.math_tree.source.clone();
        if source.contains(['\n', '\r']) {
            return Err(EditError::UnsupportedNewline);
        }
        let (source_anchor, source_cursor) = match &edit.branch {
            MathEditBranch::Source => (edit.cursor, edit.cursor),
            MathEditBranch::Text { path, .. } => {
                let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
                let text = path.resolve(tree).ok_or(EditError::InvalidPosition)?;
                let MathEditBranch::Text { anchor, .. } = &edit.branch else {
                    unreachable!()
                };
                let anchor = text_source_boundary(text, *anchor)
                    .and_then(|offset| path.host_offset(offset))
                    .ok_or(EditError::UnsupportedOpaqueContent)?;
                let local = text_source_boundary(text, edit.cursor)
                    .ok_or(EditError::UnsupportedOpaqueContent)?;
                let cursor = path.host_offset(local).ok_or(EditError::InvalidPosition)?;
                (anchor, cursor)
            }
        };
        if source_anchor > source.len()
            || source_cursor > source.len()
            || !source.is_char_boundary(source_anchor)
            || !source.is_char_boundary(source_cursor)
        {
            return Err(EditError::InvalidPosition);
        }
        let start = inline_math_slots(&paragraph)?
            .into_iter()
            .find(|(_, id)| id == math_id)
            .map(|(start, _)| start)
            .ok_or(EditError::InvalidPosition)?;
        let replacement = TextFlow {
            runs: vec![Run {
                text: source.clone(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            source_range: None,
            source_map: Vec::new(),
            extra: BTreeMap::new(),
        };
        if !replace_inline_math_with_text(&mut paragraph.inlines, math_id, replacement) {
            return Err(EditError::InvalidPosition);
        }
        let result_map = TextMap::new(&paragraph)?;
        let result_boundaries = result_map.grapheme_boundaries.clone();
        let collapsed = source_anchor == source_cursor;
        if !collapsed
            && (!result_boundaries.contains(&(start + source_anchor))
                || !result_boundaries.contains(&(start + source_cursor)))
        {
            return Err(EditError::InvalidPosition);
        }
        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let before_pending_style = self.pending_inline_style.clone();
        let before_states = history::paragraph_states_for_ids(
            &self.document,
            &self.paragraph_index,
            [paragraph_id.clone()],
        );
        let before_math =
            MathEditPosition::with_branch(edit.target.clone(), edit.cursor, edit.branch.clone());
        let end = start + OBJECT_REPLACEMENT.len_utf8();
        self.replace_paragraph(&position, paragraph)?;
        self.text_map_cache.clear();
        let anchor_affinity = if collapsed {
            Affinity::Downstream
        } else if source_anchor < source_cursor {
            Affinity::Upstream
        } else {
            Affinity::Downstream
        };
        let focus_affinity = if source_anchor < source_cursor {
            Affinity::Downstream
        } else {
            Affinity::Upstream
        };
        let anchor_offset =
            result_map.snap_to_grapheme_boundary(start + source_anchor, anchor_affinity);
        let anchor = Pos {
            offset: anchor_offset,
            affinity: anchor_affinity,
            ..position.clone()
        };
        let focus = if source_anchor == source_cursor {
            anchor.clone()
        } else {
            Pos {
                offset: result_map.snap_to_grapheme_boundary(start + source_cursor, focus_affinity),
                affinity: focus_affinity,
                ..position
            }
        };
        let visual_affinity = match focus.affinity {
            Affinity::Upstream => VisualAffinity::Upstream,
            Affinity::Downstream => VisualAffinity::Downstream,
        };
        self.selection = Selection { anchor, focus };
        self.selection_visual_affinity = visual_affinity;
        self.pending_inline_style.clear();
        self.math_edit = None;
        let after_states = history::paragraph_states_for_ids(
            &self.document,
            &self.paragraph_index,
            [paragraph_id.clone()],
        );
        self.record_math_history_with_pending_style(
            DocumentPatch::Paragraphs {
                before: before_states,
                after: after_states,
                merge_group: None,
                merge_epoch: self.history_merge_epoch,
            },
            (before_selection.clone(), before_visual_affinity),
            (self.selection.clone(), self.selection_visual_affinity),
            Some(before_math.clone()),
            None,
            before_pending_style,
        );
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            changed_paragraph_id: Some(paragraph_id.clone()),
            position_map: Some(PositionMap::TextEdit {
                paragraph_id: paragraph_id.clone(),
                start,
                end,
                inserted_len: source.len(),
                snap_graphemes: true,
                result_boundaries,
            }),
            ..Effect::default()
        })
    }
}

fn replace_inline_math_with_text(
    contents: &mut [ParagraphContent],
    target_id: &NodeId,
    replacement: TextFlow,
) -> bool {
    let mut replacement = Some(replacement);
    replace_inline_math_with_text_in_place(contents, target_id, &mut replacement)
}

fn replace_inline_math_with_text_in_place(
    contents: &mut [ParagraphContent],
    target_id: &NodeId,
    replacement: &mut Option<TextFlow>,
) -> bool {
    for content in contents {
        if matches!(content, ParagraphContent::Math(math) if &math.id == target_id) {
            let Some(replacement) = replacement.take() else {
                return false;
            };
            *content = ParagraphContent::Text(replacement);
            return true;
        }
        if let ParagraphContent::Environment(environment) = content
            && replace_inline_math_with_text_in_place(
                &mut environment.children,
                target_id,
                replacement,
            )
        {
            return true;
        }
    }
    false
}
