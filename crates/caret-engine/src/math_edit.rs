//! 在 Caret 自有公式树上执行原子数学编辑。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use crate::editing::MathPos;
use caret_doc::{MathNode, MathNodeKind, MathTree, NodeId};
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use unicode_segmentation::UnicodeSegmentation;

#[cfg(test)]
mod tests;

/// 公式树编辑操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathEdit {
    /// 在普通符号、文本内容或空参数槽输入字符。
    InsertText(String),
    /// 在 Row 子节点边界插入分数并进入分子槽。
    InsertFraction,
    /// 在 Row 子节点边界插入平方根并进入被开方数槽。
    InsertRadical,
}

/// 单次编辑后的新树与公式内插入点。
#[derive(Debug, Clone, PartialEq)]
pub struct MathEditResult {
    pub tree: MathTree,
    pub position: MathPos,
}

/// 数学树编辑被拒绝；调用方原树不会被修改。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathEditError {
    InvalidPosition,
    UnsupportedTarget,
    InvalidText,
    InvalidTree,
}

impl Display for MathEditError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPosition => "math.edit_invalid_position",
            Self::UnsupportedTarget => "math.edit_unsupported_target",
            Self::InvalidText => "math.edit_invalid_text",
            Self::InvalidTree => "math.edit_invalid_tree",
        })
    }
}

impl std::error::Error for MathEditError {}

/// 根据已验证的 `MathPos` 生成编辑后的新树，不在失败前修改调用方数据。
pub fn apply_math_edit(
    tree: &MathTree,
    position: &MathPos,
    edit: MathEdit,
) -> Result<MathEditResult, MathEditError> {
    validate_editable_tree(tree)?;
    if position.resolve(tree).is_none() {
        return Err(MathEditError::InvalidPosition);
    }
    match &edit {
        MathEdit::InsertText(text) => {
            if text.len() > caret_doc::MAX_MATH_SOURCE_BYTES {
                return Err(MathEditError::InvalidText);
            }
        }
        MathEdit::InsertFraction | MathEdit::InsertRadical => {}
    }
    let mut candidate = tree.clone();
    match edit {
        MathEdit::InsertText(text) => insert_text(&mut candidate, position, &text),
        MathEdit::InsertFraction => insert_structure(
            &mut candidate,
            position,
            MathNodeKind::Fraction,
            vec![MathNodeKind::Placeholder, MathNodeKind::Placeholder],
            Some((0, 0)),
        ),
        MathEdit::InsertRadical => insert_structure(
            &mut candidate,
            position,
            MathNodeKind::Radical,
            vec![MathNodeKind::Placeholder, MathNodeKind::Placeholder],
            Some((1, 0)),
        ),
    }
}

