//! 对正文与数学共用的内容流执行纯行内格式范围转换。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::ops::Range;

use caret_doc::{FlowContent, InlineEnvironment, InlineEnvironmentKind, NodeId, TextFlow};
use serde_json::Value;

use crate::editing::environment::inline_adapter::{RunPropertyEnvironment, set_run_property_value};

use super::flow_edit::{
    RangeSplit, classify_range_split, merge_run_pair, merge_runs_at_selected_seams, split_run_at,
    text_flow_boundary, text_flow_len,
};
use super::{Affinity, EditError};

const ATOM_LEN: usize = '\u{fffc}'.len_utf8();

#[derive(Clone, Copy)]
pub(super) enum InlineFormatChange<'a> {
    State(InlineEnvironmentKind, bool),
    FontFamily(&'a str),
}

#[derive(Clone, Default)]
pub(super) struct InheritedInlineStyle {
    /// 未设置仍可继承主题/模板；显式 false 必须由关闭环境覆盖。
    pub(super) bold: Option<bool>,
    pub(super) italic: Option<bool>,
    pub(super) font_family: Option<String>,
}

impl InheritedInlineStyle {
    pub(super) fn state(&self, kind: InlineEnvironmentKind) -> Option<bool> {
        match kind {
            InlineEnvironmentKind::Bold => Some(self.bold.unwrap_or(false)),
            InlineEnvironmentKind::Italic => Some(self.italic.unwrap_or(false)),
            InlineEnvironmentKind::Font => None,
        }
    }

    pub(super) fn with_environment<T>(&self, environment: &InlineEnvironment<T>) -> Self {
        let mut style = self.clone();
        let delta = environment.text_style();
        if let Some(bold) = delta.bold {
            style.bold = Some(bold);
        }
        if let Some(italic) = delta.italic {
            style.italic = Some(italic);
        }
        if let Some(font_family) = delta.font_family {
            style.font_family = Some(font_family);
        }
        style
    }
}

pub(super) struct InlineFormatContext {
    pub(super) inherited: InheritedInlineStyle,
    /// 当前列表中内容项的模型深度；新增环境会使其子项再深一层。
    pub(super) content_depth: usize,
    pub(super) max_depth: usize,
}

impl InlineFormatChange<'_> {
    fn kind(self) -> InlineEnvironmentKind {
        match self {
            Self::State(kind, _) => kind,
            Self::FontFamily(_) => InlineEnvironmentKind::Font,
        }
    }

    fn matches(self, inherited: &InheritedInlineStyle) -> bool {
        match self {
            Self::State(InlineEnvironmentKind::Bold, value) => inherited.bold == Some(value),
            Self::State(InlineEnvironmentKind::Italic, value) => inherited.italic == Some(value),
            Self::State(InlineEnvironmentKind::Font, _) => false,
            Self::FontFamily(family) => inherited.font_family.as_deref() == Some(family),
        }
    }

    fn set<T>(self, environment: &mut InlineEnvironment<T>) {
        match self {
            Self::State(_, value) => {
                environment
                    .props
                    .insert("enabled".to_owned(), Value::Bool(value));
            }
            Self::FontFamily(family) => {
                environment
                    .props
                    .insert("fontFamily".to_owned(), Value::String(family.to_owned()));
            }
        }
    }
}

