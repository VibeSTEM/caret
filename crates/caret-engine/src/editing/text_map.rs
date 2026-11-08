//! 将 Paragraph 展平为逻辑文本并映射 UTF-8 字素边界与节点。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::environment::inline_adapter::{
    INLINE_MATH_ADAPTER, RUN_PROPERTY_ENVIRONMENTS, for_inline_environment,
};
use super::flow_edit::{
    RangeSplit, classify_range_split, content_at_path, normalize_text_flow_neighborhood,
    replace_text_flow_range, siblings_at_path_mut, split_run_at, text_flow_at_path_mut,
};
pub(crate) use super::flow_map::OBJECT_REPLACEMENT;
use super::flow_map::{FlowPath, FlowPathOptions, FlowSpanKind, build_flow_map_with_spans};
use super::grapheme::GraphemeMap;
use super::{Affinity, EditError, EnvironmentKind, EnvironmentSource, Pos};
use caret_doc::{
    FlowContent, InlineEnvironment, InlineMath, NodeId, OpaqueNode, Paragraph, ParagraphContent,
    Run, SoftBreak, TextFlow,
};
use std::collections::BTreeMap;
use std::ops::Range;

/// 可编辑的临时文本流视图；文档本身始终存储 `ParagraphContent::Text(TextFlow)`。
#[derive(Debug, Clone)]
pub(super) enum EditNode {
    Run(Run),
    Environment(InlineEnvironment<EditNode>),
    SoftBreak(SoftBreak),
    Opaque(OpaqueNode),
    Math(InlineMath),
}

struct InlineEditAdapter<'a> {
    storage: &'a mut Vec<ParagraphContent>,
    nodes: Vec<EditNode>,
}

impl<'a> InlineEditAdapter<'a> {
    fn new(storage: &'a mut Vec<ParagraphContent>) -> Result<Self, EditError> {
        let nodes = to_edit_nodes(storage)?;
        Ok(Self { storage, nodes })
    }

    fn commit(self) {
        *self.storage = from_edit_nodes(self.nodes);
    }
}

pub(super) fn to_edit_nodes(contents: &[ParagraphContent]) -> Result<Vec<EditNode>, EditError> {
    let mut nodes = Vec::new();
    for content in contents {
        match content {
            FlowContent::Text(flow) => {
                if !flow.extra.is_empty() {
                    return Err(EditError::UnsupportedOpaqueContent);
                }
                nodes.extend(flow.runs.iter().cloned().map(EditNode::Run));
            }
            FlowContent::Math(math) => nodes.push(EditNode::Math(math.clone())),
            FlowContent::Environment(environment) => {
                nodes.push(EditNode::Environment(InlineEnvironment {
                    id: environment.id.clone(),
                    kind: environment.kind,
                    definition_id: environment.definition_id.clone(),
                    props: environment.props.clone(),
                    children: to_edit_nodes(&environment.children)?,
                    extra: environment.extra.clone(),
                }))
            }
            FlowContent::SoftBreak(value) => nodes.push(EditNode::SoftBreak(value.clone())),
            FlowContent::Opaque(value) => nodes.push(EditNode::Opaque(value.clone())),
        }
    }
    Ok(nodes)
}

