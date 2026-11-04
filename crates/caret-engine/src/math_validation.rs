//! 在编辑器服务边界校验数学源码与持久化树，并重建源码投影的瞬时范围。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{
    Align, InlineEnvironment, MathContent, MathFlow, MathNode, MathSourceRange, MathTree, TextFlow,
};
#[cfg(test)]
use caret_doc::{Block, Document, FlowContent, ParagraphContent};
use caret_math::MathInputKind;
use std::collections::BTreeMap;

pub(crate) fn project_tree_for_edit(
    tree: &MathTree,
    kind: MathInputKind,
) -> Result<(caret_math::MathSourceMap<'static>, MathTree), String> {
    let canonical_source = caret_math::serialize_tree(tree);
    let projection = caret_math::parse_projection(&canonical_source, kind)
        .map_err(|error| format!("could not project formula tree to TeX: {error}"))?;
    let (caret_math::ProjectedMath::Tree(projected), mut source_map) = projection.into_parts()
    else {
        return Err("formula tree projected to a different formula kind".to_owned());
    };
    let mut updated = tree.clone();
    if !refresh_tree_ranges(&mut updated, &projected) {
        return Err("formula tree cannot be projected without losing structure".to_owned());
    }
    if !source_map.bind_tree_after_validated_edit(&updated) {
        return Err("formula tree ranges cannot be bound to its source map".to_owned());
    }
    updated.validate().map_err(|error| error.to_string())?;
    Ok((source_map, updated))
}

pub(crate) fn project_align_for_edit(
    align: &Align,
) -> Result<(caret_math::MathSourceMap<'static>, Align), String> {
    let canonical_source = caret_math::serialize_align(align);
    let projection = caret_math::parse_projection(&canonical_source, MathInputKind::Align)
        .map_err(|error| format!("could not project align tree to TeX: {error}"))?;
    let (caret_math::ProjectedMath::Align(projected), mut source_map) = projection.into_parts()
    else {
        return Err("align tree projected to a different formula kind".to_owned());
    };
    let mut updated = align.clone();
    if !refresh_align_ranges(&mut updated, &projected) {
        return Err("align projection changed its structure".to_owned());
    }
    if !source_map.bind_align_after_validated_edit(&updated) {
        return Err("align ranges cannot be bound to their source map".to_owned());
    }
    updated.validate().map_err(|error| error.to_string())?;
    Ok((source_map, updated))
}

/// 为历史源码重建瞬时范围，并要求持久化字段可完整映射。
#[cfg(test)]
pub(crate) fn tree_for_history_projection(
    tree: &MathTree,
    source: &str,
    kind: MathInputKind,
) -> Option<MathTree> {
    tree_for_history_projection_with_map(tree, source, kind).map(|(tree, _)| tree)
}

pub(crate) fn tree_for_history_projection_with_map(
    tree: &MathTree,
    source: &str,
    kind: MathInputKind,
) -> Option<(MathTree, caret_math::MathTreeSourceMap)> {
    let projection = caret_math::parse_projection(source, kind).ok()?;
    let (caret_math::ProjectedMath::Tree(projected), mut source_map) = projection.into_parts()
    else {
        return None;
    };
    let mut updated = tree.clone();
    if !refresh_tree_ranges(&mut updated, &projected)
        || !source_map.bind_tree_after_validated_edit(&updated)
    {
        return None;
    }
    Some((updated, source_map.tree()?.clone()))
}

/// 恢复源码分支时，允许源写法不同于持久树的规范序列化，但语义树必须一致。
#[cfg(test)]
pub(crate) fn tree_for_source_history_projection(
    tree: &MathTree,
    source: &str,
    kind: MathInputKind,
) -> Option<MathTree> {
    tree_for_source_history_projection_with_map(tree, source, kind).map(|(tree, _)| tree)
}

pub(crate) fn tree_for_source_history_projection_with_map(
    tree: &MathTree,
    source: &str,
    kind: MathInputKind,
) -> Option<(MathTree, caret_math::MathTreeSourceMap)> {
    if let Some(projected) = tree_for_history_projection_with_map(tree, source, kind) {
        return Some(projected);
    }
    let projection = caret_math::parse_projection(source, kind).ok()?;
    let (caret_math::ProjectedMath::Tree(projected), source_map) = projection.into_parts() else {
        return None;
    };
    let source_map = source_map.tree()?.clone();
    if caret_math::serialize_tree(&projected) != caret_math::serialize_tree(tree)
        || placeholder_positions(&projected.root) != placeholder_positions(&tree.root)
        || text_flow_range_signature(&projected.root) != text_flow_range_signature(&tree.root)
        || node_range_signature(&projected.root) != node_range_signature(&tree.root)
    {
        return None;
    }
    Some((tree.clone(), source_map))
}

