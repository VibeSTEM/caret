//! 解析宿主焦点及其 Paragraph 局部位置。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod inline;

use super::super::{
    Affinity, EditError, HostCaretKind, HostCaretLocalPosition, HostCaretPosition,
    MathCaretCoordinate, MathCaretLocalPosition, MathEditTarget, MathTextPosition,
    MathTreePosition, Pos, Session, TextMap,
};
use crate::editing::paragraph_ops::compare_positions;
use crate::editing::position::compare_affinity;
use caret_doc::NodeId;
use caret_structure::{
    DocumentNodeEdge, DocumentNodeKind, compare_document_node_edges, find_document_node,
};
use std::cmp::Ordering;

impl Session {
    pub(in crate::editing::session::document_position) fn compare_text_with_host(
        &self,
        text: &Pos,
        host: &HostCaretPosition<HostCaretLocalPosition>,
    ) -> Result<Ordering, EditError> {
        if let HostCaretPosition::Inside {
            host_id,
            local: HostCaretLocalPosition::Math(math),
            ..
        } = host
            && matches!(math.target, MathEditTarget::Block { .. })
        {
            self.validate_host_caret(host)?;
            return self
                .compare_block_math_to_text(host_id, text)
                .map(Ordering::reverse);
        }
        self.compare_text_with_inline_host(text, host)
    }