/// 格式转换不改逻辑文本；错误时输入树保持不变，原子节点只保留而不格式化。
///
/// 只有实际拆分节点时才拒绝无法分配给两个片段的未知字段；完整节点的字段原样保留。
pub(super) fn format_flow_range<T: Clone + PartialEq>(
    contents: &[FlowContent<T>],
    range: Range<usize>,
    change: InlineFormatChange<'_>,
    context: InlineFormatContext,
) -> Result<Vec<FlowContent<T>>, EditError> {
    match change {
        InlineFormatChange::State(InlineEnvironmentKind::Font, _) => {
            return Err(EditError::UnsupportedInlineFormat);
        }
        InlineFormatChange::FontFamily(family) if family.trim().is_empty() => {
            return Err(EditError::InvalidFontFamily);
        }
        _ => {}
    }
    if range.start > range.end
        || !flow_boundary(contents, range.start)?
        || !flow_boundary(contents, range.end)?
    {
        return Err(EditError::InvalidPosition);
    }
    if range.is_empty() || !range_needs_change(contents, &range, change, &context.inherited, 0)? {
        return Ok(contents.to_vec());
    }
    let (prefix_and_selected, after) = split_contents(contents.to_vec(), range.end)?;
    let (before, selected) = split_contents(prefix_and_selected, range.start)?;
    let mut selected = apply_selected(
        selected,
        change,
        &context.inherited,
        context.content_depth,
        context.max_depth,
    )?;
    normalize(&mut selected);
    let before_len = before.len();
    let mut selected_end = before_len + selected.len();
    let mut result = before;
    result.extend(selected);
    result.extend(after);
    if before_len > 0 && merge_adjacent(&mut result, before_len - 1) {
        selected_end = selected_end.saturating_sub(1);
    }
    merge_adjacent(&mut result, selected_end.saturating_sub(1));
    Ok(result)
}

/// 对共同内容流中的 Run 属性执行范围转换，正文与数学 TextFlow 共用拆分和归一化。
pub(super) fn format_flow_run_property<T: Clone>(
    contents: &[FlowContent<T>],
    range: Range<usize>,
    property: RunPropertyEnvironment,
    value: Option<bool>,
) -> Result<Vec<FlowContent<T>>, EditError> {
    if range.start > range.end
        || !flow_boundary(contents, range.start)?
        || !flow_boundary(contents, range.end)?
    {
        return Err(EditError::InvalidPosition);
    }
    if range.is_empty() {
        return Ok(contents.to_vec());
    }

    fn split_runs_at(runs: &mut Vec<caret_doc::Run>, cut: usize) -> Result<(), EditError> {
        let mut offset = 0usize;
        for index in 0..runs.len() {
            let end = offset
                .checked_add(runs[index].text.len())
                .ok_or(EditError::InvalidPosition)?;
            if cut == offset || cut == end {
                return Ok(());
            }
            if offset < cut && cut < end {
                let (left, right) = split_run_at(&runs[index], cut - offset)?;
                runs[index] = left;
                runs.insert(index + 1, right);
                return Ok(());
            }
            offset = end;
        }
        (cut == offset)
            .then_some(())
            .ok_or(EditError::InvalidPosition)
    }

    fn apply<T>(
        contents: &mut [FlowContent<T>],
        property: RunPropertyEnvironment,
        value: Option<bool>,
        range: &Range<usize>,
        offset: &mut usize,
    ) -> Result<(), EditError> {
        for content in contents {
            match content {
                FlowContent::Text(text) => {
                    let text_len = text_flow_len(&text.runs);
                    let text_end = offset
                        .checked_add(text_len)
                        .ok_or(EditError::InvalidPosition)?;
                    let local_start = range.start.saturating_sub(*offset).min(text_len);
                    let local_end = range.end.saturating_sub(*offset).min(text_len);
                    if local_start < local_end {
                        split_runs_at(&mut text.runs, local_end)?;
                        split_runs_at(&mut text.runs, local_start)?;
                    }
                    for run in &mut text.runs {
                        let end = offset
                            .checked_add(run.text.len())
                            .ok_or(EditError::InvalidPosition)?;
                        if !run.text.is_empty() && range.start < end && *offset < range.end {
                            set_run_property_value(run, property, value)?;
                        }
                        *offset = end;
                    }
                    if text.extra.is_empty() {
                        merge_runs_at_selected_seams(&mut text.runs, local_start..local_end);
                    }
                    debug_assert_eq!(*offset, text_end);
                }
                FlowContent::Environment(environment) => {
                    apply(&mut environment.children, property, value, range, offset)?;
                }
                FlowContent::SoftBreak(_) => {
                    *offset = offset.checked_add(1).ok_or(EditError::InvalidPosition)?;
                }
                FlowContent::Math(_) => {
                    *offset = offset
                        .checked_add(ATOM_LEN)
                        .ok_or(EditError::InvalidPosition)?;
                }
                FlowContent::Opaque(_) => {
                    let end = offset
                        .checked_add(ATOM_LEN)
                        .ok_or(EditError::InvalidPosition)?;
                    if range.start < end && *offset < range.end {
                        return Err(EditError::UnsupportedOpaqueContent);
                    }
                    *offset = end;
                }
            }
        }
        Ok(())
    }
    let mut updated = contents.to_vec();
    let mut offset = 0usize;
    apply(&mut updated, property, value, &range, &mut offset)?;
    Ok(updated)
}

