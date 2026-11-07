//! 将 Math TextFlow 选区提升为同级数学节点的直接树事务。

// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::EditError;
use super::super::path::content_slot_mut;
use super::super::{MathTextFlowPath, text_run_split::split_text_flow_for_replacement};
use crate::editing::flow_edit::text_flow_boundary;
use caret_doc::{MathContent, MathNode, Run};
use std::ops::Range;

/// 以解析后的插入内容替换文本岛选区，并保留两侧可继续编辑的 TextFlow。
///
/// 源码位置仅属于活动 projection sidecar；树事务只改显示文本和结构节点。
pub(in crate::editing::session::math_edit) fn apply_math_text_split_tree_edit(
    tree: &mut caret_doc::MathTree,
    path: &MathTextFlowPath,
    display_range: Range<usize>,
    replacement: &str,
    inserted_contents: &[MathContent],
) -> Result<(), EditError> {
    let original = path
        .resolve(tree)
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let display_len = original
        .runs
        .iter()
        .map(|run| run.text.len())
        .sum::<usize>();
    if display_range.start > display_range.end
        || display_range.end > display_len
        || !text_flow_boundary(&original.runs, display_range.start)
        || !text_flow_boundary(&original.runs, display_range.end)
    {
        return Err(EditError::InvalidPosition);
    }

    let split = split_text_flow_for_replacement(&original, &display_range)
        .ok_or(EditError::UnsupportedOpaqueContent)?;
    let fragment_count = usize::from(split.has_left) + usize::from(split.has_right);

    let mut contents = Vec::with_capacity(
        fragment_count
            + inserted_contents.len()
            + usize::from(inserted_contents.is_empty() && replacement == "$$"),
    );
    if split.has_left {
        contents.push(MathContent::Text(text_flow_fragment(
            &original,
            split.left_runs,
        )?));
    }
    contents.extend(inserted_contents.iter().cloned());
    if inserted_contents.is_empty() && replacement == "$$" {
        contents.push(MathContent::Math(MathNode {
            id: caret_doc::NodeId::generate(),
            kind: caret_doc::MathNodeKind::Placeholder,
            text: None,
            attributes: Default::default(),
            children: Vec::new(),
            extra: Default::default(),
        }));
    }
    if split.has_right {
        contents.push(MathContent::Text(text_flow_fragment(
            &original,
            split.right_runs,
        )?));
    }

    let (parent, index) =
        content_slot_mut(&mut tree.root, path.steps()).ok_or(EditError::InvalidPosition)?;
    parent.splice(index..index + 1, contents);
    Ok(())
}

fn text_flow_fragment(
    original: &caret_doc::TextFlow,
    mut runs: Vec<Run>,
) -> Result<caret_doc::TextFlow, EditError> {
    if runs.is_empty() {
        runs.push(Run {
            text: String::new(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        });
    }
    let mut fragment = original.clone();
    fragment.runs = runs;
    Ok(fragment)
}
