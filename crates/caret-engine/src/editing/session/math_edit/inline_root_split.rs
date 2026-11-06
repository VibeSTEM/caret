//! 将 InlineMath 根流中的文本提升到宿主 Paragraph，并映射活动位置。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::flow_map::OBJECT_REPLACEMENT;
use super::super::super::grapheme::GraphemeMap;
use super::EditError;
use caret_doc::{
    FlowContent, InlineEnvironment, InlineMath, MathContent, MathNodeKind, ParagraphContent,
    TextFlow,
};
use caret_math::{
    MathContentSourceMap, MathInputKind, MathRangeSourceMap, MathTextSourceMap, MathTreeSourceMap,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::editing::session) enum InlineRootFocus {
    Math {
        math_id: caret_doc::NodeId,
        source: String,
        cursor: usize,
        paragraph_offset: usize,
    },
    Text {
        anchor_offset: usize,
        cursor_offset: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::editing::session) struct InlineRootSplit {
    pub(in crate::editing::session) contents: Vec<ParagraphContent>,
    pub(in crate::editing::session) focus: InlineRootFocus,
    pub(in crate::editing::session) inserted_len: usize,
}

/// 根层 TextFlow 是 Paragraph 的内容；MathNode 子树仍是数学槽位边界。
pub(in crate::editing::session) fn split_inline_math_root_with_map(
    math: &InlineMath,
    source_map: &MathTreeSourceMap,
    source_anchor: usize,
    source_cursor: usize,
    source: &str,
) -> Result<Option<InlineRootSplit>, EditError> {
    if !has_promotable_text(&math.math_tree.root.content) {
        return Ok(None);
    }
    if !math.extra.is_empty()
        || !math.math_tree.extra.is_empty()
        || !math.math_tree.root.extra.is_empty()
        || !source_map.root().range_extra().is_empty()
    {
        return Err(EditError::UnsupportedOpaqueContent);
    }
    if source_anchor > source.len()
        || !source.is_char_boundary(source_anchor)
        || source_cursor > source.len()
        || !source.is_char_boundary(source_cursor)
    {
        return Err(EditError::InvalidPosition);
    }

    let mut context = SplitContext {
        source,
        source_anchor,
        source_cursor,
        first_math_id: Some(math.id.clone()),
        first_root_id: Some(math.math_tree.root.id.clone()),
        focus_tokens: Vec::new(),
        paragraph_offset: 0,
        contents: Vec::new(),
    };
    context.convert_items(&math.math_tree.root.content, source_map.root().content())?;

    let focus = context.resolve_focus()?;
    Ok(Some(InlineRootSplit {
        contents: context.contents,
        focus,
        inserted_len: context.paragraph_offset,
    }))
}

#[cfg(test)]
fn split_inline_math_root(
    math: &InlineMath,
    source_anchor: usize,
    source_cursor: usize,
    source: &str,
) -> Result<Option<InlineRootSplit>, EditError> {
    let projection = caret_math::parse_projection(source, MathInputKind::Inline)
        .map_err(|error| EditError::InvalidMath(error.to_string()))?;
    let (caret_math::ProjectedMath::Tree(parsed), projection) = projection.into_parts() else {
        return Err(EditError::InvalidPosition);
    };
    let tree_map = projection.tree().ok_or(EditError::InvalidPosition)?;
    let source_map = tree_map
        .reconcile_for_tree(&parsed, &math.math_tree, None, source, None)
        .ok_or(EditError::InvalidPosition)?;
    split_inline_math_root_with_map(math, &source_map, source_anchor, source_cursor, source)
}

pub(in crate::editing::session) fn replace_inline_math_with_split(
    contents: &mut Vec<ParagraphContent>,
    math_id: &caret_doc::NodeId,
    replacement: Vec<ParagraphContent>,
) -> bool {
    let mut replacement = Some(replacement);
    replace_inline_math_in_place(contents, math_id, &mut replacement)
}

fn replace_inline_math_in_place(
    contents: &mut Vec<ParagraphContent>,
    math_id: &caret_doc::NodeId,
    replacement: &mut Option<Vec<ParagraphContent>>,
) -> bool {
    for index in 0..contents.len() {
        if matches!(&contents[index], FlowContent::Math(math) if &math.id == math_id) {
            let Some(replacement) = replacement.take() else {
                return false;
            };
            contents.splice(index..=index, replacement);
            return true;
        }
        if let FlowContent::Environment(environment) = &mut contents[index]
            && replace_inline_math_in_place(&mut environment.children, math_id, replacement)
        {
            return true;
        }
    }
    false
}

fn has_promotable_text(contents: &[MathContent]) -> bool {
    contents.iter().any(|content| match content {
        MathContent::Text(_) => true,
        MathContent::Environment(environment) => has_promotable_text(&environment.children),
        MathContent::Math(node) if node.kind == MathNodeKind::Row => node
            .children
            .iter()
            .any(|flow| has_promotable_text(&flow.content)),
        MathContent::Math(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
    })
}

struct SplitContext<'a> {
    source: &'a str,
    source_anchor: usize,
    source_cursor: usize,
    first_math_id: Option<caret_doc::NodeId>,
    first_root_id: Option<caret_doc::NodeId>,
    focus_tokens: Vec<FocusToken>,
    paragraph_offset: usize,
    contents: Vec<ParagraphContent>,
}

#[derive(Clone)]
struct FocusToken {
    source_start: usize,
    source_end: usize,
    paragraph_offset: usize,
    kind: FocusTokenKind,
}

#[derive(Clone)]
enum FocusTokenKind {
    Math {
        math_id: caret_doc::NodeId,
        source: String,
    },
    Text {
        source_map: MathTextSourceMap,
        display: String,
    },
}

impl SplitContext<'_> {
    fn resolve_focus(&self) -> Result<InlineRootFocus, EditError> {
        let (anchor_index, anchor_offset) = self
            .nearest_focus_token(self.source_anchor)
            .ok_or(EditError::InvalidPosition)?;
        let (cursor_index, cursor_offset) = self
            .nearest_focus_token(self.source_cursor)
            .ok_or(EditError::InvalidPosition)?;
        if anchor_index != cursor_index {
            return Err(EditError::InvalidPosition);
        }
        match &self.focus_tokens[cursor_index].kind {
            FocusTokenKind::Text { .. } => Ok(InlineRootFocus::Text {
                anchor_offset: self.focus_tokens[cursor_index]
                    .paragraph_offset
                    .checked_add(anchor_offset)
                    .ok_or(EditError::InvalidPosition)?,
                cursor_offset: self.focus_tokens[cursor_index]
                    .paragraph_offset
                    .checked_add(cursor_offset)
                    .ok_or(EditError::InvalidPosition)?,
            }),
            FocusTokenKind::Math { math_id, source } => {
                if anchor_offset != cursor_offset {
                    return Err(EditError::InvalidPosition);
                }
                Ok(InlineRootFocus::Math {
                    math_id: math_id.clone(),
                    source: source.clone(),
                    cursor: cursor_offset,
                    paragraph_offset: self.focus_tokens[cursor_index].paragraph_offset,
                })
            }
        }
    }

    fn nearest_focus_token(&self, source_offset: usize) -> Option<(usize, usize)> {
        self.focus_tokens
            .iter()
            .enumerate()
            .filter_map(|(index, token)| {
                let begins_math_after_text = matches!(&token.kind, FocusTokenKind::Math { .. })
                    && source_offset == token.source_start
                    && index > 0
                    && matches!(
                        &self.focus_tokens[index - 1].kind,
                        FocusTokenKind::Text { .. }
                    );
                if begins_math_after_text {
                    // 归一化会移除 `}`；文本命令后的源码光标应落在正文文本与下一数学原子之间。
                    return None;
                }
                let (distance, prefer_downstream, local_offset) = if source_offset
                    < token.source_start
                {
                    (token.source_start - source_offset, true, 0)
                } else if source_offset > token.source_end {
                    (
                        source_offset - token.source_end,
                        false,
                        match &token.kind {
                            FocusTokenKind::Math { source, .. } => source.len(),
                            FocusTokenKind::Text { display, .. } => display.len(),
                        },
                    )
                } else {
                    let local_offset = match &token.kind {
                        FocusTokenKind::Math { source, .. } => {
                            let offset = source_offset - token.source_start;
                            if !source.is_char_boundary(offset) {
                                return None;
                            }
                            offset
                        }
                        FocusTokenKind::Text {
                            source_map,
                            display,
                        } => source_to_display_offset_in_map(source_map, source_offset)
                            .or_else(|| nearest_source_to_display_in_map(source_map, source_offset))
                            .filter(|offset| *offset <= display.len())?,
                    };
                    (0, source_offset == token.source_start, local_offset)
                };
                let valid_position = match &token.kind {
                    FocusTokenKind::Text { display, .. } => {
                        GraphemeMap::is_boundary_in(display, local_offset)
                    }
                    FocusTokenKind::Math { .. } => true,
                };
                Some((
                    (distance, !prefer_downstream, index),
                    (index, local_offset),
                    valid_position,
                ))
            })
            .min_by_key(|(score, _, _)| *score)
            .and_then(|(_, position, valid)| valid.then_some(position))
    }
}