/// 保持正文适配器对未知 TextFlow 扩展字段的保守编辑边界。
pub(super) fn reject_text_flow_extensions_in_range<T>(
    contents: &[FlowContent<T>],
    range: Range<usize>,
) -> Result<(), EditError> {
    if range.start > range.end
        || !flow_boundary(contents, range.start)?
        || !flow_boundary(contents, range.end)?
    {
        return Err(EditError::InvalidPosition);
    }
    fn visit<T>(
        contents: &[FlowContent<T>],
        range: &Range<usize>,
        offset: &mut usize,
    ) -> Result<(), EditError> {
        for content in contents {
            let end = offset
                .checked_add(content_len(content)?)
                .ok_or(EditError::InvalidPosition)?;
            if range.start < end && *offset < range.end {
                match content {
                    FlowContent::Text(text) if !text.extra.is_empty() => {
                        return Err(EditError::UnsupportedOpaqueContent);
                    }
                    FlowContent::Environment(environment) => {
                        visit(&environment.children, range, offset)?;
                    }
                    _ => {}
                }
            }
            *offset = end;
        }
        Ok(())
    }
    visit(contents, &range, &mut 0)
}

/// 返回选区是否包含 Run，以及是否有任一 Run 未启用该属性。
pub(super) fn run_property_range_state<T>(
    contents: &[FlowContent<T>],
    range: Range<usize>,
    property: RunPropertyEnvironment,
) -> Result<(bool, bool), EditError> {
    if range.start > range.end
        || !flow_boundary(contents, range.start)?
        || !flow_boundary(contents, range.end)?
    {
        return Err(EditError::InvalidPosition);
    }
    fn collect<T>(
        contents: &[FlowContent<T>],
        range: &Range<usize>,
        property: RunPropertyEnvironment,
        offset: &mut usize,
        state: &mut (bool, bool),
    ) -> Result<(), EditError> {
        for content in contents {
            let end = *offset + content_len(content)?;
            if range.start < end && *offset < range.end {
                match content {
                    FlowContent::Text(text) => {
                        let mut run_offset = *offset;
                        for run in &text.runs {
                            let run_end = run_offset + run.text.len();
                            if !run.text.is_empty()
                                && range.start < run_end
                                && run_offset < range.end
                            {
                                state.0 = true;
                                state.1 |= !run
                                    .props
                                    .get(property.key)
                                    .is_some_and(|value| (property.is_active)(Some(value)));
                            }
                            run_offset = run_end;
                        }
                    }
                    FlowContent::Environment(environment) => {
                        collect(&environment.children, range, property, offset, state)?;
                    }
                    FlowContent::SoftBreak(_) | FlowContent::Math(_) => {}
                    FlowContent::Opaque(_) => {
                        return Err(EditError::UnsupportedOpaqueContent);
                    }
                }
            }
            *offset = end;
        }
        Ok(())
    }
    let mut state = (false, false);
    collect(contents, &range, property, &mut 0, &mut state)?;
    Ok(state)
}