/// 为历史 Align 源码重建单元格范围，并保留所有持久化字段。
pub(crate) fn align_for_history_projection_with_map(
    align: &Align,
    source: &str,
) -> Option<(Align, caret_math::AlignSourceMap)> {
    let projection = caret_math::parse_projection(source, MathInputKind::Align).ok()?;
    let (caret_math::ProjectedMath::Align(projected), mut source_map) = projection.into_parts()
    else {
        return None;
    };
    let mut updated = align.clone();
    if !refresh_align_ranges(&mut updated, &projected)
        || !source_map.bind_align_after_validated_edit(&updated)
    {
        return None;
    }
    Some((updated, source_map.align()?.clone()))
}

/// 恢复源码分支的 Align 时验证规范语义并保留持久化行列与元数据。
pub(crate) fn align_for_source_history_projection_with_map(
    align: &Align,
    source: &str,
) -> Option<(Align, caret_math::AlignSourceMap)> {
    if let Some(projected) = align_for_history_projection_with_map(align, source) {
        return Some(projected);
    }
    let projection = caret_math::parse_projection(source, MathInputKind::Align).ok()?;
    let (caret_math::ProjectedMath::Align(projected), source_map) = projection.into_parts() else {
        return None;
    };
    let source_map = source_map.align()?.clone();
    let projected_anchors = projected
        .rows
        .iter()
        .flat_map(|row| &row.cells)
        .map(|tree| placeholder_positions(&tree.root))
        .collect::<Vec<_>>();
    let stored_anchors = align
        .rows
        .iter()
        .flat_map(|row| &row.cells)
        .map(|tree| placeholder_positions(&tree.root))
        .collect::<Vec<_>>();
    let projected_text_ranges = align_text_flow_range_signature(&projected);
    let stored_text_ranges = align_text_flow_range_signature(align);
    let projected_node_ranges = align_node_range_signature(&projected);
    let stored_node_ranges = align_node_range_signature(align);
    if caret_math::serialize_align(&projected) != caret_math::serialize_align(align)
        || projected_anchors != stored_anchors
        || projected_text_ranges != stored_text_ranges
        || projected_node_ranges != stored_node_ranges
    {
        return None;
    }
    Some((align.clone(), source_map))
}

type TextFlowRangeSignature = (String, Option<(u32, u32)>, Vec<(u32, u32, u32, u32)>);
type NodeRangeSignature = Vec<Option<(u32, u32)>>;
type AlignNodeRangeSignature = Vec<Vec<NodeRangeSignature>>;

fn text_flow_range_signature(flow: &MathFlow) -> Vec<TextFlowRangeSignature> {
    fn collect_content(content: &MathContent, signature: &mut Vec<TextFlowRangeSignature>) {
        match content {
            MathContent::Text(text) => signature.push((
                text.runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
                text.source_range
                    .as_ref()
                    .map(|range| (range.start, range.end)),
                text.source_map
                    .iter()
                    .map(|entry| {
                        (
                            entry.text_start,
                            entry.text_end,
                            entry.source_range.start,
                            entry.source_range.end,
                        )
                    })
                    .collect(),
            )),
            MathContent::Math(node) => {
                for child in &node.children {
                    collect_flow(child, signature);
                }
            }
            MathContent::Environment(environment) => {
                for child in &environment.children {
                    collect_content(child, signature);
                }
            }
            MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
        }
    }

    fn collect_flow(flow: &MathFlow, signature: &mut Vec<TextFlowRangeSignature>) {
        for content in &flow.content {
            collect_content(content, signature);
        }
    }

    let mut signature = Vec::new();
    collect_flow(flow, &mut signature);
    signature
}

fn align_text_flow_range_signature(align: &Align) -> Vec<Vec<Vec<TextFlowRangeSignature>>> {
    align
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|tree| text_flow_range_signature(&tree.root))
                .collect()
        })
        .collect()
}

