//! 保存在解析公式树之外的结构化 LaTeX 源码范围映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::{collections::BTreeMap, mem::size_of, ops::Range};

use caret_doc::{MathContent, MathFlow, MathNodeKind, MathPathStep, MathTree};
use serde_json::Value;

/// 一个可选源码范围及其无法由解析器解释的扩展元数据。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MathRangeSourceMap {
    pub(super) range: Option<Range<usize>>,
    pub(super) extra: BTreeMap<String, Value>,
}

impl MathRangeSourceMap {
    pub(crate) fn from_parser_range(
        range: Option<Range<usize>>,
        extra: BTreeMap<String, Value>,
    ) -> Self {
        Self { range, extra }
    }

    pub(crate) fn extend_parser_end(&mut self, end: usize) -> Option<()> {
        let range = self.range.as_mut()?;
        if end < range.end {
            return None;
        }
        range.end = end;
        Some(())
    }

    /// 返回相对完整输入的 UTF-8 字节范围。
    pub fn range(&self) -> Option<Range<usize>> {
        self.range.clone()
    }

    /// 返回需随结构位置保留的范围扩展元数据。
    pub fn extra(&self) -> &BTreeMap<String, Value> {
        &self.extra
    }

    fn same_location(&self, other: &Self) -> bool {
        self.range == other.range
    }

    fn validates_source(&self, source: &str, scope: Option<&Range<usize>>) -> Option<()> {
        if let Some(range) = &self.range
            && (range.start > range.end
                || range.end > source.len()
                || !source.is_char_boundary(range.start)
                || !source.is_char_boundary(range.end)
                || scope.is_some_and(|scope| range.start < scope.start || range.end > scope.end))
        {
            return None;
        }
        Some(())
    }
}

/// 数学流映射，与其所属 MathFlow 的内容顺序保持一致。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MathFlowSourceMap {
    pub(super) range: MathRangeSourceMap,
    pub(super) content: Vec<MathContentSourceMap>,
}

impl MathFlowSourceMap {
    pub(crate) fn from_parser_parts(
        range: MathRangeSourceMap,
        content: Vec<MathContentSourceMap>,
    ) -> Self {
        Self { range, content }
    }

    /// 返回此流在完整公式源码中的范围。
    pub fn range(&self) -> Option<Range<usize>> {
        self.range.range()
    }

    /// 返回按文档树结构顺序排列的内容映射。
    pub fn content(&self) -> &[MathContentSourceMap] {
        &self.content
    }

    pub fn range_extra(&self) -> &BTreeMap<String, Value> {
        self.range.extra()
    }

    fn same_locations(&self, other: &Self) -> bool {
        self.range.same_location(&other.range)
            && self.content.len() == other.content.len()
            && self
                .content
                .iter()
                .zip(&other.content)
                .all(|(left, right)| left.same_locations(right))
    }
}

/// 文档数学内容项的源码位置；MathNode 操作数与环境子项按树继续嵌套。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathContentSourceMap {
    Text(MathTextSourceMap),
    Math {
        range: MathRangeSourceMap,
        operands: Vec<MathFlowSourceMap>,
        placeholder: bool,
    },
    Environment {
        range: MathRangeSourceMap,
        children: Vec<MathContentSourceMap>,
    },
    Other,
}

impl MathContentSourceMap {
    /// 返回内容项在完整公式源码中的范围。
    pub fn range(&self) -> Option<Range<usize>> {
        match self {
            Self::Text(text) => text.range.range(),
            Self::Math { range, .. } | Self::Environment { range, .. } => range.range(),
            Self::Other => None,
        }
    }

    /// 返回文本流映射；其他节点没有显示文本到源码片段映射。
    pub fn text(&self) -> Option<&MathTextSourceMap> {
        match self {
            Self::Text(text) => Some(text),
            Self::Math { .. } | Self::Environment { .. } | Self::Other => None,
        }
    }

    /// 返回数学节点的操作数流映射。
    pub fn operands(&self) -> Option<&[MathFlowSourceMap]> {
        match self {
            Self::Math { operands, .. } => Some(operands),
            Self::Text(_) | Self::Environment { .. } | Self::Other => None,
        }
    }

    /// 返回行内环境子项映射。
    pub fn children(&self) -> Option<&[MathContentSourceMap]> {
        match self {
            Self::Environment { children, .. } => Some(children),
            Self::Text(_) | Self::Math { .. } | Self::Other => None,
        }
    }