impl SplitContext<'_> {
    fn convert_items(
        &mut self,
        items: &[MathContent],
        source_maps: &[MathContentSourceMap],
    ) -> Result<(), EditError> {
        let mut math_items = Vec::new();
        self.convert_items_into(items, source_maps, &mut math_items)?;
        self.flush_math(&mut math_items)
    }

    fn convert_items_into(
        &mut self,
        items: &[MathContent],
        source_maps: &[MathContentSourceMap],
        math_items: &mut Vec<(MathContent, MathContentSourceMap)>,
    ) -> Result<(), EditError> {
        if items.len() != source_maps.len() {
            return Err(EditError::InvalidPosition);
        }
        for (item, source_map) in items.iter().zip(source_maps) {
            match (item, source_map) {
                (MathContent::Text(text), MathContentSourceMap::Text(source_map)) => {
                    self.flush_math(math_items)?;
                    self.push_text(text, source_map)?;
                }
                (
                    MathContent::Math(node),
                    MathContentSourceMap::Math {
                        range,
                        operands,
                        placeholder: false,
                    },
                ) if node.kind == MathNodeKind::Row
                    && node
                        .children
                        .iter()
                        .any(|flow| has_promotable_text(&flow.content)) =>
                {
                    if !node.extra.is_empty()
                        || !node.attributes.is_empty()
                        || node.text.is_some()
                        || !range.extra().is_empty()
                        || node.children.len() != operands.len()
                    {
                        return Err(EditError::UnsupportedOpaqueContent);
                    }
                    for (flow, map) in node.children.iter().zip(operands) {
                        if !flow.extra.is_empty() || !map.range_extra().is_empty() {
                            return Err(EditError::UnsupportedOpaqueContent);
                        }
                        self.convert_items_into(&flow.content, map.content(), math_items)?;
                    }
                }
                (
                    MathContent::Environment(environment),
                    MathContentSourceMap::Environment { range, children },
                ) if has_promotable_text(&environment.children) => {
                    self.flush_math(math_items)?;
                    self.push_environment(environment, range, children)?;
                }
                (
                    MathContent::Math(_),
                    MathContentSourceMap::Math { .. } | MathContentSourceMap::Other,
                )
                | (
                    MathContent::Environment(_),
                    MathContentSourceMap::Environment { .. } | MathContentSourceMap::Other,
                ) => {
                    math_items.push((item.clone(), source_map.clone()));
                }
                (
                    MathContent::SoftBreak(_) | MathContent::Opaque(_),
                    MathContentSourceMap::Other,
                ) => {
                    return Err(EditError::UnsupportedOpaqueContent);
                }
                _ => return Err(EditError::InvalidPosition),
            }
        }
        Ok(())
    }

    fn flush_math(
        &mut self,
        items: &mut Vec<(MathContent, MathContentSourceMap)>,
    ) -> Result<(), EditError> {
        if items.is_empty() {
            return Ok(());
        }
        let start = items
            .first()
            .and_then(|(_, source_map)| source_map.range())
            .map(|range| range.start)
            .ok_or(EditError::UnsupportedOpaqueContent)?;
        let end = items
            .last()
            .and_then(|(_, source_map)| source_map.range())
            .map(|range| range.end)
            .ok_or(EditError::UnsupportedOpaqueContent)?;
        let source = self
            .source
            .get(start..end)
            .ok_or(EditError::InvalidPosition)?;
        let mut source = source.to_owned();
        for (item, source_map) in items.iter() {
            let MathContent::Math(node) = item else {
                continue;
            };
            let MathContentSourceMap::Math {
                range,
                placeholder: true,
                ..
            } = source_map
            else {
                continue;
            };
            if node.kind != caret_doc::MathNodeKind::Placeholder {
                continue;
            }
            let Some(range) = range.range() else {
                continue;
            };
            let Some(local_start) = range.start.checked_sub(start) else {
                continue;
            };
            let Some(local_end) = range.end.checked_sub(start) else {
                continue;
            };
            if source.get(local_start..local_end) == Some("$$") {
                // 独立片段中的 `$$` 会被识别为块公式；等长的 `{}` 保留空公式占位语义。
                source.replace_range(local_start..local_end, "{}");
            }
        }
        let caret_math::ProjectedMath::Tree(mut parsed) =
            caret_math::parse_projection(&source, MathInputKind::Inline)
                .map_err(|error| EditError::InvalidMath(error.to_string()))?
                .into_parts()
                .0
        else {
            return Err(EditError::InvalidMath("expected inline math".to_owned()));
        };
        let mut math_items = Vec::with_capacity(items.len());
        for (item, _) in items.iter() {
            math_items.push(item.clone());
        }
        parsed.root.content = math_items;
        if let Some(id) = self.first_root_id.take() {
            parsed.root.id = id;
        }
        let (parsed, _) = crate::math_validation::tree_for_history_projection_with_map(
            &parsed,
            &source,
            MathInputKind::Inline,
        )
        .ok_or_else(|| EditError::InvalidMath("split math source and tree disagree".to_owned()))?;
        let math_id = self
            .first_math_id
            .take()
            .unwrap_or_else(caret_doc::NodeId::generate);
        let tree_source = source.to_owned();
        self.focus_tokens.push(FocusToken {
            source_start: start,
            source_end: end,
            paragraph_offset: self.paragraph_offset,
            kind: FocusTokenKind::Math {
                math_id: math_id.clone(),
                source: tree_source,
            },
        });
        self.contents.push(FlowContent::Math(InlineMath {
            id: math_id,
            math_tree: parsed,
            extra: BTreeMap::new(),
        }));
        self.paragraph_offset = self
            .paragraph_offset
            .checked_add(OBJECT_REPLACEMENT.len_utf8())
            .ok_or(EditError::InvalidPosition)?;
        items.clear();
        Ok(())
    }

    fn push_text(
        &mut self,
        text: &TextFlow,
        source_map: &MathTextSourceMap,
    ) -> Result<(), EditError> {
        if !source_map.range_extra().is_empty()
            || source_map
                .segments()
                .iter()
                .any(|segment| !segment.extra().is_empty() || !segment.source_extra().is_empty())
        {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let promoted = text.clone();
        let display = promoted
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        let display_len = display.len();
        let source_span = source_map
            .range()
            .ok_or(EditError::UnsupportedOpaqueContent)?;
        self.focus_tokens.push(FocusToken {
            source_start: source_span.start,
            source_end: source_span.end,
            paragraph_offset: self.paragraph_offset,
            kind: FocusTokenKind::Text {
                source_map: source_map.clone(),
                display,
            },
        });
        self.paragraph_offset = self
            .paragraph_offset
            .checked_add(display_len)
            .ok_or(EditError::InvalidPosition)?;
        self.contents.push(FlowContent::Text(promoted));
        Ok(())
    }

    fn push_environment(
        &mut self,
        environment: &InlineEnvironment<MathContent>,
        range: &MathRangeSourceMap,
        source_maps: &[MathContentSourceMap],
    ) -> Result<(), EditError> {
        if source_maps.len() != environment.children.len() {
            return Err(EditError::InvalidPosition);
        }
        if !range.extra().is_empty() {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let saved_contents = std::mem::take(&mut self.contents);
        let mut child_context = SplitContext {
            source: self.source,
            source_anchor: self.source_anchor,
            source_cursor: self.source_cursor,
            first_math_id: self.first_math_id.take(),
            first_root_id: self.first_root_id.take(),
            focus_tokens: std::mem::take(&mut self.focus_tokens),
            paragraph_offset: self.paragraph_offset,
            contents: Vec::new(),
        };
        child_context.convert_items(&environment.children, source_maps)?;
        self.paragraph_offset = child_context.paragraph_offset;
        self.first_math_id = child_context.first_math_id;
        self.first_root_id = child_context.first_root_id;
        self.focus_tokens = child_context.focus_tokens;
        self.contents = saved_contents;
        self.contents
            .push(FlowContent::Environment(InlineEnvironment {
                id: environment.id.clone(),
                kind: environment.kind,
                definition_id: environment.definition_id.clone(),
                props: environment.props.clone(),
                children: child_context.contents,
                extra: environment.extra.clone(),
            }));
        Ok(())
    }
}

fn source_to_display_offset_in_map(
    source_map: &MathTextSourceMap,
    source_offset: usize,
) -> Option<usize> {
    if source_map.range().as_ref() == Some(&(source_offset..source_offset))
        && source_map.segments().is_empty()
    {
        return Some(0);
    }
    source_map.segments().iter().find_map(|segment| {
        let display = segment.display();
        let source = segment.source()?;
        let display_start = display.start;
        let display_end = display.end;
        let source_start = source.start;
        let source_end = source.end;
        if source_offset == source_start {
            Some(display_start)
        } else if source_offset == source_end {
            Some(display_end)
        } else if source_start < source_offset
            && source_offset < source_end
            && source_end - source_start == display_end - display_start
        {
            Some(display_start + source_offset - source_start)
        } else {
            None
        }
    })
}

fn nearest_source_to_display_in_map(
    source_map: &MathTextSourceMap,
    source_offset: usize,
) -> Option<usize> {
    source_map
        .segments()
        .iter()
        .flat_map(|segment| {
            let source = segment.source();
            let display = segment.display();
            source.map(|source| [(source.start, display.start), (source.end, display.end)])
        })
        .flatten()
        .min_by_key(|(source, _)| source.abs_diff(source_offset))
        .map(|(_, display)| display)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap_root_in_row(math: &mut InlineMath, flow_count: usize) {
        assert!(flow_count > 0);
        let contents = std::mem::take(&mut math.math_tree.root.content);
        let mut flows = Vec::with_capacity(flow_count);
        let mut offset = 0;
        for index in 0..flow_count {
            let remaining_flows = flow_count - index;
            let remaining_contents = contents.len().saturating_sub(offset);
            let count = remaining_contents.div_ceil(remaining_flows);
            flows.push(caret_doc::MathFlow {
                id: caret_doc::NodeId::generate(),
                content: contents[offset..offset + count].to_vec(),
                extra: BTreeMap::new(),
            });
            offset += count;
        }
        math.math_tree.root.content = vec![MathContent::Math(caret_doc::MathNode {
            id: caret_doc::NodeId::generate(),
            kind: MathNodeKind::Row,
            text: None,
            attributes: BTreeMap::new(),
            children: flows,
            extra: BTreeMap::new(),
        })];
    }

    fn inline_math(source: &str) -> InlineMath {
        let math_tree = match caret_math::parse_projection(source, MathInputKind::Inline) {
            Ok(projection) => match projection.into_parts().0 {
                caret_math::ProjectedMath::Tree(math_tree) => math_tree,
                caret_math::ProjectedMath::Align(_) => panic!("inline source parsed as Align"),
            },
            Err(error) => panic!("valid inline source failed to parse: {error}"),
        };
        InlineMath {
            id: caret_doc::NodeId::generate(),
            math_tree,
            extra: BTreeMap::new(),
        }
    }

    fn split(math: &InlineMath, source: &str, anchor: usize, cursor: usize) -> InlineRootSplit {
        match split_inline_math_root(math, anchor, cursor, source) {
            Ok(Some(split)) => split,
            Ok(None) => panic!("root text should split"),
            Err(error) => panic!("root split should succeed: {error}"),
        }
    }

    #[test]
    fn empty_inline_math_placeholder_keeps_adjacent_math_source_parseable() {
        let projection = caret_math::parse_projection("{}x", MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("source map should parse: {error}"));
        let source_maps = projection
            .source_map()
            .tree()
            .unwrap_or_else(|| panic!("tree source map should exist"))
            .root()
            .content();
        let mut context = SplitContext {
            source: "$$x",
            source_anchor: 3,
            source_cursor: 3,
            first_math_id: Some(caret_doc::NodeId::generate()),
            first_root_id: Some(caret_doc::NodeId::generate()),
            focus_tokens: Vec::new(),
            paragraph_offset: 0,
            contents: Vec::new(),
        };
        let mut items = vec![
            (
                MathContent::Math(caret_doc::MathNode {
                    id: caret_doc::NodeId::generate(),
                    kind: caret_doc::MathNodeKind::Placeholder,
                    text: None,
                    attributes: BTreeMap::new(),
                    children: Vec::new(),
                    extra: BTreeMap::new(),
                }),
                source_maps[0].clone(),
            ),
            (
                MathContent::Math(caret_doc::MathNode {
                    id: caret_doc::NodeId::generate(),
                    kind: caret_doc::MathNodeKind::Symbol,
                    text: Some("x".to_owned()),
                    attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
                    children: Vec::new(),
                    extra: BTreeMap::new(),
                }),
                source_maps[1].clone(),
            ),
        ];

        if let Err(error) = context.flush_math(&mut items) {
            panic!("empty placeholder normalization should succeed: {error}");
        }

        let [ParagraphContent::Math(math)] = context.contents.as_slice() else {
            panic!("the math run should remain one InlineMath item");
        };
        assert_eq!(caret_math::serialize_tree(&math.math_tree), "{}x");
        assert!(matches!(
            math.math_tree.root.content.as_slice(),
            [
                MathContent::Math(placeholder),
                MathContent::Math(symbol)
            ] if placeholder.kind == caret_doc::MathNodeKind::Placeholder
                && symbol.kind == caret_doc::MathNodeKind::Symbol
        ));
    }

    #[test]
    fn root_focus_stays_in_math_when_source_cursor_precedes_text_island() {
        let math = inline_math(r"a\text{x}b");
        let split = split(&math, r"a\text{x}b", 0, 0);
        assert!(matches!(
            split.focus,
            InlineRootFocus::Math {
                source,
                cursor: 0,
                ..
            } if source == "a"
        ));
    }

    #[test]
    fn root_focus_maps_to_the_matching_text_island_among_several() {
        let source = r"a\text{x}b\text{y}c";
        let math = inline_math(source);
        let cursor = source
            .find('y')
            .unwrap_or_else(|| panic!("second text island"))
            + 1;
        let split = split(&math, source, cursor, cursor);
        assert_eq!(
            split.focus,
            InlineRootFocus::Text {
                anchor_offset: 8,
                cursor_offset: 8,
            }
        );
    }

    #[test]
    fn root_focus_maps_removed_text_command_prefix_to_its_start() {
        let math = inline_math(r"\text{x}");
        let split = split(&math, r"\text{x}", 0, 0);
        assert!(matches!(
            split.focus,
            InlineRootFocus::Text {
                anchor_offset: 0,
                cursor_offset: 0,
            }
        ));
    }

    #[test]
    fn transparent_root_row_promotes_text_and_preserves_mixed_order() {
        let source = r"x+\text{T}y";
        let mut math = inline_math(source);
        wrap_root_in_row(&mut math, 2);
        let cursor = source.find('T').unwrap_or_else(|| panic!("text island")) + 1;

        let split = split(&math, source, cursor, cursor);
        assert!(matches!(
            split.contents.as_slice(),
            [
                ParagraphContent::Math(_),
                ParagraphContent::Text(_),
                ParagraphContent::Math(_)
            ]
        ));
        assert_eq!(
            split.focus,
            InlineRootFocus::Text {
                anchor_offset: 4,
                cursor_offset: 4,
            }
        );
    }

    #[test]
    fn transparent_root_row_merges_math_across_child_flow_boundaries() {
        let source = r"x+y+\text{T}";
        let mut math = inline_math(source);
        wrap_root_in_row(&mut math, 3);
        let cursor = source.find('T').unwrap_or_else(|| panic!("text island")) + 1;

        let split = split(&math, source, cursor, cursor);
        let [ParagraphContent::Math(math), ParagraphContent::Text(_)] = split.contents.as_slice()
        else {
            panic!("consecutive math across Row flows should remain one InlineMath");
        };
        assert_eq!(caret_math::serialize_tree(&math.math_tree), "x+y+");
    }

    #[test]
    fn transparent_root_row_does_not_promote_text_from_fraction_slots() {
        let source = r"\frac{\text{x}}{1}";
        let mut math = inline_math(source);
        wrap_root_in_row(&mut math, 1);

        assert_eq!(split_inline_math_root(&math, 0, 0, source), Ok(None));
    }

    #[test]
    fn stale_sidecar_fails_without_changing_the_math_tree() {
        let source = r"\text{x}";
        let mut math = inline_math(source);
        wrap_root_in_row(&mut math, 1);
        let original = math.math_tree.clone();
        let projection = caret_math::parse_projection("different", MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("stale source should parse: {error}"));
        let stale_map = projection
            .source_map()
            .tree()
            .unwrap_or_else(|| panic!("tree sidecar should exist"));

        assert!(split_inline_math_root_with_map(&math, stale_map, 0, 0, source).is_err());
        assert_eq!(math.math_tree, original);
    }

    #[test]
    fn root_split_rejects_selection_spanning_different_output_items() {
        let source = r"a\text{x}b";
        let math = inline_math(source);
        let cursor = source.find('x').unwrap_or_else(|| panic!("text island")) + 1;
        assert_eq!(
            split_inline_math_root(
                &math,
                0,
                cursor,
                &caret_math::serialize_tree(&math.math_tree)
            ),
            Err(EditError::InvalidPosition)
        );
    }

    #[test]
    fn root_split_keeps_one_format_wrapper_around_mixed_math_and_text_children() {
        let source = r"\text{\textbf{$a\text{x}b$}}";
        let math = inline_math(source);
        let Some(MathContent::Environment(environment)) = math.math_tree.root.content.first()
        else {
            panic!("expected a parsed bold environment");
        };
        let environment_id = environment.id.clone();

        let cursor = source.find('x').unwrap_or_else(|| panic!("text island")) + 1;
        let split = split(&math, source, cursor, cursor);
        let [ParagraphContent::Environment(environment)] = split.contents.as_slice() else {
            panic!("split should preserve a single wrapper");
        };
        assert_eq!(environment.id, environment_id);
        assert_eq!(environment.kind, caret_doc::InlineEnvironmentKind::Bold);
        assert!(matches!(
            environment.children.as_slice(),
            [
                ParagraphContent::Math(_),
                ParagraphContent::Text(_),
                ParagraphContent::Math(_)
            ]
        ));
    }

    #[test]
    fn root_split_preserves_run_properties_and_maps_escaped_unicode_text() {
        let source = "a\\text{\\textchar{5F}e\u{301}}b";
        let mut math = inline_math(source);
        let Some(MathContent::Text(text)) = math
            .math_tree
            .root
            .content
            .iter_mut()
            .find(|item| matches!(item, MathContent::Text(_)))
        else {
            panic!("expected root text flow");
        };
        text.runs[0]
            .props
            .insert("underline".to_owned(), serde_json::Value::Bool(true));

        let cursor = source
            .find("5F")
            .unwrap_or_else(|| panic!("escaped text character"))
            + 1;
        let split = split(&math, source, cursor, cursor);
        let Some(ParagraphContent::Text(text)) = split.contents.get(1) else {
            panic!("expected promoted text flow");
        };
        assert_eq!(
            text.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "_e\u{301}"
        );
        assert_eq!(
            text.runs[0].props.get("underline"),
            Some(&serde_json::Value::Bool(true))
        );
        assert_eq!(
            split.focus,
            InlineRootFocus::Text {
                anchor_offset: 4,
                cursor_offset: 4,
            }
        );
        assert_eq!(split.inserted_len, 3 + "_e\u{301}".len() + 3);
    }

    #[test]
    fn root_split_preserves_metadata_in_unpromoted_math_text() {
        fn mark_first_text(content: &mut [MathContent]) -> bool {
            for item in content {
                match item {
                    MathContent::Text(text) => {
                        let Some(run) = text.runs.first_mut() else {
                            continue;
                        };
                        run.props
                            .insert("underline".to_owned(), serde_json::Value::Bool(true));
                        text.extra.insert(
                            "vendorText".to_owned(),
                            serde_json::Value::String("kept".to_owned()),
                        );
                        return true;
                    }
                    MathContent::Math(node) => {
                        for child in &mut node.children {
                            if mark_first_text(&mut child.content) {
                                return true;
                            }
                        }
                    }
                    MathContent::Environment(environment) => {
                        if mark_first_text(&mut environment.children) {
                            return true;
                        }
                    }
                    MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
                }
            }
            false
        }

        fn has_marked_text(content: &[MathContent]) -> bool {
            content.iter().any(|item| match item {
                MathContent::Text(text) => {
                    text.runs.first().is_some_and(|run| {
                        run.props.get("underline") == Some(&serde_json::Value::Bool(true))
                    }) && text.extra.get("vendorText")
                        == Some(&serde_json::Value::String("kept".to_owned()))
                }
                MathContent::Math(node) => node
                    .children
                    .iter()
                    .any(|child| has_marked_text(&child.content)),
                MathContent::Environment(environment) => has_marked_text(&environment.children),
                MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
            })
        }

        let source = r"\frac{\text{a}\text{b}}{x}\text{c}";
        let mut math = inline_math(source);
        assert!(mark_first_text(&mut math.math_tree.root.content));
        let cursor = source
            .find(r"\text{c}")
            .unwrap_or_else(|| panic!("root text flow"))
            + r"\text{".len()
            + 1;

        let split = split(&math, source, cursor, cursor);
        let Some(ParagraphContent::Math(unpromoted_math)) = split.contents.first() else {
            panic!("fraction should stay in the math tree");
        };
        assert!(has_marked_text(&unpromoted_math.math_tree.root.content));
        assert!(matches!(
            split.contents.get(1),
            Some(ParagraphContent::Text(_))
        ));
    }

    #[test]
    fn root_split_rebinds_adjacent_operand_text_flows_to_the_fragment_map() {
        let source = r"\text{c}\frac{\text{a}\text{b}}{x}";
        let math = inline_math(source);
        let split = split(&math, source, r"\text{c}".len(), r"\text{c}".len());
        let Some(ParagraphContent::Math(fragment)) = split.contents.last() else {
            panic!("the fraction should remain in an InlineMath fragment");
        };
        let Some(MathContent::Math(fraction)) = fragment.math_tree.root.content.first() else {
            panic!("the retained fragment should start with a fraction");
        };
        assert!(fraction.children.iter().any(|flow| {
            flow.content
                .iter()
                .filter(|content| matches!(content, MathContent::Text(_)))
                .count()
                == 2
        }));

        let fragment_source = source
            .strip_prefix(r"\text{c}")
            .unwrap_or_else(|| panic!("the fragment source should follow the promoted text"));
        let projection = caret_math::parse_projection(fragment_source, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("fragment should reparse: {error}"));
        let (caret_math::ProjectedMath::Tree(parsed), mut source_projection) =
            projection.into_parts()
        else {
            panic!("fragment should parse as a tree");
        };
        let parsed_map = source_projection
            .tree()
            .cloned()
            .unwrap_or_else(|| panic!("fragment sidecar should exist"));
        let mapped = parsed_map
            .reconcile_for_tree(&parsed, &fragment.math_tree, None, fragment_source, None)
            .unwrap_or_else(|| panic!("fragment sidecar should bind to its tree"));
        assert!(
            source_projection.bind_tree_sidecar_after_validated_edit(&fragment.math_tree, mapped)
        );
    }

    #[test]
    fn root_split_rejects_a_source_position_inside_a_promoted_grapheme() {
        let source = "a\\text{e\u{301}}b";
        let math = inline_math(source);
        let cursor = source
            .find("e\u{301}")
            .unwrap_or_else(|| panic!("combined grapheme"))
            + 1;
        assert_eq!(
            split_inline_math_root(
                &math,
                cursor,
                cursor,
                &caret_math::serialize_tree(&math.math_tree)
            ),
            Err(EditError::InvalidPosition)
        );
    }
}
