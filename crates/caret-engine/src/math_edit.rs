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
    let node = node_at_path_mut(&mut tree.root, path).ok_or(MathEditError::InvalidPosition)?;
    match node.kind {
        MathNodeKind::Placeholder => {
            let text_slot = matches!(
                parent_kind,
                Some(MathNodeKind::Text | MathNodeKind::TextStyle)
            );
            if !text_slot && !is_ordinary_math_text(text) {
                return Err(MathEditError::InvalidText);
            }
            if !node.extra.is_empty() || node.source_range.is_some() {
                return Err(MathEditError::UnsupportedTarget);
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
            if !is_ordinary_math_text(text) {
                return Err(MathEditError::InvalidText);
            }
            let value = node.text.as_mut().ok_or(MathEditError::UnsupportedTarget)?;
            value.insert_str(position.offset, text);
            finish_edit(
                tree,
                MathPos {
                    path: path.clone(),
                    offset: position.offset + text.len(),
                },
            )
        }
        MathNodeKind::TextContent => {
            let value = node.text.as_mut().ok_or(MathEditError::UnsupportedTarget)?;
            value.insert_str(position.offset, text);
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
            if !matches!(node.kind, MathNodeKind::Text | MathNodeKind::TextStyle)
                && !is_ordinary_math_text(text)
            {
                return Err(MathEditError::InvalidText);
            }
            let index = position.offset.min(node.children.len());
            let new_node = text_node(node.kind, text);
            let id = new_node.id.clone();
            node.children.insert(index, new_node);
            let mut result_path = path.clone();
            result_path.push(id);
            finish_edit(
                tree,
                MathPos {
                    path: result_path,
                    offset: text.len(),
                },
            )
        }
        _ => Err(MathEditError::UnsupportedTarget),
    }
}

fn is_ordinary_math_text(text: &str) -> bool {
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
        let slot_nodes = slots.into_iter().map(empty_node).collect::<Vec<_>>();
        let focus = focus_slot
            .and_then(|(slot, offset)| slot_nodes.get(slot).map(|node| (node.id.clone(), offset)))
            .ok_or(MathEditError::UnsupportedTarget)?;
        node.kind = kind;
        node.text = None;
        node.attributes = if kind == MathNodeKind::Fraction {
            BTreeMap::from([("style".to_owned(), "frac".to_owned())])
        } else {
            BTreeMap::new()
        };
        node.children = slot_nodes;
        let mut result_path = position.path.clone();
        result_path.push(focus.0);
        return finish_edit(
            tree,
            MathPos {
                path: result_path,
                offset: focus.1,
            },
        );
    }
    if node.kind != MathNodeKind::Row || matrix_row || position.offset > node.children.len() {
        return Err(MathEditError::UnsupportedTarget);
    }
    let insertion_index = position.offset;
    let slot_nodes = slots.into_iter().map(empty_node).collect::<Vec<_>>();
    let focus = focus_slot
        .and_then(|(slot, offset)| slot_nodes.get(slot).map(|node| (node.id.clone(), offset)))
        .ok_or(MathEditError::UnsupportedTarget)?;
    let mut attributes = BTreeMap::new();
    if kind == MathNodeKind::Fraction {
        attributes.insert("style".to_owned(), "frac".to_owned());
    }
    let structure = MathNode {
        id: NodeId::generate(),
        kind,
        text: None,
        attributes,
        source_range: None,
        children: slot_nodes,
        extra: BTreeMap::new(),
    };
    let structure_id = structure.id.clone();
    node.children.insert(insertion_index, structure);
    let mut result_path = position.path.clone();
    result_path.push(structure_id);
    result_path.push(focus.0);
    finish_edit(
        tree,
        MathPos {
            path: result_path,
            offset: focus.1,
        },
    )
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
