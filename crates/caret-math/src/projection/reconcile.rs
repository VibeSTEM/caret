//! 将已解析源码的临时位置与目标数学树结构对齐。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{MathContent, MathFlow, MathNodeKind, MathTree, NodeId};
use std::collections::BTreeMap;

/// 验证解析树与持久树具有相同的规范语义，并保留持久树身份和元数据。
///
/// 源码位置由独立 sidecar 管理；此函数不改变树结构，也不附加瞬时映射。
pub fn reconcile_tree_ranges(tree: &MathTree, parsed: &MathTree) -> Option<MathTree> {
    (tree.schema_version == parsed.schema_version
        && crate::serialize_tree(tree) == crate::serialize_tree(parsed))
    .then(|| tree.clone())
}

/// 验证解析 Align 与持久 Align 具有相同的规范语义，并保留持久身份和元数据。
pub fn reconcile_align_ranges(
    align: &caret_doc::Align,
    parsed: &caret_doc::Align,
) -> Option<caret_doc::Align> {
    (crate::serialize_align(align) == crate::serialize_align(parsed)).then(|| align.clone())
}

pub fn source_environment_props_match(
    kind: caret_doc::InlineEnvironmentKind,
    stored: &std::collections::BTreeMap<String, serde_json::Value>,
    parsed: &std::collections::BTreeMap<String, serde_json::Value>,
) -> bool {
    // 源码映射只约束可表达的样式；具体字体族保存在 MathTree 中。
    let parsed_props_match = parsed.iter().all(|(key, value)| {
        if kind.supports_font_family() && key == "fontFamily" {
            font_family_source_style(stored.get(key)) == font_family_source_style(Some(value))
        } else {
            stored.get(key) == Some(value)
        }
    });
    if !parsed_props_match {
        return false;
    }

    let enabled_is_representable = !kind.supports_enabled()
        || match stored.get("enabled") {
            None => true,
            Some(serde_json::Value::Bool(enabled)) => {
                parsed
                    .get("enabled")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true)
                    == *enabled
            }
            Some(_) => false,
        };
    let font_is_representable = !kind.supports_font_family()
        || font_family_source_style(stored.get("fontFamily"))
            == font_family_source_style(parsed.get("fontFamily"));

    enabled_is_representable && font_is_representable
}

fn font_family_source_style(value: Option<&serde_json::Value>) -> Option<&'static str> {
    match value {
        None => Some("rm"),
        Some(serde_json::Value::String(family)) if family == "sans-serif" => Some("sf"),
        Some(serde_json::Value::String(family)) if family == "monospace" => Some("tt"),
        Some(serde_json::Value::String(_)) => Some("rm"),
        Some(_) => None,
    }
}

fn same_display_text(left: &caret_doc::TextFlow, right: &caret_doc::TextFlow) -> bool {
    left.runs
        .iter()
        .flat_map(|run| run.text.chars())
        .eq(right.runs.iter().flat_map(|run| run.text.chars()))
}

// 源码范围只存在于 sidecar；不向目标文档树写入中间位置。
pub(super) fn reconcile_tree_sidecar(
    target: &MathTree,
    parsed: &MathTree,
    parsed_map: &super::source_map::MathTreeSourceMap,
    previous: Option<&super::source_map::MathTreeSourceMap>,
    source: &str,
    scope: Option<&std::ops::Range<usize>>,
) -> Option<super::source_map::MathTreeSourceMap> {
    if target.schema_version != parsed.schema_version {
        return None;
    }
    if previous.is_some_and(|map| sidecar_flow_has_extra(map.root())) {
        return None;
    }
    parsed_map.validates_projection(parsed, source, scope)?;
    let root = reconcile_sidecar_flow(&target.root, &parsed.root, &parsed_map.root)?;
    let result = super::source_map::MathTreeSourceMap::from_parser_tree(
        root,
        target,
        parsed_map.semantic_signature.is_some(),
    );
    result.validates_projection(target, source, scope)?;
    Some(result)
}

fn reconcile_sidecar_flow(
    target: &MathFlow,
    parsed: &MathFlow,
    map: &super::source_map::MathFlowSourceMap,
) -> Option<super::source_map::MathFlowSourceMap> {
    if empty_target_matches_parsed_placeholder(&target.content, &parsed.content) {
        return Some(super::source_map::MathFlowSourceMap::from_parser_parts(
            map.range.clone(),
            Vec::new(),
        ));
    }
    Some(super::source_map::MathFlowSourceMap::from_parser_parts(
        map.range.clone(),
        reconcile_sidecar_sequence(
            &target.content,
            &parsed.content,
            &map.content,
            map.range.range.as_ref(),
        )?,
    ))
}