pub(super) fn from_edit_nodes(nodes: Vec<EditNode>) -> Vec<ParagraphContent> {
    let mut contents = Vec::new();
    let mut runs = Vec::new();
    let flush = |runs: &mut Vec<Run>, contents: &mut Vec<ParagraphContent>| {
        if !runs.is_empty() {
            contents.push(FlowContent::Text(TextFlow {
                runs: std::mem::take(runs),
                extra: Default::default(),
            }));
        }
    };
    for node in nodes {
        match node {
            EditNode::Run(run) => runs.push(run),
            EditNode::Environment(environment) => {
                flush(&mut runs, &mut contents);
                contents.push(FlowContent::Environment(InlineEnvironment {
                    id: environment.id,
                    kind: environment.kind,
                    definition_id: environment.definition_id,
                    props: environment.props,
                    children: from_edit_nodes(environment.children),
                    extra: environment.extra,
                }));
            }
            EditNode::SoftBreak(value) => {
                flush(&mut runs, &mut contents);
                contents.push(FlowContent::SoftBreak(value));
            }
            EditNode::Opaque(value) => {
                flush(&mut runs, &mut contents);
                contents.push(FlowContent::Opaque(value));
            }
            EditNode::Math(value) => {
                flush(&mut runs, &mut contents);
                contents.push(FlowContent::Math(value));
            }
        }
    }
    flush(&mut runs, &mut contents);
    contents
}

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
    flow_path: FlowPath,
    flow_offset: usize,
    path: Vec<usize>,
    /// 原子节点（软换行或行内公式）：整体插入与删除，内部无插入点。
    is_soft_break: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TextMap {
    pub(super) text: String,
    spans: Vec<Span>,
    pub(super) environment_spans: Vec<EnvironmentSpan>,
    /// 原子对象（行内公式）作为行内环境的范围；不参与格式边界判断。
    pub(super) object_spans: Vec<EnvironmentSpan>,
    /// 由连续 Run 下划线属性投影出的逻辑范围。
    property_spans: Vec<EnvironmentSpan>,
    pub(super) grapheme_boundaries: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MergedInlineMathFocus {
    pub paragraph_id: NodeId,
    pub math_id: NodeId,
    pub root_offset: usize,
    pub delete_end: usize,
}

#[derive(Debug, Clone)]
pub(super) struct EnvironmentSpan {
    pub(super) id: Option<NodeId>,
    pub(super) source: EnvironmentSource,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) kind: EnvironmentKind,
    pub(super) depth: usize,
}

