//! 执行行内格式环境命令并管理当前插入点的待输入格式。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::environment::inline_adapter::{
    RunPropertyEnvironment, run_property_for_key, run_property_for_kind,
};
use super::super::flow_format::{
    InheritedInlineStyle, inline_format_range_state, run_property_range_state,
};
use super::super::paragraph_ops::{ParagraphLocation, paragraph_at_location};
use super::super::session::history::{DocumentPatch, SessionHistoryEdit, SessionHistoryState};
use super::super::text_map::TextMap;
use super::super::{Affinity, EditError, Effect, InlineSpan, Pos};
use super::Session;
use caret_doc::{DocumentFontTarget, InlineEnvironmentKind, NodeId, Paragraph};
#[cfg(test)]
use inline_tree::format_value;
use inline_tree::{
    apply_font_family_to_range, apply_formats_to_range, clear_run_property_to_range,
    effective_format_at_with_inherited, format_spans, format_spans_with_inherited,
    set_run_property_to_range, unwrap_inline_environment, wrap_format_range,
};
use std::collections::BTreeMap;

fn inherited_inline_style(
    theme: Option<&caret_doc::ThemeSnapshot>,
) -> Result<InheritedInlineStyle, EditError> {
    InheritedInlineStyle::from_theme(theme)
        .map_err(|error| EditError::InvalidDocument(error.to_string()))
}

fn is_collapsed_text_range(anchor: &Pos, focus: &Pos) -> bool {
    anchor.story == focus.story
        && anchor.paragraph_id == focus.paragraph_id
        && anchor.offset == focus.offset
}

/// 以选区起点为模板，把编辑目标换到 `location` 所在段落。
fn paragraph_edit_position(start: &Pos, location: &ParagraphLocation) -> Pos {
    Pos {
        paragraph_id: location.paragraph_id.clone(),
        ..start.clone()
    }
}

