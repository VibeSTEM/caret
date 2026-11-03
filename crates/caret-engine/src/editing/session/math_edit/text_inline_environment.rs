//! 将数学 TextFlow 格式命令映射到 LaTeX 源码与 MathTree 事务。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::history::{DocumentPatch, MathEditPosition, SessionHistoryEdit};
use super::super::{EditError, Effect};
use super::text_branch::{MathTextEdit, MathTextFormat, text_source_range};
use super::{
    MathContent, MathEdit, MathEditBranch, MathTextFlowPath, MathTextFlowPathStep, Session,
};
use crate::editing::session::history::SessionHistoryState;
use caret_doc::{InlineEnvironmentKind, MathTree};

impl Session {
    pub(super) fn toggle_math_text_inline_environment(
        &mut self,
        kind: InlineEnvironmentKind,
    ) -> Result<Effect, EditError> {
        if !matches!(
            kind,
            InlineEnvironmentKind::Bold | InlineEnvironmentKind::Italic
        ) {
            return Err(EditError::UnsupportedInlineFormat);
        }
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }

        let edit = self.math_edit.clone().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Text { path, anchor } = edit.branch.clone() else {
            return Err(EditError::UnsupportedInlineFormat);
        };
        let tree = self.math_tree_for_edit(&edit, path.align_cell())?;
        let text = path.resolve(tree).ok_or(EditError::InvalidPosition)?;
        let display = text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        let range = anchor.min(edit.cursor)..anchor.max(edit.cursor);
        if range.end > display.len()
            || !display.is_char_boundary(range.start)
            || !display.is_char_boundary(range.end)
            || !super::is_grapheme_boundary(&display, range.start)
            || !super::is_grapheme_boundary(&display, range.end)
        {
            return Err(EditError::InvalidPosition);
        }

        let inherited =
            inherited_environment_value(tree, &path, kind).ok_or(EditError::InvalidPosition)?;
        let current = self.pending_inline_style.get(kind).unwrap_or(inherited);
        let enabled = !current;
        if range.is_empty() {
            return self.set_pending_math_environment(&edit, &path, kind, enabled);
        }

        let local_source_range =
            text_source_range(text, range.clone()).ok_or(EditError::InvalidPosition)?;
        let host_source_range = path
            .host_offset(local_source_range.start)
            .and_then(|start| {
                path.host_offset(local_source_range.end)
                    .map(|end| start..end)
            })
            .ok_or(EditError::InvalidPosition)?;
        let prefix = environment_command(kind, enabled)?;
        let selected_source = edit
            .source()
            .get(host_source_range.clone())
            .ok_or(EditError::InvalidPosition)?;
        let replacement = format!("{prefix}{selected_source}}}");
        let mut source = edit.source().to_owned();
        source.replace_range(host_source_range.clone(), &replacement);
        let format = MathTextFormat {
            kind,
            enabled,
            selected_source_start: local_source_range.start + prefix.len(),
            selected_source_len: local_source_range.len(),
            selected_text: display
                .get(range.clone())
                .ok_or(EditError::InvalidPosition)?
                .to_owned(),
            display_range: range.clone(),
            selection_reversed: anchor > edit.cursor,
        };
        self.write_math_source_with_metadata(
            source,
            host_source_range.start + prefix.len() + selected_source.len(),
            None,
            host_source_range,
            Some((path.clone(), range.end)),
            Some(MathTextEdit {
                path,
                range,
                inserted: String::new(),
                source_edit: local_source_range,
                source_replacement_len: replacement.len(),
                snap_forward: false,
                removes_entire_flow: false,
                split_math_entry: false,
                format: Some(format),
            }),
        )
    }

    fn set_pending_math_environment(
        &mut self,
        edit: &MathEdit,
        path: &MathTextFlowPath,
        kind: InlineEnvironmentKind,
        enabled: bool,
    ) -> Result<Effect, EditError> {
        if self.pending_inline_style.get(kind) == Some(enabled) {
            return Ok(Effect::default());
        }
        let before_pending = self.pending_inline_style.clone();
        let before_selection = self.selection.clone();
        let before_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let before_math = super::history_position(edit);
        self.pending_inline_style.set(kind, enabled)?;
        self.advance_history_merge_epoch();
        let after_math = MathEditPosition::with_branch(
            edit.target.clone(),
            edit.cursor,
            MathEditBranch::Text {
                path: path.clone(),
                anchor: edit.cursor,
            },
        );
        let before = SessionHistoryState::new(
            before_selection.clone(),
            before_affinity,
            before_pending,
            before_state_id,
        )
        .with_math_edit(Some(before_math));
        let after = SessionHistoryState::new(
            before_selection,
            before_affinity,
            self.pending_inline_style.clone(),
            before_state_id,
        )
        .with_math_edit(Some(after_math));
        self.record_history_edit(SessionHistoryEdit::new(
            DocumentPatch::PendingStyle,
            before,
            after,
        ));
        Ok(Effect::default())
    }
}

