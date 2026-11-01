//! 在行内节点树中计算格式范围、拆分节点并按语义嵌套格式环境。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::environment::inline_adapter::{
    RUN_PROPERTY_ENVIRONMENTS, RunPropertyEnvironment, run_property_for_kind,
};
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
    /// 由集中适配表识别的 Run 属性值。
    pub(super) properties: BTreeMap<&'static str, bool>,
    /// 公式是独立编辑对象；普通文字格式命令不能把它当作可格式化文本。
    pub(super) formatable: bool,
    /// 仅 Run 可承载 `Run.props`；软换行可套 InlineEnvironment，但不能改 Run 属性。
    pub(super) run_property_target: bool,
}

pub(in crate::editing::session) fn apply_formats_to_range(
    paragraph: &mut caret_doc::Paragraph,
    start: usize,
    end: usize,
    formats: &[(InlineEnvironmentKind, bool)],
    inline_depth: usize,
) -> Result<(), EditError> {
    let mut updated = paragraph.inlines.clone();
    for (kind, enabled) in formats {
        wrap_format_range(&mut updated, start, end, *kind, *enabled, inline_depth)?;
    }
    paragraph.inlines = updated;
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
    let mut updated = paragraph.inlines.clone();
    wrap_font_family_range(&mut updated, start, end, family, inline_depth)?;
    paragraph.inlines = updated;
    Ok(())
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
                            properties: RUN_PROPERTY_ENVIRONMENTS
                                .iter()
                                .filter_map(|descriptor| {
                                    run.props.get(descriptor.key).map(|value| {
                                        (descriptor.key, (descriptor.is_active)(Some(value)))
                                    })
                                })
                                .collect(),
                            formatable: true,
                            run_property_target: true,
                        });
                    }
                }
                InlineNode::SoftBreak(_) => {
                    output.push(FormatSpan {
                        start: *offset,
                        end: *offset + 1,
                        bold,
                        italic,
                        properties: BTreeMap::new(),
                        formatable: true,
                        run_property_target: false,
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
                InlineNode::Math(_) => {
                    let start = *offset;
                    *offset += '\u{fffc}'.len_utf8();
                    output.push(FormatSpan {
                        start,
                        end: *offset,
                        bold,
                        italic,
                        properties: BTreeMap::new(),
                        formatable: false,
                        run_property_target: false,
                    });
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
    if let Some(property) =
        run_property_for_kind(crate::editing::EnvironmentKind::from_inline(kind))
    {
        return span.properties.get(property.key).copied().unwrap_or(false);
    }
    match kind {
        InlineEnvironmentKind::Bold => span.bold,
        InlineEnvironmentKind::Italic => span.italic,
        InlineEnvironmentKind::Font => false,
    }
}

pub(super) fn set_run_property_to_range(
    nodes: &mut Vec<InlineNode>,
    start: usize,
    end: usize,
    property: RunPropertyEnvironment,
    enabled: bool,
) -> Result<(), EditError> {
    let mut updated = nodes.clone();
    edit_run_property_to_range(&mut updated, start, end, property, Some(enabled))?;
    *nodes = updated;
    Ok(())
}

pub(super) fn clear_run_property_to_range(
    nodes: &mut Vec<InlineNode>,
    start: usize,
    end: usize,
    property: RunPropertyEnvironment,
) -> Result<(), EditError> {
    let mut updated = nodes.clone();
    edit_run_property_to_range(&mut updated, start, end, property, None)?;
    *nodes = updated;
    Ok(())
}

fn edit_run_property_to_range(
    nodes: &mut Vec<InlineNode>,
    start: usize,
    end: usize,
    property: RunPropertyEnvironment,
    value: Option<bool>,
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
    fn set(
        nodes: &mut [InlineNode],
        property: RunPropertyEnvironment,
        value: Option<bool>,
    ) -> Result<(), EditError> {
        for node in nodes {
            match node {
                InlineNode::Run(run) => {
                    if let Some(enabled) = value {
                        let Some(value) =
                            property.value_for_state.map(|to_value| to_value(enabled))
                        else {
                            return Err(EditError::UnsupportedInlineFormat);
                        };
                        run.props.insert(property.key.to_owned(), value);
                    } else {
                        run.props.remove(property.key);
                    }
                }
                InlineNode::Environment(environment) => {
                    set(&mut environment.children, property, value)?
                }
                InlineNode::SoftBreak(_) | InlineNode::Math(_) => {}
                InlineNode::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
            }
        }
        Ok(())
    }
    set(&mut selected, property, value)?;
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
    let mut updated = nodes.clone();
    wrap_format_range_inner(&mut updated, start, end, kind, enabled, inline_depth)?;
    *nodes = updated;
    Ok(())
}

fn wrap_format_range_inner(
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
            InlineNode::Opaque(_) | InlineNode::Math(_) => {
                flush_pending(&mut pending, &mut output)?;
                output.push(node);
            }
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
            InlineNode::Opaque(_) | InlineNode::Math(_) => {
                flush_pending(&mut pending, &mut output)?;
                output.push(node);
            }
        }
    }
    flush_pending(&mut pending, &mut output)?;
    Ok(output)
}
