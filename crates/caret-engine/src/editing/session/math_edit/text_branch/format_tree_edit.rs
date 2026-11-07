//! 将共享内容流格式转换适配到数学槽位、路径与端点。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::ops::Range;

use caret_doc::{InlineEnvironmentKind, MathContent, MathFlow, MathTree};

use super::super::super::EditError;
use super::super::path::{resolve_flow_path, resolve_flow_path_mut};
use super::super::{
    MathPositionAffinity, MathTextFlowPath, MathTextFlowPathStep, MathTextPosition,
};
use crate::editing::Affinity;
use crate::editing::flow_format::{
    InheritedInlineStyle, InlineFormatChange, InlineFormatContext, format_flow_range,
    text_flow_offset, text_position_at_offset,
};

#[cfg(test)]
pub(in crate::editing::session::math_edit) fn apply_math_text_environment_tree_range(
    tree: &mut MathTree,
    path: &MathTextFlowPath,
    range: Range<usize>,
    kind: InlineEnvironmentKind,
    enabled: bool,
) -> Result<MathTextFlowPath, EditError> {
    apply_single_flow_range(
        tree,
        path,
        range,
        InlineFormatChange::State(kind, enabled),
        InheritedInlineStyle::default(),
    )
}

pub(in crate::editing::session::math_edit) fn apply_math_text_environment_tree_range_with_style(
    tree: &mut MathTree,
    path: &MathTextFlowPath,
    range: Range<usize>,
    kind: InlineEnvironmentKind,
    enabled: bool,
    inherited: InheritedInlineStyle,
) -> Result<MathTextFlowPath, EditError> {
    apply_single_flow_range(
        tree,
        path,
        range,
        InlineFormatChange::State(kind, enabled),
        inherited,
    )
}

#[cfg(test)]
pub(in crate::editing::session::math_edit) fn apply_math_text_font_family_tree_range(
    tree: &mut MathTree,
    path: &MathTextFlowPath,
    range: Range<usize>,
    family: &str,
) -> Result<MathTextFlowPath, EditError> {
    apply_single_flow_range(
        tree,
        path,
        range,
        InlineFormatChange::FontFamily(family),
        InheritedInlineStyle::default(),
    )
}

pub(in crate::editing::session::math_edit) fn apply_math_text_font_family_tree_range_with_style(
    tree: &mut MathTree,
    path: &MathTextFlowPath,
    range: Range<usize>,
    family: &str,
    inherited: InheritedInlineStyle,
) -> Result<MathTextFlowPath, EditError> {
    apply_single_flow_range(
        tree,
        path,
        range,
        InlineFormatChange::FontFamily(family),
        inherited,
    )
}

fn apply_single_flow_range(
    tree: &mut MathTree,
    path: &MathTextFlowPath,
    range: Range<usize>,
    change: InlineFormatChange<'_>,
    inherited: InheritedInlineStyle,
) -> Result<MathTextFlowPath, EditError> {
    let anchor = MathTextPosition::new(path.clone(), range.start, MathPositionAffinity::Downstream);
    let focus = MathTextPosition::new(path.clone(), range.end, MathPositionAffinity::Upstream);
    let (_, focus) =
        apply_math_text_format_range_with_style(tree, &anchor, &focus, change, inherited)?;
    Ok(focus.path)
}