fn node_range_signature(flow: &MathFlow) -> NodeRangeSignature {
    fn collect_content(content: &MathContent, ranges: &mut Vec<Option<(u32, u32)>>) {
        match content {
            MathContent::Math(node) => {
                ranges.push(
                    node.source_range
                        .as_ref()
                        .map(|range| (range.start, range.end)),
                );
                for child in &node.children {
                    collect_flow(child, ranges);
                }
            }
            MathContent::Environment(environment) => {
                ranges.push(
                    environment
                        .source_range
                        .as_ref()
                        .map(|range| (range.start, range.end)),
                );
                for child in &environment.children {
                    collect_content(child, ranges);
                }
            }
            MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
        }
    }

    fn collect_flow(flow: &MathFlow, ranges: &mut Vec<Option<(u32, u32)>>) {
        ranges.push(
            flow.source_range
                .as_ref()
                .map(|range| (range.start, range.end)),
        );
        for content in &flow.content {
            collect_content(content, ranges);
        }
    }

    let mut ranges = Vec::new();
    collect_flow(flow, &mut ranges);
    ranges
}

fn align_node_range_signature(align: &Align) -> AlignNodeRangeSignature {
    align
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|tree| node_range_signature(&tree.root))
                .collect()
        })
        .collect()
}

fn placeholder_positions(flow: &MathFlow) -> Vec<usize> {
    fn collect_content(content: &MathContent, text_offset: &mut usize, positions: &mut Vec<usize>) {
        match content {
            MathContent::Text(text) => {
                *text_offset += text.runs.iter().map(|run| run.text.len()).sum::<usize>();
            }
            MathContent::Math(node) if node.kind == caret_doc::MathNodeKind::Placeholder => {
                positions.push(*text_offset);
            }
            MathContent::Math(node) => {
                for child in &node.children {
                    collect_flow(child, text_offset, positions);
                }
            }
            MathContent::Environment(environment) => {
                for child in &environment.children {
                    collect_content(child, text_offset, positions);
                }
            }
            MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
        }
    }

    fn collect_flow(flow: &MathFlow, text_offset: &mut usize, positions: &mut Vec<usize>) {
        for content in &flow.content {
            collect_content(content, text_offset, positions);
        }
    }

    let mut positions = Vec::new();
    let mut text_offset = 0;
    collect_flow(flow, &mut text_offset, &mut positions);
    positions
}

fn refresh_tree_ranges(tree: &mut MathTree, projection: &MathTree) -> bool {
    if tree.schema_version != projection.schema_version
        || !refresh_range(&mut tree.root.source_range, &projection.root.source_range)
    {
        return false;
    }
    refresh_flow_ranges(&mut tree.root, &projection.root)
}

fn refresh_align_ranges(align: &mut Align, projection: &Align) -> bool {
    if align.rows.len() != projection.rows.len() {
        return false;
    }
    align
        .rows
        .iter_mut()
        .zip(&projection.rows)
        .all(|(row, projected_row)| {
            row.cells.len() == projected_row.cells.len()
                && row
                    .cells
                    .iter_mut()
                    .zip(&projected_row.cells)
                    .all(|(cell, projected_cell)| refresh_tree_ranges(cell, projected_cell))
        })
}

fn refresh_range(range: &mut Option<MathSourceRange>, projected: &Option<MathSourceRange>) -> bool {
    match (range.as_ref(), projected) {
        (Some(current), Some(projected)) => {
            let mut refreshed = projected.clone();
            refreshed.extra.clone_from(&current.extra);
            *range = Some(refreshed);
        }
        (Some(current), None) if !current.extra.is_empty() => return false,
        (_, None) => *range = None,
        (None, Some(projected)) => *range = Some(projected.clone()),
    }
    true
}

fn refresh_flow_ranges(tree: &mut MathFlow, projection: &MathFlow) -> bool {
    if !refresh_range(&mut tree.source_range, &projection.source_range) {
        return false;
    }
    refresh_content_sequence_ranges(&mut tree.content, &projection.content)
}

