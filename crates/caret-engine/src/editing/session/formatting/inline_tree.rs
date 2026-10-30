//! 在行内节点树中计算格式范围、拆分节点并按语义嵌套格式环境。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::text_map::split_inline_nodes;
use super::super::{Affinity, EditError};
use caret_doc::{InlineEnvironment, InlineEnvironmentKind, InlineNode, NodeId};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub(super) struct FormatSpan {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) bold: bool,
    pub(super) italic: bool,
    /// `None` 表示软换行占据偏移位置，但它本身不是可设置下划线的文字。
    pub(super) underline: Option<bool>,
}

pub(in crate::editing::session) fn apply_formats_to_range(
    paragraph: &mut caret_doc::Paragraph,
    start: usize,
    end: usize,
    formats: &[(InlineEnvironmentKind, bool)],
    inline_depth: usize,
) -> Result<(), EditError> {
    for (kind, enabled) in formats {
        wrap_format_range(
            &mut paragraph.inlines,
            start,
            end,
            *kind,
            *enabled,
            inline_depth,
        )?;
    }
    Ok(())
}

pub(in crate::editing::session) fn apply_font_family_to_range(
    paragraph: &mut caret_doc::Paragraph,
    start: usize,
    end: usize,
    family: &str,
    inline_depth: usize,
) -> Result<(), EditError> {
    if start >= end {
        return Ok(());
    }
    wrap_font_family_range(&mut paragraph.inlines, start, end, family, inline_depth)
}

/// 移除指定行内环境外壳，保留其子节点并归一化新相邻的行内节点。
pub(super) fn unwrap_inline_environment(nodes: &mut Vec<InlineNode>, target: &NodeId) -> bool {
    for index in 0..nodes.len() {
        if matches!(
            nodes.get(index),
            Some(InlineNode::Environment(environment)) if &environment.id == target
        ) {
            let Some(InlineNode::Environment(environment)) = nodes.get(index).cloned() else {
                return false;
            };
            nodes.splice(index..=index, environment.children);
            normalize_inline_tree(nodes);
            return true;
        }
        if let Some(InlineNode::Environment(environment)) = nodes.get_mut(index)
            && unwrap_inline_environment(&mut environment.children, target)
        {
            normalize_inline_tree(&mut environment.children);
            return true;
        }
    }
    false
}

pub(super) fn format_spans(nodes: &[InlineNode]) -> Result<Vec<FormatSpan>, EditError> {
    fn collect(
        nodes: &[InlineNode],
        offset: &mut usize,
        bold: bool,
        italic: bool,
        output: &mut Vec<FormatSpan>,
    ) -> Result<(), EditError> {
        for node in nodes {
            match node {
                InlineNode::Run(run) => {
                    let start = *offset;
                    *offset += run.text.len();
                    if start < *offset {
                        output.push(FormatSpan {
                            start,
                            end: *offset,
                            bold,
                            italic,
                            underline: Some(
                                run.props
                                    .get("underline")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                            ),
                        });
                    }
                }
                InlineNode::SoftBreak(_) => {
                    output.push(FormatSpan {
                        start: *offset,
                        end: *offset + 1,
                        bold,
                        italic,
                        underline: None,
                    });
                    *offset += 1;
                }
                InlineNode::Environment(environment) => {
                    let mut nested_bold = bold;
                    let mut nested_italic = italic;
                    match environment.kind {
                        InlineEnvironmentKind::Bold => {
                            nested_bold = environment
                                .props
                                .get("enabled")
                                .and_then(Value::as_bool)
                                .unwrap_or(true);
                        }
                        InlineEnvironmentKind::Italic => {
                            nested_italic = environment
                                .props
                                .get("enabled")
                                .and_then(Value::as_bool)
                                .unwrap_or(true);
                        }
                        InlineEnvironmentKind::Font => {}
                    }
                    collect(
                        &environment.children,
                        offset,
                        nested_bold,
                        nested_italic,
                        output,
                    )?;
                }
                InlineNode::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
            }
        }
        Ok(())
    }

    let mut spans = Vec::new();
    let mut offset = 0;
    collect(nodes, &mut offset, false, false, &mut spans)?;
    Ok(spans)
}

pub(super) fn format_value(span: &FormatSpan, kind: InlineEnvironmentKind) -> bool {
    match kind {
        InlineEnvironmentKind::Bold => span.bold,
        InlineEnvironmentKind::Italic => span.italic,
        InlineEnvironmentKind::Font => false,
    }
}

