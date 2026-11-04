//! 保存在解析公式树之外的结构化 LaTeX 源码范围映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::{collections::BTreeMap, mem::size_of, ops::Range};

use caret_doc::{MathContent, MathFlow, MathNodeKind, MathPathStep, MathSourceRange, MathTree};
use serde_json::Value;

/// 一个可选源码范围及其无法由解析器解释的扩展元数据。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MathRangeSourceMap {
    range: Option<Range<usize>>,
    extra: BTreeMap<String, Value>,
}

impl MathRangeSourceMap {
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
}

/// 数学流映射，与其所属 MathFlow 的内容顺序保持一致。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MathFlowSourceMap {
    range: MathRangeSourceMap,
    content: Vec<MathContentSourceMap>,
}

impl MathFlowSourceMap {
    /// 返回此流在完整公式源码中的范围。
    pub fn range(&self) -> Option<Range<usize>> {
        self.range.range()
    }

    /// 返回按文档树结构顺序排列的内容映射。
    pub fn content(&self) -> &[MathContentSourceMap] {
        &self.content
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
    range: MathRangeSourceMap,
    segments: Vec<MathTextSourceSegment>,
}

impl MathTextSourceMap {
    /// 返回文本流在完整公式源码中的范围。
    pub fn range(&self) -> Option<Range<usize>> {
        self.range.range()
    }

    /// 返回映射片段；保留解析树中的顺序，包括零宽扩展锚点。
    pub fn segments(&self) -> &[MathTextSourceSegment] {
        &self.segments
    }

    fn same_locations(&self, other: &Self) -> bool {
        self.range.same_location(&other.range)
            && normalized_segments(&self.segments) == normalized_segments(&other.segments)
    }
}

/// 一个显示文本区间及其对应的完整源码范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathTextSourceSegment {
    display: Range<usize>,
    source: MathRangeSourceMap,
    extra: BTreeMap<String, Value>,
}

impl MathTextSourceSegment {
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

/// 与 MathTree 同形的临时源码范围树；不会写入 `.cdoc`。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MathTreeSourceMap {
    root: MathFlowSourceMap,
    semantic_signature: Option<u64>,
}

impl MathTreeSourceMap {
    /// 按树结构捕获范围并转换到完整公式源码坐标；Align cell 可提供坐标基址和边界。
    pub(crate) fn capture(
        tree: &MathTree,
        source: &str,
        offset: usize,
        scope: Option<&Range<usize>>,
        capture_signature: bool,
    ) -> Option<Self> {
        let root = capture_flow(&tree.root, source, offset, scope)?;
        let semantic_signature = capture_signature.then(|| signature(&crate::serialize_tree(tree)));
        Some(Self {
            root,
            semantic_signature,
        })
    }

    pub(crate) fn semantic_signature(&self) -> Option<u64> {
        self.semantic_signature
    }

    /// 返回数学树根流的范围映射。
    pub fn root(&self) -> &MathFlowSourceMap {
        &self.root
    }

