//! 数学符号输入及其树结构辅助操作。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

pub(super) fn empty_math_flow(content: Vec<MathContent>) -> MathFlow {
    MathFlow {
        id: NodeId::generate(),
        content,
        extra: BTreeMap::new(),
    }
}

pub(super) fn placeholder_math_content() -> MathContent {
    MathContent::Math(MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Placeholder,
        text: None,
        attributes: BTreeMap::new(),
        children: Vec::new(),
        extra: BTreeMap::new(),
    })
}

pub(super) fn insert_math_script(
    tree: &mut MathTree,
    position: &mut MathTreePosition,
    script: &str,
    selected_base: Option<Vec<MathContent>>,
) -> Result<(), EditError> {
    let contents = tree_position::resolve_sequence_mut(tree, &position.path)
        .ok_or(EditError::InvalidPosition)?;
    if position.offset > contents.len() {
        return Err(EditError::InvalidPosition);
    }
    let selected_base = selected_base.filter(|items| !items.is_empty());
    if selected_base.is_none()
        && let Some(base_index) = position.offset.checked_sub(1)
        && let Some(MathContent::Math(node)) = contents.get_mut(base_index)
        && matches!(
            node.kind,
            MathNodeKind::Superscript | MathNodeKind::Subscript
        )
    {
        let (operand_index, insert_at) = match (script, node.kind) {
            ("^", MathNodeKind::Subscript) if node.children.len() == 2 => (2, None),
            ("_", MathNodeKind::Superscript) if node.children.len() == 2 => (1, Some(1)),
            _ => {
                return Err(EditError::InvalidMath(
                    "a script of the same kind already exists".to_owned(),
                ));
            }
        };
        let operand = empty_math_flow(Vec::new());
        if let Some(index) = insert_at {
            node.children.insert(index, operand);
        } else {
            node.children.push(operand);
        }
        node.kind = MathNodeKind::SubSup;
        position.path = child_sequence_path(
            &MathTreePosition {
                host: position.host.clone(),
                path: position.path.clone(),
                offset: position.offset,
                affinity: position.affinity,
            },
            MathSequenceStep::Operand {
                content_index: base_index,
                operand_index,
            },
        );
        position.offset = 0;
        return Ok(());
    }

    let base_index = if selected_base.is_some() {
        position.offset
    } else {
        position.offset.saturating_sub(1)
    };
    let base_contents = if let Some(selected) = selected_base {
        selected
    } else if base_index < position.offset {
        vec![contents.remove(base_index)]
    } else {
        vec![placeholder_math_content()]
    };
    let mut children = vec![empty_math_flow(base_contents), empty_math_flow(Vec::new())];
    let kind = if script == "^" {
        MathNodeKind::Superscript
    } else {
        MathNodeKind::Subscript
    };
    let operand_index = 1;
    contents.insert(
        base_index,
        MathContent::Math(MathNode {
            id: NodeId::generate(),
            kind,
            text: None,
            attributes: BTreeMap::new(),
            children: std::mem::take(&mut children),
            extra: BTreeMap::new(),
        }),
    );
    position.path = child_sequence_path(
        &MathTreePosition {
            host: position.host.clone(),
            path: position.path.clone(),
            offset: position.offset,
            affinity: position.affinity,
        },
        MathSequenceStep::Operand {
            content_index: base_index,
            operand_index,
        },
    );
    position.offset = 0;
    Ok(())
}

pub(super) fn open_math_group(
    tree: &mut MathTree,
    position: &mut MathTreePosition,
) -> Result<(), EditError> {
    let contents = tree_position::resolve_sequence_mut(tree, &position.path)
        .ok_or(EditError::InvalidPosition)?;
    if position.offset > contents.len() {
        return Err(EditError::InvalidPosition);
    }
    let content_index = position.offset;
    contents.insert(
        content_index,
        MathContent::Math(MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Group,
            text: None,
            attributes: BTreeMap::new(),
            children: vec![empty_math_flow(Vec::new())],
            extra: BTreeMap::new(),
        }),
    );
    position.path = child_sequence_path(
        &MathTreePosition {
            host: position.host.clone(),
            path: position.path.clone(),
            offset: position.offset,
            affinity: position.affinity,
        },
        MathSequenceStep::Operand {
            content_index,
            operand_index: 0,
        },
    );
    position.offset = 0;
    Ok(())
}

