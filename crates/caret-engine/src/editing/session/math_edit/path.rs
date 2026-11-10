//! 定位数学树内 TextFlow 的稳定编辑路径。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::grapheme::GraphemeMap;
use super::MathPositionAffinity;
use caret_doc::{MathContent, MathFlow, MathTree, TextFlow};
use caret_math::{MathContentSourceMap, MathSourceMap, MathTextSourceMap, MathTreeSourceMap};
use std::mem::size_of;
use std::ops::Range;

/// 数学树结构路径步骤由文档模型统一定义，避免编辑器和源码 sidecar 各持一份。
pub use caret_doc::MathPathStep as MathTextFlowPathStep;

/// 通过结构步骤定位公式树或 Align cell 内的 TextFlow。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathTextFlowPath {
    steps: Vec<MathTextFlowPathStep>,
    align_cell: Option<(usize, usize)>,
}

impl MathTextFlowPath {
    pub(super) fn new(
        steps: Vec<MathTextFlowPathStep>,
        align_cell: Option<(usize, usize)>,
    ) -> Self {
        Self { steps, align_cell }
    }

    /// 将同代 MathLayout 的稳定结构路径转成可由 Session 校验的文本路径。
    pub(crate) fn from_layout_path(
        steps: Vec<MathTextFlowPathStep>,
        align_cell: Option<(usize, usize)>,
    ) -> Self {
        Self { steps, align_cell }
    }

    /// 从界面提交的类型化路径构造候选；Session 在修改前会重新校验路径和文本边界。
    pub fn from_interaction_path(
        steps: Vec<MathTextFlowPathStep>,
        align_cell: Option<(usize, usize)>,
    ) -> Self {
        Self { steps, align_cell }
    }

    pub(super) fn with_structural_steps(&self, steps: Vec<MathTextFlowPathStep>) -> Self {
        Self::new(steps, self.align_cell)
    }

    /// 返回从公式根流开始的类型化路径。
    pub fn steps(&self) -> &[MathTextFlowPathStep] {
        &self.steps
    }

    /// 返回所属 Align 的行和 cell；独立数学树不含该归属。
    pub fn align_cell(&self) -> Option<(usize, usize)> {
        self.align_cell
    }

    pub(in crate::editing::session) fn estimated_bytes(&self) -> usize {
        size_of::<Self>() + self.steps.capacity() * size_of::<MathTextFlowPathStep>()
    }

    pub(super) fn resolve<'a>(&self, tree: &'a MathTree) -> Option<&'a TextFlow> {
        self.resolve_structurally(tree)
    }

    /// 以当前源码投影中的 sidecar 校验路径，并返回对应 TextFlow 映射。
    pub(super) fn resolve_in_projection<'a>(
        &self,
        tree: &'a MathTree,
        projection: &'a MathSourceMap<'_>,
    ) -> Option<(&'a TextFlow, &'a MathTextSourceMap)> {
        let source_map = self.text_source_map_in_projection(projection)?;
        let text = self.resolve_structurally(tree)?;
        source_map_covers_text(text, source_map).then_some((text, source_map))
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

    /// 按类型化树路径解析 TextFlow，不要求临时源码范围仍与旧投影一致。
    pub(super) fn resolve_structurally<'a>(&self, tree: &'a MathTree) -> Option<&'a TextFlow> {
        let MathContent::Text(text) = resolve_content_path(&tree.root, &self.steps)? else {
            return None;
        };
        Some(text)
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
}

/// 一个可编辑文本岛内、由具体 TextFlow 承载的端点。
///
/// `offset` 是所属 TextFlow 展平后的 UTF-8 字节偏移；`affinity` 在相邻 TextFlow
/// 的共享边界决定端点归属上一个还是下一个流。两端可以拥有不同路径，但必须位于
/// 同一个连续文本岛中。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathTextPosition {
    pub path: MathTextFlowPath,
    pub offset: usize,
    pub affinity: MathPositionAffinity,
}

impl MathTextPosition {
    pub fn new(path: MathTextFlowPath, offset: usize, affinity: MathPositionAffinity) -> Self {
        Self {
            path,
            offset,
            affinity,
        }
    }

    /// 比较同一 MathTree TextFlow 中的合法字素边界。
    ///
    /// 不同 TextFlow、MathTree、宿主或文档代际需要由外层 adapter 解析，不能直接比较路径或偏移。
    pub fn compare_in_same_flow(
        &self,
        other: &Self,
        tree: &MathTree,
    ) -> Option<std::cmp::Ordering> {
        if self.path != other.path {
            return None;
        }
        let island = contiguous_text_flow_text(tree, &self.path)?;
        let self_offset = island.global_offset(self)?;
        let other_offset = island.global_offset(other)?;
        if !GraphemeMap::is_boundary_in(island.display(), self_offset)
            || !GraphemeMap::is_boundary_in(island.display(), other_offset)
        {
            return None;
        }
        Some(
            self_offset
                .cmp(&other_offset)
                .then_with(|| match (self.affinity, other.affinity) {
                    (MathPositionAffinity::Upstream, MathPositionAffinity::Downstream) => {
                        std::cmp::Ordering::Less
                    }
                    (MathPositionAffinity::Downstream, MathPositionAffinity::Upstream) => {
                        std::cmp::Ordering::Greater
                    }
                    _ => std::cmp::Ordering::Equal,
                }),
        )
    }
}

