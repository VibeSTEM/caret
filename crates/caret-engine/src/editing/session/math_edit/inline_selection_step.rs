//! 在已保留正文或数学锚点的选区内移动当前数学焦点。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;
use crate::DocumentRangeEndpoint;

/// 返回相邻树项或文本字素；到达整个公式边界时交给文档入口选取外侧位置。
pub(in crate::editing::session) fn step(
    tree: &MathTree,
    endpoint: &DocumentRangeEndpoint,
    forward: bool,
) -> Result<Option<DocumentRangeEndpoint>, EditError> {
    use DocumentRangeEndpoint::{InlineMathText, InlineMathTree};
    let affinity = if forward {
        MathPositionAffinity::Upstream
    } else {
        MathPositionAffinity::Downstream
    };
    let advance =
        |position: &MathTreePosition| advance_inline_selection_position(tree, position, forward);
    let (paragraph_id, math_id, direction, entry_forward, moved) = match endpoint {
        InlineMathText {
            paragraph_id,
            math_id,
            position,
            entry_direction,
            entry_math_forward,
        } => {
            let moved = match step_text(tree, math_id, position, forward, affinity)? {
                MathTreeCaretMove::Position(edge) => advance(&edge)?,
                MathTreeCaretMove::TextFlow(path, offset) => {
                    return Ok(Some(InlineMathText {
                        paragraph_id: paragraph_id.clone(),
                        math_id: math_id.clone(),
                        position: MathTextPosition::new(path, offset, affinity),
                        entry_direction: *entry_direction,
                        entry_math_forward: *entry_math_forward,
                    }));
                }
                MathTreeCaretMove::Boundary => return Err(EditError::InvalidPosition),
            };
            (
                paragraph_id,
                math_id,
                entry_direction,
                entry_math_forward,
                moved,
            )
        }
        InlineMathTree {
            paragraph_id,
            position,
            entry_direction,
            entry_math_forward,
        } => {
            let MathPositionHost::InlineMath { owner_id } = &position.host else {
                return Err(EditError::InvalidPosition);
            };
            (
                paragraph_id,
                owner_id,
                entry_direction,
                entry_math_forward,
                advance(position)?,
            )
        }
        _ => return Err(EditError::InvalidPosition),
    };
    // 从树进入文本岛时，移动到第一个字素的远侧，避免多按一次键才看见范围变化。
    let moved = match moved {
        MathTreeCaretMove::TextFlow(path, offset) => step_text(
            tree,
            math_id,
            &MathTextPosition::new(path, offset, affinity),
            forward,
            affinity,
        )?,
        moved => moved,
    };
    Ok(match moved {
        MathTreeCaretMove::Boundary => None,
        MathTreeCaretMove::Position(position)
            if position.path == MathSequencePath::root()
                && position.offset == if forward { tree.root.content.len() } else { 0 } =>
        {
            None
        }
        MathTreeCaretMove::Position(position) => Some(DocumentRangeEndpoint::InlineMathTree {
            paragraph_id: paragraph_id.clone(),
            position: MathTreePosition {
                affinity,
                ..position
            },
            entry_direction: *direction,
            entry_math_forward: *entry_forward,
        }),
        MathTreeCaretMove::TextFlow(path, offset) => Some(DocumentRangeEndpoint::InlineMathText {
            paragraph_id: paragraph_id.clone(),
            math_id: math_id.clone(),
            position: MathTextPosition::new(path, offset, affinity),
            entry_direction: *direction,
            entry_math_forward: *entry_forward,
        }),
    })
}