/// 同一 MathFlow 内的文本选择只转换一次，再按未变逻辑偏移重建两端物理路径。
pub(in crate::editing::session::math_edit) fn apply_math_text_format_range_with_style(
    tree: &mut MathTree,
    anchor: &MathTextPosition,
    focus: &MathTextPosition,
    change: InlineFormatChange<'_>,
    inherited: InheritedInlineStyle,
) -> Result<(MathTextPosition, MathTextPosition), EditError> {
    let (prefix, context) = scope_context(tree, &anchor.path, inherited.clone())?;
    let (focus_prefix, _) = scope_context(tree, &focus.path, inherited)?;
    if prefix != focus_prefix || anchor.path.align_cell() != focus.path.align_cell() {
        return Err(EditError::InvalidPosition);
    }
    let scope = resolve_flow_path(&tree.root, &prefix).ok_or(EditError::InvalidPosition)?;
    let anchor_offset = text_flow_offset(
        &scope.content,
        &local_path(&anchor.path, prefix.len())?,
        anchor.offset,
    )?;
    let focus_offset = text_flow_offset(
        &scope.content,
        &local_path(&focus.path, prefix.len())?,
        focus.offset,
    )?;
    let updated = format_flow_range(
        &scope.content,
        anchor_offset.min(focus_offset)..anchor_offset.max(focus_offset),
        change,
        context,
    )?;
    if updated == scope.content {
        return Ok((anchor.clone(), focus.clone()));
    }
    let rebuild = |old: &MathTextPosition, offset| {
        let affinity = if offset == anchor_offset.min(focus_offset) {
            Affinity::Downstream
        } else {
            Affinity::Upstream
        };
        let (local_path, offset) = text_position_at_offset(&updated, offset, affinity)?;
        let mut steps = prefix.clone();
        steps.extend(local_path.into_iter().enumerate().map(|(depth, index)| {
            if depth == 0 {
                MathTextFlowPathStep::Content(index)
            } else {
                MathTextFlowPathStep::EnvironmentChild(index)
            }
        }));
        Ok(MathTextPosition::new(
            old.path.with_structural_steps(steps),
            offset,
            old.affinity,
        ))
    };
    let next_anchor = rebuild(anchor, anchor_offset)?;
    let next_focus = rebuild(focus, focus_offset)?;
    resolve_flow_path_mut(&mut tree.root, &prefix)
        .ok_or(EditError::InvalidPosition)?
        .content = updated;
    Ok((next_anchor, next_focus))
}

fn local_path(path: &MathTextFlowPath, prefix_len: usize) -> Result<Vec<usize>, EditError> {
    path.steps()[prefix_len..]
        .iter()
        .enumerate()
        .map(|(depth, step)| match (depth, step) {
            (0, MathTextFlowPathStep::Content(index)) => Ok(*index),
            (_, MathTextFlowPathStep::EnvironmentChild(index)) if depth > 0 => Ok(*index),
            _ => Err(EditError::InvalidPosition),
        })
        .collect()
}

fn scope_context(
    tree: &MathTree,
    path: &MathTextFlowPath,
    mut inherited: InheritedInlineStyle,
) -> Result<(Vec<MathTextFlowPathStep>, InlineFormatContext), EditError> {
    enum Target<'a> {
        Flow(&'a MathFlow),
        Content(&'a MathContent),
    }
    let prefix_len = path
        .steps()
        .iter()
        .rposition(|step| matches!(step, MathTextFlowPathStep::Operand(_)))
        .map_or(0, |index| index + 1);
    let prefix = path.steps()[..prefix_len].to_vec();
    let mut depth = 0;
    let mut target = Target::Flow(&tree.root);
    for step in &prefix {
        target = match (target, step) {
            (Target::Flow(flow), MathTextFlowPathStep::Content(index)) => {
                depth += 1;
                Target::Content(flow.content.get(*index).ok_or(EditError::InvalidPosition)?)
            }
            (
                Target::Content(MathContent::Environment(environment)),
                MathTextFlowPathStep::EnvironmentChild(index),
            ) => {
                inherited = inherited.with_environment(environment);
                depth += 1;
                Target::Content(
                    environment
                        .children
                        .get(*index)
                        .ok_or(EditError::InvalidPosition)?,
                )
            }
            (Target::Content(MathContent::Math(node)), MathTextFlowPathStep::Operand(index)) => {
                Target::Flow(
                    node.children
                        .get(*index)
                        .ok_or(EditError::InvalidPosition)?,
                )
            }
            _ => return Err(EditError::InvalidPosition),
        };
    }
    if !matches!(target, Target::Flow(_)) {
        return Err(EditError::InvalidPosition);
    }
    Ok((
        prefix,
        InlineFormatContext {
            inherited,
            content_depth: depth + 1,
            max_depth: caret_doc::MAX_MATH_TREE_DEPTH,
        },
    ))
}