fn source_map_covers_text(text: &TextFlow, source_map: &MathTextSourceMap) -> bool {
    let Some(display_len) = text_flow_display_len(text) else {
        return false;
    };
    if display_len == 0 && source_map.segments().is_empty() {
        return source_map
            .range()
            .is_some_and(|range| range.start == range.end);
    }
    let Some(source_range) = source_map.range() else {
        return false;
    };
    let mut covered_until = 0;
    let mut run_index = 0;
    let mut run_start = 0;
    let mut source_until = source_range.start;
    for segment in source_map.segments() {
        let display = segment.display();
        if display.start > display.end
            || display.start != covered_until
            || display.end > display_len
            || !text_flow_is_boundary(text, display.start, &mut run_index, &mut run_start)
            || !text_flow_is_boundary(text, display.end, &mut run_index, &mut run_start)
        {
            return false;
        }
        let Some(source) = segment.source() else {
            return false;
        };
        if source.start > source.end
            || source.start < source_until
            || source.start < source_range.start
            || source.end > source_range.end
        {
            return false;
        }
        source_until = source.end;
        covered_until = display.end;
    }
    covered_until == display_len
}

fn text_flow_display_len(text: &TextFlow) -> Option<usize> {
    text.runs
        .iter()
        .try_fold(0usize, |length, run| length.checked_add(run.text.len()))
}

fn text_flow_is_boundary(
    text: &TextFlow,
    offset: usize,
    run_index: &mut usize,
    run_start: &mut usize,
) -> bool {
    while let Some(run) = text.runs.get(*run_index) {
        let Some(end) = run_start.checked_add(run.text.len()) else {
            return false;
        };
        if offset <= end {
            return offset >= *run_start && run.text.is_char_boundary(offset - *run_start);
        }
        *run_start = end;
        *run_index += 1;
    }
    offset == *run_start
}

pub(super) fn path_at_source_boundary(
    source_map: &MathTreeSourceMap,
    source_offset: usize,
    forward: bool,
    align_cell: Option<(usize, usize)>,
) -> Option<MathTextFlowPath> {
    fn visit(
        contents: &[MathContentSourceMap],
        environment_children: bool,
        path: &mut Vec<MathTextFlowPathStep>,
        source_offset: usize,
        forward: bool,
        align_cell: Option<(usize, usize)>,
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
                    (boundary == source_offset)
                        .then(|| MathTextFlowPath::new(path.clone(), align_cell))
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
                        );
                        path.pop();
                        if found.is_some() {
                            break;
                        }
                    }
                    found
                }
                MathContentSourceMap::Environment { children, .. } => {
                    visit(children, true, path, source_offset, forward, align_cell)
                }
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
    )
}

/// 返回相邻 TextFlow 的结构路径；源码偏移由宿主映射适配器单独绑定。
pub(super) fn adjacent_text_flow_path(
    tree: &MathTree,
    current: &MathTextFlowPath,
    forward: bool,
) -> Option<MathTextFlowPath> {
    current.resolve_structurally(tree)?;
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
    Some(adjacent)
}

/// 返回当前槽位中由 InlineEnvironment 包装连接的连续文本流及其边界。
pub(super) fn contiguous_text_flow_paths(
    tree: &MathTree,
    current: &MathTextFlowPath,
) -> Option<Vec<MathTextFlowPath>> {
    current.resolve_structurally(tree)?;
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
    let current_index = candidates.iter().position(|candidate| {
        candidate
            .as_ref()
            .is_some_and(|candidate| candidate.steps == current.steps)
    })?;
    let start = candidates[..current_index]
        .iter()
        .rposition(Option::is_none)
        .map_or(0, |index| index + 1);
    let end = candidates[current_index + 1..]
        .iter()
        .position(Option::is_none)
        .map_or(candidates.len(), |index| current_index + 1 + index);
    candidates[start..end]
        .iter()
        .cloned()
        .collect::<Option<Vec<_>>>()
}

/// 同一文本岛按结构顺序拼接后的显示文本和各流 UTF-8 范围。
pub struct TextIslandSnapshot {
    pub(super) paths: Vec<MathTextFlowPath>,
    pub(super) ranges: Vec<Range<usize>>,
    pub(super) display: String,
    pub(super) current_index: usize,
}

impl TextIslandSnapshot {
    /// 返回同一连续文本岛的拼接显示文本。
    pub fn display(&self) -> &str {
        &self.display
    }