/// 按布局确认的视觉岛边界，从对应结构侧继续扩选。
pub(in crate::editing::session) fn step_from_text_visual_edge(
    tree: &MathTree,
    endpoint: &DocumentRangeEndpoint,
    forward: bool,
) -> Result<Option<DocumentRangeEndpoint>, EditError> {
    let DocumentRangeEndpoint::InlineMathText {
        paragraph_id,
        math_id,
        position: _,
        entry_direction,
        entry_math_forward,
    } = endpoint
    else {
        return Err(EditError::InvalidPosition);
    };
    let affinity = if forward {
        MathPositionAffinity::Upstream
    } else {
        MathPositionAffinity::Downstream
    };
    let boundary = text_visual_edge_position(tree, endpoint, forward, affinity)?;
    let moved = advance_inline_selection_position(tree, &boundary, forward)?;
    let moved = match moved {
        MathTreeCaretMove::TextFlow(path, offset) => step_text(
            tree,
            math_id,
            &MathTextPosition::new(path, offset, affinity),
            forward,
            affinity,
        )?,
        moved => moved,
    };
    Ok(match moved {
        MathTreeCaretMove::Boundary => None,
        MathTreeCaretMove::Position(position)
            if position.path == MathSequencePath::root()
                && position.offset == if forward { tree.root.content.len() } else { 0 } =>
        {
            None
        }
        MathTreeCaretMove::Position(position) => Some(DocumentRangeEndpoint::InlineMathTree {
            paragraph_id: paragraph_id.clone(),
            position: MathTreePosition {
                affinity,
                ..position
            },
            entry_direction: Direction::Forward,
            entry_math_forward: true,
        }),
        MathTreeCaretMove::TextFlow(path, offset) => Some(DocumentRangeEndpoint::InlineMathText {
            paragraph_id: paragraph_id.clone(),
            math_id: math_id.clone(),
            position: MathTextPosition::new(path, offset, affinity),
            entry_direction: *entry_direction,
            entry_math_forward: *entry_math_forward,
        }),
    })
}

fn text_visual_edge_position(
    tree: &MathTree,
    endpoint: &DocumentRangeEndpoint,
    forward: bool,
    affinity: MathPositionAffinity,
) -> Result<MathTreePosition, EditError> {
    let DocumentRangeEndpoint::InlineMathText {
        math_id, position, ..
    } = endpoint
    else {
        return Err(EditError::InvalidPosition);
    };
    let island =
        path::contiguous_text_flow_text(tree, &position.path).ok_or(EditError::InvalidPosition)?;
    let edge_path = if forward {
        island.paths.last()
    } else {
        island.paths.first()
    }
    .ok_or(EditError::InvalidPosition)?;
    let boundary = MathTreePosition::from_text_flow_boundary(
        tree,
        MathPositionHost::InlineMath {
            owner_id: math_id.clone(),
        },
        edge_path.steps(),
        None,
        if forward {
            MathTextFlowBoundary::End
        } else {
            MathTextFlowBoundary::Start
        },
        affinity,
    )
    .map_err(|_| EditError::InvalidPosition)?;

    Ok(boundary)
}

fn advance_inline_selection_position(
    tree: &MathTree,
    boundary: &MathTreePosition,
    forward: bool,
) -> Result<MathTreeCaretMove, EditError> {
    let mut boundary = boundary.clone();
    loop {
        let contents = boundary
            .resolve(tree)
            .map_err(|_| EditError::InvalidPosition)?;
        let has_adjacent = if forward {
            boundary.offset < contents.len()
        } else {
            boundary.offset > 0
        };
        if has_adjacent {
            return tree_position::advance_math_tree_selection_position(
                tree, &boundary, forward, None,
            )
            .map_err(|_| EditError::InvalidPosition);
        }

        let Some((last, parent_steps)) = boundary.path.steps().split_last() else {
            return Ok(MathTreeCaretMove::Boundary);
        };
        let content_index = match *last {
            MathSequenceStep::Operand { content_index, .. }
            | MathSequenceStep::EnvironmentChildren { content_index } => content_index,
        };
        boundary = MathTreePosition {
            host: boundary.host,
            path: MathSequencePath::new(parent_steps.to_vec()),
            offset: content_index + usize::from(forward),
            affinity: boundary.affinity,
        };
    }
}