    /// 校验结构路径对应的源码位置；忽略临时映射上的扩展元数据。
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

fn capture_flow(
    flow: &MathFlow,
    source: &str,
    offset: usize,
    scope: Option<&Range<usize>>,
) -> Option<MathFlowSourceMap> {
    let range = capture_range(flow.source_range.as_ref(), source, offset, scope)?;
    let content = flow
        .content
        .iter()
        .map(|content| capture_content(content, source, offset, scope))
        .collect::<Option<Vec<_>>>()?;
    Some(MathFlowSourceMap { range, content })
}

fn capture_content(
    content: &MathContent,
    source: &str,
    offset: usize,
    scope: Option<&Range<usize>>,
) -> Option<MathContentSourceMap> {
    match content {
        MathContent::Text(text) => {
            let range = capture_range(Some(text.source_range.as_ref()?), source, offset, scope)?;
            let mut display_len = 0usize;
            let mut run_ranges = Vec::with_capacity(text.runs.len());
            for run in &text.runs {
                let start = display_len;
                display_len = display_len.checked_add(run.text.len())?;
                run_ranges.push(start..display_len);
            }
            let segments = text
                .source_map
                .iter()
                .map(|entry| {
                    let display = entry.text_start as usize..entry.text_end as usize;
                    if display.start > display.end
                        || display.end > display_len
                        || !display_boundary(text, &run_ranges, display_len, display.start)
                        || !display_boundary(text, &run_ranges, display_len, display.end)
                    {
                        return None;
                    }
                    let source = capture_range(Some(&entry.source_range), source, offset, scope)?;
                    source.range.as_ref()?;
                    Some(MathTextSourceSegment {
                        display,
                        source,
                        extra: entry.extra.clone(),
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(MathContentSourceMap::Text(MathTextSourceMap {
                range,
                segments,
            }))
        }
        MathContent::Math(node) => {
            let range = capture_range(node.source_range.as_ref(), source, offset, scope)?;
            let operands = node
                .children
                .iter()
                .map(|flow| capture_flow(flow, source, offset, scope))
                .collect::<Option<Vec<_>>>()?;
            Some(MathContentSourceMap::Math {
                range,
                operands,
                placeholder: node.kind == MathNodeKind::Placeholder,
            })
        }
        MathContent::Environment(environment) => {
            let range = capture_range(environment.source_range.as_ref(), source, offset, scope)?;
            let children = environment
                .children
                .iter()
                .map(|child| capture_content(child, source, offset, scope))
                .collect::<Option<Vec<_>>>()?;
            Some(MathContentSourceMap::Environment { range, children })
        }
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => Some(MathContentSourceMap::Other),
    }
}

fn capture_range(
    range: Option<&MathSourceRange>,
    source: &str,
    offset: usize,
    scope: Option<&Range<usize>>,
) -> Option<MathRangeSourceMap> {
    let Some(range) = range else {
        return Some(MathRangeSourceMap::default());
    };
    let start = offset.checked_add(range.start as usize)?;
    let end = offset.checked_add(range.end as usize)?;
    if start > end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
        || scope.is_some_and(|scope| start < scope.start || end > scope.end)
    {
        return None;
    }
    Some(MathRangeSourceMap {
        range: Some(start..end),
        extra: range.extra.clone(),
    })
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
    use caret_doc::{MathSourceRange, TextFlowSourceMapEntry};

    #[test]
    fn location_comparison_normalizes_adjacent_linear_text_segments() {
        let source = r"\text{abcd}";
        let projection = crate::parse_projection(source, crate::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("text source should project: {error}"));
        let (ProjectedMath::Tree(tree), _) = projection.into_parts() else {
            panic!("inline text source should produce a tree");
        };
        let mut segmented = tree.clone();
        let Some(MathContent::Text(text)) = segmented.root.content.first_mut() else {
            panic!("text source should start with a TextFlow");
        };
        text.source_map = (0..4)
            .map(|offset| TextFlowSourceMapEntry {
                text_start: offset,
                text_end: offset + 1,
                source_range: MathSourceRange {
                    start: 6 + offset,
                    end: 7 + offset,
                    extra: BTreeMap::new(),
                },
                extra: BTreeMap::new(),
            })
            .collect();
        let first = MathTreeSourceMap::capture(&tree, source, 0, None, false)
            .unwrap_or_else(|| panic!("parsed tree should have valid source locations"));
        let second = MathTreeSourceMap::capture(&segmented, source, 0, None, false)
            .unwrap_or_else(|| panic!("segmented text should have valid source locations"));
        assert!(first.same_locations(&second));
    }

    #[test]
    fn location_comparison_accepts_the_center_anchor_of_empty_math_delimiters() {
        let source = r"\text{ab$$cd}";
        let projection = crate::parse_projection(source, crate::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("empty math anchor should project: {error}"));
        let (ProjectedMath::Tree(tree), _) = projection.into_parts() else {
            panic!("inline source should produce a tree");
        };
        let original = MathTreeSourceMap::capture(&tree, source, 0, None, false)
            .unwrap_or_else(|| panic!("parsed anchor should have valid locations"));

        let mut centered = tree.clone();
        let Some(MathContent::Math(node)) = centered.root.content.get_mut(1) else {
            panic!("empty math delimiter should project to a math node");
        };
        assert_eq!(node.kind, MathNodeKind::Placeholder);
        node.source_range = Some(MathSourceRange {
            start: 9,
            end: 9,
            extra: BTreeMap::new(),
        });
        let centered = MathTreeSourceMap::capture(&centered, source, 0, None, false)
            .unwrap_or_else(|| panic!("center anchor should remain within source bounds"));
        assert!(original.same_locations(&centered));

        let mut misplaced = tree;
        let Some(MathContent::Math(node)) = misplaced.root.content.get_mut(1) else {
            panic!("empty math delimiter should project to a math node");
        };
        node.source_range = Some(MathSourceRange {
            start: 8,
            end: 8,
            extra: BTreeMap::new(),
        });
        let misplaced = MathTreeSourceMap::capture(&misplaced, source, 0, None, false)
            .unwrap_or_else(|| panic!("in-range misplaced anchor should still be capturable"));
        assert!(!original.same_locations(&misplaced));
    }
}