    /// 将具体 TextFlow 端点转换为文本岛拼接后的 UTF-8 字节偏移。
    pub(super) fn global_offset(&self, position: &MathTextPosition) -> Option<usize> {
        let index = self.paths.iter().position(|path| path == &position.path)?;
        let range = self.ranges.get(index)?;
        let text_len = range.end.checked_sub(range.start)?;
        (position.offset <= text_len)
            .then(|| range.start.checked_add(position.offset))
            .flatten()
    }

    /// 将文本岛拼接后的 UTF-8 字节偏移反向映射为具体 TextFlow 端点。
    pub fn position_at_offset(
        &self,
        offset: usize,
        affinity: MathPositionAffinity,
    ) -> Option<MathTextPosition> {
        let downstream = affinity == MathPositionAffinity::Downstream;
        let (path, local_offset) = self.path_at_offset(offset, downstream)?;
        Some(MathTextPosition::new(path, local_offset, affinity))
    }

    /// 将拼接文本中的有效 caret 偏移映射回其宿主 TextFlow。
    pub(super) fn path_at_offset(
        &self,
        offset: usize,
        downstream: bool,
    ) -> Option<(MathTextFlowPath, usize)> {
        if offset > self.display.len() || !self.display.is_char_boundary(offset) {
            return None;
        }
        let index = if downstream {
            self.ranges
                .iter()
                .position(|range| range.start <= offset && offset < range.end)
                .or_else(|| {
                    self.ranges
                        .iter()
                        .enumerate()
                        .find(|(_, range)| range.is_empty() && range.start == offset)
                        .map(|(index, _)| index)
                })
                .or_else(|| self.ranges.iter().rposition(|range| range.end == offset))
        } else {
            self.ranges
                .iter()
                .rposition(|range| range.start < offset && offset <= range.end)
                .or_else(|| {
                    self.ranges
                        .iter()
                        .enumerate()
                        .find(|(_, range)| range.is_empty() && range.start == offset)
                        .map(|(index, _)| index)
                })
                .or_else(|| self.ranges.iter().position(|range| range.start == offset))
        }?;
        let range = self.ranges.get(index)?;
        Some((
            self.paths.get(index)?.clone(),
            offset.checked_sub(range.start)?,
        ))
    }
}

/// 汇总同一文本岛中的 TextFlow，所有范围使用拼接后显示文本的 UTF-8 字节坐标。
pub(super) fn contiguous_text_flow_text(
    tree: &MathTree,
    current: &MathTextFlowPath,
) -> Option<TextIslandSnapshot> {
    let paths = contiguous_text_flow_paths(tree, current)?;
    let current_index = paths
        .iter()
        .position(|path| path.steps() == current.steps())?;
    let mut display = String::new();
    let mut ranges = Vec::with_capacity(paths.len());
    for path in &paths {
        let text = path.resolve_structurally(tree)?;
        let start = display.len();
        for run in &text.runs {
            display.push_str(&run.text);
        }
        ranges.push(start..display.len());
    }
    Some(TextIslandSnapshot {
        paths,
        ranges,
        display,
        current_index,
    })
}

/// 返回没有数学原子/换行/未知节点的根 MathFlow 首末 TextFlow 入口。
///
/// 结构导航只依赖树路径；源码位置属于显式源码编辑的临时投影，不应阻止普通文本流进入树编辑。
pub(super) fn root_text_flow_path(
    tree: &MathTree,
    forward: bool,
    align_cell: Option<(usize, usize)>,
) -> Option<MathTextFlowPath> {
    let mut candidates = Vec::new();
    collect_text_island_candidates(
        &tree.root.content,
        false,
        &mut Vec::new(),
        align_cell,
        &mut candidates,
    );
    if candidates.is_empty() || candidates.iter().any(Option::is_none) {
        return None;
    }
    let candidate = if forward {
        candidates.first()?
    } else {
        candidates.last()?
    };
    candidate.clone()
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
            MathContent::Text(_) => {
                candidates.push(Some(MathTextFlowPath::new(path.clone(), align_cell)))
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

pub(super) fn resolve_flow_path<'a>(
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

pub(super) fn resolve_content_path_mut<'a>(
    root: &'a mut MathFlow,
    steps: &[MathTextFlowPathStep],
) -> Option<&'a mut MathContent> {
    match resolve_path_target_mut(root, steps)? {
        PathTargetMut::Content(content) => Some(content),
        PathTargetMut::Flow(_) => None,
    }
}

pub(super) fn resolve_flow_path_mut<'a>(
    root: &'a mut MathFlow,
    steps: &[MathTextFlowPathStep],
) -> Option<&'a mut MathFlow> {
    match resolve_path_target_mut(root, steps)? {
        PathTargetMut::Flow(flow) => Some(flow),
        PathTargetMut::Content(_) => None,
    }
}

fn resolve_path_target_mut<'a>(
    root: &'a mut MathFlow,
    steps: &[MathTextFlowPathStep],
) -> Option<PathTargetMut<'a>> {
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
    Some(target)
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
