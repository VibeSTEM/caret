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
use caret_doc::{
    MathContent, MathFlow, MathSourceRange, MathTree, NodeId, ParagraphContent, Run, TextFlow,
};
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
        let source = caret_math::serialize_tree(&math.math_tree);
        if source.contains(['\n', '\r']) {
            return Err(EditError::UnsupportedNewline);
        }
        let current_tree = self.math_tree_for_edit(&edit, None)?;
        let caret_math::ParsedMath::Tree(projected_tree) =
            caret_math::parse(&source, caret_math::MathInputKind::Inline)
                .map_err(|error| EditError::InvalidMath(error.to_string()))?
        else {
            return Err(EditError::InvalidPosition);
        };
        let (source_anchor, source_cursor) = match &edit.branch {
            MathEditBranch::Source => {
                let cursor = map_source_position(
                    current_tree,
                    &projected_tree,
                    edit.source(),
                    &source,
                    edit.cursor,
                )
                .ok_or(EditError::UnsupportedOpaqueContent)?;
                (cursor, cursor)
            }
            MathEditBranch::Text { path, .. } => {
                if !trees_correspond_for_position_mapping(current_tree, &projected_tree) {
                    return Err(EditError::UnsupportedOpaqueContent);
                }
                path.resolve(current_tree)
                    .ok_or(EditError::InvalidPosition)?;
                let text = path
                    .resolve_structurally(&projected_tree)
                    .ok_or(EditError::UnsupportedOpaqueContent)?;
                let MathEditBranch::Text { anchor, .. } = &edit.branch else {
                    unreachable!()
                };
                let anchor = text_source_boundary(text, *anchor)
                    .ok_or(EditError::UnsupportedOpaqueContent)?;
                let local = text_source_boundary(text, edit.cursor)
                    .ok_or(EditError::UnsupportedOpaqueContent)?;
                (anchor, local)
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

fn map_source_position(
    previous: &MathTree,
    projected: &MathTree,
    previous_source: &str,
    projected_source: &str,
    position: usize,
) -> Option<usize> {
    if position > previous_source.len() || !previous_source.is_char_boundary(position) {
        return None;
    }
    if previous_source == projected_source {
        return Some(position);
    }

    let mut common_prefix = previous_source
        .bytes()
        .zip(projected_source.bytes())
        .take_while(|(left, right)| left == right)
        .count();
    while !previous_source.is_char_boundary(common_prefix)
        || !projected_source.is_char_boundary(common_prefix)
    {
        common_prefix = common_prefix.checked_sub(1)?;
    }
    if position <= common_prefix {
        return Some(position);
    }

    let max_suffix = previous_source
        .len()
        .min(projected_source.len())
        .saturating_sub(common_prefix);
    let mut common_suffix = previous_source
        .bytes()
        .rev()
        .zip(projected_source.bytes().rev())
        .take(max_suffix)
        .take_while(|(left, right)| left == right)
        .count();
    while !previous_source.is_char_boundary(previous_source.len() - common_suffix)
        || !projected_source.is_char_boundary(projected_source.len() - common_suffix)
    {
        common_suffix = common_suffix.checked_sub(1)?;
    }
    if position >= previous_source.len() - common_suffix {
        return projected_source
            .len()
            .checked_sub(previous_source.len() - position);
    }

    if !trees_correspond_for_position_mapping(previous, projected) {
        return None;
    }
    let mut anchors = Vec::new();
    collect_flow_anchors(&previous.root, &projected.root, &mut anchors);
    let mut matches = anchors
        .into_iter()
        .filter_map(|(source, destination)| (source == position).then_some(destination));
    let destination = matches.next()?;
    matches
        .all(|candidate| candidate == destination)
        .then_some(destination)
}

fn trees_correspond_for_position_mapping(previous: &MathTree, projected: &MathTree) -> bool {
    previous.extra == projected.extra
        && flows_correspond_for_position_mapping(&previous.root, &projected.root)
}

fn flows_correspond_for_position_mapping(previous: &MathFlow, projected: &MathFlow) -> bool {
    previous.extra == projected.extra
        && previous.content.len() == projected.content.len()
        && previous
            .content
            .iter()
            .zip(&projected.content)
            .all(|(previous, projected)| {
                contents_correspond_for_position_mapping(previous, projected)
            })
}

fn contents_correspond_for_position_mapping(
    previous: &MathContent,
    projected: &MathContent,
) -> bool {
    match (previous, projected) {
        (MathContent::Text(previous), MathContent::Text(projected)) => {
            previous.runs == projected.runs && previous.extra == projected.extra
        }
        (MathContent::Math(previous), MathContent::Math(projected)) => {
            previous.kind == projected.kind
                && previous.text == projected.text
                && previous.attributes == projected.attributes
                && previous.extra == projected.extra
                && previous.children.len() == projected.children.len()
                && previous
                    .children
                    .iter()
                    .zip(&projected.children)
                    .all(|(previous, projected)| {
                        flows_correspond_for_position_mapping(previous, projected)
                    })
        }
        (MathContent::Environment(previous), MathContent::Environment(projected)) => {
            previous.kind == projected.kind
                && previous.definition_id == projected.definition_id
                && previous.props == projected.props
                && previous.extra == projected.extra
                && previous.children.len() == projected.children.len()
                && previous
                    .children
                    .iter()
                    .zip(&projected.children)
                    .all(|(previous, projected)| {
                        contents_correspond_for_position_mapping(previous, projected)
                    })
        }
        (MathContent::SoftBreak(previous), MathContent::SoftBreak(projected)) => {
            previous.extra == projected.extra
        }
        (MathContent::Opaque(previous), MathContent::Opaque(projected)) => previous == projected,
        _ => false,
    }
}

fn collect_flow_anchors(
    previous: &MathFlow,
    projected: &MathFlow,
    output: &mut Vec<(usize, usize)>,
) {
    push_range_anchors(
        previous.source_range.as_ref(),
        projected.source_range.as_ref(),
        output,
    );
    for (previous, projected) in previous.content.iter().zip(&projected.content) {
        match (previous, projected) {
            (MathContent::Text(previous), MathContent::Text(projected)) => {
                for (previous, projected) in previous.source_map.iter().zip(&projected.source_map) {
                    push_range_anchors(
                        Some(&previous.source_range),
                        Some(&projected.source_range),
                        output,
                    );
                }
            }
            (MathContent::Math(previous), MathContent::Math(projected)) => {
                push_range_anchors(
                    previous.source_range.as_ref(),
                    projected.source_range.as_ref(),
                    output,
                );
                for (previous, projected) in previous.children.iter().zip(&projected.children) {
                    collect_flow_anchors(previous, projected, output);
                }
            }
            (MathContent::Environment(previous), MathContent::Environment(projected)) => {
                push_range_anchors(
                    previous.source_range.as_ref(),
                    projected.source_range.as_ref(),
                    output,
                );
                for (previous, projected) in previous.children.iter().zip(&projected.children) {
                    collect_content_anchors(previous, projected, output);
                }
            }
            _ => {}
        }
    }
}

fn collect_content_anchors(
    previous: &MathContent,
    projected: &MathContent,
    output: &mut Vec<(usize, usize)>,
) {
    match (previous, projected) {
        (MathContent::Text(previous), MathContent::Text(projected)) => {
            for (previous, projected) in previous.source_map.iter().zip(&projected.source_map) {
                push_range_anchors(
                    Some(&previous.source_range),
                    Some(&projected.source_range),
                    output,
                );
            }
        }
        (MathContent::Math(previous), MathContent::Math(projected)) => {
            push_range_anchors(
                previous.source_range.as_ref(),
                projected.source_range.as_ref(),
                output,
            );
            for (previous, projected) in previous.children.iter().zip(&projected.children) {
                collect_flow_anchors(previous, projected, output);
            }
        }
        (MathContent::Environment(previous), MathContent::Environment(projected)) => {
            push_range_anchors(
                previous.source_range.as_ref(),
                projected.source_range.as_ref(),
                output,
            );
            for (previous, projected) in previous.children.iter().zip(&projected.children) {
                collect_content_anchors(previous, projected, output);
            }
        }
        _ => {}
    }
}

fn push_range_anchors(
    previous: Option<&MathSourceRange>,
    projected: Option<&MathSourceRange>,
    output: &mut Vec<(usize, usize)>,
) {
    let (Some(previous), Some(projected)) = (previous, projected) else {
        return;
    };
    output.push((previous.start as usize, projected.start as usize));
    output.push((previous.end as usize, projected.end as usize));
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

#[cfg(test)]
mod tests {
    use super::{map_source_position, trees_correspond_for_position_mapping};
    use caret_math::{MathInputKind, ParsedMath, parse, serialize_tree};

    fn parse_tree(source: &str) -> Result<caret_doc::MathTree, Box<dyn std::error::Error>> {
        let ParsedMath::Tree(tree) = parse(source, MathInputKind::Inline)? else {
            return Err("expected inline math tree".into());
        };
        Ok(tree)
    }

    #[test]
    fn source_demotion_maps_exact_positions_and_unique_tree_boundaries()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = r"\frac{a}{b}";
        let original = parse_tree(source)?;
        let canonical = serialize_tree(&original);
        let projected = parse_tree(&canonical)?;
        assert_eq!(
            map_source_position(&original, &projected, source, &canonical, 5),
            Some(5)
        );

        let source = "abc";
        let original = parse_tree(source)?;
        let canonical = serialize_tree(&original);
        let projected = parse_tree(&canonical)?;
        assert_eq!(
            map_source_position(&original, &projected, source, &canonical, 2),
            Some(2)
        );

        let source = "x ";
        let original = parse_tree(source)?;
        let canonical = serialize_tree(&original);
        let projected = parse_tree(&canonical)?;
        assert_eq!(canonical, "x");
        assert_eq!(
            map_source_position(&original, &projected, source, &canonical, 2),
            Some(1)
        );

        let source = "a + b";
        let original = parse_tree(source)?;
        let canonical = serialize_tree(&original);
        let projected = parse_tree(&canonical)?;
        assert_ne!(source, canonical);
        assert_eq!(
            map_source_position(&original, &projected, source, &canonical, 3),
            Some(2)
        );
        Ok(())
    }

    #[test]
    fn position_mapping_rejects_text_flows_merged_by_canonical_serialization()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = r"\text{a}\text{b}\alpha\text{d}\text{e}";
        let original = parse_tree(source)?;
        let canonical = serialize_tree(&original);
        let projected = parse_tree(&canonical)?;
        assert_ne!(canonical, source);
        assert!(!trees_correspond_for_position_mapping(
            &original, &projected
        ));
        let position = source.find("\\alpha").ok_or("missing middle atom")?;
        assert_eq!(
            map_source_position(&original, &projected, source, &canonical, position),
            None
        );
        Ok(())
    }
}
