//! 依据 MathLayout 几何生成并排序公式内部的垂直导航候选。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{
    DocumentLayoutSnapshot, MathTextVerticalTarget, MathTreeVerticalTarget, PageCaretGeometry,
    PageMathStructureHit, math_environment_at_layout_path, math_flow_at_layout_path,
    math_sequence_path_for_environment_children, math_sequence_path_for_flow,
};
use crate::{
    MathEditTarget, MathPositionAffinity, MathPositionHost, MathSequencePath, MathSequenceStep,
    MathTextFlowPath, MathTextPosition, MathTreePosition, VisualAffinity,
};

struct MathTreeSearch<'a> {
    current: PageCaretGeometry,
    path: &'a [MathSequenceStep],
    offset: usize,
    position: Option<&'a MathTreePosition>,
    host: Option<&'a crate::MathPositionHost>,
    line_delta: isize,
    preferred_x: f32,
}

impl DocumentLayoutSnapshot {
    /// 在当前公式里寻找方向上的最近数学停靠点，保留脚本/槽位的实际光标几何。
    pub fn move_math_tree_vertical_target(
        &self,
        target: &MathEditTarget,
        position: &MathTreePosition,
        line_delta: isize,
        preferred_x: Option<f32>,
    ) -> Option<MathTreeVerticalTarget> {
        if line_delta == 0 {
            return None;
        }
        let current = self.math_tree_caret_geometry(target, position)?;
        let preferred_x = preferred_x
            .filter(|value| value.is_finite())
            .unwrap_or(current.x_pt as f32);
        let mut best: Option<((usize, f64, f64, u8), MathTreeVerticalTarget)> = None;
        let mut consider = |caret: PageCaretGeometry, candidate: MathTreeVerticalTarget| {
            let Some((page_distance, vertical_distance, horizontal_distance)) =
                vertical_caret_key(current, caret, line_delta, preferred_x)
            else {
                return;
            };
            let kind_priority = match candidate {
                MathTreeVerticalTarget::Text(_) => 0,
                MathTreeVerticalTarget::Structure(_) => 1,
            };
            let key = (
                page_distance,
                vertical_distance,
                horizontal_distance,
                kind_priority,
            );
            if best.as_ref().is_none_or(|(best_key, _)| key < *best_key) {
                best = Some((key, candidate));
            }
        };

        if let Some(hit) = self.nearest_math_tree_structure_hit(
            target,
            MathTreeSearch {
                current,
                path: position.path.steps(),
                offset: position.offset,
                position: Some(position),
                host: Some(&position.host),
                line_delta,
                preferred_x,
            },
        ) {
            consider(hit.caret, MathTreeVerticalTarget::Structure(hit));
        }

        for page_index in 0..self.pages.len() {
            for stop in self.math_text_drag_stops(page_index) {
                if &stop.target != target {
                    continue;
                }
                let Some((candidate_path, _)) = math_sequence_path_for_text(&stop.position.path)
                else {
                    continue;
                };
                let Some(candidate_host) = self.math_text_host(target, &stop.position) else {
                    continue;
                };
                if !math_tree_candidate_is_vertical_neighbor(
                    &position.host,
                    position.path.steps(),
                    position.offset,
                    &candidate_host,
                    candidate_path.steps(),
                ) {
                    continue;
                }
                let Some(hit) = self.math_text_drag_hit(
                    page_index,
                    stop.token,
                    target,
                    &stop.position,
                    stop.order_utf8,
                    stop.visual_affinity,
                ) else {
                    continue;
                };
                consider(hit.caret, MathTreeVerticalTarget::Text(hit));
            }
        }
        best.map(|(_, candidate)| candidate)
    }

    /// 在 TextFlow 内先使用 Parley 移动；到达岛边后比较相邻文本与结构停靠点。
    pub fn move_math_text_vertical_target(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        line_delta: isize,
        preferred_x: Option<f32>,
    ) -> Option<MathTextVerticalTarget> {
        if let Some((position, visual_affinity, preferred_x_pt)) =
            self.move_math_text_vertical(target, position, visual_affinity, line_delta, preferred_x)
        {
            return Some(MathTextVerticalTarget::SameTextIsland {
                position,
                visual_affinity,
                preferred_x_pt,
            });
        }
        let current = self.math_text_caret_geometry(target, position, visual_affinity)?;
        let preferred_x = preferred_x.unwrap_or(current.x_pt as f32);
        let text_hit = self.adjacent_math_text_hit(
            target,
            position,
            visual_affinity,
            line_delta,
            Some(preferred_x),
        );
        let (sequence_path, content_index) = math_sequence_path_for_text(&position.path)?;
        let current_host = self.math_text_host(target, position);
        let structure_hit = self.nearest_math_tree_structure_hit(
            target,
            MathTreeSearch {
                current,
                path: sequence_path.steps(),
                offset: content_index,
                position: None,
                host: current_host.as_ref(),
                line_delta,
                preferred_x,
            },
        );
        let text_key = text_hit.and_then(|hit| {
            vertical_caret_key(current, hit.caret, line_delta, preferred_x).map(|key| (key, hit))
        });
        let structure_key = structure_hit.and_then(|hit| {
            vertical_caret_key(current, hit.caret, line_delta, preferred_x).map(|key| (key, hit))
        });
        match (text_key, structure_key) {
            (Some((text_key, text)), Some((structure_key, structure))) => {
                if text_key <= structure_key {
                    Some(MathTextVerticalTarget::DifferentTextIsland(text))
                } else {
                    Some(MathTextVerticalTarget::DifferentMathStructure(structure))
                }
            }
            (Some((_, text)), None) => Some(MathTextVerticalTarget::DifferentTextIsland(text)),
            (None, Some((_, structure))) => {
                Some(MathTextVerticalTarget::DifferentMathStructure(structure))
            }
            (None, None) => None,
        }
    }

