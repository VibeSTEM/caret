//! 定位数学树内 TextFlow 的稳定编辑路径。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{MathContent, MathFlow, MathTree, TextFlow};
use caret_math::{MathContentSourceMap, MathSourceMap, MathTextSourceMap, MathTreeSourceMap};
use std::mem::size_of;
use std::ops::Range;

/// 数学树结构路径步骤由文档模型统一定义，避免编辑器和源码 sidecar 各持一份。
pub use caret_doc::MathPathStep as MathTextFlowPathStep;

/// 定位公式树或 Align cell 内一个带源范围的 TextFlow。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathTextFlowPath {
    steps: Vec<MathTextFlowPathStep>,
    source_range: (u32, u32),
    host_source_range: (usize, usize),
    align_cell: Option<(usize, usize)>,
}

impl MathTextFlowPath {
    pub(super) fn new(
        steps: Vec<MathTextFlowPathStep>,
        source_range: (u32, u32),
        align_cell: Option<(usize, usize)>,
    ) -> Self {
        Self {
            steps,
            source_range,
            host_source_range: (source_range.0 as usize, source_range.1 as usize),
            align_cell,
        }
    }

    /// 返回从公式根流开始的类型化路径。
    pub fn steps(&self) -> &[MathTextFlowPathStep] {
        &self.steps
    }

    /// 返回用于校验路径仍指向原文本流的源码字节区间。
    pub fn source_range(&self) -> (u32, u32) {
        self.source_range
    }

    /// 返回相对 MathEdit.source 的范围；Align cell 路径会转换为宿主源码偏移。
    pub(super) fn host_source_range(&self) -> (usize, usize) {
        self.host_source_range
    }

    pub(super) fn host_offset(&self, local_offset: usize) -> Option<usize> {
        let base = self
            .host_source_range
            .0
            .checked_sub(self.source_range.0 as usize)?;
        base.checked_add(local_offset)
    }

    pub(super) fn source_offset(&self, host_offset: usize) -> Option<usize> {
        let base = self
            .host_source_range
            .0
            .checked_sub(self.source_range.0 as usize)?;
        host_offset.checked_sub(base)
    }

    pub(super) fn with_host_source_base(mut self, base: usize) -> Option<Self> {
        self.host_source_range = (
            base.checked_add(self.source_range.0 as usize)?,
            base.checked_add(self.source_range.1 as usize)?,
        );
        Some(self)
    }

    /// 返回所属 Align 的行和 cell；独立数学树不含该归属。
    pub fn align_cell(&self) -> Option<(usize, usize)> {
        self.align_cell
    }

    pub(in crate::editing::session) fn estimated_bytes(&self) -> usize {
        size_of::<Self>() + self.steps.capacity() * size_of::<MathTextFlowPathStep>()
    }