pub(super) fn formatted_text_selection(
    tree: &MathTree,
    previous_path: &MathTextFlowPath,
    format: &MathTextFormat,
) -> Option<(MathTextFlowPath, usize, usize)> {
    let source_start =
        (previous_path.source_range().0 as usize).checked_add(format.selected_source_start)?;
    let mut path =
        super::path::path_at_source_boundary(tree, source_start, true, previous_path.align_cell())?;
    if previous_path.align_cell().is_some() {
        let host_base = previous_path
            .host_source_range()
            .0
            .checked_sub(previous_path.source_range().0 as usize)?;
        path = path.with_host_source_base(host_base)?;
    }
    let text = path.resolve(tree)?;
    let display = text
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    if display != format.selected_text
        || inherited_environment_value(tree, &path, format.kind) != Some(format.enabled)
    {
        return None;
    }
    let focus = if format.selection_reversed {
        0
    } else {
        display.len()
    };
    let anchor = if format.selection_reversed {
        display.len()
    } else {
        0
    };
    Some((path, anchor, focus))
}

fn environment_command(
    kind: InlineEnvironmentKind,
    enabled: bool,
) -> Result<&'static str, EditError> {
    match (kind, enabled) {
        (InlineEnvironmentKind::Bold, true) => Ok(r"\textbf{"),
        (InlineEnvironmentKind::Bold, false) => Ok(r"\textmd{"),
        (InlineEnvironmentKind::Italic, true) => Ok(r"\textit{"),
        (InlineEnvironmentKind::Italic, false) => Ok(r"\textup{"),
        (InlineEnvironmentKind::Font, _) => Err(EditError::UnsupportedInlineFormat),
    }
}

fn inherited_environment_value(
    tree: &MathTree,
    path: &MathTextFlowPath,
    kind: InlineEnvironmentKind,
) -> Option<bool> {
    fn in_flow(
        flow: &caret_doc::MathFlow,
        steps: &[MathTextFlowPathStep],
        kind: InlineEnvironmentKind,
        inherited: bool,
    ) -> Option<bool> {
        let MathTextFlowPathStep::Content(index) = steps.first()? else {
            return None;
        };
        in_content(flow.content.get(*index)?, &steps[1..], kind, inherited)
    }

    fn in_content(
        content: &MathContent,
        steps: &[MathTextFlowPathStep],
        kind: InlineEnvironmentKind,
        inherited: bool,
    ) -> Option<bool> {
        match content {
            MathContent::Text(_) if steps.is_empty() => Some(inherited),
            MathContent::Environment(environment) => {
                let MathTextFlowPathStep::EnvironmentChild(index) = steps.first()? else {
                    return None;
                };
                let inherited = if environment.kind == kind {
                    environment
                        .props
                        .get("enabled")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true)
                } else {
                    inherited
                };
                in_content(
                    environment.children.get(*index)?,
                    &steps[1..],
                    kind,
                    inherited,
                )
            }
            MathContent::Math(node) => {
                let MathTextFlowPathStep::Operand(index) = steps.first()? else {
                    return None;
                };
                in_flow(node.children.get(*index)?, &steps[1..], kind, inherited)
            }
            _ => None,
        }
    }

    in_flow(&tree.root, path.steps(), kind, false)
}
