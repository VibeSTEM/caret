//! 将 Paragraph 展平为逻辑文本并映射 UTF-8 字素边界与节点。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{Affinity, EditError, Pos};
use caret_doc::{InlineEnvironment, InlineNode, NodeId, Paragraph, Run, SoftBreak};
use unicode_segmentation::UnicodeSegmentation;

/// Maps a Qt UTF-16 code-unit offset to a Rust UTF-8 byte offset.
pub fn utf16_offset_to_utf8(text: &str, offset: usize) -> Result<usize, EditError> {
    let mut units = 0usize;
    for (byte_offset, character) in text.char_indices() {
        if units == offset {
            return Ok(byte_offset);
        }
        let next = units + character.len_utf16();
        if offset < next {
            return Err(EditError::InvalidUtf16Offset);
        }
        units = next;
    }
    (units == offset)
        .then_some(text.len())
        .ok_or(EditError::InvalidUtf16Offset)
}

/// Maps a Rust UTF-8 byte offset to the equivalent Qt UTF-16 code-unit offset.
pub fn utf8_offset_to_utf16(text: &str, offset: usize) -> Result<usize, EditError> {
    let prefix = text.get(..offset).ok_or(EditError::InvalidPosition)?;
    Ok(prefix.encode_utf16().count())
}

#[derive(Debug, Clone)]
struct Span {
    start: usize,
    end: usize,
    path: Vec<usize>,
    is_soft_break: bool,
    is_math: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TextMap {
    pub(super) text: String,
    spans: Vec<Span>,
    pub(super) environment_spans: Vec<EnvironmentSpan>,
    pub(super) grapheme_boundaries: Vec<usize>,
}

#[derive(Debug, Clone)]
pub(super) struct EnvironmentSpan {
    pub(super) id: NodeId,
    pub(super) start: usize,
    pub(super) end: usize,
    depth: usize,
}

impl TextMap {
    pub(super) fn new(paragraph: &Paragraph) -> Result<Self, EditError> {
        let mut text = String::new();
        let mut spans = Vec::new();
        let mut environment_spans = Vec::new();
        collect_text(
            &paragraph.inlines,
            &mut Vec::new(),
            &mut text,
            &mut spans,
            &mut environment_spans,
            0,
        )?;
        let mut grapheme_boundaries: Vec<_> = text.grapheme_indices(true).map(|(i, _)| i).collect();
        grapheme_boundaries.push(text.len());
        grapheme_boundaries.dedup();
        Ok(Self {
            text,
            spans,
            environment_spans,
            grapheme_boundaries,
        })
    }

    pub(super) fn validate_pos(&self, pos: &Pos) -> Result<(), EditError> {
        if pos.offset > self.text.len()
            || !self.text.is_char_boundary(pos.offset)
            || self.grapheme_boundaries.binary_search(&pos.offset).is_err()
        {
            return Err(EditError::InvalidPosition);
        }
        Ok(())
    }

    pub(super) fn inline_environment_at(
        &self,
        offset: usize,
        affinity: Affinity,
    ) -> Option<(NodeId, usize, usize)> {
        self.environment_spans
            .iter()
            .filter(|span| {
                span.start < span.end
                    && ((span.start < offset && offset < span.end)
                        || (offset == span.start && affinity == Affinity::Downstream)
                        || (offset == span.end && affinity == Affinity::Upstream))
            })
            .max_by_key(|span| span.depth)
            .map(|span| (span.id.clone(), span.start, span.end))
    }

    pub(super) fn has_environment_start(&self, offset: usize) -> bool {
        self.environment_spans
            .iter()
            .any(|span| span.start == offset)
    }

    pub(super) fn has_environment_end(&self, offset: usize) -> bool {
        self.environment_spans.iter().any(|span| span.end == offset)
    }

    pub(super) fn has_environment_boundary(&self, offset: usize) -> bool {
        self.has_environment_start(offset) || self.has_environment_end(offset)
    }

    pub(super) fn snap_to_grapheme_boundary(&self, offset: usize, affinity: Affinity) -> usize {
        match self.grapheme_boundaries.binary_search(&offset) {
            Ok(_) => offset,
            Err(index) => match affinity {
                Affinity::Upstream => self
                    .grapheme_boundaries
                    .get(index.saturating_sub(1))
                    .copied()
                    .unwrap_or(0),
                Affinity::Downstream => self
                    .grapheme_boundaries
                    .get(index)
                    .copied()
                    .unwrap_or(self.text.len()),
            },
        }
    }