    pub(super) fn resolve<'a>(&self, tree: &'a MathTree) -> Option<&'a TextFlow> {
        let text = self.resolve_structurally(tree)?;
        (text
            .source_range
            .as_ref()
            .map(|range| (range.start, range.end))
            == Some(self.source_range))
        .then_some(text)
    }

    /// 以当前源码投影中的 sidecar 校验路径，并返回对应 TextFlow 映射。
    pub(super) fn resolve_in_projection<'a>(
        &self,
        tree: &'a MathTree,
        projection: &'a MathSourceMap<'_>,
    ) -> Option<(&'a TextFlow, &'a MathTextSourceMap)> {
        let source_map = self.validate_projection(projection)?;
        let text = self.resolve(tree)?;
        source_map_matches_text(text, source_map, self.source_map_base(projection)?)
            .then_some((text, source_map))
    }

    pub(super) fn resolve_mut_in_projection<'a>(
        &self,
        tree: &'a mut MathTree,
        projection: &MathSourceMap<'_>,
    ) -> Option<&'a mut TextFlow> {
        let source_map = self.validate_projection(projection)?;
        let text = self.resolve_mut(tree)?;
        source_map_matches_text(text, source_map, self.source_map_base(projection)?).then_some(text)
    }

    fn validate_projection<'a>(
        &self,
        projection: &'a MathSourceMap<'_>,
    ) -> Option<&'a MathTextSourceMap> {
        let source_map = self.text_source_map_in_projection(projection)?;
        let edit_base = projection.edit_range().start;
        let expected = edit_base.checked_add(self.host_source_range.0)?
            ..edit_base.checked_add(self.host_source_range.1)?;
        (source_map.range()? == expected).then_some(source_map)
    }

    pub(super) fn text_source_map_in_projection<'a>(
        &self,
        projection: &'a MathSourceMap<'_>,
    ) -> Option<&'a MathTextSourceMap> {
        let tree_map = match self.align_cell {
            Some((row, cell)) => projection.align()?.cell_tree(row, cell)?,
            None => projection.tree()?,
        };
        tree_map.content_at(&self.steps)?.text()
    }

    fn source_map_base(&self, projection: &MathSourceMap<'_>) -> Option<usize> {
        match self.align_cell {
            Some((row, cell)) => projection
                .align()?
                .cell_range(row, cell)
                .map(|range| range.start),
            None => Some(0),
        }
    }

    /// 按类型化树路径解析 TextFlow，不要求临时源码范围仍与旧投影一致。
    pub(super) fn resolve_structurally<'a>(&self, tree: &'a MathTree) -> Option<&'a TextFlow> {
        let MathContent::Text(text) = resolve_content_path(&tree.root, &self.steps)? else {
            return None;
        };
        Some(text)
    }

    /// 更新解析器重建临时范围后的映射，同时保留稳定的结构路径和宿主偏移基址。
    pub(super) fn rebind_ranges(&self, tree: &MathTree, host_base: usize) -> Option<Self> {
        let MathContent::Text(text) = resolve_content_path(&tree.root, &self.steps)? else {
            return None;
        };
        let range = text.source_range.as_ref()?;
        let host_start = host_base.checked_add(range.start as usize)?;
        let host_end = host_base.checked_add(range.end as usize)?;
        Some(
            Self::new(
                self.steps.clone(),
                (range.start, range.end),
                self.align_cell,
            )
            .with_host_source_range((host_start, host_end)),
        )
    }

    pub(super) fn resolve_mut<'a>(&self, tree: &'a mut MathTree) -> Option<&'a mut TextFlow> {
        let text = self.resolve_structurally_mut(tree)?;
        (text
            .source_range
            .as_ref()
            .map(|range| (range.start, range.end))
            == Some(self.source_range))
        .then_some(text)
    }

    pub(super) fn resolve_structurally_mut<'a>(
        &self,
        tree: &'a mut MathTree,
    ) -> Option<&'a mut TextFlow> {
        let content = resolve_content_path_mut(&mut tree.root, &self.steps)?;
        let MathContent::Text(text) = content else {
            return None;
        };
        Some(text)
    }

    /// 依照同一结构路径取得新解析树中的对应文本流及其新源码范围。
    pub(super) fn rebase(
        &self,
        tree: &MathTree,
        old_edit: Range<usize>,
        new_edit: Range<usize>,
        host_old_edit: Range<usize>,
        host_new_edit: Range<usize>,
    ) -> Option<Self> {
        let MathContent::Text(text) = resolve_content_path(&tree.root, &self.steps)? else {
            return None;
        };
        let range = text.source_range.as_ref()?;
        let (old_start, old_end) = (self.source_range.0 as usize, self.source_range.1 as usize);
        let delta = new_edit.end as isize - old_edit.end as isize;
        let expected = if old_start <= old_edit.start && old_end >= old_edit.end {
            (old_start, old_end.checked_add_signed(delta)?)
        } else if old_end < old_edit.start {
            (old_start, old_end)
        } else if old_start > old_edit.end {
            (
                old_start.checked_add_signed(delta)?,
                old_end.checked_add_signed(delta)?,
            )
        } else {
            return None;
        };
        if (range.start as usize, range.end as usize) != expected {
            return None;
        }
        let host_range = rebase_span(self.host_source_range, host_old_edit, host_new_edit)?;
        Some(
            Self::new(
                self.steps.clone(),
                (range.start, range.end),
                self.align_cell,
            )
            .with_host_source_range(host_range),
        )
    }

    fn with_host_source_range(mut self, range: (usize, usize)) -> Self {
        self.host_source_range = range;
        self
    }
}