fn insert_text(
    tree: &mut MathTree,
    position: &MathPos,
    text: &str,
) -> Result<MathEditResult, MathEditError> {
    if text.is_empty()
        || text.len() > caret_doc::MAX_MATH_SOURCE_BYTES
        || text.chars().any(char::is_control)
    {
        return Err(MathEditError::InvalidText);
    }
    let path = &position.path;
    let parent_kind = parent_at_path(tree, path).map(|parent| parent.kind);
    let matrix_row = is_matrix_environment_row(tree, path);
    let target = node_at_path(&tree.root, path).ok_or(MathEditError::InvalidPosition)?;
    let target_is_ordinary_symbol = target.kind == MathNodeKind::Symbol
        && target.attributes.get("atomClass").map(String::as_str) == Some("ord");
    if (matches!(
        target.kind,
        MathNodeKind::Symbol
            | MathNodeKind::Operator
            | MathNodeKind::TextSymbol
            | MathNodeKind::SizedDelimiter
            | MathNodeKind::TextSpace
            | MathNodeKind::Space
            | MathNodeKind::Strut
    ) && !target_is_ordinary_symbol)
        || (target_is_ordinary_symbol && !is_ordinary_math_text(text))
    {
        return insert_adjacent_nodes(tree, position, parse_math_input_nodes(text)?);
    }
    let node = node_at_path_mut(&mut tree.root, path).ok_or(MathEditError::InvalidPosition)?;
    match node.kind {
        MathNodeKind::Placeholder => {
            let text_slot = matches!(
                parent_kind,
                Some(MathNodeKind::Text | MathNodeKind::TextStyle)
            );
            if !node.extra.is_empty() || node.source_range.is_some() {
                return Err(MathEditError::UnsupportedTarget);
            }
            if !text_slot {
                let mut nodes = parse_math_input_nodes(text)?;
                if nodes.len() == 1 {
                    let mut replacement = nodes.remove(0);
                    replacement.id.clone_from(&node.id);
                    *node = replacement;
                    let focus = last_node_position(node, path.clone());
                    return finish_edit(tree, focus);
                }
                node.kind = MathNodeKind::Row;
                node.text = None;
                node.attributes.clear();
                node.children = nodes;
                let focus = last_node_position(node, path.clone());
                return finish_edit(tree, focus);
            }
            node.kind = if text_slot {
                MathNodeKind::TextContent
            } else {
                MathNodeKind::Symbol
            };
            node.text = Some(text.to_owned());
            node.attributes = if text_slot {
                BTreeMap::new()
            } else {
                BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())])
            };
            finish_edit(
                tree,
                MathPos {
                    path: path.clone(),
                    offset: text.len(),
                },
            )
        }
        MathNodeKind::Symbol
            if node.attributes.get("atomClass").map(String::as_str) == Some("ord") =>
        {
            ensure_source_range_can_be_invalidated(node)?;
            let value = node.text.as_mut().ok_or(MathEditError::UnsupportedTarget)?;
            value.insert_str(position.offset, text);
            node.source_range = None;
            finish_edit(
                tree,
                MathPos {
                    path: path.clone(),
                    offset: position.offset + text.len(),
                },
            )
        }
        MathNodeKind::TextContent => {
            ensure_source_range_can_be_invalidated(node)?;
            let value = node.text.as_mut().ok_or(MathEditError::UnsupportedTarget)?;
            value.insert_str(position.offset, text);
            node.source_range = None;
            finish_edit(
                tree,
                MathPos {
                    path: path.clone(),
                    offset: position.offset + text.len(),
                },
            )
        }
        MathNodeKind::Row | MathNodeKind::Text | MathNodeKind::TextStyle => {
            if node.kind == MathNodeKind::Row && matrix_row {
                return Err(MathEditError::UnsupportedTarget);
            }
            let index = position.offset.min(node.children.len());
            let nodes = if matches!(node.kind, MathNodeKind::Text | MathNodeKind::TextStyle) {
                vec![text_node(node.kind, text)]
            } else {
                parse_math_input_nodes(text)?
            };
            let inserted = nodes.last().ok_or(MathEditError::InvalidText)?;
            let mut inserted_path = path.clone();
            inserted_path.push(inserted.id.clone());
            let focus = last_node_position(inserted, inserted_path);
            node.children.splice(index..index, nodes);
            finish_edit(tree, focus)
        }
        _ => Err(MathEditError::UnsupportedTarget),
    }
}

fn ensure_source_range_can_be_invalidated(node: &MathNode) -> Result<(), MathEditError> {
    if node
        .source_range
        .as_ref()
        .is_some_and(|source_range| !source_range.extra.is_empty())
    {
        return Err(MathEditError::UnsupportedTarget);
    }
    Ok(())
}

fn parse_math_input_nodes(text: &str) -> Result<Vec<MathNode>, MathEditError> {
    let parsed = caret_math::parse(text, caret_math::MathInputKind::Inline)
        .map_err(|_| MathEditError::InvalidText)?;
    let imported = crate::import_parsed_math(&parsed).map_err(|_| MathEditError::InvalidText)?;
    let crate::ImportedMath::Inline(imported) = imported else {
        return Err(MathEditError::InvalidText);
    };
    let mut nodes = if imported.math_tree.root.kind == MathNodeKind::Row {
        imported.math_tree.root.children
    } else {
        vec![imported.math_tree.root]
    };
    let mut pending = nodes.iter_mut().collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        node.source_range = None;
        pending.extend(node.children.iter_mut());
    }
    if nodes.is_empty() {
        return Err(MathEditError::InvalidText);
    }
    Ok(nodes)
}