impl TextMap {
    pub(super) fn new(paragraph: &Paragraph) -> Result<Self, EditError> {
        let mut object_spans = Vec::new();
        let mut current_flow_path = Vec::new();
        let mut current_flow_offset = 0;
        let flow_map = build_flow_map_with_spans(
            &paragraph.inlines,
            |math| math.id.clone(),
            FlowPathOptions {
                physical: true,
                legacy_edit: true,
            },
            |span| {
                let flow_path = span.path.ok_or(EditError::InvalidPosition)?;
                let flow_offset = if matches!(&span.kind, FlowSpanKind::Run) {
                    if current_flow_path != flow_path.content_indices {
                        current_flow_path.clone_from(&flow_path.content_indices);
                        current_flow_offset = 0;
                    }
                    let flow_offset = current_flow_offset;
                    current_flow_offset += span.range.len();
                    flow_offset
                } else {
                    0
                };
                let is_soft_break = match span.kind {
                    FlowSpanKind::Run => false,
                    FlowSpanKind::SoftBreak => true,
                    FlowSpanKind::MathAtom(id) => {
                        object_spans.push(EnvironmentSpan {
                            id: Some(id),
                            source: INLINE_MATH_ADAPTER.source,
                            start: span.range.start,
                            end: span.range.end,
                            kind: INLINE_MATH_ADAPTER.kind,
                            depth: span.environment_depth,
                        });
                        true
                    }
                };
                Ok(Span {
                    start: span.range.start,
                    end: span.range.end,
                    flow_path,
                    flow_offset,
                    path: span.legacy_edit_path.ok_or(EditError::InvalidPosition)?,
                    is_soft_break,
                })
            },
        )?;
        let text = flow_map.text;
        let grapheme_boundaries = flow_map.grapheme_boundaries;
        let environment_spans = flow_map
            .environments
            .into_iter()
            .map(|environment| {
                let adapter = for_inline_environment(environment.kind);
                EnvironmentSpan {
                    id: Some(environment.id),
                    source: adapter.source,
                    start: environment.range.start,
                    end: environment.range.end,
                    kind: adapter.kind,
                    depth: environment.depth,
                }
            })
            .collect::<Vec<_>>();
        let spans = flow_map.spans;
        let property_spans = RUN_PROPERTY_ENVIRONMENTS
            .iter()
            .map(|property| collect_paragraph_run_property_spans(&paragraph.inlines, *property))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        Ok(Self {
            text,
            spans,
            environment_spans,
            object_spans,
            property_spans,
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
            .and_then(|span| span.id.clone().map(|id| (id, span.start, span.end)))
    }

    /// 返回焦点按亲和性所属的全部行内环境（含原子公式），由外到内排列。
    pub(super) fn inline_environment_stack_at(
        &self,
        offset: usize,
        affinity: Affinity,
    ) -> Vec<&EnvironmentSpan> {
        let mut stack = self
            .environment_spans
            .iter()
            .chain(self.object_spans.iter())
            .chain(self.property_spans.iter())
            .filter(|span| {
                span.start < span.end
                    && ((span.start < offset && offset < span.end)
                        || (offset == span.start && affinity == Affinity::Downstream)
                        || (offset == span.end && affinity == Affinity::Upstream))
            })
            .collect::<Vec<_>>();
        stack.sort_by_key(|span| span.depth);
        stack
    }

    pub(super) fn inline_math_span(&self, id: &NodeId) -> Option<&EnvironmentSpan> {
        self.object_spans
            .iter()
            .find(|span| span.id.as_ref() == Some(id))
    }

    pub(super) fn has_environment_start(&self, offset: usize) -> bool {
        self.environment_spans
            .iter()
            .chain(self.object_spans.iter())
            .chain(self.property_spans.iter())
            .any(|span| span.start == offset)
    }

    pub(super) fn has_environment_end(&self, offset: usize) -> bool {
        self.environment_spans
            .iter()
            .chain(self.object_spans.iter())
            .chain(self.property_spans.iter())
            .any(|span| span.end == offset)
    }

    pub(super) fn has_environment_boundary(&self, offset: usize) -> bool {
        self.has_environment_start(offset) || self.has_environment_end(offset)
    }

    pub(super) fn snap_to_grapheme_boundary(&self, offset: usize, affinity: Affinity) -> usize {
        GraphemeMap::snap_boundaries(&self.grapheme_boundaries, offset, affinity)
    }

    pub(super) fn previous_boundary(&self, offset: usize) -> usize {
        GraphemeMap::previous_boundary(&self.grapheme_boundaries, offset).unwrap_or(offset)
    }

    pub(super) fn next_boundary(&self, offset: usize) -> usize {
        GraphemeMap::next_boundary(&self.grapheme_boundaries, offset).unwrap_or(offset)
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
        validate_paragraph_text_flows(&paragraph.inlines)?;
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
        if let Some(span) = candidate
            && let Some(run_index) = span.flow_path.run_index
        {
            let local = offset.saturating_sub(span.start);
            let mut flow_path = span.flow_path.clone();
            flow_path.run_index = None;
            let text_flow = text_flow_at_path_mut(&mut paragraph.inlines, &flow_path)
                .ok_or(EditError::InvalidPosition)?;
            let local = text_flow.runs[..run_index]
                .iter()
                .try_fold(local, |offset, run| offset.checked_add(run.text.len()))
                .ok_or(EditError::InvalidPosition)?;
            replace_text_flow_range(text_flow, local..local, affinity, text)?;
            let (siblings, index) = siblings_at_path_mut(&mut paragraph.inlines, &flow_path)
                .ok_or(EditError::InvalidPosition)?;
            normalize_text_flow_neighborhood(siblings, index, run_index);
            return Ok(());
        }
        let soft_break = self.soft_break_at_boundary(offset, affinity);
        if let Some(span) = soft_break {
            let (list, index) = siblings_at_path_mut(&mut paragraph.inlines, &span.flow_path)
                .ok_or(EditError::InvalidPosition)?;
            let insertion_index = index + usize::from(span.end == offset);
            list.insert(
                insertion_index,
                FlowContent::Text(single_run_text_flow(text)),
            );
            normalize_text_flow_neighborhood(list, insertion_index, 0);
            return Ok(());
        }
        let content = FlowContent::Text(single_run_text_flow(text));
        if offset == 0 && affinity == Affinity::Upstream {
            paragraph.inlines.insert(0, content);
            normalize_text_flow_neighborhood(&mut paragraph.inlines, 0, 0);
        } else {
            paragraph.inlines.push(content);
            let index = paragraph.inlines.len() - 1;
            normalize_text_flow_neighborhood(&mut paragraph.inlines, index, 0);
        }
        Ok(())
    }

    pub(super) fn insert_soft_break(
        &self,
        paragraph: &mut Paragraph,
        offset: usize,
        affinity: Affinity,
    ) -> Result<(), EditError> {
        self.insert_atomic(
            paragraph,
            offset,
            affinity,
            FlowContent::SoftBreak(SoftBreak::default()),
        )
    }

    /// 在插入点放入一个原子行内节点（软换行或行内公式），必要时拆分所在文本段。
    pub(super) fn insert_atomic(
        &self,
        paragraph: &mut Paragraph,
        offset: usize,
        affinity: Affinity,
        node: ParagraphContent,
    ) -> Result<(), EditError> {
        let mut adapter = InlineEditAdapter::new(&mut paragraph.inlines)?;
        let node = to_edit_nodes(&[node])?
            .into_iter()
            .next()
            .ok_or(EditError::InvalidPosition)?;
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
                let list = inline_list_at_path_mut(&mut adapter.nodes, &span.path)
                    .ok_or(EditError::InvalidPosition)?;
                let index = *span.path.last().ok_or(EditError::InvalidPosition)?;
                let insertion_index = index + usize::from(span.end == offset);
                list.insert(insertion_index, node.clone());
            } else if offset == 0 && affinity == Affinity::Upstream {
                adapter.nodes.insert(0, node.clone());
            } else {
                adapter.nodes.push(node.clone());
            }
            adapter.commit();
            return Ok(());
        };
        let local = offset.saturating_sub(span.start);
        let Some(list) = inline_list_at_path_mut(&mut adapter.nodes, &span.path) else {
            return Err(EditError::InvalidPosition);
        };
        let index = *span.path.last().ok_or(EditError::InvalidPosition)?;
        let Some(EditNode::Run(run)) = list.get(index) else {
            return Err(EditError::InvalidPosition);
        };
        if local > run.text.len() || !run.text.is_char_boundary(local) {
            return Err(EditError::InvalidPosition);
        }
        if run.text.is_empty() {
            let break_index = index + usize::from(affinity == Affinity::Upstream);
            list.insert(break_index, node.clone());
            adapter.commit();
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
            replacement.push(EditNode::Run(left));
        }
        replacement.push(node.clone());
        if local < run.text.len() {
            replacement.push(EditNode::Run(right));
        }
        list.splice(index..=index, replacement);
        adapter.commit();
        Ok(())
    }