/// 返回格式化选区是否包含可处理文本，以及是否有内容尚未启用目标行内格式。
pub(super) fn inline_format_range_state<T>(
    contents: &[FlowContent<T>],
    range: Range<usize>,
    kind: InlineEnvironmentKind,
    inherited: &InheritedInlineStyle,
) -> Result<(bool, bool), EditError> {
    if range.start > range.end
        || !flow_boundary(contents, range.start)?
        || !flow_boundary(contents, range.end)?
    {
        return Err(EditError::InvalidPosition);
    }

    fn collect<T>(
        contents: &[FlowContent<T>],
        range: &Range<usize>,
        kind: InlineEnvironmentKind,
        inherited: &InheritedInlineStyle,
        offset: &mut usize,
        state: &mut (bool, bool),
    ) -> Result<(), EditError> {
        for content in contents {
            let end = offset
                .checked_add(content_len(content)?)
                .ok_or(EditError::InvalidPosition)?;
            if range.start < end && *offset < range.end {
                match content {
                    FlowContent::Text(text) => {
                        let text_len = text_flow_len(&text.runs);
                        let local_start = range.start.saturating_sub(*offset).min(text_len);
                        let local_end = range.end.saturating_sub(*offset).min(text_len);
                        let (contains_text, contains_disabled) = text_flow_inline_format_state(
                            text,
                            local_start..local_end,
                            kind,
                            inherited,
                        )?;
                        state.0 |= contains_text;
                        state.1 |= contains_disabled;
                    }
                    FlowContent::Environment(environment) => {
                        collect(
                            &environment.children,
                            range,
                            kind,
                            &inherited.with_environment(environment),
                            offset,
                            state,
                        )?;
                    }
                    FlowContent::SoftBreak(_) => {
                        state.0 = true;
                        state.1 |= inherited.state(kind) != Some(true);
                    }
                    FlowContent::Math(_) => {}
                    FlowContent::Opaque(_) => {
                        return Err(EditError::UnsupportedOpaqueContent);
                    }
                }
            }
            *offset = end;
        }
        Ok(())
    }

    if kind == InlineEnvironmentKind::Font {
        return Err(EditError::UnsupportedInlineFormat);
    }
    let mut state = (false, false);
    collect(contents, &range, kind, inherited, &mut 0, &mut state)?;
    Ok(state)
}

/// 为独立数学 TextFlow 提供与正文内容流相同的格式状态计算，不复制整段文本。
pub(super) fn text_flow_inline_format_state(
    text: &TextFlow,
    range: Range<usize>,
    kind: InlineEnvironmentKind,
    inherited: &InheritedInlineStyle,
) -> Result<(bool, bool), EditError> {
    if range.start > range.end
        || !text_flow_boundary(&text.runs, range.start)
        || !text_flow_boundary(&text.runs, range.end)
    {
        return Err(EditError::InvalidPosition);
    }
    if kind == InlineEnvironmentKind::Font {
        return Err(EditError::UnsupportedInlineFormat);
    }
    let mut state = (false, false);
    let mut offset = 0usize;
    for run in &text.runs {
        let end = offset
            .checked_add(run.text.len())
            .ok_or(EditError::InvalidPosition)?;
        if !run.text.is_empty() && range.start < end && offset < range.end {
            state.0 = true;
            state.1 |= inherited.state(kind) != Some(true);
        }
        offset = end;
    }
    Ok(state)
}

fn content_len<T>(content: &FlowContent<T>) -> Result<usize, EditError> {
    match content {
        FlowContent::Text(text) => Ok(text_flow_len(&text.runs)),
        FlowContent::Environment(environment) => flow_len(&environment.children),
        FlowContent::SoftBreak(_) => Ok(1),
        FlowContent::Math(_) | FlowContent::Opaque(_) => Ok(ATOM_LEN),
    }
}

fn flow_len<T>(contents: &[FlowContent<T>]) -> Result<usize, EditError> {
    contents.iter().try_fold(0usize, |total, item| {
        total
            .checked_add(content_len(item)?)
            .ok_or(EditError::InvalidPosition)
    })
}

fn flow_boundary<T>(contents: &[FlowContent<T>], offset: usize) -> Result<bool, EditError> {
    let mut consumed = 0;
    for content in contents {
        let end = consumed + content_len(content)?;
        if offset == consumed || offset == end {
            return Ok(true);
        }
        if consumed < offset && offset < end {
            return match content {
                FlowContent::Text(text) => Ok(text_flow_boundary(&text.runs, offset - consumed)),
                FlowContent::Environment(environment) => {
                    flow_boundary(&environment.children, offset - consumed)
                }
                _ => Ok(false),
            };
        }
        consumed = end;
    }
    Ok(offset == consumed)
}