    fn nearest_math_tree_structure_hit(
        &self,
        target: &MathEditTarget,
        search: MathTreeSearch<'_>,
    ) -> Option<PageMathStructureHit> {
        let mut best: Option<((usize, f64, f64), PageMathStructureHit)> = None;
        for projection in self.math_projections_for_target(target) {
            let Some((layout, _)) = self.math_projection_items(projection) else {
                continue;
            };
            let Some(host) = self.math_host(&projection.math_id) else {
                continue;
            };
            for node in &layout.nodes {
                if !node.width.is_finite()
                    || !node.height.is_finite()
                    || node.width <= 0.0
                    || node.height <= 0.0
                {
                    continue;
                }
                let Some((path, contents)) = (|| {
                    if node.is_flow && !node.is_text_flow {
                        let root = self.math_scope_root(projection, &node.scope_id)?;
                        let flow = math_flow_at_layout_path(root, &node.path)?;
                        Some((
                            math_sequence_path_for_flow(&node.path)?,
                            flow.content.as_slice(),
                        ))
                    } else if node.is_container {
                        let root = self.math_scope_root(projection, &node.scope_id)?;
                        let environment = math_environment_at_layout_path(root, &node.path)?;
                        Some((
                            math_sequence_path_for_environment_children(&node.path)?,
                            environment.children.as_slice(),
                        ))
                    } else {
                        None
                    }
                })() else {
                    continue;
                };
                let Some(scope_host) = host.for_scope(&node.scope_id) else {
                    continue;
                };
                for offset in 0..=contents.len() {
                    if offset.checked_sub(1).is_some_and(|index| {
                        matches!(
                            contents.get(index),
                            Some(caret_doc::MathContent::Math(node))
                                if node.kind == caret_doc::MathNodeKind::Placeholder
                        )
                    }) {
                        continue;
                    }
                    let candidate_position = MathTreePosition {
                        host: scope_host.host.clone(),
                        path: path.clone(),
                        offset,
                        affinity: if offset == 0 {
                            MathPositionAffinity::Downstream
                        } else {
                            MathPositionAffinity::Upstream
                        },
                    };
                    if search.position == Some(&candidate_position)
                        || !search.host.is_some_and(|current_host| {
                            math_tree_candidate_is_vertical_neighbor(
                                current_host,
                                search.path,
                                search.offset,
                                &candidate_position.host,
                                candidate_position.path.steps(),
                            )
                        })
                    {
                        continue;
                    }
                    let Some(caret) = self.math_tree_caret_geometry(target, &candidate_position)
                    else {
                        continue;
                    };
                    let Some(key) = vertical_caret_key(
                        search.current,
                        caret,
                        search.line_delta,
                        search.preferred_x,
                    ) else {
                        continue;
                    };
                    if best.as_ref().is_none_or(|(best_key, _)| key < *best_key) {
                        best = Some((
                            key,
                            PageMathStructureHit {
                                session_id: self.session_id.clone(),
                                document_generation: self.document_generation,
                                font_generation: self.font_generation,
                                target: target.clone(),
                                position: candidate_position,
                                caret,
                            },
                        ));
                    }
                }
            }
        }
        best.map(|(_, hit)| hit)
    }
}

pub(super) fn math_text_candidate_is_vertical_neighbor(
    current_host: &MathPositionHost,
    current_path: &[MathSequenceStep],
    current_offset: usize,
    candidate_host: &MathPositionHost,
    candidate_path: &MathTextFlowPath,
) -> bool {
    if let (
        MathPositionHost::AlignCell {
            row_id: current_row,
            cell_id: current_cell,
        },
        MathPositionHost::AlignCell {
            row_id: candidate_row,
            cell_id: candidate_cell,
        },
    ) = (current_host, candidate_host)
    {
        if current_row != candidate_row {
            return true;
        }
        if current_cell != candidate_cell {
            return false;
        }
    }
    let Ok((candidate_path, _)) =
        MathTreePosition::sequence_boundary_from_text_path(candidate_path.steps())
    else {
        return false;
    };
    math_tree_paths_are_vertical_neighbors(current_path, candidate_path.steps(), current_offset)
}

