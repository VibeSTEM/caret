//! 在行内节点树中计算格式范围、拆分节点并按语义嵌套格式环境。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::environment::inline_adapter::{
    RUN_PROPERTY_ENVIRONMENTS, RunPropertyEnvironment, run_property_for_kind,
};
use super::super::super::flow_edit::{merge_run_pair, runs_can_merge};
use super::super::super::flow_format::{
    InheritedInlineStyle, InlineFormatChange, InlineFormatContext, format_flow_range,
    format_flow_run_property, reject_text_flow_extensions_in_range,
};
use super::super::super::text_map::{EditNode, from_edit_nodes, to_edit_nodes};
use super::super::{Affinity, EditError};
use caret_doc::{InlineEnvironmentKind, NodeId, ParagraphContent};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub(super) struct FormatSpan {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) bold: bool,
    pub(super) italic: bool,
    /// 由集中适配表识别的 Run 属性值。
    pub(super) properties: BTreeMap<&'static str, bool>,
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
    let _editable = to_edit_nodes(&paragraph.inlines)?;
    let mut updated = paragraph.inlines.clone();
    for (kind, enabled) in formats {
        updated = format_flow_range(
            &updated,
            start..end,
            InlineFormatChange::State(*kind, *enabled),
            format_context(inline_depth),
        )?;
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
    let _editable = to_edit_nodes(&paragraph.inlines)?;
    paragraph.inlines = format_flow_range(
        &paragraph.inlines,
        start..end,
        InlineFormatChange::FontFamily(family),
        format_context(inline_depth),
    )?;
    Ok(())
}

fn format_context(inline_depth: usize) -> InlineFormatContext {
    InlineFormatContext {
        inherited: InheritedInlineStyle::default(),
        content_depth: inline_depth,
        max_depth: caret_doc::MAX_DOCUMENT_NESTING_DEPTH,
    }
}

/// 移除指定行内环境外壳，保留其子节点并归一化新相邻的行内节点。
pub(super) fn unwrap_inline_environment(
    nodes: &mut Vec<ParagraphContent>,
    target: &NodeId,
) -> bool {
    let Ok(mut editable) = to_edit_nodes(nodes) else {
        return false;
    };
    if !unwrap_editable_environment(&mut editable, target) {
        return false;
    }
    *nodes = from_edit_nodes(editable);
    true
}

fn unwrap_editable_environment(nodes: &mut Vec<EditNode>, target: &NodeId) -> bool {
    for index in 0..nodes.len() {
        if matches!(
            nodes.get(index),
            Some(EditNode::Environment(environment)) if &environment.id == target
        ) {
            let Some(EditNode::Environment(environment)) = nodes.get(index).cloned() else {
                return false;
            };
            nodes.splice(index..=index, environment.children);
            normalize_inline_tree(nodes);
            return true;
        }
        if let Some(EditNode::Environment(environment)) = nodes.get_mut(index)
            && unwrap_editable_environment(&mut environment.children, target)
        {
            normalize_inline_tree(&mut environment.children);
            return true;
        }
    }
    false
}