pub(super) fn close_math_group(
    tree: &MathTree,
    position: &mut MathTreePosition,
) -> Result<(), EditError> {
    let Some((
        MathSequenceStep::Operand {
            content_index,
            operand_index,
        },
        parent_steps,
    )) = position.path.steps().split_last()
    else {
        return Err(EditError::InvalidMath("unmatched closing brace".to_owned()));
    };
    let content_index = *content_index;
    let parent_path = MathSequencePath::new(parent_steps.to_vec());
    let parent_position = MathTreePosition {
        host: position.host.clone(),
        path: parent_path.clone(),
        offset: content_index,
        affinity: position.affinity,
    };
    let parent = parent_position
        .resolve(tree)
        .map_err(|_| EditError::InvalidPosition)?;
    let Some(MathContent::Math(node)) = parent.get(content_index) else {
        return Err(EditError::InvalidPosition);
    };
    let closes_group =
        node.kind == MathNodeKind::Group && *operand_index == 0 && node.children.len() == 1;
    let closes_script = matches!(
        node.kind,
        MathNodeKind::Superscript | MathNodeKind::Subscript | MathNodeKind::SubSup
    ) && *operand_index > 0
        && *operand_index < node.children.len();
    if !closes_group && !closes_script {
        return Err(EditError::InvalidMath("unmatched closing brace".to_owned()));
    }
    position.path = parent_path;
    position.offset = content_index
        .checked_add(1)
        .ok_or(EditError::InvalidPosition)?;
    Ok(())
}

pub(super) fn is_literal_math_input(grapheme: &str) -> bool {
    !matches!(
        grapheme.chars().next(),
        Some('\\' | '{' | '}' | '$' | '%' | '&' | '#' | '_' | '^' | '~')
    )
}

pub(super) fn script_base_parent_for_start_insertion(
    tree: &MathTree,
    position: &MathTreePosition,
) -> Option<(MathSequencePath, usize)> {
    if position.offset != 0 {
        return None;
    }
    let steps = position.path.steps();
    for (depth, step) in steps.iter().enumerate() {
        let MathSequenceStep::Operand {
            content_index,
            operand_index: 0,
        } = *step
        else {
            continue;
        };
        let parent_path = MathSequencePath::new(steps[..depth].to_vec());
        let parent_position = MathTreePosition {
            host: position.host.clone(),
            path: parent_path.clone(),
            offset: content_index,
            affinity: position.affinity,
        };
        let contents = parent_position.resolve(tree).ok()?;
        let Some(MathContent::Math(node)) = contents.get(content_index) else {
            continue;
        };
        if !matches!(
            node.kind,
            MathNodeKind::Superscript | MathNodeKind::Subscript | MathNodeKind::SubSup
        ) {
            continue;
        }
        if node.children.is_empty() {
            continue;
        }
        let mut base_path = steps[..=depth].to_vec();
        let mut base_position = MathTreePosition {
            host: position.host.clone(),
            path: MathSequencePath::new(base_path.clone()),
            offset: 0,
            affinity: position.affinity,
        };
        let mut base_contents = base_position.resolve(tree).ok()?;
        if base_contents.is_empty() || is_placeholder_at(base_contents, 0) {
            return None;
        }
        let mut nested_path_is_group_start = true;
        for nested_step in &steps[depth + 1..] {
            if *nested_step
                != (MathSequenceStep::Operand {
                    content_index: 0,
                    operand_index: 0,
                })
            {
                nested_path_is_group_start = false;
                break;
            }
            let Some(MathContent::Math(group)) = base_contents.first() else {
                nested_path_is_group_start = false;
                break;
            };
            if group.kind != MathNodeKind::Group || group.children.len() != 1 {
                nested_path_is_group_start = false;
                break;
            }
            base_path.push(*nested_step);
            base_position.path = MathSequencePath::new(base_path.clone());
            base_contents = base_position.resolve(tree).ok()?;
            if base_contents.is_empty() || is_placeholder_at(base_contents, 0) {
                return None;
            }
        }
        if nested_path_is_group_start {
            return Some((parent_path, content_index));
        }
    }
    None
}