    pub(super) fn delete_range(
        &self,
        paragraph: &mut Paragraph,
        start: usize,
        end: usize,
    ) -> Result<(), EditError> {
        self.delete_range_inner(paragraph, start, end, false)
            .map(|_| ())
    }

    /// 删除完整的空文本岛时安全合并相邻公式，并返回应恢复到 MathTree 的接缝位置。
    pub(super) fn delete_range_and_merge_inline_math(
        &self,
        paragraph: &mut Paragraph,
        start: usize,
        end: usize,
    ) -> Result<Option<MergedInlineMathFocus>, EditError> {
        self.delete_range_inner(paragraph, start, end, true)
    }

    fn delete_range_inner(
        &self,
        paragraph: &mut Paragraph,
        start: usize,
        end: usize,
        merge_inline_math: bool,
    ) -> Result<Option<MergedInlineMathFocus>, EditError> {
        validate_paragraph_text_flows(&paragraph.inlines)?;
        if start >= end || end > self.text.len() {
            return Err(EditError::InvalidPosition);
        }
        let selected_text_paths = if merge_inline_math {
            fully_selected_text_flow_paths(&self.spans, start, end)
        } else {
            Vec::new()
        };
        let mut run_edits = BTreeMap::<Vec<usize>, Range<usize>>::new();
        let mut removals = Vec::<FlowPath>::new();
        for span in &self.spans {
            let overlap_start = start.max(span.start);
            let overlap_end = end.min(span.end);
            if overlap_start >= overlap_end {
                continue;
            }
            if span.is_soft_break {
                match content_at_path(&paragraph.inlines, &span.flow_path)
                    .ok_or(EditError::InvalidPosition)?
                {
                    FlowContent::SoftBreak(value) if value.extra.is_empty() => {
                        removals.push(span.flow_path.clone());
                    }
                    FlowContent::SoftBreak(_) => {
                        return Err(EditError::UnsupportedOpaqueContent);
                    }
                    // 行内公式按 U+FFFC 原子删除。
                    FlowContent::Math(_) => removals.push(span.flow_path.clone()),
                    _ => return Err(EditError::InvalidPosition),
                }
                continue;
            }
            let local_start = overlap_start - span.start;
            let local_end = overlap_end - span.start;
            let flow_path = FlowPath {
                content_indices: span.flow_path.content_indices.clone(),
                run_index: None,
            };
            let Some(FlowContent::Text(text_flow)) =
                content_at_path(&paragraph.inlines, &flow_path)
            else {
                return Err(EditError::InvalidPosition);
            };
            let run_index = span.flow_path.run_index.ok_or(EditError::InvalidPosition)?;
            let run = text_flow
                .runs
                .get(run_index)
                .ok_or(EditError::InvalidPosition)?;
            if run.text.get(local_start..local_end).is_none() {
                return Err(EditError::InvalidPosition);
            }
            let range = span.flow_offset + local_start..span.flow_offset + local_end;
            run_edits
                .entry(flow_path.content_indices)
                .and_modify(|existing| {
                    existing.start = existing.start.min(range.start);
                    existing.end = existing.end.max(range.end);
                })
                .or_insert(range);
        }
        for (content_indices, range) in run_edits {
            let flow_path = FlowPath {
                content_indices,
                run_index: None,
            };
            let text_flow = text_flow_at_path_mut(&mut paragraph.inlines, &flow_path)
                .ok_or(EditError::InvalidPosition)?;
            replace_text_flow_range(text_flow, range, Affinity::Downstream, "")?;
        }
        let removed_math_or_break = !removals.is_empty();
        removals.sort_by(|left, right| right.content_indices.cmp(&left.content_indices));
        for path in removals {
            let (siblings, index) = siblings_at_path_mut(&mut paragraph.inlines, &path)
                .ok_or(EditError::InvalidPosition)?;
            if index >= siblings.len() {
                return Err(EditError::InvalidPosition);
            }
            siblings.remove(index);
        }
        let merged_focus = if !removed_math_or_break {
            selected_text_paths
                .into_iter()
                .find_map(|path| merge_math_at_empty_text_flow(paragraph, &path, &self.spans))
        } else {
            None
        };
        Ok(merged_focus)
    }
}

