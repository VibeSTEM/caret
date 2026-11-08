//! 在编辑器服务边界校验数学树，并将解析位置绑定到独立源码 sidecar。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{Align, MathContent, MathFlow, MathNode, MathNodeKind, MathTree};
#[cfg(test)]
use caret_doc::{Block, Document, FlowContent, InlineEnvironment, ParagraphContent};
use caret_math::MathInputKind;

pub(crate) fn bind_tree_sidecar(
    source_map: &mut caret_math::MathSourceMap<'_>,
    parsed: &MathTree,
    target: &MathTree,
    previous: Option<&caret_math::MathTreeSourceMap>,
) -> bool {
    let Some(parsed_map) = source_map.tree().cloned() else {
        return false;
    };
    let Some(mapped) =
        parsed_map.reconcile_for_tree(parsed, target, previous, source_map.source(), None)
    else {
        return false;
    };
    source_map.bind_reconciled_tree_sidecar_after_validated_edit(target, mapped)
}

pub(crate) fn bind_align_sidecars(
    source_map: &mut caret_math::MathSourceMap<'_>,
    parsed: &Align,
    target: &Align,
    previous: Option<&caret_math::AlignSourceMap>,
) -> bool {
    let Some(parsed_sidecar) = source_map.align() else {
        return false;
    };
    if parsed.rows.len() != target.rows.len() {
        return false;
    }
    let source = source_map.source();
    let mut mapped_rows = Vec::with_capacity(target.rows.len());
    for (row_index, (parsed_row, target_row)) in parsed.rows.iter().zip(&target.rows).enumerate() {
        if parsed_row.cells.len() != target_row.cells.len() {
            return false;
        }
        let mut mapped_cells = Vec::with_capacity(target_row.cells.len());
        for (cell_index, (parsed_cell, target_cell)) in
            parsed_row.cells.iter().zip(&target_row.cells).enumerate()
        {
            let (Some(parsed_map), Some(scope)) = (
                parsed_sidecar.cell_tree(row_index, cell_index).cloned(),
                parsed_sidecar.cell_range(row_index, cell_index),
            ) else {
                return false;
            };
            let Some(mapped) = parsed_map.reconcile_for_tree(
                parsed_cell,
                target_cell,
                previous.and_then(|previous| previous.cell_tree(row_index, cell_index)),
                source,
                Some(&scope),
            ) else {
                return false;
            };
            mapped_cells.push(mapped);
        }
        mapped_rows.push(mapped_cells);
    }
    source_map.bind_reconciled_align_sidecars_after_validated_edit(target, mapped_rows)
}

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
    let updated = caret_math::reconcile_tree_ranges(tree, &projected)
        .ok_or_else(|| "formula tree cannot be projected without losing structure".to_owned())?;
    if !bind_tree_sidecar(&mut source_map, &projected, &updated, None) {
        return Err("formula tree cannot be bound to its source sidecar".to_owned());
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
    let updated = caret_math::reconcile_align_ranges(align, &projected)
        .ok_or_else(|| "align projection changed its structure".to_owned())?;
    if !bind_align_sidecars(&mut source_map, &projected, &updated, None) {
        return Err("align tree cannot be bound to its source sidecar".to_owned());
    }
    updated.validate().map_err(|error| error.to_string())?;
    Ok((source_map, updated))
}

/// 为历史源码重建独立位置 sidecar，并要求持久化树可完整映射。
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
    let updated = caret_math::reconcile_tree_ranges(tree, &projected)?;
    if !bind_tree_sidecar(&mut source_map, &projected, &updated, None) {
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
    let projection = caret_math::parse_projection(source, kind).ok()?;
    tree_for_source_history_projection_with_map(tree, projection.source_map(), kind)
        .map(|(tree, _)| tree)
}

/// 用历史持有的源码映射验证恢复目标；调用方负责校验投影身份和文档版本。
pub(crate) fn tree_for_source_history_projection_with_map(
    tree: &MathTree,
    stored_projection: &caret_math::MathSourceMap<'_>,
    kind: MathInputKind,
) -> Option<(MathTree, caret_math::MathTreeSourceMap)> {
    if stored_projection.kind() != kind {
        return None;
    }
    let stored_map = stored_projection.tree()?;
    let projection = caret_math::parse_projection(stored_projection.source(), kind).ok()?;
    let (caret_math::ProjectedMath::Tree(projected), mut parsed_projection) =
        projection.into_parts()
    else {
        return None;
    };
    if tree.schema_version != projected.schema_version
        || caret_math::serialize_tree(&projected) != caret_math::serialize_tree(tree)
    {
        return None;
    }
    let updated = caret_math::reconcile_tree_ranges(tree, &projected)?;
    if !bind_tree_sidecar(&mut parsed_projection, &projected, &updated, None) {
        return None;
    }
    let candidate_map = parsed_projection.tree()?;
    if !stored_map.same_locations(candidate_map)
        || !source_map_extras_preserved(candidate_map.root(), stored_map.root())
    {
        return None;
    }
    Some((tree.clone(), stored_map.clone()))
}

