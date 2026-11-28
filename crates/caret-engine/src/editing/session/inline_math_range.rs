//! 将正文与行内公式序列的选区解析为同一个正文切口与数学切口。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod formula_range;
mod navigation;
use super::math_edit::{inline_math_slots, math_tree_for_target};
use super::*;
use crate::editing::{NavigationCandidate, text_map::OBJECT_REPLACEMENT};
use formula_range::{FormulaRange, LocalCut};

/// 当前文档范围在正文和行内公式序列中的只读投影，两端均来自同一权威范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineMathRangeParts {
    pub body: Selection,
    formulae: Vec<FormulaRange>,
}

impl InlineMathRangeParts {
    pub(super) fn formula_ranges(&self) -> impl Iterator<Item = (&MathEditTarget, &FormulaRange)> {
        self.formulae.iter().map(|range| (&range.target, range))
    }

    pub(super) fn copy_with_body(
        &self,
        document: &Document,
        body: String,
    ) -> Result<String, EditError> {
        let mut result = String::new();
        let mut suffix = String::new();
        for range in &self.formulae {
            let tree = math_tree_for_target(document, &range.target, None)
                .ok_or(EditError::InvalidPosition)?;
            range.copy_into(
                tree,
                if range.before_body {
                    &mut result
                } else {
                    &mut suffix
                },
            )?;
        }
        result.push_str(&body);
        result.push_str(&suffix);
        Ok(result)
    }
}

impl Session {
    /// 返回行内公式对象在正文中的两个 UTF-8 边界，供不兼容数学局部路径退回对象范围。
    pub fn inline_math_pointer_bounds(
        &self,
        target: &MathEditTarget,
    ) -> Result<(Pos, Pos), EditError> {
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = target
        else {
            return Err(EditError::InvalidPosition);
        };
        self.inline_math_slot_bounds(paragraph_id, math_id)
    }

    /// 安装正文、公式序列或公式文本之间的文档范围，保留手势的锚点和焦点。
    ///
    /// 跨宿主替换落在结构删除后合并的正文切口；同一数学文本岛内的编辑使用本地文本选区。
    pub fn set_pointer_inline_math_selection(
        &mut self,
        range: StructureSelection,
        anchor_visual_affinity: caret_layout::VisualAffinity,
        focus_visual_affinity: caret_layout::VisualAffinity,
    ) -> Result<Effect, EditError> {
        self.install_inline_math_selection_inner(
            range,
            anchor_visual_affinity,
            focus_visual_affinity,
            true,
        )
    }

    /// 键盘范围保留进入公式时的视觉方向与局部步进关系，包括反向跨过原锚点的情况。
    pub(super) fn install_inline_math_selection(
        &mut self,
        range: StructureSelection,
        anchor_visual_affinity: VisualAffinity,
        focus_visual_affinity: VisualAffinity,
    ) -> Result<Effect, EditError> {
        self.install_inline_math_selection_inner(
            range,
            anchor_visual_affinity,
            focus_visual_affinity,
            false,
        )
    }