    fn same_locations(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(left), Self::Text(right)) => left.same_locations(right),
            (
                Self::Math {
                    range: left_range,
                    operands: left_operands,
                    placeholder: left_placeholder,
                },
                Self::Math {
                    range: right_range,
                    operands: right_operands,
                    placeholder: right_placeholder,
                },
            ) => {
                (left_range.same_location(right_range)
                    || (*left_placeholder
                        && *right_placeholder
                        && same_placeholder_location(left_range.range(), right_range.range())))
                    && left_operands.len() == right_operands.len()
                    && left_operands
                        .iter()
                        .zip(right_operands)
                        .all(|(left, right)| left.same_locations(right))
            }
            (
                Self::Environment {
                    range: left_range,
                    children: left_children,
                },
                Self::Environment {
                    range: right_range,
                    children: right_children,
                },
            ) => {
                left_range.same_location(right_range)
                    && left_children.len() == right_children.len()
                    && left_children
                        .iter()
                        .zip(right_children)
                        .all(|(left, right)| left.same_locations(right))
            }
            (Self::Other, Self::Other) => true,
            _ => false,
        }
    }
}

/// TextFlow 的整体范围和显示文本到 LaTeX 源码的分段映射。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MathTextSourceMap {
    pub(super) range: MathRangeSourceMap,
    pub(super) segments: Vec<MathTextSourceSegment>,
}

impl MathTextSourceMap {
    pub(crate) fn from_parser_parts(
        range: MathRangeSourceMap,
        segments: Vec<MathTextSourceSegment>,
    ) -> Self {
        Self { range, segments }
    }

    pub(crate) fn append_parser_segment(
        &mut self,
        display: Range<usize>,
        source: MathRangeSourceMap,
    ) -> Option<()> {
        self.range.extend_parser_end(source.range.as_ref()?.end)?;
        self.segments.push(MathTextSourceSegment {
            display,
            source,
            extra: BTreeMap::new(),
        });
        Some(())
    }

    /// 返回文本流在完整公式源码中的范围。
    pub fn range(&self) -> Option<Range<usize>> {
        self.range.range()
    }

    pub fn range_extra(&self) -> &BTreeMap<String, Value> {
        self.range.extra()
    }

    /// 返回映射片段；保留解析树中的顺序，包括零宽扩展锚点。
    pub fn segments(&self) -> &[MathTextSourceSegment] {
        &self.segments
    }

    fn same_locations(&self, other: &Self) -> bool {
        self.range.same_location(&other.range)
            && normalized_segments(&self.segments) == normalized_segments(&other.segments)
    }

    fn validates(
        &self,
        text: &caret_doc::TextFlow,
        source: &str,
        scope: Option<&Range<usize>>,
    ) -> Option<()> {
        self.range.range.as_ref()?;
        self.range.validates_source(source, scope)?;
        let mut display_len = 0usize;
        let mut run_ranges = Vec::with_capacity(text.runs.len());
        for run in &text.runs {
            let end = display_len.checked_add(run.text.len())?;
            run_ranges.push(display_len..end);
            display_len = end;
        }
        let mut display_end = 0usize;
        let mut source_end = self.range.range.as_ref()?.start;
        for segment in &self.segments {
            let display = &segment.display;
            if display.start != display_end
                || display.start > display.end
                || display.end > display_len
                || !display_boundary(text, &run_ranges, display_len, display.start)
                || !display_boundary(text, &run_ranges, display_len, display.end)
            {
                return None;
            }
            let source_range = segment.source.range.as_ref()?;
            segment.source.validates_source(source, scope)?;
            if source_range.start < source_end
                || self.range.range.as_ref().is_some_and(|flow_range| {
                    source_range.start < flow_range.start || source_range.end > flow_range.end
                })
            {
                return None;
            }
            source_end = source_range.end;
            display_end = display.end;
        }
        if display_end != display_len {
            return None;
        }
        if display_len == 0 {
            let anchor = if self.segments.is_empty() {
                let range = self.range.range.as_ref()?;
                if range.start != range.end {
                    return None;
                }
                range.start
            } else {
                let mut anchor = None;
                for segment in &self.segments {
                    let range = segment.source.range.as_ref()?;
                    if range.start != range.end
                        || anchor.is_some_and(|previous| previous != range.start)
                    {
                        return None;
                    }
                    anchor = Some(range.start);
                }
                anchor?
            };
            if self
                .range
                .range
                .as_ref()
                .is_some_and(|range| anchor < range.start || anchor > range.end)
            {
                return None;
            }
        }
        Some(())
    }
}