    pub(super) fn previous_boundary(&self, offset: usize) -> usize {
        let Ok(index) = self.grapheme_boundaries.binary_search(&offset) else {
            return offset;
        };
        index
            .checked_sub(1)
            .and_then(|previous| self.grapheme_boundaries.get(previous).copied())
            .unwrap_or(offset)
    }

    pub(super) fn next_boundary(&self, offset: usize) -> usize {
        let Ok(index) = self.grapheme_boundaries.binary_search(&offset) else {
            return offset;
        };
        self.grapheme_boundaries
            .get(index + 1)
            .copied()
            .unwrap_or(offset)
    }

    fn soft_break_at_boundary(&self, offset: usize, affinity: Affinity) -> Option<&Span> {
        let before = || {
            self.spans
                .iter()
                .rev()
                .find(|span| span.is_soft_break && span.end == offset)
        };
        let after = || {
            self.spans
                .iter()
                .find(|span| span.is_soft_break && span.start == offset)
        };
        match affinity {
            Affinity::Upstream => before(),
            Affinity::Downstream => after(),
        }
    }

    pub(super) fn insert_text(
        &self,
        paragraph: &mut Paragraph,
        offset: usize,
        affinity: Affinity,
        text: &str,
    ) -> Result<(), EditError> {
        if text.is_empty() {
            return Ok(());
        }
        let interior = self
            .spans
            .iter()
            .find(|span| !span.is_soft_break && span.start < offset && offset < span.end);
        let candidate =
            interior
                .or_else(|| match affinity {
                    Affinity::Upstream => self.spans.iter().rev().find(|span| {
                        !span.is_soft_break && span.end == offset && span.start < span.end
                    }),
                    Affinity::Downstream => self.spans.iter().find(|span| {
                        !span.is_soft_break && span.start == offset && span.start < span.end
                    }),
                })
                .or_else(|| match affinity {
                    Affinity::Upstream => self.spans.iter().rev().find(|span| {
                        !span.is_soft_break && span.start == offset && span.end == offset
                    }),
                    Affinity::Downstream => self.spans.iter().find(|span| {
                        !span.is_soft_break && span.start == offset && span.end == offset
                    }),
                });
        if let Some(span) = candidate {
            if span.is_math {
                let list = inline_list_at_path_mut(&mut paragraph.inlines, &span.path)
                    .ok_or(EditError::InvalidPosition)?;
                let index = *span.path.last().ok_or(EditError::InvalidPosition)?;
                let insertion_index = if offset <= span.start {
                    index
                } else {
                    index + 1
                };
                list.insert(
                    insertion_index,
                    InlineNode::Run(Run {
                        text: text.to_owned(),
                        language: None,
                        props: Default::default(),
                        extra: Default::default(),
                    }),
                );
                normalize_run_neighborhood(list, insertion_index);
                return Ok(());
            }
            let local = offset.saturating_sub(span.start);
            if mutate_inline_at_path(&mut paragraph.inlines, &span.path, &mut |node| {
                if let InlineNode::Run(run) = node {
                    run.text.insert_str(local, text);
                }
            }) {
                normalize_run_neighborhood(
                    inline_list_at_path_mut(&mut paragraph.inlines, &span.path)
                        .ok_or(EditError::InvalidPosition)?,
                    *span.path.last().ok_or(EditError::InvalidPosition)?,
                );
                return Ok(());
            }
        }
        let soft_break = self.soft_break_at_boundary(offset, affinity);
        if let Some(span) = soft_break {
            let list = inline_list_at_path_mut(&mut paragraph.inlines, &span.path)
                .ok_or(EditError::InvalidPosition)?;
            let index = *span.path.last().ok_or(EditError::InvalidPosition)?;
            let insertion_index = index + usize::from(span.end == offset);
            list.insert(
                insertion_index,
                InlineNode::Run(Run {
                    text: text.to_owned(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                }),
            );
            normalize_run_neighborhood(list, insertion_index);
            return Ok(());
        }
        let run = InlineNode::Run(Run {
            text: text.to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        });
        if offset == 0 && affinity == Affinity::Upstream {
            paragraph.inlines.insert(0, run);
            normalize_run_neighborhood(&mut paragraph.inlines, 0);
        } else {
            paragraph.inlines.push(run);
            let index = paragraph.inlines.len() - 1;
            normalize_run_neighborhood(&mut paragraph.inlines, index);
        }
        Ok(())
    }

    pub(super) fn insert_soft_break(
        &self,
        paragraph: &mut Paragraph,
        offset: usize,
        affinity: Affinity,
    ) -> Result<(), EditError> {
        let interior = self
            .spans
            .iter()
            .find(|span| !span.is_soft_break && span.start < offset && offset < span.end);
        let candidate =
            interior
                .or_else(|| match affinity {
                    Affinity::Upstream => self.spans.iter().rev().find(|span| {
                        !span.is_soft_break && span.end == offset && span.start < span.end
                    }),
                    Affinity::Downstream => self.spans.iter().find(|span| {
                        !span.is_soft_break && span.start == offset && span.start < span.end
                    }),
                })
                .or_else(|| match affinity {
                    Affinity::Upstream => self.spans.iter().rev().find(|span| {
                        !span.is_soft_break && span.start == offset && span.end == offset
                    }),
                    Affinity::Downstream => self.spans.iter().find(|span| {
                        !span.is_soft_break && span.start == offset && span.end == offset
                    }),
                });
        let Some(span) = candidate else {
            let soft_break = self.soft_break_at_boundary(offset, affinity);
            if let Some(span) = soft_break {
                let list = inline_list_at_path_mut(&mut paragraph.inlines, &span.path)
                    .ok_or(EditError::InvalidPosition)?;
                let index = *span.path.last().ok_or(EditError::InvalidPosition)?;
                let insertion_index = index + usize::from(span.end == offset);
                list.insert(insertion_index, InlineNode::SoftBreak(SoftBreak::default()));
            } else if offset == 0 && affinity == Affinity::Upstream {
                paragraph
                    .inlines
                    .insert(0, InlineNode::SoftBreak(SoftBreak::default()));
            } else {
                paragraph
                    .inlines
                    .push(InlineNode::SoftBreak(SoftBreak::default()));
            }
            return Ok(());
        };
        if span.is_math {
            let list = inline_list_at_path_mut(&mut paragraph.inlines, &span.path)
                .ok_or(EditError::InvalidPosition)?;
            let index = *span.path.last().ok_or(EditError::InvalidPosition)?;
            let insertion_index = if offset <= span.start {
                index
            } else {
                index + 1
            };
            list.insert(insertion_index, InlineNode::SoftBreak(SoftBreak::default()));
            return Ok(());
        }
        let local = offset.saturating_sub(span.start);
        let Some(list) = inline_list_at_path_mut(&mut paragraph.inlines, &span.path) else {
            return Err(EditError::InvalidPosition);
        };
        let index = *span.path.last().ok_or(EditError::InvalidPosition)?;
        let Some(InlineNode::Run(run)) = list.get(index) else {
            return Err(EditError::InvalidPosition);
        };
        if local > run.text.len() || !run.text.is_char_boundary(local) {
            return Err(EditError::InvalidPosition);
        }
        if run.text.is_empty() {
            let break_index = index + usize::from(affinity == Affinity::Upstream);
            list.insert(break_index, InlineNode::SoftBreak(SoftBreak::default()));
            return Ok(());
        }
        let left = Run {
            text: run.text[..local].to_owned(),
            language: run.language.clone(),
            props: run.props.clone(),
            extra: run.extra.clone(),
        };
        let right = Run {
            text: run.text[local..].to_owned(),
            language: run.language.clone(),
            props: run.props.clone(),
            extra: run.extra.clone(),
        };
        if local > 0 && local < run.text.len() && !run.extra.is_empty() {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let mut replacement = Vec::with_capacity(3);
        if local > 0 {
            replacement.push(InlineNode::Run(left));
        }
        replacement.push(InlineNode::SoftBreak(SoftBreak::default()));
        if local < run.text.len() {
            replacement.push(InlineNode::Run(right));
        }
        list.splice(index..=index, replacement);
        Ok(())
    }

    pub(super) fn delete_range(
        &self,
        paragraph: &mut Paragraph,
        start: usize,
        end: usize,
    ) -> Result<(), EditError> {
        if start >= end || end > self.text.len() {
            return Err(EditError::InvalidPosition);
        }
        let mut run_edits = Vec::new();
        let mut removals = Vec::new();
        for span in &self.spans {
            let overlap_start = start.max(span.start);
            let overlap_end = end.min(span.end);
            if overlap_start >= overlap_end {
                continue;
            }
            if span.is_math {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            if span.is_soft_break {
                let node = inline_at_path(&paragraph.inlines, &span.path)
                    .ok_or(EditError::InvalidPosition)?;
                match node {
                    InlineNode::SoftBreak(value) if value.extra.is_empty() => {
                        removals.push(span.path.clone());
                    }
                    InlineNode::SoftBreak(_) => return Err(EditError::UnsupportedOpaqueContent),
                    _ => return Err(EditError::InvalidPosition),
                }
                continue;
            }
            let local_start = overlap_start - span.start;
            let local_end = overlap_end - span.start;
            let Some(InlineNode::Run(run)) = inline_at_path(&paragraph.inlines, &span.path) else {
                return Err(EditError::InvalidPosition);
            };
            if run.text.get(local_start..local_end).is_none() {
                return Err(EditError::InvalidPosition);
            }
            run_edits.push((span.path.clone(), local_start, local_end));
        }
        for (path, local_start, local_end) in run_edits {
            let mut found = false;
            mutate_inline_at_path(&mut paragraph.inlines, &path, &mut |node| {
                if let InlineNode::Run(run) = node {
                    run.text.replace_range(local_start..local_end, "");
                    found = true;
                }
            });
            if !found {
                return Err(EditError::InvalidPosition);
            }
        }
        removals.sort_by(|left, right| right.cmp(left));
        for path in removals {
            remove_inline_at_path(&mut paragraph.inlines, &path)
                .ok_or(EditError::InvalidPosition)?;
        }
        Ok(())
    }
}

fn collect_text(
    nodes: &[InlineNode],
    path: &mut Vec<usize>,
    text: &mut String,
    spans: &mut Vec<Span>,
    environment_spans: &mut Vec<EnvironmentSpan>,
    environment_depth: usize,
) -> Result<(), EditError> {
    for (index, node) in nodes.iter().enumerate() {
        path.push(index);
        match node {
            InlineNode::Run(run) => {
                let start = text.len();
                text.push_str(&run.text);
                spans.push(Span {
                    start,
                    end: text.len(),
                    path: path.clone(),
                    is_soft_break: false,
                    is_math: false,
                });
            }
            InlineNode::Math(_) => {
                let start = text.len();
                text.push('\u{fffc}');
                spans.push(Span {
                    start,
                    end: text.len(),
                    path: path.clone(),
                    is_soft_break: false,
                    is_math: true,
                });
            }
            InlineNode::Environment(InlineEnvironment { id, children, .. }) => {
                let start = text.len();
                collect_text(
                    children,
                    path,
                    text,
                    spans,
                    environment_spans,
                    environment_depth + 1,
                )?;
                environment_spans.push(EnvironmentSpan {
                    id: id.clone(),
                    start,
                    end: text.len(),
                    depth: environment_depth,
                });
            }
            InlineNode::SoftBreak(_) => {
                let start = text.len();
                text.push('\n');
                spans.push(Span {
                    start,
                    end: text.len(),
                    path: path.clone(),
                    is_soft_break: true,
                    is_math: false,
                });
            }
            InlineNode::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
        }
        path.pop();
    }
    Ok(())
}

fn normalize_run_neighborhood(nodes: &mut Vec<InlineNode>, index: usize) {
    if nodes.is_empty() || !matches!(nodes.get(index), Some(InlineNode::Run(_))) {
        return;
    }
    let mut current = index;
    while current > 0 {
        let previous_is_empty_default = matches!(
            nodes.get(current - 1),
            Some(InlineNode::Run(run))
                if run.text.is_empty()
                    && run.language.is_none()
                    && run.props.is_empty()
                    && run.extra.is_empty()
        );
        if previous_is_empty_default {
            nodes.remove(current - 1);
            current -= 1;
            continue;
        }
        let can_merge = match (nodes.get(current - 1), nodes.get(current)) {
            (Some(InlineNode::Run(previous)), Some(InlineNode::Run(current))) => {
                previous.language == current.language
                    && previous.props == current.props
                    && previous.extra.is_empty()
                    && current.extra.is_empty()
            }
            _ => false,
        };
        if !can_merge {
            break;
        }
        let Some(InlineNode::Run(current_run)) = nodes.get(current) else {
            break;
        };
        let current_text = current_run.text.clone();
        if let Some(InlineNode::Run(previous_run)) = nodes.get_mut(current - 1) {
            previous_run.text.push_str(&current_text);
        }
        nodes.remove(current);
        current -= 1;
    }
    while current + 1 < nodes.len() {
        let next_is_empty_default = matches!(
            nodes.get(current + 1),
            Some(InlineNode::Run(run))
                if run.text.is_empty()
                    && run.language.is_none()
                    && run.props.is_empty()
                    && run.extra.is_empty()
        );
        if next_is_empty_default {
            nodes.remove(current + 1);
            continue;
        }
        let can_merge = match (nodes.get(current), nodes.get(current + 1)) {
            (Some(InlineNode::Run(current)), Some(InlineNode::Run(next))) => {
                current.language == next.language
                    && current.props == next.props
                    && current.extra.is_empty()
                    && next.extra.is_empty()
            }
            _ => false,
        };
        if !can_merge {
            break;
        }
        let Some(InlineNode::Run(next_run)) = nodes.get(current + 1) else {
            break;
        };
        let next_text = next_run.text.clone();
        if let Some(InlineNode::Run(current_run)) = nodes.get_mut(current) {
            current_run.text.push_str(&next_text);
        }
        nodes.remove(current + 1);
    }
}

pub(super) fn inline_length(node: &InlineNode) -> Result<usize, EditError> {
    match node {
        InlineNode::Run(run) => Ok(run.text.len()),
        InlineNode::Environment(environment) => {
            environment
                .children
                .iter()
                .try_fold(0usize, |length, child| {
                    length
                        .checked_add(inline_length(child)?)
                        .ok_or(EditError::InvalidPosition)
                })
        }
        InlineNode::SoftBreak(_) => Ok(1),
        InlineNode::Math(_) => Ok('\u{fffc}'.len_utf8()),
        InlineNode::Opaque(_) => Err(EditError::UnsupportedOpaqueContent),
    }
}

pub(super) fn split_inline_nodes(
    nodes: Vec<InlineNode>,
    offset: usize,
) -> Result<(Vec<InlineNode>, Vec<InlineNode>), EditError> {
    let total = nodes.iter().try_fold(0usize, |length, node| {
        length
            .checked_add(inline_length(node)?)
            .ok_or(EditError::InvalidPosition)
    })?;
    if offset > total {
        return Err(EditError::InvalidPosition);
    }
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut consumed = 0usize;
    for node in nodes {
        let length = inline_length(&node)?;
        let end = consumed + length;
        if end <= offset {
            left.push(node);
        } else if consumed >= offset {
            right.push(node);
        } else {
            let (left_node, right_node) = split_inline_node(node, offset - consumed)?;
            if let Some(left_node) = left_node {
                left.push(left_node);
            }
            if let Some(right_node) = right_node {
                right.push(right_node);
            }
        }
        consumed = end;
    }
    Ok((left, right))
}

fn split_inline_node(
    node: InlineNode,
    offset: usize,
) -> Result<(Option<InlineNode>, Option<InlineNode>), EditError> {
    let length = inline_length(&node)?;
    if offset > length {
        return Err(EditError::InvalidPosition);
    }
    if offset == 0 {
        return Ok((None, Some(node)));
    }
    if offset == length {
        return Ok((Some(node), None));
    }
    match node {
        InlineNode::Run(run) => {
            if !run.extra.is_empty() || !run.text.is_char_boundary(offset) {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            let left = Run {
                text: run.text[..offset].to_owned(),
                language: run.language.clone(),
                props: run.props.clone(),
                extra: Default::default(),
            };
            let right = Run {
                text: run.text[offset..].to_owned(),
                language: run.language,
                props: run.props,
                extra: Default::default(),
            };
            Ok((Some(InlineNode::Run(left)), Some(InlineNode::Run(right))))
        }
        InlineNode::Environment(mut environment) => {
            if !environment.extra.is_empty() {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            let (left_children, right_children) = split_inline_nodes(environment.children, offset)?;
            let right_environment = InlineEnvironment {
                id: NodeId::generate(),
                kind: environment.kind,
                definition_id: environment.definition_id.clone(),
                props: environment.props.clone(),
                children: right_children,
                extra: Default::default(),
            };
            environment.children = left_children;
            Ok((
                Some(InlineNode::Environment(environment)),
                Some(InlineNode::Environment(right_environment)),
            ))
        }
        InlineNode::Math(_) | InlineNode::SoftBreak(_) | InlineNode::Opaque(_) => {
            Err(EditError::InvalidPosition)
        }
    }
}

fn inline_at_path<'a>(nodes: &'a [InlineNode], path: &[usize]) -> Option<&'a InlineNode> {
    let (index, rest) = path.split_first()?;
    let node = nodes.get(*index)?;
    if rest.is_empty() {
        return Some(node);
    }
    match node {
        InlineNode::Environment(environment) => inline_at_path(&environment.children, rest),
        _ => None,
    }
}

fn inline_list_at_path_mut<'a>(
    nodes: &'a mut Vec<InlineNode>,
    path: &[usize],
) -> Option<&'a mut Vec<InlineNode>> {
    let (index, rest) = path.split_first()?;
    if rest.is_empty() {
        return Some(nodes);
    }
    match nodes.get_mut(*index)? {
        InlineNode::Environment(environment) => {
            inline_list_at_path_mut(&mut environment.children, rest)
        }
        _ => None,
    }
}

fn mutate_inline_at_path(
    nodes: &mut [InlineNode],
    path: &[usize],
    operation: &mut impl FnMut(&mut InlineNode),
) -> bool {
    let Some((index, rest)) = path.split_first() else {
        return false;
    };
    let Some(node) = nodes.get_mut(*index) else {
        return false;
    };
    if rest.is_empty() {
        operation(node);
        return true;
    }
    match node {
        InlineNode::Environment(environment) => {
            mutate_inline_at_path(&mut environment.children, rest, operation)
        }
        _ => false,
    }
}

fn remove_inline_at_path(nodes: &mut Vec<InlineNode>, path: &[usize]) -> Option<InlineNode> {
    let (index, rest) = path.split_first()?;
    if rest.is_empty() {
        return (*index < nodes.len()).then(|| nodes.remove(*index));
    }
    match nodes.get_mut(*index)? {
        InlineNode::Environment(environment) => {
            remove_inline_at_path(&mut environment.children, rest)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caret_doc::{
        InlineEnvironment, InlineEnvironmentKind, InlineMath, MATH_TREE_SCHEMA_VERSION, MathNode,
        MathNodeKind, MathTree,
    };

    fn math() -> InlineNode {
        InlineNode::Math(InlineMath {
            id: NodeId::generate(),
            math_tree: MathTree {
                schema_version: MATH_TREE_SCHEMA_VERSION,
                source: "x".to_owned(),
                root: MathNode {
                    id: NodeId::generate(),
                    kind: MathNodeKind::Symbol,
                    text: Some("x".to_owned()),
                    attributes: Default::default(),
                    source_range: None,
                    children: Vec::new(),
                    extra: Default::default(),
                },
                extra: Default::default(),
            },
            extra: Default::default(),
        })
    }

    #[test]
    fn math_object_uses_utf8_placeholder_length_when_splitting_text() -> Result<(), EditError> {
        let nodes = vec![
            InlineNode::Run(Run {
                text: "a".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }),
            math(),
            InlineNode::Run(Run {
                text: "b".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }),
        ];
        let (left, right) = split_inline_nodes(nodes, 4)?;
        assert_eq!(left.len(), 2);
        assert_eq!(right.len(), 1);
        assert!(matches!(left[1], InlineNode::Math(_)));
        assert!(matches!(&right[0], InlineNode::Run(run) if run.text == "b"));
        Ok(())
    }

    #[test]
    fn soft_break_can_be_inserted_on_both_math_object_edges() -> Result<(), EditError> {
        let mut paragraph = Paragraph::empty(NodeId::generate());
        paragraph.inlines = vec![
            InlineNode::Run(Run {
                text: "a".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }),
            math(),
            InlineNode::Run(Run {
                text: "b".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }),
        ];

        TextMap::new(&paragraph)?.insert_soft_break(&mut paragraph, 1, Affinity::Downstream)?;
        assert!(matches!(paragraph.inlines[1], InlineNode::SoftBreak(_)));
        let map = TextMap::new(&paragraph)?;
        map.insert_soft_break(&mut paragraph, 5, Affinity::Upstream)?;
        assert!(matches!(paragraph.inlines[3], InlineNode::SoftBreak(_)));
        Ok(())
    }

    #[test]
    fn soft_break_at_nested_math_edge_stays_inside_its_environment() -> Result<(), EditError> {
        let mut paragraph = Paragraph::empty(NodeId::generate());
        paragraph.inlines = vec![InlineNode::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: Default::default(),
            children: vec![math()],
            extra: Default::default(),
        })];
        TextMap::new(&paragraph)?.insert_soft_break(&mut paragraph, 0, Affinity::Downstream)?;
        let InlineNode::Environment(environment) = &paragraph.inlines[0] else {
            return Err(EditError::InvalidPosition);
        };
        assert!(matches!(environment.children[0], InlineNode::SoftBreak(_)));
        assert!(matches!(environment.children[1], InlineNode::Math(_)));
        Ok(())
    }
}
