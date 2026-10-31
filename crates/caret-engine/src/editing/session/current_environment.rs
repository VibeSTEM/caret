//! 从当前焦点解析行内环境、直接下划线 Run 与导航边界。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::Session;
use crate::editing::{Affinity, EditError, InlineEnvironmentRange, Pos, Selection};
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

    /// 返回当前焦点的行内高亮范围。嵌套环境优先；无环境时，若折叠光标位于带下划线属性的 Run，
    /// 则高亮该 Run。两种来源保持为独立模型。
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

    /// 返回折叠光标所属的直接下划线 Run，不将 Run 属性伪装成行内环境。
    pub fn current_inline_underline_range(&mut self) -> Result<Option<Selection>, EditError> {
        if self.math_focus().is_some()
            || self.block_environment_exit().is_some()
            || self.selected_block_environment().is_some()
            || !self.selected_block_environments().is_empty()
        {
            return Ok(None);
        }
        let selection = self.selection().clone();
        if selection.anchor != selection.focus {
            return Ok(None);
        }
        let focus = selection.focus;
        let Some(range) = self
            .text_map_for(&focus)?
            .underlined_run_at(focus.offset, focus.affinity)
        else {
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

    /// 视觉导航目标离开当前行内范围时返回外侧亲和性。环境和直接 Run 属性仍是不同来源，环境优先。
    pub fn navigation_inline_scope_exit_affinity(
        &mut self,
        destination: Option<&Pos>,
    ) -> Result<Option<Affinity>, EditError> {
        let selection = self.selection().clone();
        if selection.anchor != selection.focus {
            return Ok(None);
        }
        let environment = self.current_inline_environment()?;
        let direct_underline = if environment.is_none() {
            self.current_inline_underline_range()?
        } else {
            None
        };
        let active_range = environment
            .as_ref()
            .map(|range| {
                (
                    range.story.clone(),
                    range.paragraph_id.clone(),
                    range.start_utf8,
                    range.end_utf8,
                )
            })
            .or_else(|| {
                direct_underline.as_ref().map(|range| {
                    (
                        range.focus.story.clone(),
                        range.focus.paragraph_id.clone(),
                        range.anchor.offset,
                        range.focus.offset,
                    )
                })
            });
        let Some((story, paragraph_id, start, end)) = active_range else {
            return Ok(None);
        };
        let destination_is_outside = destination.is_none_or(|position| {
            position.story != story
                || position.paragraph_id != paragraph_id
                || position.offset < start
                || position.offset > end
        });
        let focus = &selection.focus;
        let affinity = if destination_is_outside
            && focus.offset == start
            && focus.affinity == Affinity::Downstream
        {
            Some(Affinity::Upstream)
        } else if destination_is_outside
            && focus.offset == end
            && focus.affinity == Affinity::Upstream
        {
            Some(Affinity::Downstream)
        } else {
            None
        };
        Ok(affinity)
    }
}