/// 一个显示文本区间及其对应的完整源码范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathTextSourceSegment {
    pub(super) display: Range<usize>,
    pub(super) source: MathRangeSourceMap,
    pub(super) extra: BTreeMap<String, Value>,
}

impl MathTextSourceSegment {
    pub(crate) fn from_parser_parts(display: Range<usize>, source: MathRangeSourceMap) -> Self {
        Self {
            display,
            source,
            extra: BTreeMap::new(),
        }
    }

    /// 返回 TextFlow 显示文本中的 UTF-8 字节范围。
    pub fn display(&self) -> Range<usize> {
        self.display.clone()
    }

    /// 返回完整公式源码中的 UTF-8 字节范围。
    pub fn source(&self) -> Option<Range<usize>> {
        self.source.range()
    }

    /// 返回源映射片段的扩展元数据。
    pub fn extra(&self) -> &BTreeMap<String, Value> {
        &self.extra
    }

    /// 返回源码范围自身的扩展元数据。
    pub fn source_extra(&self) -> &BTreeMap<String, Value> {
        self.source.extra()
    }
}

fn normalized_segments(segments: &[MathTextSourceSegment]) -> Vec<(Range<usize>, Range<usize>)> {
    let mut normalized: Vec<(Range<usize>, Range<usize>)> = Vec::with_capacity(segments.len());
    for segment in segments {
        let Some(source) = segment.source.range() else {
            continue;
        };
        if segment.display.start != segment.display.end
            && segment.display.len() == source.len()
            && let Some((display, previous_source)) = normalized.last_mut()
            && display.start != display.end
            && display.end == segment.display.start
            && previous_source.end == source.start
            && display.len() == previous_source.len()
        {
            display.end = segment.display.end;
            previous_source.end = source.end;
        } else {
            normalized.push((segment.display.clone(), source));
        }
    }
    normalized
}

fn same_placeholder_location(left: Option<Range<usize>>, right: Option<Range<usize>>) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return false;
    };
    let centered = |point: &Range<usize>, span: &Range<usize>| {
        point.start == point.end
            && span.len() == 2
            && point.start.checked_mul(2) == span.start.checked_add(span.end)
    };
    centered(&left, &right) || centered(&right, &left)
}

fn validate_flow_projection(
    flow: &MathFlow,
    map: &MathFlowSourceMap,
    source: &str,
    scope: Option<&Range<usize>>,
) -> Option<()> {
    map.range.validates_source(source, scope)?;
    if flow.content.len() != map.content.len() {
        return None;
    }
    let child_scope = map.range.range.as_ref().or(scope);
    let mut previous_end = None;
    for (content, content_map) in flow.content.iter().zip(&map.content) {
        if let Some(range) = content_map.range() {
            if previous_end.is_some_and(|end| range.start < end) {
                return None;
            }
            previous_end = Some(range.end);
        }
        validate_content_projection(content, content_map, source, child_scope)?;
    }
    Some(())
}

