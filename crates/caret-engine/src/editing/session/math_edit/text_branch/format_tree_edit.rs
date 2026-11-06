//! 数学 TextFlow 行内格式的直接树事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::EditError;
use super::super::path::{content_slot_mut, resolve_content_path_mut};
use super::super::{MathTextFlowPath, text_run_split::split_text_flow_runs_for_format};
use caret_doc::{InlineEnvironment, InlineEnvironmentKind, MathContent, NodeId};
use serde_json::Value;
use std::collections::BTreeMap;
use std::ops::Range;

/// 按显示字节范围把 TextFlow 拆为普通文本与行内环境，完全不读写源码映射。
pub(in crate::editing::session::math_edit) fn apply_math_text_environment_tree_range(
    tree: &mut caret_doc::MathTree,
    path: &MathTextFlowPath,
    range: Range<usize>,
    kind: InlineEnvironmentKind,
    enabled: bool,
) -> Result<MathTextFlowPath, EditError> {
    let mut props = BTreeMap::new();
    if !enabled {
        props.insert("enabled".to_owned(), Value::Bool(false));
    }
    apply_math_text_inline_environment_tree_range(tree, path, range, kind, props)
}

pub(in crate::editing::session::math_edit) fn apply_math_text_font_family_tree_range(
    tree: &mut caret_doc::MathTree,
    path: &MathTextFlowPath,
    range: Range<usize>,
    family: &str,
) -> Result<MathTextFlowPath, EditError> {
    if family.trim().is_empty() {
        return Err(EditError::InvalidFontFamily);
    }
    apply_math_text_inline_environment_tree_range(
        tree,
        path,
        range,
        InlineEnvironmentKind::Font,
        BTreeMap::from([("fontFamily".to_owned(), Value::String(family.to_owned()))]),
    )
}

fn apply_math_text_inline_environment_tree_range(
    tree: &mut caret_doc::MathTree,
    path: &MathTextFlowPath,
    range: Range<usize>,
    kind: InlineEnvironmentKind,
    props: BTreeMap<String, Value>,
) -> Result<MathTextFlowPath, EditError> {
    let original = path
        .resolve_structurally(tree)
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let display = original
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    if range == (0..display.len())
        && let Some(super::super::MathTextFlowPathStep::EnvironmentChild(0)) = path.steps().last()
    {
        let parent_steps = &path.steps()[..path.steps().len() - 1];
        if let Some(MathContent::Environment(environment)) =
            resolve_content_path_mut(&mut tree.root, parent_steps)
            && environment.kind == kind
            && environment.children.len() == 1
            && matches!(environment.children.first(), Some(MathContent::Text(_)))
        {
            if kind == InlineEnvironmentKind::Font {
                let family = props
                    .get("fontFamily")
                    .ok_or(EditError::InvalidFontFamily)?;
                environment
                    .props
                    .insert("fontFamily".to_owned(), family.clone());
            } else if let Some(enabled) = props.get("enabled").and_then(Value::as_bool) {
                if enabled {
                    environment.props.remove("enabled");
                } else {
                    environment
                        .props
                        .insert("enabled".to_owned(), Value::Bool(false));
                }
            } else {
                environment.props.remove("enabled");
            }
            return Ok(path.clone());
        }
    }
    let (left_runs, selected_runs, right_runs) = split_text_flow_runs_for_format(&original, &range)
        .ok_or(EditError::UnsupportedOpaqueContent)?;
    let has_left = !left_runs.is_empty();
    let has_right = !right_runs.is_empty();
    let fragment_count = usize::from(has_left) + 1 + usize::from(has_right);
    if fragment_count > 1 && !original.extra.is_empty() {
        return Err(EditError::UnsupportedOpaqueContent);
    }

    let mut replacement = Vec::with_capacity(fragment_count);
    if has_left {
        replacement.push(MathContent::Text(caret_doc::TextFlow {
            runs: left_runs,
            ..Default::default()
        }));
    }
    let mut selected = caret_doc::TextFlow {
        runs: selected_runs,
        ..Default::default()
    };
    if fragment_count == 1 {
        selected.extra = original.extra;
    }
    replacement.push(MathContent::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind,
        definition_id: None,
        props,
        children: vec![MathContent::Text(selected)],
        extra: BTreeMap::new(),
    }));
    if has_right {
        replacement.push(MathContent::Text(caret_doc::TextFlow {
            runs: right_runs,
            ..Default::default()
        }));
    }

    let (contents, index) =
        content_slot_mut(&mut tree.root, path.steps()).ok_or(EditError::InvalidPosition)?;
    let environment_index = index + usize::from(has_left);
    let mut selected_path = path.steps()[..path.steps().len() - 1].to_vec();
    let environment_step = match *path.steps().last().ok_or(EditError::InvalidPosition)? {
        super::super::MathTextFlowPathStep::Content(_) => {
            super::super::MathTextFlowPathStep::Content(environment_index)
        }
        super::super::MathTextFlowPathStep::EnvironmentChild(_) => {
            super::super::MathTextFlowPathStep::EnvironmentChild(environment_index)
        }
        super::super::MathTextFlowPathStep::Operand(_) => {
            return Err(EditError::InvalidPosition);
        }
    };
    contents.splice(index..index + 1, replacement);
    selected_path.push(environment_step);
    selected_path.push(super::super::MathTextFlowPathStep::EnvironmentChild(0));
    Ok(path.with_structural_steps(selected_path))
}
