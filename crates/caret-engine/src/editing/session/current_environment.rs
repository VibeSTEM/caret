//! 从文档和编辑焦点推导分层的 Inline/Block 当前环境候选。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::Session;
use crate::editing::environment::block_adapter::{for_block_environment, for_math_block};
use crate::editing::environment::inline_adapter::INLINE_MATH_ADAPTER;
use crate::editing::session::math_edit::MathEditTarget;
use caret_doc::BlockEnvironmentKind;
use caret_structure::{StructureKind, StructurePosition};

use crate::editing::{
    CurrentEnvironmentView, EditError, EnvironmentEntry, EnvironmentSource, InlineEnvironmentRange,
    InlineSpan,
};

impl Session {
    /// 返回当前真实 InlineEnvironment 的最内层范围，不含 Run 属性和公式对象。
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

    /// 从编辑状态推导 Inline 和 Block 候选；此查询不依赖布局快照。
    pub fn current_environment_view(&mut self) -> Result<CurrentEnvironmentView, EditError> {
        if self.block_environment_exit().is_some()
            || self.selected_block_environment().is_some()
            || !self.selected_block_environments().is_empty()
        {
            return Ok(CurrentEnvironmentView::default());
        }
        let focus = self.selection().focus.clone();
        let structure_stack = self
            .structure_index()
            .at(&StructurePosition {
                paragraph_id: focus.paragraph_id.clone(),
                offset_utf8: focus.offset,
            })
            .into_iter()
            .collect::<Vec<_>>();
        let mut block = Vec::new();
        for entry in structure_stack {
            let adapter = match entry.kind {
                StructureKind::Section => for_block_environment(BlockEnvironmentKind::Section),
                StructureKind::Custom => for_block_environment(BlockEnvironmentKind::Custom),
                _ => continue,
            };
            block.push(EnvironmentEntry {
                id: Some(entry.id.clone()),
                kind: adapter.kind,
                source: adapter.source,
                inline_range: None,
                depth: block.len(),
            });
        }

        if let Some(edit) = self.math_edit()
            && let MathEditTarget::Block { block_id, align } = &edit.target
        {
            let adapter = for_math_block(*align);
            block.push(EnvironmentEntry {
                id: Some(block_id.clone()),
                kind: adapter.kind,
                source: adapter.source,
                inline_range: None,
                depth: block.len(),
            });
        }

        if matches!(
            self.math_edit().map(|edit| &edit.target),
            Some(MathEditTarget::Block { .. })
        ) {
            return Ok(CurrentEnvironmentView::new(Vec::new(), block));
        }

        let active_inline_math_id = self.math_edit().and_then(|edit| match &edit.target {
            MathEditTarget::Inline { math_id, .. } => Some(math_id.clone()),
            MathEditTarget::Block { .. } => None,
        });
        let map = self.text_map_for(&focus)?;
        let mut inline = map
            .inline_environment_stack_at(focus.offset, focus.affinity)
            .into_iter()
            .map(|span| EnvironmentEntry {
                id: span.id.clone(),
                kind: span.kind,
                source: span.source,
                inline_range: Some(InlineSpan {
                    paragraph_id: focus.paragraph_id.clone(),
                    story: focus.story.clone(),
                    start_utf8: span.start,
                    end_utf8: span.end,
                }),
                depth: span.depth,
            })
            .collect::<Vec<_>>();
        if let Some(math_id) = active_inline_math_id
            && !inline.iter().any(|entry| {
                entry.source == EnvironmentSource::InlineMath && entry.id.as_ref() == Some(&math_id)
            })
            && let Some(span) = map.inline_math_span(&math_id)
        {
            inline.push(EnvironmentEntry {
                id: Some(math_id),
                kind: INLINE_MATH_ADAPTER.kind,
                source: INLINE_MATH_ADAPTER.source,
                inline_range: Some(InlineSpan {
                    paragraph_id: focus.paragraph_id.clone(),
                    story: focus.story.clone(),
                    start_utf8: span.start,
                    end_utf8: span.end,
                }),
                depth: span.depth,
            });
        }
        inline.sort_by(|left, right| {
            let left_range = left
                .inline_range
                .as_ref()
                .map(|range| range.end_utf8 - range.start_utf8)
                .unwrap_or(usize::MAX);
            let right_range = right
                .inline_range
                .as_ref()
                .map(|range| range.end_utf8 - range.start_utf8)
                .unwrap_or(usize::MAX);
            right_range
                .cmp(&left_range)
                .then_with(|| left.depth.cmp(&right.depth))
                .then_with(|| {
                    super::super::environment::inline_adapter::precedence_for(
                        left.source,
                        left.kind,
                    )
                    .cmp(
                        &super::super::environment::inline_adapter::precedence_for(
                            right.source,
                            right.kind,
                        ),
                    )
                })
        });

        Ok(CurrentEnvironmentView::new(inline, block))
    }
}
