//! 比较行内宿主边界并验证其段落映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::{
    Affinity, EditError, HostCaretKind, HostCaretLocalPosition, HostCaretPosition,
    MathCaretLocalPosition, MathEditTarget, Pos, Session, TextMap,
};
use crate::editing::paragraph_ops::compare_positions;
use crate::editing::position::compare_affinity;
use caret_doc::NodeId;
use caret_structure::{DocumentNodeKind, find_document_node};
use std::cmp::Ordering;

impl Session {
    pub(in crate::editing::session::document_position) fn compare_text_with_inline_host(
        &self,
        text: &Pos,
        host: &HostCaretPosition<HostCaretLocalPosition>,
    ) -> Result<Ordering, EditError> {
        self.validate_host_caret(host)?;
        match host {
            HostCaretPosition::Inside {
                host_id,
                kind,
                local: HostCaretLocalPosition::Paragraph(local),
            } => {
                if *kind == HostCaretKind::Inline
                    && text.paragraph_id == local.paragraph_id
                    && text.offset == local.offset
                {
                    let map = self.text_map_for_caret(local)?;
                    if map.is_empty_inline_environment(text.offset, host_id) {
                        return Err(EditError::InvalidPosition);
                    }
                }
                compare_positions(&self.document.body, &self.paragraph_index, text, local)
            }
            HostCaretPosition::Before {
                host_id,
                kind: HostCaretKind::Inline,
            }
            | HostCaretPosition::After {
                host_id,
                kind: HostCaretKind::Inline,
            } => {
                let (paragraph_id, span) = self.inline_host_span(host_id)?;
                self.compare_text_with_inline_span(
                    text,
                    &paragraph_id,
                    &span,
                    matches!(host, HostCaretPosition::Before { .. }),
                    false,
                )
            }
            HostCaretPosition::Inside {
                host_id,
                local: HostCaretLocalPosition::Math(math),
                ..
            } if matches!(math.target, MathEditTarget::Inline { .. }) => {
                let (paragraph_id, span) = self.inline_math_span(host_id)?;
                self.compare_text_with_inline_span(text, &paragraph_id, &span, false, true)
            }
            HostCaretPosition::Before {
                kind: HostCaretKind::Block,
                ..
            }
            | HostCaretPosition::After {
                kind: HostCaretKind::Block,
                ..
            } => self.compare_text_with_block_host_edge(host.host_id(), host, text),
            _ => Err(EditError::InvalidPosition),
        }
    }

    pub(super) fn compare_inline_math_carets(
        &self,
        left: &MathCaretLocalPosition,
        right: &MathCaretLocalPosition,
    ) -> Result<Ordering, EditError> {
        let left_id = match &left.target {
            MathEditTarget::Inline { math_id, .. } => math_id,
            MathEditTarget::Block { .. } => return Err(EditError::InvalidPosition),
        };
        let right_id = match &right.target {
            MathEditTarget::Inline { math_id, .. } => math_id,
            MathEditTarget::Block { .. } => return Err(EditError::InvalidPosition),
        };
        let (left_paragraph, left_span) = self.inline_math_span(left_id)?;
        let (right_paragraph, right_span) = self.inline_math_span(right_id)?;
        if left_paragraph == right_paragraph {
            if left_span.1 <= right_span.0 {
                return Ok(Ordering::Less);
            }
            if right_span.1 <= left_span.0 {
                return Ok(Ordering::Greater);
            }
            return Err(EditError::InvalidPosition);
        }
        let left_anchor = self.inline_span_anchor(&left_paragraph, left_span.0)?;
        let right_anchor = self.inline_span_anchor(&right_paragraph, right_span.0)?;
        compare_positions(
            &self.document.body,
            &self.paragraph_index,
            &left_anchor,
            &right_anchor,
        )
    }

    pub(super) fn compare_inline_math_to_paragraph(
        &self,
        math: &MathCaretLocalPosition,
        paragraph: &Pos,
    ) -> Result<Ordering, EditError> {
        let math_id = match &math.target {
            MathEditTarget::Inline { math_id, .. } => math_id,
            MathEditTarget::Block { .. } => return Err(EditError::InvalidPosition),
        };
        let (paragraph_id, span) = self.inline_math_span(math_id)?;
        self.compare_text_with_inline_span(paragraph, &paragraph_id, &span, false, true)
            .map(Ordering::reverse)
    }