fn range_needs_change<T>(
    contents: &[FlowContent<T>],
    range: &Range<usize>,
    change: InlineFormatChange<'_>,
    inherited: &InheritedInlineStyle,
    mut offset: usize,
) -> Result<bool, EditError> {
    let mut needs_change = false;
    for content in contents {
        let end = offset + content_len(content)?;
        if range.start < end && offset < range.end {
            match content {
                FlowContent::Text(_) | FlowContent::SoftBreak(_) => {
                    needs_change |= !change.matches(inherited)
                }
                FlowContent::Environment(environment) => {
                    needs_change |= range_needs_change(
                        &environment.children,
                        range,
                        change,
                        &inherited.with_environment(environment),
                        offset,
                    )?;
                }
                FlowContent::Math(_) => {}
                FlowContent::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
            }
        }
        offset = end;
    }
    Ok(needs_change)
}

type SplitContents<T> = (Vec<FlowContent<T>>, Vec<FlowContent<T>>);

fn split_contents<T: Clone>(
    contents: Vec<FlowContent<T>>,
    cut: usize,
) -> Result<SplitContents<T>, EditError> {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut offset = 0;
    for content in contents {
        let end = offset + content_len(&content)?;
        match classify_range_split(offset, end, cut) {
            RangeSplit::Left => left.push(content),
            RangeSplit::Right => match content {
                FlowContent::Text(mut text) if offset == cut && text.extra.is_empty() => {
                    let leading = text
                        .runs
                        .iter()
                        .take_while(|run| run.text.is_empty())
                        .count();
                    if leading > 0 {
                        let remaining = text.runs.split_off(leading);
                        left.push(FlowContent::Text(text));
                        right.push(FlowContent::Text(TextFlow {
                            runs: remaining,
                            extra: Default::default(),
                        }));
                    } else {
                        right.push(FlowContent::Text(text));
                    }
                }
                content => right.push(content),
            },
            RangeSplit::Inside(local) => match content {
                FlowContent::Text(text) => {
                    if !text.extra.is_empty() {
                        return Err(EditError::UnsupportedOpaqueContent);
                    }
                    let mut left_runs = Vec::new();
                    let mut right_runs = Vec::new();
                    let mut run_offset = 0;
                    for run in text.runs {
                        let run_end = run_offset + run.text.len();
                        match classify_range_split(run_offset, run_end, local) {
                            RangeSplit::Left => left_runs.push(run),
                            RangeSplit::Right => right_runs.push(run),
                            RangeSplit::Inside(run_cut) => {
                                let (before, after) = split_run_at(&run, run_cut)?;
                                left_runs.push(before);
                                right_runs.push(after);
                            }
                        }
                        run_offset = run_end;
                    }
                    left.push(FlowContent::Text(TextFlow {
                        runs: left_runs,
                        extra: Default::default(),
                    }));
                    right.push(FlowContent::Text(TextFlow {
                        runs: right_runs,
                        extra: Default::default(),
                    }));
                }
                FlowContent::Environment(mut environment) => {
                    if !environment.extra.is_empty() {
                        return Err(EditError::UnsupportedOpaqueContent);
                    }
                    let (before, after) =
                        split_contents(std::mem::take(&mut environment.children), local)?;
                    let right_environment = InlineEnvironment {
                        id: NodeId::generate(),
                        children: after,
                        ..environment.clone()
                    };
                    environment.children = before;
                    left.push(FlowContent::Environment(environment));
                    right.push(FlowContent::Environment(right_environment));
                }
                _ => return Err(EditError::InvalidPosition),
            },
        }
        offset = end;
    }
    Ok((left, right))
}