fn is_placeholder_at(contents: &[MathContent], index: usize) -> bool {
    matches!(contents.get(index), Some(MathContent::Math(node)) if node.kind == MathNodeKind::Placeholder)
}

pub(super) fn flush_math_literal_batch(
    tree: &mut MathTree,
    position: &mut MathTreePosition,
    batch: &mut String,
) -> Result<(), EditError> {
    if batch.is_empty() {
        return Ok(());
    }
    insert_math_symbol_run(tree, position, batch)?;
    batch.clear();
    Ok(())
}

pub(super) fn insert_math_symbol_run(
    tree: &mut MathTree,
    position: &mut MathTreePosition,
    text: &str,
) -> Result<(), EditError> {
    let contents = tree_position::resolve_sequence_mut(tree, &position.path)
        .ok_or(EditError::InvalidPosition)?;
    let cursor = position.offset;
    let next_cursor = resegment_literal_symbol_run(
        contents,
        cursor,
        Some(text),
        MathPositionAffinity::Downstream,
    )?
    .ok_or(EditError::InvalidPosition)?;
    position.offset = next_cursor;
    Ok(())
}

pub(super) fn insert_math_symbol(
    tree: &mut MathTree,
    position: &mut MathTreePosition,
    grapheme: &str,
) -> Result<(), EditError> {
    let character = grapheme.chars().next().ok_or(EditError::InvalidPosition)?;
    let contents = tree_position::resolve_sequence_mut(tree, &position.path)
        .ok_or(EditError::InvalidPosition)?;
    if position.offset > contents.len() {
        return Err(EditError::InvalidPosition);
    }
    let index = position.offset;
    for (neighbor, inserted_first) in [(index.checked_sub(1), false), (Some(index), true)] {
        let Some(neighbor) = neighbor.filter(|neighbor| *neighbor < contents.len()) else {
            continue;
        };
        let Some(neighbor_text) = contents.get(neighbor).and_then(symbol_text) else {
            continue;
        };
        let joined = if inserted_first {
            format!("{grapheme}{neighbor_text}")
        } else {
            format!("{neighbor_text}{grapheme}")
        };
        if one_grapheme(&joined)
            && contents
                .get(neighbor)
                .is_some_and(symbol_has_opaque_metadata)
        {
            return Err(EditError::UnsupportedOpaqueContent);
        }
    }
    let left_index = index.checked_sub(1);
    let left_text = left_index
        .and_then(|left| contents.get(left))
        .and_then(symbol_text);
    let right_text = contents.get(index).and_then(symbol_text);
    let joined_left = left_text.map(|left| format!("{left}{grapheme}"));
    let merge_left = joined_left.as_deref().is_some_and(one_grapheme)
        && left_index
            .and_then(|left| contents.get(left))
            .is_some_and(|content| literal_symbol_text(content).is_some());
    if joined_left.as_deref().is_some_and(one_grapheme)
        && !merge_left
        && left_index
            .and_then(|left| contents.get(left))
            .is_some_and(symbol_has_opaque_metadata)
    {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    if merge_left {
        let joined = joined_left.ok_or(EditError::InvalidPosition)?;
        let joined_with_right = right_text.map(|right| format!("{joined}{right}"));
        let right_would_join = joined_with_right.as_deref().is_some_and(one_grapheme);
        let merge_right = right_would_join
            && contents
                .get(index)
                .is_some_and(|content| literal_symbol_text(content).is_some());
        if right_would_join
            && !merge_right
            && contents.get(index).is_some_and(symbol_has_opaque_metadata)
        {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let MathContent::Math(node) = contents
            .get_mut(left_index.ok_or(EditError::InvalidPosition)?)
            .ok_or(EditError::InvalidPosition)?
        else {
            return Err(EditError::InvalidPosition);
        };
        node.text = Some(if merge_right {
            joined_with_right.ok_or(EditError::InvalidPosition)?
        } else {
            joined
        });
        if let Some(first_character) = node.text.as_deref().and_then(|text| text.chars().next()) {
            node.attributes.insert(
                "atomClass".to_owned(),
                caret_math::symbols::literal_class(first_character)
                    .as_str()
                    .to_owned(),
            );
        }
        if merge_right {
            contents.remove(index);
        }
        position.offset = index;
        return Ok(());
    }
    if let Some(right_text) = right_text {
        let joined = format!("{grapheme}{right_text}");
        if one_grapheme(&joined) {
            let merge_right = contents
                .get(index)
                .is_some_and(|content| literal_symbol_text(content).is_some());
            if !merge_right && contents.get(index).is_some_and(symbol_has_opaque_metadata) {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            if merge_right {
                let MathContent::Math(node) =
                    contents.get_mut(index).ok_or(EditError::InvalidPosition)?
                else {
                    return Err(EditError::InvalidPosition);
                };
                let first_character = joined.chars().next().ok_or(EditError::InvalidPosition)?;
                node.text = Some(joined);
                node.attributes.insert(
                    "atomClass".to_owned(),
                    caret_math::symbols::literal_class(first_character)
                        .as_str()
                        .to_owned(),
                );
                position.offset = index + 1;
                return Ok(());
            }
        }
    }
    let atom_class = caret_math::symbols::literal_class(character);
    contents.insert(
        index,
        MathContent::Math(MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Symbol,
            text: Some(grapheme.to_owned()),
            attributes: BTreeMap::from([("atomClass".to_owned(), atom_class.as_str().to_owned())]),
            children: Vec::new(),
            extra: BTreeMap::new(),
        }),
    );
    position.offset = index + 1;
    Ok(())
}

pub(super) fn symbol_text(content: &MathContent) -> Option<&str> {
    match content {
        MathContent::Math(node)
            if node.kind == MathNodeKind::Symbol && node.children.is_empty() =>
        {
            node.text.as_deref()
        }
        _ => None,
    }
}

pub(super) fn resegment_literal_symbol_run(
    contents: &mut Vec<MathContent>,
    index: usize,
    insertion: Option<&str>,
    affinity: MathPositionAffinity,
) -> Result<Option<usize>, EditError> {
    if index > contents.len() {
        return Err(EditError::InvalidPosition);
    }
    reject_opaque_symbol_resegmentation(contents, index, insertion)?;
    let mut start = index;
    while start > 0
        && contents
            .get(start - 1)
            .and_then(literal_symbol_text)
            .is_some()
    {
        start -= 1;
    }
    let mut end = index;
    while end < contents.len() && contents.get(end).and_then(literal_symbol_text).is_some() {
        end += 1;
    }
    if insertion.is_none() && start == end {
        return Ok(None);
    }

    let mut source = String::new();
    let mut original = Vec::with_capacity(end - start);
    for content in &contents[start..index] {
        let text = literal_symbol_text(content).ok_or(EditError::InvalidPosition)?;
        let byte_start = source.len();
        source.push_str(text);
        original.push((
            symbol_id(content).ok_or(EditError::InvalidPosition)?,
            byte_start..source.len(),
        ));
    }
    let insertion_start = source.len();
    if let Some(insertion) = insertion {
        source.push_str(insertion);
    }
    let insertion_end = source.len();
    for content in &contents[index..end] {
        let text = literal_symbol_text(content).ok_or(EditError::InvalidPosition)?;
        let byte_start = source.len();
        source.push_str(text);
        original.push((
            symbol_id(content).ok_or(EditError::InvalidPosition)?,
            byte_start..source.len(),
        ));
    }
    let boundaries = GraphemeMap::new_with_fences(&source, &[]).into_boundaries();
    let graphemes = boundaries
        .windows(2)
        .filter_map(|range| source.get(range[0]..range[1]).map(str::to_owned))
        .collect::<Vec<_>>();
    let mut assigned_ids = vec![None; graphemes.len()];
    for (original_index, (_, original_range)) in original.iter().enumerate() {
        let first_output = boundaries
            .partition_point(|boundary| *boundary <= original_range.start)
            .saturating_sub(1);
        let output_end = boundaries.partition_point(|boundary| *boundary < original_range.end);
        let best_output = (first_output..output_end)
            .filter_map(|output_index| {
                let range = &boundaries[output_index..=output_index + 1];
                let overlap = original_range
                    .end
                    .min(range[1])
                    .saturating_sub(original_range.start.max(range[0]));
                (overlap > 0).then_some((output_index, overlap))
            })
            .max_by_key(|(output_index, overlap)| {
                let range = &boundaries[*output_index..=*output_index + 1];
                let original_center = original_range.start + original_range.end;
                let output_center = range[0] + range[1];
                (
                    *overlap,
                    std::cmp::Reverse(original_center.abs_diff(output_center)),
                )
            })
            .map(|(output_index, _)| output_index);
        if let Some(output_index) = best_output
            && assigned_ids[output_index].is_none()
        {
            assigned_ids[output_index] = Some(original_index);
        }
    }
    let mut replacement = Vec::with_capacity(graphemes.len());
    for (output_index, grapheme) in graphemes.into_iter().enumerate() {
        let character = grapheme.chars().next().ok_or(EditError::InvalidPosition)?;
        let id = assigned_ids[output_index]
            .and_then(|original_index| original.get(original_index).map(|(id, _)| (*id).clone()))
            .unwrap_or_else(NodeId::generate);
        let attributes = BTreeMap::from([(
            "atomClass".to_owned(),
            caret_math::symbols::literal_class(character)
                .as_str()
                .to_owned(),
        )]);
        let node = MathNode {
            id,
            kind: MathNodeKind::Symbol,
            text: Some(grapheme),
            attributes,
            children: Vec::new(),
            extra: BTreeMap::new(),
        };
        replacement.push(MathContent::Math(node));
    }
    contents.splice(start..end, replacement);

    let target_byte = if insertion.is_some() {
        insertion_end
    } else {
        insertion_start
    };
    let caret_byte = match affinity {
        MathPositionAffinity::Upstream => boundaries
            .iter()
            .copied()
            .take_while(|boundary| *boundary <= target_byte)
            .last()
            .unwrap_or(0),
        MathPositionAffinity::Downstream => boundaries
            .iter()
            .copied()
            .find(|boundary| *boundary >= target_byte)
            .unwrap_or_else(|| boundaries.last().copied().unwrap_or(0)),
    };
    Ok(Some(
        start + boundaries.binary_search(&caret_byte).unwrap_or(0),
    ))
}

fn reject_opaque_symbol_resegmentation(
    contents: &[MathContent],
    index: usize,
    insertion: Option<&str>,
) -> Result<(), EditError> {
    let mut start = index;
    while start > 0
        && contents
            .get(start - 1)
            .and_then(symbol_text)
            .is_some_and(one_grapheme)
    {
        start -= 1;
    }
    let mut end = index;
    while end < contents.len()
        && contents
            .get(end)
            .and_then(symbol_text)
            .is_some_and(one_grapheme)
    {
        end += 1;
    }
    if start == end && insertion.is_none() {
        return Ok(());
    }

    let mut source = String::new();
    let mut protected_ranges = Vec::new();
    for content in &contents[start..index] {
        let text = symbol_text(content).ok_or(EditError::InvalidPosition)?;
        let range_start = source.len();
        source.push_str(text);
        if symbol_has_opaque_metadata(content) {
            protected_ranges.push(range_start..source.len());
        }
    }
    if let Some(insertion) = insertion {
        source.push_str(insertion);
    }
    for content in &contents[index..end] {
        let text = symbol_text(content).ok_or(EditError::InvalidPosition)?;
        let range_start = source.len();
        source.push_str(text);
        if symbol_has_opaque_metadata(content) {
            protected_ranges.push(range_start..source.len());
        }
    }

    let boundaries = GraphemeMap::new_with_fences(&source, &[]).into_boundaries();
    if protected_ranges.iter().any(|range| {
        boundaries.binary_search(&range.start).is_err()
            || boundaries.binary_search(&range.end).is_err()
    }) {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    Ok(())
}

fn symbol_id(content: &MathContent) -> Option<&NodeId> {
    match content {
        MathContent::Math(node) if node.kind == MathNodeKind::Symbol => Some(&node.id),
        _ => None,
    }
}

pub(super) fn literal_symbol_text(content: &MathContent) -> Option<&str> {
    let MathContent::Math(node) = content else {
        return None;
    };
    let text = symbol_text(content)?;
    let mut characters = text.chars();
    let first = characters.next()?;
    if !one_grapheme(text)
        || matches!(
            first,
            '\\' | '{' | '}' | '$' | '%' | '&' | '#' | '_' | '^' | '~'
        )
        || !node.extra.is_empty()
        || node.attributes.len() != 1
        || node.attributes.get("atomClass").map(String::as_str)
            != Some(caret_math::symbols::literal_class(first).as_str())
    {
        return None;
    }
    Some(text)
}

fn symbol_has_opaque_metadata(content: &MathContent) -> bool {
    let MathContent::Math(node) = content else {
        return false;
    };
    node.kind == MathNodeKind::Symbol
        && (!node.extra.is_empty()
            || node.attributes.iter().any(|(key, value)| {
                key != "atomClass"
                    || !matches!(
                        value.as_str(),
                        "ord" | "op" | "bin" | "rel" | "open" | "close" | "punct" | "inner"
                    )
            }))
}

fn one_grapheme(text: &str) -> bool {
    GraphemeMap::new_with_fences(text, &[])
        .into_boundaries()
        .len()
        == 2
}

#[cfg(test)]
pub(super) fn display_math_block_with_id(
    align: bool,
    source: &str,
    id: NodeId,
) -> Result<Block, EditError> {
    display_math_block_with_projection(align, source, id).map(|(block, _)| block)
}

#[cfg(test)]
pub(super) fn display_math_block_with_projection(
    align: bool,
    source: &str,
    id: NodeId,
) -> Result<(Block, caret_math::MathSourceMap<'static>), EditError> {
    if align {
        let full = format!("{ALIGN_BEGIN}{source}{ALIGN_END}");
        let projection = caret_math::parse_projection(&full, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let (content, source_projection) = projection.into_parts();
        let caret_math::ProjectedMath::Align(parsed) = content else {
            return Err(EditError::InvalidMath("expected aligned math".to_owned()));
        };
        return Ok((Block::Align(Align { id, ..parsed }), source_projection));
    }
    let projection = caret_math::parse_projection(source, caret_math::MathInputKind::Equation)
        .map_err(|error| EditError::InvalidMath(error.to_string()))?;
    let (content, source_projection) = projection.into_parts();
    let caret_math::ProjectedMath::Tree(math_tree) = content else {
        return Err(EditError::InvalidMath("expected equation".to_owned()));
    };
    let block = Block::Equation(Equation {
        id,
        math_tree,
        extra: Default::default(),
    });
    Ok((block, source_projection))
}

pub(super) fn changed_source_range(
    previous: &str,
    next: &str,
) -> Option<(Range<usize>, Range<usize>)> {
    let mut prefix = previous
        .bytes()
        .zip(next.bytes())
        .take_while(|(left, right)| left == right)
        .count();
    while !previous.is_char_boundary(prefix) || !next.is_char_boundary(prefix) {
        prefix = prefix.checked_sub(1)?;
    }
    let remaining_previous = previous.len().checked_sub(prefix)?;
    let remaining_next = next.len().checked_sub(prefix)?;
    let mut suffix = previous[previous.len() - remaining_previous..]
        .bytes()
        .rev()
        .zip(next[next.len() - remaining_next..].bytes().rev())
        .take_while(|(left, right)| left == right)
        .count();
    while suffix > 0
        && (!previous.is_char_boundary(previous.len() - suffix)
            || !next.is_char_boundary(next.len() - suffix))
    {
        suffix -= 1;
    }
    Some((
        prefix..previous.len().checked_sub(suffix)?,
        prefix..next.len().checked_sub(suffix)?,
    ))
}

pub(super) fn absolute_source_edit(
    projection: &caret_math::MathSourceMap<'_>,
    edit: &Range<usize>,
) -> Option<Range<usize>> {
    if edit.start > edit.end || edit.end > projection.edit_source().len() {
        return None;
    }
    Some(
        projection.edit_range().start.checked_add(edit.start)?
            ..projection.edit_range().start.checked_add(edit.end)?,
    )
}