    pub(super) fn compare_host_carets(
        &self,
        left: &HostCaretPosition<HostCaretLocalPosition>,
        right: &HostCaretPosition<HostCaretLocalPosition>,
    ) -> Result<Ordering, EditError> {
        self.validate_host_caret(left)?;
        self.validate_host_caret(right)?;
        if left.host_id() == right.host_id() {
            if left.kind() != right.kind() {
                return Err(EditError::InvalidPosition);
            }
            return match (left, right) {
                (HostCaretPosition::Before { .. }, HostCaretPosition::Before { .. })
                | (HostCaretPosition::After { .. }, HostCaretPosition::After { .. }) => {
                    Ok(Ordering::Equal)
                }
                (HostCaretPosition::Before { .. }, _) | (_, HostCaretPosition::After { .. }) => {
                    Ok(Ordering::Less)
                }
                (HostCaretPosition::After { .. }, _) | (_, HostCaretPosition::Before { .. }) => {
                    Ok(Ordering::Greater)
                }
                (
                    HostCaretPosition::Inside { local: left, .. },
                    HostCaretPosition::Inside { local: right, .. },
                ) => self.compare_host_local(left, right),
            };
        }

        match (left, right) {
            (
                HostCaretPosition::Before {
                    host_id: left_id, ..
                }
                | HostCaretPosition::After {
                    host_id: left_id, ..
                },
                HostCaretPosition::Before {
                    host_id: right_id, ..
                }
                | HostCaretPosition::After {
                    host_id: right_id, ..
                },
            ) => {
                let left_edge = match left {
                    HostCaretPosition::Before { .. } => DocumentNodeEdge::Before,
                    _ => DocumentNodeEdge::After,
                };
                let right_edge = match right {
                    HostCaretPosition::Before { .. } => DocumentNodeEdge::Before,
                    _ => DocumentNodeEdge::After,
                };
                let physical_order = compare_document_node_edges(
                    &self.document,
                    left_id,
                    left_edge,
                    right_id,
                    right_edge,
                )
                .ok_or(EditError::InvalidPosition)?;
                if physical_order == Ordering::Equal
                    && left_id != right_id
                    && left.kind() == HostCaretKind::Inline
                    && right.kind() == HostCaretKind::Inline
                {
                    return Ok(compare_affinity(
                        inline_edge_affinity(left),
                        inline_edge_affinity(right),
                    ));
                }
                Ok(physical_order)
            }
            (
                HostCaretPosition::Inside {
                    local: HostCaretLocalPosition::Math(left),
                    ..
                },
                HostCaretPosition::Inside {
                    local: HostCaretLocalPosition::Math(right),
                    ..
                },
            ) if matches!(left.target, MathEditTarget::Inline { .. })
                && matches!(right.target, MathEditTarget::Inline { .. }) =>
            {
                self.compare_inline_math_carets(left, right)
            }
            (
                HostCaretPosition::Inside {
                    host_id: left_id,
                    local: HostCaretLocalPosition::Math(_),
                    ..
                },
                HostCaretPosition::Inside {
                    host_id: right_id,
                    local: HostCaretLocalPosition::Math(_),
                    ..
                },
            ) => self.compare_math_host_ranges(left_id, right_id),
            (
                HostCaretPosition::Inside {
                    local: HostCaretLocalPosition::Math(math),
                    ..
                },
                HostCaretPosition::Inside {
                    local: HostCaretLocalPosition::Paragraph(paragraph),
                    ..
                },
            ) if matches!(math.target, MathEditTarget::Inline { .. }) => {
                self.compare_inline_math_to_paragraph(math, paragraph)
            }
            (
                HostCaretPosition::Inside {
                    local: HostCaretLocalPosition::Paragraph(paragraph),
                    ..
                },
                HostCaretPosition::Inside {
                    local: HostCaretLocalPosition::Math(math),
                    ..
                },
            ) if matches!(math.target, MathEditTarget::Inline { .. }) => self
                .compare_inline_math_to_paragraph(math, paragraph)
                .map(Ordering::reverse),
            (
                HostCaretPosition::Inside {
                    host_id,
                    local: HostCaretLocalPosition::Math(math),
                    ..
                },
                HostCaretPosition::Inside {
                    local: HostCaretLocalPosition::Paragraph(paragraph),
                    ..
                },
            ) if matches!(math.target, MathEditTarget::Block { .. }) => {
                self.compare_block_math_to_text(host_id, paragraph)
            }
            (
                HostCaretPosition::Inside {
                    local: HostCaretLocalPosition::Paragraph(paragraph),
                    ..
                },
                HostCaretPosition::Inside {
                    host_id,
                    local: HostCaretLocalPosition::Math(math),
                    ..
                },
            ) if matches!(math.target, MathEditTarget::Block { .. }) => self
                .compare_block_math_to_text(host_id, paragraph)
                .map(Ordering::reverse),
            (
                HostCaretPosition::Inside {
                    host_id,
                    local: HostCaretLocalPosition::Math(math),
                    ..
                },
                outside @ (HostCaretPosition::Before { .. } | HostCaretPosition::After { .. }),
            ) if matches!(math.target, MathEditTarget::Block { .. }) => {
                self.compare_math_host_to_host_edge(host_id, outside)
            }
            (
                outside @ (HostCaretPosition::Before { .. } | HostCaretPosition::After { .. }),
                HostCaretPosition::Inside {
                    host_id,
                    local: HostCaretLocalPosition::Math(math),
                    ..
                },
            ) if matches!(math.target, MathEditTarget::Block { .. }) => self
                .compare_math_host_to_host_edge(host_id, outside)
                .map(Ordering::reverse),
            (
                HostCaretPosition::Inside {
                    host_id,
                    local: HostCaretLocalPosition::Math(math),
                    ..
                },
                outside @ (HostCaretPosition::Before { .. } | HostCaretPosition::After { .. }),
            ) if matches!(math.target, MathEditTarget::Inline { .. }) => {
                self.compare_math_host_to_host_edge(host_id, outside)
            }
            (
                outside @ (HostCaretPosition::Before { .. } | HostCaretPosition::After { .. }),
                HostCaretPosition::Inside {
                    host_id,
                    local: HostCaretLocalPosition::Math(math),
                    ..
                },
            ) if matches!(math.target, MathEditTarget::Inline { .. }) => self
                .compare_math_host_to_host_edge(host_id, outside)
                .map(Ordering::reverse),
            (
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Block,
                }
                | HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Block,
                },
                HostCaretPosition::Inside {
                    local: HostCaretLocalPosition::Paragraph(paragraph),
                    host_id: inner_host_id,
                    kind: inner_kind,
                    ..
                },
            ) => {
                self.ensure_block_edge_mapping_is_unambiguous(
                    inner_host_id,
                    *inner_kind,
                    paragraph,
                )?;
                self.compare_text_with_block_host_edge(host_id, left, paragraph)
                    .map(Ordering::reverse)
            }
            (
                HostCaretPosition::Inside {
                    local: HostCaretLocalPosition::Paragraph(paragraph),
                    host_id: inner_host_id,
                    kind: inner_kind,
                    ..
                },
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Block,
                }
                | HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Block,
                },
            ) => {
                self.ensure_block_edge_mapping_is_unambiguous(
                    inner_host_id,
                    *inner_kind,
                    paragraph,
                )?;
                self.compare_text_with_block_host_edge(host_id, right, paragraph)
            }
            (
                HostCaretPosition::Inside {
                    local: HostCaretLocalPosition::Paragraph(left),
                    host_id: left_id,
                    ..
                },
                HostCaretPosition::Inside {
                    local: HostCaretLocalPosition::Paragraph(right),
                    host_id: right_id,
                    ..
                },
            ) => {
                if left.paragraph_id == right.paragraph_id
                    && left.offset == right.offset
                    && left_id != right_id
                {
                    return Err(EditError::InvalidPosition);
                }
                compare_positions(&self.document.body, &self.paragraph_index, left, right)
            }
            _ => Err(EditError::InvalidPosition),
        }
    }

    fn compare_text_with_block_host_edge(
        &self,
        host_id: &NodeId,
        host: &HostCaretPosition<HostCaretLocalPosition>,
        text: &Pos,
    ) -> Result<Ordering, EditError> {
        self.validate_host_caret(host)?;
        let edge = match host {
            HostCaretPosition::Before { .. } => DocumentNodeEdge::Before,
            HostCaretPosition::After { .. } => DocumentNodeEdge::After,
            HostCaretPosition::Inside { .. } => return Err(EditError::InvalidPosition),
        };
        let map = self.text_map_for_caret(text)?;
        map.validate_pos(text)?;
        let edge_to_paragraph_start = compare_document_node_edges(
            &self.document,
            host_id,
            edge,
            &text.paragraph_id,
            DocumentNodeEdge::Before,
        )
        .ok_or(EditError::InvalidPosition)?;
        if edge_to_paragraph_start == Ordering::Less {
            return Ok(Ordering::Greater);
        }
        if edge_to_paragraph_start == Ordering::Equal {
            return Ok(if text.offset == 0 {
                compare_affinity(text.affinity, Affinity::Upstream)
            } else {
                Ordering::Greater
            });
        }
        let edge_to_paragraph_end = compare_document_node_edges(
            &self.document,
            host_id,
            edge,
            &text.paragraph_id,
            DocumentNodeEdge::After,
        )
        .ok_or(EditError::InvalidPosition)?;
        if edge_to_paragraph_end == Ordering::Greater {
            return Ok(Ordering::Less);
        }
        if edge_to_paragraph_end == Ordering::Equal {
            return Ok(if text.offset == map.text.len() {
                compare_affinity(text.affinity, Affinity::Downstream)
            } else {
                Ordering::Less
            });
        }
        Err(EditError::InvalidPosition)
    }

    fn compare_math_host_ranges(
        &self,
        left_host_id: &NodeId,
        right_host_id: &NodeId,
    ) -> Result<Ordering, EditError> {
        let left_after_to_right_before = compare_document_node_edges(
            &self.document,
            left_host_id,
            DocumentNodeEdge::After,
            right_host_id,
            DocumentNodeEdge::Before,
        )
        .ok_or(EditError::InvalidPosition)?;
        if left_after_to_right_before != Ordering::Greater {
            return Ok(Ordering::Less);
        }
        let right_after_to_left_before = compare_document_node_edges(
            &self.document,
            right_host_id,
            DocumentNodeEdge::After,
            left_host_id,
            DocumentNodeEdge::Before,
        )
        .ok_or(EditError::InvalidPosition)?;
        if right_after_to_left_before != Ordering::Greater {
            return Ok(Ordering::Greater);
        }
        Err(EditError::InvalidPosition)
    }

    fn compare_block_math_to_text(
        &self,
        block_id: &NodeId,
        text: &Pos,
    ) -> Result<Ordering, EditError> {
        let map = self.text_map_for_caret(text)?;
        map.validate_pos(text)?;
        let block_after_to_text_start = compare_document_node_edges(
            &self.document,
            block_id,
            DocumentNodeEdge::After,
            &text.paragraph_id,
            DocumentNodeEdge::Before,
        )
        .ok_or(EditError::InvalidPosition)?;
        if block_after_to_text_start != Ordering::Greater {
            return Ok(Ordering::Less);
        }
        let block_before_to_text_end = compare_document_node_edges(
            &self.document,
            block_id,
            DocumentNodeEdge::Before,
            &text.paragraph_id,
            DocumentNodeEdge::After,
        )
        .ok_or(EditError::InvalidPosition)?;
        if block_before_to_text_end != Ordering::Less {
            return Ok(Ordering::Greater);
        }
        Err(EditError::InvalidPosition)
    }

    fn compare_math_host_to_host_edge(
        &self,
        math_id: &NodeId,
        outside: &HostCaretPosition<HostCaretLocalPosition>,
    ) -> Result<Ordering, EditError> {
        let external_id = outside.host_id();
        let external_edge = match outside {
            HostCaretPosition::Before { .. } => DocumentNodeEdge::Before,
            HostCaretPosition::After { .. } => DocumentNodeEdge::After,
            HostCaretPosition::Inside { .. } => return Err(EditError::InvalidPosition),
        };
        let math_after_to_external = compare_document_node_edges(
            &self.document,
            math_id,
            DocumentNodeEdge::After,
            external_id,
            external_edge,
        )
        .ok_or(EditError::InvalidPosition)?;
        if math_after_to_external != Ordering::Greater {
            return Ok(Ordering::Less);
        }
        let math_before_to_external = compare_document_node_edges(
            &self.document,
            math_id,
            DocumentNodeEdge::Before,
            external_id,
            external_edge,
        )
        .ok_or(EditError::InvalidPosition)?;
        if math_before_to_external != Ordering::Less {
            return Ok(Ordering::Greater);
        }
        Err(EditError::InvalidPosition)
    }

    fn ensure_block_edge_mapping_is_unambiguous(
        &self,
        host_id: &NodeId,
        kind: HostCaretKind,
        position: &Pos,
    ) -> Result<(), EditError> {
        if kind == HostCaretKind::Block {
            return Ok(());
        }
        let map = self.text_map_for_caret(position)?;
        if map.is_empty_inline_environment(position.offset, host_id) {
            return Err(EditError::InvalidPosition);
        }
        Ok(())
    }

    fn compare_host_local(
        &self,
        left: &HostCaretLocalPosition,
        right: &HostCaretLocalPosition,
    ) -> Result<Ordering, EditError> {
        match (left, right) {
            (HostCaretLocalPosition::Paragraph(left), HostCaretLocalPosition::Paragraph(right)) => {
                compare_positions(&self.document.body, &self.paragraph_index, left, right)
            }
            (HostCaretLocalPosition::Math(left), HostCaretLocalPosition::Math(right)) => {
                if !matches!(&left.target, MathEditTarget::Block { align: true, .. }) {
                    return self.compare_math_local(left, right);
                }
                if left.target != right.target {
                    return Err(EditError::InvalidPosition);
                }
                let left_cell = self.align_cell_for_math_coordinate(left)?;
                let right_cell = self.align_cell_for_math_coordinate(right)?;
                match (left_cell, right_cell) {
                    (Some(left_cell), Some(right_cell)) if left_cell != right_cell => {
                        Ok(left_cell.cmp(&right_cell))
                    }
                    (Some(_), Some(_)) | (None, None) => self.compare_math_local(left, right),
                    _ => Err(EditError::InvalidPosition),
                }
            }
            _ => Err(EditError::InvalidPosition),
        }
    }

    fn align_cell_for_math_coordinate(
        &self,
        local: &MathCaretLocalPosition,
    ) -> Result<Option<(usize, usize)>, EditError> {
        match &local.coordinate {
            MathCaretCoordinate::SourceUtf8 { .. } => Ok(None),
            MathCaretCoordinate::TreeBoundary { host, .. } => {
                self.align_cell_for_target_and_host(&local.target, host)
            }
            MathCaretCoordinate::TextFlowUtf8 { path, .. } => {
                self.math_position_host_for_text_path(path, &local.target)?;
                Ok(path.align_cell())
            }
        }
    }

    pub(super) fn validate_host_caret(
        &self,
        position: &HostCaretPosition<HostCaretLocalPosition>,
    ) -> Result<(), EditError> {
        let location = find_document_node(&self.document, position.host_id())
            .ok_or(EditError::InvalidPosition)?;
        let valid_kind = match position.kind() {
            HostCaretKind::Block => matches!(
                location.kind,
                DocumentNodeKind::Environment
                    | DocumentNodeKind::Equation
                    | DocumentNodeKind::Align
            ),
            HostCaretKind::Inline => matches!(
                location.kind,
                DocumentNodeKind::InlineEnvironment | DocumentNodeKind::InlineMath
            ),
        };
        if !valid_kind {
            return Err(EditError::InvalidPosition);
        }
        if position.kind() == HostCaretKind::Inline
            && !matches!(position, HostCaretPosition::Inside { .. })
        {
            self.validate_inline_host_edge(position.host_id(), position)?;
        }
        if let HostCaretPosition::Inside {
            local,
            host_id,
            kind,
        } = position
        {
            match local {
                HostCaretLocalPosition::Paragraph(position) => {
                    let valid_paragraph_host = match kind {
                        HostCaretKind::Inline => {
                            location.kind == DocumentNodeKind::InlineEnvironment
                                && location.paragraph_id.as_ref() == Some(&position.paragraph_id)
                        }
                        HostCaretKind::Block => {
                            location.kind == DocumentNodeKind::Environment
                                && find_document_node(&self.document, &position.paragraph_id)
                                    .is_some_and(|paragraph_location| {
                                        paragraph_location
                                            .parent_environment_ids
                                            .iter()
                                            .any(|ancestor_id| ancestor_id == host_id)
                                    })
                        }
                    };
                    if !valid_paragraph_host {
                        return Err(EditError::InvalidPosition);
                    }
                    if self.story_for_paragraph(&position.paragraph_id)
                        != Some(position.story.clone())
                    {
                        return Err(EditError::WrongStory);
                    }
                    let map = self.text_map_for_caret(position)?;
                    map.validate_pos(position)?;
                    if *kind == HostCaretKind::Inline {
                        self.validate_inline_paragraph_host(host_id, position)?;
                    }
                }
                HostCaretLocalPosition::Math(local) => {
                    let target_host_id = match &local.target {
                        MathEditTarget::Inline { math_id, .. } => math_id,
                        MathEditTarget::Block { block_id, .. } => block_id,
                    };
                    if target_host_id != host_id {
                        return Err(EditError::InvalidPosition);
                    }
                    let target_kind_matches = match &local.target {
                        MathEditTarget::Inline { paragraph_id, .. } => {
                            location.kind == DocumentNodeKind::InlineMath
                                && location.paragraph_id.as_ref() == Some(paragraph_id)
                        }
                        MathEditTarget::Block { align: false, .. } => {
                            location.kind == DocumentNodeKind::Equation
                        }
                        MathEditTarget::Block { align: true, .. } => {
                            location.kind == DocumentNodeKind::Align
                        }
                    };
                    if !target_kind_matches {
                        return Err(EditError::InvalidPosition);
                    }
                    match &local.coordinate {
                        MathCaretCoordinate::SourceUtf8 { .. } => {
                            self.validate_source_caret(&local.target, &local.coordinate)?;
                        }
                        MathCaretCoordinate::TreeBoundary {
                            host,
                            path,
                            index,
                            affinity,
                        } => {
                            let (tree, _) = self.math_tree_for_caret(host, &local.target)?;
                            MathTreePosition {
                                host: host.clone(),
                                path: path.clone(),
                                offset: *index,
                                affinity: *affinity,
                            }
                            .validate_reachable(tree)
                            .map_err(|_| EditError::InvalidPosition)?;
                        }
                        MathCaretCoordinate::TextFlowUtf8 {
                            path,
                            offset,
                            affinity,
                        } => {
                            let (tree, _) =
                                self.math_tree_for_caret_from_path(path, &local.target)?;
                            let (sequence_path, text_index) =
                                MathTreePosition::sequence_boundary_from_text_path(path.steps())
                                    .map_err(|_| EditError::InvalidPosition)?;
                            let host =
                                self.math_position_host_for_text_path(path, &local.target)?;
                            MathTreePosition {
                                host,
                                path: sequence_path,
                                offset: text_index,
                                affinity: *affinity,
                            }
                            .validate_reachable(tree)
                            .map_err(|_| EditError::InvalidPosition)?;
                            let position = MathTextPosition::new(path.clone(), *offset, *affinity);
                            position
                                .compare_in_same_flow(&position, tree)
                                .ok_or(EditError::InvalidPosition)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn text_map_for_caret(&self, position: &Pos) -> Result<TextMap, EditError> {
        TextMap::new(self.paragraph_for(position)?)
    }
}

fn inline_edge_affinity(position: &HostCaretPosition<HostCaretLocalPosition>) -> Affinity {
    match position {
        HostCaretPosition::Before { .. } => Affinity::Upstream,
        HostCaretPosition::After { .. } => Affinity::Downstream,
        HostCaretPosition::Inside { .. } => unreachable!("仅外侧 Inline 边界有亲和性映射"),
    }
}
