//! 将 InlineMath 根流中的文本提升到宿主 Paragraph，并映射活动位置。
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::super::flow_map::OBJECT_REPLACEMENT;
use super::super::super::grapheme::GraphemeMap;
use super::EditError;
use caret_doc::{
    FlowContent, InlineEnvironment, InlineMath, MathContent, MathNode, MathSourceRange, MathTree,
    ParagraphContent, TextFlow,
};
use caret_math::MathInputKind;
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
pub(in crate::editing::session) fn split_inline_math_root(
    math: &InlineMath,
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
        || math
            .math_tree
            .root
            .source_range
            .as_ref()
            .is_some_and(|range| !range.extra.is_empty())
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
    context.convert_items(&math.math_tree.root.content)?;

    let focus = context.resolve_focus()?;
    Ok(Some(InlineRootSplit {
        contents: context.contents,
        focus,
        inserted_len: context.paragraph_offset,
    }))
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
        source_map: Vec<caret_doc::TextFlowSourceMapEntry>,
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
    fn convert_items(&mut self, items: &[MathContent]) -> Result<(), EditError> {
        let mut math_items = Vec::new();
        for item in items {
            match item {
                MathContent::Text(text) => {
                    self.flush_math(&mut math_items)?;
                    self.push_text(text)?;
                }
                MathContent::Environment(environment)
                    if has_promotable_text(&environment.children) =>
                {
                    self.flush_math(&mut math_items)?;
                    self.push_environment(environment)?;
                }
                MathContent::Math(_) | MathContent::Environment(_) => {
                    math_items.push(item.clone());
                }
                MathContent::SoftBreak(_) | MathContent::Opaque(_) => {
                    return Err(EditError::UnsupportedOpaqueContent);
                }
            }
        }
        self.flush_math(&mut math_items)
    }

    fn flush_math(&mut self, items: &mut Vec<MathContent>) -> Result<(), EditError> {
        if items.is_empty() {
            return Ok(());
        }
        let start = items
            .first()
            .and_then(math_content_source_range)
            .map(|range| range.start as usize)
            .ok_or(EditError::UnsupportedOpaqueContent)?;
        let end = items
            .last()
            .and_then(math_content_source_range)
            .map(|range| range.end as usize)
            .ok_or(EditError::UnsupportedOpaqueContent)?;
        let source = self
            .source
            .get(start..end)
            .ok_or(EditError::InvalidPosition)?;
        let mut source = source.to_owned();
        for item in items.iter() {
            let MathContent::Math(node) = item else {
                continue;
            };
            if node.kind != caret_doc::MathNodeKind::Placeholder {
                continue;
            }
            let Some(range) = node.source_range.as_ref() else {
                continue;
            };
            let Some(local_start) = (range.start as usize).checked_sub(start) else {
                continue;
            };
            let Some(local_end) = (range.end as usize).checked_sub(start) else {
                continue;
            };
            if source.get(local_start..local_end) == Some("$$") {
                // 独立片段中的 `$$` 会被识别为块公式；等长的 `{}` 保留空公式占位语义。
                source.replace_range(local_start..local_end, "{}");
            }
        }
        let caret_math::ParsedMath::Tree(mut parsed) =
            caret_math::parse(&source, MathInputKind::Inline)
                .map_err(|error| EditError::InvalidMath(error.to_string()))?
        else {
            return Err(EditError::InvalidMath("expected inline math".to_owned()));
        };
        for item in items.iter_mut() {
            rebase_content_ranges(item, start)?;
        }
        parsed.root.content = std::mem::take(items);
        if let Some(id) = self.first_root_id.take() {
            parsed.root.id = id;
        }
        if caret_engine_math_tree_check(&parsed).is_none() {
            return Err(EditError::InvalidMath(
                "split math source and tree disagree".to_owned(),
            ));
        }
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
        Ok(())
    }

    fn push_text(&mut self, text: &TextFlow) -> Result<(), EditError> {
        if text
            .source_range
            .as_ref()
            .is_some_and(|range| !range.extra.is_empty())
            || text
                .source_map
                .iter()
                .any(|entry| !entry.extra.is_empty() || !entry.source_range.extra.is_empty())
        {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let mut promoted = text.clone();
        let display = promoted
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        let display_len = display.len();
        let source_span = source_span(&promoted).ok_or(EditError::UnsupportedOpaqueContent)?;
        self.focus_tokens.push(FocusToken {
            source_start: source_span.0,
            source_end: source_span.1,
            paragraph_offset: self.paragraph_offset,
            kind: FocusTokenKind::Text {
                source_map: promoted.source_map.clone(),
                display,
            },
        });
        promoted.source_range = None;
        promoted.source_map.clear();
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
    ) -> Result<(), EditError> {
        if environment
            .source_range
            .as_ref()
            .is_some_and(|range| !range.extra.is_empty())
        {
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
        child_context.convert_items(&environment.children)?;
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
                source_range: None,
                props: environment.props.clone(),
                children: child_context.contents,
                extra: environment.extra.clone(),
            }));
        Ok(())
    }
}

fn math_content_source_range(content: &MathContent) -> Option<&MathSourceRange> {
    match content {
        MathContent::Math(node) => node.source_range.as_ref(),
        MathContent::Environment(environment) => environment.source_range.as_ref(),
        MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => None,
    }
}