fn apply_selected<T: Clone>(
    contents: Vec<FlowContent<T>>,
    change: InlineFormatChange<'_>,
    inherited: &InheritedInlineStyle,
    depth: usize,
    max_depth: usize,
) -> Result<Vec<FlowContent<T>>, EditError> {
    let mut result = Vec::new();
    let mut pending = Vec::new();
    let flush = |pending: &mut Vec<FlowContent<T>>, result: &mut Vec<FlowContent<T>>| {
        if pending.is_empty() {
            return Ok(());
        }
        if depth >= max_depth {
            return Err(EditError::NestingLimit);
        }
        let mut environment = InlineEnvironment {
            id: NodeId::generate(),
            kind: change.kind(),
            definition_id: None,
            props: Default::default(),
            children: std::mem::take(pending),
            extra: Default::default(),
        };
        change.set(&mut environment);
        result.push(FlowContent::Environment(environment));
        Ok(())
    };
    for content in contents {
        if !has_logical_content(std::slice::from_ref(&content)) {
            flush(&mut pending, &mut result)?;
            result.push(content);
            continue;
        }
        match content {
            FlowContent::Text(_) | FlowContent::SoftBreak(_) if !change.matches(inherited) => {
                pending.push(content)
            }
            FlowContent::Text(_) | FlowContent::SoftBreak(_) => {
                flush(&mut pending, &mut result)?;
                result.push(content);
            }
            FlowContent::Environment(mut environment) => {
                flush(&mut pending, &mut result)?;
                // 覆盖放在已有祖先内侧，解除新层后仍能恢复原样式与模板优先级。
                let child_style = inherited.with_environment(&environment);
                environment.children = apply_selected(
                    environment.children,
                    change,
                    &child_style,
                    depth + 1,
                    max_depth,
                )?;
                result.push(FlowContent::Environment(environment));
            }
            FlowContent::Math(_) => {
                flush(&mut pending, &mut result)?;
                result.push(content);
            }
            FlowContent::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
        }
    }
    flush(&mut pending, &mut result)?;
    Ok(result)
}

fn has_logical_content<T>(contents: &[FlowContent<T>]) -> bool {
    contents.iter().any(|content| match content {
        FlowContent::Text(text) => text.runs.iter().any(|run| !run.text.is_empty()),
        FlowContent::Environment(environment) => has_logical_content(&environment.children),
        FlowContent::SoftBreak(_) | FlowContent::Math(_) | FlowContent::Opaque(_) => true,
    })
}

fn normalize<T>(contents: &mut Vec<FlowContent<T>>) {
    let mut result = Vec::with_capacity(contents.len());
    for mut content in std::mem::take(contents) {
        match &mut content {
            FlowContent::Text(text) if text.extra.is_empty() => {
                let mut runs = Vec::with_capacity(text.runs.len());
                for run in std::mem::take(&mut text.runs) {
                    if let Some(previous) = runs.last_mut()
                        && merge_run_pair(previous, &run)
                    {
                        continue;
                    }
                    runs.push(run);
                }
                text.runs = runs;
            }
            FlowContent::Environment(environment) if has_logical_content(&environment.children) => {
                normalize(&mut environment.children)
            }
            _ => {}
        }
        result.push(content);
        if result.len() > 1 {
            let seam = result.len() - 2;
            while merge_adjacent(&mut result, seam) {}
        }
    }
    *contents = result;
}

fn merge_adjacent<T>(contents: &mut Vec<FlowContent<T>>, index: usize) -> bool {
    let Some(next) = index.checked_add(1) else {
        return false;
    };
    let compatible = match (contents.get(index), contents.get(next)) {
        (Some(FlowContent::Text(left)), Some(FlowContent::Text(right))) => {
            left.extra.is_empty() && right.extra.is_empty()
        }
        (Some(FlowContent::Environment(left)), Some(FlowContent::Environment(right))) => {
            environments_can_merge(left, right)
        }
        _ => false,
    };
    if !compatible {
        return false;
    }
    let right = contents.remove(next);
    match (&mut contents[index], right) {
        (FlowContent::Text(left), FlowContent::Text(mut right)) => {
            let seam = left.runs.len();
            left.runs.append(&mut right.runs);
            if seam > 0 && seam < left.runs.len() {
                let right = left.runs[seam].clone();
                if merge_run_pair(&mut left.runs[seam - 1], &right) {
                    left.runs.remove(seam);
                }
            }
        }
        (FlowContent::Environment(left), FlowContent::Environment(mut right)) => {
            let seam = left.children.len();
            left.children.append(&mut right.children);
            if seam > 0 {
                merge_adjacent(&mut left.children, seam - 1);
            }
        }
        _ => unreachable!("merge compatibility was checked before removing a node"),
    }
    true
}

