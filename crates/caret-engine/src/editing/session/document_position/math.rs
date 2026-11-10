//! 校验公式局部坐标并比较同一数学宿主内的位置。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::math_edit::MathTextFlowBoundary;
use super::super::math_edit::compare_math_position_affinity;
use super::super::{
    EditError, MathCaretCoordinate, MathCaretLocalPosition, MathEdit, MathEditBranch,
    MathEditTarget, MathPositionAffinity, MathPositionHost, MathTextFlowPath, MathTextPosition,
    MathTreePosition, Session,
};
use crate::editing::grapheme::GraphemeMap;
use std::cmp::Ordering;

type MathTreeCaretLocation<'a> = (&'a caret_doc::MathTree, Option<(usize, usize)>);

impl Session {
    pub(in crate::editing::session) fn resolve_math_source_revision<'a>(
        &'a self,
        target: &MathEditTarget,
        draft_revision: Option<&(caret_doc::NodeId, u64)>,
        projection_id: Option<&caret_doc::NodeId>,
    ) -> Result<(&'a MathEdit, &'a str), EditError> {
        self.ensure_math_edit_revision_current()?;
        let edit = self.math_edit.as_ref().ok_or(EditError::InvalidPosition)?;
        if &edit.target != target || !matches!(edit.branch, MathEditBranch::Source) {
            return Err(EditError::InvalidPosition);
        }
        let source = if let Some(draft) = &edit.source_draft {
            let current_revision = (draft.id().clone(), draft.revision());
            if draft_revision != Some(&current_revision) || projection_id.is_some() {
                return Err(EditError::InvalidPosition);
            }
            draft.source()
        } else {
            if draft_revision.is_some() || projection_id != Some(&edit.revision.projection_id) {
                return Err(EditError::InvalidPosition);
            }
            edit.source_projection
                .as_deref()
                .ok_or(EditError::InvalidPosition)?
                .edit_source()
        };
        Ok((edit, source))
    }

    pub(super) fn compare_math_local(
        &self,
        left: &MathCaretLocalPosition,
        right: &MathCaretLocalPosition,
    ) -> Result<Ordering, EditError> {
        if left.target != right.target {
            return Err(EditError::InvalidPosition);
        }
        match (&left.coordinate, &right.coordinate) {
            (
                MathCaretCoordinate::SourceUtf8 {
                    offset: left_offset,
                    draft_revision: left_draft,
                    projection_id: left_projection,
                },
                MathCaretCoordinate::SourceUtf8 {
                    offset: right_offset,
                    draft_revision: right_draft,
                    projection_id: right_projection,
                },
            ) if left_draft == right_draft && left_projection == right_projection => {
                self.validate_source_caret(&left.target, &left.coordinate)?;
                self.validate_source_caret(&right.target, &right.coordinate)?;
                Ok(left_offset.cmp(right_offset))
            }
            (
                MathCaretCoordinate::TreeBoundary {
                    host: left_host,
                    path: left_path,
                    index: left_index,
                    affinity: left_affinity,
                },
                MathCaretCoordinate::TreeBoundary {
                    host: right_host,
                    path: right_path,
                    index: right_index,
                    affinity: right_affinity,
                },
            ) => {
                let (tree, align_cell) = self.math_tree_for_caret(left_host, &left.target)?;
                if self.math_tree_for_caret(right_host, &right.target)?.1 != align_cell {
                    return Err(EditError::InvalidPosition);
                }
                let left_position = MathTreePosition {
                    host: left_host.clone(),
                    path: left_path.clone(),
                    offset: *left_index,
                    affinity: *left_affinity,
                };
                let right_position = MathTreePosition {
                    host: right_host.clone(),
                    path: right_path.clone(),
                    offset: *right_index,
                    affinity: *right_affinity,
                };
                left_position
                    .validate_reachable(tree)
                    .and_then(|()| right_position.validate_reachable(tree))
                    .map_err(|_| EditError::InvalidPosition)?;
                left_position
                    .compare_in_tree(&right_position, tree)
                    .map_err(|_| EditError::InvalidPosition)
            }
            (
                MathCaretCoordinate::TextFlowUtf8 {
                    path: left_path,
                    offset: left_offset,
                    affinity: left_affinity,
                },
                MathCaretCoordinate::TextFlowUtf8 {
                    path: right_path,
                    offset: right_offset,
                    affinity: right_affinity,
                },
            ) => {
                let (tree, left_cell) =
                    self.math_tree_for_caret_from_path(left_path, &left.target)?;
                if self
                    .math_tree_for_caret_from_path(right_path, &right.target)?
                    .1
                    != left_cell
                {
                    return Err(EditError::InvalidPosition);
                }
                MathTextPosition::new(left_path.clone(), *left_offset, *left_affinity)
                    .compare_in_same_text_island(
                        &MathTextPosition::new(right_path.clone(), *right_offset, *right_affinity),
                        tree,
                    )
                    .ok_or(EditError::InvalidPosition)
            }
            (
                MathCaretCoordinate::TreeBoundary {
                    host,
                    path,
                    index,
                    affinity,
                },
                MathCaretCoordinate::TextFlowUtf8 {
                    path: text_path,
                    offset,
                    affinity: text_affinity,
                },
            ) => self.compare_tree_to_math_text(
                &MathTreePosition {
                    host: host.clone(),
                    path: path.clone(),
                    offset: *index,
                    affinity: *affinity,
                },
                text_path,
                *offset,
                *text_affinity,
                &left.target,
            ),
            (
                MathCaretCoordinate::TextFlowUtf8 {
                    path: text_path,
                    offset,
                    affinity: text_affinity,
                },
                MathCaretCoordinate::TreeBoundary {
                    host,
                    path,
                    index,
                    affinity,
                },
            ) => self
                .compare_tree_to_math_text(
                    &MathTreePosition {
                        host: host.clone(),
                        path: path.clone(),
                        offset: *index,
                        affinity: *affinity,
                    },
                    text_path,
                    *offset,
                    *text_affinity,
                    &left.target,
                )
                .map(Ordering::reverse),
            _ => Err(EditError::InvalidPosition),
        }
    }

    fn compare_tree_to_math_text(
        &self,
        tree_position: &MathTreePosition,
        text_path: &MathTextFlowPath,
        text_offset: usize,
        text_affinity: MathPositionAffinity,
        target: &MathEditTarget,
    ) -> Result<Ordering, EditError> {
        let (tree, align_cell) = self.math_tree_for_caret_from_path(text_path, target)?;
        let host = self.math_position_host_for_text_path(text_path, target)?;
        let start = MathTreePosition::from_text_flow_boundary(
            tree,
            host.clone(),
            text_path.steps(),
            align_cell,
            MathTextFlowBoundary::Start,
            text_affinity,
        )
        .map_err(|_| EditError::InvalidPosition)?;
        let end = MathTreePosition::from_text_flow_boundary(
            tree,
            host,
            text_path.steps(),
            align_cell,
            MathTextFlowBoundary::End,
            text_affinity,
        )
        .map_err(|_| EditError::InvalidPosition)?;
        let text_len = super::super::math_edit::math_text_flow_display_len(tree, text_path)
            .ok_or(EditError::InvalidPosition)?;
        if tree_position.path == start.path && tree_position.offset == start.offset {
            if text_offset == 0
                && super::super::math_edit::math_text_flow_boundary_is_shared(
                    tree,
                    text_path,
                    MathTextFlowBoundary::Start,
                )
                .ok_or(EditError::InvalidPosition)?
            {
                return Err(EditError::InvalidPosition);
            }
            if text_len == 0 && text_affinity == MathPositionAffinity::Downstream {
                return Ok(Ordering::Less);
            }
            return Ok(if text_offset == 0 {
                compare_math_position_affinity(tree_position.affinity, text_affinity)
            } else {
                Ordering::Less
            });
        }
        if tree_position.path == end.path && tree_position.offset == end.offset {
            if text_offset == text_len
                && super::super::math_edit::math_text_flow_boundary_is_shared(
                    tree,
                    text_path,
                    MathTextFlowBoundary::End,
                )
                .ok_or(EditError::InvalidPosition)?
            {
                return Err(EditError::InvalidPosition);
            }
            if text_len == 0 && text_affinity == MathPositionAffinity::Upstream {
                return Ok(Ordering::Greater);
            }
            return Ok(if text_offset == text_len {
                compare_math_position_affinity(tree_position.affinity, text_affinity)
            } else {
                Ordering::Greater
            });
        }

        let before = tree_position
            .compare_in_tree(&start, tree)
            .map_err(|_| EditError::InvalidPosition)?;
        if before == Ordering::Less {
            return Ok(Ordering::Less);
        }
        let after = tree_position
            .compare_in_tree(&end, tree)
            .map_err(|_| EditError::InvalidPosition)?;
        if after == Ordering::Greater {
            return Ok(Ordering::Greater);
        }
        Err(EditError::InvalidPosition)
    }

    pub(super) fn math_tree_for_caret<'a>(
        &'a self,
        host: &MathPositionHost,
        target: &MathEditTarget,
    ) -> Result<MathTreeCaretLocation<'a>, EditError> {
        let align_cell = self.align_cell_for_target_and_host(target, host)?;
        let tree =
            super::super::math_edit::math_tree_for_target(&self.document, target, align_cell)
                .ok_or(EditError::InvalidPosition)?;
        Ok((tree, align_cell))
    }

    pub(super) fn math_tree_for_caret_from_path<'a>(
        &'a self,
        path: &MathTextFlowPath,
        target: &MathEditTarget,
    ) -> Result<MathTreeCaretLocation<'a>, EditError> {
        let align_cell = path.align_cell();
        let tree =
            super::super::math_edit::math_tree_for_target(&self.document, target, align_cell)
                .ok_or(EditError::InvalidPosition)?;
        Ok((tree, align_cell))
    }

    pub(super) fn math_position_host_for_text_path(
        &self,
        path: &MathTextFlowPath,
        target: &MathEditTarget,
    ) -> Result<MathPositionHost, EditError> {
        self.math_position_host_for_target(target, path.align_cell())
    }

    pub(super) fn validate_source_caret<'a>(
        &'a self,
        target: &MathEditTarget,
        coordinate: &MathCaretCoordinate,
    ) -> Result<&'a str, EditError> {
        let MathCaretCoordinate::SourceUtf8 {
            offset,
            draft_revision,
            projection_id,
        } = coordinate
        else {
            return Err(EditError::InvalidPosition);
        };
        let (edit, source) = self.resolve_math_source_revision(
            target,
            draft_revision.as_ref(),
            projection_id.as_ref(),
        )?;
        let body = super::super::math_edit::math_source_body_range(edit)
            .ok_or(EditError::InvalidPosition)?;
        let body_source = source.get(body.clone()).ok_or(EditError::InvalidPosition)?;
        if *offset < body.start
            || *offset > body.end
            || *offset > source.len()
            || !source.is_char_boundary(*offset)
            || !GraphemeMap::is_boundary_in(body_source, offset.saturating_sub(body.start))
        {
            return Err(EditError::InvalidPosition);
        }
        Ok(source)
    }
}