fn refresh_content_sequence_ranges(tree: &mut [MathContent], projection: &[MathContent]) -> bool {
    let (mut tree_index, mut projection_index) = (0, 0);
    while tree_index < tree.len() || projection_index < projection.len() {
        if let (Some(MathContent::Text(current)), Some(MathContent::Text(parsed))) =
            (tree.get(tree_index), projection.get(projection_index))
        {
            let has_following_empty_flow = matches!(tree.get(tree_index + 1),
                Some(MathContent::Text(next))
                    if next.runs.iter().all(|run| run.text.is_empty()));
            if same_display_text(current, parsed) && !has_following_empty_flow {
                let Some(MathContent::Text(current)) = tree.get_mut(tree_index) else {
                    return false;
                };
                if !refresh_text_ranges(current, parsed) {
                    return false;
                }
                tree_index += 1;
                projection_index += 1;
                continue;
            }
            if let Some((next_tree_index, pieces)) =
                collect_tree_text_sequence(tree, tree_index, parsed)
            {
                let mut piece_index = 0;
                for content in &mut tree[tree_index..next_tree_index] {
                    let MathContent::Text(current) = content else {
                        return false;
                    };
                    let Some((source_range, source_map)) = pieces.get(piece_index).cloned() else {
                        return false;
                    };
                    if !refresh_text_ranges_from_map(current, source_range, source_map) {
                        return false;
                    }
                    piece_index += 1;
                }
                if piece_index != pieces.len() {
                    return false;
                }
                tree_index = next_tree_index;
                projection_index += 1;
                continue;
            }
            let Some((source_range, source_map, next_projection_index)) =
                collect_projected_text_sequence(current, projection, projection_index)
            else {
                return false;
            };
            let Some(MathContent::Text(current)) = tree.get_mut(tree_index) else {
                return false;
            };
            if !refresh_text_ranges_from_map(current, source_range, source_map) {
                return false;
            }
            tree_index += 1;
            projection_index = next_projection_index;
            continue;
        }
        match (tree.get_mut(tree_index), projection.get(projection_index)) {
            (Some(current), Some(projected)) => {
                if !refresh_content_ranges(current, projected) {
                    return false;
                }
                tree_index += 1;
                projection_index += 1;
            }
            (None, None) => return true,
            _ => return false,
        }
    }
    true
}

fn refresh_content_ranges(tree: &mut MathContent, projection: &MathContent) -> bool {
    match (tree, projection) {
        (MathContent::Text(previous), MathContent::Text(projected)) => {
            if !same_display_text(previous, projected) {
                return false;
            }
            refresh_text_ranges(previous, projected)
        }
        (MathContent::Math(previous), MathContent::Math(projected)) => {
            refresh_node_ranges(previous, projected)
        }
        (MathContent::Environment(previous), MathContent::Environment(projected)) => {
            refresh_environment_ranges(previous, projected)
        }
        (MathContent::SoftBreak(_), MathContent::SoftBreak(_)) => true,
        (MathContent::Opaque(previous), MathContent::Opaque(projected)) => {
            previous.raw == projected.raw
        }
        _ => false,
    }
}

fn refresh_text_ranges(tree: &mut TextFlow, projection: &TextFlow) -> bool {
    refresh_text_ranges_from_map(
        tree,
        projection.source_range.clone(),
        projection.source_map.clone(),
    )
}

fn collect_projected_text_sequence(
    text: &TextFlow,
    content: &[MathContent],
    start: usize,
) -> Option<(
    Option<MathSourceRange>,
    Vec<caret_doc::TextFlowSourceMapEntry>,
    usize,
)> {
    let expected = text
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    let (mut display, mut source_range, mut source_map) =
        (String::new(), None::<MathSourceRange>, Vec::new());
    let mut index = start;
    while let Some(item) = content.get(index) {
        match item {
            MathContent::Text(projected) => {
                let text_offset = u32::try_from(display.len()).ok()?;
                let fragment = projected
                    .runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>();
                if !expected.get(display.len()..)?.starts_with(&fragment) {
                    return None;
                }
                display.push_str(&fragment);
                match (&mut source_range, &projected.source_range) {
                    (Some(current), Some(next)) => current.end = next.end,
                    (None, Some(next)) => source_range = Some(next.clone()),
                    (_, None) => return None,
                }
                for entry in &projected.source_map {
                    source_map.push(caret_doc::TextFlowSourceMapEntry {
                        text_start: text_offset.checked_add(entry.text_start)?,
                        text_end: text_offset.checked_add(entry.text_end)?,
                        source_range: entry.source_range.clone(),
                        extra: entry.extra.clone(),
                    });
                }
                index += 1;
                if display == expected {
                    return Some((source_range, source_map, index));
                }
            }
            _ => return None,
        }
    }
    None
}

type TextRangeProjection = (
    Option<MathSourceRange>,
    Vec<caret_doc::TextFlowSourceMapEntry>,
);
type SplitTextProjection = (usize, Vec<TextRangeProjection>);

