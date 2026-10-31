//! 从当前焦点解析其所属的环境栈与最内层行内环境。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::Session;
use caret_structure::{StructureKind, StructurePosition};

use crate::editing::{
    CurrentEnvironment, CurrentEnvironmentStack, EditError, EnvironmentKind,
    InlineEnvironmentRange, InlineSpan,
};

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

    /// 返回焦点所处的全部环境：块环境由结构索引给出，行内环境（含公式）按逻辑亲和性给出。
    pub fn current_environment_stack(&mut self) -> Result<CurrentEnvironmentStack, EditError> {
        if self.block_environment_exit().is_some()
            || self.selected_block_environment().is_some()
            || !self.selected_block_environments().is_empty()
        {
            return Ok(CurrentEnvironmentStack::default());
        }
        let focus = self.selection().focus.clone();
        let mut entries = self
            .structure_index()
            .at(&StructurePosition {
                paragraph_id: focus.paragraph_id.clone(),
                offset_utf8: focus.offset,
            })
            .into_iter()
            .filter_map(|entry| {
                let kind = match entry.kind {
                    StructureKind::Section => EnvironmentKind::Section,
                    StructureKind::Custom => EnvironmentKind::Custom,
                    _ => return None,
                };
                Some(CurrentEnvironment {
                    id: entry.id.clone(),
                    kind,
                    inline_range: None,
                })
            })
            .collect::<Vec<_>>();
        let map = self.text_map_for(&focus)?;
        entries.extend(
            map.inline_environment_stack_at(focus.offset, focus.affinity)
                .into_iter()
                .map(|span| CurrentEnvironment {
                    id: span.id.clone(),
                    kind: span.kind,
                    inline_range: Some(InlineSpan {
                        paragraph_id: focus.paragraph_id.clone(),
                        story: focus.story.clone(),
                        start_utf8: span.start,
                        end_utf8: span.end,
                    }),
                }),
        );
        Ok(CurrentEnvironmentStack::new(entries))
    }
}