fn source_to_display_offset_in_map(
    source_map: &[caret_doc::TextFlowSourceMapEntry],
    source_offset: usize,
) -> Option<usize> {
    source_map.iter().find_map(|entry| {
        let display_start = entry.text_start as usize;
        let display_end = entry.text_end as usize;
        let source_start = entry.source_range.start as usize;
        let source_end = entry.source_range.end as usize;
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
    source_map: &[caret_doc::TextFlowSourceMapEntry],
    source_offset: usize,
) -> Option<usize> {
    source_map
        .iter()
        .flat_map(|entry| {
            [
                (entry.source_range.start as usize, entry.text_start as usize),
                (entry.source_range.end as usize, entry.text_end as usize),
            ]
        })
        .min_by_key(|(source, _)| source.abs_diff(source_offset))
        .map(|(_, display)| display)
}

fn source_span(text: &TextFlow) -> Option<(usize, usize)> {
    let start = text
        .source_map
        .iter()
        .map(|entry| entry.source_range.start as usize)
        .min()?;
    let end = text
        .source_map
        .iter()
        .map(|entry| entry.source_range.end as usize)
        .max()?;
    Some((start, end))
}

fn rebase_content_ranges(content: &mut MathContent, offset: usize) -> Result<(), EditError> {
    match content {
        MathContent::Math(node) => rebase_node_ranges(node, offset),
        MathContent::Environment(environment) => {
            rebase_range(&mut environment.source_range, offset)?;
            for child in &mut environment.children {
                rebase_content_ranges(child, offset)?;
            }
            Ok(())
        }
        MathContent::Text(text) => {
            rebase_range(&mut text.source_range, offset)?;
            for entry in &mut text.source_map {
                entry.source_range.start = entry
                    .source_range
                    .start
                    .checked_sub(offset as u32)
                    .ok_or(EditError::InvalidPosition)?;
                entry.source_range.end = entry
                    .source_range
                    .end
                    .checked_sub(offset as u32)
                    .ok_or(EditError::InvalidPosition)?;
            }
            Ok(())
        }
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => {
            Err(EditError::UnsupportedOpaqueContent)
        }
    }
}

fn rebase_node_ranges(node: &mut MathNode, offset: usize) -> Result<(), EditError> {
    rebase_range(&mut node.source_range, offset)?;
    for child in &mut node.children {
        rebase_range(&mut child.source_range, offset)?;
        for content in &mut child.content {
            rebase_content_ranges(content, offset)?;
        }
    }
    Ok(())
}

fn rebase_range(range: &mut Option<MathSourceRange>, offset: usize) -> Result<(), EditError> {
    if let Some(range) = range {
        range.start = range
            .start
            .checked_sub(offset as u32)
            .ok_or(EditError::InvalidPosition)?;
        range.end = range
            .end
            .checked_sub(offset as u32)
            .ok_or(EditError::InvalidPosition)?;
    }
    Ok(())
}

fn caret_engine_math_tree_check(tree: &MathTree) -> Option<MathTree> {
    crate::math_validation::parse_tree_with_same_semantics(tree, MathInputKind::Inline)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inline_math(source: &str) -> InlineMath {
        let math_tree = match caret_math::parse(source, MathInputKind::Inline) {
            Ok(caret_math::ParsedMath::Tree(math_tree)) => math_tree,
            Ok(caret_math::ParsedMath::Align(_)) => panic!("inline source parsed as Align"),
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
            MathContent::Math(MathNode {
                id: caret_doc::NodeId::generate(),
                kind: caret_doc::MathNodeKind::Placeholder,
                text: None,
                attributes: BTreeMap::new(),
                source_range: Some(MathSourceRange {
                    start: 0,
                    end: 2,
                    extra: BTreeMap::new(),
                }),
                children: Vec::new(),
                extra: BTreeMap::new(),
            }),
            MathContent::Math(MathNode {
                id: caret_doc::NodeId::generate(),
                kind: caret_doc::MathNodeKind::Symbol,
                text: Some("x".to_owned()),
                attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
                source_range: Some(MathSourceRange {
                    start: 2,
                    end: 3,
                    extra: BTreeMap::new(),
                }),
                children: Vec::new(),
                extra: BTreeMap::new(),
            }),
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
        let source = r"a\text{x}b";
        let mut math = inline_math(source);
        let environment_id = caret_doc::NodeId::generate();
        let children = std::mem::take(&mut math.math_tree.root.content);
        math.math_tree
            .root
            .content
            .push(MathContent::Environment(InlineEnvironment {
                id: environment_id.clone(),
                kind: caret_doc::InlineEnvironmentKind::Bold,
                definition_id: None,
                source_range: Some(MathSourceRange {
                    start: 0,
                    end: source.len() as u32,
                    extra: BTreeMap::new(),
                }),
                props: BTreeMap::new(),
                children,
                extra: BTreeMap::new(),
            }));

        let cursor = source.find('x').unwrap_or_else(|| panic!("text island")) + 1;
        let split = split(&math, source, cursor, cursor);
        let [ParagraphContent::Environment(environment)] = split.contents.as_slice() else {
            panic!("split should preserve a single wrapper");
        };
        assert_eq!(environment.id, environment_id);
        assert_eq!(environment.kind, caret_doc::InlineEnvironmentKind::Bold);
        assert!(environment.source_range.is_none());
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