fn insert_adjacent_nodes(
    tree: &mut MathTree,
    position: &MathPos,
    nodes: Vec<MathNode>,
) -> Result<MathEditResult, MathEditError> {
    if let Some(target) = node_at_path(&tree.root, &position.path)
        && target.kind == MathNodeKind::Symbol
        && target.attributes.get("atomClass").map(String::as_str) == Some("ord")
        && target
            .text
            .as_deref()
            .is_some_and(|text| position.offset > 0 && position.offset < text.len())
    {
        let last_node = nodes.last().ok_or(MathEditError::InvalidText)?;
        let focus = last_node_position(last_node, vec![last_node.id.clone()]);
        return insert_between_symbol_halves(tree, position, nodes, focus.path, focus.offset);
    }
    let (parent_path, child_index, after_atom) =
        atomic_insertion_location(tree, position).ok_or(MathEditError::UnsupportedTarget)?;
    let parent = node_at_path(&tree.root, &parent_path).ok_or(MathEditError::InvalidPosition)?;
    if parent.kind == MathNodeKind::Row && is_matrix_environment_row(tree, &parent_path) {
        return Err(MathEditError::UnsupportedTarget);
    }
    if !matches!(
        parent.kind,
        MathNodeKind::Row | MathNodeKind::Fraction | MathNodeKind::Radical
    ) {
        return Err(MathEditError::UnsupportedTarget);
    }
    let last_id = nodes
        .last()
        .map(|node| node.id.clone())
        .ok_or(MathEditError::InvalidText)?;
    let parent =
        node_at_path_mut(&mut tree.root, &parent_path).ok_or(MathEditError::InvalidPosition)?;
    let wrapper_id = if matches!(parent.kind, MathNodeKind::Fraction | MathNodeKind::Radical) {
        let atom = parent
            .children
            .get(child_index)
            .cloned()
            .ok_or(MathEditError::InvalidPosition)?;
        let mut children = nodes;
        if after_atom {
            children.insert(0, atom);
        } else {
            children.push(atom);
        }
        let row_id = NodeId::generate();
        parent.children[child_index] = MathNode {
            id: row_id.clone(),
            kind: MathNodeKind::Row,
            text: None,
            attributes: BTreeMap::new(),
            source_range: None,
            children,
            extra: BTreeMap::new(),
        };
        Some(row_id)
    } else {
        let insertion_index = child_index + usize::from(after_atom);
        parent
            .children
            .splice(insertion_index..insertion_index, nodes);
        None
    };
    let mut result_path = parent_path;
    if let Some(wrapper_id) = wrapper_id {
        result_path.push(wrapper_id);
    }
    result_path.push(last_id);
    let last_node = node_at_path(&tree.root, &result_path).ok_or(MathEditError::InvalidPosition)?;
    finish_edit(tree, last_node_position(last_node, result_path))
}