fn math_tree_candidate_is_vertical_neighbor(
    current_host: &MathPositionHost,
    current_path: &[MathSequenceStep],
    current_offset: usize,
    candidate_host: &MathPositionHost,
    candidate_path: &[MathSequenceStep],
) -> bool {
    if let (
        MathPositionHost::AlignCell {
            row_id: current_row,
            cell_id: current_cell,
        },
        MathPositionHost::AlignCell {
            row_id: candidate_row,
            cell_id: candidate_cell,
        },
    ) = (current_host, candidate_host)
    {
        if current_row != candidate_row {
            return true;
        }
        if current_cell != candidate_cell {
            return false;
        }
    }
    math_tree_paths_are_vertical_neighbors(current_path, candidate_path, current_offset)
}

fn math_sequence_path_for_text(text_path: &MathTextFlowPath) -> Option<(MathSequencePath, usize)> {
    MathTreePosition::sequence_boundary_from_text_path(text_path.steps()).ok()
}

fn vertical_caret_key(
    current: PageCaretGeometry,
    candidate: PageCaretGeometry,
    line_delta: isize,
    preferred_x: f32,
) -> Option<(usize, f64, f64)> {
    if line_delta == 0 {
        return None;
    }
    let current_y = (current.y0_pt + current.y1_pt) / 2.0;
    let candidate_y = (candidate.y0_pt + candidate.y1_pt) / 2.0;
    if candidate.page_index != current.page_index {
        let is_ahead = if line_delta > 0 {
            candidate.page_index > current.page_index
        } else {
            candidate.page_index < current.page_index
        };
        if !is_ahead {
            return None;
        }
        return Some((
            candidate.page_index.abs_diff(current.page_index),
            if line_delta > 0 {
                candidate_y
            } else {
                -candidate_y
            },
            (candidate.x_pt - f64::from(preferred_x)).abs(),
        ));
    }
    let delta_y = candidate_y - current_y;
    let threshold = (current.y1_pt - current.y0_pt)
        .abs()
        .min((candidate.y1_pt - candidate.y0_pt).abs())
        * 0.25;
    if (line_delta > 0 && delta_y <= threshold) || (line_delta < 0 && delta_y >= -threshold) {
        return None;
    }
    Some((
        0,
        delta_y.abs(),
        (candidate.x_pt - f64::from(preferred_x)).abs(),
    ))
}

fn math_tree_paths_are_vertical_neighbors(
    current: &[MathSequenceStep],
    candidate: &[MathSequenceStep],
    current_offset: usize,
) -> bool {
    if current == candidate {
        return true;
    }
    for index in 0..current.len().min(candidate.len()) {
        if current[index] != candidate[index] {
            return matches!(
                (current[index], candidate[index]),
                (
                    MathSequenceStep::Operand {
                        content_index: current_content,
                        operand_index: current_operand,
                    },
                    MathSequenceStep::Operand {
                        content_index: candidate_content,
                        operand_index: candidate_operand,
                    }
                ) if current_content == candidate_content && current_operand != candidate_operand
            );
        }
    }
    if candidate.len() == current.len() + 1
        && candidate.starts_with(current)
        && matches!(
            candidate.last(),
            Some(MathSequenceStep::Operand { content_index, .. })
                if *content_index == current_offset || content_index.saturating_add(1) == current_offset
        )
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use caret_doc::NodeId;

    #[test]
    fn textflow_candidates_must_belong_to_the_current_math_structure() {
        let host = MathPositionHost::Equation {
            owner_id: NodeId::generate(),
        };
        let current_path = [MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }];
        let other_operand = MathTextFlowPath::from_interaction_path(
            vec![
                caret_doc::MathPathStep::Content(0),
                caret_doc::MathPathStep::Operand(1),
                caret_doc::MathPathStep::Content(0),
            ],
            None,
        );
        let adjacent_fraction = MathTextFlowPath::from_interaction_path(
            vec![
                caret_doc::MathPathStep::Content(1),
                caret_doc::MathPathStep::Operand(0),
                caret_doc::MathPathStep::Content(0),
            ],
            None,
        );

        assert!(math_text_candidate_is_vertical_neighbor(
            &host,
            &current_path,
            1,
            &host,
            &other_operand,
        ));
        assert!(!math_text_candidate_is_vertical_neighbor(
            &host,
            &current_path,
            1,
            &host,
            &adjacent_fraction,
        ));
    }
}