/// 恢复源码分支的 Align 时验证规范语义并保留持久化行列与元数据。
pub(crate) fn align_for_source_history_projection_with_map(
    align: &Align,
    stored_projection: &caret_math::MathSourceMap<'_>,
) -> Option<(Align, caret_math::AlignSourceMap)> {
    if stored_projection.kind() != MathInputKind::Align {
        return None;
    }
    let stored_map = stored_projection.align()?;
    let projection =
        caret_math::parse_projection(stored_projection.source(), MathInputKind::Align).ok()?;
    let (caret_math::ProjectedMath::Align(projected), mut parsed_projection) =
        projection.into_parts()
    else {
        return None;
    };
    if caret_math::serialize_align(&projected) != caret_math::serialize_align(align) {
        return None;
    }
    let updated = caret_math::reconcile_align_ranges(align, &projected)?;
    if !bind_align_sidecars(&mut parsed_projection, &projected, &updated, None) {
        return None;
    }
    let candidate_map = parsed_projection.align()?;
    if !stored_map.same_locations(candidate_map) {
        return None;
    }
    for (row_index, row) in align.rows.iter().enumerate() {
        for cell_index in 0..row.cells.len() {
            let candidate = candidate_map.cell_tree(row_index, cell_index)?;
            let stored = stored_map.cell_tree(row_index, cell_index)?;
            if !source_map_extras_preserved(candidate.root(), stored.root()) {
                return None;
            }
        }
    }
    Some((align.clone(), stored_map.clone()))
}

fn source_map_extras_preserved(
    candidate: &caret_math::MathFlowSourceMap,
    stored: &caret_math::MathFlowSourceMap,
) -> bool {
    extras_contained(candidate.range_extra(), stored.range_extra())
        && candidate.content().len() == stored.content().len()
        && candidate
            .content()
            .iter()
            .zip(stored.content())
            .all(|(candidate, stored)| content_map_extras_preserved(candidate, stored))
}

fn content_map_extras_preserved(
    candidate: &caret_math::MathContentSourceMap,
    stored: &caret_math::MathContentSourceMap,
) -> bool {
    use caret_math::MathContentSourceMap;

    match (candidate, stored) {
        (MathContentSourceMap::Text(candidate), MathContentSourceMap::Text(stored)) => {
            extras_contained(candidate.range_extra(), stored.range_extra())
                && text_map_extras_preserved(candidate.segments(), stored.segments())
        }
        (
            MathContentSourceMap::Math {
                range: candidate_range,
                operands: candidate_operands,
                ..
            },
            MathContentSourceMap::Math {
                range: stored_range,
                operands: stored_operands,
                ..
            },
        ) => {
            extras_contained(candidate_range.extra(), stored_range.extra())
                && candidate_operands.len() == stored_operands.len()
                && candidate_operands
                    .iter()
                    .zip(stored_operands)
                    .all(|(candidate, stored)| source_map_extras_preserved(candidate, stored))
        }
        (
            MathContentSourceMap::Environment {
                range: candidate_range,
                children: candidate_children,
            },
            MathContentSourceMap::Environment {
                range: stored_range,
                children: stored_children,
            },
        ) => {
            extras_contained(candidate_range.extra(), stored_range.extra())
                && candidate_children.len() == stored_children.len()
                && candidate_children
                    .iter()
                    .zip(stored_children)
                    .all(|(candidate, stored)| content_map_extras_preserved(candidate, stored))
        }
        (MathContentSourceMap::Other, MathContentSourceMap::Other) => true,
        _ => false,
    }
}