fn insert_between_symbol_halves(
    tree: &mut MathTree,
    position: &MathPos,
    nodes: Vec<MathNode>,
    focus_path_suffix: Vec<NodeId>,
    focus_offset: usize,
) -> Result<MathEditResult, MathEditError> {
    let target = node_at_path(&tree.root, &position.path).ok_or(MathEditError::InvalidPosition)?;
    if !target.extra.is_empty() {
        return Err(MathEditError::UnsupportedTarget);
    }
    ensure_source_range_can_be_invalidated(target)?;
    let original_text = target
        .text
        .as_deref()
        .ok_or(MathEditError::UnsupportedTarget)?;
    let (left_text, right_text) = original_text.split_at(position.offset);
    let mut left = target.clone();
    left.text = Some(left_text.to_owned());
    left.source_range = None;
    let right = MathNode {
        id: NodeId::generate(),
        kind: target.kind,
        text: Some(right_text.to_owned()),
        attributes: target.attributes.clone(),
        source_range: None,
        children: Vec::new(),
        extra: BTreeMap::new(),
    };

    let (target_id, parent_path) = position
        .path
        .split_last()
        .ok_or(MathEditError::InvalidPosition)?;
    let parent = node_at_path(&tree.root, parent_path).ok_or(MathEditError::InvalidPosition)?;
    if parent.kind == MathNodeKind::Row && is_matrix_environment_row(tree, parent_path) {
        return Err(MathEditError::UnsupportedTarget);
    }
    if !matches!(
        parent.kind,
        MathNodeKind::Row | MathNodeKind::Fraction | MathNodeKind::Radical
    ) {
        return Err(MathEditError::UnsupportedTarget);
    }
    let child_index = parent
        .children
        .iter()
        .position(|child| &child.id == target_id)
        .ok_or(MathEditError::InvalidPosition)?;
    let parent =
        node_at_path_mut(&mut tree.root, parent_path).ok_or(MathEditError::InvalidPosition)?;
    let mut result_path = parent_path.to_vec();
    if parent.kind == MathNodeKind::Row {
        parent.children[child_index] = left;
        let insertion_index = child_index + 1;
        let mut inserted = nodes;
        inserted.push(right);
        parent
            .children
            .splice(insertion_index..insertion_index, inserted);
        result_path.extend(focus_path_suffix);
    } else {
        let wrapper_id = NodeId::generate();
        let mut children = vec![left];
        children.extend(nodes);
        children.push(right);
        parent.children[child_index] = MathNode {
            id: wrapper_id.clone(),
            kind: MathNodeKind::Row,
            text: None,
            attributes: BTreeMap::new(),
            source_range: None,
            children,
            extra: BTreeMap::new(),
        };
        result_path.push(wrapper_id);
        result_path.extend(focus_path_suffix);
    }
    finish_edit(
        tree,
        MathPos {
            path: result_path,
            offset: focus_offset,
        },
    )
}

fn last_node_position(node: &MathNode, path: Vec<NodeId>) -> MathPos {
    match node.kind {
        MathNodeKind::Symbol
            if node.attributes.get("atomClass").map(String::as_str) == Some("ord") =>
        {
            MathPos {
                path,
                offset: node.text.as_deref().unwrap_or_default().len(),
            }
        }
        MathNodeKind::TextContent => MathPos {
            path,
            offset: node.text.as_deref().unwrap_or_default().len(),
        },
        MathNodeKind::Placeholder => MathPos { path, offset: 0 },
        MathNodeKind::Symbol
        | MathNodeKind::Operator
        | MathNodeKind::TextSymbol
        | MathNodeKind::SizedDelimiter
        | MathNodeKind::TextSpace
        | MathNodeKind::Space
        | MathNodeKind::Strut => MathPos { path, offset: 1 },
        _ if !node.children.is_empty() => {
            let child = &node.children[node.children.len() - 1];
            let mut child_path = path;
            child_path.push(child.id.clone());
            last_node_position(child, child_path)
        }
        _ => MathPos {
            path,
            offset: node.children.len(),
        },
    }
}

pub(crate) fn is_ordinary_math_text(text: &str) -> bool {
    let Ok(parsed) = caret_math::parse(text, caret_math::MathInputKind::Inline) else {
        return false;
    };
    let Ok(syntax) = parsed.syntax_tree() else {
        return false;
    };
    let is_ordinary_symbol = |node: &caret_math::MathSyntaxNode| {
        node.kind == caret_math::MathSyntaxKind::Symbol
            && node.attributes.get("atomClass").map(String::as_str) == Some("ord")
            && node.children.is_empty()
    };
    let is_ordinary = is_ordinary_symbol(&syntax.root)
        || (syntax.root.kind == caret_math::MathSyntaxKind::Row
            && !syntax.root.children.is_empty()
            && syntax.root.children.iter().all(is_ordinary_symbol));
    let parsed_text = if syntax.root.kind == caret_math::MathSyntaxKind::Symbol {
        syntax.root.text.as_deref().unwrap_or_default().to_owned()
    } else {
        syntax
            .root
            .children
            .iter()
            .filter_map(|node| node.text.as_deref())
            .collect::<String>()
    };
    is_ordinary && parsed_text == text
}

