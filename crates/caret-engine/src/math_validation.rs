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
pub(crate) fn tree_for_history_projection(
    tree: &MathTree,
    source: &str,
    kind: MathInputKind,
) -> Option<MathTree> {
    let projection = caret_math::parse_projection(source, kind).ok()?;
    let (caret_math::ProjectedMath::Tree(projected), _) = projection.into_parts() else {
        return None;
    };
    let mut updated = tree.clone();
    refresh_tree_ranges(&mut updated, &projected).then_some(updated)
}

/// 恢复源码分支时，允许源写法不同于持久树的规范序列化，但语义树必须一致。
pub(crate) fn tree_for_source_history_projection(
    tree: &MathTree,
    source: &str,
    kind: MathInputKind,
) -> Option<MathTree> {
    if let Some(updated) = tree_for_history_projection(tree, source, kind) {
        return Some(updated);
    }
    let projection = caret_math::parse_projection(source, kind).ok()?;
    let (caret_math::ProjectedMath::Tree(projected), _) = projection.into_parts() else {
        return None;
    };
    if caret_math::serialize_tree(&projected) != caret_math::serialize_tree(tree)
        || placeholder_positions(&projected.root) != placeholder_positions(&tree.root)
        || text_flow_range_signature(&projected.root) != text_flow_range_signature(&tree.root)
        || node_range_signature(&projected.root) != node_range_signature(&tree.root)
    {
        return None;
    }
    Some(tree.clone())
}

/// 为历史 Align 源码重建单元格范围，并保留所有持久化字段。
pub(crate) fn align_for_history_projection(align: &Align, source: &str) -> Option<Align> {
    let projection = caret_math::parse_projection(source, MathInputKind::Align).ok()?;
    let (caret_math::ProjectedMath::Align(projected), _) = projection.into_parts() else {
        return None;
    };
    let mut updated = align.clone();
    refresh_align_ranges(&mut updated, &projected).then_some(updated)
}

/// 恢复源码分支的 Align 时验证规范语义并保留持久化行列与元数据。
pub(crate) fn align_for_source_history_projection(align: &Align, source: &str) -> Option<Align> {
    if let Some(updated) = align_for_history_projection(align, source) {
        return Some(updated);
    }
    let projection = caret_math::parse_projection(source, MathInputKind::Align).ok()?;
    let (caret_math::ProjectedMath::Align(projected), _) = projection.into_parts() else {
        return None;
    };
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
    Some(align.clone())
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
    let (mut tree_index, mut projection_index) = (0, 0);
    while tree_index < tree.content.len() || projection_index < projection.content.len() {
        match (
            tree.content.get_mut(tree_index),
            projection.content.get(projection_index),
        ) {
            (Some(MathContent::Math(node)), Some(MathContent::Math(projected)))
                if node.kind == caret_doc::MathNodeKind::Placeholder
                    && projected.kind == node.kind =>
            {
                if !refresh_range(&mut node.source_range, &projected.source_range) {
                    return false;
                }
                tree_index += 1;
                projection_index += 1;
            }
            (Some(MathContent::Text(current)), Some(MathContent::Text(parsed)))
                if same_display_text(current, parsed) =>
            {
                if !refresh_text_ranges(current, parsed) {
                    return false;
                }
                tree_index += 1;
                projection_index += 1;
            }
            (Some(MathContent::Text(current)), _) => {
                let Some((source_range, source_map, next_projection_index)) =
                    collect_projected_text_sequence(current, &projection.content, projection_index)
                else {
                    return false;
                };
                if !refresh_text_ranges_from_map(current, source_range, source_map) {
                    return false;
                }
                tree_index += 1;
                projection_index = next_projection_index;
            }
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
        || tree.children.len() != projection.children.len()
        || !refresh_range(&mut tree.source_range, &projection.source_range)
    {
        return false;
    }
    tree.children
        .iter_mut()
        .zip(&projection.children)
        .all(|(tree, projection)| refresh_content_ranges(tree, projection))
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
    left.content.len() == right.content.len()
        && left
            .content
            .iter()
            .zip(&right.content)
            .all(|(left, right)| {
                if compare_ranges {
                    same_content_semantics(left, right)
                } else {
                    same_content_projection(left, right, false)
                }
            })
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
                && left.children.len() == right.children.len()
                && left
                    .children
                    .iter()
                    .zip(&right.children)
                    .all(|(l, r)| same_content_projection(l, r, compare_ranges))
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