pub(super) fn set_run_underline_to_range(
    nodes: &mut Vec<InlineNode>,
    start: usize,
    end: usize,
    enabled: bool,
) -> Result<(), EditError> {
    if start >= end {
        return Ok(());
    }
    let original = std::mem::take(nodes);
    let (prefix_and_selected, after) = split_inline_nodes(original, end)?;
    let (before, mut selected) = split_inline_nodes(prefix_and_selected, start)?;
    if selected.is_empty() {
        return Err(EditError::InvalidPosition);
    }
    fn set(nodes: &mut [InlineNode], enabled: bool) -> Result<(), EditError> {
        for node in nodes {
            match node {
                InlineNode::Run(run) => {
                    run.props
                        .insert("underline".to_owned(), Value::Bool(enabled));
                }
                InlineNode::Environment(environment) => set(&mut environment.children, enabled)?,
                InlineNode::SoftBreak(_) => {}
                InlineNode::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
            }
        }
        Ok(())
    }
    set(&mut selected, enabled)?;
    normalize_inline_tree(&mut selected);
    let before_len = before.len();
    let mut selected_end = before_len + selected.len();
    nodes.reserve(before_len + selected.len() + after.len());
    nodes.extend(before);
    nodes.extend(selected);
    nodes.extend(after);
    if before_len > 0 && merge_adjacent_inline_nodes(nodes, before_len - 1) {
        selected_end = selected_end.saturating_sub(1);
    }
    merge_adjacent_inline_nodes(nodes, selected_end.saturating_sub(1));
    Ok(())
}

pub(super) fn effective_format_at(
    nodes: &[InlineNode],
    offset: usize,
    affinity: Affinity,
    kind: InlineEnvironmentKind,
) -> Result<bool, EditError> {
    let spans = format_spans(nodes)?;
    Ok(spans
        .iter()
        .find(|span| {
            (span.start < offset && offset < span.end)
                || (offset == span.start && affinity == Affinity::Downstream)
                || (offset == span.end && affinity == Affinity::Upstream)
        })
        .is_some_and(|span| format_value(span, kind)))
}

pub(super) fn wrap_format_range(
    nodes: &mut Vec<InlineNode>,
    start: usize,
    end: usize,
    kind: InlineEnvironmentKind,
    enabled: bool,
    inline_depth: usize,
) -> Result<(), EditError> {
    if start >= end {
        return Ok(());
    }
    let original = std::mem::take(nodes);
    let (prefix_and_selected, after) = split_inline_nodes(original, end)?;
    let (before, selected) = split_inline_nodes(prefix_and_selected, start)?;
    if selected.is_empty() {
        return Err(EditError::InvalidPosition);
    }
    let mut selected = apply_format_at_deepest(selected, kind, enabled, None, inline_depth)?;
    normalize_inline_tree(&mut selected);
    let before_len = before.len();
    let mut selected_end = before.len() + selected.len();
    nodes.reserve(before.len() + after.len() + selected.len());
    nodes.extend(before);
    nodes.extend(selected);
    nodes.extend(after);
    if before_len > 0 {
        let left_seam = before_len - 1;
        if merge_adjacent_inline_nodes(nodes, left_seam) {
            selected_end = selected_end.saturating_sub(1);
        }
    }
    let right_seam = selected_end.saturating_sub(1);
    merge_adjacent_inline_nodes(nodes, right_seam);
    Ok(())
}

/// 合并相邻且语义完全兼容的行内环境，并压紧环境边界上的兼容文本 Run。
///
/// 节点 ID 不参与兼容性比较；合并时保留文档顺序靠前的节点 ID。
fn normalize_inline_tree(nodes: &mut Vec<InlineNode>) {
    let mut normalized = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        if let InlineNode::Environment(environment) = &mut node {
            normalize_inline_tree(&mut environment.children);
        }
        normalized.push(node);
        if normalized.len() > 1 {
            let index = normalized.len() - 2;
            while merge_adjacent_inline_nodes(&mut normalized, index) {}
        }
    }
    *nodes = normalized;
}

fn merge_adjacent_inline_nodes(nodes: &mut Vec<InlineNode>, index: usize) -> bool {
    let Some(next_index) = index.checked_add(1) else {
        return false;
    };
    let Some(next) = nodes.get(next_index) else {
        return false;
    };
    let can_merge = match (nodes.get(index), next) {
        (Some(InlineNode::Run(left)), InlineNode::Run(right)) => {
            left.language == right.language
                && left.props == right.props
                && left.extra.is_empty()
                && right.extra.is_empty()
        }
        (Some(InlineNode::Environment(left)), InlineNode::Environment(right)) => {
            left.kind == right.kind
                && left.definition_id == right.definition_id
                && left.props == right.props
                && left.extra.is_empty()
                && right.extra.is_empty()
        }
        _ => false,
    };
    if !can_merge {
        return false;
    }

    let right = nodes.remove(next_index);
    match (nodes.get_mut(index), right) {
        (Some(InlineNode::Run(left)), InlineNode::Run(right)) => {
            left.text.push_str(&right.text);
        }
        (Some(InlineNode::Environment(left)), InlineNode::Environment(mut right)) => {
            let seam = left.children.len();
            left.children.append(&mut right.children);
            if seam > 0 && seam < left.children.len() {
                merge_adjacent_inline_nodes(&mut left.children, seam - 1);
            }
        }
        _ => unreachable!("merge compatibility was validated before removing the right node"),
    }
    true
}