fn validate_editable_tree(tree: &MathTree) -> Result<(), MathEditError> {
    tree.validate().map_err(|_| MathEditError::InvalidTree)?;
    let root_row_wrapper = needs_parser_row_wrapper_for_root(&tree.root);
    let mut pending = vec![(&tree.root, usize::from(root_row_wrapper), None)];
    let mut parser_nodes = usize::from(root_row_wrapper);
    while let Some((node, depth, parent_kind)) = pending.pop() {
        if depth >= caret_math::MAX_MATH_NESTING_DEPTH {
            return Err(MathEditError::InvalidTree);
        }
        parser_nodes = parser_nodes.saturating_add(1);
        if node.kind == MathNodeKind::Symbol
            && node.attributes.get("atomClass").map(String::as_str) == Some("ord")
        {
            let text = node.text.as_deref().ok_or(MathEditError::InvalidTree)?;
            parser_nodes = parser_nodes.saturating_add(text.chars().count().saturating_sub(1));
        }
        if parser_nodes > caret_math::MAX_MATH_SYNTAX_NODES {
            return Err(MathEditError::InvalidTree);
        }
        for child in &node.children {
            let wrapped_in_row = needs_parser_row_wrapper(node.kind, child, parent_kind);
            if wrapped_in_row {
                parser_nodes = parser_nodes.saturating_add(1);
                if parser_nodes > caret_math::MAX_MATH_SYNTAX_NODES {
                    return Err(MathEditError::InvalidTree);
                }
            }
            pending.push((
                child,
                depth + 1 + usize::from(wrapped_in_row),
                Some(node.kind),
            ));
        }
    }
    Ok(())
}

fn needs_parser_row_wrapper_for_root(root: &MathNode) -> bool {
    root.kind == MathNodeKind::Symbol
        && root.attributes.get("atomClass").map(String::as_str) == Some("ord")
        && root
            .text
            .as_deref()
            .is_some_and(|text| text.chars().count() > 1)
}

fn needs_parser_row_wrapper(
    parent: MathNodeKind,
    child: &MathNode,
    grandparent: Option<MathNodeKind>,
) -> bool {
    (parent != MathNodeKind::Row || grandparent == Some(MathNodeKind::Matrix))
        && child.kind == MathNodeKind::Symbol
        && child.attributes.get("atomClass").map(String::as_str) == Some("ord")
        && child
            .text
            .as_deref()
            .is_some_and(|text| text.chars().count() > 1)
}