fn validate_content_projection(
    content: &MathContent,
    map: &MathContentSourceMap,
    source: &str,
    scope: Option<&Range<usize>>,
) -> Option<()> {
    match (content, map) {
        (MathContent::Text(text), MathContentSourceMap::Text(text_map)) => {
            text_map.validates(text, source, scope)?;
        }
        (
            MathContent::Math(node),
            MathContentSourceMap::Math {
                range,
                operands,
                placeholder,
            },
        ) => {
            range.validates_source(source, scope)?;
            if *placeholder != (node.kind == MathNodeKind::Placeholder)
                || operands.len() != node.children.len()
            {
                return None;
            }
            let child_scope = range.range.as_ref().or(scope);
            // 只有语义槽顺序固定且等于语法参数顺序的节点才要求操作数范围递增。
            let syntax_order_matches_slots = matches!(
                node.kind,
                MathNodeKind::Fraction
                    | MathNodeKind::Radical
                    | MathNodeKind::Matrix
                    | MathNodeKind::Substack
                    | MathNodeKind::CancelTo
            );
            let mut previous_operand_end = None;
            for (operand, operand_map) in node.children.iter().zip(operands) {
                if syntax_order_matches_slots
                    && let Some(operand_range) = operand_map.range.range.as_ref()
                {
                    if previous_operand_end.is_some_and(|end| operand_range.start < end) {
                        return None;
                    }
                    previous_operand_end = Some(operand_range.end);
                }
                validate_flow_projection(operand, operand_map, source, child_scope)?;
            }
        }
        (
            MathContent::Environment(environment),
            MathContentSourceMap::Environment { range, children },
        ) => {
            range.validates_source(source, scope)?;
            if children.len() != environment.children.len() {
                return None;
            }
            let child_scope = range.range.as_ref().or(scope);
            let mut previous_child_end = None;
            for (child, child_map) in environment.children.iter().zip(children) {
                if let Some(child_range) = child_map.range() {
                    if previous_child_end.is_some_and(|end| child_range.start < end) {
                        return None;
                    }
                    previous_child_end = Some(child_range.end);
                }
                validate_content_projection(child, child_map, source, child_scope)?;
            }
        }
        (MathContent::SoftBreak(_) | MathContent::Opaque(_), MathContentSourceMap::Other) => {}
        _ => return None,
    }
    Some(())
}

/// 与 MathTree 同形的临时源码范围树；不会写入 `.cdoc`。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MathTreeSourceMap {
    pub(super) root: MathFlowSourceMap,
    pub(super) semantic_signature: Option<u64>,
}

impl MathTreeSourceMap {
    /// 将解析映射按目标树结构重新分配，并保留可唯一归属的旧映射扩展。
    ///
    /// 映射中的范围相对完整 `source`；Align 单元格须传入完整源码和该 cell 的 `scope`。
    /// 调用方须先确认旧映射属于目标树版本及解析树与目标树的语义等价。结构、源码范围或
    /// 扩展归属不明确时返回 `None`，不修改任何输入；目前旧映射含扩展时一律拒绝重绑，
    /// 避免按树序号猜测扩展归属。返回映射由调用方持有。
    pub fn reconcile_for_tree(
        &self,
        parsed: &MathTree,
        target: &MathTree,
        previous: Option<&Self>,
        source: &str,
        scope: Option<&Range<usize>>,
    ) -> Option<Self> {
        super::reconcile::reconcile_tree_sidecar(target, parsed, self, previous, source, scope)
    }

    pub(crate) fn from_parser_tree(
        root: MathFlowSourceMap,
        tree: &MathTree,
        capture_signature: bool,
    ) -> Self {
        let semantic_signature = capture_signature.then(|| signature(&crate::serialize_tree(tree)));
        Self {
            root,
            semantic_signature,
        }
    }

    pub(crate) fn semantic_signature(&self) -> Option<u64> {
        self.semantic_signature
    }

    pub(crate) fn signature_matches_tree(&self, tree: &MathTree) -> bool {
        self.semantic_signature
            .is_none_or(|expected| expected == signature(&crate::serialize_tree(tree)))
    }

    /// 校验 sidecar 的范围与目标树结构相容；不单独证明目标树与源码语义相同。
    pub(crate) fn validates_projection(
        &self,
        tree: &MathTree,
        source: &str,
        scope: Option<&Range<usize>>,
    ) -> Option<()> {
        validate_flow_projection(&tree.root, &self.root, source, scope)
    }

    /// 返回数学树根流的范围映射。
    pub fn root(&self) -> &MathFlowSourceMap {
        &self.root
    }

    /// 比较结构路径对应的源码位置身份。
    ///
    /// 忽略映射扩展元数据；相邻线性文本片段可合并视为相同，空公式占位符的零宽中点锚点
    /// 与其空定界符 `{}` 的两字节范围也视为同一位置。
    pub fn same_locations(&self, other: &Self) -> bool {
        self.root.same_locations(&other.root)
    }