/// 段落内容允许嵌套行内环境的最大层数：块环境深度加上标题槽。
fn paragraph_inline_depth(location: &ParagraphLocation) -> usize {
    location.block_path.len() + usize::from(location.is_title)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PendingInlineStyle {
    bold: Option<bool>,
    italic: Option<bool>,
    run_properties: BTreeMap<&'static str, bool>,
    font_family: Option<String>,
}

impl PendingInlineStyle {
    pub(super) fn clear(&mut self) {
        self.bold = None;
        self.italic = None;
        self.run_properties.clear();
        self.font_family = None;
    }

    pub(super) fn get(&self, kind: InlineEnvironmentKind) -> Option<bool> {
        match kind {
            InlineEnvironmentKind::Bold => self.bold,
            InlineEnvironmentKind::Italic => self.italic,
            InlineEnvironmentKind::Font => None,
        }
    }

    pub(super) fn set(
        &mut self,
        kind: InlineEnvironmentKind,
        value: bool,
    ) -> Result<(), EditError> {
        match kind {
            InlineEnvironmentKind::Bold => self.bold = Some(value),
            InlineEnvironmentKind::Italic => self.italic = Some(value),
            InlineEnvironmentKind::Font => return Err(EditError::UnsupportedInlineFormat),
        }
        Ok(())
    }

    pub(super) fn active_formats(
        &self,
    ) -> impl Iterator<Item = (InlineEnvironmentKind, bool)> + '_ {
        [
            self.bold.map(|value| (InlineEnvironmentKind::Bold, value)),
            self.italic
                .map(|value| (InlineEnvironmentKind::Italic, value)),
        ]
        .into_iter()
        .flatten()
    }

    pub(super) fn active_font_family(&self) -> Option<&str> {
        self.font_family.as_deref()
    }

    pub(super) fn set_font_family(&mut self, family: String) {
        self.font_family = Some(family);
    }

    pub(super) fn run_property(&self, property: RunPropertyEnvironment) -> Option<bool> {
        self.run_properties.get(property.key).copied()
    }

    pub(super) fn set_run_property(&mut self, property: RunPropertyEnvironment, enabled: bool) {
        self.run_properties.insert(property.key, enabled);
    }

    pub(super) fn active_run_properties(&self) -> impl Iterator<Item = (&'static str, bool)> + '_ {
        self.run_properties
            .iter()
            .map(|(key, enabled)| (*key, *enabled))
    }

    pub(super) fn is_active(&self) -> bool {
        self.bold.is_some()
            || self.italic.is_some()
            || !self.run_properties.is_empty()
            || self.font_family.is_some()
    }

    pub(super) fn shows_insertion_slot(&self) -> bool {
        self.bold == Some(true)
            || self.italic == Some(true)
            || self.run_properties.values().any(|enabled| *enabled)
            || self.font_family.is_some()
    }
}

impl Session {
    pub(super) fn remove_inline_environment_preserving_content(
        &mut self,
        environment_id: &NodeId,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        let position = self.selection.focus.clone();
        let mut paragraph = self.paragraph_for(&position)?.clone();
        if !unwrap_inline_environment(&mut paragraph.inlines, environment_id) {
            return Ok(Effect::default());
        }
        self.replace_paragraph(&position, paragraph)?;
        self.text_map_cache.clear();
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();
        Ok(Effect {
            document_changed: true,
            changed_paragraph_id: Some(position.paragraph_id),
            ..Effect::default()
        })
    }

    pub(super) fn remove_run_property_preserving_inheritance(
        &mut self,
        range: &InlineSpan,
        property: RunPropertyEnvironment,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        let position = self.selection.focus.clone();
        if position.paragraph_id != range.paragraph_id || position.story != range.story {
            return Ok(Effect::default());
        }
        let mut paragraph = self.paragraph_for(&position)?.clone();
        clear_run_property_to_range(
            &mut paragraph.inlines,
            range.start_utf8,
            range.end_utf8,
            property,
        )?;
        if paragraph == *self.paragraph_for(&position)? {
            return Ok(Effect::default());
        }
        self.replace_paragraph(&position, paragraph)?;
        self.text_map_cache.clear();
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();
        Ok(Effect {
            document_changed: true,
            changed_paragraph_id: Some(position.paragraph_id),
            ..Effect::default()
        })
    }

    pub(super) fn remove_run_property_from_current_range(
        &mut self,
        kind: crate::editing::EnvironmentKind,
    ) -> Result<Effect, EditError> {
        let Some(property) = run_property_for_kind(kind).copied() else {
            return Ok(Effect::default());
        };
        let range = self
            .current_environment_view()?
            .inline()
            .iter()
            .rev()
            .find(|entry| {
                entry.source == property.adapter.source && entry.kind == property.adapter.kind
            })
            .and_then(|entry| entry.inline_range.clone());
        let Some(range) = range else {
            return Ok(Effect::default());
        };
        self.remove_run_property_preserving_inheritance(&range, property)
    }

    pub(super) fn toggle_run_underline(&mut self) -> Result<Effect, EditError> {
        let property = run_property_for_kind(crate::editing::EnvironmentKind::Underline)
            .copied()
            .ok_or(EditError::UnsupportedInlineFormat)?;
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let (anchor, focus) = self.edit_range()?;
        if is_collapsed_text_range(&anchor, &focus) {
            let position = self.selection.focus.clone();
            let paragraph = self.paragraph_for(&position)?;
            let inherited = format_spans(&paragraph.inlines)?
                .into_iter()
                .find(|span| {
                    span.run_property_target
                        && ((span.start < position.offset && position.offset < span.end)
                            || (position.offset == span.start
                                && position.affinity == Affinity::Downstream)
                            || (position.offset == span.end
                                && position.affinity == Affinity::Upstream))
                })
                .and_then(|span| span.properties.get(property.key).copied())
                .unwrap_or(false);
            let current = self
                .pending_inline_style
                .run_property(property)
                .unwrap_or(inherited);
            let enabled = !current;
            self.record_pending_style_edit(|style| {
                style.set_run_property(property, enabled);
                Ok(())
            })?;
            return Ok(Effect::default());
        }

        let (start, end) = self.ordered_text_range(&anchor, &focus)?;
        let mut ranges = Vec::new();
        let mut any_disabled = false;
        let mut any_text = false;
        self.visit_ordered_paragraph_ranges(&start, &end, |location, paragraph, _, range| {
            if range.is_empty() {
                return Ok(());
            }
            let (contains_run, contains_disabled) =
                run_property_range_state(&paragraph.inlines, range.clone(), property)?;
            any_text |= contains_run;
            any_disabled |= contains_disabled;
            ranges.push((location.clone(), range));
            Ok(())
        })?;
        if !any_text {
            return Ok(Effect::default());
        }
        let enabled = any_disabled;
        let mut edits = Vec::with_capacity(ranges.len());
        for (location, range) in ranges {
            let original = paragraph_at_location(&self.document.body, &location)
                .ok_or(EditError::InvalidPosition)?;
            let map = TextMap::new(original)?;
            let mut paragraph = original.clone();
            set_run_property_to_range(
                &mut paragraph.inlines,
                map.snap_to_grapheme_boundary(range.start, Affinity::Upstream),
                map.snap_to_grapheme_boundary(range.end, Affinity::Downstream),
                property,
                enabled,
            )?;
            if paragraph != *original {
                edits.push((paragraph_edit_position(&start, &location), paragraph));
            }
        }
        self.apply_paragraph_edits(edits, &start, &end)
    }

    pub(super) fn set_document_font_family(
        &mut self,
        target: &DocumentFontTarget,
        family: Option<String>,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let target = target
            .normalized()
            .map_err(|_| EditError::InvalidFontFamily)?;
        if family
            .as_deref()
            .is_some_and(|family| family.trim().is_empty())
        {
            return Err(EditError::InvalidFontFamily);
        }
        if self.document.global_settings.font_family(&target) == family.as_deref() {
            return Ok(Effect::default());
        }
        self.document
            .global_settings
            .set_font_family(&target, family)
            .map_err(|_| EditError::InvalidFontFamily)?;
        Ok(Effect {
            document_changed: true,
            ..Effect::default()
        })
    }

    pub(super) fn inline_depth_for_paragraph(
        &self,
        paragraph_id: &NodeId,
    ) -> Result<usize, EditError> {
        self.paragraph_index
            .get(paragraph_id)
            .map(paragraph_inline_depth)
            .ok_or(EditError::InvalidPosition)
    }

    pub(super) fn set_default_font_family(
        &mut self,
        family: Option<String>,
    ) -> Result<Effect, EditError> {
        self.set_document_font_family(&DocumentFontTarget::Script("Latn".to_owned()), family)
    }

    pub(super) fn set_inline_font_family(&mut self, family: &str) -> Result<Effect, EditError> {
        if family.trim().is_empty() {
            return Err(EditError::InvalidFontFamily);
        }
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let inherited = inherited_inline_style(self.document.global_settings.theme.as_ref())?;
        let (anchor, focus) = self.edit_range()?;
        if is_collapsed_text_range(&anchor, &focus) {
            if self.pending_inline_style.font_family.as_deref() == Some(family) {
                return Ok(Effect::default());
            }
            self.record_pending_style_edit(|style| {
                style.set_font_family(family.to_owned());
                Ok(())
            })?;
            return Ok(Effect::default());
        }

        let (start, end) = self.ordered_text_range(&anchor, &focus)?;
        if start.paragraph_id == end.paragraph_id {
            let location = self
                .paragraph_index
                .get(&start.paragraph_id)
                .ok_or(EditError::InvalidPosition)?;
            let original = paragraph_at_location(&self.document.body, location)
                .ok_or(EditError::InvalidPosition)?;
            let map = TextMap::new(original)?;
            if start.offset >= end.offset || end.offset > map.text.len() {
                return Err(EditError::InvalidPosition);
            }
            map.validate_pos(&start)?;
            map.validate_pos(&end)?;
            let mut paragraph = original.clone();
            apply_font_family_to_range(
                &mut paragraph,
                start.offset,
                end.offset,
                family,
                paragraph_inline_depth(location),
                inherited,
            )?;
            let mut edits = Vec::new();
            if paragraph != *original {
                edits.push((start.clone(), paragraph));
            }
            return self.apply_paragraph_edits(edits, &start, &end);
        }

        let mut edits = Vec::new();
        self.visit_ordered_paragraph_ranges(&start, &end, |location, original, _, range| {
            if range.is_empty() {
                return Ok(());
            }
            let mut paragraph = original.clone();
            apply_font_family_to_range(
                &mut paragraph,
                range.start,
                range.end,
                family,
                paragraph_inline_depth(location),
                inherited.clone(),
            )?;
            if paragraph != *original {
                edits.push((paragraph_edit_position(&start, location), paragraph));
            }
            Ok(())
        })?;
        self.apply_paragraph_edits(edits, &start, &end)
    }

    pub(super) fn toggle_inline_format(
        &mut self,
        kind: InlineEnvironmentKind,
    ) -> Result<Effect, EditError> {
        if matches!(kind, InlineEnvironmentKind::Font) {
            return Err(EditError::UnsupportedInlineFormat);
        }
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let inherited = inherited_inline_style(self.document.global_settings.theme.as_ref())?;

        let (anchor, focus) = self.edit_range()?;
        if is_collapsed_text_range(&anchor, &focus) {
            let position = self.selection.focus.clone();
            let inherited_value = effective_format_at_with_inherited(
                &self.paragraph_for(&position)?.inlines,
                position.offset,
                position.affinity,
                kind,
                inherited,
            )?;
            let current = self
                .pending_inline_style
                .get(kind)
                .unwrap_or(inherited_value);
            self.record_pending_style_edit(|style| style.set(kind, !current))?;
            return Ok(Effect::default());
        }

        let (start, end) = self.ordered_text_range(&anchor, &focus)?;
        let mut ranges = Vec::new();
        let mut any_disabled = false;
        let mut any_content = false;
        self.visit_ordered_paragraph_ranges(&start, &end, |location, paragraph, _, range| {
            if range.is_empty() {
                return Ok(());
            }
            // 正文旧适配器对段落内未知 TextFlow 扩展采取整体拒绝策略，继续保留该边界。
            format_spans_with_inherited(&paragraph.inlines, inherited.clone())?;
            let (contains_text, contains_disabled) =
                inline_format_range_state(&paragraph.inlines, range.clone(), kind, &inherited)?;
            any_content |= contains_text;
            any_disabled |= contains_disabled;
            ranges.push((location.clone(), range));
            Ok(())
        })?;
        if !any_content {
            return Ok(Effect::default());
        }
        let enabled = any_disabled;

        // 先在段落副本中完成所有切分；任何一处失败都不会留下部分格式修改。
        let mut edits = Vec::with_capacity(ranges.len());
        for (location, range) in ranges {
            let mut paragraph = paragraph_at_location(&self.document.body, &location)
                .ok_or(EditError::InvalidPosition)?
                .clone();
            wrap_format_range(
                &mut paragraph.inlines,
                range.start,
                range.end,
                kind,
                enabled,
                paragraph_inline_depth(&location),
                inherited.clone(),
            )?;
            edits.push((paragraph_edit_position(&start, &location), paragraph));
        }
        self.apply_paragraph_edits(edits, &start, &end)
    }

    pub(super) fn apply_pending_style_to_inserted_text(
        &mut self,
        paragraph_id: &NodeId,
        start: usize,
        end: usize,
    ) -> Result<(), EditError> {
        if !self.pending_inline_style.is_active() || start >= end {
            return Ok(());
        }
        let position = Pos {
            story: self
                .paragraph_index
                .get(paragraph_id)
                .ok_or(EditError::InvalidPosition)?
                .story
                .clone(),
            paragraph_id: paragraph_id.clone(),
            offset: start,
            affinity: Affinity::Downstream,
        };
        let mut paragraph = self.paragraph_for(&position)?.clone();
        self.apply_pending_style_to_paragraph(
            &mut paragraph,
            start,
            end,
            self.inline_depth_for_paragraph(paragraph_id)?,
        )?;
        self.replace_paragraph(&position, paragraph)?;
        self.text_map_cache.clear();
        Ok(())
    }

    pub(super) fn apply_pending_style_to_paragraph(
        &self,
        paragraph: &mut Paragraph,
        start: usize,
        end: usize,
        inline_depth: usize,
    ) -> Result<(), EditError> {
        let inherited = inherited_inline_style(self.document.global_settings.theme.as_ref())?;
        let formats: Vec<_> = self.pending_inline_style.active_formats().collect();
        let run_properties: Vec<_> = self
            .pending_inline_style
            .run_properties
            .iter()
            .filter_map(|(key, enabled)| {
                run_property_for_key(key).map(|property| (*property, *enabled))
            })
            .collect();
        let font_family = self
            .pending_inline_style
            .active_font_family()
            .map(str::to_owned);
        if (formats.is_empty() && run_properties.is_empty() && font_family.is_none())
            || start >= end
        {
            return Ok(());
        }
        let map = TextMap::new(paragraph)?;
        let snapped_start = map.snap_to_grapheme_boundary(start, Affinity::Upstream);
        let snapped_end = map.snap_to_grapheme_boundary(end, Affinity::Downstream);
        apply_formats_to_range(
            paragraph,
            snapped_start,
            snapped_end,
            &formats,
            inline_depth,
            inherited.clone(),
        )?;
        for (property, enabled) in run_properties {
            set_run_property_to_range(
                &mut paragraph.inlines,
                snapped_start,
                snapped_end,
                property,
                enabled,
            )?;
        }
        if let Some(family) = font_family {
            apply_font_family_to_range(
                paragraph,
                snapped_start,
                snapped_end,
                &family,
                inline_depth,
                inherited,
            )?;
        }
        Ok(())
    }

    /// 记录一次仅修改待输入格式的事务；撤销合并粒度沿用 `PendingStyle` 补丁语义。
    fn record_pending_style_edit(
        &mut self,
        mutate: impl FnOnce(&mut PendingInlineStyle) -> Result<(), EditError>,
    ) -> Result<(), EditError> {
        let before_pending_style = self.pending_inline_style.clone();
        let before_selection = self.selection.clone();
        let before_anchor_visual_affinity = self.selection_anchor_visual_affinity;
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        mutate(&mut self.pending_inline_style)?;
        self.advance_history_merge_epoch();
        self.record_history_edit(SessionHistoryEdit::new(
            DocumentPatch::PendingStyle,
            SessionHistoryState::new(
                before_selection,
                before_visual_affinity,
                before_pending_style,
                before_state_id,
            )
            .with_anchor_visual_affinity(before_anchor_visual_affinity),
            SessionHistoryState::new(
                self.selection.clone(),
                self.selection_visual_affinity,
                self.pending_inline_style.clone(),
                before_state_id,
            )
            .with_anchor_visual_affinity(self.selection_anchor_visual_affinity),
        ));
        self.history_state_id = before_state_id;
        Ok(())
    }

    /// 提交已收集的段落替换：全部落盘后统一清缓存、清待输入格式并推进合并轮次。
    fn apply_paragraph_edits(
        &mut self,
        edits: Vec<(Pos, Paragraph)>,
        start: &Pos,
        end: &Pos,
    ) -> Result<Effect, EditError> {
        if edits.is_empty() {
            return Ok(Effect::default());
        }
        for (position, paragraph) in edits {
            self.replace_paragraph(&position, paragraph)?;
        }
        self.text_map_cache.clear();
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();
        Ok(Effect {
            document_changed: true,
            changed_paragraph_id: (start.paragraph_id == end.paragraph_id)
                .then_some(start.paragraph_id.clone()),
            ..Effect::default()
        })
    }
}

pub(super) mod inline_tree;

#[cfg(test)]
mod tests;