fn source_map_matches_text(
    text: &TextFlow,
    source_map: &MathTextSourceMap,
    source_base: usize,
) -> bool {
    let mut tree_segments = Vec::with_capacity(text.source_map.len());
    for entry in &text.source_map {
        let display = entry.text_start as usize..entry.text_end as usize;
        let Some(start) = source_base.checked_add(entry.source_range.start as usize) else {
            return false;
        };
        let Some(end) = source_base.checked_add(entry.source_range.end as usize) else {
            return false;
        };
        let source = start..end;
        tree_segments.push((display, source));
    }

    let mut projection_segments = Vec::with_capacity(source_map.segments().len());
    for segment in source_map.segments() {
        let (Some(source), Some(display)) = (segment.source(), Some(segment.display())) else {
            return false;
        };
        projection_segments.push((display, source));
    }

    normalized_linear_segments(tree_segments) == normalized_linear_segments(projection_segments)
}

fn normalized_linear_segments(
    segments: Vec<(Range<usize>, Range<usize>)>,
) -> Vec<(Range<usize>, Range<usize>)> {
    let mut normalized: Vec<(Range<usize>, Range<usize>)> = Vec::with_capacity(segments.len());
    for (display, source) in segments {
        if display.start != display.end
            && source.start != source.end
            && display.len() == source.len()
            && let Some((previous_display, previous_source)) = normalized.last_mut()
            && previous_display.start != previous_display.end
            && previous_source.start != previous_source.end
            && previous_display.end == display.start
            && previous_source.end == source.start
            && previous_display.len() == previous_source.len()
        {
            previous_display.end = display.end;
            previous_source.end = source.end;
        } else {
            normalized.push((display, source));
        }
    }
    normalized
}

fn rebase_span(
    range: (usize, usize),
    old_edit: Range<usize>,
    new_edit: Range<usize>,
) -> Option<(usize, usize)> {
    let (start, end) = range;
    let delta = new_edit.end as isize - old_edit.end as isize;
    if start <= old_edit.start && end >= old_edit.end {
        Some((start, end.checked_add_signed(delta)?))
    } else if end < old_edit.start {
        Some((start, end))
    } else if start > old_edit.end {
        Some((
            start.checked_add_signed(delta)?,
            end.checked_add_signed(delta)?,
        ))
    } else {
        None
    }
}