    /// 按文档模型结构路径取得内容映射；路径指向流而非内容时返回 `None`。
    pub fn content_at(&self, path: &[MathPathStep]) -> Option<&MathContentSourceMap> {
        enum Target<'a> {
            Flow(&'a MathFlowSourceMap),
            Content(&'a MathContentSourceMap),
        }

        let mut current = Target::Flow(&self.root);
        for step in path {
            current = match (current, step) {
                (Target::Flow(flow), MathPathStep::Content(index)) => {
                    Target::Content(flow.content.get(*index)?)
                }
                (
                    Target::Content(MathContentSourceMap::Environment { children, .. }),
                    MathPathStep::EnvironmentChild(index),
                ) => Target::Content(children.get(*index)?),
                (
                    Target::Content(MathContentSourceMap::Math { operands, .. }),
                    MathPathStep::Operand(index),
                ) => Target::Flow(operands.get(*index)?),
                _ => return None,
            };
        }
        match current {
            Target::Content(content) => Some(content),
            Target::Flow(_) => None,
        }
    }

    /// 估算范围树本身的堆占用，供编辑历史预算使用。
    pub fn estimated_bytes(&self) -> usize {
        size_of::<Self>() + estimate_flow(&self.root)
    }
}

fn signature(value: &str) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn display_boundary(
    text: &caret_doc::TextFlow,
    run_ranges: &[Range<usize>],
    display_len: usize,
    offset: usize,
) -> bool {
    if offset == 0 || offset == display_len {
        return true;
    }
    let index = run_ranges.partition_point(|range| range.end < offset);
    let Some(range) = run_ranges.get(index) else {
        return false;
    };
    if offset < range.start || offset > range.end {
        return false;
    }
    text.runs[index].text.is_char_boundary(offset - range.start)
}

fn estimate_flow(flow: &MathFlowSourceMap) -> usize {
    size_of::<MathFlowSourceMap>()
        + estimate_range(&flow.range)
        + flow.content.capacity() * size_of::<MathContentSourceMap>()
        + flow.content.iter().map(estimate_content).sum::<usize>()
}

fn estimate_content(content: &MathContentSourceMap) -> usize {
    match content {
        MathContentSourceMap::Text(text) => {
            size_of::<MathTextSourceMap>()
                + estimate_range(&text.range)
                + text.segments.capacity() * size_of::<MathTextSourceSegment>()
                + text
                    .segments
                    .iter()
                    .map(|segment| estimate_range(&segment.source) + estimate_extra(&segment.extra))
                    .sum::<usize>()
        }
        MathContentSourceMap::Math {
            range, operands, ..
        } => {
            estimate_range(range)
                + operands.capacity() * size_of::<MathFlowSourceMap>()
                + operands.iter().map(estimate_flow).sum::<usize>()
        }
        MathContentSourceMap::Environment { range, children } => {
            estimate_range(range)
                + children.capacity() * size_of::<MathContentSourceMap>()
                + children.iter().map(estimate_content).sum::<usize>()
        }
        MathContentSourceMap::Other => 0,
    }
}

fn estimate_range(range: &MathRangeSourceMap) -> usize {
    size_of::<MathRangeSourceMap>() + estimate_extra(&range.extra)
}

fn estimate_extra(extra: &BTreeMap<String, Value>) -> usize {
    extra
        .iter()
        .map(|(key, value)| {
            size_of::<(String, Value)>() + key.capacity() + value.to_string().capacity()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectedMath;
    use caret_doc::{Run, TextFlow};

    #[test]
    fn bind_tree_sidecar_checks_shape_kind_and_signature_atomically() {
        let projection = crate::parse_projection(r"\text{abc}", crate::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("{error}"));
        let (ProjectedMath::Tree(tree), mut projection) = projection.into_parts() else {
            panic!("inline tree");
        };
        let mapped = projection
            .tree()
            .cloned()
            .unwrap_or_else(|| panic!("sidecar"));
        assert!(projection.bind_tree_sidecar_after_validated_edit(&tree, mapped.clone()));
        let before = projection.clone();
        let mut outside = mapped.clone();
        outside.root.range.range = Some(0..projection.source().len() + 1);
        assert!(!projection.bind_tree_sidecar_after_validated_edit(&tree, outside));
        assert_eq!(projection, before);

        let mut wrong = tree.clone();
        wrong.root.content.clear();
        assert!(!projection.bind_tree_sidecar_after_validated_edit(&wrong, mapped.clone()));
        assert_eq!(projection, before);
        let other = crate::parse_projection(r"\text{xyz}", crate::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("{error}"));
        let (ProjectedMath::Tree(other_tree), other_map) = other.into_parts() else {
            panic!("inline tree")
        };
        let other_map = other_map
            .tree()
            .cloned()
            .unwrap_or_else(|| panic!("sidecar"));
        assert!(!projection.bind_tree_sidecar_after_validated_edit(&other_tree, other_map));
        assert_eq!(projection, before);
        projection.kind = crate::MathInputKind::Align;
        let wrong_kind = projection.clone();
        assert!(!projection.bind_tree_sidecar_after_validated_edit(&tree, mapped));
        assert_eq!(projection, wrong_kind);
    }

    #[test]
    fn bind_align_sidecars_checks_all_cells_before_replacement() {
        let source = r"\begin{aligned}\text{a}&\text{b}\\x&y\end{aligned}";
        let projection = crate::parse_projection(source, crate::MathInputKind::Align)
            .unwrap_or_else(|error| panic!("{error}"));
        let (ProjectedMath::Align(align), mut projection) = projection.into_parts() else {
            panic!("align")
        };
        let align_map = projection.align().unwrap_or_else(|| panic!("align map"));
        let maps = (0..2)
            .map(|row| {
                (0..2)
                    .map(|cell| {
                        align_map
                            .cell_tree(row, cell)
                            .cloned()
                            .unwrap_or_else(|| panic!("cell map"))
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert!(projection.bind_align_sidecars_after_validated_edit(&align, maps.clone()));
        let before = projection.clone();
        let mut wrong = maps.clone();
        wrong[1][1] = maps[0][0].clone();
        assert!(!projection.bind_align_sidecars_after_validated_edit(&align, wrong));
        assert_eq!(projection, before);
        let mut wrong = maps;
        wrong[1].pop();
        assert!(!projection.bind_align_sidecars_after_validated_edit(&align, wrong));
        assert_eq!(projection, before);
    }

    #[test]
    fn bind_align_sidecars_accepts_empty_cells_comments_and_delimiters() {
        for source in [
            r"\begin{aligned}a&&b\end{aligned}",
            "\\begin{aligned}a&% comment\n b\\end{aligned}",
            r"\[\begin{aligned}x&y\end{aligned}\]",
        ] {
            let projection = crate::parse_projection(source, crate::MathInputKind::Align)
                .unwrap_or_else(|error| panic!("{source}: {error}"));
            let (ProjectedMath::Align(align), mut projection) = projection.into_parts() else {
                panic!("align")
            };
            let sidecar = projection.align().unwrap_or_else(|| panic!("align map"));
            let maps = align
                .rows
                .iter()
                .enumerate()
                .map(|(row_index, row)| {
                    row.cells
                        .iter()
                        .enumerate()
                        .map(|(cell_index, _)| {
                            sidecar
                                .cell_tree(row_index, cell_index)
                                .cloned()
                                .unwrap_or_else(|| panic!("cell map"))
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            assert!(
                projection.bind_align_sidecars_after_validated_edit(&align, maps),
                "{source}"
            );
        }
    }

    #[test]
    fn text_flow_projection_requires_display_segments_to_cover_text() {
        let text = TextFlow {
            runs: vec![Run {
                text: "x".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        };
        let map = MathTextSourceMap {
            range: MathRangeSourceMap::default(),
            segments: Vec::new(),
        };
        assert!(map.validates(&text, "x", None).is_none());
    }

    #[test]
    fn projection_validation_uses_independent_sidecar_ranges() {
        let source = r"\frac{\textbf{a\textit{b}}}{2}";
        let projection = crate::parse_projection(source, crate::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("math source should project: {error}"));
        let (ProjectedMath::Tree(tree), source_map) = projection.into_parts() else {
            panic!("inline source should produce a tree");
        };
        let map = source_map
            .tree()
            .unwrap_or_else(|| panic!("projection should contain a tree map"));
        assert!(map.validates_projection(&tree, source, None).is_some());
    }

    #[test]
    fn projection_validation_rejects_missing_text_anchors_and_reordered_operands() {
        let text = TextFlow {
            runs: vec![Run {
                text: String::new(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        };
        let missing_anchor = MathTextSourceMap {
            range: MathRangeSourceMap::from_parser_range(Some(0..0), BTreeMap::new()),
            segments: Vec::new(),
        };
        assert!(missing_anchor.validates(&text, "", None).is_some());
        let absent_range = MathTextSourceMap {
            range: MathRangeSourceMap::default(),
            segments: Vec::new(),
        };
        assert!(absent_range.validates(&text, "", None).is_none());
        let ambiguous_anchor = MathTextSourceMap {
            range: MathRangeSourceMap::from_parser_range(Some(0..1), BTreeMap::new()),
            segments: vec![MathTextSourceSegment::from_parser_parts(
                0..0,
                MathRangeSourceMap::from_parser_range(Some(0..1), BTreeMap::new()),
            )],
        };
        assert!(ambiguous_anchor.validates(&text, "x", None).is_none());

        let source = r"\frac{x}{y}";
        let projection = crate::parse_projection(source, crate::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("fraction should project: {error}"));
        let (ProjectedMath::Tree(tree), source_map) = projection.into_parts() else {
            panic!("inline source should produce a tree");
        };
        let mut map = source_map
            .tree()
            .cloned()
            .unwrap_or_else(|| panic!("fraction should have a sidecar"));
        let Some(MathContentSourceMap::Math { operands, .. }) = map.root.content.first_mut() else {
            panic!("fraction should map as a math node");
        };
        operands.swap(0, 1);
        assert!(map.validates_projection(&tree, source, None).is_none());

        let source = r"\textbf{a\textit{b}c}";
        let projection = crate::parse_projection(source, crate::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("nested text styles should project: {error}"));
        let (ProjectedMath::Tree(tree), source_map) = projection.into_parts() else {
            panic!("inline source should produce a tree");
        };
        let mut map = source_map
            .tree()
            .cloned()
            .unwrap_or_else(|| panic!("text styles should have a sidecar"));
        let Some(MathContentSourceMap::Environment { children, .. }) = map.root.content.first_mut()
        else {
            panic!("text style should map as an environment");
        };
        children.swap(0, 2);
        assert!(map.validates_projection(&tree, source, None).is_none());

        for source in [
            "x^a_b",
            "x_b^a",
            r"\sum^n_i",
            r"\sum_i^n",
            r"\prod^n_{i=1}",
            r"\int^1_0",
            "x'_i^2",
            "x'^2_i",
        ] {
            assert!(
                crate::parse_projection(source, crate::MathInputKind::Inline).is_ok(),
                "valid script ordering should retain a source map: {source}"
            );
        }
    }

    #[test]
    fn location_comparison_normalizes_adjacent_linear_text_segments() {
        let source = r"\text{abcd}";
        let projection = crate::parse_projection(source, crate::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("text source should project: {error}"));
        let (ProjectedMath::Tree(_tree), source_map) = projection.into_parts() else {
            panic!("inline text source should produce a tree");
        };
        let first = source_map
            .tree()
            .cloned()
            .unwrap_or_else(|| panic!("missing map"));
        let mut second = first.clone();
        let Some(MathContentSourceMap::Text(text)) = second.root.content.first_mut() else {
            panic!("text source should start with a text map");
        };
        text.segments = (0..4)
            .map(|offset| {
                MathTextSourceSegment::from_parser_parts(
                    offset..offset + 1,
                    MathRangeSourceMap::from_parser_range(
                        Some(6 + offset..7 + offset),
                        BTreeMap::new(),
                    ),
                )
            })
            .collect();
        assert!(first.same_locations(&second));
    }

    #[test]
    fn location_comparison_accepts_the_center_anchor_of_empty_math_delimiters() {
        let source = r"\text{ab$$cd}";
        let projection = crate::parse_projection(source, crate::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("empty math anchor should project: {error}"));
        let (ProjectedMath::Tree(_tree), source_map) = projection.into_parts() else {
            panic!("inline source should produce a tree");
        };
        let original = source_map.tree().cloned().unwrap_or_else(|| panic!("map"));

        let mut centered = original.clone();
        let Some(MathContentSourceMap::Math { range, .. }) = centered.root.content.get_mut(1)
        else {
            panic!("empty math delimiter should map to a math node");
        };
        range.range = Some(9..9);
        assert!(original.same_locations(&centered));

        let mut misplaced = original.clone();
        let Some(MathContentSourceMap::Math { range, .. }) = misplaced.root.content.get_mut(1)
        else {
            panic!("empty math delimiter should map to a math node");
        };
        range.range = Some(8..8);
        assert!(!original.same_locations(&misplaced));
    }
}