fn empty_target_matches_parsed_placeholder(target: &[MathContent], parsed: &[MathContent]) -> bool {
    let is_placeholder = |contents: &[MathContent]| {
        matches!(
            contents,
            [MathContent::Math(node)]
                if node.kind == MathNodeKind::Placeholder
                    && node.text.is_none()
                    && node.attributes.is_empty()
                    && node.children.is_empty()
                    && node.extra.is_empty()
        )
    };
    target.is_empty() && is_placeholder(parsed)
}

fn reconcile_sidecar_sequence(
    target: &[MathContent],
    parsed: &[MathContent],
    maps: &[super::source_map::MathContentSourceMap],
    enclosing_range: Option<&std::ops::Range<usize>>,
) -> Option<Vec<super::source_map::MathContentSourceMap>> {
    use super::source_map::MathContentSourceMap as Map;
    if parsed.len() != maps.len() {
        return None;
    }
    let (mut i, mut j) = (0, 0);
    let mut result = Vec::with_capacity(target.len());
    while i < target.len() && j < parsed.len() {
        if let MathContent::Math(node) = &target[i]
            && node.kind == MathNodeKind::Row
            && !matches!(parsed.get(j), Some(MathContent::Math(parsed)) if parsed.kind == MathNodeKind::Row)
        {
            let (row_map, parsed_end) =
                reconcile_transparent_row(node, parsed, maps, j, enclosing_range)?;
            result.push(Map::Math {
                range: row_map.0,
                operands: row_map.1,
                placeholder: false,
            });
            i += 1;
            j = parsed_end;
            continue;
        }
        if is_empty_text_flow(&target[i]) && !is_empty_text_flow(&parsed[j]) {
            let anchor = sequence_anchor(maps, enclosing_range, j)?;
            result.push(empty_text_sidecar(anchor));
            i += 1;
            continue;
        }
        if matches!(target[i], MathContent::Text(_)) && matches!(parsed[j], MathContent::Text(_)) {
            let target_end = i + target[i..]
                .iter()
                .take_while(|c| matches!(c, MathContent::Text(_)))
                .count();
            let parsed_end = j + parsed[j..]
                .iter()
                .take_while(|c| matches!(c, MathContent::Text(_)))
                .count();
            result.extend(reconcile_sidecar_texts(
                &target[i..target_end],
                &parsed[j..parsed_end],
                &maps[j..parsed_end],
            )?);
            i = target_end;
            j = parsed_end;
            continue;
        }
        let item = match (&target[i], &parsed[j], &maps[j]) {
            (
                MathContent::Math(a),
                MathContent::Math(b),
                Map::Math {
                    range,
                    operands,
                    placeholder,
                },
            ) if source_math_node_kinds_match(a.kind, b.kind)
                && a.text == b.text
                && a.attributes == b.attributes
                && a.children.len() == b.children.len()
                && operands.len() == b.children.len()
                && *placeholder == (a.kind == caret_doc::MathNodeKind::Placeholder) =>
            {
                let children = a
                    .children
                    .iter()
                    .zip(&b.children)
                    .zip(operands)
                    .map(|((a, b), map)| reconcile_sidecar_flow(a, b, map))
                    .collect::<Option<Vec<_>>>()?;
                Map::Math {
                    range: range.clone(),
                    operands: children,
                    placeholder: *placeholder,
                }
            }
            (
                MathContent::Environment(a),
                MathContent::Environment(b),
                Map::Environment { range, children },
            ) if a.kind == b.kind && source_environment_props_match(a.kind, &a.props, &b.props) => {
                Map::Environment {
                    range: range.clone(),
                    children: reconcile_sidecar_sequence(
                        &a.children,
                        &b.children,
                        children,
                        range.range.as_ref(),
                    )?,
                }
            }
            (MathContent::SoftBreak(_), MathContent::SoftBreak(_), Map::Other) => Map::Other,
            (MathContent::Opaque(a), MathContent::Opaque(b), Map::Other) if a.raw == b.raw => {
                Map::Other
            }
            _ => return None,
        };
        result.push(item);
        i += 1;
        j += 1;
    }
    if j == parsed.len() {
        while i < target.len() && is_empty_text_flow(&target[i]) {
            let anchor = sequence_anchor(maps, enclosing_range, j)?;
            result.push(empty_text_sidecar(anchor));
            i += 1;
        }
    }
    (i == target.len() && j == parsed.len()).then_some(result)
}