fn wrap_font_family_range(
    nodes: &mut Vec<InlineNode>,
    start: usize,
    end: usize,
    family: &str,
    inline_depth: usize,
) -> Result<(), EditError> {
    let original = std::mem::take(nodes);
    let (prefix_and_selected, after) = split_inline_nodes(original, end)?;
    let (before, selected) = split_inline_nodes(prefix_and_selected, start)?;
    if selected.is_empty() {
        return Err(EditError::InvalidPosition);
    }
    let mut selected = apply_font_family_at_deepest(selected, family, None, inline_depth)?;
    normalize_inline_tree(&mut selected);
    let before_len = before.len();
    let mut selected_end = before.len() + selected.len();
    nodes.reserve(before.len() + after.len() + selected.len());
    nodes.extend(before);
    nodes.extend(selected);
    nodes.extend(after);
    if before_len > 0 && merge_adjacent_inline_nodes(nodes, before_len - 1) {
        selected_end = selected_end.saturating_sub(1);
    }
    merge_adjacent_inline_nodes(nodes, selected_end.saturating_sub(1));
    Ok(())
}

fn apply_font_family_at_deepest(
    nodes: Vec<InlineNode>,
    family: &str,
    inherited_family: Option<&str>,
    depth: usize,
) -> Result<Vec<InlineNode>, EditError> {
    let mut output = Vec::with_capacity(nodes.len());
    let mut pending = Vec::new();
    let flush_pending = |pending: &mut Vec<InlineNode>, output: &mut Vec<InlineNode>| {
        if pending.is_empty() {
            return Ok(());
        }
        if depth >= caret_doc::MAX_DOCUMENT_NESTING_DEPTH {
            return Err(EditError::NestingLimit);
        }
        output.push(InlineNode::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Font,
            definition_id: None,
            props: BTreeMap::from([("fontFamily".to_owned(), Value::String(family.to_owned()))]),
            children: std::mem::take(pending),
            extra: BTreeMap::new(),
        }));
        Ok(())
    };

    for node in nodes {
        match node {
            InlineNode::Run(_) | InlineNode::SoftBreak(_) if inherited_family != Some(family) => {
                pending.push(node);
            }
            InlineNode::Run(_) | InlineNode::SoftBreak(_) => {
                flush_pending(&mut pending, &mut output)?;
                output.push(node);
            }
            InlineNode::Environment(mut environment) => {
                flush_pending(&mut pending, &mut output)?;
                let nested_family = if environment.kind == InlineEnvironmentKind::Font {
                    environment
                        .props
                        .get("fontFamily")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| inherited_family.map(str::to_owned))
                } else {
                    inherited_family.map(str::to_owned)
                };
                environment.children = apply_font_family_at_deepest(
                    environment.children,
                    family,
                    nested_family.as_deref(),
                    depth + 1,
                )?;
                output.push(InlineNode::Environment(environment));
            }
            InlineNode::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
        }
    }
    flush_pending(&mut pending, &mut output)?;
    Ok(output)
}

fn apply_format_at_deepest(
    nodes: Vec<InlineNode>,
    kind: InlineEnvironmentKind,
    enabled: bool,
    inherited_state: Option<bool>,
    depth: usize,
) -> Result<Vec<InlineNode>, EditError> {
    let mut output = Vec::with_capacity(nodes.len());
    let mut pending = Vec::new();
    let flush_pending = |pending: &mut Vec<InlineNode>, output: &mut Vec<InlineNode>| {
        if pending.is_empty() {
            return Ok(());
        }
        if depth >= caret_doc::MAX_DOCUMENT_NESTING_DEPTH {
            return Err(EditError::NestingLimit);
        }
        let mut props = BTreeMap::new();
        props.insert("enabled".to_owned(), Value::Bool(enabled));
        output.push(InlineNode::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind,
            definition_id: None,
            props,
            children: std::mem::take(pending),
            extra: BTreeMap::new(),
        }));
        Ok(())
    };

    for node in nodes {
        match node {
            InlineNode::Run(_) | InlineNode::SoftBreak(_) if inherited_state != Some(enabled) => {
                pending.push(node);
            }
            InlineNode::Run(_) | InlineNode::SoftBreak(_) => {
                flush_pending(&mut pending, &mut output)?;
                output.push(node);
            }
            InlineNode::Environment(mut environment) => {
                flush_pending(&mut pending, &mut output)?;
                let child_state = if environment.kind == kind {
                    environment
                        .props
                        .insert("enabled".to_owned(), Value::Bool(enabled));
                    Some(enabled)
                } else {
                    inherited_state
                };
                environment.children = apply_format_at_deepest(
                    environment.children,
                    kind,
                    enabled,
                    child_state,
                    depth + 1,
                )?;
                output.push(InlineNode::Environment(environment));
            }
            InlineNode::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
        }
    }
    flush_pending(&mut pending, &mut output)?;
    Ok(output)
}
