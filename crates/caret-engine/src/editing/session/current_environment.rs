//! 从当前焦点解析其所属的最内层行内环境。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::Session;
use crate::editing::{Affinity, EditError, InlineEnvironmentRange, Pos, Selection};
use caret_doc::InlineNode;
use std::ops::Range;

impl Session {
    /// 返回焦点按逻辑亲和性所属的最内层行内环境范围。
    pub fn current_inline_environment(
        &mut self,
    ) -> Result<Option<InlineEnvironmentRange>, EditError> {
        if self.block_environment_exit().is_some()
            || self.selected_block_environment().is_some()
            || !self.selected_block_environments().is_empty()
        {
            return Ok(None);
        }

        let focus = self.selection().focus.clone();
        let Some((environment_id, start_utf8, end_utf8)) = self
            .text_map_for(&focus)?
            .inline_environment_at(focus.offset, focus.affinity)
        else {
            return Ok(None);
        };

        Ok(Some(InlineEnvironmentRange {
            environment_id,
            paragraph_id: focus.paragraph_id,
            story: focus.story,
            start_utf8,
            end_utf8,
        }))
    }

    /// Returns the active inline highlight range. Nested environments win; absent an environment,
    /// an enabled direct underline property highlights its containing Run while the caret is
    /// collapsed there.
    pub fn current_inline_highlight_range(&mut self) -> Result<Option<Selection>, EditError> {
        if let Some(range) = self.current_inline_environment()? {
            return Ok(Some(Selection {
                anchor: Pos {
                    story: range.story.clone(),
                    paragraph_id: range.paragraph_id.clone(),
                    offset: range.start_utf8,
                    affinity: Affinity::Downstream,
                },
                focus: Pos {
                    story: range.story,
                    paragraph_id: range.paragraph_id,
                    offset: range.end_utf8,
                    affinity: Affinity::Upstream,
                },
            }));
        }
        self.current_inline_underline_range()
    }

    /// 返回焦点所属的显式下划线 Run；用于将直接行内格式投影为当前格式高亮。
    fn current_inline_underline_range(&mut self) -> Result<Option<Selection>, EditError> {
        if self.math_focus().is_some()
            || self.block_environment_exit().is_some()
            || self.selected_block_environment().is_some()
            || !self.selected_block_environments().is_empty()
        {
            return Ok(None);
        }
        let selection = self.selection().clone();
        if selection.anchor.story != selection.focus.story
            || selection.anchor.paragraph_id != selection.focus.paragraph_id
            || selection.anchor.offset != selection.focus.offset
        {
            return Ok(None);
        }
        let focus = selection.focus;
        let paragraph = self.paragraph_for(&focus)?;
        let mut offset = 0;
        let Some(range) = underlined_run_at(
            &paragraph.inlines,
            &mut offset,
            focus.offset,
            focus.affinity,
        ) else {
            return Ok(None);
        };
        Ok(Some(Selection {
            anchor: Pos {
                story: focus.story.clone(),
                paragraph_id: focus.paragraph_id.clone(),
                offset: range.start,
                affinity: Affinity::Downstream,
            },
            focus: Pos {
                story: focus.story,
                paragraph_id: focus.paragraph_id,
                offset: range.end,
                affinity: Affinity::Upstream,
            },
        }))
    }
}

fn underlined_run_at(
    nodes: &[InlineNode],
    offset: &mut usize,
    focus: usize,
    affinity: Affinity,
) -> Option<Range<usize>> {
    for node in nodes {
        match node {
            InlineNode::Run(run) => {
                let start = *offset;
                let end = start.checked_add(run.text.len())?;
                *offset = end;
                let underlined = run
                    .props
                    .get("underline")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true);
                let contains_focus = match affinity {
                    Affinity::Downstream => start <= focus && focus < end,
                    Affinity::Upstream => start < focus && focus <= end,
                };
                if underlined && contains_focus && start < end {
                    return Some(start..end);
                }
            }
            InlineNode::Environment(environment) => {
                if let Some(range) =
                    underlined_run_at(&environment.children, offset, focus, affinity)
                {
                    return Some(range);
                }
            }
            InlineNode::Math(_) | InlineNode::Opaque(_) => {
                *offset = offset.checked_add('\u{fffc}'.len_utf8())?;
            }
            InlineNode::SoftBreak(_) => *offset = offset.checked_add('\n'.len_utf8())?,
        }
    }
    None
}
