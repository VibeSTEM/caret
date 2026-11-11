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
    ///
    /// 该查询是 [`Session::current_environment_view`] 的投影：块外停靠、结构选区或
    /// 公式块聚焦时视图本身没有行内候选，因此这里无需重复判断。
    pub fn current_inline_environment(
        &mut self,
    ) -> Result<Option<InlineEnvironmentRange>, EditError> {
        let view = self.current_environment_view()?;
        let Some(entry) = view
            .inline()
            .iter()
            .rev()
            .find(|entry| entry.source == EnvironmentSource::InlineEnvironment)
        else {
            return Ok(None);
        };
        let (Some(environment_id), Some(range)) = (entry.id.clone(), entry.inline_range.clone())
        else {
            return Ok(None);
        };
        Ok(Some(InlineEnvironmentRange {
            environment_id,
            paragraph_id: range.paragraph_id,
            story: range.story,
            start_utf8: range.start_utf8,
            end_utf8: range.end_utf8,
        }))
    }

    /// 从编辑状态推导 Inline 和 Block 候选；此查询不依赖布局快照。
    pub fn current_environment_view(&mut self) -> Result<CurrentEnvironmentView, EditError> {
        self.ensure_math_edit_revision_current()?;
        if self.has_structure_selection() || !self.selected_math_blocks().is_empty() {
            return Ok(CurrentEnvironmentView::default());
        }
        let exited_environment_id = self
            .block_boundary_exit()
            .map(|(environment_id, _)| environment_id.clone());
        let focus = self.selection().focus.clone();
        let active_block_id = self
            .current_math_edit_target()
            .and_then(|target| match target {
                MathEditTarget::Block { block_id, .. } => Some(block_id.clone()),
                MathEditTarget::Inline { .. } => None,
            })
            .or_else(|| self.block_boundary_exit().map(|(id, _)| id));
        let structure_index = self.structure_index();
        if active_block_id
            .as_ref()
            .is_some_and(|block_id| structure_index.get(block_id).is_none())
        {
            return Err(EditError::InvalidPosition);
        }
        let structure_stack = match active_block_id.as_ref() {
            Some(block_id) => structure_index
                .stack_for_id(block_id)
                .into_iter()
                .filter(|entry| &entry.id != block_id)
                .collect(),
            None => structure_index.at(&StructurePosition {
                paragraph_id: focus.paragraph_id.clone(),
                offset_utf8: focus.offset,
            }),
        };
        let mut block = Vec::new();
        for entry in structure_stack {
            if exited_environment_id.as_ref() == Some(&entry.id) {
                continue;
            }
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

        if let Some(MathEditTarget::Block { block_id, align }) = self.current_math_edit_target() {
            let adapter = for_math_block(*align);
            block.push(EnvironmentEntry {
                id: Some(block_id.clone()),
                kind: adapter.kind,
                source: adapter.source,
                inline_range: None,
                depth: block.len(),
            });
        }

        if exited_environment_id.is_some() {
            return Ok(CurrentEnvironmentView::new(Vec::new(), block));
        }

        if matches!(
            self.current_math_edit_target(),
            Some(MathEditTarget::Block { .. })
        ) {
            return Ok(CurrentEnvironmentView::new(Vec::new(), block));
        }

        let active_inline_math_id =
            self.current_math_edit_target()
                .and_then(|target| match target {
                    MathEditTarget::Inline { math_id, .. } => Some(math_id.clone()),
                    MathEditTarget::Block { .. } => None,
                });
        let host_caret_position = self.host_caret_position.clone();
        let map = self.text_map_for(&focus)?;
        let mut inline = map
            .inline_environment_stack_at(focus.offset, focus.affinity)
            .into_iter()
            .filter(|span| {
                span.source != EnvironmentSource::InlineMath
                    || span.id.as_ref() == active_inline_math_id.as_ref()
            })
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
        if let Some(crate::editing::HostCaretPosition::Inside {
            host_id,
            kind: crate::editing::HostCaretKind::Inline,
            local,
        }) = host_caret_position.as_ref()
            && local == &focus
        {
            inline.extend(
                map.empty_inline_environment_stack_for_host(focus.offset, host_id)
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
                    }),
            );
        }
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