fn text_map_extras_preserved(
    candidate: &[caret_math::MathTextSourceSegment],
    stored: &[caret_math::MathTextSourceSegment],
) -> bool {
    if candidate
        .iter()
        .all(|segment| segment.extra().is_empty() && segment.source_extra().is_empty())
    {
        return true;
    }
    let mut stored_by_position: std::collections::BTreeMap<_, Vec<_>> =
        std::collections::BTreeMap::new();
    for segment in stored {
        stored_by_position
            .entry(text_segment_position(segment))
            .or_default()
            .push(segment);
    }
    let mut occurrences = std::collections::BTreeMap::new();
    for segment in candidate {
        let position = text_segment_position(segment);
        let occurrence = occurrences.entry(position).or_insert(0usize);
        if !segment.extra().is_empty() || !segment.source_extra().is_empty() {
            let Some(stored) = stored_by_position
                .get(&position)
                .and_then(|segments| segments.get(*occurrence))
            else {
                return false;
            };
            if !extras_contained(segment.extra(), stored.extra())
                || !extras_contained(segment.source_extra(), stored.source_extra())
            {
                return false;
            }
        }
        *occurrence += 1;
    }
    true
}

fn text_segment_position(
    segment: &caret_math::MathTextSourceSegment,
) -> (usize, usize, Option<(usize, usize)>) {
    let display = segment.display();
    let source = segment.source().map(|range| (range.start, range.end));
    (display.start, display.end, source)
}

fn extras_contained(
    candidate: &std::collections::BTreeMap<String, serde_json::Value>,
    stored: &std::collections::BTreeMap<String, serde_json::Value>,
) -> bool {
    candidate
        .iter()
        .all(|(key, value)| stored.get(key) == Some(value))
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
    let projection = caret_math::parse_projection(&source, kind)
        .map_err(|error| format!("math {id} source does not parse: {error}"))?;
    let caret_math::ProjectedMath::Tree(parsed) = projection.into_parts().0 else {
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
    let projection = caret_math::parse_projection(&source, MathInputKind::Align)
        .map_err(|error| format!("align {} source does not parse: {error}", align.id))?;
    let caret_math::ProjectedMath::Align(parsed) = projection.into_parts().0 else {
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
    same_flow_structure(left, right)
}

fn same_flow_structure(left: &MathFlow, right: &MathFlow) -> bool {
    empty_tree_flow_matches_parsed_placeholder(&left.content, &right.content)
        || same_content_sequence_projection(&left.content, &right.content)
}

pub(crate) fn empty_tree_flow_matches_parsed_placeholder(
    left: &[MathContent],
    right: &[MathContent],
) -> bool {
    fn is_plain_placeholder(content: &[MathContent]) -> bool {
        matches!(
            content,
            [MathContent::Math(node)]
                if node.kind == MathNodeKind::Placeholder
                    && node.text.is_none()
                    && node.attributes.is_empty()
                    && node.children.is_empty()
                    && node.extra.is_empty()
        )
    }

    left.is_empty() && is_plain_placeholder(right)
}

fn same_content_sequence_projection(left: &[MathContent], right: &[MathContent]) -> bool {
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() && right_index < right.len() {
        if matches!(left[left_index], MathContent::Text(_))
            && matches!(right[right_index], MathContent::Text(_))
        {
            let left_end = text_group_end(left, left_index);
            let right_end = text_group_end(right, right_index);
            if left_end - left_index == right_end - right_index {
                if !left[left_index..left_end]
                    .iter()
                    .zip(&right[right_index..right_end])
                    .all(|(left, right)| same_content_projection(left, right))
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
        if !same_content_projection(&left[left_index], &right[right_index]) {
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

fn same_content_projection(left: &MathContent, right: &MathContent) -> bool {
    match (left, right) {
        (MathContent::Text(left), MathContent::Text(right)) => left
            .runs
            .iter()
            .flat_map(|run| run.text.chars())
            .eq(right.runs.iter().flat_map(|run| run.text.chars())),
        (MathContent::Math(left), MathContent::Math(right)) => same_node_projection(left, right),
        (MathContent::Environment(left), MathContent::Environment(right)) => {
            left.kind == right.kind
                && caret_math::source_environment_props_match(left.kind, &left.props, &right.props)
                && same_content_sequence_projection(&left.children, &right.children)
        }
        (MathContent::SoftBreak(_), MathContent::SoftBreak(_)) => true,
        (MathContent::Opaque(left), MathContent::Opaque(right)) => left.raw == right.raw,
        _ => false,
    }
}

fn same_node_projection(left: &MathNode, right: &MathNode) -> bool {
    left.kind == right.kind
        && left.text == right.text
        && left.attributes == right.attributes
        && left.children.len() == right.children.len()
        && left
            .children
            .iter()
            .zip(&right.children)
            .all(|(left, right)| same_flow_structure(left, right))
}

#[cfg(test)]
mod tests;