fn collect_tree_text_sequence(
    content: &[MathContent],
    start: usize,
    projected: &TextFlow,
) -> Option<SplitTextProjection> {
    if !projected
        .source_range
        .as_ref()
        .is_some_and(|range| range.extra.is_empty())
    {
        return None;
    }
    let expected = projected
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    let total = content
        .get(start..)?
        .iter()
        .take_while(|item| matches!(item, MathContent::Text(_)))
        .map(|item| match item {
            MathContent::Text(flow) => flow
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            _ => String::new(),
        })
        .collect::<String>();
    if total != expected {
        return None;
    }
    let sequence_end = start.checked_add(
        content
            .get(start..)?
            .iter()
            .take_while(|item| matches!(item, MathContent::Text(_)))
            .count(),
    )?;

    let mut pieces = Vec::new();
    let mut display_start = 0usize;
    let mut index = start;
    while index < sequence_end {
        let MathContent::Text(flow) = content.get(index)? else {
            return None;
        };
        let display_len = flow
            .runs
            .iter()
            .try_fold(0usize, |length, run| length.checked_add(run.text.len()))?;
        let display_end = display_start.checked_add(display_len)?;
        let source_start = text_display_source_boundary(projected, display_start)?;
        let source_end = text_display_source_boundary(projected, display_end)?;
        let source_range = if display_start == 0 && display_end == expected.len() {
            projected.source_range.clone()
        } else {
            Some(MathSourceRange {
                start: u32::try_from(source_start).ok()?,
                end: u32::try_from(source_end).ok()?,
                extra: BTreeMap::new(),
            })
        };
        let source_map = text_display_source_map_piece(projected, display_start, display_end)?;
        pieces.push((source_range, source_map));
        display_start = display_end;
        index += 1;
    }
    (index > start && index <= content.len()).then_some((index, pieces))
}

fn text_display_source_boundary(flow: &TextFlow, display: usize) -> Option<usize> {
    if display == 0 {
        return Some(flow.source_range.as_ref()?.start as usize);
    }
    let display_len = flow
        .runs
        .iter()
        .try_fold(0usize, |length, run| length.checked_add(run.text.len()))?;
    if display == display_len {
        return Some(flow.source_range.as_ref()?.end as usize);
    }
    let mut candidates = Vec::new();
    for entry in &flow.source_map {
        let display_start = entry.text_start as usize;
        let display_end = entry.text_end as usize;
        let source_start = entry.source_range.start as usize;
        let source_end = entry.source_range.end as usize;
        if display == display_start {
            candidates.push(source_start);
        } else if display == display_end {
            candidates.push(source_end);
        } else if display_start < display
            && display < display_end
            && display_end - display_start == source_end - source_start
        {
            candidates.push(source_start.checked_add(display - display_start)?);
        }
    }
    candidates.sort_unstable();
    candidates.dedup();
    (candidates.len() == 1).then(|| candidates[0])
}

