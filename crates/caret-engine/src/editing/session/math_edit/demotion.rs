//! 行内公式源码降级为正文文本的事务与映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::text_map::{OBJECT_REPLACEMENT, TextMap};
use super::super::history::{self, DocumentPatch, MathEditPosition};
use super::super::{Affinity, EditError, Effect, Pos, PositionMap, Selection};
use super::metadata::can_materialize_source_as_text;
use super::text_branch::text_source_boundary_from_map;
use super::{
    MathEditBranch, MathEditTarget, MathPositionAffinity, MathSequenceStep, MathTreePosition,
    Session, find_inline_math, inline_math_slots,
};
use crate::VisualAffinity;
use caret_doc::{MathContent, MathFlow, MathTree, NodeId, ParagraphContent, Run, TextFlow};
use caret_math::{MathContentSourceMap, MathFlowSourceMap, MathTreeSourceMap};
use std::collections::BTreeMap;

impl Session {
    /// 将行内公式原子降级为源码文本，并以单次历史事务保留可撤销性。
    pub(in crate::editing::session) fn math_mod_backspace(&mut self) -> Result<Effect, EditError> {
        let before_document = self.document.clone();
        let before_edit = self.math_edit.clone();
        let result = self.math_mod_backspace_transaction();
        if result.is_err() {
            self.document = before_document;
            self.math_edit = before_edit;
        }
        result
    }

