//! 将段落到行内公式根序列的选区解析为同一个正文切口与数学切口。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::math_edit::{inline_math_slots, math_tree_for_target};
use super::*;
use crate::editing::{NavigationCandidate, text_map::OBJECT_REPLACEMENT};

/// 当前文档范围在正文和行内公式根序列中的只读投影，两端均来自同一权威范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineMathRangeParts {
    pub body: Selection,
    pub target: MathEditTarget,
    pub math_anchor: MathTreePosition,
    pub math_focus: MathTreePosition,
    pub entry_direction: Direction,
    pub entry_math_forward: bool,
}

impl InlineMathRangeParts {
    pub(super) fn indices(&self) -> std::ops::Range<usize> {
        let a = self.math_anchor.offset;
        let b = self.math_focus.offset;
        a.min(b)..a.max(b)
    }
}

impl Session {
    /// 返回活动结构选区的正文切口及同宿主数学切口，普通选区返回空。
    pub fn inline_math_range_parts(&self) -> Option<InlineMathRangeParts> {
        self.resolve_inline_math_parts(self.structure_selection.as_ref()?)
            .ok()
            .flatten()
    }

    pub(super) fn resolve_inline_math_parts(
        &self,
        selection: &StructureSelection,
    ) -> Result<Option<InlineMathRangeParts>, EditError> {
        resolve_inline_math_parts(&self.document, &self.paragraph_index, selection)
    }

    pub(super) fn step_inline_math_range_adapter(
        &mut self,
        direction: Direction,
        candidate: Option<&NavigationCandidate>,
    ) -> Result<Option<Effect>, EditError> {
        if let Some(candidate) = candidate {
            let NavigationCandidate::Document {
                generation, focus, ..
            } = candidate
            else {
                return Ok(None);
            };
            if generation.session_id != self.session_id
                || generation.document_generation != self.document_generation
            {
                return Err(EditError::InvalidPosition);
            }
            self.text_map_for(focus)?.validate_pos(focus)?;
        }
        let (anchor, math_id, index, entry_direction, entry_math_forward, math_forward) =
            if let Some(parts) = self.inline_math_range_parts() {
                let MathEditTarget::Inline { math_id, .. } = &parts.target else {
                    return Err(EditError::InvalidPosition);
                };
                if !matches!(&parts.math_anchor.host, MathPositionHost::InlineMath { .. }) {
                    return Err(EditError::InvalidPosition);
                }
                let length = math_edit::math_tree_for_target(&self.document, &parts.target, None)
                    .ok_or(EditError::InvalidPosition)?
                    .root
                    .content
                    .len();
                let from_before = parts.math_anchor.offset == 0;
                if !from_before && parts.math_anchor.offset != length {
                    return Err(EditError::InvalidPosition);
                }
                let math_forward = if direction == parts.entry_direction {
                    parts.entry_math_forward
                } else {
                    !parts.entry_math_forward
                };
                (
                    parts.body.anchor,
                    math_id.clone(),
                    parts.math_focus.offset,
                    parts.entry_direction,
                    parts.entry_math_forward,
                    math_forward,
                )
            } else {
                if self.has_structure_selection() || self.block_boundary_exit().is_some() {
                    return Ok(None);
                }
                let focus = &self.selection.focus;
                let anchor = &self.selection.anchor;
                if focus.paragraph_id != anchor.paragraph_id || focus.story != anchor.story {
                    return Ok(None);
                }
                let paragraph = self.paragraph_for(focus)?;
                let candidate_focus = match candidate {
                    Some(NavigationCandidate::Document { focus, .. }) => Some(focus),
                    Some(_) => return Ok(None),
                    None => None,
                };
                let Some((_, math_id, math_forward)) = inline_math_slots(paragraph)?
                    .into_iter()
                    .find_map(|(offset, math_id)| {
                        if let Some(next) = candidate_focus {
                            inline_math_crossing_direction(focus.offset, next.offset, offset)
                                .map(|math_forward| (offset, math_id, math_forward))
                        } else if direction == Direction::Forward && offset == focus.offset {
                            Some((offset, math_id, true))
                        } else if direction == Direction::Backward
                            && offset + OBJECT_REPLACEMENT.len_utf8() == focus.offset
                        {
                            Some((offset, math_id, false))
                        } else {
                            None
                        }
                    })
                else {
                    return Ok(None);
                };
                if candidate_focus.is_some_and(|next| next.paragraph_id != focus.paragraph_id) {
                    return Ok(None);
                }
                let tree = &math_edit::find_inline_math(&paragraph.inlines, &math_id)
                    .ok_or(EditError::InvalidPosition)?
                    .math_tree;
                if tree.root.content.len() < 2 {
                    return Ok(None);
                }
                (
                    anchor.clone(),
                    math_id,
                    if math_forward {
                        0
                    } else {
                        tree.root.content.len()
                    },
                    direction,
                    math_forward,
                    math_forward,
                )
            };
        let target = MathEditTarget::Inline {
            paragraph_id: anchor.paragraph_id.clone(),
            math_id: math_id.clone(),
        };
        let tree = math_tree_for_target(&self.document, &target, None)
            .ok_or(EditError::InvalidPosition)?;
        let length = tree.root.content.len();
        let next = if math_forward {
            index.checked_add(1)
        } else {
            index.checked_sub(1)
        }
        .ok_or(EditError::InvalidPosition)?;
        if next == 0 || next == length {
            let (before, after) = self.inline_math_slot_bounds(&anchor.paragraph_id, &math_id)?;
            let focus = if next == 0 { before } else { after };
            let visual = super::document_position::visual_affinity_for(focus.affinity);
            return self
                .set_selection_with_visual_affinity(Some(Selection { anchor, focus }), visual)
                .map(Some);
        }
        let position = MathTreePosition {
            host: MathPositionHost::InlineMath { owner_id: math_id },
            path: MathSequencePath::root(),
            offset: next,
            affinity: if math_forward {
                MathPositionAffinity::Upstream
            } else {
                MathPositionAffinity::Downstream
            },
        };
        let range = StructureSelection {
            anchor: DocumentRangeEndpoint::Text(anchor.clone()),
            focus: DocumentRangeEndpoint::InlineMathTree {
                paragraph_id: anchor.paragraph_id,
                position,
                entry_direction,
                entry_math_forward,
            },
        };
        let parts = self
            .resolve_inline_math_parts(&range)?
            .ok_or(EditError::InvalidPosition)?;
        self.install_structure_selection(range)?;
        self.selection = Selection {
            anchor: parts.body.focus.clone(),
            focus: parts.body.focus,
        };
        self.clear_host_caret_position();
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();
        Ok(Some(Effect {
            selection_changed: true,
            ..Default::default()
        }))
    }
}

