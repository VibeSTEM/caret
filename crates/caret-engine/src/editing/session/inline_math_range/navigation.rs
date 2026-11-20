//! 保留文档锚点，从当前正文、数学项或文本字素继续扩选与折叠。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

impl Session {
    pub(in crate::editing::session) fn focus_inline_range_endpoint(
        &mut self,
        endpoint: &DocumentRangeEndpoint,
        visual: VisualAffinity,
    ) -> Result<(), EditError> {
        let RangeEnd::Math(m) = RangeEnd::resolve(&self.document, &self.paragraph_index, endpoint)?
        else {
            return Err(EditError::InvalidPosition);
        };
        let (offset, branch) = match m.cut.text {
            Some(text) => (
                text.offset,
                MathEditBranch::text(text.path, text.offset, text.affinity, visual),
            ),
            None => (
                m.cut.tree.offset,
                MathEditBranch::Tree {
                    path: m.cut.tree.path.clone(),
                    anchor: m.cut.tree,
                },
            ),
        };
        let edit = MathEdit::entering(m.target, offset, branch, self.current_math_edit_revision());
        self.selection = Selection {
            anchor: m.after.clone(),
            focus: m.after,
        };
        self.set_math_edit(Some(edit));
        self.clear_structure_selection_state();
        self.clear_host_caret_position();
        self.selection_visual_affinity = visual;
        self.selection_anchor_visual_affinity = visual;
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();
        Ok(())
    }

    pub(super) fn step_active_inline_range(
        &mut self,
        range: StructureSelection,
        direction: Direction,
        candidate: Option<&NavigationCandidate>,
    ) -> Result<Effect, EditError> {
        let mut candidate_visual = None;
        let focus = match RangeEnd::resolve(&self.document, &self.paragraph_index, &range.focus)? {
            RangeEnd::Math(m) => {
                if let Some(NavigationCandidate::MathText {
                    focus,
                    visual_affinity,
                    generation,
                }) = candidate
                {
                    self.validate_navigation_generation(generation)?;
                    let MathEditTarget::Inline {
                        paragraph_id,
                        math_id,
                    } = &m.target
                    else {
                        return Err(EditError::InvalidPosition);
                    };
                    let Some(current_text) = m.cut.text.as_ref() else {
                        return Err(EditError::InvalidPosition);
                    };
                    if current_text
                        .compare_in_same_text_island(focus, m.tree)
                        .is_none()
                        || !math_edit::math_text_offsets_are_boundaries(
                            m.tree,
                            &focus.path,
                            &[focus.offset],
                        )
                    {
                        return Err(EditError::InvalidPosition);
                    }
                    candidate_visual = Some(*visual_affinity);
                    DocumentRangeEndpoint::InlineMathText {
                        paragraph_id: paragraph_id.clone(),
                        math_id: math_id.clone(),
                        position: focus.clone(),
                        entry_direction: m.entry_direction,
                        entry_math_forward: m.entry_math_forward,
                    }
                } else if let Some(NavigationCandidate::InlineMathRangeEdge {
                    body_focus,
                    body_visual_affinity,
                    forward,
                    generation,
                }) = candidate
                {
                    self.validate_navigation_generation(generation)?;
                    if *forward != (direction == Direction::Forward) {
                        return Err(EditError::InvalidPosition);
                    }
                    let next = if m.cut.text.is_some() {
                        math_edit::inline_selection_step::step_from_text_visual_edge(
                            m.tree,
                            &range.focus,
                            *forward,
                        )?
                    } else {
                        let tree_forward = (m.entry_direction == direction) == m.entry_math_forward;
                        math_edit::inline_selection_step::step(m.tree, &range.focus, tree_forward)?
                    };
                    match next {
                        Some(next) => next,
                        None => {
                            let body_focus =
                                body_focus.as_ref().ok_or(EditError::InvalidPosition)?;
                            if ![&m.before, &m.after].iter().any(|edge| {
                                edge.story == body_focus.story
                                    && edge.paragraph_id == body_focus.paragraph_id
                                    && edge.offset == body_focus.offset
                            }) {
                                return Err(EditError::InvalidPosition);
                            }
                            candidate_visual = *body_visual_affinity;
                            DocumentRangeEndpoint::Text(body_focus.clone())
                        }
                    }
                } else {
                    let forward = (m.entry_direction == direction) == m.entry_math_forward;
                    math_edit::inline_selection_step::step(m.tree, &range.focus, forward)?
                        .unwrap_or_else(|| {
                            DocumentRangeEndpoint::Text(m.body_edge(forward).clone())
                        })
                }
            }
            RangeEnd::Body(body) => {
                let body = body.clone();
                let next = match candidate {
                    Some(NavigationCandidate::Document {
                        focus,
                        visual_affinity,
                        generation,
                    }) => {
                        self.validate_navigation_generation(generation)?;
                        candidate_visual = Some(*visual_affinity);
                        Some(DocumentRangeEndpoint::Text(focus.clone()))
                    }
                    None => self.next_range_focus_step(&range.focus, direction)?,
                    _ => return Err(EditError::InvalidPosition),
                };
                let Some(next) = next else {
                    return Ok(Effect::default());
                };
                // 从正文回拖到公式后再转向时，保留原数学锚点，并由被跨过的对象判定进入侧。
                if let DocumentRangeEndpoint::Text(next_text) = &next
                    && body.paragraph_id == next_text.paragraph_id
                    && let Some((math_id, forward)) = inline_math_slots(self.paragraph_for(&body)?)?
                        .into_iter()
                        .find_map(|(offset, id)| {
                            inline_math_crossing_direction(body.offset, next_text.offset, offset)
                                .map(|f| (id, f))
                        })
                {
                    let target = MathEditTarget::Inline {
                        paragraph_id: body.paragraph_id.clone(),
                        math_id: math_id.clone(),
                    };
                    let tree = math_tree_for_target(&self.document, &target, None)
                        .ok_or(EditError::InvalidPosition)?;
                    let edge = DocumentRangeEndpoint::InlineMathTree {
                        paragraph_id: body.paragraph_id.clone(),
                        position: MathTreePosition {
                            host: MathPositionHost::InlineMath { owner_id: math_id },
                            path: MathSequencePath::root(),
                            offset: if forward { 0 } else { tree.root.content.len() },
                            affinity: MathPositionAffinity::Downstream,
                        },
                        entry_direction: direction,
                        entry_math_forward: forward,
                    };
                    math_edit::inline_selection_step::step(tree, &edge, forward)?.unwrap_or(next)
                } else {
                    next
                }
            }
            RangeEnd::Block => return self.extend_structure_selection(direction),
        };
        let visual = candidate_visual.unwrap_or_else(|| {
            if direction == Direction::Forward {
                VisualAffinity::Upstream
            } else {
                VisualAffinity::Downstream
            }
        });
        match (&range.anchor, &focus) {
            (DocumentRangeEndpoint::Text(a), DocumentRangeEndpoint::Text(f)) => self
                .set_pointer_selection_range(
                    a.clone(),
                    f.clone(),
                    self.selection_anchor_visual_affinity,
                    visual,
                ),
            (_, DocumentRangeEndpoint::BlockEdge { .. }) => {
                self.commit_range_focus(focus, None, true)
            }
            _ => self.install_inline_math_selection(
                StructureSelection {
                    anchor: range.anchor,
                    focus,
                },
                self.selection_anchor_visual_affinity,
                visual,
            ),
        }
    }
}