    fn math_mod_backspace_transaction(&mut self) -> Result<Effect, EditError> {
        let Some(edit) = self.math_edit.clone() else {
            return Ok(Effect::default());
        };
        if !matches!(&edit.target, MathEditTarget::Inline { .. }) {
            return Ok(Effect::default());
        }
        if self.math_edit_is_text_branch() {
            self.refresh_math_text_projection()?;
        }
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
        let edit_source = edit.source().unwrap_or(&source);
        if source.contains(['\n', '\r']) {
            return Err(EditError::UnsupportedNewline);
        }
        let current_tree = self.math_tree_for_edit(&edit, None)?;
        let (caret_math::ProjectedMath::Tree(projected_tree), projected_source_map) =
            caret_math::parse_projection(&source, caret_math::MathInputKind::Inline)
                .map_err(|error| EditError::InvalidMath(error.to_string()))?
                .into_parts()
        else {
            return Err(EditError::InvalidPosition);
        };
        let reconciled_tree_map = if matches!(edit.branch, MathEditBranch::Tree { .. }) {
            let parsed_map = projected_source_map
                .tree()
                .ok_or(EditError::InvalidPosition)?;
            Some(
                parsed_map
                    .reconcile_for_tree(&projected_tree, current_tree, None, &source, None)
                    .ok_or(EditError::UnsupportedOpaqueContent)?,
            )
        } else {
            None
        };
        let previous_map = match &edit.branch {
            MathEditBranch::Tree { .. } => reconciled_tree_map
                .as_ref()
                .ok_or(EditError::InvalidPosition)?,
            MathEditBranch::Source | MathEditBranch::Text { .. } => {
                let previous_projection = edit.required_source_projection()?;
                if previous_projection.edit_source() != edit_source {
                    return Err(EditError::InvalidPosition);
                }
                previous_projection
                    .tree()
                    .ok_or(EditError::InvalidPosition)?
            }
        };
        let previous = TreeProjection {
            tree: current_tree,
            map: previous_map,
            source: edit_source,
        };
        if !source_map_can_materialize_as_text(previous.map.root()) {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let projected = TreeProjection {
            tree: &projected_tree,
            map: projected_source_map
                .tree()
                .ok_or(EditError::InvalidPosition)?,
            source: &source,
        };
        let (source_anchor, source_cursor) = match &edit.branch {
            MathEditBranch::Source => {
                let cursor = map_source_position(previous, projected, edit.cursor)
                    .ok_or(EditError::UnsupportedOpaqueContent)?;
                (cursor, cursor)
            }
            MathEditBranch::Text {
                path,
                anchor,
                focus_affinity,
                ..
            } => {
                if !trees_correspond_for_position_mapping(current_tree, &projected_tree) {
                    return Err(EditError::UnsupportedOpaqueContent);
                }
                let Some((anchor_text, anchor_map)) = anchor
                    .path
                    .resolve_in_projection(current_tree, edit.required_source_projection()?)
                else {
                    return Err(EditError::InvalidPosition);
                };
                let Some((focus_text, focus_map)) =
                    path.resolve_in_projection(current_tree, edit.required_source_projection()?)
                else {
                    return Err(EditError::InvalidPosition);
                };
                let source_base = edit.required_source_projection()?.edit_range().start;
                let Some(source_anchor) = text_source_boundary_from_map(
                    anchor_text,
                    anchor_map,
                    source_base,
                    anchor.offset,
                    match anchor.affinity {
                        MathPositionAffinity::Upstream => Affinity::Upstream,
                        MathPositionAffinity::Downstream => Affinity::Downstream,
                    },
                ) else {
                    return Err(EditError::UnsupportedOpaqueContent);
                };
                let Some(source_cursor) = text_source_boundary_from_map(
                    focus_text,
                    focus_map,
                    source_base,
                    edit.cursor,
                    match focus_affinity {
                        MathPositionAffinity::Upstream => Affinity::Upstream,
                        MathPositionAffinity::Downstream => Affinity::Downstream,
                    },
                ) else {
                    return Err(EditError::UnsupportedOpaqueContent);
                };
                let mapped_anchor = map_source_position(previous, projected, source_anchor);
                let mapped_cursor = map_source_position(previous, projected, source_cursor);
                (
                    mapped_anchor.ok_or(EditError::UnsupportedOpaqueContent)?,
                    mapped_cursor.ok_or(EditError::UnsupportedOpaqueContent)?,
                )
            }
            MathEditBranch::Tree { path, anchor } => {
                if anchor.path != *path
                    || !trees_correspond_for_position_mapping(current_tree, &projected_tree)
                {
                    return Err(EditError::UnsupportedOpaqueContent);
                }
                let mut focus = anchor.clone();
                focus.offset = edit.cursor;
                let source_anchor =
                    tree_position_source_boundary(current_tree, previous.map, anchor, &source)
                        .ok_or(EditError::UnsupportedOpaqueContent)?;
                let source_cursor =
                    tree_position_source_boundary(current_tree, previous.map, &focus, &source)
                        .ok_or(EditError::UnsupportedOpaqueContent)?;
                (source_anchor, source_cursor)
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
        let before_math = MathEditPosition::from_edit(&edit);
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

#[derive(Clone, Copy)]
struct TreeProjection<'a> {
    tree: &'a MathTree,
    map: &'a MathTreeSourceMap,
    source: &'a str,
}

fn source_map_can_materialize_as_text(flow: &MathFlowSourceMap) -> bool {
    flow.range_extra().is_empty()
        && flow
            .content()
            .iter()
            .all(source_content_map_can_materialize_as_text)
}

fn source_content_map_can_materialize_as_text(content: &MathContentSourceMap) -> bool {
    match content {
        MathContentSourceMap::Text(text) => {
            text.range_extra().is_empty()
                && text
                    .segments()
                    .iter()
                    .all(|segment| segment.extra().is_empty() && segment.source_extra().is_empty())
        }
        MathContentSourceMap::Math {
            range, operands, ..
        } => range.extra().is_empty() && operands.iter().all(source_map_can_materialize_as_text),
        MathContentSourceMap::Environment { range, children } => {
            range.extra().is_empty()
                && children
                    .iter()
                    .all(source_content_map_can_materialize_as_text)
        }
        MathContentSourceMap::Other => true,
    }
}

fn map_source_position(
    previous: TreeProjection<'_>,
    projected: TreeProjection<'_>,
    position: usize,
) -> Option<usize> {
    let previous_source = previous.source;
    let projected_source = projected.source;
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

    if !trees_correspond_for_position_mapping(previous.tree, projected.tree) {
        return None;
    }
    if let Some(mapped) = map_text_source_position(previous, projected, position) {
        return Some(mapped);
    }
    let mut anchors = Vec::new();
    collect_flow_anchors(
        &previous.tree.root,
        previous.map.root(),
        &projected.tree.root,
        projected.map.root(),
        &mut anchors,
    )?;
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

fn tree_position_source_boundary(
    tree: &MathTree,
    map: &MathTreeSourceMap,
    position: &MathTreePosition,
    source: &str,
) -> Option<usize> {
    let mut contents = tree.root.content.as_slice();
    let mut content_maps = map.root().content();
    let mut flow_range = map.root().range();

    for step in position.path.steps() {
        let (child_contents, child_maps, child_range) = match *step {
            MathSequenceStep::Operand {
                content_index,
                operand_index,
            } => {
                let MathContent::Math(node) = contents.get(content_index)? else {
                    return None;
                };
                let MathContentSourceMap::Math { operands, .. } =
                    content_maps.get(content_index)?
                else {
                    return None;
                };
                let child_flow = node.children.get(operand_index)?;
                let child_map = operands.get(operand_index)?;
                (
                    child_flow.content.as_slice(),
                    child_map.content(),
                    child_map.range(),
                )
            }
            MathSequenceStep::EnvironmentChildren { content_index } => {
                let MathContent::Environment(environment) = contents.get(content_index)? else {
                    return None;
                };
                let MathContentSourceMap::Environment { children, range } =
                    content_maps.get(content_index)?
                else {
                    return None;
                };
                (
                    environment.children.as_slice(),
                    children.as_slice(),
                    range.range(),
                )
            }
        };
        contents = child_contents;
        content_maps = child_maps;
        flow_range = child_range;
    }

    let contents = position.resolve(tree).ok()?;
    if contents.len() != content_maps.len() || position.offset > contents.len() {
        return None;
    }
    let previous = position
        .offset
        .checked_sub(1)
        .and_then(|index| content_maps.get(index))
        .and_then(MathContentSourceMap::range);
    let next = content_maps
        .get(position.offset)
        .and_then(MathContentSourceMap::range);
    let boundary = match position.affinity {
        MathPositionAffinity::Downstream => next
            .map(|range| range.start)
            .or_else(|| previous.map(|range| range.end)),
        MathPositionAffinity::Upstream => previous
            .map(|range| range.end)
            .or_else(|| next.map(|range| range.start)),
    }
    .or_else(|| {
        flow_range.map(|range| {
            if position.offset == 0 {
                range.start
            } else {
                range.end
            }
        })
    })
    .or_else(|| (source.is_empty() && contents.is_empty()).then_some(0))?;
    (boundary <= source.len() && source.is_char_boundary(boundary)).then_some(boundary)
}

fn flows_correspond_for_position_mapping(previous: &MathFlow, projected: &MathFlow) -> bool {
    previous.extra == projected.extra
        && content_sequences_correspond_for_position_mapping(&previous.content, &projected.content)
}

fn content_sequences_correspond_for_position_mapping(
    previous: &[MathContent],
    projected: &[MathContent],
) -> bool {
    let mut previous_index = 0;
    let mut projected_index = 0;
    while previous_index < previous.len() && projected_index < projected.len() {
        if matches!(previous[previous_index], MathContent::Text(_))
            && matches!(projected[projected_index], MathContent::Text(_))
        {
            let previous_end = text_group_end(previous, previous_index);
            let projected_end = text_group_end(projected, projected_index);
            if !text_groups_correspond(
                &previous[previous_index..previous_end],
                &projected[projected_index..projected_end],
            ) {
                return false;
            }
            previous_index = previous_end;
            projected_index = projected_end;
            continue;
        }
        if !contents_correspond_for_position_mapping(
            &previous[previous_index],
            &projected[projected_index],
        ) {
            return false;
        }
        previous_index += 1;
        projected_index += 1;
    }
    previous_index == previous.len() && projected_index == projected.len()
}

fn text_group_end(contents: &[MathContent], start: usize) -> usize {
    start
        + contents[start..]
            .iter()
            .take_while(|content| matches!(content, MathContent::Text(_)))
            .count()
}

fn text_groups_correspond(previous: &[MathContent], projected: &[MathContent]) -> bool {
    if previous.len() == 1 && projected.len() == 1 {
        return contents_correspond_for_position_mapping(&previous[0], &projected[0]);
    }
    normalized_text_group_runs(previous).is_some_and(|previous_runs| {
        normalized_text_group_runs(projected).is_some_and(|projected_runs| {
            previous_runs == projected_runs
                && text_group_extra(previous) == text_group_extra(projected)
        })
    })
}

fn normalized_text_group_runs(contents: &[MathContent]) -> Option<Vec<Run>> {
    let mut runs: Vec<Run> = Vec::new();
    for content in contents {
        let MathContent::Text(flow) = content else {
            return None;
        };
        for run in &flow.runs {
            if let Some(previous) = runs.last_mut()
                && previous.language == run.language
                && previous.props == run.props
                && previous.extra == run.extra
            {
                previous.text.push_str(&run.text);
            } else {
                runs.push(run.clone());
            }
        }
    }
    Some(runs)
}

fn text_group_extra(contents: &[MathContent]) -> Option<&BTreeMap<String, serde_json::Value>> {
    let mut extra = None;
    for content in contents {
        let MathContent::Text(flow) = content else {
            return None;
        };
        if extra.is_some_and(|previous| previous != &flow.extra) {
            return None;
        }
        extra = Some(&flow.extra);
    }
    extra
}

fn map_text_source_position(
    previous: TreeProjection<'_>,
    projected: TreeProjection<'_>,
    source_position: usize,
) -> Option<usize> {
    map_text_position_in_contents(
        &previous.tree.root.content,
        previous.map.root().content(),
        &projected.tree.root.content,
        projected.map.root().content(),
        source_position,
    )
}

fn map_text_position_in_contents(
    previous: &[MathContent],
    previous_maps: &[MathContentSourceMap],
    projected: &[MathContent],
    projected_maps: &[MathContentSourceMap],
    source_position: usize,
) -> Option<usize> {
    if previous.len() != previous_maps.len() || projected.len() != projected_maps.len() {
        return None;
    }
    let mut previous_index = 0;
    let mut projected_index = 0;
    while previous_index < previous.len() && projected_index < projected.len() {
        if matches!(previous[previous_index], MathContent::Text(_))
            && matches!(projected[projected_index], MathContent::Text(_))
        {
            let previous_end = text_group_end(previous, previous_index);
            let projected_end = text_group_end(projected, projected_index);
            if let Some(display_position) = text_group_source_to_display(
                &previous[previous_index..previous_end],
                &previous_maps[previous_index..previous_end],
                source_position,
            ) {
                return text_group_display_to_source(
                    &projected[projected_index..projected_end],
                    &projected_maps[projected_index..projected_end],
                    display_position,
                );
            }
            previous_index = previous_end;
            projected_index = projected_end;
            continue;
        }
        let previous_map = &previous_maps[previous_index];
        let projected_map = &projected_maps[projected_index];
        let mapped = match (&previous[previous_index], &projected[projected_index]) {
            (MathContent::Math(previous), MathContent::Math(projected)) => {
                let previous_operands = previous_map.operands()?;
                let projected_operands = projected_map.operands()?;
                if previous.children.len() != previous_operands.len()
                    || projected.children.len() != projected_operands.len()
                {
                    return None;
                }
                previous
                    .children
                    .iter()
                    .zip(previous_operands)
                    .zip(projected.children.iter().zip(projected_operands))
                    .find_map(|((previous, previous_map), (projected, projected_map))| {
                        map_text_position_in_contents(
                            &previous.content,
                            previous_map.content(),
                            &projected.content,
                            projected_map.content(),
                            source_position,
                        )
                    })
            }
            (MathContent::Environment(previous), MathContent::Environment(projected)) => {
                map_text_position_in_contents(
                    &previous.children,
                    previous_map.children()?,
                    &projected.children,
                    projected_map.children()?,
                    source_position,
                )
            }
            _ => None,
        };
        if mapped.is_some() {
            return mapped;
        }
        previous_index += 1;
        projected_index += 1;
    }
    None
}

fn text_group_source_to_display(
    contents: &[MathContent],
    maps: &[MathContentSourceMap],
    source_position: usize,
) -> Option<usize> {
    if contents.len() != maps.len() {
        return None;
    }
    let mut display_base = 0usize;
    let mut candidates = Vec::new();
    for (content, map) in contents.iter().zip(maps) {
        let MathContent::Text(flow) = content else {
            return None;
        };
        let map = map.text()?;
        let text_len = flow
            .runs
            .iter()
            .try_fold(0usize, |length, run| length.checked_add(run.text.len()))?;
        if let Some(range) = map.range() {
            if source_position == range.start {
                candidates.push(display_base);
            }
            if source_position == range.end {
                candidates.push(display_base.checked_add(text_len)?);
            }
        }
        for segment in map.segments() {
            let display = segment.display();
            let source = segment.source()?;
            let display_start = display_base.checked_add(display.start)?;
            let display_end = display_base.checked_add(display.end)?;
            if source_position == source.start {
                candidates.push(display_start);
            } else if source_position == source.end {
                candidates.push(display_end);
            } else if display_end.checked_sub(display_start)?
                == source.end.checked_sub(source.start)?
                && source.start < source_position
                && source_position < source.end
            {
                candidates.push(display_start.checked_add(source_position - source.start)?);
            }
        }
        display_base = display_base.checked_add(text_len)?;
    }
    unique_position(candidates)
}

fn text_group_display_to_source(
    contents: &[MathContent],
    maps: &[MathContentSourceMap],
    display_position: usize,
) -> Option<usize> {
    if contents.len() != maps.len() {
        return None;
    }
    let mut display_base = 0usize;
    let mut candidates = Vec::new();
    for (content, map) in contents.iter().zip(maps) {
        let MathContent::Text(flow) = content else {
            return None;
        };
        let map = map.text()?;
        let text_len = flow
            .runs
            .iter()
            .try_fold(0usize, |length, run| length.checked_add(run.text.len()))?;
        let group_display_end = display_base.checked_add(text_len)?;
        if let Some(range) = map.range() {
            if display_position == display_base {
                candidates.push(range.start);
            }
            if display_position == group_display_end {
                candidates.push(range.end);
            }
        }
        for segment in map.segments() {
            let display = segment.display();
            let source = segment.source()?;
            let display_start = display_base.checked_add(display.start)?;
            let display_end = display_base.checked_add(display.end)?;
            if display_position == display_start {
                candidates.push(source.start);
            } else if display_position == display_end {
                candidates.push(source.end);
            } else if display_end.checked_sub(display_start)?
                == source.end.checked_sub(source.start)?
                && display_start < display_position
                && display_position < display_end
            {
                candidates.push(source.start.checked_add(display_position - display_start)?);
            }
        }
        display_base = group_display_end;
    }
    unique_position(candidates)
}

fn unique_position(mut candidates: Vec<usize>) -> Option<usize> {
    candidates.sort_unstable();
    candidates.dedup();
    (candidates.len() == 1).then(|| candidates[0])
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
                && content_sequences_correspond_for_position_mapping(
                    &previous.children,
                    &projected.children,
                )
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
    previous_map: &MathFlowSourceMap,
    projected: &MathFlow,
    projected_map: &MathFlowSourceMap,
    output: &mut Vec<(usize, usize)>,
) -> Option<()> {
    push_range_anchors(previous_map.range(), projected_map.range(), output);
    collect_content_anchors(
        &previous.content,
        previous_map.content(),
        &projected.content,
        projected_map.content(),
        output,
    )
}

fn collect_content_anchors(
    previous: &[MathContent],
    previous_maps: &[MathContentSourceMap],
    projected: &[MathContent],
    projected_maps: &[MathContentSourceMap],
    output: &mut Vec<(usize, usize)>,
) -> Option<()> {
    if previous.len() != previous_maps.len() || projected.len() != projected_maps.len() {
        return None;
    }
    let mut previous_index = 0;
    let mut projected_index = 0;
    while previous_index < previous.len() && projected_index < projected.len() {
        if matches!(previous[previous_index], MathContent::Text(_))
            && matches!(projected[projected_index], MathContent::Text(_))
        {
            let previous_end = text_group_end(previous, previous_index);
            let projected_end = text_group_end(projected, projected_index);
            push_text_group_range_anchors(
                &previous_maps[previous_index..previous_end],
                &projected_maps[projected_index..projected_end],
                output,
            )?;
            previous_index = previous_end;
            projected_index = projected_end;
            continue;
        }
        let previous_map = &previous_maps[previous_index];
        let projected_map = &projected_maps[projected_index];
        match (&previous[previous_index], &projected[projected_index]) {
            (MathContent::Math(previous), MathContent::Math(projected)) => {
                let previous_operands = previous_map.operands()?;
                let projected_operands = projected_map.operands()?;
                if previous.children.len() != previous_operands.len()
                    || projected.children.len() != projected_operands.len()
                {
                    return None;
                }
                push_range_anchors(previous_map.range(), projected_map.range(), output);
                for ((previous, previous_map), (projected, projected_map)) in previous
                    .children
                    .iter()
                    .zip(previous_operands)
                    .zip(projected.children.iter().zip(projected_operands))
                {
                    collect_flow_anchors(previous, previous_map, projected, projected_map, output)?;
                }
            }
            (MathContent::Environment(previous), MathContent::Environment(projected)) => {
                push_range_anchors(previous_map.range(), projected_map.range(), output);
                collect_content_anchors(
                    &previous.children,
                    previous_map.children()?,
                    &projected.children,
                    projected_map.children()?,
                    output,
                )?;
            }
            _ => {}
        }
        previous_index += 1;
        projected_index += 1;
    }
    (previous_index == previous.len() && projected_index == projected.len()).then_some(())
}

fn push_text_group_range_anchors(
    previous: &[MathContentSourceMap],
    projected: &[MathContentSourceMap],
    output: &mut Vec<(usize, usize)>,
) -> Option<()> {
    if previous.iter().any(|content| content.text().is_none())
        || projected.iter().any(|content| content.text().is_none())
    {
        return None;
    }
    let previous_start = previous.first()?.range()?;
    let projected_start = projected.first()?.range()?;
    let previous_end = previous.last()?.range()?;
    let projected_end = projected.last()?.range()?;
    output.push((previous_start.start, projected_start.start));
    output.push((previous_end.end, projected_end.end));
    Some(())
}

fn push_range_anchors(
    previous: Option<std::ops::Range<usize>>,
    projected: Option<std::ops::Range<usize>>,
    output: &mut Vec<(usize, usize)>,
) {
    let (Some(previous), Some(projected)) = (previous, projected) else {
        return;
    };
    output.push((previous.start, projected.start));
    output.push((previous.end, projected.end));
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
    use super::{
        TreeProjection, map_source_position as map_using_sidecars,
        source_map_can_materialize_as_text, trees_correspond_for_position_mapping,
    };
    use caret_math::{MathInputKind, ProjectedMath, parse_projection, serialize_tree};

    fn parse_tree(source: &str) -> Result<caret_doc::MathTree, Box<dyn std::error::Error>> {
        let ProjectedMath::Tree(tree) = parse_projection(source, MathInputKind::Inline)?
            .into_parts()
            .0
        else {
            return Err("expected inline math tree".into());
        };
        Ok(tree)
    }

    fn map_source_position(
        previous: &caret_doc::MathTree,
        projected: &caret_doc::MathTree,
        previous_source: &str,
        projected_source: &str,
        position: usize,
    ) -> Option<usize> {
        let previous_projection = parse_projection(previous_source, MathInputKind::Inline).ok()?;
        let projected_projection =
            parse_projection(projected_source, MathInputKind::Inline).ok()?;
        map_using_sidecars(
            TreeProjection {
                tree: previous,
                map: previous_projection.source_map().tree()?,
                source: previous_source,
            },
            TreeProjection {
                tree: projected,
                map: projected_projection.source_map().tree()?,
                source: projected_source,
            },
            position,
        )
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
    fn position_mapping_preserves_text_and_math_boundaries_when_flows_merge()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = r"\text{a}\text{b}\alpha\text{d}\text{e}";
        let original = parse_tree(source)?;
        let canonical = serialize_tree(&original);
        let projected = parse_tree(&canonical)?;
        assert_ne!(canonical, source);
        assert!(trees_correspond_for_position_mapping(&original, &projected));
        let alpha_start = source.find("\\alpha").ok_or("missing middle atom")?;
        let canonical_alpha_start = canonical
            .find("\\alpha")
            .ok_or("missing canonical middle atom")?;
        let alpha_end = alpha_start + "\\alpha".len();
        let canonical_alpha_end = canonical_alpha_start + "\\alpha".len();
        for (source_position, expected) in [
            (
                source.find("{a}").ok_or("missing first text")? + 1,
                canonical.find("{ab}").ok_or("missing merged text")? + 1,
            ),
            (
                source.find("{a}").ok_or("missing first text")? + 2,
                canonical.find("{ab}").ok_or("missing merged text")? + 2,
            ),
            (
                source.find("{b}").ok_or("missing second text")? + 1,
                canonical.find("{ab}").ok_or("missing merged text")? + 2,
            ),
            (alpha_start, canonical_alpha_start),
            (alpha_end, canonical_alpha_end),
            (
                source.find("{d}").ok_or("missing third text")? + 1,
                canonical
                    .rfind("{de}")
                    .ok_or("missing merged trailing text")?
                    + 1,
            ),
            (
                source.find("{e}").ok_or("missing fourth text")? + 1,
                canonical
                    .rfind("{de}")
                    .ok_or("missing merged trailing text")?
                    + 2,
            ),
        ] {
            assert_eq!(
                map_source_position(&original, &projected, source, &canonical, source_position,),
                Some(expected),
                "failed to map source byte {source_position} in {source:?}"
            );
        }

        let source = r"\text{\%é}\text{界}\alpha";
        let original = parse_tree(source)?;
        let canonical = serialize_tree(&original);
        let projected = parse_tree(&canonical)?;
        let escaped_start = source.find(r"\%").ok_or("missing escaped percent")?;
        let canonical_escaped_start = canonical
            .find(r"\%")
            .ok_or("missing canonical escaped percent")?;
        for (source_position, expected) in [
            (escaped_start, canonical_escaped_start),
            (escaped_start + 2, canonical_escaped_start + 2),
            (
                source.find("é").ok_or("missing non-ASCII text")?,
                canonical
                    .find("é")
                    .ok_or("missing canonical non-ASCII text")?,
            ),
            (
                source.find("界").ok_or("missing CJK text")?,
                canonical.find("界").ok_or("missing canonical CJK text")?,
            ),
        ] {
            assert_eq!(
                map_source_position(&original, &projected, source, &canonical, source_position,),
                Some(expected),
                "failed to map source byte {source_position} in {source:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn position_mapping_rejects_merging_text_with_different_run_semantics()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut original = parse_tree(r"\text{a}\text{b}")?;
        let Some(caret_doc::MathContent::Text(first)) = original.root.content.first_mut() else {
            return Err("first text flow should be present".into());
        };
        let Some(run) = first.runs.first_mut() else {
            return Err("first text flow should have a Run".into());
        };
        run.props
            .insert("underline".to_owned(), serde_json::json!(true));
        let canonical = serialize_tree(&original);
        let projected = parse_tree(&canonical)?;

        assert!(!trees_correspond_for_position_mapping(
            &original, &projected
        ));
        Ok(())
    }

    #[test]
    fn position_mapping_does_not_require_temporary_ranges_in_document_trees()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = r"\text{a}\text{b}\alpha\text{d}\text{e}";
        let original = parse_tree(source)?;
        let canonical = serialize_tree(&original);
        let projected = parse_tree(&canonical)?;
        let source_position = source.find("{b}").ok_or("missing second text")? + 1;
        let expected = canonical.find("{ab}").ok_or("missing merged text")? + 2;
        let prefix = source
            .bytes()
            .zip(canonical.bytes())
            .take_while(|(left, right)| left == right)
            .count();
        let suffix = source
            .bytes()
            .rev()
            .zip(canonical.bytes().rev())
            .take_while(|(left, right)| left == right)
            .count();
        assert!(prefix < source_position && source_position < source.len() - suffix);
        assert_eq!(
            map_source_position(&original, &projected, source, &canonical, source_position),
            Some(expected)
        );
        Ok(())
    }

    #[test]
    fn sidecar_maps_nested_math_and_escaped_unicode_without_dto_ranges()
    -> Result<(), Box<dyn std::error::Error>> {
        for (source, marker) in [
            (r"\frac {a}{b} + \sqrt {x}", "b"),
            (r"\text{a}\text{\%é}\alpha\text{d}\text{e}", "é"),
        ] {
            let original = parse_tree(source)?;
            let canonical = serialize_tree(&original);
            let projected = parse_tree(&canonical)?;
            let source_position = source.find(marker).ok_or("source marker missing")?;
            let expected = canonical.find(marker).ok_or("canonical marker missing")?;
            let prefix = source
                .bytes()
                .zip(canonical.bytes())
                .take_while(|(left, right)| left == right)
                .count();
            let suffix = source
                .bytes()
                .rev()
                .zip(canonical.bytes().rev())
                .take_while(|(left, right)| left == right)
                .count();
            assert!(prefix < source_position && source_position < source.len() - suffix);
            assert_eq!(
                map_source_position(&original, &projected, source, &canonical, source_position),
                Some(expected)
            );
        }
        Ok(())
    }

    #[test]
    fn parser_sidecar_without_extensions_is_materializable()
    -> Result<(), Box<dyn std::error::Error>> {
        let projection = parse_projection(r"\text{x}", MathInputKind::Inline)?;
        let (ProjectedMath::Tree(tree), mut source_map) = projection.into_parts() else {
            return Err("expected inline math tree".into());
        };
        let parsed_map = source_map.tree().cloned().ok_or("sidecar missing")?;
        let mapped = parsed_map
            .reconcile_for_tree(&tree, &tree, None, source_map.source(), None)
            .ok_or("sidecar reconciliation failed")?;
        assert!(source_map.bind_tree_sidecar_after_validated_edit(&tree, mapped));
        let map = source_map.tree().ok_or("sidecar missing")?;
        assert!(source_map_can_materialize_as_text(map.root()));
        Ok(())
    }
}