fn insert_structure(
    tree: &mut MathTree,
    position: &MathPos,
    kind: MathNodeKind,
    slots: Vec<MathNodeKind>,
    focus_slot: Option<(usize, usize)>,
) -> Result<MathEditResult, MathEditError> {
    if let Some(target) = node_at_path(&tree.root, &position.path)
        && target.kind == MathNodeKind::Symbol
        && target.attributes.get("atomClass").map(String::as_str) == Some("ord")
        && target
            .text
            .as_deref()
            .is_some_and(|text| position.offset > 0 && position.offset < text.len())
    {
        let (structure, focus_id, focus_offset) = new_structure(kind, slots, focus_slot)?;
        let structure_id = structure.id.clone();
        return insert_between_symbol_halves(
            tree,
            position,
            vec![structure],
            vec![structure_id, focus_id],
            focus_offset,
        );
    }
    if let Some((parent_path, child_index, after_atom)) = atomic_insertion_location(tree, position)
    {
        let parent =
            node_at_path(&tree.root, &parent_path).ok_or(MathEditError::InvalidPosition)?;
        if parent.kind == MathNodeKind::Row && is_matrix_environment_row(tree, &parent_path) {
            return Err(MathEditError::UnsupportedTarget);
        }
        if !matches!(
            parent.kind,
            MathNodeKind::Row | MathNodeKind::Fraction | MathNodeKind::Radical
        ) {
            return Err(MathEditError::UnsupportedTarget);
        }
        let (structure, focus_id, focus_offset) = new_structure(kind, slots, focus_slot)?;
        let structure_id = structure.id.clone();
        let parent =
            node_at_path_mut(&mut tree.root, &parent_path).ok_or(MathEditError::InvalidPosition)?;
        let wrapper_id = if matches!(parent.kind, MathNodeKind::Fraction | MathNodeKind::Radical) {
            let atom = parent
                .children
                .get(child_index)
                .cloned()
                .ok_or(MathEditError::InvalidPosition)?;
            let row_id = NodeId::generate();
            let children = if after_atom {
                vec![atom, structure]
            } else {
                vec![structure, atom]
            };
            parent.children[child_index] = MathNode {
                id: row_id.clone(),
                kind: MathNodeKind::Row,
                text: None,
                attributes: BTreeMap::new(),
                source_range: None,
                children,
                extra: BTreeMap::new(),
            };
            Some(row_id)
        } else {
            parent
                .children
                .insert(child_index + usize::from(after_atom), structure);
            None
        };
        let mut result_path = parent_path;
        if let Some(wrapper_id) = wrapper_id {
            result_path.push(wrapper_id);
        }
        result_path.push(structure_id);
        result_path.push(focus_id);
        return finish_edit(
            tree,
            MathPos {
                path: result_path,
                offset: focus_offset,
            },
        );
    }
    let parent_kind = parent_at_path(tree, &position.path).map(|parent| parent.kind);
    let matrix_row = is_matrix_environment_row(tree, &position.path);
    let node =
        node_at_path_mut(&mut tree.root, &position.path).ok_or(MathEditError::InvalidPosition)?;
    if node.kind == MathNodeKind::Placeholder {
        if parent_kind == Some(MathNodeKind::Matrix)
            || !node.extra.is_empty()
            || node.source_range.is_some()
        {
            return Err(MathEditError::UnsupportedTarget);
        }
        let (structure, focus_id, focus_offset) = new_structure(kind, slots, focus_slot)?;
        node.kind = kind;
        node.text = None;
        node.attributes = structure.attributes;
        node.children = structure.children;
        let mut result_path = position.path.clone();
        result_path.push(focus_id);
        return finish_edit(
            tree,
            MathPos {
                path: result_path,
                offset: focus_offset,
            },
        );
    }
    if node.kind != MathNodeKind::Row || matrix_row || position.offset > node.children.len() {
        return Err(MathEditError::UnsupportedTarget);
    }
    let insertion_index = position.offset;
    let (structure, focus_id, focus_offset) = new_structure(kind, slots, focus_slot)?;
    let structure_id = structure.id.clone();
    node.children.insert(insertion_index, structure);
    let mut result_path = position.path.clone();
    result_path.push(structure_id);
    result_path.push(focus_id);
    finish_edit(
        tree,
        MathPos {
            path: result_path,
            offset: focus_offset,
        },
    )
}

fn atomic_insertion_location(
    tree: &MathTree,
    position: &MathPos,
) -> Option<(Vec<NodeId>, usize, bool)> {
    let node = node_at_path(&tree.root, &position.path)?;
    let edge = match node.kind {
        MathNodeKind::Symbol
            if node.attributes.get("atomClass").map(String::as_str) == Some("ord") =>
        {
            let text = node.text.as_deref()?;
            if position.offset == 0 {
                false
            } else if position.offset == text.len() {
                true
            } else {
                return None;
            }
        }
        MathNodeKind::Symbol
        | MathNodeKind::Operator
        | MathNodeKind::TextSymbol
        | MathNodeKind::SizedDelimiter
        | MathNodeKind::TextSpace
        | MathNodeKind::Space
        | MathNodeKind::Strut => match position.offset {
            0 => false,
            1 => true,
            _ => return None,
        },
        _ => return None,
    };
    let (child_id, parent_path) = position.path.split_last()?;
    let parent = node_at_path(&tree.root, parent_path)?;
    if !matches!(
        parent.kind,
        MathNodeKind::Row | MathNodeKind::Fraction | MathNodeKind::Radical
    ) {
        return None;
    }
    let child_index = parent
        .children
        .iter()
        .position(|child| &child.id == child_id)?;
    Some((parent_path.to_vec(), child_index, edge))
}