    fn install_inline_math_selection_inner(
        &mut self,
        range: StructureSelection,
        anchor_visual_affinity: VisualAffinity,
        focus_visual_affinity: VisualAffinity,
        promote_across_table_cells: bool,
    ) -> Result<Effect, EditError> {
        if let Some(id) = self.composition.as_ref().map(|composition| composition.id) {
            // 指针端点无效时须保留预编辑，因此预编辑撤销与选区安装在临时会话中一起提交。
            let mut candidate = self.clone_preserving_identity();
            let cancel = candidate.cancel_composition(id)?;
            let mut effect = candidate.install_inline_math_selection_inner(
                range,
                anchor_visual_affinity,
                focus_visual_affinity,
                promote_across_table_cells,
            )?;
            effect.composition_changed |= cancel.composition_changed;
            *self = candidate;
            return Ok(effect);
        }
        self.ensure_math_edit_revision_current()?;
        if self.has_uncommitted_math_source_draft() {
            return Err(EditError::InvalidMath(
                "resolve or cancel the incomplete math source draft before dragging a selection"
                    .to_owned(),
            ));
        }
        if promote_across_table_cells
            && self
                .crossed_pointer_tables(&range.anchor, &range.focus)
                .is_some()
        {
            // 分别解析两端，先确认数学宿主、局部路径及正文坐标都有效；成对投影会
            // 拒绝 TableCell story 不同的端点，跨单元格指针范围应先提升到表格边界。
            for endpoint in [&range.anchor, &range.focus] {
                match endpoint {
                    DocumentRangeEndpoint::BlockEdge { .. } => {
                        self.compare_range_endpoints(endpoint, endpoint)?;
                    }
                    _ => {
                        if let RangeEnd::Math(math_end) =
                            RangeEnd::resolve(&self.document, &self.paragraph_index, endpoint)?
                        {
                            FormulaRange::between(
                                math_end.target.clone(),
                                math_end.tree,
                                &math_end.cut,
                                &math_end.cut,
                                false,
                            )?;
                        }
                    }
                }
            }
            if let Some(effect) = self.promote_pointer_range_across_cells(
                range.anchor.clone(),
                range.focus.clone(),
                anchor_visual_affinity,
                focus_visual_affinity,
            )? {
                return Ok(effect);
            }
        }
        let parts = self
            .resolve_inline_math_parts(&range)?
            .ok_or(EditError::InvalidPosition)?;
        let order = self.compare_range_endpoints(&range.anchor, &range.focus)?;
        let has_block_endpoint = matches!(range.anchor, DocumentRangeEndpoint::BlockEdge { .. })
            || matches!(range.focus, DocumentRangeEndpoint::BlockEdge { .. });
        let covers_block = if has_block_endpoint {
            let (start, end) = if order.is_gt() {
                (&range.focus, &range.anchor)
            } else {
                (&range.anchor, &range.focus)
            };
            let (environments, math) = self.covered_block_nodes(start, end)?;
            !environments.is_empty() || !math.is_empty()
        } else {
            false
        };
        if !covers_block
            && parts.formulae.iter().all(FormulaRange::is_empty)
            && parts.body.anchor.paragraph_id == parts.body.focus.paragraph_id
            && parts.body.anchor.offset == parts.body.focus.offset
        {
            if let DocumentRangeEndpoint::Text(text) = &range.focus {
                self.set_selection(Some(Selection {
                    anchor: text.clone(),
                    focus: text.clone(),
                }))?;
                self.clear_math_edit();
            } else if matches!(range.focus, DocumentRangeEndpoint::BlockEdge { .. }) {
                let (caret, host) = self.park_at_range_endpoint(&range.focus)?;
                self.clear_structure_selection_state();
                self.clear_math_edit();
                self.selection = Selection {
                    anchor: caret.clone(),
                    focus: caret,
                };
                self.set_host_caret_position(host);
            } else {
                self.focus_inline_range_endpoint(&range.focus, focus_visual_affinity)?;
            }
        } else {
            self.install_structure_selection(range)?;
            self.clear_math_edit();
            self.clear_host_caret_position();
            self.selection = Selection {
                anchor: parts.body.focus.clone(),
                focus: parts.body.focus,
            };
        }
        self.selection_anchor_visual_affinity = anchor_visual_affinity;
        self.selection_visual_affinity = focus_visual_affinity;
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    /// 返回活动结构选区的正文切口及两端公式中被选中的内容。
    pub fn inline_math_range_parts(&self) -> Option<InlineMathRangeParts> {
        self.resolve_inline_math_parts(self.structure_selection.as_ref()?)
            .ok()
            .flatten()
    }

    pub(super) fn resolve_inline_math_parts(
        &self,
        selection: &StructureSelection,
    ) -> Result<Option<InlineMathRangeParts>, EditError> {
        let (block, math, block_is_anchor) = match (&selection.anchor, &selection.focus) {
            (block @ DocumentRangeEndpoint::BlockEdge { .. }, math) if math.is_inline_math() => {
                (block, math, true)
            }
            (math, block @ DocumentRangeEndpoint::BlockEdge { .. }) if math.is_inline_math() => {
                (block, math, false)
            }
            _ => {
                return resolve_inline_math_parts(&self.document, &self.paragraph_index, selection);
            }
        };
        // 文字切口留在公式的 Story 内，块覆盖继续使用原始端点。
        let block_precedes_math = self.compare_range_endpoints(block, math)?.is_lt();
        let direction = if block_precedes_math {
            Direction::Forward
        } else {
            Direction::Backward
        };
        let RangeEnd::Math(math_end) =
            RangeEnd::resolve(&self.document, &self.paragraph_index, math)?
        else {
            return Err(EditError::InvalidPosition);
        };
        let projected = self
            .text_endpoint_on_side(block, direction, Some(&math_end.before.story))
            .ok_or(EditError::InvalidPosition)?;
        let mut range = selection.clone();
        if block_is_anchor {
            range.anchor = DocumentRangeEndpoint::Text(projected);
        } else {
            range.focus = DocumentRangeEndpoint::Text(projected);
        }
        resolve_inline_math_parts(&self.document, &self.paragraph_index, &range)
    }

    pub(super) fn step_inline_math_range_adapter(
        &mut self,
        direction: Direction,
        candidate: Option<&NavigationCandidate>,
    ) -> Result<Option<Effect>, EditError> {
        if let Some(candidate) = candidate {
            match candidate {
                NavigationCandidate::Document {
                    generation, focus, ..
                } => {
                    self.validate_navigation_generation(generation)?;
                    self.text_map_for(focus)?.validate_pos(focus)?;
                }
                NavigationCandidate::MathText { generation, .. }
                | NavigationCandidate::InlineMathRangeEdge { generation, .. }
                | NavigationCandidate::MathTextHit { generation, .. }
                | NavigationCandidate::MathTreeHit { generation, .. } => {
                    self.validate_navigation_generation(generation)?;
                }
                NavigationCandidate::BlockEdge { .. } => {}
            }
        }
        if let Some(range) = self.structure_selection.clone()
            && (range.anchor.is_inline_math() || range.focus.is_inline_math())
        {
            return self
                .step_active_inline_range(range, direction, candidate)
                .map(Some);
        }
        if self.has_structure_selection() || self.block_boundary_exit().is_some() {
            return Ok(None);
        }
        let focus = &self.selection.focus;
        let candidate_focus = match candidate {
            Some(NavigationCandidate::Document { focus: next, .. })
                if next.paragraph_id == focus.paragraph_id =>
            {
                Some(next)
            }
            Some(_) => return Ok(None),
            None => None,
        };
        let paragraph = self.paragraph_for(focus)?;
        let Some((math_id, forward)) =
            inline_math_slots(paragraph)?
                .into_iter()
                .find_map(|(offset, id)| {
                    let forward = match candidate_focus {
                        Some(next) => {
                            inline_math_crossing_direction(focus.offset, next.offset, offset)?
                        }
                        None if direction == Direction::Forward && offset == focus.offset => true,
                        None if direction == Direction::Backward
                            && offset + OBJECT_REPLACEMENT.len_utf8() == focus.offset =>
                        {
                            false
                        }
                        None => return None,
                    };
                    Some((id, forward))
                })
        else {
            return Ok(None);
        };
        let tree = &math_edit::find_inline_math(&paragraph.inlines, &math_id)
            .ok_or(EditError::InvalidPosition)?
            .math_tree;
        let edge = DocumentRangeEndpoint::InlineMathTree {
            paragraph_id: focus.paragraph_id.clone(),
            position: MathTreePosition {
                host: MathPositionHost::InlineMath { owner_id: math_id },
                path: MathSequencePath::root(),
                offset: if forward { 0 } else { tree.root.content.len() },
                affinity: MathPositionAffinity::Downstream,
            },
            entry_direction: direction,
            entry_math_forward: forward,
        };
        let Some(next) = math_edit::inline_selection_step::step(tree, &edge, forward)? else {
            return Ok(None);
        };
        self.install_inline_math_selection(
            StructureSelection {
                anchor: DocumentRangeEndpoint::Text(self.selection.anchor.clone()),
                focus: next,
            },
            self.selection_anchor_visual_affinity,
            if forward {
                VisualAffinity::Upstream
            } else {
                VisualAffinity::Downstream
            },
        )
        .map(Some)
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
    pub(super) fn is_inline_math(&self) -> bool {
        matches!(
            self,
            Self::InlineMathTree { .. } | Self::InlineMathText { .. }
        )
    }

    pub(super) fn as_document_caret(&self) -> Result<DocumentCaretPosition, EditError> {
        match self {
            Self::Text(p) => Ok(DocumentCaretPosition::Text(p.clone())),
            Self::BlockEdge { block_id, side } => Ok(DocumentCaretPosition::Host(
                HostCaretPosition::outside(block_id.clone(), *side),
            )),
            Self::InlineMathText {
                paragraph_id,
                math_id,
                position,
                ..
            } => Ok(DocumentCaretPosition::Host(HostCaretPosition::Inside {
                kind: HostCaretKind::Inline,
                host_id: math_id.clone(),
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: MathEditTarget::Inline {
                        paragraph_id: paragraph_id.clone(),
                        math_id: math_id.clone(),
                    },
                    coordinate: MathCaretCoordinate::TextFlowUtf8 {
                        path: position.path.clone(),
                        offset: position.offset,
                        affinity: position.affinity,
                    },
                }),
            })),
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

fn resolve_inline_math_parts(
    document: &Document,
    index: &ParagraphIndex,
    selection: &StructureSelection,
) -> Result<Option<InlineMathRangeParts>, EditError> {
    if !selection.anchor.is_inline_math() && !selection.focus.is_inline_math() {
        return Ok(None);
    }
    let a = RangeEnd::resolve(document, index, &selection.anchor)?;
    let b = RangeEnd::resolve(document, index, &selection.focus)?;
    let (body, formulae) = match (&a, &b) {
        (RangeEnd::Math(left), RangeEnd::Math(right)) if left.target == right.target => (
            Selection {
                anchor: left.before.clone(),
                focus: left.before.clone(),
            },
            vec![left.range(&left.cut, &right.cut, false)?],
        ),
        (RangeEnd::Math(anchor), RangeEnd::Math(focus)) => {
            if anchor.before.story != focus.before.story {
                return Err(EditError::WrongStory);
            }
            let forward = before_or_equal(document, index, &anchor.after, &focus.before)?;
            let (first, last) = if forward {
                (anchor, focus)
            } else {
                (focus, anchor)
            };
            let prefix = first.range(&first.cut, &first.edge(true), true)?;
            let suffix = last.range(&last.edge(false), &last.cut, false)?;
            (
                Selection {
                    anchor: anchor.body_edge(forward).clone(),
                    focus: focus.body_edge(!forward).clone(),
                },
                vec![prefix, suffix],
            )
        }
        (RangeEnd::Body(text), RangeEnd::Math(m)) | (RangeEnd::Math(m), RangeEnd::Body(text)) => {
            if text.story != m.before.story {
                return Err(EditError::WrongStory);
            }
            let ahead = before_or_equal(document, index, text, &m.before)?;
            if !ahead && !before_or_equal(document, index, &m.after, text)? {
                return Err(EditError::InvalidPosition);
            }
            let range = m.range(&m.edge(!ahead), &m.cut, !ahead)?;
            let body_edge = m.body_edge(!ahead).clone();
            let text = (*text).clone();
            let (anchor, focus) = if matches!(a, RangeEnd::Body(_)) {
                (text, body_edge)
            } else {
                (body_edge, text)
            };
            (Selection { anchor, focus }, vec![range])
        }
        _ => return Ok(None),
    };
    Ok(Some(InlineMathRangeParts { body, formulae }))
}

fn before_or_equal(
    document: &Document,
    index: &ParagraphIndex,
    a: &Pos,
    b: &Pos,
) -> Result<bool, EditError> {
    Ok(!crate::editing::paragraph_ops::compare_positions(&document.body, index, a, b)?.is_gt())
}

struct MathEnd<'a> {
    target: MathEditTarget,
    tree: &'a caret_doc::MathTree,
    before: Pos,
    after: Pos,
    cut: LocalCut,
    entry_direction: Direction,
    entry_math_forward: bool,
}

impl MathEnd<'_> {
    fn edge(&self, after: bool) -> LocalCut {
        LocalCut::boundary(MathTreePosition {
            path: MathSequencePath::root(),
            offset: if after {
                self.tree.root.content.len()
            } else {
                0
            },
            ..self.cut.tree.clone()
        })
    }
    fn body_edge(&self, after: bool) -> &Pos {
        if after { &self.after } else { &self.before }
    }
    fn range(
        &self,
        a: &LocalCut,
        b: &LocalCut,
        before_body: bool,
    ) -> Result<FormulaRange, EditError> {
        FormulaRange::between(self.target.clone(), self.tree, a, b, before_body)
    }
}

enum RangeEnd<'a> {
    Body(&'a Pos),
    Math(Box<MathEnd<'a>>),
    Block,
}

impl<'a> RangeEnd<'a> {
    fn resolve(
        document: &'a Document,
        index: &ParagraphIndex,
        endpoint: &'a DocumentRangeEndpoint,
    ) -> Result<Self, EditError> {
        let (paragraph_id, math_id, cut, entry_direction, entry_math_forward) = match endpoint {
            DocumentRangeEndpoint::BlockEdge { .. } => return Ok(Self::Block),
            DocumentRangeEndpoint::Text(text) => {
                let location = index
                    .get(&text.paragraph_id)
                    .ok_or(EditError::InvalidPosition)?;
                if location.story != text.story {
                    return Err(EditError::WrongStory);
                }
                TextMap::new(
                    paragraph_at_location(&document.body, location)
                        .ok_or(EditError::InvalidPosition)?,
                )?
                .validate_pos(text)?;
                return Ok(Self::Body(text));
            }
            DocumentRangeEndpoint::InlineMathTree {
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
                    LocalCut::boundary(position.clone()),
                    entry_direction,
                    entry_math_forward,
                )
            }
            DocumentRangeEndpoint::InlineMathText {
                paragraph_id,
                math_id,
                position,
                entry_direction,
                entry_math_forward,
            } => {
                let host = MathPositionHost::InlineMath {
                    owner_id: math_id.clone(),
                };
                (
                    paragraph_id,
                    math_id,
                    LocalCut::text(host, position)?,
                    entry_direction,
                    entry_math_forward,
                )
            }
        };
        let location = index.get(paragraph_id).ok_or(EditError::InvalidPosition)?;
        let paragraph =
            paragraph_at_location(&document.body, location).ok_or(EditError::InvalidPosition)?;
        let target = MathEditTarget::Inline {
            paragraph_id: paragraph_id.clone(),
            math_id: math_id.clone(),
        };
        let tree =
            math_tree_for_target(document, &target, None).ok_or(EditError::InvalidPosition)?;
        let (before, after) =
            math_edit::selection_extension::slot_bounds(paragraph, &location.story, math_id)?;
        Ok(Self::Math(Box::new(MathEnd {
            target,
            tree,
            before,
            after,
            cut,
            entry_direction: *entry_direction,
            entry_math_forward: *entry_math_forward,
        })))
    }
}