fn text_display_source_map_piece(
    flow: &TextFlow,
    display_start: usize,
    display_end: usize,
) -> Option<Vec<caret_doc::TextFlowSourceMapEntry>> {
    let mut result = Vec::new();
    for entry in &flow.source_map {
        let start = entry.text_start as usize;
        let end = entry.text_end as usize;
        if start == end {
            let belongs = display_start <= start
                && (start < display_end
                    || display_end == flow_display_len(flow) && start == display_end);
            if belongs {
                let mut anchor = entry.clone();
                anchor.text_start = u32::try_from(start - display_start).ok()?;
                anchor.text_end = anchor.text_start;
                result.push(anchor);
            }
            continue;
        }
        let overlap_start = start.max(display_start);
        let overlap_end = end.min(display_end);
        if overlap_start >= overlap_end {
            continue;
        }
        let source_start = entry.source_range.start as usize;
        let source_end = entry.source_range.end as usize;
        let linear = end - start == source_end - source_start;
        if !linear && (overlap_start != start || overlap_end != end) {
            return None;
        }
        if (overlap_start != start || overlap_end != end)
            && (!entry.extra.is_empty() || !entry.source_range.extra.is_empty())
        {
            return None;
        }
        let mut piece = entry.clone();
        piece.text_start = u32::try_from(overlap_start - display_start).ok()?;
        piece.text_end = u32::try_from(overlap_end - display_start).ok()?;
        if linear {
            piece.source_range.start =
                u32::try_from(source_start.checked_add(overlap_start - start)?).ok()?;
            piece.source_range.end =
                u32::try_from(source_start.checked_add(overlap_end - start)?).ok()?;
        }
        result.push(piece);
    }
    if display_start == display_end
        && !result
            .iter()
            .any(|entry| entry.text_start == 0 && entry.text_end == 0)
    {
        let source = text_display_source_boundary(flow, display_start)?;
        let boundary = u32::try_from(source).ok()?;
        result.push(caret_doc::TextFlowSourceMapEntry {
            text_start: 0,
            text_end: 0,
            source_range: MathSourceRange {
                start: boundary,
                end: boundary,
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        });
    }
    Some(result)
}

fn flow_display_len(flow: &TextFlow) -> usize {
    flow.runs.iter().map(|run| run.text.len()).sum()
}

fn refresh_text_ranges_from_map(
    tree: &mut TextFlow,
    projected_range: Option<MathSourceRange>,
    mut projected_map: Vec<caret_doc::TextFlowSourceMapEntry>,
) -> bool {
    if !refresh_range(&mut tree.source_range, &projected_range) {
        return false;
    }
    let mut used = vec![false; projected_map.len()];
    for old in &tree.source_map {
        let Some((index, projected)) =
            projected_map
                .iter_mut()
                .enumerate()
                .find(|(index, projected)| {
                    !used[*index]
                        && (projected.text_start, projected.text_end)
                            == (old.text_start, old.text_end)
                })
        else {
            if !old.extra.is_empty() || !old.source_range.extra.is_empty() {
                return false;
            }
            continue;
        };
        used[index] = true;
        projected.extra.clone_from(&old.extra);
        projected
            .source_range
            .extra
            .clone_from(&old.source_range.extra);
    }
    tree.source_map = projected_map;
    true
}

fn refresh_node_ranges(tree: &mut MathNode, projection: &MathNode) -> bool {
    if tree.kind == caret_doc::MathNodeKind::Placeholder {
        return projection.kind == tree.kind
            && refresh_range(&mut tree.source_range, &projection.source_range);
    }
    if tree.kind != projection.kind
        || tree.text != projection.text
        || tree.attributes != projection.attributes
        || tree.children.len() != projection.children.len()
        || !refresh_range(&mut tree.source_range, &projection.source_range)
    {
        return false;
    }
    tree.children
        .iter_mut()
        .zip(&projection.children)
        .all(|(tree, projection)| refresh_flow_ranges(tree, projection))
}

fn refresh_environment_ranges(
    tree: &mut InlineEnvironment<MathContent>,
    projection: &InlineEnvironment<MathContent>,
) -> bool {
    if tree.kind != projection.kind
        || !source_environment_props_match(tree.kind, &tree.props, &projection.props)
        || !refresh_range(&mut tree.source_range, &projection.source_range)
    {
        return false;
    }
    refresh_content_sequence_ranges(&mut tree.children, &projection.children)
}

fn same_display_text(previous: &TextFlow, projected: &TextFlow) -> bool {
    previous
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>()
        == projected
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()
}

/// 验证文档中每棵数学树都与其 LaTeX 源码表达相同语义。
/// `caret-doc` 只校验稳定 schema；这里依赖解析器，因此不反向依赖 `caret-math`。
#[cfg(test)]
fn validate_document_math_sources(document: &Document) -> Result<(), String> {
    fn validate_inlines(nodes: &[ParagraphContent]) -> Result<(), String> {
        for node in nodes {
            match node {
                FlowContent::Math(math) => {
                    validate_tree(&math.math_tree, MathInputKind::Inline, &math.id.to_string())?;
                }
                FlowContent::Environment(environment) => validate_inlines(&environment.children)?,
                FlowContent::Text(_) | FlowContent::SoftBreak(_) | FlowContent::Opaque(_) => {}
            }
        }
        Ok(())
    }

    fn validate_blocks(blocks: &[Block]) -> Result<(), String> {
        for block in blocks {
            match block {
                Block::Paragraph(paragraph) => validate_inlines(&paragraph.inlines)?,
                Block::Equation(equation) => validate_tree(
                    &equation.math_tree,
                    MathInputKind::Equation,
                    &equation.id.to_string(),
                )?,
                Block::Align(align) => validate_align(align)?,
                Block::Environment(environment) => {
                    if let Some(title) = &environment.title {
                        validate_inlines(&title.inlines)?;
                    }
                    validate_blocks(&environment.children)?;
                }
                Block::ExplicitBreak(_) | Block::Opaque(_) => {}
            }
        }
        Ok(())
    }

    validate_blocks(&document.body)
}

#[cfg(test)]
fn validate_tree(tree: &MathTree, kind: MathInputKind, id: &str) -> Result<(), String> {
    let source = caret_math::serialize_tree(tree);
    let parsed = caret_math::parse(&source, kind)
        .map_err(|error| format!("math {id} source does not parse: {error}"))?;
    let caret_math::ParsedMath::Tree(parsed) = parsed else {
        return Err(format!(
            "math {id} source parsed as a different formula kind"
        ));
    };
    if same_flow_semantics(&tree.root, &parsed.root) {
        Ok(())
    } else {
        Err(format!("math {id} source and tree disagree"))
    }
}

/// 返回源码可表达语义一致的解析树；树独有元数据由编辑事务另行核验。
pub(crate) fn parse_tree_with_same_semantics(
    tree: &MathTree,
    kind: MathInputKind,
) -> Option<MathTree> {
    let source = caret_math::serialize_tree(tree);
    let caret_math::ParsedMath::Tree(parsed) = caret_math::parse(&source, kind).ok()? else {
        return None;
    };
    same_flow_structure(&tree.root, &parsed.root).then_some(parsed)
}

#[cfg(test)]
fn validate_align(align: &Align) -> Result<(), String> {
    let source = caret_math::serialize_align(align);
    let parsed = caret_math::parse(&source, MathInputKind::Align)
        .map_err(|error| format!("align {} source does not parse: {error}", align.id))?;
    let caret_math::ParsedMath::Align(parsed) = parsed else {
        return Err(format!(
            "align {} source parsed as a different formula kind",
            align.id
        ));
    };
    if parsed.rows.len() != align.rows.len()
        || parsed.rows.iter().zip(&align.rows).any(|(parsed, stored)| {
            parsed.cells.len() != stored.cells.len()
                || parsed
                    .cells
                    .iter()
                    .zip(&stored.cells)
                    .any(|(parsed, stored)| !same_flow_semantics(&stored.root, &parsed.root))
        })
    {
        return Err(format!("align {} source and cells disagree", align.id));
    }
    for row in &align.rows {
        for cell in &row.cells {
            if is_empty_align_placeholder(cell) {
                continue;
            }
            validate_tree(cell, MathInputKind::Inline, &row.id.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn is_empty_align_placeholder(tree: &MathTree) -> bool {
    tree.root.content.is_empty()
}

#[cfg(test)]
fn same_flow_semantics(left: &MathFlow, right: &MathFlow) -> bool {
    same_flow_projection(left, right, true)
}

fn same_flow_structure(left: &MathFlow, right: &MathFlow) -> bool {
    same_flow_projection(left, right, false)
}

fn same_flow_projection(left: &MathFlow, right: &MathFlow, compare_ranges: bool) -> bool {
    same_content_sequence_projection(&left.content, &right.content, compare_ranges)
}

fn same_content_sequence_projection(
    left: &[MathContent],
    right: &[MathContent],
    compare_ranges: bool,
) -> bool {
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() && right_index < right.len() {
        if matches!(left[left_index], MathContent::Text(_))
            && matches!(right[right_index], MathContent::Text(_))
        {
            let left_end = text_group_end(left, left_index);
            let right_end = text_group_end(right, right_index);
            if compare_ranges {
                if left_end - left_index != right_end - right_index
                    || !left[left_index..left_end]
                        .iter()
                        .zip(&right[right_index..right_end])
                        .all(|(left, right)| same_content_semantics(left, right))
                {
                    return false;
                }
            } else if left_end - left_index == right_end - right_index {
                if !left[left_index..left_end]
                    .iter()
                    .zip(&right[right_index..right_end])
                    .all(|(left, right)| same_content_projection(left, right, false))
                {
                    return false;
                }
            } else if !same_text_group(&left[left_index..left_end], &right[right_index..right_end])
            {
                return false;
            }
            left_index = left_end;
            right_index = right_end;
            continue;
        }
        let matches = if compare_ranges {
            same_content_semantics(&left[left_index], &right[right_index])
        } else {
            same_content_projection(&left[left_index], &right[right_index], false)
        };
        if !matches {
            return false;
        }
        left_index += 1;
        right_index += 1;
    }
    left_index == left.len() && right_index == right.len()
}

fn text_group_end(contents: &[MathContent], start: usize) -> usize {
    start
        + contents[start..]
            .iter()
            .take_while(|content| matches!(content, MathContent::Text(_)))
            .count()
}

fn same_text_group(left: &[MathContent], right: &[MathContent]) -> bool {
    // 源码形状比较只核对可见文本；Run 元数据仍在持久树上，降级资格由宿主适配器单独检查。
    let text = |contents: &[MathContent]| {
        contents
            .iter()
            .filter_map(|content| match content {
                MathContent::Text(flow) => Some(
                    flow.runs
                        .iter()
                        .flat_map(|run| run.text.chars())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<String>()
    };
    text(left) == text(right)
}

fn same_content_semantics(left: &MathContent, right: &MathContent) -> bool {
    same_content_projection(left, right, true)
}

fn same_content_projection(left: &MathContent, right: &MathContent, compare_ranges: bool) -> bool {
    match (left, right) {
        (MathContent::Text(left), MathContent::Text(right)) => {
            (!compare_ranges
                || (left
                    .source_range
                    .as_ref()
                    .map(|range| (range.start, range.end))
                    == right
                        .source_range
                        .as_ref()
                        .map(|range| (range.start, range.end))
                    && left.source_map.len() == right.source_map.len()
                    && left
                        .source_map
                        .iter()
                        .zip(&right.source_map)
                        .all(|(left, right)| {
                            (left.text_start, left.text_end) == (right.text_start, right.text_end)
                                && (left.source_range.start, left.source_range.end)
                                    == (right.source_range.start, right.source_range.end)
                        })))
                && left
                    .runs
                    .iter()
                    .flat_map(|run| run.text.chars())
                    .eq(right.runs.iter().flat_map(|run| run.text.chars()))
        }
        (MathContent::Math(left), MathContent::Math(right)) => {
            if compare_ranges {
                same_node_semantics(left, right)
            } else {
                same_node_projection(left, right, false)
            }
        }
        (MathContent::Environment(left), MathContent::Environment(right)) => {
            let children_match = if compare_ranges {
                left.children.len() == right.children.len()
                    && left
                        .children
                        .iter()
                        .zip(&right.children)
                        .all(|(left, right)| same_content_projection(left, right, true))
            } else {
                same_content_sequence_projection(&left.children, &right.children, false)
            };
            left.kind == right.kind
                && source_environment_props_match(left.kind, &left.props, &right.props)
                && (!compare_ranges
                    || left
                        .source_range
                        .as_ref()
                        .map(|range| (range.start, range.end))
                        == right
                            .source_range
                            .as_ref()
                            .map(|range| (range.start, range.end)))
                && children_match
        }
        (MathContent::SoftBreak(_), MathContent::SoftBreak(_)) => true,
        (MathContent::Opaque(left), MathContent::Opaque(right)) => left.raw == right.raw,
        _ => false,
    }
}

fn source_environment_props_match(
    kind: caret_doc::InlineEnvironmentKind,
    stored: &std::collections::BTreeMap<String, serde_json::Value>,
    parsed: &std::collections::BTreeMap<String, serde_json::Value>,
) -> bool {
    // 未知属性可作为元数据保留；已知样式属性必须能由源码准确表达。
    let parsed_props_match = parsed
        .iter()
        .all(|(key, value)| stored.get(key) == Some(value));
    if !parsed_props_match {
        return false;
    }

    let enabled_is_representable = !kind.supports_enabled()
        || match stored.get("enabled") {
            None => true,
            Some(serde_json::Value::Bool(enabled)) => {
                parsed
                    .get("enabled")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true)
                    == *enabled
            }
            Some(_) => false,
        };
    let font_is_representable = !kind.supports_font_family()
        || match stored.get("fontFamily") {
            None => true,
            Some(family) => parsed.get("fontFamily") == Some(family),
        };

    enabled_is_representable && font_is_representable
}

fn same_node_semantics(left: &MathNode, right: &MathNode) -> bool {
    same_node_projection(left, right, true)
}

fn same_node_projection(left: &MathNode, right: &MathNode, compare_ranges: bool) -> bool {
    left.kind == right.kind
        && left.text == right.text
        && left.attributes == right.attributes
        && (!compare_ranges
            || left
                .source_range
                .as_ref()
                .map(|range| (range.start, range.end))
                == right
                    .source_range
                    .as_ref()
                    .map(|range| (range.start, range.end)))
        && left.children.len() == right.children.len()
        && left
            .children
            .iter()
            .zip(&right.children)
            .all(|(left, right)| same_flow_projection(left, right, compare_ranges))
}

#[cfg(test)]
mod tests;