    fn compare_text_with_inline_span(
        &self,
        text: &Pos,
        paragraph_id: &NodeId,
        span: &(usize, usize),
        is_before: bool,
        is_inside: bool,
    ) -> Result<Ordering, EditError> {
        let map = self.text_map_for_caret(text)?;
        map.validate_pos(text)?;
        if &text.paragraph_id == paragraph_id {
            if is_inside {
                return if text.offset <= span.0 {
                    Ok(Ordering::Less)
                } else if text.offset >= span.1 {
                    Ok(Ordering::Greater)
                } else {
                    Err(EditError::InvalidPosition)
                };
            }
            let (edge, affinity) = if is_before {
                (span.0, Affinity::Upstream)
            } else {
                (span.1, Affinity::Downstream)
            };
            if text.offset == edge && matching_inline_edges_at_offset(&map, edge, is_before) > 1 {
                return Err(EditError::InvalidPosition);
            }
            return Ok(text
                .offset
                .cmp(&edge)
                .then_with(|| compare_affinity(text.affinity, affinity)));
        }
        let edge_offset = if is_before || is_inside {
            span.0
        } else {
            span.1
        };
        let anchor = self.inline_span_anchor(paragraph_id, edge_offset)?;
        compare_positions(&self.document.body, &self.paragraph_index, text, &anchor)
    }

    fn inline_math_span(&self, math_id: &NodeId) -> Result<(NodeId, (usize, usize)), EditError> {
        let (kind, paragraph_id, span, _) = self.resolve_inline_host_span(math_id)?;
        if kind != DocumentNodeKind::InlineMath {
            return Err(EditError::InvalidPosition);
        }
        Ok((paragraph_id, span))
    }

    fn inline_host_span(&self, host_id: &NodeId) -> Result<(NodeId, (usize, usize)), EditError> {
        let (_, paragraph_id, span, _) = self.resolve_inline_host_span(host_id)?;
        Ok((paragraph_id, span))
    }

    fn resolve_inline_host_span(
        &self,
        host_id: &NodeId,
    ) -> Result<(DocumentNodeKind, NodeId, (usize, usize), TextMap), EditError> {
        let location =
            find_document_node(&self.document, host_id).ok_or(EditError::InvalidPosition)?;
        if !matches!(
            location.kind,
            DocumentNodeKind::InlineEnvironment | DocumentNodeKind::InlineMath
        ) {
            return Err(EditError::InvalidPosition);
        }
        let paragraph_id = location.paragraph_id.ok_or(EditError::InvalidPosition)?;
        let paragraph_location = self
            .paragraph_index
            .get(&paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        let position = Pos {
            story: paragraph_location.story.clone(),
            paragraph_id: paragraph_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        let map = self.text_map_for_caret(&position)?;
        let span = match location.kind {
            DocumentNodeKind::InlineMath => map
                .object_spans
                .iter()
                .find(|span| span.id.as_ref() == Some(host_id)),
            DocumentNodeKind::InlineEnvironment => map
                .environment_spans
                .iter()
                .find(|span| span.id.as_ref() == Some(host_id)),
            _ => None,
        }
        .ok_or(EditError::InvalidPosition)?;
        Ok((location.kind, paragraph_id, (span.start, span.end), map))
    }

    fn inline_span_anchor(&self, paragraph_id: &NodeId, offset: usize) -> Result<Pos, EditError> {
        let location = self
            .paragraph_index
            .get(paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        let position = Pos {
            story: location.story.clone(),
            paragraph_id: paragraph_id.clone(),
            offset,
            affinity: Affinity::Downstream,
        };
        self.text_map_for_caret(&position)?
            .validate_pos(&position)?;
        Ok(position)
    }

    pub(super) fn validate_inline_paragraph_host(
        &self,
        host_id: &NodeId,
        position: &Pos,
    ) -> Result<(), EditError> {
        let map = self.text_map_for_caret(position)?;
        map.validate_pos(position)?;
        if map
            .inline_environment_stack_at(position.offset, position.affinity)
            .iter()
            .any(|span| span.id.as_ref() == Some(host_id))
            || map.is_empty_inline_environment(position.offset, host_id)
        {
            return Ok(());
        }
        Err(EditError::InvalidPosition)
    }

    pub(super) fn validate_inline_host_edge(
        &self,
        host_id: &NodeId,
        position: &HostCaretPosition<HostCaretLocalPosition>,
    ) -> Result<(), EditError> {
        let (kind, _, span, map) = self.resolve_inline_host_span(host_id)?;
        let offset = match position {
            HostCaretPosition::Before { .. } => span.0,
            HostCaretPosition::After { .. } => span.1,
            HostCaretPosition::Inside { .. } => return Err(EditError::InvalidPosition),
        };
        if kind == DocumentNodeKind::InlineEnvironment
            && map.grapheme_boundaries.binary_search(&offset).is_err()
        {
            return Err(EditError::InvalidPosition);
        }
        Ok(())
    }
}

fn matching_inline_edges_at_offset(map: &TextMap, offset: usize, is_before: bool) -> usize {
    let matches_edge = |start: usize, end: usize| {
        if is_before {
            start == offset
        } else {
            end == offset
        }
    };
    map.environment_spans
        .iter()
        .filter(|span| matches_edge(span.start, span.end))
        .count()
        + map
            .object_spans
            .iter()
            .filter(|span| matches_edge(span.start, span.end))
            .count()
}