pub(super) fn path_at_source_boundary(
    source_map: &MathTreeSourceMap,
    source_offset: usize,
    forward: bool,
    align_cell: Option<(usize, usize)>,
    source_base: usize,
) -> Option<MathTextFlowPath> {
    fn visit(
        contents: &[MathContentSourceMap],
        environment_children: bool,
        path: &mut Vec<MathTextFlowPathStep>,
        source_offset: usize,
        forward: bool,
        align_cell: Option<(usize, usize)>,
        source_base: usize,
    ) -> Option<MathTextFlowPath> {
        for (index, content) in contents.iter().enumerate() {
            path.push(if environment_children {
                MathTextFlowPathStep::EnvironmentChild(index)
            } else {
                MathTextFlowPathStep::Content(index)
            });
            let found = match content {
                MathContentSourceMap::Text(text) => text.range().and_then(|range| {
                    let boundary = if forward { range.start } else { range.end };
                    let start = range.start.checked_sub(source_base)?;
                    let end = range.end.checked_sub(source_base)?;
                    let start = u32::try_from(start).ok()?;
                    let end = u32::try_from(end).ok()?;
                    (boundary == source_offset)
                        .then(|| MathTextFlowPath::new(path.clone(), (start, end), align_cell))
                }),
                MathContentSourceMap::Math { operands, .. } => {
                    let mut found = None;
                    for (operand, flow) in operands.iter().enumerate() {
                        path.push(MathTextFlowPathStep::Operand(operand));
                        found = visit(
                            flow.content(),
                            false,
                            path,
                            source_offset,
                            forward,
                            align_cell,
                            source_base,
                        );
                        path.pop();
                        if found.is_some() {
                            break;
                        }
                    }
                    found
                }
                MathContentSourceMap::Environment { children, .. } => visit(
                    children,
                    true,
                    path,
                    source_offset,
                    forward,
                    align_cell,
                    source_base,
                ),
                MathContentSourceMap::Other => None,
            };
            path.pop();
            if found.is_some() {
                return found;
            }
        }
        None
    }

    visit(
        source_map.root().content(),
        false,
        &mut Vec::new(),
        source_offset,
        forward,
        align_cell,
        source_base,
    )
}

/// 查找同一 MathFlow 槽位中相邻的文本岛；环境包装透明，数学节点与软换行形成边界。
pub(super) fn adjacent_text_flow_path(
    tree: &MathTree,
    current: &MathTextFlowPath,
    forward: bool,
) -> Option<MathTextFlowPath> {
    current.resolve(tree)?;
    let flow_prefix_end = current
        .steps
        .iter()
        .rposition(|step| matches!(step, MathTextFlowPathStep::Operand(_)))
        .map_or(0, |index| index + 1);
    let flow_prefix = &current.steps[..flow_prefix_end];
    let flow = resolve_flow_path(&tree.root, flow_prefix)?;
    let mut candidates = Vec::new();
    collect_text_island_candidates(
        &flow.content,
        false,
        &mut flow_prefix.to_vec(),
        current.align_cell,
        &mut candidates,
    );
    let index = candidates.iter().position(|candidate| {
        candidate
            .as_ref()
            .is_some_and(|candidate| candidate.steps == current.steps)
    })?;
    let adjacent = if forward {
        candidates.get(index + 1)?
    } else {
        candidates.get(index.checked_sub(1)?)?
    };
    let adjacent = adjacent.clone()?;
    let host_base = current
        .host_source_range
        .0
        .checked_sub(current.source_range.0 as usize)?;
    adjacent.with_host_source_base(host_base)
}

fn collect_text_island_candidates(
    contents: &[MathContent],
    environment_children: bool,
    path: &mut Vec<MathTextFlowPathStep>,
    align_cell: Option<(usize, usize)>,
    candidates: &mut Vec<Option<MathTextFlowPath>>,
) {
    for (index, content) in contents.iter().enumerate() {
        path.push(if environment_children {
            MathTextFlowPathStep::EnvironmentChild(index)
        } else {
            MathTextFlowPathStep::Content(index)
        });
        match content {
            MathContent::Text(text) => {
                candidates.push(text.source_range.as_ref().map(|range| {
                    MathTextFlowPath::new(path.clone(), (range.start, range.end), align_cell)
                }));
            }
            MathContent::Environment(environment) => collect_text_island_candidates(
                &environment.children,
                true,
                path,
                align_cell,
                candidates,
            ),
            MathContent::Math(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => {
                candidates.push(None);
            }
        }
        path.pop();
    }
}

enum PathTarget<'a> {
    Flow(&'a MathFlow),
    Content(&'a MathContent),
}

enum PathTargetMut<'a> {
    Flow(&'a mut MathFlow),
    Content(&'a mut MathContent),
}

fn resolve_content_path<'a>(
    root: &'a MathFlow,
    steps: &[MathTextFlowPathStep],
) -> Option<&'a MathContent> {
    match resolve_path_target(root, steps)? {
        PathTarget::Content(content) => Some(content),
        PathTarget::Flow(_) => None,
    }
}

fn resolve_flow_path<'a>(
    root: &'a MathFlow,
    steps: &[MathTextFlowPathStep],
) -> Option<&'a MathFlow> {
    match resolve_path_target(root, steps)? {
        PathTarget::Flow(flow) => Some(flow),
        PathTarget::Content(_) => None,
    }
}