/// 在连续文本岛内跨过一个字素，或返回岛边缘对应的树切口。
fn step_text(
    tree: &MathTree,
    math_id: &NodeId,
    position: &MathTextPosition,
    forward: bool,
    affinity: MathPositionAffinity,
) -> Result<MathTreeCaretMove, EditError> {
    let island =
        path::contiguous_text_flow_text(tree, &position.path).ok_or(EditError::InvalidPosition)?;
    let at = island
        .global_offset(position)
        .ok_or(EditError::InvalidPosition)?;
    let next = if forward {
        next_grapheme_end(&island.display, at)
    } else {
        previous_grapheme_start(&island.display, at)
    };
    if let Some(next) = next {
        let p = island
            .position_at_offset(next, affinity)
            .ok_or(EditError::InvalidPosition)?;
        return Ok(MathTreeCaretMove::TextFlow(p.path, p.offset));
    }
    let edge_path = if forward {
        island.paths.last()
    } else {
        island.paths.first()
    }
    .ok_or(EditError::InvalidPosition)?;
    MathTreePosition::from_text_flow_boundary(
        tree,
        MathPositionHost::InlineMath {
            owner_id: math_id.clone(),
        },
        edge_path.steps(),
        None,
        if forward {
            MathTextFlowBoundary::End
        } else {
            MathTextFlowBoundary::Start
        },
        affinity,
    )
    .map(MathTreeCaretMove::Position)
    .map_err(|_| EditError::InvalidPosition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use caret_doc::{
        MathContent, MathFlow, MathNode, MathNodeKind, MathTree, NodeId, Run, TextFlow,
    };
    use std::collections::BTreeMap;

    #[test]
    fn visual_text_island_edge_skips_trailing_empty_text_flow_before_sibling() {
        let math_id = NodeId::generate();
        let tree = MathTree {
            schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
            root: MathFlow {
                id: NodeId::generate(),
                content: vec![MathContent::Math(MathNode {
                    id: NodeId::generate(),
                    kind: MathNodeKind::Group,
                    text: None,
                    attributes: BTreeMap::new(),
                    children: vec![MathFlow {
                        id: NodeId::generate(),
                        content: vec![
                            MathContent::Text(TextFlow {
                                runs: vec![Run {
                                    text: "a".to_owned(),
                                    language: None,
                                    props: BTreeMap::new(),
                                    extra: BTreeMap::new(),
                                }],
                                extra: BTreeMap::new(),
                            }),
                            MathContent::Text(TextFlow {
                                runs: Vec::new(),
                                extra: BTreeMap::new(),
                            }),
                            MathContent::Math(MathNode {
                                id: NodeId::generate(),
                                kind: MathNodeKind::Symbol,
                                text: Some("x".to_owned()),
                                attributes: BTreeMap::new(),
                                children: Vec::new(),
                                extra: BTreeMap::new(),
                            }),
                        ],
                        extra: BTreeMap::new(),
                    }],
                    extra: BTreeMap::new(),
                })],
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        };
        let endpoint = DocumentRangeEndpoint::InlineMathText {
            paragraph_id: NodeId::generate(),
            math_id,
            position: MathTextPosition::new(
                MathTextFlowPath::from_interaction_path(
                    vec![
                        caret_doc::MathPathStep::Content(0),
                        caret_doc::MathPathStep::Operand(0),
                        caret_doc::MathPathStep::Content(0),
                    ],
                    None,
                ),
                1,
                MathPositionAffinity::Upstream,
            ),
            entry_direction: Direction::Forward,
            entry_math_forward: true,
        };

        let next = step_from_text_visual_edge(&tree, &endpoint, true)
            .unwrap_or_else(|error| panic!("visual edge step failed: {error}"));
        assert!(matches!(
            next,
            Some(DocumentRangeEndpoint::InlineMathTree { .. })
        ));
    }
}