/// 从同一代正文候选判断视觉移动实际跨过了 InlineMath 的哪一侧。
fn inline_math_crossing_direction(current: usize, next: usize, start: usize) -> Option<bool> {
    let end = start.checked_add(OBJECT_REPLACEMENT.len_utf8())?;
    if current == start && next == end {
        Some(true)
    } else if current == end && next == start {
        Some(false)
    } else {
        None
    }
}

impl DocumentRangeEndpoint {
    pub(super) fn as_document_caret(&self) -> Result<DocumentCaretPosition, EditError> {
        match self {
            Self::Text(p) => Ok(DocumentCaretPosition::Text(p.clone())),
            Self::BlockEdge { block_id, side } => Ok(DocumentCaretPosition::Host(
                HostCaretPosition::outside(block_id.clone(), *side),
            )),
            Self::InlineMathTree {
                paragraph_id,
                position,
                ..
            } => {
                let MathPositionHost::InlineMath { owner_id } = &position.host else {
                    return Err(EditError::InvalidPosition);
                };
                Ok(DocumentCaretPosition::Host(HostCaretPosition::Inside {
                    kind: HostCaretKind::Inline,
                    host_id: owner_id.clone(),
                    local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                        target: MathEditTarget::Inline {
                            paragraph_id: paragraph_id.clone(),
                            math_id: owner_id.clone(),
                        },
                        coordinate: MathCaretCoordinate::TreeBoundary {
                            host: position.host.clone(),
                            path: position.path.clone(),
                            index: position.offset,
                            affinity: position.affinity,
                        },
                    }),
                }))
            }
        }
    }
}

pub(super) fn resolve_inline_math_parts(
    document: &Document,
    index: &ParagraphIndex,
    selection: &StructureSelection,
) -> Result<Option<InlineMathRangeParts>, EditError> {
    let (
        DocumentRangeEndpoint::Text(anchor),
        DocumentRangeEndpoint::InlineMathTree {
            paragraph_id,
            position,
            entry_direction,
            entry_math_forward,
        },
    ) = (&selection.anchor, &selection.focus)
    else {
        return Ok(None);
    };
    let MathPositionHost::InlineMath { owner_id } = &position.host else {
        return Err(EditError::InvalidPosition);
    };
    if &anchor.paragraph_id != paragraph_id || position.path != MathSequencePath::root() {
        return Err(EditError::InvalidPosition);
    }
    let location = index.get(paragraph_id).ok_or(EditError::InvalidPosition)?;
    if anchor.story != location.story {
        return Err(EditError::WrongStory);
    }
    let paragraph =
        paragraph_at_location(&document.body, location).ok_or(EditError::InvalidPosition)?;
    TextMap::new(paragraph)?.validate_pos(anchor)?;
    let target = MathEditTarget::Inline {
        paragraph_id: paragraph_id.clone(),
        math_id: owner_id.clone(),
    };
    let tree = math_tree_for_target(document, &target, None).ok_or(EditError::InvalidPosition)?;
    position
        .validate_reachable(tree)
        .map_err(|_| EditError::InvalidPosition)?;
    let (before, after) =
        math_edit::selection_extension::slot_bounds(paragraph, &location.story, owner_id)?;
    let (edge, offset) = if anchor.offset <= before.offset {
        (before, 0)
    } else if anchor.offset >= after.offset {
        (after, tree.root.content.len())
    } else {
        return Err(EditError::InvalidPosition);
    };
    if position.offset == 0 || position.offset == tree.root.content.len() {
        return Err(EditError::InvalidPosition);
    }
    Ok(Some(InlineMathRangeParts {
        body: Selection {
            anchor: anchor.clone(),
            focus: edge,
        },
        target,
        math_anchor: MathTreePosition {
            offset,
            ..position.clone()
        },
        math_focus: position.clone(),
        entry_direction: *entry_direction,
        entry_math_forward: *entry_math_forward,
    }))
}