pub(super) fn format_spans(nodes: &[ParagraphContent]) -> Result<Vec<FormatSpan>, EditError> {
    // 保留正文适配器拒绝未知 TextFlow 扩展的既有兼容边界。
    let _editable = to_edit_nodes(nodes)?;
    fn collect(
        nodes: &[ParagraphContent],
        offset: &mut usize,
        inherited: InheritedInlineStyle,
        output: &mut Vec<FormatSpan>,
    ) -> Result<(), EditError> {
        for node in nodes {
            match node {
                ParagraphContent::Text(text) => {
                    for run in &text.runs {
                        let start = *offset;
                        *offset += run.text.len();
                        if start < *offset {
                            output.push(FormatSpan {
                                start,
                                end: *offset,
                                bold: inherited.bold.unwrap_or(false),
                                italic: inherited.italic.unwrap_or(false),
                                properties: RUN_PROPERTY_ENVIRONMENTS
                                    .iter()
                                    .filter_map(|descriptor| {
                                        run.props.get(descriptor.key).map(|value| {
                                            (descriptor.key, (descriptor.is_active)(Some(value)))
                                        })
                                    })
                                    .collect(),
                                run_property_target: true,
                            });
                        }
                    }
                }
                ParagraphContent::SoftBreak(_) => {
                    output.push(FormatSpan {
                        start: *offset,
                        end: *offset + 1,
                        bold: inherited.bold.unwrap_or(false),
                        italic: inherited.italic.unwrap_or(false),
                        properties: BTreeMap::new(),
                        run_property_target: false,
                    });
                    *offset += 1;
                }
                ParagraphContent::Environment(environment) => {
                    collect(
                        &environment.children,
                        offset,
                        inherited.with_environment(environment),
                        output,
                    )?;
                }
                ParagraphContent::Math(_) => {
                    let start = *offset;
                    *offset += '\u{fffc}'.len_utf8();
                    output.push(FormatSpan {
                        start,
                        end: *offset,
                        bold: inherited.bold.unwrap_or(false),
                        italic: inherited.italic.unwrap_or(false),
                        properties: BTreeMap::new(),
                        run_property_target: false,
                    });
                }
                ParagraphContent::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
            }
        }
        Ok(())
    }
    let mut spans = Vec::new();
    collect(nodes, &mut 0, InheritedInlineStyle::default(), &mut spans)?;
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
    nodes: &mut Vec<ParagraphContent>,
    start: usize,
    end: usize,
    property: RunPropertyEnvironment,
    enabled: bool,
) -> Result<(), EditError> {
    reject_text_flow_extensions_in_range(nodes, start..end)?;
    *nodes = format_flow_run_property(nodes, start..end, property, Some(enabled))?;
    Ok(())
}

pub(super) fn clear_run_property_to_range(
    nodes: &mut Vec<ParagraphContent>,
    start: usize,
    end: usize,
    property: RunPropertyEnvironment,
) -> Result<(), EditError> {
    reject_text_flow_extensions_in_range(nodes, start..end)?;
    *nodes = format_flow_run_property(nodes, start..end, property, None)?;
    Ok(())
}

pub(super) fn effective_format_at(
    nodes: &[ParagraphContent],
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
    nodes: &mut Vec<ParagraphContent>,
    start: usize,
    end: usize,
    kind: InlineEnvironmentKind,
    enabled: bool,
    inline_depth: usize,
) -> Result<(), EditError> {
    *nodes = format_flow_range(
        nodes,
        start..end,
        InlineFormatChange::State(kind, enabled),
        format_context(inline_depth),
    )?;
    Ok(())
}

/// 合并相邻且语义完全兼容的行内环境，并压紧环境边界上的兼容文本 Run。
///
/// 节点 ID 不参与兼容性比较；合并时保留文档顺序靠前的节点 ID。
fn normalize_inline_tree(nodes: &mut Vec<EditNode>) {
    let mut normalized = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        if let EditNode::Environment(environment) = &mut node {
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

fn merge_adjacent_inline_nodes(nodes: &mut Vec<EditNode>, index: usize) -> bool {
    let Some(next_index) = index.checked_add(1) else {
        return false;
    };
    let Some(next) = nodes.get(next_index) else {
        return false;
    };
    let can_merge = match (nodes.get(index), next) {
        (Some(EditNode::Run(left)), EditNode::Run(right)) => runs_can_merge(left, right),
        (Some(EditNode::Environment(left)), EditNode::Environment(right)) => {
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
        (Some(EditNode::Run(left)), EditNode::Run(right)) => {
            let _ = merge_run_pair(left, &right);
        }
        (Some(EditNode::Environment(left)), EditNode::Environment(mut right)) => {
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