fn new_structure(
    kind: MathNodeKind,
    slots: Vec<MathNodeKind>,
    focus_slot: Option<(usize, usize)>,
) -> Result<(MathNode, NodeId, usize), MathEditError> {
    let children = slots.into_iter().map(empty_node).collect::<Vec<_>>();
    let (focus_index, focus_offset) = focus_slot.ok_or(MathEditError::UnsupportedTarget)?;
    let focus_id = children
        .get(focus_index)
        .map(|child| child.id.clone())
        .ok_or(MathEditError::UnsupportedTarget)?;
    let attributes = if kind == MathNodeKind::Fraction {
        BTreeMap::from([("style".to_owned(), "frac".to_owned())])
    } else {
        BTreeMap::new()
    };
    Ok((
        MathNode {
            id: NodeId::generate(),
            kind,
            text: None,
            attributes,
            source_range: None,
            children,
            extra: BTreeMap::new(),
        },
        focus_id,
        focus_offset,
    ))
}

fn finish_edit(tree: &MathTree, mut position: MathPos) -> Result<MathEditResult, MathEditError> {
    let node = node_at_path(&tree.root, &position.path).ok_or(MathEditError::InvalidPosition)?;
    if node.kind == MathNodeKind::TextContent
        || (node.kind == MathNodeKind::Symbol
            && node.attributes.get("atomClass").map(String::as_str) == Some("ord"))
    {
        let text = node.text.as_deref().ok_or(MathEditError::InvalidText)?;
        let boundaries = text
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .chain(std::iter::once(text.len()))
            .collect::<Vec<_>>();
        if !boundaries.contains(&position.offset) {
            position.offset = boundaries
                .into_iter()
                .find(|offset| *offset > position.offset)
                .ok_or(MathEditError::InvalidText)?;
        }
    }
    if position.resolve(tree).is_none() {
        return Err(MathEditError::InvalidText);
    }
    validate_editable_tree(tree)?;
    Ok(MathEditResult {
        tree: tree.clone(),
        position,
    })
}

fn empty_node(kind: MathNodeKind) -> MathNode {
    MathNode {
        id: NodeId::generate(),
        kind,
        text: None,
        attributes: BTreeMap::new(),
        source_range: None,
        children: Vec::new(),
        extra: BTreeMap::new(),
    }
}

fn text_node(parent_kind: MathNodeKind, text: &str) -> MathNode {
    match parent_kind {
        MathNodeKind::Text | MathNodeKind::TextStyle => MathNode {
            text: Some(text.to_owned()),
            ..empty_node(MathNodeKind::TextContent)
        },
        _ => MathNode {
            text: Some(text.to_owned()),
            attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
            ..empty_node(MathNodeKind::Symbol)
        },
    }
}

fn node_at_path_mut<'tree>(
    node: &'tree mut MathNode,
    path: &[NodeId],
) -> Option<&'tree mut MathNode> {
    let (root, descendants) = path.split_first()?;
    if root != &node.id {
        return None;
    }
    let mut current = node;
    for id in descendants {
        let mut matches = current.children.iter_mut().filter(|child| &child.id == id);
        let child = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        current = child;
    }
    Some(current)
}

fn node_at_path<'tree>(node: &'tree MathNode, path: &[NodeId]) -> Option<&'tree MathNode> {
    let (root, descendants) = path.split_first()?;
    if root != &node.id {
        return None;
    }
    let mut current = node;
    for id in descendants {
        let mut matches = current.children.iter().filter(|child| &child.id == id);
        let child = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        current = child;
    }
    Some(current)
}

fn parent_at_path<'tree>(tree: &'tree MathTree, path: &[NodeId]) -> Option<&'tree MathNode> {
    let (parent_path, _) = path.split_at(path.len().checked_sub(1)?);
    node_at_path(&tree.root, parent_path)
}

fn is_matrix_environment_row(tree: &MathTree, path: &[NodeId]) -> bool {
    node_at_path(&tree.root, path).is_some_and(|node| node.kind == MathNodeKind::Row)
        && parent_at_path(tree, path).is_some_and(|parent| parent.kind == MathNodeKind::Matrix)
}