fn fully_selected_text_flow_paths(spans: &[Span], start: usize, end: usize) -> Vec<Vec<usize>> {
    let mut ranges = BTreeMap::<Vec<usize>, Range<usize>>::new();
    for span in spans.iter().filter(|span| !span.is_soft_break) {
        ranges
            .entry(span.flow_path.content_indices.clone())
            .and_modify(|range| {
                range.start = range.start.min(span.start);
                range.end = range.end.max(span.end);
            })
            .or_insert(span.start..span.end);
    }
    ranges
        .into_iter()
        .filter_map(|(path, range)| {
            (range.start < range.end && start <= range.start && range.end <= end).then_some(path)
        })
        .collect()
}

fn merge_math_at_empty_text_flow(
    paragraph: &mut Paragraph,
    path: &[usize],
    spans: &[Span],
) -> Option<MergedInlineMathFocus> {
    let (siblings, index) = siblings_at_path_mut(
        &mut paragraph.inlines,
        &FlowPath {
            content_indices: path.to_vec(),
            run_index: None,
        },
    )?;
    let left_index = index.checked_sub(1)?;
    let (left, text, right) = (
        siblings.get(left_index)?,
        siblings.get(index)?,
        siblings.get(index + 1)?,
    );
    let (FlowContent::Math(left), FlowContent::Text(text), FlowContent::Math(right)) =
        (left, text, right)
    else {
        return None;
    };
    if !empty_unstyled_text_flow(text) || !mergeable_inline_math_pair(left, right) {
        return None;
    }
    let mut right_path = path.to_vec();
    let right_index = right_path.last_mut()?;
    *right_index = right_index.checked_add(1)?;
    let delete_end = spans
        .iter()
        .filter(|span| span.flow_path.content_indices == right_path)
        .map(|span| span.end)
        .max()?;
    let left_id = left.id.clone();
    let root_offset = left.math_tree.root.content.len();
    let right = siblings.remove(index + 1);
    siblings.remove(index);
    let FlowContent::Math(right) = right else {
        unreachable!("a mergeable math pair has a math right-hand sibling");
    };
    let FlowContent::Math(left) = &mut siblings[left_index] else {
        unreachable!("a mergeable math pair has a math left-hand sibling");
    };
    left.math_tree
        .root
        .content
        .extend(right.math_tree.root.content);
    Some(MergedInlineMathFocus {
        paragraph_id: paragraph.id.clone(),
        math_id: left_id,
        root_offset,
        delete_end,
    })
}