fn source_math_node_kinds_match(stored: MathNodeKind, parsed: MathNodeKind) -> bool {
    stored == parsed
        || matches!(
            (stored, parsed),
            (MathNodeKind::Color, MathNodeKind::TextColor)
                | (MathNodeKind::TextColor, MathNodeKind::Color)
        )
}

/// `Row` 在序列化中不显示包装层；将解析器展开后的连续子项映射回其有序 `MathFlow` 槽。
fn reconcile_transparent_row(
    row: &caret_doc::MathNode,
    parsed: &[MathContent],
    maps: &[super::source_map::MathContentSourceMap],
    start: usize,
    enclosing_range: Option<&std::ops::Range<usize>>,
) -> Option<(
    (
        super::source_map::MathRangeSourceMap,
        Vec<super::source_map::MathFlowSourceMap>,
    ),
    usize,
)> {
    use super::source_map::{MathFlowSourceMap, MathRangeSourceMap};

    if !row.extra.is_empty()
        || !row.attributes.is_empty()
        || row.text.is_some()
        || row.children.iter().any(|flow| !flow.extra.is_empty())
        || parsed.len() != maps.len()
    {
        return None;
    }

    let target_contents = row
        .children
        .iter()
        .flat_map(|flow| flow.content.iter())
        .collect::<Vec<_>>();
    let target_source = serialize_sequence(target_contents.iter().copied());
    let mut parsed_end = None;
    if target_source.is_empty() {
        parsed_end = Some(start);
    } else {
        // 规范序列化长度随追加内容单调增长；二分定位候选，避免长 Row 逐前缀重序列化的平方开销。
        let mut low = start + 1;
        let mut high = parsed.len() + 1;
        while low < high {
            let middle = low + (high - low) / 2;
            let length = serialize_sequence(parsed[start..middle].iter()).len();
            if length < target_source.len() {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if low <= parsed.len() && serialize_sequence(parsed[start..low].iter()) == target_source {
            let next = low + 1;
            if next <= parsed.len()
                && serialize_sequence(parsed[start..next].iter()).len() == target_source.len()
            {
                // 相邻前缀长度相同且序列化结果相同时，源码无法判断空内容属于哪个 `Row`。
                return None;
            }
            parsed_end = Some(low);
        }
    }
    let parsed_end = parsed_end?;
    let target_maps = reconcile_sidecar_sequence(
        &target_contents
            .iter()
            .map(|item| (*item).clone())
            .collect::<Vec<_>>(),
        &parsed[start..parsed_end],
        &maps[start..parsed_end],
        enclosing_range,
    )?;
    let row_range = maps_range(&target_maps)
        .or_else(|| sequence_anchor(maps, enclosing_range, start).map(|anchor| anchor..anchor))?;

    let mut operands = Vec::with_capacity(row.children.len());
    let mut content_offset = 0usize;
    let mut previous_end = None;
    let mut has_empty_flow = false;
    for flow in &row.children {
        let content_end = content_offset.checked_add(flow.content.len())?;
        let flow_maps = target_maps.get(content_offset..content_end)?.to_vec();
        let range = if let Some(range) = maps_range(&flow_maps) {
            previous_end = Some(range.end);
            range
        } else if flow.content.is_empty() {
            if has_empty_flow {
                // 多个空流没有源码证据可唯一确定各自锚点。
                return None;
            }
            has_empty_flow = true;
            let next_start = maps_range(&target_maps[content_end..]).map(|range| range.start);
            let anchor = match (previous_end, next_start) {
                (Some(previous), Some(next)) if previous == next => previous,
                (Some(_), Some(_)) => return None,
                (Some(previous), None) => previous,
                (None, Some(next)) => next,
                (None, None) => enclosing_range
                    .map(|range| range.start)
                    .or_else(|| sequence_anchor(maps, None, start))?,
            };
            anchor..anchor
        } else {
            return None;
        };
        operands.push(MathFlowSourceMap::from_parser_parts(
            MathRangeSourceMap::from_parser_range(Some(range), BTreeMap::new()),
            flow_maps,
        ));
        content_offset = content_end;
    }
    Some((
        (
            MathRangeSourceMap::from_parser_range(Some(row_range), BTreeMap::new()),
            operands,
        ),
        parsed_end,
    ))
}

fn serialize_sequence<'a>(contents: impl Iterator<Item = &'a MathContent>) -> String {
    let tree = MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: MathFlow {
            id: NodeId::generate(),
            content: contents.cloned().collect(),
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    crate::serialize_tree(&tree)
}

fn maps_range(maps: &[super::source_map::MathContentSourceMap]) -> Option<std::ops::Range<usize>> {
    let first = maps
        .iter()
        .find_map(super::source_map::MathContentSourceMap::range)?;
    let last = maps
        .iter()
        .rev()
        .find_map(super::source_map::MathContentSourceMap::range)?;
    Some(first.start..last.end)
}

fn is_empty_text_flow(content: &MathContent) -> bool {
    matches!(content, MathContent::Text(text) if text.runs.iter().all(|run| run.text.is_empty()))
}

fn sequence_anchor(
    maps: &[super::source_map::MathContentSourceMap],
    enclosing_range: Option<&std::ops::Range<usize>>,
    index: usize,
) -> Option<usize> {
    if let Some(previous) = index
        .checked_sub(1)
        .and_then(|previous| maps.get(previous))
        .and_then(super::source_map::MathContentSourceMap::range)
    {
        return Some(previous.end);
    }
    maps.get(index)
        .and_then(super::source_map::MathContentSourceMap::range)
        .map(|range| range.start)
        .or_else(|| enclosing_range.map(|range| range.start))
}

fn empty_text_sidecar(anchor: usize) -> super::source_map::MathContentSourceMap {
    use super::source_map::{MathContentSourceMap, MathRangeSourceMap, MathTextSourceMap};
    MathContentSourceMap::Text(MathTextSourceMap::from_parser_parts(
        MathRangeSourceMap::from_parser_range(Some(anchor..anchor), BTreeMap::new()),
        vec![super::source_map::MathTextSourceSegment::from_parser_parts(
            0..0,
            MathRangeSourceMap::from_parser_range(Some(anchor..anchor), BTreeMap::new()),
        )],
    ))
}

fn reconcile_sidecar_texts(
    target: &[MathContent],
    parsed: &[MathContent],
    maps: &[super::source_map::MathContentSourceMap],
) -> Option<Vec<super::source_map::MathContentSourceMap>> {
    use super::source_map::{
        MathContentSourceMap as Map, MathRangeSourceMap, MathTextSourceMap, MathTextSourceSegment,
    };
    let display = |items: &[MathContent]| -> Option<String> {
        let mut result = String::new();
        for item in items {
            let MathContent::Text(text) = item else {
                return None;
            };
            for run in &text.runs {
                result.push_str(&run.text);
            }
        }
        Some(result)
    };
    let value = display(target)?;
    if value != display(parsed)? || parsed.len() != maps.len() {
        return None;
    }
    if target.len() == parsed.len()
        && target.iter().zip(parsed).all(|(a, b)| match (a, b) {
            (MathContent::Text(a), MathContent::Text(b)) => same_display_text(a, b),
            _ => false,
        })
    {
        return maps
            .iter()
            .map(|map| match map {
                Map::Text(text) => Some(Map::Text(text.clone())),
                _ => None,
            })
            .collect();
    }
    if value.is_empty() {
        if target.len() != parsed.len() {
            return None;
        }
        return maps
            .iter()
            .map(|map| match map {
                Map::Text(text) => Some(Map::Text(text.clone())),
                _ => None,
            })
            .collect();
    }
    if target.len() == 1 && parsed.len() == 1 {
        return match &maps[0] {
            Map::Text(text) => Some(vec![Map::Text(text.clone())]),
            _ => None,
        };
    }
    let mut joined_range: Option<std::ops::Range<usize>> = None;
    let mut segments = Vec::new();
    let mut offset = 0usize;
    for (item, map) in parsed.iter().zip(maps) {
        let (MathContent::Text(text), Map::Text(map)) = (item, map) else {
            return None;
        };
        if !map.range.extra.is_empty() {
            return None;
        }
        let range = map.range.range.clone()?;
        if let Some(current) = &mut joined_range {
            if current.end > range.start {
                return None;
            }
            current.end = range.end;
        } else {
            joined_range = Some(range);
        }
        for segment in &map.segments {
            let mut segment = segment.clone();
            segment.display = offset.checked_add(segment.display.start)?
                ..offset.checked_add(segment.display.end)?;
            segments.push(segment);
        }
        offset = text
            .runs
            .iter()
            .try_fold(offset, |n, run| n.checked_add(run.text.len()))?;
    }
    let joined_range = joined_range?;
    // 同一零宽锚点可能归属拆分边界两侧，带扩展时不能猜测其身份。
    if target.len() > 1
        && segments.iter().any(|segment| {
            segment.display.is_empty()
                && (!segment.extra.is_empty() || !segment.source.extra.is_empty())
        })
    {
        return None;
    }
    let mut boundary_sources = Vec::with_capacity(segments.len().saturating_mul(2));
    for segment in &segments {
        let source = segment.source.range.as_ref()?;
        boundary_sources.push((segment.display.start, source.start));
        boundary_sources.push((segment.display.end, source.end));
    }
    boundary_sources.sort_unstable();
    let boundary = |at: usize| -> Option<usize> {
        if at == 0 {
            return Some(joined_range.start);
        }
        if at == value.len() {
            return Some(joined_range.end);
        }
        let first = boundary_sources.partition_point(|(display, _)| *display < at);
        let last = boundary_sources.partition_point(|(display, _)| *display <= at);
        if first < last {
            // 源码注释、转义或相邻文本命令可在同一显示边界留下间隙；
            // 将间隙归入前一段，保证两个相邻目标范围共享一个确定边界。
            return boundary_sources[first..last]
                .iter()
                .map(|(_, source)| *source)
                .max();
        }
        let index = segments.partition_point(|segment| segment.display.start < at);
        let segment = segments.get(index.checked_sub(1)?)?;
        let source = segment.source.range.as_ref()?;
        (segment.display.start < at
            && at < segment.display.end
            && segment.display.len() == source.len())
        .then(|| source.start.checked_add(at - segment.display.start))?
    };
    let mut result = Vec::with_capacity(target.len());
    let mut start = 0usize;
    for (index, item) in target.iter().enumerate() {
        let MathContent::Text(text) = item else {
            return None;
        };
        let end = text
            .runs
            .iter()
            .try_fold(start, |n, run| n.checked_add(run.text.len()))?;
        let range = boundary(start)?..boundary(end)?;
        let mut pieces = Vec::new();
        let first = segments.partition_point(|segment| {
            segment.display.end < start
                || (!segment.display.is_empty() && segment.display.end == start)
        });
        let last = segments.partition_point(|segment| segment.display.start <= end);
        for segment in &segments[first..last] {
            let d = &segment.display;
            if d.is_empty() {
                if (start <= d.start && d.start < end)
                    || (index + 1 == target.len() && d.start == end)
                {
                    let mut piece = segment.clone();
                    piece.display = d.start.checked_sub(start)?..d.end.checked_sub(start)?;
                    pieces.push(piece);
                }
                continue;
            }
            let left = start.max(d.start);
            let right = end.min(d.end);
            if left >= right {
                continue;
            }
            let source = segment.source.range.as_ref()?;
            let split = left != d.start || right != d.end;
            if split
                && (d.len() != source.len()
                    || !segment.extra.is_empty()
                    || !segment.source.extra.is_empty())
            {
                return None;
            }
            let mut piece = segment.clone();
            piece.display = left.checked_sub(start)?..right.checked_sub(start)?;
            if split {
                piece.source.range = Some(
                    source.start.checked_add(left - d.start)?
                        ..source.start.checked_add(right - d.start)?,
                );
            }
            pieces.push(piece);
        }
        if start == end && pieces.is_empty() {
            pieces.push(MathTextSourceSegment::from_parser_parts(
                0..0,
                MathRangeSourceMap::from_parser_range(
                    Some(range.start..range.start),
                    BTreeMap::new(),
                ),
            ));
        }
        result.push(Map::Text(MathTextSourceMap::from_parser_parts(
            MathRangeSourceMap::from_parser_range(Some(range), BTreeMap::new()),
            pieces,
        )));
        start = end;
    }
    Some(result)
}

fn sidecar_flow_has_extra(flow: &super::source_map::MathFlowSourceMap) -> bool {
    !flow.range.extra.is_empty() || sidecar_sequence_has_extra(&flow.content)
}

fn sidecar_sequence_has_extra(content: &[super::source_map::MathContentSourceMap]) -> bool {
    content.iter().any(sidecar_content_has_extra)
}

fn sidecar_content_has_extra(content: &super::source_map::MathContentSourceMap) -> bool {
    use super::source_map::MathContentSourceMap as Map;
    match content {
        Map::Text(text) => {
            !text.range.extra.is_empty()
                || text
                    .segments
                    .iter()
                    .any(|s| !s.extra.is_empty() || !s.source.extra.is_empty())
        }
        Map::Math {
            range, operands, ..
        } => !range.extra.is_empty() || operands.iter().any(sidecar_flow_has_extra),
        Map::Environment { range, children } => {
            !range.extra.is_empty() || sidecar_sequence_has_extra(children)
        }
        Map::Other => false,
    }
}

#[cfg(test)]
mod tests;