fn environments_can_merge<T>(
    left: &InlineEnvironment<FlowContent<T>>,
    right: &InlineEnvironment<FlowContent<T>>,
) -> bool {
    left.kind == right.kind
        && left.definition_id == right.definition_id
        && left.props == right.props
        && left.extra.is_empty()
        && right.extra.is_empty()
        && has_logical_content(&left.children)
        && has_logical_content(&right.children)
}

/// 返回 TextFlow 局部偏移在共同逻辑流中的位置；环境路径只含物理 content 下标。
pub(super) fn text_flow_offset<T>(
    contents: &[FlowContent<T>],
    path: &[usize],
    local: usize,
) -> Result<usize, EditError> {
    let (index, rest) = path.split_first().ok_or(EditError::InvalidPosition)?;
    let mut offset = 0usize;
    for content in contents.get(..*index).ok_or(EditError::InvalidPosition)? {
        offset = offset
            .checked_add(content_len(content)?)
            .ok_or(EditError::InvalidPosition)?;
    }
    let target = contents.get(*index).ok_or(EditError::InvalidPosition)?;
    let local = match target {
        FlowContent::Text(text) if rest.is_empty() && text_flow_boundary(&text.runs, local) => {
            local
        }
        FlowContent::Environment(environment) => {
            text_flow_offset(&environment.children, rest, local)?
        }
        _ => return Err(EditError::InvalidPosition),
    };
    offset.checked_add(local).ok_or(EditError::InvalidPosition)
}

/// 在格式转换后的流中重建 TextFlow 端点；逻辑字节位置和 affinity 不变。
pub(super) fn text_position_at_offset<T>(
    contents: &[FlowContent<T>],
    offset: usize,
    affinity: Affinity,
) -> Result<(Vec<usize>, usize), EditError> {
    fn collect<T>(
        contents: &[FlowContent<T>],
        path: &mut Vec<usize>,
        offset: &mut usize,
        output: &mut Vec<(Vec<usize>, Range<usize>)>,
    ) -> Result<(), EditError> {
        for (index, content) in contents.iter().enumerate() {
            path.push(index);
            match content {
                FlowContent::Text(text) => {
                    let start = *offset;
                    *offset += text_flow_len(&text.runs);
                    output.push((path.clone(), start..*offset));
                }
                FlowContent::Environment(environment) => {
                    collect(&environment.children, path, offset, output)?
                }
                _ => *offset += content_len(content)?,
            }
            path.pop();
        }
        Ok(())
    }
    let mut positions = Vec::new();
    collect(contents, &mut Vec::new(), &mut 0, &mut positions)?;
    let chosen = match affinity {
        Affinity::Downstream => positions
            .iter()
            .find(|(_, range)| range.start <= offset && offset < range.end)
            .or_else(|| {
                positions
                    .iter()
                    .find(|(_, range)| range.is_empty() && range.start == offset)
            })
            .or_else(|| {
                positions
                    .iter()
                    .rev()
                    .find(|(_, range)| range.end == offset)
            }),
        Affinity::Upstream => positions
            .iter()
            .rev()
            .find(|(_, range)| range.start < offset && offset <= range.end)
            .or_else(|| {
                positions
                    .iter()
                    .find(|(_, range)| range.is_empty() && range.start == offset)
            })
            .or_else(|| positions.iter().find(|(_, range)| range.start == offset)),
    }
    .ok_or(EditError::InvalidPosition)?;
    Ok((chosen.0.clone(), offset - chosen.1.start))
}

#[cfg(test)]
mod tests;