fn empty_unstyled_text_flow(text: &TextFlow) -> bool {
    text.extra.is_empty()
        && text.runs.iter().all(|run| {
            run.text.is_empty()
                && run.language.is_none()
                && run.props.is_empty()
                && run.extra.is_empty()
        })
}

fn mergeable_inline_math_pair(left: &InlineMath, right: &InlineMath) -> bool {
    left.extra.is_empty()
        && right.extra.is_empty()
        && left.math_tree.schema_version == right.math_tree.schema_version
        && left.math_tree.extra.is_empty()
        && right.math_tree.extra.is_empty()
        && left.math_tree.root.extra.is_empty()
        && right.math_tree.root.extra.is_empty()
}

fn single_run_text_flow(text: &str) -> TextFlow {
    TextFlow {
        runs: vec![Run {
            text: text.to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        }],
        extra: Default::default(),
    }
}

fn validate_paragraph_text_flows(contents: &[ParagraphContent]) -> Result<(), EditError> {
    for content in contents {
        match content {
            FlowContent::Text(text) if !text.extra.is_empty() => {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            FlowContent::Environment(environment) => {
                validate_paragraph_text_flows(&environment.children)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_paragraph_run_property_spans(
    contents: &[ParagraphContent],
    property: super::environment::inline_adapter::RunPropertyEnvironment,
) -> Result<Vec<EnvironmentSpan>, EditError> {
    fn visit(
        contents: &[ParagraphContent],
        offset: &mut usize,
        active_start: &mut Option<usize>,
        output: &mut Vec<EnvironmentSpan>,
        property: super::environment::inline_adapter::RunPropertyEnvironment,
    ) -> Result<(), EditError> {
        let flush = |active_start: &mut Option<usize>, offset: usize, output: &mut Vec<_>| {
            if let Some(start) = active_start.take()
                && start < offset
            {
                output.push(EnvironmentSpan {
                    id: None,
                    source: property.adapter.source,
                    start,
                    end: offset,
                    kind: property.adapter.kind,
                    depth: 0,
                });
            }
        };
        for content in contents {
            match content {
                FlowContent::Text(flow) => {
                    for run in &flow.runs {
                        if (property.is_active)(run.props.get(property.key)) {
                            active_start.get_or_insert(*offset);
                        } else {
                            flush(active_start, *offset, output);
                        }
                        *offset += run.text.len();
                    }
                }
                FlowContent::Environment(environment) => visit(
                    &environment.children,
                    offset,
                    active_start,
                    output,
                    property,
                )?,
                FlowContent::SoftBreak(_) => {
                    flush(active_start, *offset, output);
                    *offset += 1;
                }
                FlowContent::Math(_) => {
                    flush(active_start, *offset, output);
                    *offset += OBJECT_REPLACEMENT.len_utf8();
                }
                FlowContent::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
            }
        }
        Ok(())
    }

    let mut output = Vec::new();
    let mut offset = 0;
    let mut active_start = None;
    visit(
        contents,
        &mut offset,
        &mut active_start,
        &mut output,
        property,
    )?;
    let flush = |active_start: &mut Option<usize>, offset: usize, output: &mut Vec<_>| {
        if let Some(start) = active_start.take()
            && start < offset
        {
            output.push(EnvironmentSpan {
                id: None,
                source: property.adapter.source,
                start,
                end: offset,
                kind: property.adapter.kind,
                depth: 0,
            });
        }
    };
    flush(&mut active_start, offset, &mut output);
    Ok(output)
}

pub(super) fn inline_length(node: &ParagraphContent) -> Result<usize, EditError> {
    fn flow_length(contents: &[ParagraphContent]) -> Result<usize, EditError> {
        contents.iter().try_fold(0usize, |length, content| {
            let content_length = match content {
                FlowContent::Text(flow) => flow.runs.iter().try_fold(0usize, |length, run| {
                    length
                        .checked_add(run.text.len())
                        .ok_or(EditError::InvalidPosition)
                })?,
                FlowContent::Environment(environment) => flow_length(&environment.children)?,
                FlowContent::SoftBreak(_) => 1,
                FlowContent::Math(_) => OBJECT_REPLACEMENT.len_utf8(),
                FlowContent::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
            };
            length
                .checked_add(content_length)
                .ok_or(EditError::InvalidPosition)
        })
    }

    flow_length(std::slice::from_ref(node))
}

pub(super) fn split_inline_nodes(
    nodes: Vec<ParagraphContent>,
    offset: usize,
) -> Result<(Vec<ParagraphContent>, Vec<ParagraphContent>), EditError> {
    let edit = to_edit_nodes(&nodes)?;
    let (left, right) = split_edit_nodes(edit, offset)?;
    Ok((from_edit_nodes(left), from_edit_nodes(right)))
}

fn edit_inline_length(node: &EditNode) -> Result<usize, EditError> {
    match node {
        EditNode::Run(run) => Ok(run.text.len()),
        EditNode::Environment(environment) => {
            environment
                .children
                .iter()
                .try_fold(0usize, |length, child| {
                    length
                        .checked_add(edit_inline_length(child)?)
                        .ok_or(EditError::InvalidPosition)
                })
        }
        EditNode::SoftBreak(_) => Ok(1),
        EditNode::Math(_) => Ok(OBJECT_REPLACEMENT.len_utf8()),
        EditNode::Opaque(_) => Err(EditError::UnsupportedOpaqueContent),
    }
}

pub(super) fn split_edit_nodes(
    nodes: Vec<EditNode>,
    offset: usize,
) -> Result<(Vec<EditNode>, Vec<EditNode>), EditError> {
    let total = nodes.iter().try_fold(0usize, |length, node| {
        length
            .checked_add(edit_inline_length(node)?)
            .ok_or(EditError::InvalidPosition)
    })?;
    if offset > total {
        return Err(EditError::InvalidPosition);
    }
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut consumed = 0usize;
    for node in nodes {
        let length = edit_inline_length(&node)?;
        let end = consumed + length;
        match classify_range_split(consumed, end, offset) {
            RangeSplit::Left => left.push(node),
            RangeSplit::Right => right.push(node),
            RangeSplit::Inside(local_offset) => {
                let (left_node, right_node) = split_edit_node(node, local_offset)?;
                if let Some(left_node) = left_node {
                    left.push(left_node);
                }
                if let Some(right_node) = right_node {
                    right.push(right_node);
                }
            }
        }
        consumed = end;
    }
    Ok((left, right))
}

fn split_edit_node(
    node: EditNode,
    offset: usize,
) -> Result<(Option<EditNode>, Option<EditNode>), EditError> {
    let length = edit_inline_length(&node)?;
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
        EditNode::Run(run) => {
            let (left, right) = split_run_at(&run, offset).map_err(|error| match error {
                EditError::InvalidPosition => EditError::UnsupportedOpaqueContent,
                other => other,
            })?;
            Ok((Some(EditNode::Run(left)), Some(EditNode::Run(right))))
        }
        EditNode::Environment(mut environment) => {
            if !environment.extra.is_empty() {
                return Err(EditError::UnsupportedOpaqueContent);
            }
            let (left_children, right_children) = split_edit_nodes(environment.children, offset)?;
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
                Some(EditNode::Environment(environment)),
                Some(EditNode::Environment(right_environment)),
            ))
        }
        EditNode::SoftBreak(_) | EditNode::Opaque(_) | EditNode::Math(_) => {
            Err(EditError::InvalidPosition)
        }
    }
}

fn inline_list_at_path_mut<'a>(
    nodes: &'a mut Vec<EditNode>,
    path: &[usize],
) -> Option<&'a mut Vec<EditNode>> {
    let (index, rest) = path.split_first()?;
    if rest.is_empty() {
        return Some(nodes);
    }
    match nodes.get_mut(*index)? {
        EditNode::Environment(environment) => {
            inline_list_at_path_mut(&mut environment.children, rest)
        }
        _ => None,
    }
}
