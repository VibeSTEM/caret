//! 将 Paragraph 展平为逻辑文本并映射 UTF-8 字素边界与节点。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::environment::inline_adapter::{
    INLINE_MATH_ADAPTER, RUN_PROPERTY_ENVIRONMENTS, for_inline_environment,
};
use super::{Affinity, EditError, EnvironmentKind, EnvironmentSource, Pos};
use caret_doc::{
    FlowContent, InlineEnvironment, InlineMath, NodeId, OpaqueNode, Paragraph, ParagraphContent,
    Run, SoftBreak, TextFlow,
};
use unicode_segmentation::UnicodeSegmentation;

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
                if flow.source_range.is_some() || !flow.extra.is_empty() {
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
                    source_range: None,
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
                source_range: None,
                source_map: Vec::new(),
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
                    source_range: None,
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

/// 行内原子对象在逻辑文本中的占位字符。
pub(crate) const OBJECT_REPLACEMENT: char = '\u{fffc}';

#[derive(Debug, Clone)]
struct Span {
    start: usize,
    end: usize,
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
        let mut text = String::new();
        let mut spans = Vec::new();
        let mut environment_spans = Vec::new();
        let mut object_spans = Vec::new();
        collect_paragraph_text(
            &paragraph.inlines,
            &mut Vec::new(),
            &mut text,
            &mut spans,
            &mut EnvironmentSpans {
                environments: &mut environment_spans,
                objects: &mut object_spans,
            },
            0,
        )?;
        let property_spans = RUN_PROPERTY_ENVIRONMENTS
            .iter()
            .map(|property| collect_paragraph_run_property_spans(&paragraph.inlines, *property))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        let mut grapheme_boundaries: Vec<_> = text.grapheme_indices(true).map(|(i, _)| i).collect();
        grapheme_boundaries.push(text.len());
        grapheme_boundaries.dedup();
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
        let mut adapter = InlineEditAdapter::new(&mut paragraph.inlines)?;
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
            let local = offset.saturating_sub(span.start);
            if mutate_inline_at_path(&mut adapter.nodes, &span.path, &mut |node| {
                if let EditNode::Run(run) = node {
                    run.text.insert_str(local, text);
                }
            }) {
                normalize_run_neighborhood(
                    inline_list_at_path_mut(&mut adapter.nodes, &span.path)
                        .ok_or(EditError::InvalidPosition)?,
                    *span.path.last().ok_or(EditError::InvalidPosition)?,
                );
                adapter.commit();
                return Ok(());
            }
        }
        let soft_break = self.soft_break_at_boundary(offset, affinity);
        if let Some(span) = soft_break {
            let list = inline_list_at_path_mut(&mut adapter.nodes, &span.path)
                .ok_or(EditError::InvalidPosition)?;
            let index = *span.path.last().ok_or(EditError::InvalidPosition)?;
            let insertion_index = index + usize::from(span.end == offset);
            list.insert(
                insertion_index,
                EditNode::Run(Run {
                    text: text.to_owned(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                }),
            );
            normalize_run_neighborhood(list, insertion_index);
            adapter.commit();
            return Ok(());
        }
        let run = EditNode::Run(Run {
            text: text.to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        });
        if offset == 0 && affinity == Affinity::Upstream {
            adapter.nodes.insert(0, run);
            normalize_run_neighborhood(&mut adapter.nodes, 0);
        } else {
            adapter.nodes.push(run);
            let index = adapter.nodes.len() - 1;
            normalize_run_neighborhood(&mut adapter.nodes, index);
        }
        adapter.commit();
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
        let mut adapter = InlineEditAdapter::new(&mut paragraph.inlines)?;
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
            if span.is_soft_break {
                let node =
                    inline_at_path(&adapter.nodes, &span.path).ok_or(EditError::InvalidPosition)?;
                match node {
                    EditNode::SoftBreak(value) if value.extra.is_empty() => {
                        removals.push(span.path.clone());
                    }
                    EditNode::SoftBreak(_) => return Err(EditError::UnsupportedOpaqueContent),
                    // 行内公式按 U+FFFC 原子删除。
                    EditNode::Math(_) => removals.push(span.path.clone()),
                    _ => return Err(EditError::InvalidPosition),
                }
                continue;
            }
            let local_start = overlap_start - span.start;
            let local_end = overlap_end - span.start;
            let Some(EditNode::Run(run)) = inline_at_path(&adapter.nodes, &span.path) else {
                return Err(EditError::InvalidPosition);
            };
            if run.text.get(local_start..local_end).is_none() {
                return Err(EditError::InvalidPosition);
            }
            run_edits.push((span.path.clone(), local_start, local_end));
        }
        for (path, local_start, local_end) in run_edits {
            let mut found = false;
            mutate_inline_at_path(&mut adapter.nodes, &path, &mut |node| {
                if let EditNode::Run(run) = node {
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
            remove_inline_at_path(&mut adapter.nodes, &path).ok_or(EditError::InvalidPosition)?;
        }
        adapter.commit();
        Ok(())
    }
}

/// Build logical spans directly from the persisted tree. This read path runs for every caret
/// lookup, so it must not clone the entire inline tree just to flatten TextFlow runs.
fn collect_paragraph_text(
    contents: &[ParagraphContent],
    path: &mut Vec<usize>,
    text: &mut String,
    spans: &mut Vec<Span>,
    environment_spans: &mut EnvironmentSpans<'_>,
    environment_depth: usize,
) -> Result<(), EditError> {
    let mut logical_index = 0;
    for content in contents {
        match content {
            FlowContent::Text(flow) => {
                for run in &flow.runs {
                    path.push(logical_index);
                    let start = text.len();
                    text.push_str(&run.text);
                    spans.push(Span {
                        start,
                        end: text.len(),
                        path: path.clone(),
                        is_soft_break: false,
                    });
                    path.pop();
                    logical_index += 1;
                }
            }
            FlowContent::Environment(environment) => {
                path.push(logical_index);
                let start = text.len();
                collect_paragraph_text(
                    &environment.children,
                    path,
                    text,
                    spans,
                    environment_spans,
                    environment_depth + 1,
                )?;
                let adapter = for_inline_environment(environment.kind);
                environment_spans.environments.push(EnvironmentSpan {
                    id: Some(environment.id.clone()),
                    source: adapter.source,
                    start,
                    end: text.len(),
                    kind: adapter.kind,
                    depth: environment_depth,
                });
                path.pop();
                logical_index += 1;
            }
            FlowContent::SoftBreak(_) => {
                path.push(logical_index);
                let start = text.len();
                text.push('\n');
                spans.push(Span {
                    start,
                    end: text.len(),
                    path: path.clone(),
                    is_soft_break: true,
                });
                path.pop();
                logical_index += 1;
            }
            FlowContent::Math(math) => {
                path.push(logical_index);
                let start = text.len();
                text.push(OBJECT_REPLACEMENT);
                environment_spans.objects.push(EnvironmentSpan {
                    id: Some(math.id.clone()),
                    source: INLINE_MATH_ADAPTER.source,
                    start,
                    end: text.len(),
                    kind: INLINE_MATH_ADAPTER.kind,
                    depth: environment_depth,
                });
                spans.push(Span {
                    start,
                    end: text.len(),
                    path: path.clone(),
                    is_soft_break: true,
                });
                path.pop();
                logical_index += 1;
            }
            FlowContent::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
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

/// 收集文本时同时累积格式环境与原子对象两类范围。
struct EnvironmentSpans<'a> {
    environments: &'a mut Vec<EnvironmentSpan>,
    objects: &'a mut Vec<EnvironmentSpan>,
}

fn normalize_run_neighborhood(nodes: &mut Vec<EditNode>, index: usize) {
    if nodes.is_empty() || !matches!(nodes.get(index), Some(EditNode::Run(_))) {
        return;
    }
    let mut current = index;
    while current > 0 {
        let previous_is_empty_default = matches!(
            nodes.get(current - 1),
            Some(EditNode::Run(run))
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
            (Some(EditNode::Run(previous)), Some(EditNode::Run(current))) => {
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
        let Some(EditNode::Run(current_run)) = nodes.get(current) else {
            break;
        };
        let current_text = current_run.text.clone();
        if let Some(EditNode::Run(previous_run)) = nodes.get_mut(current - 1) {
            previous_run.text.push_str(&current_text);
        }
        nodes.remove(current);
        current -= 1;
    }
    while current + 1 < nodes.len() {
        let next_is_empty_default = matches!(
            nodes.get(current + 1),
            Some(EditNode::Run(run))
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
            (Some(EditNode::Run(current)), Some(EditNode::Run(next))) => {
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
        let Some(EditNode::Run(next_run)) = nodes.get(current + 1) else {
            break;
        };
        let next_text = next_run.text.clone();
        if let Some(EditNode::Run(current_run)) = nodes.get_mut(current) {
            current_run.text.push_str(&next_text);
        }
        nodes.remove(current + 1);
    }
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
        if end <= offset {
            left.push(node);
        } else if consumed >= offset {
            right.push(node);
        } else {
            let (left_node, right_node) = split_edit_node(node, offset - consumed)?;
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
                source_range: None,
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

fn inline_at_path<'a>(nodes: &'a [EditNode], path: &[usize]) -> Option<&'a EditNode> {
    let (index, rest) = path.split_first()?;
    let node = nodes.get(*index)?;
    if rest.is_empty() {
        return Some(node);
    }
    match node {
        EditNode::Environment(environment) => inline_at_path(&environment.children, rest),
        _ => None,
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

fn mutate_inline_at_path(
    nodes: &mut [EditNode],
    path: &[usize],
    operation: &mut impl FnMut(&mut EditNode),
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
        EditNode::Environment(environment) => {
            mutate_inline_at_path(&mut environment.children, rest, operation)
        }
        _ => false,
    }
}

fn remove_inline_at_path(nodes: &mut Vec<EditNode>, path: &[usize]) -> Option<EditNode> {
    let (index, rest) = path.split_first()?;
    if rest.is_empty() {
        return (*index < nodes.len()).then(|| nodes.remove(*index));
    }
    match nodes.get_mut(*index)? {
        EditNode::Environment(environment) => {
            remove_inline_at_path(&mut environment.children, rest)
        }
        _ => None,
    }
}