fn resolve_path_target<'a>(
    root: &'a MathFlow,
    steps: &[MathTextFlowPathStep],
) -> Option<PathTarget<'a>> {
    let mut target = PathTarget::Flow(root);
    for step in steps {
        target = match (target, step) {
            (PathTarget::Flow(flow), MathTextFlowPathStep::Content(index)) => {
                PathTarget::Content(flow.content.get(*index)?)
            }
            (
                PathTarget::Content(MathContent::Environment(environment)),
                MathTextFlowPathStep::EnvironmentChild(index),
            ) => PathTarget::Content(environment.children.get(*index)?),
            (
                PathTarget::Content(MathContent::Math(node)),
                MathTextFlowPathStep::Operand(index),
            ) => PathTarget::Flow(node.children.get(*index)?),
            _ => return None,
        };
    }
    Some(target)
}

fn resolve_content_path_mut<'a>(
    root: &'a mut MathFlow,
    steps: &[MathTextFlowPathStep],
) -> Option<&'a mut MathContent> {
    let mut target = PathTargetMut::Flow(root);
    for step in steps {
        target = match (target, step) {
            (PathTargetMut::Flow(flow), MathTextFlowPathStep::Content(index)) => {
                PathTargetMut::Content(flow.content.get_mut(*index)?)
            }
            (
                PathTargetMut::Content(MathContent::Environment(environment)),
                MathTextFlowPathStep::EnvironmentChild(index),
            ) => PathTargetMut::Content(environment.children.get_mut(*index)?),
            (
                PathTargetMut::Content(MathContent::Math(node)),
                MathTextFlowPathStep::Operand(index),
            ) => PathTargetMut::Flow(node.children.get_mut(*index)?),
            _ => return None,
        };
    }
    match target {
        PathTargetMut::Content(content) => Some(content),
        PathTargetMut::Flow(_) => None,
    }
}

/// 返回路径末端内容所在的宿主列表与槽位，用于以同级节点替换该内容。
pub(super) fn content_slot_mut<'a>(
    flow: &'a mut MathFlow,
    steps: &[MathTextFlowPathStep],
) -> Option<(&'a mut Vec<MathContent>, usize)> {
    let MathTextFlowPathStep::Content(index) = steps.first()? else {
        return None;
    };
    if steps.len() == 1 {
        return Some((&mut flow.content, *index));
    }
    content_slot_mut_content(flow.content.get_mut(*index)?, &steps[1..])
}

fn content_slot_mut_content<'a>(
    content: &'a mut MathContent,
    steps: &[MathTextFlowPathStep],
) -> Option<(&'a mut Vec<MathContent>, usize)> {
    match content {
        MathContent::Environment(environment) => {
            let MathTextFlowPathStep::EnvironmentChild(index) = steps.first()? else {
                return None;
            };
            if steps.len() == 1 {
                return Some((&mut environment.children, *index));
            }
            content_slot_mut_content(environment.children.get_mut(*index)?, &steps[1..])
        }
        MathContent::Math(node) => {
            let MathTextFlowPathStep::Operand(index) = steps.first()? else {
                return None;
            };
            content_slot_mut(node.children.get_mut(*index)?, &steps[1..])
        }
        MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => None,
    }
}

#[cfg(test)]
mod tests;
