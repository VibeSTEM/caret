//! 公式编辑状态的行为测试。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::text_branch::{text_source_boundary_from_map, text_source_range_from_map};
use super::*;

fn source_text(edit: &MathEdit) -> &str {
    edit.source()
        .unwrap_or_else(|| panic!("source text is available only in source-edit mode"))
}

fn first_align(session: &Session) -> Result<&Align, EditError> {
    session
        .document()
        .body
        .iter()
        .find_map(|block| match block {
            Block::Align(align) => Some(align),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)
}

fn assert_align_edit_target(
    session: &Session,
    block_id: &NodeId,
    row: usize,
    cell: usize,
) -> Result<(), EditError> {
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(matches!(
        &edit.target,
        MathEditTarget::Block {
            block_id: active_id,
            align: true,
        } if active_id == block_id
    ));
    assert_eq!(
        edit.align_cell_at_cursor()
            .map(|(active_row, active_cell, _)| (active_row, active_cell)),
        Some((row, cell))
    );
    Ok(())
}

fn source_projection(edit: &MathEdit) -> &Arc<caret_math::MathSourceMap<'static>> {
    edit.source_projection
        .as_ref()
        .unwrap_or_else(|| panic!("source projection is available only in source-edit mode"))
}

fn source_position_projection(
    position: &MathEditPosition,
) -> &Arc<caret_math::MathSourceMap<'static>> {
    position
        .source_projection
        .as_ref()
        .unwrap_or_else(|| panic!("source position is available only in source-edit mode"))
}

fn math_content_range_at(
    edit: &MathEdit,
    path: &[caret_doc::MathPathStep],
) -> Option<std::ops::Range<usize>> {
    edit.source_projection
        .as_deref()?
        .tree()?
        .content_at(path)?
        .range()
}

fn first_text_source_range(
    flow: &MathFlow,
    map: &caret_math::MathFlowSourceMap,
) -> Option<std::ops::Range<usize>> {
    first_text_source_range_in_items(&flow.content, map.content())
}

fn first_text_source_range_in_items(
    items: &[MathContent],
    maps: &[caret_math::MathContentSourceMap],
) -> Option<std::ops::Range<usize>> {
    for (content, source_map) in items.iter().zip(maps) {
        match (content, source_map) {
            (MathContent::Text(_), caret_math::MathContentSourceMap::Text(text)) => {
                return text.range();
            }
            (MathContent::Math(node), caret_math::MathContentSourceMap::Math { operands, .. }) => {
                for (child, child_map) in node.children.iter().zip(operands) {
                    if let Some(range) = first_text_source_range(child, child_map) {
                        return Some(range);
                    }
                }
            }
            (
                MathContent::Environment(environment),
                caret_math::MathContentSourceMap::Environment { children, .. },
            ) => {
                if let Some(range) =
                    first_text_source_range_in_items(&environment.children, children)
                {
                    return Some(range);
                }
            }
            _ => {}
        }
    }
    None
}

fn projected_align_cell_at(source: &str, cursor: usize) -> Option<(usize, usize, usize)> {
    let wrapped = format!("\\begin{{aligned}}{source}\\end{{aligned}}");
    let projection =
        caret_math::parse_projection(&wrapped, caret_math::MathInputKind::Align).ok()?;
    align_cell_at(projection.source_map(), cursor)
}

fn aligned_body_offset(align: &Align) -> usize {
    let source = caret_math::serialize_align(align);
    let Ok(projection) = caret_math::parse_projection(&source, caret_math::MathInputKind::Align)
    else {
        panic!("Align source should produce an edit projection");
    };
    projection.source_map().edit_range().start
}

fn aligned_body_len(align: &Align) -> usize {
    let source = caret_math::serialize_align(align);
    let projection = caret_math::parse_projection(&source, caret_math::MathInputKind::Align)
        .unwrap_or_else(|error| panic!("Align source should produce an edit projection: {error}"));
    projection.source_map().edit_source().len()
}

fn aligned_cell_range(align: &Align, row: usize, cell: usize) -> std::ops::Range<usize> {
    let source = caret_math::serialize_align(align);
    let projection = caret_math::parse_projection(&source, caret_math::MathInputKind::Align)
        .unwrap_or_else(|error| panic!("Align source should produce an edit projection: {error}"));
    projection
        .source_map()
        .align()
        .and_then(|map| map.cell_range(row, cell))
        .unwrap_or_else(|| panic!("Align cell should have a source range"))
}

fn preserve_align_metadata_for_test(
    previous: &Align,
    updated: &mut Align,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
) -> bool {
    let previous_source = caret_math::serialize_align(previous);
    let updated_source = caret_math::serialize_align(updated);
    let Ok(previous_projection) =
        caret_math::parse_projection(&previous_source, caret_math::MathInputKind::Align)
    else {
        return false;
    };
    let Ok(updated_projection) =
        caret_math::parse_projection(&updated_source, caret_math::MathInputKind::Align)
    else {
        return false;
    };
    preserve_align_metadata(
        previous,
        updated,
        old_edit,
        new_edit,
        previous_projection.source_map(),
        updated_projection.source_map(),
    )
}

fn assert_math_document_is_not_yet_saveable(document: &caret_doc::Document) {
    assert!(matches!(
        caret_doc::CdocFile::new(document.clone()),
        Err(caret_doc::DocumentError::ReadOnlyPreview)
    ));
}

fn enter_source_edit(session: &mut Session, direction: Direction) -> Result<(), EditError> {
    if session.math_edit().is_none() {
        session.run(Command::EnterMath(direction))?;
    }
    let edit = session
        .math_edit
        .clone()
        .ok_or(EditError::InvalidPosition)?;
    let kind = match &edit.target {
        MathEditTarget::Inline { .. } => caret_math::MathInputKind::Inline,
        MathEditTarget::Block { align: true, .. } => caret_math::MathInputKind::Align,
        MathEditTarget::Block { align: false, .. } => caret_math::MathInputKind::Equation,
    };
    let projection = match &edit.target {
        MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } => session
            .document()
            .body
            .iter()
            .find_map(|block| match block {
                Block::Paragraph(paragraph) if &paragraph.id == paragraph_id => {
                    paragraph.inlines.iter().find_map(|inline| match inline {
                        ParagraphContent::Math(math) if &math.id == math_id => {
                            Some(math.math_tree.clone())
                        }
                        _ => None,
                    })
                }
                _ => None,
            })
            .ok_or(EditError::InvalidPosition)
            .and_then(|tree| {
                crate::math_validation::project_tree_for_edit(&tree, kind)
                    .map(|(projection, _)| projection)
                    .map_err(|_| EditError::InvalidPosition)
            })?,
        MathEditTarget::Block { block_id, align } => {
            let block = session
                .document()
                .body
                .iter()
                .find(|block| match block {
                    Block::Equation(value) => !align && &value.id == block_id,
                    Block::Align(value) => *align && &value.id == block_id,
                    _ => false,
                })
                .cloned()
                .ok_or(EditError::InvalidPosition)?;
            match block {
                Block::Equation(equation) if !align => {
                    let (projection, _) =
                        crate::math_validation::project_tree_for_edit(&equation.math_tree, kind)
                            .map_err(EditError::InvalidMath)?;
                    projection
                }
                Block::Align(value) if *align => {
                    if let Ok((projection, _)) =
                        crate::math_validation::project_align_for_edit(&value)
                    {
                        projection
                    } else {
                        let source = caret_math::serialize_align(&value);
                        let parsed =
                            caret_math::parse_projection(&source, caret_math::MathInputKind::Align)
                                .map_err(|error| EditError::InvalidMath(error.to_string()))?;
                        let (caret_math::ProjectedMath::Align(mut projected), projection) =
                            parsed.into_parts()
                        else {
                            return Err(EditError::InvalidPosition);
                        };
                        projected.id = value.id.clone();
                        let Some(Block::Align(target)) = session.document.body.iter_mut().find(
                            |block| matches!(block, Block::Align(align) if align.id == value.id),
                        ) else {
                            return Err(EditError::InvalidPosition);
                        };
                        *target = projected;
                        projection
                    }
                }
                _ => return Err(EditError::InvalidPosition),
            }
        }
    };
    let range = projection.body_range();
    let cursor = match direction {
        Direction::Forward => range.start,
        Direction::Backward => range.end,
    };
    let current = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    current.source_projection = Some(std::sync::Arc::new(projection));
    current.source_draft = None;
    current.cursor = cursor;
    current.branch = MathEditBranch::Source;
    Ok(())
}

fn first_paragraph_text(session: &Session) -> String {
    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        panic!("first block should be a paragraph");
    };
    TextMap::new(paragraph)
        .unwrap_or_else(|error| panic!("paragraph mapping failed: {error}"))
        .text
}

fn math_tree_has_font_environment(flow: &MathFlow) -> bool {
    flow.content.iter().any(math_content_has_font_environment)
}

fn text_flow_display(text: &caret_doc::TextFlow) -> String {
    text.runs.iter().map(|run| run.text.as_str()).collect()
}

fn math_content_has_symbol_text(content: &MathContent, text: &str) -> bool {
    match content {
        MathContent::Math(node) => {
            (node.kind == MathNodeKind::Symbol && node.text.as_deref() == Some(text))
                || node.children.iter().any(|flow| {
                    flow.content
                        .iter()
                        .any(|content| math_content_has_symbol_text(content, text))
                })
        }
        MathContent::Environment(environment) => environment
            .children
            .iter()
            .any(|content| math_content_has_symbol_text(content, text)),
        _ => false,
    }
}

fn math_content_has_font_environment(content: &MathContent) -> bool {
    match content {
        MathContent::Environment(environment) => {
            environment.kind == caret_doc::InlineEnvironmentKind::Font
                || environment
                    .children
                    .iter()
                    .any(math_content_has_font_environment)
        }
        MathContent::Math(node) => node.children.iter().any(math_tree_has_font_environment),
        MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
    }
}

fn math_tree_has_font_environment_for(flow: &MathFlow, family: &str, text: &str) -> bool {
    flow.content
        .iter()
        .any(|content| math_content_has_font_environment_for(content, family, text))
}

fn math_content_has_font_environment_for(content: &MathContent, family: &str, text: &str) -> bool {
    match content {
        MathContent::Environment(environment) => {
            let display = environment
                .children
                .iter()
                .filter_map(first_math_content_text)
                .flat_map(|flow| flow.runs.iter().map(|run| run.text.as_str()))
                .collect::<String>();
            (environment.kind == caret_doc::InlineEnvironmentKind::Font
                && environment
                    .props
                    .get("fontFamily")
                    .and_then(serde_json::Value::as_str)
                    == Some(family)
                && display.contains(text))
                || environment
                    .children
                    .iter()
                    .any(|child| math_content_has_font_environment_for(child, family, text))
        }
        MathContent::Math(node) => node
            .children
            .iter()
            .any(|child| math_tree_has_font_environment_for(child, family, text)),
        MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
    }
}

fn first_math_text_flow_mut(flow: &mut MathFlow) -> Option<&mut caret_doc::TextFlow> {
    flow.content
        .iter_mut()
        .find_map(first_math_content_text_mut)
}

fn first_math_text_flow(flow: &MathFlow) -> Option<&caret_doc::TextFlow> {
    flow.content.iter().find_map(first_math_content_text)
}

fn math_flow_has_placeholder(flow: &MathFlow) -> bool {
    flow.content.iter().any(|content| match content {
        MathContent::Math(node) if node.kind == caret_doc::MathNodeKind::Placeholder => true,
        MathContent::Math(node) => node.children.iter().any(math_flow_has_placeholder),
        MathContent::Environment(environment) => environment
            .children
            .iter()
            .any(math_content_has_placeholder),
        MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
    })
}

fn math_content_has_placeholder(content: &MathContent) -> bool {
    match content {
        MathContent::Math(node) if node.kind == caret_doc::MathNodeKind::Placeholder => true,
        MathContent::Math(node) => node.children.iter().any(math_flow_has_placeholder),
        MathContent::Environment(environment) => environment
            .children
            .iter()
            .any(math_content_has_placeholder),
        MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
    }
}

fn first_math_content_text(content: &MathContent) -> Option<&caret_doc::TextFlow> {
    match content {
        MathContent::Text(text) => Some(text),
        MathContent::Math(node) => node
            .children
            .iter()
            .find_map(|child| child.content.iter().find_map(first_math_content_text)),
        MathContent::Environment(environment) => environment
            .children
            .iter()
            .find_map(first_math_content_text),
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => None,
    }
}

fn active_math_text_flow(session: &Session) -> Result<&caret_doc::TextFlow, EditError> {
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    path.resolve(session.math_tree_for_edit(edit, path.align_cell())?)
        .ok_or(EditError::InvalidPosition)
}

fn first_math_content_text_mut(content: &mut MathContent) -> Option<&mut caret_doc::TextFlow> {
    match content {
        MathContent::Text(text) => Some(text),
        MathContent::Math(node) => node.children.iter_mut().find_map(first_math_text_flow_mut),
        MathContent::Environment(environment) => environment
            .children
            .iter_mut()
            .find_map(first_math_content_text_mut),
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => None,
    }
}

fn first_math_environment_mut(
    flow: &mut MathFlow,
) -> Option<&mut caret_doc::InlineEnvironment<MathContent>> {
    flow.content
        .iter_mut()
        .find_map(first_math_content_environment_mut)
}

fn first_math_content_environment_mut(
    content: &mut MathContent,
) -> Option<&mut caret_doc::InlineEnvironment<MathContent>> {
    match content {
        MathContent::Environment(environment) => Some(environment),
        MathContent::Math(node) => node
            .children
            .iter_mut()
            .find_map(first_math_environment_mut),
        MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => None,
    }
}

#[test]
fn formula_cursor_uses_the_same_grapheme_boundaries_as_text() {
    let source = "\\alpha a\u{301}β";
    let mut stops = vec![0];
    let mut cursor = 0;
    while let Some(next) = next_grapheme_end(source, cursor) {
        stops.push(next);
        cursor = next;
    }
    assert_eq!(stops, vec![0, 1, 2, 3, 4, 5, 6, 7, 10, 12]);
    assert_eq!(previous_grapheme_start(source, 10), Some(7));
}

#[test]
fn discontinuous_text_source_map_never_deletes_the_gap_between_entries() {
    let source = "\\text{ab% skipped by TeX\ncd}";
    let projection = caret_math::parse_projection(source, caret_math::MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("text source should project: {error}"));
    let caret_math::ProjectedMath::Tree(tree) = projection.content() else {
        panic!("inline text should project to a math tree");
    };
    let Some(MathContent::Text(text)) = tree.root.content.first() else {
        panic!("source should project to a TextFlow");
    };
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abcd"
    );
    let text_map = projection
        .source_map()
        .tree()
        .and_then(|map| map.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(caret_math::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("TextFlow should have a sidecar mapping"));
    assert!(text_map.segments().windows(2).any(|segments| {
        segments[0]
            .source()
            .zip(segments[1].source())
            .is_some_and(|(left, right)| left.end < right.start)
    }));

    let boundary = text_map
        .segments()
        .iter()
        .find(|segment| segment.display().start == 2)
        .and_then(|segment| segment.source())
        .map(|range| range.start)
        .unwrap_or_else(|| panic!("second text island segment should have a source range"));
    let preceding_end = text_map
        .segments()
        .first()
        .and_then(|segment| segment.source())
        .map(|range| range.end)
        .unwrap_or_else(|| panic!("first text island segment should have a source range"));
    assert_eq!(
        text_source_boundary_from_map(text, text_map, 0, 2, Affinity::Upstream),
        Some(preceding_end)
    );
    assert_eq!(
        text_source_boundary_from_map(text, text_map, 0, 2, Affinity::Downstream),
        Some(boundary)
    );
    assert_eq!(
        text_source_range_from_map(text_map, 0, 2..4),
        Some(boundary..boundary + 2)
    );
    assert_eq!(text_source_range_from_map(text_map, 0, 1..3), None);
}

#[test]
fn sidecar_text_mapping_covers_escaped_utf8_and_empty_text() {
    let projection =
        caret_math::parse_projection(r"\text{a\%b}", caret_math::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("escaped text should project: {error}"));
    let caret_math::ProjectedMath::Tree(tree) = projection.content() else {
        panic!("inline text should project to a math tree");
    };
    let Some(MathContent::Text(text)) = tree.root.content.first() else {
        panic!("escaped source should project to a TextFlow");
    };
    let text_map = projection
        .source_map()
        .tree()
        .and_then(|map| map.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(caret_math::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("escaped TextFlow should have a sidecar mapping"));
    assert_eq!(text_source_range_from_map(text_map, 0, 1..2), Some(7..9));
    assert_eq!(
        text_source_boundary_from_map(text, text_map, 0, 3, Affinity::Upstream),
        Some(10)
    );

    let projection = caret_math::parse_projection(r"\text{}", caret_math::MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("empty text should project: {error}"));
    let caret_math::ProjectedMath::Tree(tree) = projection.content() else {
        panic!("inline empty text should project to a math tree");
    };
    let Some(MathContent::Text(text)) = tree.root.content.first() else {
        panic!("empty source should retain a TextFlow");
    };
    let text_map = projection
        .source_map()
        .tree()
        .and_then(|map| map.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(caret_math::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("empty TextFlow should have a sidecar mapping"));
    assert_eq!(
        text_source_boundary_from_map(text, text_map, 0, 0, Affinity::Upstream),
        Some(6)
    );
}

#[test]
fn empty_text_flow_boundary_uses_only_a_zero_width_range() {
    assert_eq!(
        super::text_branch::empty_text_source_boundary(Some(8..8), 5),
        Some(3)
    );
    assert_eq!(
        super::text_branch::empty_text_source_boundary(Some(8..9), 5),
        None
    );
    assert_eq!(
        super::text_branch::empty_text_source_boundary(None, 5),
        None
    );
}

#[test]
fn empty_text_flow_without_mapping_segments_uses_its_projection_range() {
    let (content, projection) =
        caret_math::parse_projection(r"\text{}", caret_math::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("empty text should project: {error}"))
            .into_parts();
    let caret_math::ProjectedMath::Tree(tree) = content else {
        panic!("inline source should produce a tree");
    };
    let Some(MathContent::Text(_)) = tree.root.content.first() else {
        panic!("empty text should remain a TextFlow");
    };
    let text_map = projection
        .tree()
        .and_then(|map| map.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(caret_math::MathContentSourceMap::text)
        .unwrap_or_else(|| panic!("empty TextFlow should have a sidecar mapping"));
    let range = text_map
        .range()
        .unwrap_or_else(|| panic!("empty TextFlow should have a zero-width anchor"));
    let source_range = (
        u32::try_from(range.start).unwrap_or_else(|_| panic!("source offset fits u32")),
        u32::try_from(range.end).unwrap_or_else(|_| panic!("source offset fits u32")),
    );
    let path = MathTextFlowPath::new(vec![caret_doc::MathPathStep::Content(0)], None);
    let text_map = path
        .resolve_in_projection(&tree, &projection)
        .map(|(_, source_map)| source_map)
        .unwrap_or_else(|| panic!("zero-width range should bind without map segments"));
    let text = path
        .resolve_structurally(&tree)
        .unwrap_or_else(|| panic!("TextFlow path should remain structural"));

    assert_eq!(text_map.segments().len(), 1);
    assert!(text_map.segments()[0].display().is_empty());
    assert_eq!(text_map.segments()[0].source(), Some(range.clone()));
    assert_eq!(
        text_source_boundary_from_map(text, text_map, 0, 0, Affinity::Upstream),
        Some(source_range.0 as usize)
    );
}

#[test]
fn text_path_resolves_against_sidecar_when_tree_has_no_projection_metadata() {
    let (content, source_projection) =
        caret_math::parse_projection(r"\text{a\%b}", caret_math::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("escaped text should project: {error}"))
            .into_parts();
    let caret_math::ProjectedMath::Tree(tree) = content else {
        panic!("inline source should project to a math tree");
    };
    let Some(MathContent::Text(_text)) = tree.root.content.first() else {
        panic!("source should project to a TextFlow");
    };
    let path = MathTextFlowPath::new(vec![caret_doc::MathPathStep::Content(0)], None);

    assert!(path.resolve(&tree).is_some());
    assert!(
        path.resolve_in_projection(&tree, &source_projection)
            .is_some()
    );
}

#[test]
fn text_path_uses_sidecar_ranges_independent_of_the_document_tree() {
    let (content, source_projection) =
        caret_math::parse_projection(r"\text{ab}", caret_math::MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("text should project: {error}"))
            .into_parts();
    let caret_math::ProjectedMath::Tree(mut tree) = content else {
        panic!("inline source should project to a math tree");
    };
    let Some(MathContent::Text(_text)) = tree.root.content.first() else {
        panic!("source should project to a TextFlow");
    };
    let path = MathTextFlowPath::new(vec![caret_doc::MathPathStep::Content(0)], None);
    assert!(
        path.resolve_in_projection(&tree, &source_projection)
            .is_some()
    );
    let Some(MathContent::Text(text)) = tree.root.content.first_mut() else {
        panic!("source should retain a TextFlow");
    };
    text.runs[0].text = "a".to_owned();
    assert!(
        path.resolve_in_projection(&tree, &source_projection)
            .is_none()
    );
}

#[test]
fn arrow_treats_a_rendered_control_symbol_as_one_caret_stop() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\alpha".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(6));

    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    Ok(())
}

#[test]
fn backspace_deletes_one_grapheme_from_formula_source() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("abc".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(inline_source(&session).as_deref(), Some("ab"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    Ok(())
}

#[test]
fn inline_math_root_text_is_promoted_to_the_paragraph_flow() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"a\text{x}b".to_owned()))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(matches!(
        paragraph.inlines.as_slice(),
        [
            ParagraphContent::Math(_),
            ParagraphContent::Text(_),
            ParagraphContent::Math(_)
        ]
    ));
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x{OBJECT_REPLACEMENT}")
    );
    for content in &paragraph.inlines {
        if let ParagraphContent::Math(math) = content {
            assert!(
                !math
                    .math_tree
                    .root
                    .content
                    .iter()
                    .any(|content| { matches!(content, MathContent::Text(_)) })
            );
        }
    }

    session.run(Command::InsertText("!".to_owned()))?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x{OBJECT_REPLACEMENT}!")
    );
    session.run(Command::Undo)?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x{OBJECT_REPLACEMENT}")
    );
    session.run(Command::Redo)?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x{OBJECT_REPLACEMENT}!")
    );
    Ok(())
}

#[test]
fn inline_math_root_empty_text_is_a_paragraph_text_flow() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\text{}".to_owned()))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(
        matches!(paragraph.inlines.as_slice(), [ParagraphContent::Text(text)] if text.runs.iter().all(|run| run.text.is_empty()))
    );
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.offset, 0);
    session.run(Command::InsertText("x".to_owned()))?;
    assert_eq!(first_paragraph_text(&session), "x");
    Ok(())
}

#[test]
fn inline_math_root_split_preserves_an_empty_math_anchor_inside_text() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\text{a$$b}".to_owned()))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(matches!(
        paragraph.inlines.as_slice(),
        [
            ParagraphContent::Text(_),
            ParagraphContent::Math(math),
            ParagraphContent::Text(_)
        ] if math.math_tree.root.content.iter().any(|item| matches!(
            item,
            MathContent::Math(node) if node.kind == caret_doc::MathNodeKind::Placeholder
        ))
    ));
    Ok(())
}

#[test]
fn editing_inline_math_source_promotes_new_root_text_and_moves_focus_to_it() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("ab".to_owned()))?;
    let original_math_id = match session.document().body.first() {
        Some(Block::Paragraph(paragraph)) => match paragraph.inlines.first() {
            Some(ParagraphContent::Math(math)) => math.id.clone(),
            _ => return Err(EditError::InvalidPosition),
        },
        _ => return Err(EditError::InvalidPosition),
    };
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::InsertText(r"\text{x}".to_owned()))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(matches!(
        paragraph.inlines.as_slice(),
        [
            ParagraphContent::Math(_),
            ParagraphContent::Text(_),
            ParagraphContent::Math(_)
        ]
    ));
    assert!(matches!(
        paragraph.inlines.first(),
        Some(ParagraphContent::Math(math)) if math.id == original_math_id
    ));
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x{OBJECT_REPLACEMENT}")
    );
    assert!(session.math_edit().is_none());
    assert_eq!(
        session.selection().focus.offset,
        OBJECT_REPLACEMENT.len_utf8() + 1
    );

    session.run(Command::InsertText("!".to_owned()))?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x!{OBJECT_REPLACEMENT}")
    );
    session.run(Command::Undo)?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x{OBJECT_REPLACEMENT}")
    );
    session.run(Command::Redo)?;
    assert_eq!(
        first_paragraph_text(&session),
        format!("{OBJECT_REPLACEMENT}x!{OBJECT_REPLACEMENT}")
    );
    Ok(())
}

#[test]
fn root_math_focus_survives_split_undo_redo_and_further_source_edits() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("b".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::MoveCaret(Direction::Backward))?;

    session.run(Command::InsertText(r"\text{x}a".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("ab"));
    let split_target = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        caret_math::serialize_tree(session.math_tree_for_edit(&split_target, None)?),
        source_text(&split_target)
    );
    let split_math_id = match &split_target.target {
        MathEditTarget::Inline { math_id, .. } => math_id.clone(),
        MathEditTarget::Block { .. } => return Err(EditError::InvalidPosition),
    };
    assert_eq!(split_target.cursor, 1);
    assert!(matches!(split_target.branch, MathEditBranch::Source));

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some("b"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    session.run(Command::Redo)?;
    let restored = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(inline_source(&session).as_deref(), Some("ab"));
    assert_eq!(restored.target, split_target.target);
    assert_eq!(restored.cursor, split_target.cursor);
    assert_eq!(
        restored.revision.document_generation,
        session.document_generation()
    );

    session.run(Command::InsertText("!".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("a!b"));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some("ab"));
    assert_eq!(
        session.math_edit().map(|edit| &edit.target),
        Some(&MathEditTarget::Inline {
            paragraph_id: match &split_target.target {
                MathEditTarget::Inline { paragraph_id, .. } => paragraph_id.clone(),
                MathEditTarget::Block { .. } => return Err(EditError::InvalidPosition),
            },
            math_id: split_math_id,
        })
    );
    Ok(())
}

#[test]
fn inline_math_root_text_keeps_one_format_environment_wrapper() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\text{\textbf{x}}".to_owned()))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let [ParagraphContent::Environment(environment)] = paragraph.inlines.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(environment.kind, InlineEnvironmentKind::Bold);
    assert!(matches!(
        environment.children.as_slice(),
        [ParagraphContent::Text(_)]
    ));
    assert_eq!(first_paragraph_text(&session), "x");
    Ok(())
}

#[test]
fn inline_math_text_inside_fixed_operand_stays_in_math_tree() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\frac{a}{\text{x}}".to_owned()))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let [ParagraphContent::Math(math)] = paragraph.inlines.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(
        first_paragraph_text(&session),
        OBJECT_REPLACEMENT.to_string()
    );
    let Some(MathContent::Math(fraction)) = math.math_tree.root.content.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(fraction.children.iter().any(|flow| {
        flow.content
            .iter()
            .any(|content| matches!(content, MathContent::Text(_)))
    }));
    Ok(())
}

#[test]
fn unsupported_host_commands_do_not_escape_formula_editing() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::InsertText("host".to_owned()))?;
    session.run(Command::InsertInlineMath(r"\frac{p}{q}".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;

    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let [ParagraphContent::Environment(environment)] = paragraph.inlines.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(environment.kind, InlineEnvironmentKind::Bold);
    assert!(
        environment
            .children
            .iter()
            .any(|content| matches!(content, ParagraphContent::Math(_)))
    );

    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    for command in [Command::InsertSoftBreak, Command::InsertParagraphBreak] {
        assert_eq!(session.run(command), Err(EditError::UnsupportedNewline));
        assert_eq!(session.document(), &before_document);
        assert_eq!(session.math_edit(), before_edit.as_ref());
        assert_eq!(session.document_generation(), before_generation);
    }
    Ok(())
}

#[test]
fn math_command_line_keeps_mod_backspace_as_a_keyboard_noop() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::BeginCommandLine)?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_command_line = session.command_line().cloned();

    let effect = session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(!effect.document_changed);
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.command_line(), before_command_line.as_ref());
    Ok(())
}

#[test]
fn formula_focus_allows_global_settings_and_mouse_selection_exit() -> Result<(), EditError> {
    let mut session = session_with_text("host").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let active_edit = session.math_edit().cloned();

    session.run(Command::SetDefaultFontFamily(Some(
        "Example Sans".to_owned(),
    )))?;
    assert_eq!(session.math_edit(), active_edit.as_ref());

    let mut invalid = session.selection().focus.clone();
    invalid.offset = usize::MAX;
    assert!(
        session
            .run(Command::SetSelection(Some(Selection {
                anchor: invalid.clone(),
                focus: invalid,
            })))
            .is_err()
    );
    assert_eq!(session.math_edit(), active_edit.as_ref());

    let mut outside = session.selection().focus.clone();
    outside.offset = 0;
    outside.affinity = Affinity::Downstream;
    session.run(Command::SetSelection(Some(Selection {
        anchor: outside.clone(),
        focus: outside,
    })))?;
    assert!(session.math_edit().is_none());
    assert_eq!(
        session.selection_visual_affinity(),
        VisualAffinity::Downstream
    );
    Ok(())
}

#[test]
fn clearing_host_selection_keeps_math_focus_and_collapses_text_island_selection()
-> Result<(), EditError> {
    let mut source_session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    source_session.run(Command::InsertInlineMath("x".to_owned()))?;
    source_session.run(Command::EnterMath(Direction::Backward))?;
    let source_edit = source_session.math_edit().cloned();
    source_session.run(Command::SetSelection(None))?;
    assert_eq!(source_session.math_edit(), source_edit.as_ref());

    let mut text_session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut text_session, r"\text{abcd}")?;
    move_into_math_text(&mut text_session)?;
    text_session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    text_session.run(Command::SetSelection(None))?;
    let edit = text_session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    assert!(
        matches!(edit.branch, MathEditBranch::Text { path, anchor, ..
            } if anchor.path == path && anchor.offset == edit.cursor)
    );
    assert_eq!(
        inline_source(&text_session).as_deref(),
        Some(r"\text{abcd}")
    );
    Ok(())
}

#[test]
fn mod_backspace_does_not_touch_a_paragraph_adjacent_to_display_math() -> Result<(), EditError> {
    let mut session = session_with_text("outside").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::InsertText(" bold".to_owned()))?;
    session.insert_display_math(false)?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: false, .. })
    ));
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();

    let effect = session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(!effect.document_changed);
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn mod_backspace_rejects_unpreservable_math_metadata_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("x^2".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let Some(MathEditTarget::Inline { math_id, .. }) = session.math_edit().map(|edit| &edit.target)
    else {
        return Err(EditError::InvalidPosition);
    };
    let math_id = math_id.clone();
    let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
        return Err(EditError::InvalidPosition);
    };
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    math.math_tree
        .extra
        .insert("vendorField".to_owned(), serde_json::Value::Bool(true));

    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    assert_eq!(
        session.run(Command::EditorKey(EditorKey::ModBackspace)),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn mod_backspace_rejects_math_trees_whose_tex_projection_exceeds_the_source_budget_atomically()
-> Result<(), EditError> {
    let max_source_bytes = caret_math::MAX_MATH_SOURCE_BYTES;
    let cases = [
        "tree source exceeds budget".to_owned(),
        "escaped TeX projection exceeds budget".to_owned(),
    ];
    for (case, text) in cases.into_iter().zip([
        "x".repeat(max_source_bytes + 1),
        "#".repeat(max_source_bytes / 2 + 8),
    ]) {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.run(Command::InsertInlineMath(r"\frac{x}{\text{x}}".to_owned()))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        let Some(MathEditTarget::Inline { math_id, .. }) =
            session.math_edit().map(|edit| &edit.target)
        else {
            return Err(EditError::InvalidPosition);
        };
        let math_id = math_id.clone();
        let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
            return Err(EditError::InvalidPosition);
        };
        let math = find_inline_math_mut(&mut paragraph.inlines, &math_id)
            .ok_or(EditError::InvalidPosition)?;
        let text_flow =
            first_math_text_flow_mut(&mut math.math_tree.root).ok_or(EditError::InvalidPosition)?;
        text_flow.runs[0].text = text;
        assert!(math.math_tree.validate().is_ok(), "{case}");
        assert!(
            caret_math::serialize_tree(&math.math_tree).len() > max_source_bytes,
            "{case}"
        );

        let before_document = session.document().clone();
        let before_edit = session.math_edit().cloned();
        let before_selection = session.selection().clone();
        let before_generation = session.document_generation();
        assert_eq!(
            session.run(Command::EditorKey(EditorKey::ModBackspace)),
            Err(EditError::UnsupportedOpaqueContent),
            "{case}"
        );
        assert_eq!(session.document(), &before_document, "{case}");
        assert_eq!(session.math_edit(), before_edit.as_ref(), "{case}");
        assert_eq!(session.selection(), &before_selection, "{case}");
        assert_eq!(session.document_generation(), before_generation, "{case}");
    }
    Ok(())
}

#[test]
fn mod_backspace_is_a_noop_for_oversized_equation_text_focus() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    accept(&mut session, "equation")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText(r"\text{abc}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| (&edit.target, &edit.branch)),
        Some((
            MathEditTarget::Block { align: false, .. },
            MathEditBranch::Text { .. }
        ))
    ));
    let Some(MathEditTarget::Block { block_id, .. }) = session.math_edit().map(|edit| &edit.target)
    else {
        return Err(EditError::InvalidPosition);
    };
    let block_id = block_id.clone();
    let Some(Block::Equation(equation)) = session
        .document
        .body
        .iter_mut()
        .find(|block| matches!(block, Block::Equation(value) if value.id == block_id))
    else {
        return Err(EditError::InvalidPosition);
    };
    let text_flow =
        first_math_text_flow_mut(&mut equation.math_tree.root).ok_or(EditError::InvalidPosition)?;
    text_flow.runs[0].text = "x".repeat(caret_math::MAX_MATH_SOURCE_BYTES + 1);
    assert!(equation.math_tree.validate().is_ok());

    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_selection = session.selection().clone();
    let before_generation = session.document_generation();
    let effect = session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(!effect.document_changed);
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.selection(), &before_selection);
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn mod_backspace_rejects_text_and_environment_metadata_atomically() -> Result<(), EditError> {
    for (source, metadata_kind) in [
        (r"\frac{x}{\text{abc}}", 0),
        (r"\frac{x}{\text{abc}}", 1),
        (r"\frac{x}{\text{\textsf{abc}}}", 2),
    ] {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.run(Command::InsertInlineMath(source.to_owned()))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        let Some(MathEditTarget::Inline { math_id, .. }) =
            session.math_edit().map(|edit| &edit.target)
        else {
            return Err(EditError::InvalidPosition);
        };
        let math_id = math_id.clone();
        let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
            return Err(EditError::InvalidPosition);
        };
        let math = find_inline_math_mut(&mut paragraph.inlines, &math_id)
            .ok_or(EditError::InvalidPosition)?;
        if metadata_kind < 2 {
            let text = first_math_text_flow_mut(&mut math.math_tree.root)
                .ok_or(EditError::InvalidPosition)?;
            if metadata_kind == 0 {
                text.runs[0]
                    .props
                    .insert("underline".to_owned(), serde_json::Value::Bool(true));
            } else {
                text.extra
                    .insert("vendorField".to_owned(), serde_json::Value::Bool(true));
            }
        } else {
            let environment = first_math_environment_mut(&mut math.math_tree.root)
                .ok_or(EditError::InvalidPosition)?;
            environment
                .extra
                .insert("vendorField".to_owned(), serde_json::Value::Bool(true));
        }

        let before_document = session.document().clone();
        let before_edit = session.math_edit().cloned();
        let before_generation = session.document_generation();
        assert_eq!(
            session.run(Command::EditorKey(EditorKey::ModBackspace)),
            Err(EditError::UnsupportedOpaqueContent)
        );
        assert_eq!(session.document(), &before_document);
        assert_eq!(session.math_edit(), before_edit.as_ref());
        assert_eq!(session.document_generation(), before_generation);
    }
    Ok(())
}

#[test]
fn mod_backspace_rejects_tree_only_content_atomically() -> Result<(), EditError> {
    for metadata_kind in 0..3 {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.run(Command::InsertInlineMath("x".to_owned()))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        let Some(MathEditTarget::Inline { math_id, .. }) =
            session.math_edit().map(|edit| &edit.target)
        else {
            return Err(EditError::InvalidPosition);
        };
        let math_id = math_id.clone();
        let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
            return Err(EditError::InvalidPosition);
        };
        let math = find_inline_math_mut(&mut paragraph.inlines, &math_id)
            .ok_or(EditError::InvalidPosition)?;
        match metadata_kind {
            0 => {
                let children = std::mem::take(&mut math.math_tree.root.content);
                math.math_tree.root.content =
                    vec![MathContent::Environment(caret_doc::InlineEnvironment {
                        id: NodeId::generate(),
                        kind: caret_doc::InlineEnvironmentKind::Bold,
                        definition_id: None,
                        props: BTreeMap::new(),
                        children,
                        extra: BTreeMap::from([(
                            "unrepresentable".to_owned(),
                            serde_json::json!(true),
                        )]),
                    })];
            }
            1 => math
                .math_tree
                .root
                .content
                .push(MathContent::Text(caret_doc::TextFlow {
                    runs: vec![Run {
                        text: "secret".to_owned(),
                        language: Some("en".to_owned()),
                        props: BTreeMap::new(),
                        extra: BTreeMap::new(),
                    }],
                    extra: BTreeMap::new(),
                })),
            _ => {
                let Some(MathContent::Math(mut node)) =
                    math.math_tree.root.content.first().cloned()
                else {
                    return Err(EditError::InvalidPosition);
                };
                node.text = Some("y".to_owned());
                node.extra
                    .insert("unrepresentable".to_owned(), serde_json::json!(true));
                math.math_tree.root.content.push(MathContent::Math(node));
            }
        }

        let before_document = session.document().clone();
        let before_edit = session.math_edit().cloned();
        let before_generation = session.document_generation();
        assert_eq!(
            session.run(Command::EditorKey(EditorKey::ModBackspace)),
            Err(EditError::UnsupportedOpaqueContent)
        );
        assert_eq!(session.document(), &before_document);
        assert_eq!(session.math_edit(), before_edit.as_ref());
        assert_eq!(session.document_generation(), before_generation);
    }
    Ok(())
}

#[test]
fn mod_backspace_materializes_empty_math_as_empty_text_and_is_undoable() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(String::new()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let before = session.document().clone();

    let effect = session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(effect.document_changed);
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), "");
    assert!(matches!(
        session.document().body.first(),
        Some(Block::Paragraph(Paragraph { inlines, .. }))
            if matches!(inlines.as_slice(), [ParagraphContent::Text(flow)] if flow.runs.len() == 1 && flow.runs[0].text.is_empty())
    ));

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &before);
    assert!(session.math_edit().is_some());
    session.run(Command::Redo)?;
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn mod_backspace_preserves_text_island_selection_through_source_map() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abcd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;

    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), r"\frac{x}{\text{abcd}}");
    let prefix_len = r"\frac{x}{\text{".len();
    assert_eq!(session.selection().anchor.offset, prefix_len + 1);
    assert_eq!(session.selection().focus.offset, prefix_len + 3);
    Ok(())
}

#[test]
fn mod_backspace_preserves_reverse_text_island_selection() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abcd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 3,
        focus: 1,
    })?;

    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    let prefix_len = r"\frac{x}{\text{".len();
    assert_eq!(session.selection().anchor.offset, prefix_len + 3);
    assert_eq!(session.selection().focus.offset, prefix_len + 1);
    assert!(session.selection().anchor.offset > session.selection().focus.offset);
    Ok(())
}

#[test]
fn mod_backspace_rejects_selection_that_becomes_mid_grapheme_in_source_text()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(
        "\\frac{x}{\\text{\u{301}x}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 2,
    })?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();

    assert_eq!(
        session.run(Command::EditorKey(EditorKey::ModBackspace)),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn mod_backspace_accepts_font_properties_reconstructed_from_math_source() -> Result<(), EditError> {
    for source in [
        r"\frac{x}{\text{\textsf{sans}}}",
        r"\frac{x}{\text{\texttt {mono}}}",
    ] {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.run(Command::InsertInlineMath(source.to_owned()))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        let Some(MathEditTarget::Inline { math_id, .. }) =
            session.math_edit().map(|edit| &edit.target)
        else {
            return Err(EditError::InvalidPosition);
        };
        let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
            return Err(EditError::InvalidPosition);
        };
        let math =
            find_inline_math(&paragraph.inlines, math_id).ok_or(EditError::InvalidPosition)?;
        assert!(math_tree_has_font_environment(&math.math_tree.root));
        let canonical = caret_math::serialize_tree(&math.math_tree);
        session.run(Command::EditorKey(EditorKey::ModBackspace))?;
        assert_eq!(first_paragraph_text(&session), canonical);
        assert!(session.math_edit().is_none());
    }
    Ok(())
}

#[test]
fn mod_backspace_downgrades_multiline_math_to_canonical_single_line_text() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("x\ny".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;

    let canonical = {
        let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
            return Err(EditError::InvalidPosition);
        };
        let [ParagraphContent::Math(math)] = paragraph.inlines.as_slice() else {
            return Err(EditError::InvalidPosition);
        };
        caret_math::serialize_tree(&math.math_tree)
    };
    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert_eq!(first_paragraph_text(&session), canonical);
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn mod_backspace_keeps_text_caret_when_canonical_projection_merges_text_flows()
-> Result<(), EditError> {
    let source = r"\frac{x}{\text{a}\text{b}\alpha\text{d}\text{e}}";
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(source.to_owned()))?;
    move_into_math_text(&mut session)
        .unwrap_or_else(|error| panic!("entering a text flow failed: {error:?}"));
    session
        .run(Command::SetMathTextSelection {
            anchor: 1,
            focus: 1,
        })
        .unwrap_or_else(|error| panic!("setting the text cursor failed: {error:?}"));

    let canonical = {
        let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
            return Err(EditError::InvalidPosition);
        };
        let [ParagraphContent::Math(math)] = paragraph.inlines.as_slice() else {
            return Err(EditError::InvalidPosition);
        };
        caret_math::serialize_tree(&math.math_tree)
    };
    session
        .run(Command::EditorKey(EditorKey::ModBackspace))
        .unwrap_or_else(|error| panic!("formula demotion unexpectedly failed: {error:?}"));

    assert_eq!(first_paragraph_text(&session), canonical);
    assert_eq!(
        session.selection().focus.offset,
        canonical.rfind("{de}").ok_or(EditError::InvalidPosition)? + 3
    );
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn mod_backspace_keeps_source_caret_in_place_and_typing_continues_there() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("abc".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));

    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert_eq!(session.selection().focus.offset, 2);
    session.run(Command::InsertText("!".to_owned()))?;
    assert_eq!(first_paragraph_text(&session), "ab!c");
    Ok(())
}

#[test]
fn mod_backspace_maps_a_noncanonical_source_caret_to_canonical_text() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::InsertText(" ".to_owned()))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));

    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert_eq!(first_paragraph_text(&session), "x");
    assert_eq!(session.selection().focus.offset, 1);
    session.run(Command::InsertText("!".to_owned()))?;
    assert_eq!(first_paragraph_text(&session), "x!");
    Ok(())
}

#[test]
fn mod_backspace_snaps_collapsed_caret_downstream_when_text_graphemes_join() -> Result<(), EditError>
{
    let mut session = session_with_text("\u{301}").map_err(|_| EditError::InvalidPosition)?;
    let mut start = session.selection().focus.clone();
    start.offset = 0;
    start.affinity = Affinity::Downstream;
    session.run(Command::SetSelection(Some(Selection {
        anchor: start.clone(),
        focus: start,
    })))?;
    session.run(Command::InsertInlineMath("a".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;

    let before = session.document().clone();
    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert_eq!(first_paragraph_text(&session), "a\u{301}");
    assert_eq!(session.selection().anchor, session.selection().focus);
    assert_eq!(session.selection().focus.offset, "a\u{301}".len());
    session.run(Command::Undo)?;
    assert_eq!(session.document(), &before);
    session.run(Command::Redo)?;
    assert_eq!(session.selection().focus.offset, "a\u{301}".len());
    session.run(Command::InsertText("!".to_owned()))?;
    assert_eq!(first_paragraph_text(&session), "a\u{301}!");
    Ok(())
}

#[test]
fn deleting_empty_math_snaps_caret_when_adjacent_text_joins_a_grapheme() -> Result<(), EditError> {
    let caret_math::ParsedMath::Tree(math_tree) =
        caret_math::parse("", caret_math::MathInputKind::Inline)
            .map_err(|_| EditError::InvalidMath("empty inline fixture failed".to_owned()))?
    else {
        return Err(EditError::InvalidMath("expected inline math".to_owned()));
    };
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = vec![
        ParagraphContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: "a".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Math(InlineMath {
            id: NodeId::generate(),
            math_tree,
            extra: BTreeMap::new(),
        }),
        ParagraphContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: "\u{301}".to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }),
    ];
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session = Session::new(document)?;
    let mut after_math = session.selection().focus.clone();
    after_math.offset = "a".len() + OBJECT_REPLACEMENT.len_utf8();
    session.run(Command::SetSelection(Some(Selection {
        anchor: after_math.clone(),
        focus: after_math.clone(),
    })))?;
    let mut before_atom = after_math.clone();
    before_atom.offset -= OBJECT_REPLACEMENT.len_utf8();
    before_atom.affinity = Affinity::Upstream;
    let mut after_atom = after_math.clone();
    after_atom.affinity = Affinity::Downstream;
    session.run(Command::EnterMath(Direction::Backward))?;

    let effect = session.run(Command::Delete(Direction::Backward))?;
    let position_map = effect
        .position_map
        .as_ref()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(position_map.map(&before_atom).offset, 0);
    assert_eq!(position_map.map(&after_atom).offset, 3);
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), "a\u{301}");

    session.run(Command::Undo)?;
    assert!(session.math_edit().is_some());
    assert_eq!(
        first_paragraph_text(&session),
        format!("a{OBJECT_REPLACEMENT}\u{301}")
    );
    session.run(Command::Redo)?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.offset, 0);
    assert_eq!(first_paragraph_text(&session), "a\u{301}");
    session.run(Command::InsertText("x".to_owned()))?;
    assert_eq!(first_paragraph_text(&session), "xa\u{301}");
    session.run(Command::InsertText("y".to_owned()))?;
    Ok(())
}

#[test]
fn caret_can_stop_between_inline_math_and_following_combining_text() -> Result<(), EditError> {
    let mut session = session_with_text("a").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::InsertText("\u{301}".to_owned()))?;

    let inline_count = match session.document().body.first() {
        Some(Block::Paragraph(paragraph)) => paragraph.inlines.len(),
        _ => return Err(EditError::InvalidPosition),
    };
    assert_eq!(inline_count, 3);
    let atom_end = "a".len() + OBJECT_REPLACEMENT.len_utf8();
    let position = Pos {
        offset: atom_end,
        affinity: Affinity::Downstream,
        ..session.selection().focus.clone()
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: position.clone(),
        focus: position,
    })))?;

    assert_eq!(session.selection().focus.offset, atom_end);
    for _ in 0..4 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert_eq!(session.selection().focus.offset, atom_end + "\u{301}".len());
    Ok(())
}

#[test]
fn text_island_accepts_plain_input_and_rebases_its_path() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    let before_command = session.document().clone();
    let before_focus = session.math_edit().cloned();
    session.run(Command::BeginCommandLine)?;
    assert_eq!(
        session.command_line().map(|line| line.mode),
        Some(CommandLineMode::Math)
    );
    session.run(Command::CancelCommandLine)?;
    assert_eq!(session.document(), &before_command);
    assert_eq!(session.math_edit(), before_focus.as_ref());

    session.run(Command::InsertText("X".to_owned()))?;

    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abcX}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(4));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abc}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    Ok(())
}

#[test]
fn text_island_edit_is_tree_native_above_the_latex_source_limit() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{a}")?;
    move_into_math_text(&mut session)?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| edit.source_projection.is_none())
    );

    let inserted = "x".repeat(caret_math::MAX_MATH_SOURCE_BYTES + 1);
    session
        .run(Command::InsertText(inserted.clone()))
        .unwrap_or_else(|error| panic!("large TextFlow insertion failed: {error:?}"));
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| edit.source_projection.is_none())
    );
    assert!(
        inline_source(&session)
            .ok_or(EditError::InvalidPosition)?
            .len()
            > caret_math::MAX_MATH_SOURCE_BYTES
    );
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("large TextFlow undo failed: {error:?}"));
    assert_eq!(active_math_text_flow(&session)?.runs[0].text, "a");
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("large TextFlow redo failed: {error:?}"));
    assert_eq!(
        active_math_text_flow(&session)?.runs[0].text,
        format!("a{inserted}")
    );
    Ok(())
}

#[test]
fn math_text_run_property_splits_range_and_undoes_without_reparsing_source() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    let source = session
        .math_edit()
        .and_then(|edit| edit.source().map(str::to_owned));

    let effect = session.run(Command::ToggleRunUnderline)?;
    assert!(effect.document_changed);
    assert!(!effect.selection_changed);
    assert_eq!(
        session
            .math_edit()
            .and_then(|edit| edit.source().map(str::to_owned)),
        source
    );
    assert!(matches!(
        active_math_text_flow(&session)?.runs.as_slice(),
        [before, selected, after]
            if before.text == "a"
                && selected.text == "bc"
                && selected.props.get("underline") == Some(&serde_json::Value::Bool(true))
                && after.text == "d"
    ));

    session.run(Command::Undo)?;
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abcd"
    );
    assert_eq!(
        session.math_edit().and_then(|edit| edit.source()),
        source.as_deref()
    );
    session.run(Command::Redo)?;
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    Ok(())
}

#[test]
fn inline_math_run_property_keeps_text_selection_without_reparsing_source() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abcd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    let before_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let before_source = before_edit.source().map(str::to_owned);

    let effect = session.run(Command::ToggleRunUnderline)?;
    assert!(effect.document_changed);
    assert!(!effect.selection_changed);
    let after_edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_edit.cursor, before_edit.cursor);
    assert_eq!(after_edit.branch, before_edit.branch);
    assert_eq!(after_edit.source().map(str::to_owned), before_source);
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));

    session.run(Command::Undo)?;
    assert!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .all(|run| { run.props.get("underline") != Some(&serde_json::Value::Bool(true)) })
    );
    session.run(Command::Redo)?;
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    Ok(())
}

#[test]
fn math_text_font_family_uses_inline_environment_transactions() -> Result<(), EditError> {
    let mut selected = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    selected.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abcd}}".to_owned(),
    ))?;
    move_into_math_text(&mut selected)?;
    selected.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    let document_before = selected.document().clone();
    let edit_before = selected.math_edit().cloned();
    let generation_before = selected.document_generation();
    let history_before = selected.history_state_id();
    assert_eq!(
        selected.run(Command::SetInlineFontFamily("  ".to_owned())),
        Err(EditError::InvalidFontFamily)
    );
    assert_eq!(selected.document(), &document_before);
    assert_eq!(selected.math_edit(), edit_before.as_ref());
    assert_eq!(selected.document_generation(), generation_before);
    assert_eq!(selected.history_state_id(), history_before);

    let effect = selected.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    assert!(effect.document_changed);
    assert!(effect.selection_changed);
    let active_text = active_math_text_flow(&selected)?;
    assert_eq!(
        active_text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "bc"
    );
    assert!(math_tree_has_font_environment_for(
        &selected
            .math_tree_for_edit(
                selected.math_edit().ok_or(EditError::InvalidPosition)?,
                None,
            )?
            .root,
        "Noto Serif",
        "bc"
    ));
    selected.run(Command::Undo)?;
    assert!(!math_tree_has_font_environment(
        &selected
            .math_tree_for_edit(
                selected.math_edit().ok_or(EditError::InvalidPosition)?,
                None,
            )?
            .root
    ));
    selected.run(Command::Redo)?;
    assert!(math_tree_has_font_environment_for(
        &selected
            .math_tree_for_edit(
                selected.math_edit().ok_or(EditError::InvalidPosition)?,
                None,
            )?
            .root,
        "Noto Serif",
        "bc"
    ));
    selected.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 0,
    })?;
    selected.run(Command::SetInlineFontFamily("sans-serif".to_owned()))?;
    let reverse_edit = selected.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(reverse_edit.cursor, 0);
    assert!(matches!(
        &reverse_edit.branch,
        MathEditBranch::Text { anchor, .. } if anchor.offset == 2
    ));
    assert!(math_tree_has_font_environment_for(
        &selected.math_tree_for_edit(reverse_edit, None)?.root,
        "sans-serif",
        "bc"
    ));

    let mut navigation = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    navigation.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abcd}}".to_owned(),
    ))?;
    move_into_math_text(&mut navigation)?;
    navigation.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    navigation.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    navigation.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 0,
    })?;
    for _ in 0..3 {
        navigation.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert!(matches!(
        navigation.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));
    assert!(
        navigation
            .math_edit()
            .is_some_and(|edit| edit.source_projection.is_none())
    );
    navigation.run(Command::InsertText("q".to_owned()))?;
    let navigation_edit = navigation.math_edit().ok_or(EditError::InvalidPosition)?;
    let navigation_tree = navigation.math_tree_for_edit(navigation_edit, None)?;
    assert!(caret_math::serialize_tree(navigation_tree).contains('q'));
    assert!(math_tree_has_font_environment_for(
        &navigation_tree.root,
        "Noto Serif",
        "bc"
    ));

    let mut pending = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    pending.run(Command::InsertInlineMath(
        r"\frac{x}{\textsf{ab}}".to_owned(),
    ))?;
    move_into_math_text(&mut pending)?;
    pending.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    pending.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    assert_eq!(
        pending.pending_inline_style.active_font_family(),
        Some("Noto Serif")
    );
    pending.run(Command::SetInlineFontFamily("sans-serif".to_owned()))?;
    assert_eq!(
        pending.pending_inline_style.active_font_family(),
        Some("sans-serif")
    );
    pending.run(Command::Undo)?;
    assert_eq!(
        pending.pending_inline_style.active_font_family(),
        Some("Noto Serif")
    );
    pending.run(Command::Redo)?;
    pending.run(Command::InsertText("X".to_owned()))?;
    assert!(math_tree_has_font_environment_for(
        &pending
            .math_tree_for_edit(pending.math_edit().ok_or(EditError::InvalidPosition)?, None,)?
            .root,
        "sans-serif",
        "X"
    ));

    let (parsed_align, projection) =
        display_math_block_with_projection(true, r"x+\text{abcd}", NodeId::generate())?;
    let Block::Align(align_block) = parsed_align else {
        return Err(EditError::InvalidPosition);
    };
    let block_id = align_block.id.clone();
    let row = align_block.rows.first().ok_or(EditError::InvalidPosition)?;
    let cell = row.cells.first().ok_or(EditError::InvalidPosition)?;
    let source_map = projection
        .align()
        .and_then(|map| map.cell_tree(0, 0))
        .ok_or(EditError::InvalidPosition)?;
    let text_source =
        first_text_source_range(&cell.root, source_map.root()).ok_or(EditError::InvalidPosition)?;
    let text_path =
        path::path_at_source_boundary(source_map, text_source.start, true, Some((0, 0)))
            .ok_or(EditError::InvalidPosition)?;
    let cursor = 0;
    let branch = MathEditBranch::Text {
        path: text_path.clone(),
        anchor: MathTextPosition::new(text_path, cursor, MathPositionAffinity::Downstream),
        focus_affinity: MathPositionAffinity::Downstream,
    };
    let mut align_document = Document::blank();
    align_document.body.push(Block::Align(align_block));
    let mut align = Session::new(align_document)?;
    align.math_edit = Some(MathEdit {
        target: MathEditTarget::Block {
            block_id,
            align: true,
        },
        source_projection: Some(Arc::new(projection)),
        source_draft: None,
        revision: align.current_math_edit_revision(),
        cursor,
        branch,
    });
    align.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    align.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    assert!(math_tree_has_font_environment_for(
        &align
            .math_tree_for_edit(
                align.math_edit().ok_or(EditError::InvalidPosition)?,
                Some((0, 0)),
            )?
            .root,
        "Noto Serif",
        "bc"
    ));
    align.run(Command::Undo)?;
    assert!(!math_tree_has_font_environment(
        &align
            .math_tree_for_edit(
                align.math_edit().ok_or(EditError::InvalidPosition)?,
                Some((0, 0)),
            )?
            .root
    ));
    align.run(Command::Redo)?;
    align.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 0,
    })?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    assert!(align.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    align.run(Command::InsertText("q".to_owned()))?;
    assert!(math_tree_has_font_environment_for(
        &align
            .math_tree_for_edit(
                align.math_edit().ok_or(EditError::InvalidPosition)?,
                Some((0, 0)),
            )?
            .root,
        "Noto Serif",
        "bc"
    ));
    Ok(())
}

#[test]
fn math_text_pending_run_property_styles_inserted_text_and_moves_clear_it()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    let original_edit = session.math_edit().cloned();
    session.run(Command::ToggleRunUnderline)?;
    assert!(session.pending_inline_style.is_active());
    session.run(Command::Undo)?;
    assert!(!session.pending_inline_style.is_active());
    assert_eq!(session.math_edit(), original_edit.as_ref());
    session.run(Command::Redo)?;
    assert!(session.pending_inline_style.is_active());
    session.run(Command::InsertText("X".to_owned()))?;
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "X" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(!session.pending_inline_style.is_active());

    let mut disable = session_with_text("")?;
    insert_equation_text(&mut disable, r"\text{a}")?;
    move_into_math_text(&mut disable)?;
    disable.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 1,
    })?;
    disable.run(Command::ToggleRunUnderline)?;
    disable.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 1,
    })?;
    disable.run(Command::ToggleRunUnderline)?;
    let flow = active_math_text_flow(&disable)?;
    assert!(
        flow.runs
            .iter()
            .all(|run| run.props.get("underline") != Some(&serde_json::Value::Bool(true)))
    );
    Ok(())
}

#[test]
fn math_text_pending_underline_survives_sequential_input_until_explicit_move()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::ToggleRunUnderline)?;
    session.run(Command::InsertText("X".to_owned()))?;
    session.run(Command::InsertText("Y".to_owned()))?;

    let flow = active_math_text_flow(&session)?;
    assert!(flow.runs.iter().any(|run| {
        run.text.contains('X')
            && run.text.contains('Y')
            && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{aXb}"));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{ab}"));
    session.run(Command::Redo)?;
    session.run(Command::Redo)?;
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text.contains('X')
            && run.text.contains('Y')
            && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));

    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(!session.pending_inline_style.is_active());
    Ok(())
}

#[test]
fn pending_run_property_covers_a_grapheme_spanning_math_textflows_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, "\\text{a}\\text{}")?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let path = path::root_text_flow_path(tree, false, None).ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 0);
    session.run(Command::ToggleRunUnderline)?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::SetInlineFontFamily("Noto Serif".to_owned()))?;
    session.run(Command::InsertText("\u{301}".to_owned()))?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(
        tree,
        match &edit.branch {
            MathEditBranch::Text { path, .. } => path,
            MathEditBranch::Source => return Err(EditError::InvalidPosition.into()),
            MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition.into()),
        },
    )
    .ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "a\u{301}");
    for candidate in &island.paths {
        let text = candidate
            .resolve_structurally(tree)
            .ok_or(EditError::InvalidPosition)?;
        assert!(text.runs.iter().all(|run| {
            run.text.is_empty()
                || run.props.get("underline") == Some(&serde_json::Value::Bool(true))
        }));
        assert_eq!(
            super::text_inline_environment::inherited_environment_value(
                tree,
                candidate,
                InlineEnvironmentKind::Bold,
            ),
            Some(true)
        );
        assert_eq!(
            super::text_inline_environment::inherited_math_text_font_family(tree, candidate)
                .flatten()
                .as_deref(),
            Some("Noto Serif")
        );
    }
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{a}"));
    session.run(Command::Redo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    let path = match &edit.branch {
        MathEditBranch::Text { path, .. } => path,
        MathEditBranch::Source => return Err(EditError::InvalidPosition.into()),
        MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition.into()),
    };
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "a\u{301}");
    Ok(())
}

#[test]
fn pending_run_property_covers_a_regional_indicator_pair_across_textflows()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, "\\text{🇦}\\text{}")?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let path = path::root_text_flow_path(tree, false, None).ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 0);
    session.run(Command::ToggleRunUnderline)?;
    session.run(Command::InsertText("🇽".to_owned()))?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    let path = match &edit.branch {
        MathEditBranch::Text { path, .. } => path,
        MathEditBranch::Source => return Err(EditError::InvalidPosition.into()),
        MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition.into()),
    };
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "🇦🇽");
    let first = island.paths[0]
        .resolve_structurally(tree)
        .ok_or(EditError::InvalidPosition)?;
    assert!(first.runs.iter().any(|run| {
        run.text == "🇦" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    let second = island.paths[1]
        .resolve_structurally(tree)
        .ok_or(EditError::InvalidPosition)?;
    assert!(second.runs.iter().any(|run| {
        run.text == "🇽" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    Ok(())
}

#[test]
fn cross_flow_grapheme_format_rejects_unknown_run_metadata_without_partial_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, "\\text{a}\\text{}")?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let path = path::root_text_flow_path(session.math_tree_for_edit(&edit, None)?, false, None)
        .ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 0);
    let Some(Block::Equation(equation)) = session.document.body.first_mut() else {
        return Err(EditError::InvalidPosition.into());
    };
    let adjacent = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(0)], None);
    adjacent
        .resolve_structurally_mut(&mut equation.math_tree)
        .and_then(|text| text.runs.first_mut())
        .ok_or(EditError::InvalidPosition)?
        .extra
        .insert("futureRunField".to_owned(), serde_json::json!("opaque"));
    session.run(Command::ToggleRunUnderline)?;
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();

    assert!(matches!(
        session.run(Command::InsertText("\u{301}".to_owned())),
        Err(EditError::UnsupportedOpaqueContent)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.document_generation(), generation_before);
    Ok(())
}
#[test]
fn math_text_run_property_rejects_partial_unknown_run_metadata_atomically() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abcd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditTarget::Inline { math_id, .. } = edit.target else {
        return Err(EditError::InvalidPosition);
    };
    let Some(Block::Paragraph(paragraph)) = session.document.body.first_mut() else {
        return Err(EditError::InvalidPosition);
    };
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    first_math_text_flow_mut(&mut math.math_tree.root)
        .and_then(|text| text.runs.first_mut())
        .ok_or(EditError::InvalidPosition)?
        .extra
        .insert("futureRunField".to_owned(), serde_json::json!("opaque"));
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();

    assert_eq!(
        session.run(Command::ToggleRunUnderline),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);

    session.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 4,
    })?;
    session.run(Command::ToggleRunUnderline)?;
    let run = active_math_text_flow(&session)?
        .runs
        .first()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(run.text, "abcd");
    assert_eq!(
        run.extra.get("futureRunField"),
        Some(&serde_json::json!("opaque"))
    );
    assert_eq!(
        run.props.get("underline"),
        Some(&serde_json::Value::Bool(true))
    );
    Ok(())
}
#[test]
fn starting_math_from_a_text_island_splits_the_text_source_and_is_undoable() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;

    session.begin_inline_math()?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}{}\text{cd}")
    );
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(math_flow_has_placeholder(
        &session.math_tree_for_edit(edit, None)?.root
    ));
    assert_eq!(edit.cursor, 1);
    assert!(matches!(
        &edit.branch,
        MathEditBranch::Tree { path, anchor }
            if path.steps().is_empty() && anchor.offset == 1
    ));

    session.run(Command::InsertText("x".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}x\text{cd}")
    );
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo of inserted math text failed: {error:?}"));
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}{}\text{cd}")
    );
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(math_flow_has_placeholder(
        &session.math_tree_for_edit(edit, None)?.root
    ));
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo of inserted math text failed: {error:?}"));
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}x\text{cd}")
    );
    for _ in 0..8 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::InsertText("z".to_owned()))?;
    assert_ne!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}x\text{cd}")
    );
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}x\text{cd}")
    );
    session.run(Command::Undo)?;
    session.run(Command::Undo)?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}x\text{cd}")
    );
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("second undo failed: {error:?}"));
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab}{}\text{cd}")
    );
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo of math entry failed: {error:?}"));
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abcd}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    Ok(())
}
#[test]
fn starting_math_inside_nested_text_style_keeps_the_wrapper() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{\textbf{abcd}}")
        .unwrap_or_else(|error| panic!("insert formula failed: {error}"));
    session
        .run(Command::EnterMath(Direction::Backward))
        .unwrap_or_else(|error| panic!("enter formula failed: {error}"));
    for _ in 0..16 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            break;
        }
        session
            .run(Command::MoveCaret(Direction::Backward))
            .unwrap_or_else(|error| panic!("move into text failed: {error}"));
    }
    session
        .run(Command::SetMathTextSelection {
            anchor: 2,
            focus: 2,
        })
        .unwrap_or_else(|error| panic!("set text position failed: {error}"));

    session
        .begin_inline_math()
        .unwrap_or_else(|error| panic!("start math failed: {error}"));
    session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("insert math source failed: {error}"));
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{\textbf{ab$x$cd}}")
    );
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let Some(MathContent::Environment(bold)) = equation.math_tree.root.content.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(bold.kind, caret_doc::InlineEnvironmentKind::Bold);
    assert!(matches!(bold.children.first(), Some(MathContent::Text(_))));
    assert!(
        bold.children
            .iter()
            .any(|child| matches!(child, MathContent::Math(_)))
    );
    assert!(matches!(bold.children.last(), Some(MathContent::Text(_))));
    Ok(())
}

#[test]
fn starting_math_splits_known_run_properties_across_text_flows() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;
    let Some(Block::Equation(equation)) = session.document.body.first_mut() else {
        return Err(EditError::InvalidPosition);
    };
    let text =
        first_math_text_flow_mut(&mut equation.math_tree.root).ok_or(EditError::InvalidPosition)?;
    text.runs
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
        .props
        .insert("underline".to_owned(), serde_json::Value::Bool(true));

    session.begin_inline_math()?;
    session.run(Command::InsertText("x".to_owned()))?;
    let Some(Block::Equation(equation)) = session.document.body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let text_flows = equation
        .math_tree
        .root
        .content
        .iter()
        .filter_map(|content| match content {
            MathContent::Text(text) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(text_flows.len(), 2);
    assert_eq!(text_flows[0].runs[0].text, "ab");
    assert_eq!(text_flows[1].runs[0].text, "cd");
    for text in text_flows {
        assert_eq!(
            text.runs[0].props.get("underline"),
            Some(&serde_json::Value::Bool(true))
        );
    }
    Ok(())
}
#[test]
fn starting_math_rejects_unmappable_run_metadata_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;
    let Some(Block::Equation(equation)) = session.document.body.first_mut() else {
        return Err(EditError::InvalidPosition);
    };
    first_math_text_flow_mut(&mut equation.math_tree.root)
        .and_then(|text| text.runs.first_mut())
        .ok_or(EditError::InvalidPosition)?
        .extra
        .insert("futureRunField".to_owned(), serde_json::json!("opaque"));

    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    assert_eq!(
        session.begin_inline_math(),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}
#[test]
fn starting_math_normalizes_inline_fences_and_preserves_escaped_text() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{a\%bc}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 4,
    })?;
    session.run(Command::InsertInlineMath("$x$".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a\%bc}x\text{}")
    );
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    let display = first_math_text_flow(&equation.math_tree.root)
        .ok_or(EditError::InvalidPosition)?
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    assert_eq!(display, "a%bc");
    assert!(equation.math_tree.root.content.iter().any(|content| {
        matches!(content, MathContent::Math(node) if node.kind == MathNodeKind::Symbol && node.text.as_deref() == Some("x"))
    }));
    Ok(())
}
#[test]
fn starting_math_from_nonfirst_align_cell_keeps_unicode_host_offset()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("α & \\text{abc}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 1))
    ));
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;

    session.run(Command::InsertInlineMath("\\(y\\)".to_owned()))?;

    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { path, anchor })
            if path.steps().is_empty() && matches!(anchor.host, MathPositionHost::AlignCell { .. })
    ));
    let align = first_align(&session)?;
    let cell = &align.rows[0].cells[1];
    assert!(cell.root.content.iter().any(|content| {
        matches!(content, MathContent::Math(node) if node.kind == MathNodeKind::Symbol && node.text.as_deref() == Some("y"))
    }));
    session.run(Command::Undo)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abc"
    );
    session.run(Command::Redo)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));
    Ok(())
}

#[test]
fn starting_math_inside_equation_text_flow_is_undoable() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "equation")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText(r"\text{abc}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;

    session.run(Command::InsertInlineMath(r"\(y\)".to_owned()))?;
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(
        caret_math::serialize_tree(&equation.math_tree),
        r"\text{a}y\text{bc}"
    );
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { path, .. }) if path.steps().is_empty()
    ));

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abc}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::Redo)?;
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(
        caret_math::serialize_tree(&equation.math_tree),
        r"\text{a}y\text{bc}"
    );
    Ok(())
}

#[test]
fn invalid_inline_fence_in_text_island_preserves_document_and_focus() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();

    assert!(matches!(
        session.run(Command::InsertInlineMath("$x$$y$".to_owned())),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}
#[test]
fn invalid_math_source_from_a_text_island_keeps_document_and_focus() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();

    assert!(matches!(
        session.run(Command::InsertInlineMath("}".to_owned())),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}
#[test]
fn math_text_selection_replaces_and_undo_restores_the_selection() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;

    session.run(Command::InsertText("XY".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{aXYd}"));
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(edit.cursor, 3);
    assert!(matches!(
        &edit.branch,
        MathEditBranch::Text { anchor, .. } if anchor.offset == 3
    ));

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abcd}"));
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(edit.cursor, 3);
    assert!(matches!(
        &edit.branch,
        MathEditBranch::Text { anchor, .. } if anchor.offset == 1
    ));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{aXYd}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    Ok(())
}
#[test]
fn math_text_selection_delete_collapses_at_start_and_arrows_collapse_by_direction()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 3,
        focus: 1,
    })?;
    session.run(Command::Delete(Direction::Forward))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{ad}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));

    session.run(Command::Undo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(edit.cursor, 1);
    assert!(matches!(
        &edit.branch,
        MathEditBranch::Text { anchor, .. } if anchor.offset == 3
    ));
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { anchor, ..
                }) if anchor.offset == 3
    ));
    Ok(())
}
#[test]
fn math_text_selection_rejects_non_grapheme_positions_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("\\text{a\u{301}b}".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    assert_eq!(
        session.run(Command::SetMathTextSelection {
            anchor: 1,
            focus: 3
        }),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn math_text_selection_replaces_a_complete_escaped_source_mapping() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{a\textchar{5F}b}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 2,
    })?;
    session.run(Command::InsertText("X".to_owned()))?;

    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{aXb}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    Ok(())
}
#[test]
fn inserting_empty_text_deletes_a_selected_math_text_range() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 2,
    })?;
    session.run(Command::InsertText(String::new()))?;

    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{ac}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    Ok(())
}
#[test]
fn adjacent_text_islands_share_caret_navigation() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    let inserted = insert_equation_text(&mut session, r"\text{\textbf{a}}\text{b}");
    assert!(
        inserted.is_ok(),
        "inline math insertion failed: {inserted:?}"
    );
    let entered = session.run(Command::EnterMath(Direction::Backward));
    assert!(entered.is_ok(), "entering math failed: {entered:?}");
    let entered_text = session.run(Command::MoveCaret(Direction::Backward));
    assert!(
        entered_text.is_ok(),
        "moving into text island failed: {entered_text:?}"
    );
    assert!(
        matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ),
        "formula movement did not select a text island: {:?}",
        session.math_edit()
    );
    let second_path = match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    {
        MathEditBranch::Text { path, .. } => path.steps().to_vec(),
        MathEditBranch::Source => return Err(EditError::InvalidPosition),
        MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition),
    };
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));

    let moved_within = session.run(Command::MoveCaret(Direction::Backward));
    assert!(
        moved_within.is_ok(),
        "moving inside the island failed: {moved_within:?}"
    );
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    let crossed_backward = session.run(Command::MoveCaret(Direction::Backward));
    assert!(
        crossed_backward.is_ok(),
        "crossing to previous island failed: {crossed_backward:?}"
    );
    let first_path = match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    {
        MathEditBranch::Text { path, .. } => path.steps().to_vec(),
        MathEditBranch::Source => return Err(EditError::InvalidPosition),
        MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition),
    };
    assert_ne!(first_path, second_path);
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));

    let crossed_forward = session.run(Command::MoveCaret(Direction::Forward));
    assert!(
        crossed_forward.is_ok(),
        "crossing to next island failed: {crossed_forward:?}"
    );
    let next_path = match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    {
        MathEditBranch::Text { path, .. } => path.steps().to_vec(),
        MathEditBranch::Source => return Err(EditError::InvalidPosition),
        MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition),
    };
    assert_eq!(next_path, second_path);
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    Ok(())
}

#[test]
fn deleting_at_adjacent_text_island_boundaries_edits_the_neighbor_flow() -> Result<(), EditError> {
    let mut backward = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut backward, r"\text{a}\text{b}")?;
    backward.run(Command::EnterMath(Direction::Backward))?;
    backward.run(Command::MoveCaret(Direction::Backward))?;
    backward.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(backward.math_edit().map(|edit| edit.cursor), Some(0));
    backward.run(Command::Delete(Direction::Backward))?;
    let edit = backward.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let tree = backward.math_tree_for_edit(edit, path.align_cell())?;
    assert_eq!(
        path.resolve(tree).map(|text| text.runs[0].text.as_str()),
        Some("b")
    );
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(|content| match content {
                MathContent::Text(text) => Some(
                    text.runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["", "b"]
    );
    backward.run(Command::Undo)?;
    let edit = backward.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = backward.math_tree_for_edit(edit, None)?;
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(|content| match content {
                MathContent::Text(text) => Some(
                    text.runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["a", "b"]
    );

    let mut forward = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut forward, r"\text{a}\text{b}")?;
    forward.run(Command::EnterMath(Direction::Backward))?;
    forward.run(Command::MoveCaret(Direction::Backward))?;
    forward.run(Command::MoveCaret(Direction::Backward))?;
    forward.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(forward.math_edit().map(|edit| edit.cursor), Some(1));
    forward.run(Command::Delete(Direction::Forward))?;
    let edit = forward.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let tree = forward.math_tree_for_edit(edit, path.align_cell())?;
    assert_eq!(
        path.resolve(tree).map(|text| text.runs[0].text.as_str()),
        Some("a")
    );
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(|content| match content {
                MathContent::Text(text) => Some(
                    text.runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["a", ""]
    );
    Ok(())
}

#[test]
fn deletion_treats_empty_and_combining_textflows_as_one_logical_grapheme() -> Result<(), EditError>
{
    let source = "\\text{a}\\text{}\\text{\u{301}}";
    let mut backward = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut backward, source)?;
    move_into_math_text(&mut backward)?;
    backward.run(Command::Delete(Direction::Backward))?;
    let edit = backward.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = backward.math_tree_for_edit(edit, None)?;
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(|content| match content {
                MathContent::Text(text) => Some(
                    text.runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["", "", ""]
    );
    backward.run(Command::Undo)?;
    let edit = backward.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = backward.math_tree_for_edit(edit, None)?;
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(|content| match content {
                MathContent::Text(text) => Some(
                    text.runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["a", "", "\u{301}"]
    );

    let mut forward = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut forward, source)?;
    move_into_math_text(&mut forward)?;
    forward.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(forward.math_edit().map(|edit| edit.cursor), Some(0));
    forward.run(Command::Delete(Direction::Forward))?;
    let edit = forward.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = forward.math_tree_for_edit(edit, None)?;
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(|content| match content {
                MathContent::Text(text) => Some(
                    text.runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["", "", ""]
    );
    Ok(())
}

#[test]
fn caret_navigation_never_stops_inside_a_grapheme_split_across_textflows() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, "\\text{a}\\text{}\\text{\u{301}}")?;
    move_into_math_text(&mut session)?;

    session.run(Command::MoveCaret(Direction::Backward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let text = path
        .resolve_structurally(session.math_tree_for_edit(edit, None)?)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "a"
    );
    assert_eq!(edit.cursor, 0);

    session.run(Command::MoveCaret(Direction::Forward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let text = path
        .resolve_structurally(session.math_tree_for_edit(edit, None)?)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "\u{301}"
    );
    assert_eq!(edit.cursor, "\u{301}".len());
    Ok(())
}

#[test]
fn insertion_snaps_to_the_logical_grapheme_across_textflows_and_undoes() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, "\\text{}\\text{\u{301}}")?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let first = path::root_text_flow_path(tree, true, None).ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(first, 0);
    let document_before = session.document().clone();

    session.run(Command::InsertText("a".to_owned()))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let text = path
        .resolve_structurally(session.math_tree_for_edit(edit, None)?)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "\u{301}"
    );
    assert_eq!(edit.cursor, "\u{301}".len());

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &document_before);
    Ok(())
}

#[test]
fn math_text_selection_rejects_an_offset_inside_a_cross_flow_grapheme() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, "\\text{a}\\text{\u{301}}")?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let first = path::root_text_flow_path(tree, true, None).ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(first, 0);
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();

    assert!(matches!(
        session.run(Command::SetMathTextSelection {
            anchor: 1,
            focus: 1,
        }),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.document_generation(), generation_before);
    Ok(())
}

#[test]
fn math_atoms_separate_text_islands_but_empty_islands_keep_a_caret_stop() -> Result<(), EditError> {
    let mut separated = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut separated, r"\text{a}x\text{b}")?;
    separated.run(Command::EnterMath(Direction::Backward))?;
    separated.run(Command::MoveCaret(Direction::Backward))?;
    separated.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(separated.math_edit().map(|edit| edit.cursor), Some(0));
    separated.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        separated.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));

    let mut empty = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut empty, r"\text{}\text{b}")?;
    empty.run(Command::EnterMath(Direction::Backward))?;
    empty.run(Command::MoveCaret(Direction::Backward))?;
    empty.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(empty.math_edit().map(|edit| edit.cursor), Some(0));
    empty.run(Command::MoveCaret(Direction::Backward))?;
    let edit = empty.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let empty_path = path.steps().to_vec();
    let tree = empty.math_tree_for_edit(edit, path.align_cell())?;
    assert!(
        path.resolve_structurally(tree)
            .is_some_and(|text| text.runs.iter().all(|run| run.text.is_empty()))
    );
    assert_eq!(edit.cursor, 0);

    empty.run(Command::MoveCaret(Direction::Forward))?;
    let edit = empty.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert_ne!(path.steps(), empty_path);
    assert_eq!(edit.cursor, 0);
    empty.run(Command::MoveCaret(Direction::Backward))?;
    let edit = empty.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(path.steps(), empty_path);
    assert_eq!(edit.cursor, 0);
    Ok(())
}
#[test]
fn root_text_boundary_exits_and_reenters_tree_native_math_editing() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());
    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| { edit.source_projection.is_none() && edit.cursor == 2 })
    );
    Ok(())
}

#[test]
fn structural_math_navigation_keeps_projection_ranges_out_of_document() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\frac{a}{b}".to_owned()))?;
    let document_before = session.document().clone();
    let generation_before = session.document_generation();
    let history_before = session.history_state_id();

    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    for _ in 0..4 {
        session.run(Command::MoveCaret(Direction::Backward))?;
    }

    assert_eq!(session.document(), &document_before);
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_before);
    Ok(())
}

#[test]
fn tree_native_navigation_round_trips_nested_text_and_math_nodes() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(
        r"\frac{\textbf{a}}{b}".to_owned(),
    ))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let before = session.document().clone();
    let history = session.history_state_id();

    let mut visited = Vec::new();
    for _ in 0..64 {
        let Some(edit) = session.math_edit() else {
            break;
        };
        assert!(edit.source_projection.is_none());
        visited.push(match &edit.branch {
            MathEditBranch::Tree { path, .. } => format!("tree:{:?}:{}", path.steps(), edit.cursor),
            MathEditBranch::Text { path, .. } => format!("text:{:?}:{}", path.steps(), edit.cursor),
            MathEditBranch::Source => return Err(EditError::InvalidPosition),
        });
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    let backward = visited.clone();
    session.run(Command::EnterMath(Direction::Forward))?;
    let mut round_trip = Vec::new();
    for _ in 0..64 {
        let Some(edit) = session.math_edit() else {
            break;
        };
        assert!(edit.source_projection.is_none());
        round_trip.push(match &edit.branch {
            MathEditBranch::Tree { path, .. } => format!("tree:{:?}:{}", path.steps(), edit.cursor),
            MathEditBranch::Text { path, .. } => format!("text:{:?}:{}", path.steps(), edit.cursor),
            MathEditBranch::Source => return Err(EditError::InvalidPosition),
        });
        session.run(Command::MoveCaret(Direction::Forward))?;
    }

    assert!(
        backward
            .iter()
            .any(|position| position.starts_with("text:"))
    );
    assert!(
        backward
            .iter()
            .any(|position| position.starts_with("tree:"))
    );
    assert_eq!(
        round_trip,
        backward.iter().rev().cloned().collect::<Vec<_>>()
    );
    assert_eq!(session.document(), &before);
    assert_eq!(session.history_state_id(), history);
    Ok(())
}

#[test]
fn parsed_math_text_uses_the_same_grapheme_editing_as_tree_input() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, "xy")?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));

    session.run(Command::MoveCaret(Direction::Backward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(edit.cursor, 1);
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(inline_source(&session).as_deref(), Some("y"));
    Ok(())
}

#[test]
fn tree_operator_combining_mark_is_one_editable_grapheme() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("+".to_owned()))?;
    session.run(Command::InsertText("\u{0301}".to_owned()))?;
    let (paragraph_id, math_id) = {
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        (paragraph_id.clone(), math_id.clone())
    };
    let tree = &find_inline_math_in_blocks(&session.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree;
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(
        literal_symbol_text(&tree.root.content[0]),
        Some("+\u{0301}")
    );
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    session.run(Command::Delete(Direction::Backward))?;
    assert!(inline_source(&session).is_some_and(|source| source.is_empty()));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some("+\u{0301}"));
    session.run(Command::Redo)?;
    assert!(inline_source(&session).is_some_and(|source| source.is_empty()));
    Ok(())
}

#[test]
fn tree_literal_prepend_merge_updates_atom_class_and_round_trips() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("+".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::InsertText("\u{0600}".to_owned()))?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &edit.target
    else {
        return Err(EditError::InvalidPosition);
    };
    let tree = &find_inline_math_in_blocks(&session.document().body, paragraph_id, math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree;
    assert_eq!(tree.root.content.len(), 1);
    let MathContent::Math(symbol) = &tree.root.content[0] else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(symbol.text.as_deref(), Some("\u{0600}+"));
    assert_eq!(
        symbol.attributes.get("atomClass").map(String::as_str),
        Some("ord")
    );

    let serialized = caret_math::serialize_tree(tree);
    let parsed = caret_math::parse(&serialized, caret_math::MathInputKind::Inline)
        .map_err(|_| EditError::InvalidPosition)?;
    let reparsed = match parsed {
        caret_math::ParsedMath::Tree(tree) => tree,
        caret_math::ParsedMath::Align(_) => return Err(EditError::InvalidPosition),
    };
    let MathContent::Math(reparsed_symbol) = &reparsed.root.content[0] else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(reparsed_symbol.text.as_deref(), Some("\u{0600}+"));
    assert_eq!(
        reparsed_symbol
            .attributes
            .get("atomClass")
            .map(String::as_str),
        Some("ord")
    );
    Ok(())
}

fn append_test_literal_symbol(session: &mut Session, text: &str) -> Result<(), EditError> {
    let (paragraph_id, math_id) = {
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        (paragraph_id.clone(), math_id.clone())
    };
    let first_character = text.chars().next().ok_or(EditError::InvalidPosition)?;
    let paragraph = session
        .document
        .body
        .iter_mut()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) if paragraph.id == paragraph_id => Some(paragraph),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    math.math_tree
        .root
        .content
        .push(MathContent::Math(MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Symbol,
            text: Some(text.to_owned()),
            attributes: BTreeMap::from([(
                "atomClass".to_owned(),
                caret_math::symbols::literal_class(first_character)
                    .as_str()
                    .to_owned(),
            )]),
            children: Vec::new(),
            extra: BTreeMap::new(),
        }));
    Ok(())
}

#[test]
fn tree_native_literal_symbols_join_combining_and_regional_graphemes() -> Result<(), EditError> {
    for (first, second, expected) in [("a", "\u{301}", "a\u{301}"), ("🇦", "🇧", "🇦🇧")] {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.begin_inline_math()?;
        session.run(Command::InsertText(first.to_owned()))?;
        session.run(Command::InsertText(second.to_owned()))?;
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &session
            .math_edit()
            .ok_or(EditError::InvalidPosition)?
            .target
        else {
            return Err(EditError::InvalidPosition);
        };
        let tree = find_inline_math_in_blocks(&session.document().body, paragraph_id, math_id)
            .ok_or(EditError::InvalidPosition)?
            .math_tree
            .clone();
        assert_eq!(tree.root.content.len(), 1);
        assert_eq!(literal_symbol_text(&tree.root.content[0]), Some(expected));
        session.run(Command::MoveCaret(Direction::Backward))?;
        assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
        session.run(Command::MoveCaret(Direction::Forward))?;
        assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
        session.run(Command::Delete(Direction::Backward))?;
        assert!(inline_source(&session).is_some_and(|source| source.is_empty()));
    }
    Ok(())
}

#[test]
fn regional_indicator_resegmentation_keeps_tree_caret_and_history_consistent()
-> Result<(), EditError> {
    let mut insertion = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insertion.begin_inline_math()?;
    insertion.run(Command::InsertText("🇦🇧".to_owned()))?;
    insertion.run(Command::MoveCaret(Direction::Backward))?;
    insertion.run(Command::InsertText("🇨".to_owned()))?;

    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &insertion
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .target
    else {
        return Err(EditError::InvalidPosition);
    };
    let paragraph_id = paragraph_id.clone();
    let math_id = math_id.clone();
    let tree = find_inline_math_in_blocks(&insertion.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    let symbols = tree
        .root
        .content
        .iter()
        .filter_map(literal_symbol_text)
        .collect::<Vec<_>>();
    assert_eq!(symbols, ["🇨🇦", "🇧"]);
    assert_eq!(insertion.math_edit().map(|edit| edit.cursor), Some(1));

    insertion.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(insertion.math_edit().map(|edit| edit.cursor), Some(0));
    insertion.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(insertion.math_edit().map(|edit| edit.cursor), Some(1));
    insertion.run(Command::Undo)?;
    let tree = find_inline_math_in_blocks(&insertion.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(literal_symbol_text(&tree.root.content[0]), Some("🇦🇧"));
    insertion.run(Command::Redo)?;
    let tree = find_inline_math_in_blocks(&insertion.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(literal_symbol_text)
            .collect::<Vec<_>>(),
        ["🇨🇦", "🇧"]
    );

    let mut attached_mark = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    attached_mark.begin_inline_math()?;
    attached_mark.run(Command::InsertText("🇦🇧\u{301}".to_owned()))?;
    attached_mark.run(Command::MoveCaret(Direction::Backward))?;
    attached_mark.run(Command::InsertText("🇨".to_owned()))?;
    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &attached_mark
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .target
    else {
        return Err(EditError::InvalidPosition);
    };
    let tree = find_inline_math_in_blocks(&attached_mark.document().body, paragraph_id, math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(literal_symbol_text)
            .collect::<Vec<_>>(),
        ["🇨🇦", "🇧\u{301}"]
    );
    assert_eq!(attached_mark.math_edit().map(|edit| edit.cursor), Some(1));

    let mut deletion = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    deletion.begin_inline_math()?;
    deletion.run(Command::InsertText("🇦x🇧".to_owned()))?;
    deletion.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(deletion.math_edit().map(|edit| edit.cursor), Some(2));
    deletion.run(Command::Delete(Direction::Backward))?;

    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &deletion
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .target
    else {
        return Err(EditError::InvalidPosition);
    };
    let paragraph_id = paragraph_id.clone();
    let math_id = math_id.clone();
    let tree = find_inline_math_in_blocks(&deletion.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(literal_symbol_text(&tree.root.content[0]), Some("🇦🇧"));
    assert_eq!(deletion.math_edit().map(|edit| edit.cursor), Some(1));
    deletion.run(Command::Undo)?;
    let tree = find_inline_math_in_blocks(&deletion.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(
        tree.root
            .content
            .iter()
            .filter_map(literal_symbol_text)
            .collect::<Vec<_>>(),
        ["🇦", "x", "🇧"]
    );
    deletion.run(Command::Redo)?;
    let tree = find_inline_math_in_blocks(&deletion.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(literal_symbol_text(&tree.root.content[0]), Some("🇦🇧"));
    Ok(())
}

#[test]
fn regional_indicator_resegmentation_rejects_opaque_node_changes_atomically()
-> Result<(), EditError> {
    let mut insertion = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insertion.begin_inline_math()?;
    insertion.run(Command::InsertText("🇦🇧".to_owned()))?;
    let (paragraph_id, math_id) = {
        let edit = insertion.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        (paragraph_id.clone(), math_id.clone())
    };
    let paragraph = insertion
        .document
        .body
        .iter_mut()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) if paragraph.id == paragraph_id => Some(paragraph),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let Some(MathContent::Math(opaque)) = math.math_tree.root.content.first_mut() else {
        return Err(EditError::InvalidPosition);
    };
    opaque
        .extra
        .insert("futureSymbolValue".to_owned(), serde_json::json!(true));
    insertion.run(Command::MoveCaret(Direction::Backward))?;

    let document_before = insertion.document().clone();
    let edit_before = insertion.math_edit().cloned();
    let selection_before = insertion.selection().clone();
    let generation_before = insertion.document_generation();
    let history_before = insertion.history_state_id();
    assert_eq!(
        insertion.run(Command::InsertText("🇨".to_owned())),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(insertion.document(), &document_before);
    assert_eq!(insertion.math_edit(), edit_before.as_ref());
    assert_eq!(insertion.selection(), &selection_before);
    assert_eq!(insertion.document_generation(), generation_before);
    assert_eq!(insertion.history_state_id(), history_before);

    let mut deletion = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    deletion.begin_inline_math()?;
    deletion.run(Command::InsertText("🇨x🇦🇧".to_owned()))?;
    let (paragraph_id, math_id) = {
        let edit = deletion.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        (paragraph_id.clone(), math_id.clone())
    };
    let paragraph = deletion
        .document
        .body
        .iter_mut()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) if paragraph.id == paragraph_id => Some(paragraph),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let Some(MathContent::Math(opaque)) = math
        .math_tree
        .root
        .content
        .iter_mut()
        .find(|content| symbol_text(content).is_some_and(|text| text == "🇦🇧"))
    else {
        return Err(EditError::InvalidPosition);
    };
    opaque
        .extra
        .insert("futureSymbolValue".to_owned(), serde_json::json!(true));
    deletion.run(Command::MoveCaret(Direction::Backward))?;

    let document_before = deletion.document().clone();
    let edit_before = deletion.math_edit().cloned();
    let selection_before = deletion.selection().clone();
    let generation_before = deletion.document_generation();
    let history_before = deletion.history_state_id();
    assert_eq!(
        deletion.run(Command::Delete(Direction::Backward)),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(deletion.document(), &document_before);
    assert_eq!(deletion.math_edit(), edit_before.as_ref());
    assert_eq!(deletion.selection(), &selection_before);
    assert_eq!(deletion.document_generation(), generation_before);
    assert_eq!(deletion.history_state_id(), history_before);
    Ok(())
}

#[test]
fn tree_symbol_deletion_resegments_combining_graphemes_and_history() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("e+".to_owned()))?;
    append_test_literal_symbol(&mut session, "\u{301}")?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    let before_deletion = session.document().clone();

    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::Delete(Direction::Forward))?;

    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .target
    else {
        return Err(EditError::InvalidPosition);
    };
    let paragraph_id = paragraph_id.clone();
    let math_id = math_id.clone();
    let tree = find_inline_math_in_blocks(&session.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(literal_symbol_text(&tree.root.content[0]), Some("e\u{301}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));

    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    session.run(Command::Undo)?;
    assert_eq!(session.document(), &before_deletion);
    session.run(Command::Redo)?;
    let tree = find_inline_math_in_blocks(&session.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(literal_symbol_text(&tree.root.content[0]), Some("e\u{301}"));

    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::Delete(Direction::Backward))?;
    assert!(inline_source(&session).is_some_and(|source| source.is_empty()));
    session.run(Command::Undo)?;
    let tree = find_inline_math_in_blocks(&session.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), 1);
    assert_eq!(literal_symbol_text(&tree.root.content[0]), Some("e\u{301}"));
    session.run(Command::Redo)?;
    assert!(inline_source(&session).is_some_and(|source| source.is_empty()));
    Ok(())
}

#[test]
fn tree_symbol_deletion_rejects_opaque_resegmentation_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("e+".to_owned()))?;
    append_test_literal_symbol(&mut session, "\u{301}")?;
    let (paragraph_id, math_id) = {
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        (paragraph_id.clone(), math_id.clone())
    };
    let paragraph = session
        .document
        .body
        .iter_mut()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) if paragraph.id == paragraph_id => Some(paragraph),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let Some(MathContent::Math(opaque)) = math.math_tree.root.content.get_mut(2) else {
        return Err(EditError::InvalidPosition);
    };
    opaque
        .extra
        .insert("futureSymbolValue".to_owned(), serde_json::json!(true));

    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let selection_before = session.selection().clone();
    let generation_before = session.document_generation();
    let history_before = session.history_state_id();
    assert_eq!(
        session.run(Command::Delete(Direction::Forward)),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_before);
    Ok(())
}

#[test]
fn tree_native_large_regional_indicator_input_is_batched() -> Result<(), EditError> {
    const REGIONAL_INDICATOR_COUNT: usize = 2_048;
    let source = "🇦".repeat(REGIONAL_INDICATOR_COUNT);
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText(source))?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &edit.target
    else {
        return Err(EditError::InvalidPosition);
    };
    let tree = find_inline_math_in_blocks(&session.document().body, paragraph_id, math_id)
        .ok_or(EditError::InvalidPosition)?
        .math_tree
        .clone();
    assert_eq!(tree.root.content.len(), REGIONAL_INDICATOR_COUNT / 2);
    assert!(
        tree.root
            .content
            .iter()
            .all(|content| literal_symbol_text(content) == Some("🇦🇦"))
    );
    Ok(())
}

#[test]
fn tree_grapheme_merge_preserves_unknown_symbol_metadata_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("👩👩".to_owned()))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditTarget::Inline {
        paragraph_id,
        math_id,
    } = &edit.target
    else {
        return Err(EditError::InvalidPosition);
    };
    let paragraph_id = paragraph_id.clone();
    let math_id = math_id.clone();
    let paragraph = session
        .document
        .body
        .iter_mut()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) if paragraph.id == paragraph_id => Some(paragraph),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let Some(MathContent::Math(right)) = math.math_tree.root.content.get_mut(1) else {
        return Err(EditError::InvalidPosition);
    };
    right
        .extra
        .insert("futureSymbolValue".to_owned(), serde_json::json!(true));
    session.run(Command::MoveCaret(Direction::Backward))?;

    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();
    let history_before = session.history_state_id();
    assert_eq!(
        session.run(Command::InsertText("\u{200d}".to_owned())),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_before);
    Ok(())
}

#[test]
fn tree_grapheme_merge_keeps_command_symbols_atomic() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\%".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let (paragraph_id, math_id) = {
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert!(matches!(edit.branch, MathEditBranch::Tree { .. }));
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        (paragraph_id.clone(), math_id.clone())
    };
    let symbol_id = find_inline_math_in_blocks(&session.document().body, &paragraph_id, &math_id)
        .and_then(|math| math.math_tree.root.content.first())
        .and_then(|content| match content {
            MathContent::Math(node) => Some(node.id.clone()),
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;

    session.run(Command::InsertText("\u{301}".to_owned()))?;
    let math = find_inline_math_in_blocks(&session.document().body, &paragraph_id, &math_id)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(math.math_tree.root.content.len(), 2);
    assert!(matches!(
        math.math_tree.root.content.first(),
        Some(MathContent::Math(node)) if node.id == symbol_id && node.text.as_deref() == Some("%")
    ));
    assert!(matches!(
        math.math_tree.root.content.get(1),
        Some(MathContent::Math(node)) if node.text.as_deref() == Some("\u{301}")
    ));
    Ok(())
}

#[test]
fn empty_fixed_math_operand_backspace_exits_or_removes_structure() -> Result<(), EditError> {
    for (source, expected_source) in [(r"\frac{x}{}", r"\frac{x}{}"), (r"\frac{}{}", "")] {
        let Block::Equation(equation) = display_math_block(false, source)? else {
            return Err(EditError::InvalidPosition);
        };
        let id = equation.id.clone();
        let mut document = Document::blank();
        document.body.push(Block::Equation(equation));
        let mut session = Session::new(document)?;
        let position = MathTreePosition {
            host: MathPositionHost::Equation {
                owner_id: id.clone(),
            },
            path: MathSequencePath::new(vec![MathSequenceStep::Operand {
                content_index: 0,
                operand_index: 1,
            }]),
            offset: 0,
            affinity: MathPositionAffinity::Downstream,
        };
        session.math_edit = Some(MathEdit {
            target: MathEditTarget::Block {
                block_id: id,
                align: false,
            },
            source_projection: None,
            source_draft: None,
            revision: session.current_math_edit_revision(),
            cursor: 0,
            branch: MathEditBranch::Tree {
                path: position.path.clone(),
                anchor: position,
            },
        });
        session.run(Command::Delete(Direction::Backward))?;
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree { path, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        assert!(path.steps().is_empty());
        assert_eq!(inline_source(&session).as_deref(), Some(expected_source));
    }
    Ok(())
}

#[test]
fn empty_math_operand_backspace_preserves_opaque_metadata_atomically() -> Result<(), EditError> {
    for annotate_operand in [false, true] {
        let Block::Equation(mut equation) = display_math_block(false, r"\frac{}{}")? else {
            return Err(EditError::InvalidPosition);
        };
        let id = equation.id.clone();
        let Some(MathContent::Math(node)) = equation.math_tree.root.content.first_mut() else {
            return Err(EditError::InvalidPosition);
        };
        if annotate_operand {
            node.children
                .get_mut(1)
                .ok_or(EditError::InvalidPosition)?
                .extra
                .insert("vendorOperand".to_owned(), serde_json::json!("preserve"));
        } else {
            node.extra
                .insert("vendorNode".to_owned(), serde_json::json!("preserve"));
        }

        let mut document = Document::blank();
        document.body.push(Block::Equation(equation));
        let mut session = Session::new(document)?;
        let position = MathTreePosition {
            host: MathPositionHost::Equation {
                owner_id: id.clone(),
            },
            path: MathSequencePath::new(vec![MathSequenceStep::Operand {
                content_index: 0,
                operand_index: 1,
            }]),
            offset: 0,
            affinity: MathPositionAffinity::Downstream,
        };
        session.math_edit = Some(MathEdit {
            target: MathEditTarget::Block {
                block_id: id,
                align: false,
            },
            source_projection: None,
            source_draft: None,
            revision: session.current_math_edit_revision(),
            cursor: 0,
            branch: MathEditBranch::Tree {
                path: position.path.clone(),
                anchor: position,
            },
        });

        let document_before = session.document().clone();
        let selection_before = session.selection().clone();
        let edit_before = session.math_edit().cloned();
        let revision_before = session.current_math_edit_revision();
        let generation_before = session.document_generation();
        let history_before = session.history_state_id();
        let undo_before = session.can_undo();
        let redo_before = session.can_redo();

        assert!(matches!(
            session.run(Command::Delete(Direction::Backward)),
            Err(EditError::UnsupportedOpaqueContent)
        ));
        assert_eq!(session.document(), &document_before);
        assert_eq!(session.selection(), &selection_before);
        assert_eq!(session.math_edit(), edit_before.as_ref());
        assert_eq!(session.current_math_edit_revision(), revision_before);
        assert_eq!(session.document_generation(), generation_before);
        assert_eq!(session.history_state_id(), history_before);
        assert_eq!(session.can_undo(), undo_before);
        assert_eq!(session.can_redo(), redo_before);
    }
    Ok(())
}

#[test]
fn empty_align_cell_backspace_keeps_other_cells_and_moves_out() -> Result<(), EditError> {
    let Block::Align(align) = display_math_block(true, "x&")? else {
        return Err(EditError::InvalidPosition);
    };
    let id = align.id.clone();
    let row_id = align.rows[0].id.clone();
    let cell_id = align.rows[0].cells[1].root.id.clone();
    let mut document = Document::blank();
    document.body.push(Block::Align(align));
    let mut session = Session::new(document)?;
    let position = MathTreePosition {
        host: MathPositionHost::AlignCell { row_id, cell_id },
        path: MathSequencePath::root(),
        offset: 0,
        affinity: MathPositionAffinity::Downstream,
    };
    session.math_edit = Some(MathEdit {
        target: MathEditTarget::Block {
            block_id: id,
            align: true,
        },
        source_projection: None,
        source_draft: None,
        revision: session.current_math_edit_revision(),
        cursor: 0,
        branch: MathEditBranch::Tree {
            path: position.path.clone(),
            anchor: position,
        },
    });

    session.run(Command::Delete(Direction::Backward))?;
    let align = first_align(&session)?;
    assert_eq!(align.rows[0].cells.len(), 2);
    assert!(!align.rows[0].cells[0].root.content.is_empty());
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { anchor, .. })
            if matches!(anchor.host, MathPositionHost::AlignCell { .. })
    ));
    Ok(())
}

#[test]
fn superscript_template_uses_previous_tree_content_as_its_base() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.math_insert_template("^{}")?;
    assert_eq!(inline_source(&session).as_deref(), Some("x^{}"));
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Tree { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert!(matches!(
        path.steps(),
        [MathSequenceStep::Operand {
            operand_index: 1,
            ..
        }]
    ));
    Ok(())
}

#[test]
fn escape_cancels_math_text_composition_without_exiting_formula() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\frac{1}{\text{ab}}".to_owned()))?;
    move_into_math_text(&mut session)?;
    let before = session.document().clone();
    session.run(Command::UpdateComposition {
        id: 902,
        preedit: "中".to_owned(),
    })?;

    let effect = session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(effect.composition_changed);
    assert_eq!(session.document(), &before);
    assert!(session.composition_preview_with_range(0).is_err());
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| { matches!(edit.branch, MathEditBranch::Text { .. }) })
    );
    Ok(())
}

#[test]
fn escaped_math_text_demotion_preserves_the_display_caret() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{a\%b}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), r"\frac{1}{\text{a\%b}}");
    assert_eq!(session.selection().focus.offset, 16);
    Ok(())
}

#[test]
fn math_text_composition_previews_and_commits_through_the_tree_transaction() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\frac{1}{\text{ab}}".to_owned()))?;
    move_into_math_text(&mut session).unwrap_or_else(|error| panic!("enter text flow: {error:?}"));
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    let before = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    let before_generation = session.document_generation();
    session
        .run(Command::UpdateComposition {
            id: 81,
            preedit: "中".to_owned(),
        })
        .unwrap_or_else(|error| panic!("update composition: {error:?}"));

    let (preview, range) = session
        .composition_preview_with_range("中".encode_utf16().count())
        .unwrap_or_else(|error| panic!("preview composition: {error:?}"));
    assert!(matches!(
        range,
        crate::editing::CompositionRange::MathText { .. }
    ));
    assert_eq!(inline_source(&session).as_deref(), Some(before.as_str()));
    assert_eq!(
        inline_source(&preview).as_deref(),
        Some(r"\frac{1}{\text{a中b}}")
    );

    session
        .run(Command::CommitComposition {
            id: 81,
            text: "中".to_owned(),
        })
        .unwrap_or_else(|error| panic!("commit composition: {error:?}"));
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\frac{1}{\text{a中b}}")
    );
    assert_eq!(session.document_generation(), before_generation + 1);
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo composition: {error:?}"));
    assert_eq!(inline_source(&session).as_deref(), Some(before.as_str()));
    Ok(())
}

#[test]
fn math_text_composition_replacement_uses_utf16_and_is_one_undoable_edit() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{A😀B}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 5,
        focus: 5,
    })?;
    let before = inline_source(&session).ok_or(EditError::InvalidPosition)?;

    session.commit_composition_replacement(None, 82, 83, -2, 2, "中")?;
    assert!(session.composition().is_none());
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\frac{1}{\text{A中B}}")
    );
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(before.as_str()));
    Ok(())
}

#[test]
fn math_text_composition_replaces_a_cross_environment_selection_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{ab}\textbf{cd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    let anchor = MathTextPosition::new(island.paths[0].clone(), 1, MathPositionAffinity::Upstream);
    let focus = MathTextPosition::new(island.paths[1].clone(), 1, MathPositionAffinity::Upstream);
    session.set_math_text_selection_positions(anchor, focus)?;
    let generation_before = session.document_generation();

    session.commit_composition_replacement(None, 87, 88, 0, 0, "X")?;
    let (island, _, _) = session
        .math_text_selection_island_offsets()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display(), "aXd");
    assert_eq!(session.document_generation(), generation_before + 1);
    session.run(Command::Undo)?;
    let (island, _, _) = session
        .math_text_selection_island_offsets()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display(), "abcd");
    Ok(())
}

#[test]
fn math_text_selection_api_rejects_pending_composition_and_stale_revision() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\frac{1}{\text{ab}}".to_owned()))?;
    move_into_math_text(&mut session)?;
    let context = session
        .math_text_input_context()
        .ok_or(EditError::InvalidPosition)?;

    session.run(Command::UpdateComposition {
        id: 87,
        preedit: "中".to_owned(),
    })?;
    let pending_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        session.set_math_text_selection_positions(context.anchor.clone(), context.focus.clone()),
        Err(EditError::CompositionPending)
    );
    assert_eq!(session.math_edit(), Some(&pending_edit));
    assert_eq!(
        session.composition().map(|composition| composition.id),
        Some(87)
    );

    session.run(Command::CancelComposition { id: 87 })?;
    let revision = session.current_math_edit_revision();
    let edit = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    edit.revision.document_generation += 1;
    let stale_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        session.set_math_text_selection_positions(context.anchor, context.focus),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.math_edit(), Some(&stale_edit));

    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision = revision;
    Ok(())
}

#[test]
fn invalid_math_text_composition_replacement_keeps_the_pending_state() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\frac{1}{\text{ab}}".to_owned()))?;
    move_into_math_text(&mut session)?;
    let before_document = session.document().clone();
    let before_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let before_history = session.history_state_id();
    session.run(Command::UpdateComposition {
        id: 84,
        preedit: "中".to_owned(),
    })?;

    assert_eq!(
        session.commit_composition_replacement(Some(84), 85, 86, -200, 1, "x"),
        Err(EditError::InvalidUtf16Offset)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), Some(&before_edit));
    assert_eq!(session.history_state_id(), before_history);
    assert_eq!(
        session.composition().map(|composition| composition.id),
        Some(84)
    );
    Ok(())
}

#[test]
fn tree_branch_edit_restores_focus_through_history_without_a_source_projection()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{a}+x")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::MoveCaret(Direction::Forward))?;

    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    let before = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    session.run(Command::InsertText("q".to_owned()))?;
    let after = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    assert_ne!(after, before);
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(before.as_str()));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(after.as_str()));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));

    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(inline_source(&session).as_deref(), Some(before.as_str()));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(after.as_str()));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(before.as_str()));
    Ok(())
}

#[test]
fn empty_inline_math_starts_tree_native_and_undo_keeps_its_focus() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.begin_inline_math()?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));

    session.run(Command::InsertText("x".to_owned()))?;
    let inserted = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    assert_eq!(inserted, "x");
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(""));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(inserted.as_str()));
    Ok(())
}

#[test]
fn tree_native_mod_backspace_downgrades_and_undo_restores_editable_focus() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("xy".to_owned()))?;
    let before_demotion = session.document().clone();
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));

    session.run(Command::EditorKey(EditorKey::ModBackspace))?;
    assert!(session.math_edit().is_none());
    assert_eq!(first_paragraph_text(&session), "xy");

    session.run(Command::Undo)?;
    assert_eq!(session.document(), &before_demotion);
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::InsertText("z".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("xyz"));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    Ok(())
}

#[test]
fn tree_native_mod_backspace_round_trips_escaped_graphemes_and_literal_apostrophe()
-> Result<(), EditError> {
    for input in [
        "\u{600}#x'",
        "#\u{301}",
        "'\u{301}",
        "%\u{301}",
        "&\u{301}",
        "$\u{301}",
        "_\u{301}",
        "{\u{301}",
        "}\u{301}",
        "\\\u{301}",
        "~\u{301}",
        "^\u{301}",
    ] {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.begin_inline_math()?;
        session.run(Command::InsertText(input.to_owned()))?;
        let source = inline_source(&session).ok_or(EditError::InvalidPosition)?;
        let parsed = caret_math::parse(&source, caret_math::MathInputKind::Inline)
            .map_err(|_| EditError::InvalidPosition)?;
        assert!(matches!(parsed, caret_math::ParsedMath::Tree(_)));

        session.run(Command::EditorKey(EditorKey::ModBackspace))?;
        assert!(session.math_edit().is_none());
        assert_eq!(first_paragraph_text(&session), source);
    }
    Ok(())
}

#[test]
fn tree_native_template_enters_the_first_operand_and_restores_focus_in_history()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.begin_inline_math()?;
    session.math_insert_template(r"\frac{}{}")?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(edit.source_projection.is_none());
    let MathEditBranch::Tree { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    assert!(matches!(
        path.steps(),
        [MathSequenceStep::Operand {
            operand_index: 0,
            ..
        }]
    ));

    session.run(Command::InsertText("x".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\frac{x}{}"));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\frac{}{}"));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\frac{x}{}"));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    Ok(())
}

#[test]
fn tree_native_template_replaces_placeholder_in_a_later_operand() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.begin_inline_math()?;
    session.math_insert_template(r"\frac{}{}")?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::InsertText("y".to_owned()))?;

    assert_eq!(inline_source(&session).as_deref(), Some(r"\frac{x}{y}"));
    Ok(())
}

#[test]
fn tree_native_script_closing_brace_exits_slot_atomically() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.begin_inline_math()?;
    session.run(Command::InsertText("x^".to_owned()))?;
    session.run(Command::InsertText("}".to_owned()))?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(
        matches!(edit.branch, MathEditBranch::Tree { ref path, .. } if path.steps().is_empty())
    );
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();
    let history_before = session.history_state_id();
    assert!(matches!(
        session.run(Command::InsertText("}".to_owned())),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.document_generation(), generation_before);
    assert_eq!(session.history_state_id(), history_before);
    Ok(())
}

#[test]
fn tree_boundary_delete_removes_one_text_grapheme_and_preserves_text_metadata()
-> Result<(), EditError> {
    let Block::Equation(mut equation) = display_math_block(false, r"x+\text{abc}+y")? else {
        return Err(EditError::InvalidPosition);
    };
    let text_index = equation
        .math_tree
        .root
        .content
        .iter()
        .position(|content| matches!(content, MathContent::Text(_)))
        .ok_or(EditError::InvalidPosition)?;
    let Some(MathContent::Text(text)) = equation.math_tree.root.content.get_mut(text_index) else {
        return Err(EditError::InvalidPosition);
    };
    text.extra.insert(
        "vendorTextFlow".to_owned(),
        serde_json::json!({"keep": true}),
    );
    let equation_id = equation.id.clone();
    let mut document = Document::blank();
    document.body.push(Block::Equation(equation));
    let mut session = Session::new(document)?;
    let position = MathTreePosition {
        host: MathPositionHost::Equation {
            owner_id: equation_id.clone(),
        },
        path: MathSequencePath::root(),
        offset: text_index,
        affinity: MathPositionAffinity::Downstream,
    };
    session.math_edit = Some(MathEdit {
        target: MathEditTarget::Block {
            block_id: equation_id.clone(),
            align: false,
        },
        source_projection: None,
        source_draft: None,
        revision: session.current_math_edit_revision(),
        cursor: text_index,
        branch: MathEditBranch::Tree {
            path: MathSequencePath::root(),
            anchor: position,
        },
    });

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    let text_path = tree_position::text_path_for_content(
        match &edit.branch {
            MathEditBranch::Tree { anchor, .. } => anchor,
            _ => return Err(EditError::InvalidPosition),
        },
        text_index,
        None,
    );
    assert!(text_path.resolve_structurally(tree).is_some());
    assert!(path::contiguous_text_flow_text(tree, &text_path).is_some());

    session.run(Command::Delete(Direction::Forward))?;
    let Some(Block::Equation(updated)) = find_block_by_id(&session.document().body, &equation_id)
    else {
        return Err(EditError::InvalidPosition);
    };
    let Some(MathContent::Text(updated_text)) = updated.math_tree.root.content.get(text_index)
    else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(
        updated_text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "bc"
    );
    assert_eq!(
        updated_text.extra["vendorTextFlow"],
        serde_json::json!({"keep": true})
    );
    assert_eq!(
        caret_math::serialize_tree(&updated.math_tree),
        r"x+\text{bc}+y"
    );
    Ok(())
}

#[test]
fn tree_native_align_text_navigation_crosses_cells_in_both_directions() -> Result<(), EditError> {
    let (parsed_align, projection) =
        display_math_block_with_projection(true, r"\text{a}&\text{b}", NodeId::generate())?;
    let Block::Align(align_block) = parsed_align else {
        return Err(EditError::InvalidPosition);
    };
    let block_id = align_block.id.clone();
    let row = align_block.rows.first().ok_or(EditError::InvalidPosition)?;
    let first_cell = row.cells.first().ok_or(EditError::InvalidPosition)?;
    let row_id = row.id.clone();
    let first_cell_id = first_cell.root.id.clone();
    let source_map = projection
        .align()
        .and_then(|map| map.cell_tree(0, 0))
        .ok_or(EditError::InvalidPosition)?;
    let range = first_text_source_range(&first_cell.root, source_map.root())
        .ok_or(EditError::InvalidPosition)?;
    let text_path = path::path_at_source_boundary(source_map, range.start, true, Some((0, 0)))
        .ok_or(EditError::InvalidPosition)?;
    let mut document = Document::blank();
    document.body.push(Block::Align(align_block));
    let mut session = Session::new(document)?;
    session.math_edit = Some(MathEdit {
        target: MathEditTarget::Block {
            block_id: block_id.clone(),
            align: true,
        },
        source_projection: None,
        source_draft: None,
        revision: session.current_math_edit_revision(),
        cursor: 1,
        branch: MathEditBranch::Text {
            path: text_path.clone(),
            anchor: MathTextPosition::new(text_path, 1, MathPositionAffinity::Downstream),
            focus_affinity: MathPositionAffinity::Downstream,
        },
    });

    session.run(Command::MoveCaret(Direction::Forward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        match &edit.branch {
            MathEditBranch::Text { path, .. } => path.align_cell(),
            MathEditBranch::Tree { anchor, .. } => {
                session.align_cell_for_position(edit, anchor)?
            }
            MathEditBranch::Source => None,
        },
        Some((0, 1))
    );
    assert_eq!(active_math_text_flow(&session)?.runs[0].text, "b");
    session.run(Command::MoveCaret(Direction::Backward))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        match &edit.branch {
            MathEditBranch::Text { path, .. } => path.align_cell(),
            MathEditBranch::Tree { anchor, .. } => {
                session.align_cell_for_position(edit, anchor)?
            }
            MathEditBranch::Source => None,
        },
        Some((0, 0))
    );
    let Some(MathEditBranch::Text { path, .. }) = session.math_edit().map(|edit| &edit.branch)
    else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(path.align_cell(), Some((0, 0)));
    assert_eq!(active_math_text_flow(&session)?.runs[0].text, "a");
    assert_eq!(row_id, first_align(&session)?.rows[0].id);
    assert_eq!(
        first_cell_id,
        first_align(&session)?.rows[0].cells[0].root.id
    );
    Ok(())
}

#[test]
fn tree_native_align_separators_create_stable_rows_cells_and_history() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.insert_display_math(true)?;
    session.run(Command::InsertText("a".to_owned()))?;
    let align_id = first_align(&session)?.id.clone();
    let original_row_id = first_align(&session)?.rows[0].id.clone();
    let original_cell_id = first_align(&session)?.rows[0].cells[0].root.id.clone();
    session.run(Command::InsertText("&".to_owned()))?;
    let after_column = first_align(&session)?;
    assert_eq!(after_column.rows[0].id, original_row_id);
    assert_eq!(after_column.rows[0].cells[0].root.id, original_cell_id);
    assert_eq!(
        caret_math::serialize_tree(&after_column.rows[0].cells[0]),
        "a"
    );
    let second_cell_id = after_column.rows[0].cells[1].root.id.clone();
    session.run(Command::InsertText("b".to_owned()))?;
    session.run(Command::EditorKey(EditorKey::Enter))?;
    let after_row = first_align(&session)?;
    assert_eq!(after_row.rows.len(), 2);
    assert_eq!(after_row.rows[0].id, original_row_id);
    let new_row_id = after_row.rows[1].id.clone();
    assert_eq!(after_row.rows[0].cells[1].root.id, second_cell_id);
    let new_cell_id = after_row.rows[1].cells[1].root.id.clone();
    session.run(Command::InsertText("c".to_owned()))?;
    assert_eq!(
        caret_math::serialize_tree(&first_align(&session)?.rows[1].cells[1]),
        "c"
    );

    session.run(Command::Undo)?;
    assert_eq!(first_align(&session)?.rows[1].cells[1].root.id, new_cell_id);
    session.run(Command::Undo)?;
    assert_eq!(first_align(&session)?.rows.len(), 1);
    assert_eq!(
        first_align(&session)?.rows[0].cells[1].root.id,
        second_cell_id
    );
    session.run(Command::Redo)?;
    assert_eq!(first_align(&session)?.rows[1].id, new_row_id);
    session.run(Command::Redo)?;
    assert_eq!(first_align(&session)?.rows[1].cells[1].root.id, new_cell_id);
    assert_eq!(first_align(&session)?.id, align_id);
    Ok(())
}

#[test]
fn tree_native_scripts_and_groups_build_structure_during_typing() -> Result<(), EditError> {
    for (inputs, expected_kind, expected_source) in [
        (
            ["x", "^", "2"],
            caret_doc::MathNodeKind::Superscript,
            "x^{2}",
        ),
        (["y", "_", "1"], caret_doc::MathNodeKind::Subscript, "y_{1}"),
    ] {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.begin_inline_math()?;
        for input in inputs {
            session.run(Command::InsertText(input.to_owned()))?;
        }
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert!(edit.source_projection.is_none());
        let MathEditBranch::Tree { path, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        assert!(path.steps().iter().any(|step| matches!(
            step,
            MathSequenceStep::Operand {
                operand_index: 1,
                ..
            }
        )));
        assert_eq!(inline_source(&session).as_deref(), Some(expected_source));
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = &edit.target
        else {
            return Err(EditError::InvalidPosition);
        };
        let tree = session
            .document()
            .body
            .iter()
            .find_map(|block| match block {
                Block::Paragraph(paragraph) if &paragraph.id == paragraph_id => {
                    paragraph.inlines.iter().find_map(|content| match content {
                        ParagraphContent::Math(math) if &math.id == math_id => {
                            Some(&math.math_tree)
                        }
                        _ => None,
                    })
                }
                _ => None,
            })
            .ok_or(EditError::InvalidPosition)?;
        assert!(tree.root.content.iter().any(
            |content| matches!(content, MathContent::Math(node) if node.kind == expected_kind)
        ));
    }

    let mut grouped = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    grouped.begin_inline_math()?;
    grouped.run(Command::InsertText("{".to_owned()))?;
    grouped.run(Command::InsertText("ab".to_owned()))?;
    grouped.run(Command::InsertText("}".to_owned()))?;
    assert_eq!(inline_source(&grouped).as_deref(), Some("{ab}"));
    assert!(grouped.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    Ok(())
}

#[test]
fn tree_native_align_navigation_crosses_cells_and_exits_the_last_boundary() -> Result<(), EditError>
{
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(Paragraph::empty(NodeId::generate())),
        display_math_block(true, r"\frac{a}{b}&x")?,
        Block::Paragraph(Paragraph::empty(NodeId::generate())),
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    let align = first_align(&session)?;
    let first_row_id = align.rows[0].id.clone();
    let first_cell_id = align.rows[0].cells[0].root.id.clone();
    let second_row_id = align.rows[0].id.clone();
    let second_cell_id = align.rows[0].cells[1].root.id.clone();

    let first_cell_is_tree = session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none()
            && matches!(
                &edit.branch,
                MathEditBranch::Tree { anchor, .. }
                    if matches!(&anchor.host, MathPositionHost::AlignCell { row_id, cell_id }
                        if row_id == &first_row_id && cell_id == &first_cell_id)
            )
    });
    assert!(first_cell_is_tree);

    for _ in 0..16 {
        if session.math_edit().is_some_and(|edit| {
            matches!(
                &edit.branch,
                MathEditBranch::Text { path, .. } if path.align_cell() == Some((0, 1))
            ) || matches!(
                &edit.branch,
                MathEditBranch::Tree { anchor, .. }
                    if matches!(&anchor.host, MathPositionHost::AlignCell { row_id, cell_id }
                        if row_id == &second_row_id && cell_id == &second_cell_id)
            )
        }) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none()
            && (matches!(
                &edit.branch,
                MathEditBranch::Text { path, .. } if path.align_cell() == Some((0, 1))
            ) || matches!(
                &edit.branch,
                MathEditBranch::Tree { anchor, .. }
                    if matches!(&anchor.host, MathPositionHost::AlignCell { row_id, cell_id }
                        if row_id == &second_row_id && cell_id == &second_cell_id)
            ))
    }));

    for _ in 0..16 {
        if session.math_edit().is_some_and(|edit| {
            matches!(
                &edit.branch,
                MathEditBranch::Tree { anchor, .. }
                    if matches!(&anchor.host, MathPositionHost::AlignCell { row_id, cell_id }
                        if row_id == &first_row_id && cell_id == &first_cell_id)
            )
        }) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none()
            && matches!(
                &edit.branch,
                MathEditBranch::Tree { anchor, .. }
                    if matches!(&anchor.host, MathPositionHost::AlignCell { row_id, cell_id }
                        if row_id == &first_row_id && cell_id == &first_cell_id)
            )
    }));

    for _ in 0..16 {
        if session.math_edit().is_none() {
            break;
        }
        session.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn tree_native_align_navigation_round_trips_through_empty_cells_and_rows() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.insert_display_math(true)?;
    session.run(Command::InsertText("a".to_owned()))?;
    session.run(Command::InsertText("&".to_owned()))?;
    session.run(Command::InsertText("b".to_owned()))?;
    session.run(Command::EditorKey(EditorKey::Enter))?;
    let initial = first_align(&session)?.clone();
    assert_eq!(initial.rows.len(), 2);
    assert_eq!(initial.rows[0].cells.len(), 2);
    assert!(!initial.rows[0].cells[1].root.content.is_empty());
    assert!(
        initial.rows[1]
            .cells
            .iter()
            .all(|cell| cell.root.content.is_empty())
    );
    let generation = session.document_generation();
    let history = session.history_state_id();

    let mut backward = Vec::new();
    let mut previous = None;
    for _ in 0..32 {
        let Some(edit) = session.math_edit() else {
            break;
        };
        assert!(edit.source_projection.is_none());
        let MathEditBranch::Tree { anchor, path } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        let MathPositionHost::AlignCell { row_id, cell_id } = &anchor.host else {
            return Err(EditError::InvalidPosition);
        };
        let position = (row_id.clone(), cell_id.clone(), path.clone(), edit.cursor);
        if previous.as_ref() == Some(&position) {
            break;
        }
        backward.push(position.clone());
        previous = Some(position);
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert!(backward.iter().any(|(row, cell, _, _)| {
        row == &initial.rows[0].id && cell == &initial.rows[0].cells[1].root.id
    }));
    assert!(backward.iter().any(|(row, cell, _, _)| {
        row == &initial.rows[0].id && cell == &initial.rows[0].cells[0].root.id
    }));
    assert!(backward.iter().any(|(row, cell, _, _)| {
        row == &initial.rows[1].id && cell == &initial.rows[1].cells[0].root.id
    }));

    session.run(Command::EnterMath(Direction::Forward))?;
    let mut forward = Vec::new();
    let mut previous = None;
    for _ in 0..32 {
        let Some(edit) = session.math_edit() else {
            break;
        };
        let MathEditBranch::Tree { anchor, path } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        let MathPositionHost::AlignCell { row_id, cell_id } = &anchor.host else {
            return Err(EditError::InvalidPosition);
        };
        let position = (row_id.clone(), cell_id.clone(), path.clone(), edit.cursor);
        if previous.as_ref() == Some(&position) {
            break;
        }
        forward.push(position.clone());
        previous = Some(position);
        session.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert_eq!(forward, backward.iter().rev().cloned().collect::<Vec<_>>());
    assert_eq!(first_align(&session)?, &initial);
    assert_eq!(session.document_generation(), generation);
    assert_eq!(session.history_state_id(), history);
    Ok(())
}

#[test]
fn invalid_tree_native_host_or_path_fails_without_changing_editor_state() -> Result<(), EditError> {
    for invalid_host in [true, false] {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.begin_inline_math()?;
        session.run(Command::InsertText("x".to_owned()))?;
        assert!(session.math_edit().is_some_and(|edit| {
            edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
        }));
        let edit = session
            .math_edit
            .as_mut()
            .ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree { anchor, path } = &mut edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        if invalid_host {
            anchor.host = MathPositionHost::InlineMath {
                owner_id: NodeId::generate(),
            };
        } else {
            let invalid_path = MathSequencePath::new(vec![MathSequenceStep::Operand {
                content_index: usize::MAX,
                operand_index: 0,
            }]);
            anchor.path = invalid_path.clone();
            *path = invalid_path;
        }
        let document_before = session.document().clone();
        let edit_before = session.math_edit().cloned();
        let generation_before = session.document_generation();
        let history_before = session.history_state_id();

        assert_eq!(
            session.run(Command::MoveCaret(Direction::Backward)),
            Err(EditError::InvalidPosition)
        );
        assert_eq!(session.document(), &document_before);
        assert_eq!(session.math_edit(), edit_before.as_ref());
        assert_eq!(session.document_generation(), generation_before);
        assert_eq!(session.history_state_id(), history_before);
    }
    Ok(())
}

#[test]
fn empty_text_island_is_an_editable_caret_target() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::InsertText("x".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{x}"));
    Ok(())
}
#[test]
fn empty_text_island_at_formula_edge_exits_without_reentry_trap() -> Result<(), EditError> {
    let mut forward = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    forward.run(Command::InsertInlineMath(r"\text{}".to_owned()))?;
    forward.run(Command::EnterMath(Direction::Backward))?;
    forward.run(Command::MoveCaret(Direction::Backward))?;
    forward.run(Command::MoveCaret(Direction::Forward))?;
    if forward.math_edit().is_some() {
        assert!(matches!(
            forward.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Source)
        ));
        forward.run(Command::MoveCaret(Direction::Forward))?;
    }
    assert!(forward.math_edit().is_none());

    let mut backward = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    backward.run(Command::InsertInlineMath(r"\text{}".to_owned()))?;
    backward.run(Command::EnterMath(Direction::Backward))?;
    // 空文本岛在公式边界仍是光标停靠点；连续后退先进入再离开，后续断言防止重新陷入。
    backward.run(Command::MoveCaret(Direction::Backward))?;
    backward.run(Command::MoveCaret(Direction::Backward))?;
    if backward.math_edit().is_some() {
        assert!(matches!(
            backward.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Source)
        ));
        for _ in 0..8 {
            backward.run(Command::MoveCaret(Direction::Backward))?;
            assert!(!matches!(
                backward.math_edit().map(|edit| &edit.branch),
                Some(MathEditBranch::Text { .. })
            ));
            if backward.math_edit().is_none() {
                break;
            }
        }
    }
    assert!(backward.math_edit().is_none());
    Ok(())
}

#[test]
fn empty_text_island_between_source_content_can_be_crossed_and_reentered() -> Result<(), EditError>
{
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"a\text{}b")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let index = tree
        .root
        .content
        .iter()
        .enumerate()
        .find_map(|(index, content)| match content {
            MathContent::Text(text) if text.runs.iter().all(|run| run.text.is_empty()) => {
                Some(index)
            }
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(index)], None);
    session.enter_math_text_branch(path, 0);

    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));
    for _ in 0..3 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    Ok(())
}
#[test]
fn nested_empty_text_island_has_symmetric_one_delimiter_navigation() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{\textbf{}}")?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let text_path =
        path::root_text_flow_path(tree, true, None).ok_or(EditError::InvalidPosition)?;
    let range =
        math_content_range_at(&edit, text_path.steps()).ok_or(EditError::InvalidPosition)?;
    let source_base = source_projection(&edit).edit_range().start;
    let path = path::path_at_source_boundary(
        source_projection(&edit)
            .tree()
            .ok_or(EditError::InvalidPosition)?,
        source_base + range.end,
        false,
        None,
    )
    .ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 0);

    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());
    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| { edit.source_projection.is_none() && edit.cursor == 0 })
    );
    Ok(())
}
#[test]
fn deleting_last_character_of_a_grouped_textflow_keeps_the_empty_textflow() -> Result<(), EditError>
{
    let source = r"\text{a{b}}";
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, source)?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let first_text_end = tree
        .root
        .content
        .iter()
        .enumerate()
        .find_map(|(index, content)| match content {
            MathContent::Text(_) => {
                math_content_range_at(&edit, &[MathTextFlowPathStep::Content(index)])
                    .map(|range| range.end)
            }
            _ => None,
        })
        .ok_or(EditError::InvalidPosition)?;
    let source_base = source_projection(&edit).edit_range().start;
    let path = path::path_at_source_boundary(
        source_projection(&edit)
            .tree()
            .ok_or(EditError::InvalidPosition)?,
        source_base + first_text_end,
        false,
        None,
    )
    .ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 1);

    session.run(Command::Delete(Direction::Backward))?;

    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    assert!(inline_source(&session).is_some_and(|source| !source.contains('a')));
    session.run(Command::InsertText("z".to_owned()))?;
    assert!(inline_source(&session).is_some_and(|source| source.contains('z')));
    Ok(())
}
#[test]
fn text_island_escapes_literal_tex_characters_and_keeps_atomic_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::InsertText("%  \t\u{00a0}".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{abc\%\textchar{20}\textchar{20}\textchar{9}~}")
    );
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    let text = path
        .resolve_structurally(&equation.math_tree)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abc%  \t\u{00a0}"
    );
    assert_eq!(
        session.math_edit().map(|edit| edit.cursor),
        Some("abc%  \t\u{00a0}".len())
    );

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abc}"));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::Redo)?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{abc\%\textchar{20}\textchar{20}\textchar{9}~}")
    );
    Ok(())
}

#[test]
fn text_edit_preserves_nested_text_structure() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{\textbf{abc}}")?;
    move_into_math_text(&mut session)?;
    match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    {
        MathEditBranch::Text { path, .. } => {
            let tree = session.math_tree_for_edit(
                session.math_edit().ok_or(EditError::InvalidPosition)?,
                path.align_cell(),
            )?;
            let Some(MathContent::Environment(_environment)) = tree.root.content.first() else {
                return Err(EditError::InvalidPosition);
            };
        }
        MathEditBranch::Source => return Err(EditError::InvalidPosition),
        MathEditBranch::Tree { .. } => return Err(EditError::InvalidPosition),
    }

    session.run(Command::InsertText("Xé".to_owned()))?;

    let MathEditBranch::Text { path, .. } = &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    else {
        return Err(EditError::InvalidPosition);
    };
    let tree = session.math_tree_for_edit(
        session.math_edit().ok_or(EditError::InvalidPosition)?,
        path.align_cell(),
    )?;
    let Some(MathContent::Environment(_environment)) = tree.root.content.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(
        path.resolve(tree).map(|text| text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()),
        Some("abcXé".to_owned())
    );
    Ok(())
}

#[test]
fn math_text_formatting_keeps_selection_inside_nested_text_environment() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{\textbf{abc}}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 2,
    })?;

    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;

    let MathEditBranch::Text { path, anchor, .. } = &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .branch
    else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(anchor.offset, 0);
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    assert_eq!(
        path.resolve(
            session
                .math_tree_for_edit(session.math_edit().ok_or(EditError::InvalidPosition)?, None,)?
        )
        .map(|text| text
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()),
        Some("b".to_owned())
    );
    assert!(inline_source(&session).is_some_and(|source| source.contains(r"\textmd{b}")));

    session.run(Command::Undo)?;
    assert!(inline_source(&session).is_some_and(|source| source.contains(r"\textbf{abc}")));
    session.run(Command::Redo)?;
    assert!(inline_source(&session).is_some_and(|source| source.contains(r"\textmd{b}")));
    Ok(())
}

#[test]
fn math_text_formatting_rejects_unsafe_model_metadata_splits_atomically() -> Result<(), EditError> {
    for metadata_kind in ["flow", "run"] {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        insert_equation_text(&mut session, r"\text{abc}")?;
        move_into_math_text(&mut session)?;
        mutate_active_math_text_flow(&mut session, |text| {
            match metadata_kind {
                "flow" => {
                    text.extra.insert(
                        "futureFlowValue".to_owned(),
                        serde_json::json!(metadata_kind),
                    );
                }
                "run" => {
                    text.runs[0].extra.insert(
                        "futureRunValue".to_owned(),
                        serde_json::json!(metadata_kind),
                    );
                }
                _ => unreachable!(),
            }
            Ok(())
        })?;
        session.run(Command::SetMathTextSelection {
            anchor: 1,
            focus: 2,
        })?;
        let document_before = session.document().clone();
        let edit_before = session.math_edit().cloned();
        let generation_before = session.document_generation();
        let selection_before = session.selection().clone();

        assert!(matches!(
            session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold)),
            Err(EditError::UnsupportedOpaqueContent)
        ));
        assert_eq!(session.document(), &document_before);
        assert_eq!(session.math_edit(), edit_before.as_ref());
        assert_eq!(session.document_generation(), generation_before);
        assert_eq!(session.selection(), &selection_before);
    }

    let mut full_selection = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut full_selection, r"\text{abc}")?;
    move_into_math_text(&mut full_selection)?;
    mutate_active_math_text_flow(&mut full_selection, |text| {
        text.extra
            .insert("futureFlowValue".to_owned(), serde_json::json!(1));
        text.runs[0]
            .extra
            .insert("futureRunValue".to_owned(), serde_json::json!(2));
        Ok(())
    })?;
    full_selection.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 3,
    })?;
    full_selection.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    let selected = active_math_text_flow(&full_selection)?;
    assert_eq!(selected.extra["futureFlowValue"], 1);
    assert_eq!(selected.runs[0].extra["futureRunValue"], 2);
    Ok(())
}

#[test]
fn math_text_entry_splices_math_nodes_between_text_flows_and_round_trips_history()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 2,
    })?;

    session.run(Command::InsertInlineMath("x^2".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a}x^{2}\text{c}")
    );
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { path, .. }) if path.steps().is_empty()
    ));
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(
        matches!(equation.math_tree.root.content.first(), Some(MathContent::Text(text)) if text.runs[0].text == "a")
    );
    assert!(
        equation
            .math_tree
            .root
            .content
            .iter()
            .any(|content| matches!(content, MathContent::Math(_)))
    );
    assert!(
        matches!(equation.math_tree.root.content.last(), Some(MathContent::Text(text)) if text.runs[0].text == "c")
    );

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{abc}"));
    assert!(matches!(
        session.math_edit().map(|edit| (&edit.branch, edit.cursor)),
        Some((MathEditBranch::Text { anchor, ..
                }, 2)) if anchor.offset == 1
    ));
    session.run(Command::Redo)?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a}x^{2}\text{c}")
    );
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    session.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a}x^{2}y\text{c}")
    );
    Ok(())
}

#[test]
fn math_text_entry_at_empty_flow_end_keeps_an_editable_text_anchor() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;

    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{abc}x\text{}")
    );
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition);
    };
    assert!(
        matches!(equation.math_tree.root.content.last(), Some(MathContent::Text(text)) if text.runs.is_empty() || text.runs.iter().all(|run| run.text.is_empty()))
    );
    Ok(())
}

#[test]
fn math_text_entry_at_collapsed_interior_caret_keeps_both_text_sides_editable()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a}x\text{bc}")
    );

    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::InsertText("R".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a}x\text{Rbc}")
    );

    session.run(Command::Undo)?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{a}x\text{bc}")
    );
    for _ in 0..8 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { path, ..
                })
                if path.steps() == [MathTextFlowPathStep::Content(0)]
        ) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                })
            if path.steps() == [MathTextFlowPathStep::Content(0)]
    ));
    session.run(Command::InsertText("L".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{aL}x\text{bc}")
    );
    Ok(())
}

#[test]
fn inline_math_nested_text_split_preserves_inserted_node_ranges_and_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{\text{abc}}{z}".to_owned(),
    ))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    for _ in 0..24 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;

    session.run(Command::InsertInlineMath(r"\frac{p}{q}".to_owned()))?;
    let source = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    assert!(
        source.contains(r"\text{a}\frac{p}{q}\text{bc}"),
        "source: {source}"
    );
    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(ParagraphContent::Math(inline_math)) = paragraph
        .inlines
        .iter()
        .find(|content| matches!(content, ParagraphContent::Math(_)))
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(MathContent::Math(outer_fraction)) = inline_math.math_tree.root.content.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(numerator) = outer_fraction.children.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert!(matches!(
        numerator.content.as_slice(),
        [
            MathContent::Text(left),
            MathContent::Math(inner),
            MathContent::Text(right),
        ] if text_flow_display(left) == "a"
            && inner.kind == MathNodeKind::Fraction
            && text_flow_display(right) == "bc"
    ));

    session.run(Command::Undo)?;
    assert!(
        inline_source(&session)
            .as_deref()
            .is_some_and(|source| source.contains(r"\text{abc}"))
    );
    session.run(Command::Redo)?;
    assert!(
        inline_source(&session)
            .as_deref()
            .is_some_and(|source| source.contains(r"\text{a}\frac{p}{q}\text{bc}"))
    );
    Ok(())
}

#[test]
fn align_nonfirst_cell_math_text_split_keeps_cell_mapping_through_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("α & \\text{abc}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 1))
    ));
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 2,
    })?;
    session.run(Command::InsertInlineMath(r"\frac{p}{q}".to_owned()))?;
    let Some(Block::Align(align)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { path, anchor })
            if path.steps().is_empty() && matches!(anchor.host, MathPositionHost::AlignCell { .. })
    ));
    assert!(matches!(
        align.rows[0].cells[1].root.content.as_slice(),
        [
            MathContent::Text(left),
            MathContent::Math(fraction),
            MathContent::Text(right),
        ] if text_flow_display(left) == "a"
            && fraction.kind == MathNodeKind::Fraction
            && text_flow_display(right) == "c"
    ));

    session.run(Command::Undo)?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\begin{aligned}α & \text{abc}\end{aligned}")
    );
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 1))
    ));
    session.run(Command::Redo)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { anchor, .. })
            if matches!(anchor.host, MathPositionHost::AlignCell { .. })
    ));
    session.run(Command::Undo)?;
    session.run(Command::InsertText("z".to_owned()))?;
    assert!(inline_source(&session).is_some_and(|source| source.contains(r"\text{azc}")));
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\begin{aligned}α & \text{ac}\end{aligned}")
    );
    Ok(())
}

#[test]
fn math_text_entry_rejects_opaque_split_metadata_atomically() -> Result<(), EditError> {
    for metadata_kind in ["flow", "run"] {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        insert_equation_text(&mut session, r"\text{abc}")?;
        move_into_math_text(&mut session)?;
        mutate_active_math_text_flow(&mut session, |text| {
            match metadata_kind {
                "flow" => {
                    text.extra
                        .insert("futureFlowValue".to_owned(), serde_json::json!(1));
                }
                "run" => {
                    text.runs[0]
                        .extra
                        .insert("futureRunValue".to_owned(), serde_json::json!(1));
                }
                _ => unreachable!(),
            }
            Ok(())
        })?;
        session.run(Command::SetMathTextSelection {
            anchor: 1,
            focus: 2,
        })?;
        let document_before = session.document().clone();
        let edit_before = session.math_edit().cloned();
        let generation_before = session.document_generation();
        let selection_before = session.selection().clone();

        assert!(matches!(
            session.run(Command::InsertInlineMath("x".to_owned())),
            Err(EditError::UnsupportedOpaqueContent)
        ));
        assert_eq!(session.document(), &document_before);
        assert_eq!(session.math_edit(), edit_before.as_ref());
        assert_eq!(session.document_generation(), generation_before);
        assert_eq!(session.selection(), &selection_before);
    }
    Ok(())
}

fn mutate_active_math_text_flow(
    session: &mut Session,
    mutate: impl FnOnce(&mut caret_doc::TextFlow) -> Result<(), EditError>,
) -> Result<(), EditError> {
    let Some(Block::Equation(equation)) = session.document.body.first_mut() else {
        panic!("test should use an equation block");
    };
    let Some(text) = equation
        .math_tree
        .root
        .content
        .iter_mut()
        .find_map(first_math_content_text_mut)
    else {
        panic!("test formula should contain a TextFlow");
    };
    mutate(text)?;
    Ok(())
}

#[test]
fn text_input_is_not_limited_by_latex_source_budget() -> Result<(), EditError> {
    let prefix = r"\text{";
    let suffix = "}";
    let content_len = caret_math::MAX_MATH_SOURCE_BYTES - prefix.len() - suffix.len() - 1;
    let source = format!("{prefix}{}{suffix}", "a".repeat(content_len));
    assert_eq!(source.len(), caret_math::MAX_MATH_SOURCE_BYTES - 1);

    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, &source)?;
    move_into_math_text(&mut session)?;
    session.run(Command::InsertText("%".to_owned()))?;
    assert!(
        inline_source(&session)
            .ok_or(EditError::InvalidPosition)?
            .len()
            > caret_math::MAX_MATH_SOURCE_BYTES
    );
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();
    let selection_before = session.selection().clone();
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.document_generation(), generation_before);
    assert_ne!(session.selection(), &selection_before);
    assert!(edit_before.is_some());

    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Text { .. })
    }));
    let text_before_reentry_insert = active_math_text_flow(&session)?
        .runs
        .iter()
        .map(|run| run.text.len())
        .sum::<usize>();
    session.run(Command::InsertText("z".to_owned()))?;
    let reentered_text = active_math_text_flow(&session)?
        .runs
        .iter()
        .map(|run| run.text.len())
        .sum::<usize>();
    assert_eq!(reentered_text, text_before_reentry_insert + 1);
    Ok(())
}

#[test]
fn parsed_equation_text_reenters_through_tree_native_editing() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());

    session.run(Command::EnterMath(Direction::Backward))?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Text { .. })
    }));
    session.run(Command::InsertText("d".to_owned()))?;
    assert_eq!(active_math_text_flow(&session)?.runs[0].text, "abcd");
    Ok(())
}

#[test]
fn multi_flow_root_text_exits_at_both_outer_boundaries() -> Result<(), EditError> {
    let mut forward = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut forward, r"\text{a}\textbf{b}")?;
    move_into_math_text(&mut forward)?;
    forward.run(Command::MoveCaret(Direction::Forward))?;
    assert!(forward.math_edit().is_none());

    let mut backward = session_with_text("x").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut backward, r"\text{a}\textbf{b}")?;
    move_into_math_text(&mut backward)?;
    let edit = backward
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = backward.math_tree_for_edit(&edit, None)?;
    let first = path::root_text_flow_path(tree, true, None).ok_or(EditError::InvalidPosition)?;
    assert!(matches!(
        math_text_caret_step(tree, &first, 0, Direction::Backward)?,
        MathTextCaretStep::Boundary
    ));
    assert_eq!(
        path::root_text_flow_path(tree, true, None)
            .ok_or(EditError::InvalidPosition)?
            .steps(),
        first.steps()
    );
    backward.enter_math_text_branch(first, 0);
    backward.run(Command::MoveCaret(Direction::Backward))?;
    assert!(backward.math_edit().is_none(), "{:?}", backward.math_edit());
    Ok(())
}

#[test]
fn equation_and_align_text_islands_share_the_same_edit_branch()
-> Result<(), Box<dyn std::error::Error>> {
    let mut equation = session_with_text("")?;
    accept(&mut equation, "equation")?;
    enter_source_edit(&mut equation, Direction::Forward)?;
    equation.run(Command::InsertText(r"\text{abc}".to_owned()))?;
    equation.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        equation.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    equation.run(Command::InsertText("Xé%".to_owned()))?;
    assert_eq!(inline_source(&equation).as_deref(), Some(r"\text{abcXé\%}"));
    equation.run(Command::InsertText("Z".to_owned()))?;
    assert_eq!(
        inline_source(&equation).as_deref(),
        Some(r"\text{abcXé\%Z}")
    );
    equation.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    let effect = equation.run(Command::ToggleRunUnderline)?;
    assert!(!effect.selection_changed);
    assert!(active_math_text_flow(&equation)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    equation.run(Command::Undo)?;
    assert!(
        active_math_text_flow(&equation)?
            .runs
            .iter()
            .all(|run| { run.props.get("underline") != Some(&serde_json::Value::Bool(true)) })
    );
    equation.run(Command::Redo)?;
    assert!(active_math_text_flow(&equation)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    equation.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    assert!(inline_source(&equation).is_some_and(|source| source.contains(r"\textbf{bc}")));
    assert!(matches!(
        equation.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    equation.run(Command::Undo)?;
    assert!(inline_source(&equation).is_some_and(|source| !source.contains(r"\textbf{bc}")));
    equation.run(Command::Redo)?;
    assert!(inline_source(&equation).is_some_and(|source| source.contains(r"\textbf{bc}")));

    let mut align = session_with_text("")?;
    accept(&mut align, "align")?;
    enter_source_edit(&mut align, Direction::Forward)?;
    align.run(Command::InsertText(r"\text{abc}".to_owned()))?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        align.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 0))
    ));
    align.run(Command::InsertText("Xé%".to_owned()))?;
    assert!(inline_source(&align).is_some_and(|source| source.contains(r"\text{abcXé\%}")));
    align.run(Command::InsertText("Z".to_owned()))?;
    assert!(inline_source(&align).is_some_and(|source| source.contains(r"\text{abcXé\%Z}")));
    align.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 3,
    })?;
    let effect = align.run(Command::ToggleRunUnderline)?;
    assert!(!effect.selection_changed);
    assert!(active_math_text_flow(&align)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    align.run(Command::Undo)?;
    assert!(
        active_math_text_flow(&align)?
            .runs
            .iter()
            .all(|run| { run.props.get("underline") != Some(&serde_json::Value::Bool(true)) })
    );
    align.run(Command::Redo)?;
    assert!(active_math_text_flow(&align)?.runs.iter().any(|run| {
        run.text == "bc" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    align.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    assert!(inline_source(&align).is_some_and(|source| source.contains(r"\textbf{bc}")));
    assert!(matches!(
        align.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 0))
    ));
    Ok(())
}

#[test]
fn math_text_command_line_can_split_out_inline_math() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;

    session.run(Command::BeginCommandLine)?;
    assert_eq!(
        session.command_line().map(|line| line.mode),
        Some(CommandLineMode::Math)
    );
    session.run(Command::TypeCommandLine("math".to_owned()))?;
    let candidate = session
        .command_line_candidates()
        .into_iter()
        .find(|candidate| candidate.command_id == "math.split_inline")
        .ok_or(EditError::InvalidPosition)?;
    assert!(candidate.available);
    session.run(Command::AcceptCommandLine)?;

    assert!(session.command_line().is_none());
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { path, .. }) if path.steps().is_empty()
    ));
    let Block::Equation(equation) = session
        .document()
        .body
        .first()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    assert!(matches!(
        equation.math_tree.root.content.as_slice(),
        [
            MathContent::Text(left),
            MathContent::Math(node),
            MathContent::Text(right),
        ] if text_flow_display(left) == "a"
            && node.kind == MathNodeKind::Placeholder
            && text_flow_display(right) == "b"
    ));

    session.run(Command::Undo)?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Text { .. }))
    );
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "ab"
    );
    session.run(Command::Redo)?;
    assert!(session.math_edit().is_some_and(|edit| {
        matches!(edit.branch, MathEditBranch::Tree { .. })
            && session.math_tree_for_edit(edit, None).is_ok_and(|tree| {
                tree.root.content.iter().any(|content| {
                    matches!(
                        content,
                        MathContent::Math(node) if node.kind == MathNodeKind::Placeholder
                    )
                })
            })
    }));
    Ok(())
}

#[test]
fn math_split_command_is_unavailable_for_cross_text_flow_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{ab}+\text{cd}")?;
    move_into_math_text(&mut session)?;

    let Block::Equation(equation) = session
        .document()
        .body
        .first()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = equation.math_tree.clone();
    let edit = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, anchor, .. } = &mut edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let other_steps = if path.steps() == [MathTextFlowPathStep::Content(0)] {
        vec![MathTextFlowPathStep::Content(2)]
    } else {
        vec![MathTextFlowPathStep::Content(0)]
    };
    anchor.path = MathTextFlowPath::new(other_steps, path.align_cell());
    assert!(anchor.path.resolve_structurally(&tree).is_some());

    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("math".to_owned()))?;
    let candidate = session
        .command_line_candidates()
        .into_iter()
        .find(|candidate| candidate.command_id == "math.split_inline")
        .ok_or(EditError::InvalidPosition)?;
    assert!(!candidate.available);
    Ok(())
}

#[test]
fn math_text_math_commandline_templates_enter_the_nested_math_node()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("frac".to_owned()))?;
    session.run(Command::AcceptCommandLine)?;

    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(matches!(
        &edit.branch,
        MathEditBranch::Tree { path, .. }
            if matches!(path.steps(), [MathSequenceStep::Operand { operand_index: 0, .. }])
    ));
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(
        caret_math::serialize_tree(&equation.math_tree),
        r"\text{a}\frac{}{}\text{b}"
    );
    assert_eq!(edit.cursor, 0);
    session.run(Command::InsertText("1".to_owned()))?;
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(
        caret_math::serialize_tree(&equation.math_tree),
        r"\text{a}\frac{1}{}\text{b}"
    );
    session.run(Command::Undo)?;
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(
        caret_math::serialize_tree(&equation.math_tree),
        r"\text{a}\frac{}{}\text{b}"
    );
    session.run(Command::Undo)?;
    assert!(session.math_edit().is_some_and(|edit| {
        matches!(edit.branch, MathEditBranch::Text { .. }) && edit.source_projection.is_none()
    }));
    assert!(matches!(
        session.document().body.first(),
        Some(Block::Equation(equation))
            if caret_math::serialize_tree(&equation.math_tree) == r"\text{ab}"
    ));
    Ok(())
}

#[test]
fn math_text_control_word_template_keeps_following_letters_separate()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{ab}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("alpha".to_owned()))?;
    session.run(Command::AcceptCommandLine)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { .. })
    ));
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert!(
        equation
            .math_tree
            .root
            .content
            .iter()
            .any(|content| { math_content_has_symbol_text(content, "alpha") })
    );
    session.run(Command::InsertText("x".to_owned()))?;
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    let root = &equation.math_tree.root.content;
    assert!(matches!(
        root.as_slice(),
        [
            MathContent::Text(left),
            MathContent::Math(alpha),
            MathContent::Math(x),
            MathContent::Text(right),
        ] if text_flow_display(left) == "a"
            && alpha.text.as_deref() == Some("alpha")
            && x.text.as_deref() == Some("x")
            && text_flow_display(right) == "b"
    ));
    session.run(Command::Undo)?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Tree { .. }))
    );
    session.run(Command::Redo)?;
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert!(
        equation
            .math_tree
            .root
            .content
            .iter()
            .any(|content| { math_content_has_symbol_text(content, "x") })
    );
    Ok(())
}

#[test]
fn math_text_command_line_double_backslash_inserts_literal_backslash()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{a}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("\\".to_owned()))?;
    assert!(session.command_line_candidates().is_empty());
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    let flow = active_math_text_flow(&session)?;
    assert!(flow.runs.iter().any(|run| run.text.contains('\\')));
    Ok(())
}

#[test]
fn pending_math_text_format_applies_to_inserted_text_and_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{a}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))?;
    session.run(Command::ToggleRunUnderline)?;
    session.run(Command::InsertText("b".to_owned()))?;

    assert!(
        inline_source(&session)
            .is_some_and(|source| { source.contains(r"\text{a\textbf{\textit{b}}}") })
    );
    assert!(math_tree_has_environment(
        session.document(),
        InlineEnvironmentKind::Bold
    ));
    assert!(math_tree_has_environment(
        session.document(),
        InlineEnvironmentKind::Italic
    ));
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "b" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{a}"));
    assert!(!math_tree_has_environment(
        session.document(),
        InlineEnvironmentKind::Bold
    ));
    session.run(Command::Redo)?;
    assert!(
        inline_source(&session)
            .is_some_and(|source| { source.contains(r"\text{a\textbf{\textit{b}}}") })
    );
    assert!(math_tree_has_environment(
        session.document(),
        InlineEnvironmentKind::Bold
    ));
    assert!(math_tree_has_environment(
        session.document(),
        InlineEnvironmentKind::Italic
    ));

    let mut combining = session_with_text("")?;
    insert_equation_text(&mut combining, r"\text{a}")?;
    move_into_math_text(&mut combining)?;
    combining.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    combining.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    combining.run(Command::ToggleRunUnderline)?;
    combining.run(Command::InsertText("\u{301}".to_owned()))?;
    assert!(
        inline_source(&combining)
            .is_some_and(|source| { source.contains("\\text{\\textbf{a\u{301}}}") })
    );
    assert!(math_tree_has_environment(
        combining.document(),
        InlineEnvironmentKind::Bold
    ));
    assert!(active_math_text_flow(&combining)?.runs.iter().any(|run| {
        run.text == "a\u{301}" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));

    let mut disable = session_with_text("")?;
    insert_equation_text(&mut disable, r"\text{\textbf{a}}")?;
    move_into_math_text(&mut disable)?;
    disable.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    disable.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    disable.run(Command::InsertText("b".to_owned()))?;
    assert!(
        inline_source(&disable)
            .is_some_and(|source| { source.contains(r"\text{\textbf{a\textmd{b}}}") })
    );
    assert!(math_tree_has_environment(
        disable.document(),
        InlineEnvironmentKind::Bold
    ));

    let mut align = session_with_text("")?;
    align.insert_display_math(true)?;
    enter_source_edit(&mut align, Direction::Forward)?;
    align.run(Command::InsertText(r"\text{a}".to_owned()))?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        align.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 0))
    ));
    align.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    align.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))?;
    align.run(Command::InsertText("b".to_owned()))?;
    assert!(inline_source(&align).is_some_and(|source| { source.contains(r"\text{a\textit{b}}") }));
    assert!(math_tree_has_environment(
        align.document(),
        InlineEnvironmentKind::Italic
    ));
    align.run(Command::Undo)?;
    align.run(Command::Redo)?;
    assert!(inline_source(&align).is_some_and(|source| { source.contains(r"\text{a\textit{b}}") }));
    Ok(())
}

#[test]
fn repeated_math_text_format_toggle_updates_existing_wrapper() -> Result<(), EditError> {
    let caret_math::ParsedMath::Tree(mut tree) =
        caret_math::parse(r"\text{abc}", caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
    else {
        return Err(EditError::InvalidPosition);
    };
    let path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(0)], None);
    let path = text_branch::apply_math_text_environment_tree_range(
        &mut tree,
        &path,
        0..3,
        InlineEnvironmentKind::Bold,
        true,
    )?;
    let path = text_branch::apply_math_text_environment_tree_range(
        &mut tree,
        &path,
        0..3,
        InlineEnvironmentKind::Bold,
        false,
    )?;
    let [MathContent::Environment(environment)] = tree.root.content.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(environment.kind, InlineEnvironmentKind::Bold);
    assert_eq!(
        environment.props.get("enabled"),
        Some(&serde_json::Value::Bool(false))
    );
    assert!(matches!(
        environment.children.as_slice(),
        [MathContent::Text(text)] if text.runs.iter().map(|run| run.text.as_str()).collect::<String>() == "abc"
    ));
    assert!(matches!(
        path.steps(),
        [
            MathTextFlowPathStep::Content(0),
            MathTextFlowPathStep::EnvironmentChild(0)
        ]
    ));
    Ok(())
}

#[test]
fn pending_math_text_formats_continue_across_sequential_input()
-> Result<(), Box<dyn std::error::Error>> {
    let mut bold = session_with_text("")?;
    insert_equation_text(&mut bold, r"\text{a}")?;
    move_into_math_text(&mut bold)?;
    bold.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    bold.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    bold.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))?;
    for character in ["b", "c", "d"] {
        bold.run(Command::InsertText(character.to_owned()))?;
    }
    assert_eq!(
        inline_source(&bold).as_deref(),
        Some(r"\text{a\textbf{\textit{bcd}}}")
    );

    let mut disable = session_with_text("")?;
    insert_equation_text(&mut disable, r"\text{\textbf{a}}")?;
    move_into_math_text(&mut disable)?;
    disable.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 1,
    })?;
    disable.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    for character in ["b", "c"] {
        disable.run(Command::InsertText(character.to_owned()))?;
    }
    assert_eq!(
        inline_source(&disable).as_deref(),
        Some(r"\text{\textbf{a\textmd{bc}}}")
    );

    bold.run(Command::Undo)?;
    assert_eq!(
        inline_source(&bold).as_deref(),
        Some(r"\text{a\textbf{\textit{bc}}}")
    );
    bold.run(Command::Redo)?;
    assert_eq!(
        inline_source(&bold).as_deref(),
        Some(r"\text{a\textbf{\textit{bcd}}}")
    );
    Ok(())
}

#[test]
fn pending_math_text_formats_keep_local_cursor_after_wrapping_a_middle_range()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{abcd}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 2,
        focus: 2,
    })?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Italic))?;
    session.run(Command::InsertText("X".to_owned()))?;
    session.run(Command::InsertText("Y".to_owned()))?;

    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\text{ab\textbf{\textit{XY}}cd}")
    );
    let flow = active_math_text_flow(&session)?;
    let text = flow
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    assert_eq!(text, "XY");
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    assert!(is_grapheme_boundary(&text, 2));
    Ok(())
}

#[test]
fn tree_native_insert_snaps_after_a_grapheme_join() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, "\\text{\u{301}}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 0,
        focus: 0,
    })?;
    session.run(Command::InsertText("a".to_owned()))?;

    let flow = active_math_text_flow(&session)?;
    let text = flow
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>();
    assert_eq!(text, "a\u{301}");
    let cursor = session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .cursor;
    assert_eq!(cursor, text.len());
    assert!(is_grapheme_boundary(&text, cursor));
    Ok(())
}

#[test]
fn regional_indicator_insert_uses_the_text_island_for_caret_and_formatting()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{🇦}\text{🇧🇨}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 4,
    })?;
    session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Bold))?;
    session.run(Command::InsertText("🇩🇪".to_owned()))?;

    let source = inline_source(&session).unwrap_or_default();
    assert!(
        source.contains(r"\text{🇦🇧\textbf{🇩🇪}🇨}"),
        "source: {source}"
    );
    let flow = active_math_text_flow(&session)?;
    assert_eq!(
        flow.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "🇩🇪"
    );
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(8));

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{🇦🇧🇨}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(4));
    session.run(Command::Redo)?;
    assert!(
        inline_source(&session)
            .is_some_and(|source| { source.contains(r"\text{🇦🇧\textbf{🇩🇪}🇨}") })
    );
    Ok(())
}

#[test]
fn regional_indicator_pairing_uses_text_island_context_for_navigation_and_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{🇦}\text{🇧🇨🇩}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 4,
    })?;

    // 完整文本岛中，🇦🇧 后的合法位置落在第二个流局部的 🇧🇨 配对内部。
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                })
            if path.steps() == [MathTextFlowPathStep::Content(0)]
    ));

    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(4));
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                })
            if path.steps() == [MathTextFlowPathStep::Content(1)]
    ));
    Ok(())
}

#[test]
fn run_property_at_cross_flow_regional_indicator_boundary_targets_only_inserted_text()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{🇦}\text{🇧🇨}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 4,
    })?;
    session.run(Command::ToggleRunUnderline)?;
    session.run(Command::InsertText("x".to_owned()))?;

    let flow = active_math_text_flow(&session)?;
    assert!(flow.runs.iter().any(|run| {
        run.text == "x" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    assert!(
        flow.runs
            .iter()
            .all(|run| run.text != "🇨" || !run.props.contains_key("underline"))
    );

    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{🇦🇧🇨}"));
    session.run(Command::Redo)?;
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "x" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    Ok(())
}

#[test]
fn run_property_selection_uses_text_island_regional_indicator_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{🇦}\text{🇧🇨}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 8,
    })?;
    session.run(Command::ToggleRunUnderline)?;

    let flow = active_math_text_flow(&session)?;
    assert!(flow.runs.iter().any(|run| {
        run.text == "🇨" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    session.run(Command::Undo)?;
    session.run(Command::Redo)?;
    assert!(active_math_text_flow(&session)?.runs.iter().any(|run| {
        run.text == "🇨" && run.props.get("underline") == Some(&serde_json::Value::Bool(true))
    }));
    Ok(())
}

#[test]
fn math_split_inline_accepts_text_island_regional_indicator_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{🇦}\text{🇧🇨}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 4,
        focus: 8,
    })?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;

    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Tree { path, .. }) if path.steps().is_empty()
    ));
    let Some(Block::Equation(equation)) = session.document().body.first() else {
        return Err(EditError::InvalidPosition.into());
    };
    assert!(
        equation
            .math_tree
            .root
            .content
            .iter()
            .any(|content| { math_content_has_symbol_text(content, "x") })
    );
    Ok(())
}

#[test]
fn deleting_a_regional_indicator_grapheme_across_textflows_is_undoable()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, r"\text{🇦}\text{🇧🇨🇩}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{🇨🇩}"));
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{🇦🇧🇨🇩}"));
    session.run(Command::Redo)?;
    assert_eq!(inline_source(&session).as_deref(), Some(r"\text{🇨🇩}"));
    Ok(())
}

#[test]
fn math_text_selection_replaces_across_inline_environment_textflows_and_restores_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{ab}\textbf{cd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "abcd");
    assert_eq!(island.paths.len(), 2);
    let anchor = MathTextPosition::new(island.paths[0].clone(), 1, MathPositionAffinity::Upstream);
    let focus = MathTextPosition::new(island.paths[1].clone(), 1, MathPositionAffinity::Upstream);
    session.set_math_text_selection_positions(anchor.clone(), focus.clone())?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text {
            focus_affinity: MathPositionAffinity::Upstream,
            ..
        })
    ));
    let generation_before = session.document_generation();
    session.run(Command::InsertText("X".to_owned()))?;
    assert_eq!(session.document_generation(), generation_before + 1);
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    assert_eq!(
        path::contiguous_text_flow_text(tree, path)
            .ok_or(EditError::InvalidPosition)?
            .display,
        "aXd"
    );

    session.run(Command::Undo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text {
        path,
        anchor: restored_anchor,
        focus_affinity: restored_focus_affinity,
        ..
    } = &edit.branch
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "abcd");
    assert_eq!(island.global_offset(restored_anchor), Some(1));
    assert_eq!(
        island.global_offset(&MathTextPosition::new(
            path.clone(),
            edit.cursor,
            *restored_focus_affinity
        )),
        Some(3)
    );

    session.run(Command::Redo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    assert_eq!(
        path::contiguous_text_flow_text(tree, path)
            .ok_or(EditError::InvalidPosition)?
            .display,
        "aXd"
    );
    Ok(())
}

#[test]
fn math_text_selection_replaces_in_reverse_across_inline_environment_textflows()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{ab}\textbf{cd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    let anchor =
        MathTextPosition::new(island.paths[1].clone(), 1, MathPositionAffinity::Downstream);
    let focus = MathTextPosition::new(island.paths[0].clone(), 1, MathPositionAffinity::Upstream);
    session.set_math_text_selection_positions(anchor.clone(), focus.clone())?;
    session.run(Command::InsertText("X".to_owned()))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    assert_eq!(
        path::contiguous_text_flow_text(tree, path)
            .ok_or(EditError::InvalidPosition)?
            .display,
        "aXd"
    );
    session.run(Command::Undo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text {
        path,
        anchor: restored_anchor,
        ..
    } = &edit.branch
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.global_offset(restored_anchor), Some(3));
    assert_eq!(
        island.global_offset(&MathTextPosition::new(
            path.clone(),
            edit.cursor,
            MathPositionAffinity::Downstream,
        )),
        Some(1)
    );
    Ok(())
}

#[test]
fn math_text_selection_replaces_a_grapheme_spanning_textflows()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{🇦}\text{🇧🇨}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.display, "🇦🇧🇨");
    let anchor = MathTextPosition::new(island.paths[0].clone(), 0, MathPositionAffinity::Upstream);
    let focus = MathTextPosition::new(island.paths[1].clone(), 4, MathPositionAffinity::Downstream);
    session.set_math_text_selection_positions(anchor, focus)?;
    session.run(Command::InsertText("X".to_owned()))?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    assert_eq!(
        path::contiguous_text_flow_text(tree, path)
            .ok_or(EditError::InvalidPosition)?
            .display,
        "X🇨"
    );
    session.run(Command::Undo)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    assert_eq!(
        path::contiguous_text_flow_text(tree, path)
            .ok_or(EditError::InvalidPosition)?
            .display,
        "🇦🇧🇨"
    );
    Ok(())
}

#[test]
fn math_text_selection_rejects_a_partial_crossflow_grapheme_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{🇦}\text{🇧🇨}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    let before_history = session.history_state_id();
    assert_eq!(
        session.set_math_text_selection_positions(
            MathTextPosition::new(island.paths[0].clone(), 0, MathPositionAffinity::Upstream,),
            MathTextPosition::new(island.paths[1].clone(), 0, MathPositionAffinity::Downstream,),
        ),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    assert_eq!(session.history_state_id(), before_history);
    Ok(())
}

#[test]
fn math_text_selection_rejects_cross_operand_range_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{\text{ab}}{\text{cd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    let numerator = MathTextFlowPath::new(
        vec![
            MathTextFlowPathStep::Content(0),
            MathTextFlowPathStep::Operand(0),
            MathTextFlowPathStep::Content(0),
        ],
        None,
    );
    let denominator = MathTextFlowPath::new(
        vec![
            MathTextFlowPathStep::Content(0),
            MathTextFlowPathStep::Operand(1),
            MathTextFlowPathStep::Content(0),
        ],
        None,
    );
    assert_eq!(
        numerator.resolve_structurally(tree).map(|flow| flow
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()),
        Some("ab".to_owned())
    );
    assert_eq!(
        denominator.resolve_structurally(tree).map(|flow| flow
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()),
        Some("cd".to_owned())
    );
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    let before_history = session.history_state_id();
    assert_eq!(
        session.set_math_text_selection_positions(
            MathTextPosition::new(numerator, 1, MathPositionAffinity::Upstream,),
            MathTextPosition::new(denominator, 1, MathPositionAffinity::Downstream,),
        ),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    assert_eq!(session.history_state_id(), before_history);
    Ok(())
}

#[test]
fn math_text_selection_rejects_cross_math_atom_range_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{ab}+\text{cd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(edit, None)?;
    let MathContent::Math(fraction) = tree
        .root
        .content
        .first()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let denominator = fraction.children.get(1).ok_or(EditError::InvalidPosition)?;
    let text_paths = denominator
        .content
        .iter()
        .enumerate()
        .filter(|(_, content)| matches!(content, MathContent::Text(_)))
        .map(|(index, _)| {
            MathTextFlowPath::new(
                vec![
                    MathTextFlowPathStep::Content(0),
                    MathTextFlowPathStep::Operand(1),
                    MathTextFlowPathStep::Content(index),
                ],
                None,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(text_paths.len(), 2);
    let first = text_paths[0]
        .resolve_structurally(tree)
        .ok_or(EditError::InvalidPosition)?;
    let second = text_paths[1]
        .resolve_structurally(tree)
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        first
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "ab"
    );
    assert_eq!(
        second
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "cd"
    );
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    let before_history = session.history_state_id();
    assert_eq!(
        session.set_math_text_selection_positions(
            MathTextPosition::new(text_paths[0].clone(), 1, MathPositionAffinity::Upstream,),
            MathTextPosition::new(text_paths[1].clone(), 1, MathPositionAffinity::Downstream,),
        ),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    assert_eq!(session.history_state_id(), before_history);
    Ok(())
}

#[test]
fn math_text_selection_rejects_cross_align_cell_range_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let (align_block, _) =
        display_math_block_with_projection(true, r"\text{ab} & \text{cd}", NodeId::generate())?;
    let Block::Align(align) = &align_block else {
        return Err(EditError::InvalidPosition.into());
    };
    let cell_zero = align
        .rows
        .first()
        .and_then(|row| row.cells.first())
        .ok_or(EditError::InvalidPosition)?;
    let cell_one = align
        .rows
        .first()
        .and_then(|row| row.cells.get(1))
        .ok_or(EditError::InvalidPosition)?;
    let mut tree_zero = empty_math_tree();
    tree_zero.root = cell_zero.root.clone();
    let mut tree_one = empty_math_tree();
    tree_one.root = cell_one.root.clone();
    let path_zero = path::root_text_flow_path(&tree_zero, true, Some((0, 0)))
        .ok_or(EditError::InvalidPosition)?;
    let path_one = path::root_text_flow_path(&tree_one, true, Some((0, 1)))
        .ok_or(EditError::InvalidPosition)?;
    let block_id = align.id.clone();
    let mut document = Document::blank();
    document.body.push(align_block);
    let mut session = Session::new(document)?;
    session.math_edit = Some(MathEdit {
        target: MathEditTarget::Block {
            block_id,
            align: true,
        },
        source_projection: None,
        source_draft: None,
        revision: session.current_math_edit_revision(),
        cursor: 0,
        branch: MathEditBranch::Text {
            path: path_zero.clone(),
            anchor: MathTextPosition::new(path_zero.clone(), 0, MathPositionAffinity::Downstream),
            focus_affinity: MathPositionAffinity::Downstream,
        },
    });
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    let before_history = session.history_state_id();
    assert_eq!(
        session.set_math_text_selection_positions(
            MathTextPosition::new(path_zero, 1, MathPositionAffinity::Upstream),
            MathTextPosition::new(path_one, 1, MathPositionAffinity::Downstream),
        ),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    assert_eq!(session.history_state_id(), before_history);
    Ok(())
}

#[test]
fn failed_cross_textflow_replacement_preserves_document_selection_and_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{1}{\text{ab}\textbf{cd}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition.into());
    };
    let tree = session.math_tree_for_edit(edit, None)?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    assert_eq!(island.paths.len(), 2);
    session.set_math_text_selection_positions(
        MathTextPosition::new(island.paths[0].clone(), 1, MathPositionAffinity::Upstream),
        MathTextPosition::new(island.paths[1].clone(), 1, MathPositionAffinity::Downstream),
    )?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let MathEditTarget::Inline { math_id, .. } = edit.target else {
        return Err(EditError::InvalidPosition.into());
    };
    let Block::Paragraph(paragraph) = session
        .document
        .body
        .iter_mut()
        .find(|block| matches!(block, Block::Paragraph(paragraph) if find_inline_math(&paragraph.inlines, &math_id).is_some()))
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let MathContent::Math(fraction) = math
        .math_tree
        .root
        .content
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(MathContent::Environment(environment)) = fraction
        .children
        .get_mut(1)
        .and_then(|flow| flow.content.get_mut(1))
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let Some(MathContent::Text(text)) = environment.children.first_mut() else {
        return Err(EditError::InvalidPosition.into());
    };
    text.runs
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
        .extra
        .insert("futureRunField".to_owned(), serde_json::json!("opaque"));
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    let before_history = session.history_state_id();
    assert_eq!(
        session.run(Command::InsertText("X".to_owned())),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    assert_eq!(session.history_state_id(), before_history);
    Ok(())
}

fn math_tree_has_environment(document: &caret_doc::Document, kind: InlineEnvironmentKind) -> bool {
    fn content_has_environment(content: &MathContent, kind: InlineEnvironmentKind) -> bool {
        match content {
            MathContent::Environment(environment) => {
                environment.kind == kind
                    || environment
                        .children
                        .iter()
                        .any(|child| content_has_environment(child, kind))
            }
            MathContent::Math(node) => node.children.iter().any(|flow| {
                flow.content
                    .iter()
                    .any(|child| content_has_environment(child, kind))
            }),
            MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
        }
    }

    document.body.iter().any(|block| match block {
        Block::Equation(equation) => equation
            .math_tree
            .root
            .content
            .iter()
            .any(|content| content_has_environment(content, kind)),
        Block::Align(align) => align.rows.iter().any(|row| {
            row.cells.iter().any(|tree| {
                tree.root
                    .content
                    .iter()
                    .any(|content| content_has_environment(content, kind))
            })
        }),
        Block::Paragraph(_)
        | Block::Environment(_)
        | Block::ExplicitBreak(_)
        | Block::Opaque(_) => false,
    })
}

#[test]
fn adjacent_align_text_islands_keep_cell_host_offsets_through_edit_and_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText(r"\text{a}&\text{b}\text{c}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((0, 1))
    ));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));

    session.run(Command::InsertText("X".to_owned()))?;
    assert!(inline_source(&session).is_some_and(|source| source.contains("bXc")));
    session.run(Command::Undo)?;
    assert!(inline_source(&session).is_some_and(|source| source.contains("bc")));
    session.run(Command::Redo)?;
    assert!(inline_source(&session).is_some_and(|source| source.contains("bXc")));
    Ok(())
}

#[test]
fn equation_text_delete_snaps_after_graphemes_join() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "equation")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("\\text{🇦x🇧}".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(4));
    session.run(Command::Delete(Direction::Forward))?;

    assert_eq!(inline_source(&session).as_deref(), Some("\\text{🇦🇧}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    Ok(())
}

#[test]
fn align_text_island_uses_parser_cell_ranges_after_comments_and_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let mut align = session_with_text("")?;
    accept(&mut align, "align")?;
    enter_source_edit(&mut align, Direction::Forward)?;
    align.run(Command::PasteText(
        "x & y \\\\ % row note\n \\text{abc}".to_owned(),
    ))?;
    align.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        align.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { path, ..
                }) if path.align_cell() == Some((1, 0))
    ));
    align.run(Command::InsertText("X".to_owned()))?;
    assert!(inline_source(&align).is_some_and(|source| source.contains(r"\text{abcX}")));
    assert!(align.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none()
            && matches!(&edit.branch, MathEditBranch::Text { path, .. } if path.align_cell() == Some((1, 0)))
    }));
    Ok(())
}

#[test]
fn enter_in_align_textflow_rejects_without_using_source_offsets()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(Paragraph::empty(NodeId::generate())),
        display_math_block(true, r"\text{abc}")?,
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Text { .. })
    }));
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();

    assert_eq!(
        session.run(Command::EditorKey(EditorKey::Enter)),
        Err(EditError::UnsupportedNewline)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    Ok(())
}

#[test]
fn text_island_edits_preserve_run_properties_and_reject_opaque_run_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abc}}".to_owned(),
    ))?;
    move_into_math_text(&mut session)?;
    let paragraph = match session.document.body.first_mut() {
        Some(Block::Paragraph(paragraph)) => paragraph,
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let math_id = match paragraph.inlines.first() {
        Some(caret_doc::FlowContent::Math(math)) => math.id.clone(),
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let text =
        first_math_text_flow_mut(&mut math.math_tree.root).ok_or(EditError::InvalidPosition)?;
    text.runs[0]
        .props
        .insert("underline".to_owned(), serde_json::Value::Bool(true));
    session.run(Command::InsertText("X".to_owned()))?;
    let paragraph = match session.document.body.first() {
        Some(Block::Paragraph(paragraph)) => paragraph,
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let math = find_inline_math(&paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let text = first_math_text_flow(&math.math_tree.root).ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abcX"
    );
    assert!(
        text.runs.last().is_some_and(|run| {
            run.props.get("underline") == Some(&serde_json::Value::Bool(true))
        })
    );

    session.run(Command::Delete(Direction::Backward))?;
    session.run(Command::InsertText("Y".to_owned()))?;
    let paragraph = match session.document.body.first() {
        Some(Block::Paragraph(paragraph)) => paragraph,
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let math = find_inline_math(&paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let text = first_math_text_flow(&math.math_tree.root).ok_or(EditError::InvalidPosition)?;
    assert_eq!(
        text.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abcY"
    );
    assert!(
        text.runs.last().is_some_and(|run| {
            run.props.get("underline") == Some(&serde_json::Value::Bool(true))
        })
    );

    let paragraph = match session.document.body.first_mut() {
        Some(Block::Paragraph(paragraph)) => paragraph,
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let math =
        find_inline_math_mut(&mut paragraph.inlines, &math_id).ok_or(EditError::InvalidPosition)?;
    let text =
        first_math_text_flow_mut(&mut math.math_tree.root).ok_or(EditError::InvalidPosition)?;
    text.runs
        .last_mut()
        .ok_or(EditError::InvalidPosition)?
        .extra
        .insert("vendorRun".to_owned(), serde_json::Value::Bool(true));
    let before = session.document().clone();
    let before_edit = session.math_edit().cloned();
    assert_eq!(
        session.run(Command::InsertText("Y".to_owned())),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    Ok(())
}

#[test]
fn cross_run_grapheme_delete_preserves_unknown_run_fields_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    insert_equation_text(&mut session, "\\text{a\u{0301}}")?;
    move_into_math_text(&mut session)?;
    let equation = match session.document.body.first_mut() {
        Some(Block::Equation(equation)) => equation,
        _ => return Err(EditError::InvalidPosition.into()),
    };
    let text =
        first_math_text_flow_mut(&mut equation.math_tree.root).ok_or(EditError::InvalidPosition)?;
    text.runs[0].text = "a".to_owned();
    text.runs.push(caret_doc::Run {
        text: "\u{0301}".to_owned(),
        language: None,
        props: Default::default(),
        extra: std::collections::BTreeMap::from([(
            "vendorRun".to_owned(),
            serde_json::Value::Bool(true),
        )]),
    });
    let before = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();

    assert_eq!(
        session.run(Command::Delete(Direction::Backward)),
        Err(EditError::UnsupportedOpaqueContent)
    );
    assert_eq!(session.document(), &before);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    Ok(())
}

#[test]
fn tab_does_not_exit_math_edit_or_insert_spaces_in_the_host_paragraph() -> Result<(), EditError> {
    let mut session = session_with_text("beforeafter").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.begin_inline_math()?;
    let document_before_tab = session.document().clone();
    let math_edit_before_tab = session.math_edit().cloned();

    session.run(Command::EditorKey(EditorKey::Tab))?;

    assert_eq!(session.document(), &document_before_tab);
    assert_eq!(session.math_edit(), math_edit_before_tab.as_ref());
    Ok(())
}

#[test]
fn paste_preserves_latex_group_syntax() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::PasteText("x^{2}".to_owned()))?;

    assert_eq!(inline_source(&session).as_deref(), Some("x^{2}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(5));
    Ok(())
}

#[test]
fn align_row_break_is_one_navigation_and_deletion_unit() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText(r"a\\abc".to_owned()))?;

    for _ in 0..4 {
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("aabc"));
    Ok(())
}

#[test]
fn math_source_mode_rejects_ime_without_changing_focus_or_document()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("abc".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let before_document = session.document().clone();
    let before_edit = session.math_edit().cloned();
    let before_generation = session.document_generation();
    let before_session_id = session.session_id().clone();
    assert_eq!(
        session.run(Command::UpdateComposition {
            id: 73,
            preedit: "X".to_owned(),
        }),
        Err(EditError::CompositionPending)
    );
    assert!(matches!(
        session.composition_update_candidate(73, "X".to_owned(), None, 1),
        Err(EditError::CompositionPending)
    ));
    assert_eq!(
        session.commit_composition_replacement(None, 74, 75, -1, 1, "X"),
        Err(EditError::CompositionPending)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_edit(), before_edit.as_ref());
    assert_eq!(session.document_generation(), before_generation);
    assert_eq!(session.session_id(), &before_session_id);
    assert!(session.composition().is_none());
    Ok(())
}

#[test]
fn align_cell_mapping_counts_rows_and_cells() {
    assert_eq!(
        projected_align_cell_at("a&=b \\\\ c& =d", 12),
        Some((1, 1, 1))
    );
    assert_eq!(projected_align_cell_at("a&=b", 1), Some((0, 0, 1)));
    assert_eq!(projected_align_cell_at("a  &b", 2), Some((0, 0, 2)));
    assert_eq!(projected_align_cell_at("a  &b", 3), Some((0, 0, 3)));
    assert_eq!(projected_align_cell_at("a  ", 3), Some((0, 0, 3)));
    assert_eq!(projected_align_cell_at("a  \\\\  b", 3), Some((0, 0, 3)));
    assert_eq!(projected_align_cell_at("   ", 0), Some((0, 0, 0)));
    assert_eq!(projected_align_cell_at("   ", 1), Some((0, 0, 0)));
    assert_eq!(
        projected_align_cell_at("% empty align\n", 4),
        Some((0, 0, 0))
    );
}

#[test]
fn align_cell_mapping_ignores_nested_environment_separators() {
    let source = r"\begin{matrix}a&b\\c&d\end{matrix}&=z";
    let nested_cell = source
        .find("c&d")
        .unwrap_or_else(|| panic!("missing nested cell"))
        + 2;
    assert_eq!(
        projected_align_cell_at(source, nested_cell),
        Some((0, 0, nested_cell))
    );

    let outer_cell = source
        .rfind('&')
        .unwrap_or_else(|| panic!("missing outer separator"))
        + 1;
    assert_eq!(projected_align_cell_at(source, outer_cell), Some((0, 1, 0)));

    let commented_begin =
        "\\begin % environment name follows the comment\n{matrix}a&b\\end{matrix}&=z";
    let nested_separator = commented_begin
        .find("a&b")
        .unwrap_or_else(|| panic!("missing commented nested matrix"))
        + 2;
    assert_eq!(
        projected_align_cell_at(commented_begin, nested_separator),
        Some((0, 0, nested_separator))
    );
}

#[test]
fn align_cell_mapping_ignores_separators_in_comments() {
    let source = "a % & ignored \\\\ ignored\n+b&c";
    let in_comment = source
        .find("ignored")
        .unwrap_or_else(|| panic!("missing comment"))
        + 3;
    assert_eq!(
        projected_align_cell_at(source, in_comment),
        Some((0, 0, in_comment))
    );
    let after_outer_separator = source
        .rfind('&')
        .unwrap_or_else(|| panic!("missing separator"))
        + 2;
    assert_eq!(
        projected_align_cell_at(source, after_outer_separator),
        Some((0, 1, 1))
    );

    assert_eq!(
        projected_align_cell_at("% leading cell comment\nx", 24),
        Some((0, 0, 1))
    );
    assert_eq!(
        projected_align_cell_at("% leading cell comment\nx", 4),
        Some((0, 0, 0))
    );
    let later_cell_comment = "a&% comment\nb";
    let comment_cursor = later_cell_comment
        .find("comment")
        .unwrap_or_else(|| panic!("missing cell comment"))
        + 3;
    assert_eq!(
        projected_align_cell_at(later_cell_comment, comment_cursor),
        Some((0, 1, 0))
    );
    let later_row_comment = "a \\\\% comment\nb";
    let row_comment_cursor = later_row_comment
        .find("comment")
        .unwrap_or_else(|| panic!("missing row comment"))
        + 3;
    assert_eq!(
        projected_align_cell_at(later_row_comment, row_comment_cursor),
        Some((1, 0, 0))
    );
}

#[test]
fn preserving_math_metadata_keeps_tree_and_unchanged_node_identity() {
    let mut previous = parsed_inline_tree("a+b");
    previous
        .extra
        .insert("futureTreeValue".to_owned(), serde_json::json!(1));
    let unchanged_id = math_node(&previous.root, 0).id.clone();
    math_node_mut(&mut previous.root, 0)
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("kept"));

    let mut updated = parsed_inline_tree("a+bc");
    preserve_math_tree_metadata(&previous, &mut updated);

    assert_eq!(updated.extra["futureTreeValue"], serde_json::json!(1));
    assert_eq!(math_node(&updated.root, 0).id, unchanged_id);
    assert_eq!(
        math_node(&updated.root, 0).extra["futureNodeValue"],
        serde_json::json!("kept")
    );
}

#[test]
fn metadata_mapping_preserves_math_text_runs_and_environment_identity() {
    fn first_environment(
        flow: &mut MathFlow,
    ) -> Option<&mut caret_doc::InlineEnvironment<MathContent>> {
        for content in &mut flow.content {
            match content {
                MathContent::Environment(environment) => return Some(environment),
                MathContent::Math(node) => {
                    for child in &mut node.children {
                        if let Some(environment) = first_environment(child) {
                            return Some(environment);
                        }
                    }
                }
                MathContent::Text(_) | MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
            }
        }
        None
    }
    fn first_text(contents: &mut [MathContent]) -> Option<&mut caret_doc::TextFlow> {
        for content in contents {
            match content {
                MathContent::Text(text) => return Some(text),
                MathContent::Environment(environment) => {
                    if let Some(text) = first_text(&mut environment.children) {
                        return Some(text);
                    }
                }
                MathContent::Math(node) => {
                    for child in &mut node.children {
                        if let Some(text) = first_text(&mut child.content) {
                            return Some(text);
                        }
                    }
                }
                MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
            }
        }
        None
    }

    let source = r"\text{\textbf{a}}+x";
    let mut previous = parsed_inline_tree(source);
    let environment = first_environment(&mut previous.root)
        .unwrap_or_else(|| panic!("text formatting environment should be parsed"));
    let environment_id = environment.id.clone();
    environment.definition_id = Some("template.bold".to_owned());
    environment
        .props
        .insert("vendorProperty".to_owned(), serde_json::json!("kept"));
    environment
        .extra
        .insert("vendorEnvironment".to_owned(), serde_json::json!(3));

    let text = first_text(&mut previous.root.content)
        .unwrap_or_else(|| panic!("text mode should produce a TextFlow"));
    text.runs[0].language = Some("en".to_owned());
    text.runs[0]
        .props
        .insert("underline".to_owned(), serde_json::json!(true));
    text.runs[0]
        .extra
        .insert("vendorRun".to_owned(), serde_json::json!(4));
    let updated_source = source.replace("+x", "+y");
    let mut updated = parsed_inline_tree(&updated_source);
    let edit = (source.len() - 1)..source.len();
    assert!(preserve_math_tree_metadata_for_test(
        &previous,
        &mut updated,
        &edit,
        &edit,
    ));
    let environment = first_environment(&mut updated.root)
        .unwrap_or_else(|| panic!("mapped environment should remain"));
    assert_eq!(environment.id, environment_id);
    assert_eq!(environment.definition_id.as_deref(), Some("template.bold"));
    assert_eq!(environment.props["vendorProperty"], "kept");
    assert_eq!(environment.extra["vendorEnvironment"], 3);
    let text = first_text(&mut updated.root.content)
        .unwrap_or_else(|| panic!("mapped TextFlow should remain"));
    assert_eq!(text.runs[0].language.as_deref(), Some("en"));
    assert_eq!(text.runs[0].props["underline"], true);
    assert_eq!(text.runs[0].extra["vendorRun"], 4);
}

#[test]
fn math_edit_refuses_to_drop_unknown_fields_from_an_unmapped_node() {
    let mut previous = parsed_inline_tree("a+b");
    math_node_mut(&mut previous.root, 0)
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("keep"));
    let mut updated = parsed_inline_tree(r"\frac{a}{b}");

    assert!(!preserve_math_tree_metadata(&previous, &mut updated));
}

#[test]
fn repeated_structure_insertions_keep_metadata_on_the_original_node() {
    let mut previous = parsed_inline_tree("a+a");
    math_node_mut(&mut previous.root, 0)
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("original"));
    let original_id = math_node(&previous.root, 0).id.clone();
    let mut updated = parsed_inline_tree("a+a+a");

    assert!(preserve_math_tree_metadata_for_test(
        &previous,
        &mut updated,
        &(0..0),
        &(0..2),
    ));
    assert!(math_node(&updated.root, 0).extra.is_empty());
    assert_eq!(math_node(&updated.root, 2).id, original_id);
    assert_eq!(
        math_node(&updated.root, 2).extra["futureNodeValue"],
        "original"
    );
}

#[test]
fn deleting_a_node_with_unmappable_metadata_keeps_document_and_math_focus() -> Result<(), EditError>
{
    let mut math_tree = parsed_inline_tree("a+b");
    math_node_mut(&mut math_tree.root, 0)
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("keep"));
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(ParagraphContent::Math(InlineMath {
        id: NodeId::generate(),
        math_tree,
        extra: std::collections::BTreeMap::new(),
    }));
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session = Session::new(document).map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    enter_source_edit(&mut session, Direction::Backward)?;

    let mut committed_document = session.document().clone();
    for _ in 0..8 {
        let generation_before = session.document_generation();
        session.run(Command::Delete(Direction::Backward))?;
        if session.has_uncommitted_math_source_draft() {
            assert_eq!(session.document(), &committed_document);
            assert_eq!(session.document_generation(), generation_before);
            break;
        }
        committed_document = session.document().clone();
    }
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some(""));
    let document_before = session.document().clone();
    assert!(matches!(
        session.run(Command::Delete(Direction::Backward)),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &document_before);
    assert!(session.has_uncommitted_math_source_draft());
    Ok(())
}

#[test]
fn splitting_a_metadata_bearing_align_row_is_rejected() -> Result<(), EditError> {
    let Block::Align(mut previous) = display_math_block(true, "a+b&b")? else {
        panic!("expected Align block");
    };
    let old_row_id = previous.rows[0].id.clone();
    previous.rows[0].extra.insert(
        "futureRowValue".to_owned(),
        serde_json::json!("preserve once"),
    );
    let first_cell_id = previous.rows[0].cells[0].root.id.clone();
    let second_cell_id = previous.rows[0].cells[1].root.id.clone();
    previous.rows[0].cells[0]
        .extra
        .insert("futureCellValue".to_owned(), serde_json::json!("left"));
    previous.rows[0].cells[1]
        .extra
        .insert("futureCellValue".to_owned(), serde_json::json!("right"));

    let Block::Align(mut updated) =
        display_math_block_with_id(true, r"a+b \\ b", previous.id.clone())?
    else {
        panic!("expected parsed Align block");
    };
    let old_offset = aligned_body_offset(&previous);
    let new_offset = aligned_body_offset(&updated);
    let metadata_preserved = preserve_align_metadata_for_test(
        &previous,
        &mut updated,
        &(old_offset + 3..old_offset + 4),
        &(new_offset + 3..new_offset + 7),
    );

    assert!(!metadata_preserved);
    assert_eq!(previous.rows[0].id, old_row_id);
    assert_eq!(previous.rows[0].cells[0].root.id, first_cell_id);
    assert_eq!(previous.rows[0].cells[1].root.id, second_cell_id);
    Ok(())
}

#[test]
fn editing_align_cell_preserves_tree_and_node_metadata() -> Result<(), EditError> {
    let Block::Align(mut previous) = display_math_block(true, "a&b+c")? else {
        panic!("expected Align block");
    };
    let row_id = previous.rows[0].id.clone();
    let first_cell_id = previous.rows[0].cells[0].root.id.clone();
    let edited_cell_id = previous.rows[0].cells[1].root.id.clone();
    previous.rows[0].cells[0]
        .extra
        .insert("futureTreeValue".to_owned(), serde_json::json!("unchanged"));
    previous.rows[0].cells[0]
        .root
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("unchanged"));
    previous.rows[0].cells[1]
        .root
        .extra
        .insert("editedCellNodeValue".to_owned(), serde_json::json!("kept"));
    let Block::Align(mut updated) = display_math_block_with_id(true, "a&b+x", previous.id.clone())?
    else {
        panic!("expected parsed Align block");
    };
    let old_offset = aligned_body_offset(&previous);
    let new_offset = aligned_body_offset(&updated);
    preserve_align_metadata_for_test(
        &previous,
        &mut updated,
        &(old_offset + 4..old_offset + 5),
        &(new_offset + 4..new_offset + 5),
    );

    assert_eq!(updated.rows[0].id, row_id);
    assert_eq!(
        updated.rows[0].cells[0].extra["futureTreeValue"],
        "unchanged"
    );
    assert_eq!(updated.rows[0].cells[0].root.id, first_cell_id);
    assert_eq!(
        updated.rows[0].cells[0].root.extra["futureNodeValue"],
        "unchanged"
    );
    assert_eq!(updated.rows[0].cells[1].root.id, edited_cell_id);
    assert_eq!(
        updated.rows[0].cells[1].root.extra["editedCellNodeValue"],
        "kept"
    );
    Ok(())
}

#[test]
fn align_projection_is_regenerated_and_keeps_focus_through_edit_undo_and_redo()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "% before\n\\begin {aligned}a&b\\end{aligned} % after\n";
    let caret_math::ProjectedMath::Align(mut align) =
        caret_math::parse_projection(source, caret_math::MathInputKind::Align)?
            .into_parts()
            .0
    else {
        panic!("Align input should parse to an Align tree");
    };
    let untouched_cell_id = align.rows[0].cells[1].root.id.clone();
    align.rows[0].cells[1]
        .extra
        .insert("futureCellValue".to_owned(), serde_json::json!("preserve"));
    let canonical_source = caret_math::serialize_align(&align);

    let mut document = Document::blank();
    document.body.push(Block::Align(align));
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    enter_source_edit(&mut session, Direction::Forward)?;

    let initial_edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(source_projection(initial_edit).source(), canonical_source);
    session.run(Command::InsertText("x".to_owned()))?;
    let edited_source = session
        .math_edit()
        .map(source_text)
        .ok_or(EditError::InvalidPosition)?
        .to_owned();
    let Block::Align(edited_align) = session
        .document()
        .body
        .get(1)
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    assert_eq!(
        session.math_edit().map(source_text),
        Some(edited_source.as_str())
    );
    assert_eq!(edited_align.rows[0].cells[1].root.id, untouched_cell_id);
    assert_eq!(
        edited_align.rows[0].cells[1].extra["futureCellValue"],
        "preserve"
    );

    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().map(source_text), Some("a & b"));
    assert_eq!(session.math_edit().map(source_text), Some("a & b"));
    session.run(Command::Redo)?;
    assert_eq!(session.math_edit().map(source_text), Some("xa & b"));
    assert_eq!(session.math_edit().map(source_text), Some("xa & b"));
    Ok(())
}

#[test]
fn appending_duplicate_align_cell_keeps_old_metadata_at_source_positions() -> Result<(), EditError>
{
    let Block::Align(mut previous) = display_math_block(true, "a&a")? else {
        panic!("expected Align block");
    };
    let first_id = previous.rows[0].cells[0].root.id.clone();
    let last_id = previous.rows[0].cells[1].root.id.clone();
    previous.rows[0].cells[0]
        .extra
        .insert("origin".to_owned(), serde_json::json!("first"));
    previous.rows[0].cells[1]
        .extra
        .insert("origin".to_owned(), serde_json::json!("last"));

    let Block::Align(mut updated) = display_math_block_with_id(true, "a&a&a", previous.id.clone())?
    else {
        panic!("expected parsed Align block");
    };
    let old_offset = aligned_body_offset(&previous);
    let new_offset = aligned_body_offset(&updated);
    let old_body_len = aligned_body_len(&previous);
    let new_body_len = aligned_body_len(&updated);
    preserve_align_metadata_for_test(
        &previous,
        &mut updated,
        &(old_offset + old_body_len..old_offset + old_body_len),
        &(new_offset + old_body_len..new_offset + new_body_len),
    );

    assert_eq!(updated.rows[0].cells[0].root.id, first_id);
    assert_eq!(updated.rows[0].cells[1].root.id, last_id);
    assert_eq!(updated.rows[0].cells[0].extra["origin"], "first");
    assert_eq!(updated.rows[0].cells[1].extra["origin"], "last");
    Ok(())
}

#[test]
fn inserting_duplicate_align_cell_before_existing_cells_keeps_their_identity()
-> Result<(), EditError> {
    let Block::Align(previous) = display_math_block(true, "a&a")? else {
        panic!("expected Align block");
    };
    let first_id = previous.rows[0].cells[0].root.id.clone();
    let last_id = previous.rows[0].cells[1].root.id.clone();
    let Block::Align(mut updated) = display_math_block_with_id(true, "a&a&a", previous.id.clone())?
    else {
        panic!("expected parsed Align block");
    };
    let old_offset = aligned_body_offset(&previous);
    let new_offset = aligned_body_offset(&updated);
    let inserted_len = aligned_body_len(&updated) - aligned_body_len(&previous);
    preserve_align_metadata_for_test(
        &previous,
        &mut updated,
        &(old_offset..old_offset),
        &(new_offset..new_offset + inserted_len),
    );

    assert_eq!(updated.rows[0].cells[1].root.id, first_id);
    assert_eq!(updated.rows[0].cells[2].root.id, last_id);
    Ok(())
}

#[test]
fn filling_an_empty_align_cell_preserves_its_identity_and_unknown_fields() -> Result<(), EditError>
{
    let Block::Align(mut previous) = display_math_block(true, "a&&b")? else {
        panic!("expected Align block");
    };
    let empty_cell = &mut previous.rows[0].cells[1];
    let cell_id = empty_cell.root.id.clone();
    empty_cell
        .extra
        .insert("futureCellValue".to_owned(), serde_json::json!("keep"));
    let Block::Align(mut updated) = display_math_block_with_id(true, "a&x&b", previous.id.clone())?
    else {
        panic!("expected parsed Align block");
    };
    let old_range = aligned_cell_range(&previous, 0, 1);
    let new_range = aligned_cell_range(&updated, 0, 1);
    preserve_align_metadata_for_test(&previous, &mut updated, &old_range, &new_range);

    assert_eq!(updated.rows[0].cells[1].root.id, cell_id);
    assert_eq!(updated.rows[0].cells[1].extra["futureCellValue"], "keep");
    Ok(())
}

#[test]
fn align_metadata_matches_cells_one_to_one_across_trivia_and_merges() -> Result<(), EditError> {
    let previous_source = "\\begin{aligned}a& % before\n \\end{aligned}";
    let updated_source = "\\begin{aligned}a&x % before\n \\end{aligned}";
    let previous_projection =
        caret_math::parse_projection(previous_source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
    let updated_projection =
        caret_math::parse_projection(updated_source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
    let (caret_math::ProjectedMath::Align(mut previous), previous_map) =
        previous_projection.into_parts()
    else {
        return Err(EditError::InvalidPosition);
    };
    let (caret_math::ProjectedMath::Align(mut updated), updated_map) =
        updated_projection.into_parts()
    else {
        return Err(EditError::InvalidPosition);
    };
    let empty_cell_id = previous.rows[0].cells[1].root.id.clone();
    previous.rows[0].cells[1]
        .extra
        .insert("vendorCell".to_owned(), serde_json::json!("kept"));
    let old_cursor = previous_map
        .align()
        .ok_or(EditError::InvalidPosition)?
        .cell_navigation_ranges()[0][1]
        .start;
    let new_range = updated_map
        .align()
        .and_then(|map| map.cell_range(0, 1))
        .ok_or(EditError::InvalidPosition)?;
    assert!(preserve_align_metadata(
        &previous,
        &mut updated,
        &(old_cursor..old_cursor),
        &new_range,
        &previous_map,
        &updated_map,
    ));
    assert_eq!(updated.rows[0].cells[1].root.id, empty_cell_id);
    assert_eq!(updated.rows[0].cells[1].extra["vendorCell"], "kept");

    let previous_source = "\\begin{aligned}a&b\\end{aligned}";
    let updated_source = "\\begin{aligned}b\\end{aligned}";
    let previous_projection =
        caret_math::parse_projection(previous_source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
    let updated_projection =
        caret_math::parse_projection(updated_source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
    let (caret_math::ProjectedMath::Align(mut previous), previous_map) =
        previous_projection.into_parts()
    else {
        return Err(EditError::InvalidPosition);
    };
    let (caret_math::ProjectedMath::Align(mut updated), updated_map) =
        updated_projection.into_parts()
    else {
        return Err(EditError::InvalidPosition);
    };
    let surviving_cell_id = previous.rows[0].cells[1].root.id.clone();
    previous.rows[0].cells[1]
        .extra
        .insert("vendorCell".to_owned(), serde_json::json!("survives"));
    let old_map = previous_map.align().ok_or(EditError::InvalidPosition)?;
    let new_range = updated_map
        .align()
        .and_then(|map| map.cell_range(0, 0))
        .ok_or(EditError::InvalidPosition)?;
    let old_second_cell = old_map.cell_range(0, 1).ok_or(EditError::InvalidPosition)?;
    let old_edit = old_map.cell_navigation_ranges()[0][0].start..old_second_cell.start;
    assert!(preserve_align_metadata(
        &previous,
        &mut updated,
        &old_edit,
        &(new_range.start..new_range.start),
        &previous_map,
        &updated_map,
    ));
    assert_eq!(updated.rows[0].cells[0].root.id, surviving_cell_id);
    assert_eq!(updated.rows[0].cells[0].extra["vendorCell"], "survives");
    Ok(())
}

#[test]
fn align_metadata_matches_rows_with_trivia_around_an_edited_cell() -> Result<(), EditError> {
    let previous_source = "\\begin{aligned}a\\\\ % before\n \\sqrt{} % after\n \\\\b\\end{aligned}";
    let updated_source = "\\begin{aligned}a\\\\ % before\n % after\n \\\\b\\end{aligned}";
    let previous_projection =
        caret_math::parse_projection(previous_source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
    let updated_projection =
        caret_math::parse_projection(updated_source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
    let (caret_math::ProjectedMath::Align(mut previous), previous_map) =
        previous_projection.into_parts()
    else {
        return Err(EditError::InvalidPosition);
    };
    let (caret_math::ProjectedMath::Align(mut updated), updated_map) =
        updated_projection.into_parts()
    else {
        return Err(EditError::InvalidPosition);
    };
    let row_id = previous.rows[1].id.clone();
    let cell_id = previous.rows[1].cells[0].root.id.clone();
    previous.rows[1]
        .extra
        .insert("vendorRow".to_owned(), serde_json::json!("kept"));
    previous.rows[1].cells[0]
        .extra
        .insert("vendorCell".to_owned(), serde_json::json!("kept"));
    let old_edit_start = previous_source
        .find(r"\sqrt{}")
        .ok_or(EditError::InvalidPosition)?;
    let old_edit = old_edit_start..old_edit_start + r"\sqrt{}".len();
    let new_edit = old_edit_start..old_edit_start;

    assert_eq!(previous.rows.len(), 3);
    assert_eq!(updated.rows.len(), 3);
    assert!(preserve_align_metadata(
        &previous,
        &mut updated,
        &old_edit,
        &new_edit,
        &previous_map,
        &updated_map,
    ));
    assert_eq!(updated.rows[1].id, row_id);
    assert_eq!(updated.rows[1].extra["vendorRow"], "kept");
    assert_eq!(updated.rows[1].cells[0].root.id, cell_id);
    assert_eq!(updated.rows[1].cells[0].extra["vendorCell"], "kept");
    Ok(())
}

#[test]
fn reparsing_unranged_math_nodes_never_reuses_an_identity_twice() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\sum".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::InsertText(" ".to_owned()))?;

    session.document().validate().map_err(|error| {
        EditError::InvalidDocument(format!("math node IDs should remain unique: {error}"))
    })?;
    session
        .document()
        .validate()
        .map_err(|error| EditError::InvalidDocument(error.to_string()))
}

#[test]
fn preserving_math_metadata_keeps_the_parent_of_an_edited_subtree() {
    let old_source = "\\frac{ab}{c}";
    let new_source = "\\frac{axb}{c}";
    let mut previous = parsed_inline_tree(old_source);
    let (prefix_end, previous_tail, _) = unchanged_source_boundaries(old_source, new_source);
    let projection = caret_math::parse_projection(old_source, caret_math::MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("old source should project: {error}"));
    let operands = projection
        .source_map()
        .tree()
        .and_then(|map| map.content_at(&[caret_doc::MathPathStep::Content(0)]))
        .and_then(caret_math::MathContentSourceMap::operands)
        .unwrap_or_else(|| panic!("fraction should expose operand maps"));
    let Some(changed_index) = operands.iter().position(|flow| {
        flow.range()
            .is_some_and(|range| range.start <= prefix_end && range.end >= previous_tail)
    }) else {
        panic!("fraction operand should contain the edit");
    };
    let changed_parent = &mut math_node_mut(&mut previous.root, 0).children[changed_index];
    let changed_parent_id = changed_parent.id.clone();
    changed_parent
        .extra
        .insert("futureOperandValue".to_owned(), serde_json::json!(true));

    let mut updated = parsed_inline_tree(new_source);
    preserve_math_tree_metadata(&previous, &mut updated);

    let preserved = math_node(&updated.root, 0)
        .children
        .iter()
        .find(|flow| flow.extra.contains_key("futureOperandValue"));
    assert!(preserved.is_some_and(|node| {
        node.id == changed_parent_id && node.extra["futureOperandValue"] == true
    }));
}

use crate::editing::{CommandLineMode, EditorKey};
use caret_doc::{Document, InlineEnvironmentKind, Run};
use std::collections::BTreeMap;

fn parsed_inline_tree(source: &str) -> MathTree {
    match caret_math::parse_projection(source, caret_math::MathInputKind::Inline) {
        Ok(projection) => match projection.into_parts().0 {
            caret_math::ProjectedMath::Tree(tree) => tree,
            caret_math::ProjectedMath::Align(_) => panic!("expected inline math tree"),
        },
        Err(error) => panic!("test formula should parse: {error}"),
    }
}

fn preserve_math_tree_metadata_for_test(
    previous: &MathTree,
    updated: &mut MathTree,
    old_edit: &std::ops::Range<usize>,
    new_edit: &std::ops::Range<usize>,
) -> bool {
    let previous_source = caret_math::serialize_tree(previous);
    let updated_source = caret_math::serialize_tree(updated);
    let (Ok(previous_projection), Ok(updated_projection)) = (
        caret_math::parse_projection(&previous_source, caret_math::MathInputKind::Inline),
        caret_math::parse_projection(&updated_source, caret_math::MathInputKind::Inline),
    ) else {
        return false;
    };
    let (Some(previous_map), Some(updated_map)) = (
        previous_projection.source_map().tree(),
        updated_projection.source_map().tree(),
    ) else {
        return false;
    };
    preserve_math_tree_metadata_for_edit(
        previous,
        updated,
        old_edit,
        new_edit,
        previous_map,
        updated_map,
    )
}

fn math_node(flow: &MathFlow, index: usize) -> &MathNode {
    flow.content
        .iter()
        .filter_map(|content| match content {
            MathContent::Math(node) => Some(node),
            _ => None,
        })
        .nth(index)
        .unwrap_or_else(|| panic!("expected math node at index {index}"))
}

fn math_node_mut(flow: &mut MathFlow, index: usize) -> &mut MathNode {
    flow.content
        .iter_mut()
        .filter_map(|content| match content {
            MathContent::Math(node) => Some(node),
            _ => None,
        })
        .nth(index)
        .unwrap_or_else(|| panic!("expected math node at index {index}"))
}

fn session_with_text(text: &str) -> Result<Session, Box<dyn std::error::Error>> {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    if !text.is_empty() {
        paragraph
            .inlines
            .push(ParagraphContent::Text(caret_doc::TextFlow {
                runs: vec![Run {
                    text: text.to_owned(),
                    language: None,
                    props: BTreeMap::new(),
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            }));
    }
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session = Session::new(document)?;
    if !text.is_empty() {
        for _ in 0..text.chars().count() {
            session.run(Command::MoveCaret(Direction::Forward))?;
        }
    }
    Ok(session)
}

fn insert_equation_text(session: &mut Session, source: &str) -> Result<(), EditError> {
    session.insert_display_math(false)?;
    let edit = session
        .math_edit
        .clone()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let (projection, cursor) =
        crate::math_validation::project_tree_for_edit(tree, caret_math::MathInputKind::Equation)
            .map(|(projection, _)| {
                let cursor = projection.body_range().start;
                (projection, cursor)
            })
            .map_err(EditError::InvalidMath)?;
    let current = session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?;
    current.source_projection = Some(std::sync::Arc::new(projection));
    current.source_draft = None;
    current.cursor = cursor;
    current.branch = MathEditBranch::Source;
    session.run(Command::InsertText(source.to_owned()))?;
    Ok(())
}

fn move_into_math_text(session: &mut Session) -> Result<(), EditError> {
    session.run(Command::EnterMath(Direction::Backward))?;
    for _ in 0..64 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            return Ok(());
        }
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    Err(EditError::InvalidPosition)
}

fn accept(session: &mut Session, query: &str) -> Result<Effect, EditError> {
    // Exercise internal math editing separately from the disabled production candidates.
    match query {
        "math" => return session.begin_inline_math(),
        "equation" => return session.insert_display_math(false),
        "align" => return session.insert_display_math(true),
        _ => {}
    }
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine(query.to_owned()))?;
    session.run(Command::AcceptCommandLine)
}

fn inline_source(session: &Session) -> Option<String> {
    let edit = session.math_edit()?;
    if !matches!(edit.branch, MathEditBranch::Source) {
        match &edit.target {
            MathEditTarget::Inline { .. } => committed_inline_source(session),
            MathEditTarget::Block { block_id, .. } => {
                match find_block_by_id(&session.document.body, block_id)? {
                    Block::Equation(equation) => {
                        Some(caret_math::serialize_tree(&equation.math_tree))
                    }
                    Block::Align(align) => Some(caret_math::serialize_align(align)),
                    _ => None,
                }
            }
        }
    } else {
        Some(source_text(edit).to_owned())
    }
}

fn committed_inline_source(session: &Session) -> Option<String> {
    let Block::Paragraph(paragraph) = session.document().body.first()? else {
        return None;
    };
    let math = paragraph.inlines.iter().find_map(|content| match content {
        ParagraphContent::Math(math) => Some(math),
        ParagraphContent::Text(_)
        | ParagraphContent::Environment(_)
        | ParagraphContent::SoftBreak(_)
        | ParagraphContent::Opaque(_) => None,
    })?;
    Some(caret_math::serialize_tree(&math.math_tree))
}

#[test]
fn math_command_inserts_inline_math_and_sub_commands_edit_it()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("a")?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Backward)?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Inline { .. })
    ));
    session.run(Command::InsertText("x^2".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("x^{2}"));
    session.run(Command::InsertText("}".to_owned()))?;
    session.run(Command::BeginCommandLine)?;
    assert_eq!(
        session.command_line().map(|line| line.mode),
        Some(CommandLineMode::Math)
    );
    session.run(Command::TypeCommandLine("frac".to_owned()))?;
    session.run(Command::AcceptCommandLine)?;
    assert_eq!(inline_source(&session).as_deref(), Some("x^{2}\\frac{}{}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(11));
    session.run(Command::InsertText("1".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::InsertText("n".to_owned()))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some("x^{2}\\frac{1}{n}")
    );
    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.offset, 4);
    // 撤销是正文命令：退出公式编辑后按步恢复源码。
    session.run(Command::Undo)?;
    assert_eq!(inline_source(&session).as_deref(), Some("x^{2}\\frac{1}{}"));
    Ok(())
}

#[test]
fn math_command_query_rejects_ime_preedit_and_commit() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("frac".to_owned()))?;
    let query_before = session.command_line().cloned();
    let document_before = session.document().clone();
    let generation_before = session.document_generation();

    assert_eq!(
        session.input_method_command_line("ni".to_owned(), "字".to_owned(), 0, 0, 2),
        Err(EditError::CompositionPending)
    );
    assert_eq!(session.command_line(), query_before.as_ref());
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.document_generation(), generation_before);
    assert!(session.composition().is_none());
    session.run(Command::EditorKey(EditorKey::ShiftEnter))?;
    assert_eq!(session.command_line(), query_before.as_ref());
    assert_eq!(session.document(), &document_before);
    Ok(())
}

#[test]
fn undo_and_redo_from_math_command_query_restore_formula_editing() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    accept(&mut session, "math").map_err(|_| EditError::InvalidPosition)?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("frac".to_owned()))?;

    session.run(Command::Undo)?;
    assert!(session.command_line().is_none());
    assert_eq!(session.math_edit().map(source_text), Some(""));
    assert_eq!(inline_source(&session).as_deref(), Some(""));

    session.run(Command::Redo)?;
    assert!(session.command_line().is_none());
    assert_eq!(session.math_edit().map(source_text), Some("x"));
    assert_eq!(inline_source(&session).as_deref(), Some("x"));
    Ok(())
}

#[test]
fn undo_and_redo_restore_formula_target_and_source_cursor() -> Result<(), Box<dyn std::error::Error>>
{
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("xy".to_owned()))?;
    let after_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(source_text(&after_edit), "xy");
    assert_eq!(after_edit.cursor, 2);
    assert_eq!(after_edit.revision, session.current_math_edit_revision());

    session.run(Command::Undo)?;
    let after_undo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_undo.target, after_edit.target);
    assert_eq!(source_text(after_undo), "");
    assert_eq!(after_undo.cursor, 0);
    assert!(after_undo.source_projection.is_some());
    assert_eq!(inline_source(&session).as_deref(), Some(""));

    session.run(Command::Redo)?;
    let after_redo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_redo.target, after_edit.target);
    assert_eq!(source_text(after_redo), "xy");
    assert_eq!(after_redo.cursor, 2);
    assert_eq!(after_redo.revision, session.current_math_edit_revision());
    assert!(after_redo.source_projection.is_some());
    assert_eq!(inline_source(&session).as_deref(), Some("xy"));
    Ok(())
}

#[test]
fn stale_math_edit_revision_rejects_input_without_losing_the_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let document_before = session.document().clone();
    let revision = session.current_math_edit_revision();
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision
        .document_generation += 1;
    let stale_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let stale_revision = stale_edit.revision.clone();

    assert!(matches!(
        session.run(Command::InsertText("y".to_owned())),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), Some(&stale_edit));
    assert_eq!(
        session.math_edit().map(|edit| edit.revision.clone()),
        Some(stale_revision)
    );

    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision = revision;
    session.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("xy"));
    assert_eq!(
        session.math_edit().map(|edit| edit.revision.clone()),
        Some(session.current_math_edit_revision())
    );
    Ok(())
}

#[test]
fn stale_math_revision_keeps_pending_draft_until_explicit_escape()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::InsertText("\\".to_owned()))?;
    assert!(session.has_uncommitted_math_source_draft());

    let committed_document = session.document().clone();
    let committed_source = committed_inline_source(&session);
    let selection = session.selection().clone();
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let generation = session.document_generation();
    let can_undo = session.can_undo();
    let can_redo = session.can_redo();
    session.run(Command::BeginCommandLine)?;
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision
        .document_generation += 1;
    let stale_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;

    assert!(matches!(
        session.run(Command::InsertText("y".to_owned())),
        Err(EditError::InvalidPosition)
    ));
    assert!(matches!(
        session.run(Command::SetMathTextSelection {
            anchor: 0,
            focus: 0
        }),
        Err(EditError::InvalidPosition)
    ));
    assert!(matches!(
        session.run(Command::Undo),
        Err(EditError::InvalidPosition)
    ));
    assert!(session.begin_save_checkpoint().is_err());
    assert_eq!(session.document(), &committed_document);
    assert_eq!(committed_inline_source(&session), committed_source);
    assert_eq!(session.selection(), &selection);
    assert_eq!(session.document_generation(), generation);
    assert!(session.has_uncommitted_math_source_draft());
    assert_eq!(session.math_edit(), Some(&stale_edit));
    assert_eq!(session.can_undo(), can_undo);
    assert_eq!(session.can_redo(), can_redo);

    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.command_line().is_none());
    assert_eq!(session.math_edit(), Some(&stale_edit));
    assert!(session.has_uncommitted_math_source_draft());
    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.document(), &committed_document);
    assert_eq!(committed_inline_source(&session), committed_source);
    assert_eq!(session.selection(), &selection);
    assert_eq!(session.document_generation(), generation);
    assert!(!session.has_uncommitted_math_source_draft());
    assert_eq!(session.can_undo(), can_undo);
    assert_eq!(session.can_redo(), can_redo);
    assert!(edit.source_draft_is_pending());
    Ok(())
}

#[test]
fn stale_math_edit_can_be_abandoned_with_escape_without_changing_the_tree()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let document_before = session.document().clone();
    let selection_before = session.selection().clone();
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision
        .document_generation += 1;

    let effect = session.run(Command::EditorKey(EditorKey::Escape))?;

    assert!(effect.selection_changed);
    assert!(!effect.document_changed);
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn stale_math_edit_escape_closes_command_query_before_abandoning_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::BeginCommandLine)?;
    let document_before = session.document().clone();
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision
        .document_generation += 1;

    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.command_line().is_none());
    let stale_edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_ne!(stale_edit.revision, session.current_math_edit_revision());
    assert_eq!(session.document(), &document_before);

    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.document(), &document_before);
    Ok(())
}

#[test]
fn stale_math_edit_rejects_public_block_selection_without_losing_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut paragraph = caret_doc::Paragraph::empty(NodeId::generate());
    let paragraph_id = paragraph.id.clone();
    paragraph
        .inlines
        .push(ParagraphContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: "content".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }],
            extra: Default::default(),
        }));
    let environment_id = NodeId::generate();
    let mut document = caret_doc::Document::blank();
    document
        .body
        .push(Block::Environment(caret_doc::BlockEnvironment {
            id: environment_id.clone(),
            kind: caret_doc::BlockEnvironmentKind::Section,
            definition_id: None,
            props: Default::default(),
            title: Some(caret_doc::Paragraph::empty(NodeId::generate())),
            children: vec![Block::Paragraph(paragraph)],
            extra: Default::default(),
        }));
    let mut session = Session::new(document)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let document_before = session.document().clone();
    let selection_before = session.selection().clone();
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision
        .document_generation += 1;
    let stale_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;

    assert!(matches!(
        session.select_block_environment(environment_id),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.math_edit(), Some(&stale_edit));

    let anchor = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let focus = Pos {
        offset: "content".len(),
        affinity: Affinity::Upstream,
        ..anchor.clone()
    };
    assert!(matches!(
        session.select_block_environments_from_range(
            anchor.clone(),
            focus.clone(),
            caret_layout::VisualAffinity::Downstream,
        ),
        Err(EditError::InvalidPosition)
    ));
    assert!(matches!(
        session.set_navigation_selection(
            Selection { anchor, focus },
            caret_layout::VisualAffinity::Downstream,
        ),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert_eq!(session.math_edit(), Some(&stale_edit));
    Ok(())
}

#[test]
fn stale_math_edit_rejects_command_line_math_template_without_mutating_state()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("frac".to_owned()))?;
    let document_before = session.document().clone();
    let command_line_before = session.command_line().cloned();
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .revision
        .document_generation += 1;
    let stale_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;

    assert!(matches!(
        session.run_command_line_with_font_choices(
            Command::AcceptCommandLine,
            Vec::new(),
            Vec::new()
        ),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.command_line(), command_line_before.as_ref());
    assert_eq!(session.math_edit(), Some(&stale_edit));
    Ok(())
}

#[test]
fn stale_composition_candidate_cannot_replace_a_newer_math_edit_session()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("a")?;
    let (candidate, _, _) = session.composition_update_candidate(73, "X".to_owned(), None, 1)?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let document_before = session.document().clone();
    let math_edit_before = session.math_edit().cloned();

    assert!(matches!(
        session.install_composition_candidate(candidate),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), math_edit_before.as_ref());
    assert!(session.composition().is_none());
    Ok(())
}

#[test]
fn stale_composition_candidate_cannot_restore_an_old_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("ab")?;
    let (candidate, _, _) = session.composition_update_candidate(74, "X".to_owned(), None, 1)?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    let document_before = session.document().clone();
    let selection_before = session.selection().clone();

    assert!(matches!(
        session.install_composition_candidate(candidate),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.selection(), &selection_before);
    assert!(session.composition().is_none());
    Ok(())
}

#[test]
fn stale_composition_candidate_cannot_restore_an_old_save_checkpoint()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("ab")?;
    let (candidate, _, _) = session.composition_update_candidate(75, "X".to_owned(), None, 1)?;
    session.begin_save_checkpoint()?;
    let checkpoint_epoch = session.history_merge_epoch;

    assert!(matches!(
        session.install_composition_candidate(candidate),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.history_merge_epoch, checkpoint_epoch);
    assert!(session.composition().is_none());
    Ok(())
}

#[test]
fn restore_math_edit_reuses_matching_projection_and_rejects_mismatched_kind()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let position = history_position(&edit);

    let revision = session.current_math_edit_revision();
    let restored = restore_math_edit(&session.document, position.clone(), revision.clone())
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(restored.revision, revision);
    assert!(Arc::ptr_eq(
        restored
            .source_projection
            .as_ref()
            .ok_or(EditError::InvalidPosition)?,
        edit.source_projection
            .as_ref()
            .ok_or(EditError::InvalidPosition)?
    ));

    let mut stale_position = position;
    stale_position.source_projection = Some(Arc::new(
        caret_math::parse_projection("other", caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
            .into_parts()
            .1,
    ));
    let stale_revision = session.current_math_edit_revision();
    assert!(restore_math_edit(&session.document, stale_position, stale_revision).is_none());
    Ok(())
}

#[test]
fn restore_math_edit_rejects_mismatched_tree_and_align_locations_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    for (query, source, kind) in [
        ("math", "x+y", caret_math::MathInputKind::Inline),
        (
            "align",
            r"\begin{aligned}x+y\end{aligned}",
            caret_math::MathInputKind::Align,
        ),
    ] {
        let mut session = session_with_text("")?;
        accept(&mut session, query)?;
        enter_source_edit(&mut session, Direction::Forward)?;
        session.run(Command::InsertText("x+y".to_owned()))?;
        let edit = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        assert_eq!(source_projection(&edit).source(), source);
        let mut position = history_position(&edit);
        let document_before = session.document.clone();
        let revision = session.current_math_edit_revision();
        assert!(restore_math_edit(&session.document, position.clone(), revision.clone()).is_some());
        assert_eq!(session.document, document_before);
        let mismatched_source = source.replace("x+y", "x+z");
        let projection = caret_math::parse_projection(&mismatched_source, kind)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
            .into_parts()
            .1;

        position.source_projection = Some(Arc::new(projection));
        position.projection_identity =
            Some(Arc::as_ptr(source_position_projection(&position)) as usize);
        assert!(restore_math_edit(&session.document, position, revision).is_none());
        assert_eq!(session.document, document_before);
        assert_eq!(session.math_edit(), Some(&edit));
    }
    Ok(())
}

#[test]
fn history_restores_source_free_textflow_path_for_tree_editing() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    insert_equation_text(&mut session, r"\text{abc}")?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection {
        anchor: 1,
        focus: 2,
    })?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| edit.source_projection.is_none())
    );

    session.run(Command::InsertText("X".to_owned()))?;
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "aXc"
    );
    session.run(Command::Undo)?;
    let restored = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(restored.source_projection.is_none());
    assert!(matches!(restored.branch, MathEditBranch::Text { .. }));
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "abc"
    );

    session.run(Command::Redo)?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| edit.source_projection.is_none())
    );
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "aXc"
    );
    Ok(())
}

#[test]
fn reentering_formula_rebases_text_path_for_pending_style_undo()
-> Result<(), Box<dyn std::error::Error>> {
    let source = r"\frac {x}{\text{t}}";
    let canonical_tree =
        match caret_math::parse_projection(source, caret_math::MathInputKind::Inline)?
            .into_parts()
            .0
        {
            caret_math::ProjectedMath::Tree(tree) => tree,
            caret_math::ProjectedMath::Align(_) => return Err(EditError::InvalidPosition.into()),
        };
    let canonical_source = caret_math::serialize_tree(&canonical_tree);
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(source.to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    assert_eq!(
        session.math_edit().and_then(MathEdit::source),
        Some(canonical_source.as_str())
    );
    session.run(Command::InsertText(" ".to_owned()))?;
    let historical_source = session
        .math_edit()
        .and_then(MathEdit::source)
        .ok_or(EditError::InvalidPosition)?
        .to_owned();
    assert_ne!(historical_source, canonical_source);

    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let tree = session.math_tree_for_edit(&edit, None)?;
    let start = first_text_source_range(
        &tree.root,
        source_projection(&edit)
            .tree()
            .ok_or(EditError::InvalidPosition)?
            .root(),
    )
    .ok_or(EditError::InvalidPosition)?
    .start;
    let source_base = source_projection(&edit).edit_range().start;
    let path = path::path_at_source_boundary(
        source_projection(&edit)
            .tree()
            .ok_or(EditError::InvalidPosition)?,
        source_base + start,
        true,
        None,
    )
    .ok_or(EditError::InvalidPosition)?;
    session.enter_math_text_branch(path, 0);
    session.run(Command::ToggleRunUnderline)?;

    session.exit_math_edit(Direction::Forward, true)?;
    enter_source_edit(&mut session, Direction::Backward)?;
    assert_eq!(
        session.math_edit().and_then(MathEdit::source),
        Some(canonical_source.as_str())
    );
    session.run(Command::Undo)?;
    let restored = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert!(matches!(restored.branch, MathEditBranch::Text { .. }));
    assert!(restored.source_projection.is_none());
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "t"
    );
    assert!(
        session
            .pending_inline_style
            .active_run_properties()
            .next()
            .is_none()
    );
    session.run(Command::InsertText("z".to_owned()))?;
    assert!(inline_source(&session).is_some_and(|source| source.contains('z')));
    session.run(Command::Delete(Direction::Backward))?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| edit.source_projection.is_none())
    );
    assert_eq!(
        active_math_text_flow(&session)?
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "t"
    );
    Ok(())
}

#[test]
fn undoing_document_settings_restores_source_branch_ranges_for_text_navigation()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(
        r"\frac{x}{\text{abcd}}".to_owned(),
    ))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::InsertText(" ".to_owned()))?;
    let historical_source = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    assert!(historical_source.ends_with(' '));

    session.run(Command::SetPageGeometry(caret_doc::PageGeometry {
        paper_size: caret_doc::PaperSize::A3,
        orientation: caret_doc::Orientation::Landscape,
        margins: caret_doc::PageMargins {
            top: 36.0,
            right: 54.0,
            bottom: 72.0,
            left: 90.0,
        },
    }))?;
    session.exit_math_edit(Direction::Forward, true)?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert_ne!(
        inline_source(&session).as_deref(),
        Some(historical_source.as_str())
    );

    session.run(Command::Undo)?;
    let restored_source = inline_source(&session).ok_or(EditError::InvalidPosition)?;
    assert!(restored_source.contains(r"\text{abcd}"));
    for _ in 0..64 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            break;
        }
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert!(matches!(
        session.math_edit().map(|edit| &edit.branch),
        Some(MathEditBranch::Text { .. })
    ));
    session.run(Command::InsertText("z".to_owned()))?;
    assert!(
        inline_source(&session)
            .as_deref()
            .is_some_and(|source| source.contains('z'))
    );
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(
        inline_source(&session).as_deref(),
        Some(r"\frac{x}{\text{abcd}}")
    );
    Ok(())
}

#[test]
fn failed_math_history_restore_rolls_back_document_patch_and_keeps_history_position()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some(""));
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .cursor = usize::MAX;
    session.run(Command::Delete(Direction::Backward))?;
    assert!(session.math_edit().is_none());
    let document_before_failed_undo = session.document().clone();
    let selection_before_failed_undo = session.selection().clone();

    assert!(matches!(
        session.run(Command::Undo),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document(), &document_before_failed_undo);
    assert_eq!(session.selection(), &selection_before_failed_undo);
    assert!(session.math_edit().is_none());
    assert!(session.edit_history.can_undo());
    Ok(())
}

#[test]
fn undo_rejects_a_stale_math_source_map_and_restores_the_history_transaction()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x+y".to_owned()))?;
    enter_source_edit(&mut session, Direction::Backward)?;
    let edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    let stale_source = source_projection(&edit).source().replace("x+y", "x+z");
    let source_projection =
        caret_math::parse_projection(&stale_source, caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
            .into_parts()
            .1;
    session
        .math_edit
        .as_mut()
        .ok_or(EditError::InvalidPosition)?
        .source_projection = Some(Arc::new(source_projection));

    session.run(Command::SetPageGeometry(caret_doc::PageGeometry {
        paper_size: caret_doc::PaperSize::A3,
        orientation: caret_doc::Orientation::Landscape,
        margins: caret_doc::PageMargins {
            top: 36.0,
            right: 54.0,
            bottom: 72.0,
            left: 90.0,
        },
    }))?;
    let document_before_failed_undo = session.document.clone();
    let selection_before_failed_undo = session.selection.clone();
    let edit_before_failed_undo = session.math_edit().cloned();
    let generation_before_failed_undo = session.document_generation;
    let history_state_before_failed_undo = session.history_state_id;
    assert!(session.can_undo());

    assert!(matches!(
        session.run(Command::Undo),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document, document_before_failed_undo);
    assert_eq!(session.selection, selection_before_failed_undo);
    assert_eq!(session.math_edit(), edit_before_failed_undo.as_ref());
    assert_eq!(session.document_generation, generation_before_failed_undo);
    assert_eq!(session.history_state_id, history_state_before_failed_undo);
    assert!(session.can_undo());
    assert!(!session.can_redo());
    Ok(())
}

#[test]
fn redo_rejects_a_missing_math_edit_target_without_consuming_history()
-> Result<(), Box<dyn std::error::Error>> {
    let geometry = caret_doc::PageGeometry {
        paper_size: caret_doc::PaperSize::A3,
        orientation: caret_doc::Orientation::Landscape,
        margins: caret_doc::PageMargins {
            top: 36.0,
            right: 54.0,
            bottom: 72.0,
            left: 90.0,
        },
    };
    let mut valid_session = session_with_text("")?;
    valid_session.run(Command::InsertInlineMath("x+y".to_owned()))?;
    valid_session.run(Command::EnterMath(Direction::Backward))?;
    valid_session.run(Command::SetPageGeometry(geometry))?;
    valid_session.run(Command::Undo)?;
    valid_session.run(Command::Redo)?;
    assert!(valid_session.can_undo());
    assert!(!valid_session.can_redo());

    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("x+y".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::SetPageGeometry(geometry))?;
    session.run(Command::Undo)?;
    assert!(session.can_redo());

    let MathEditTarget::Inline { math_id, .. } = session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .target
        .clone()
    else {
        return Err(EditError::InvalidPosition.into());
    };
    let Block::Paragraph(paragraph) = session
        .document
        .body
        .first_mut()
        .ok_or(EditError::InvalidPosition)?
    else {
        return Err(EditError::InvalidPosition.into());
    };
    // 故障注入已失效的公式身份，检查重放失败不会消费 redo 事务。
    find_inline_math_mut(&mut paragraph.inlines, &math_id)
        .ok_or(EditError::InvalidPosition)?
        .id = NodeId::generate();

    let document_before_failed_redo = session.document.clone();
    let selection_before_failed_redo = session.selection.clone();
    let edit_before_failed_redo = session.math_edit().cloned();
    let revision_before_failed_redo = edit_before_failed_redo
        .as_ref()
        .map(|edit| edit.revision.clone());
    let generation_before_failed_redo = session.document_generation;
    let history_state_before_failed_redo = session.history_state_id;

    assert!(matches!(
        session.run(Command::Redo),
        Err(EditError::InvalidPosition)
    ));
    assert_eq!(session.document, document_before_failed_redo);
    assert_eq!(session.selection, selection_before_failed_redo);
    assert_eq!(session.math_edit(), edit_before_failed_redo.as_ref());
    assert_eq!(
        session.math_edit().map(|edit| edit.revision.clone()),
        revision_before_failed_redo
    );
    assert_eq!(session.document_generation, generation_before_failed_redo);
    assert_eq!(session.history_state_id, history_state_before_failed_redo);
    assert!(session.can_undo());
    assert!(session.can_redo());
    Ok(())
}

#[test]
fn reopening_a_canonical_projection_does_not_break_formula_creation_undo()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("a + b".to_owned()))?;
    assert!(session.math_edit().is_none());
    enter_source_edit(&mut session, Direction::Backward)?;
    assert_eq!(session.math_edit().map(source_text), Some("a+b"));

    session.run(Command::Undo)?;
    assert!(session.math_edit().is_none());
    assert!(session.document().body.iter().all(|block| {
        !matches!(block, Block::Paragraph(paragraph) if paragraph.inlines.iter().any(|node| matches!(node, ParagraphContent::Math(_))))
    }));
    session.run(Command::Redo)?;
    assert!(session.document().body.iter().any(|block| {
        matches!(block, Block::Paragraph(paragraph) if paragraph.inlines.iter().any(|node| matches!(node, ParagraphContent::Math(_))))
    }));
    Ok(())
}

#[test]
fn tree_native_text_edit_keeps_empty_textflow_without_source_anchors() -> Result<(), EditError> {
    for source in [r"\text{a}\text{}", r"\text{a}\text{}\text{b}"] {
        let (content, _) = caret_math::parse_projection(source, caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
            .into_parts();
        let caret_math::ProjectedMath::Tree(tree) = content else {
            return Err(EditError::InvalidPosition);
        };
        let empty_index = tree
            .root
            .content
            .iter()
            .position(|content| {
                matches!(content, MathContent::Text(text) if text.runs.iter().all(|run| run.text.is_empty()))
            })
            .ok_or(EditError::InvalidPosition)?;

        let (source_map, refreshed_tree) =
            crate::math_validation::project_tree_for_edit(&tree, caret_math::MathInputKind::Inline)
                .map_err(|_| EditError::InvalidPosition)?;
        let Some(MathContent::Text(empty)) = refreshed_tree.root.content.get(empty_index) else {
            return Err(EditError::InvalidPosition);
        };
        let text_map = source_map
            .tree()
            .and_then(|map| map.content_at(&[caret_doc::MathPathStep::Content(empty_index)]))
            .and_then(caret_math::MathContentSourceMap::text)
            .ok_or(EditError::InvalidPosition)?;
        let source_range = text_map.range().ok_or(EditError::InvalidPosition)?;
        assert_eq!(source_range.start, source_range.end);
        assert!(
            text_map
                .segments()
                .iter()
                .any(|segment| segment.display().is_empty()
                    && segment.source() == Some(source_range.clone()))
        );
        assert_eq!(
            text_source_boundary_from_map(empty, text_map, 0, 0, Affinity::Downstream,),
            Some(source_range.start)
        );

        let mut document = Document::blank();
        document.body = vec![
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
            Block::Equation(Equation {
                id: NodeId::generate(),
                math_tree: refreshed_tree,
                extra: BTreeMap::new(),
            }),
        ];
        let mut session = Session::new(document)?;
        session.run(Command::EnterMath(Direction::Forward))?;
        let path = MathTextFlowPath::new(vec![MathTextFlowPathStep::Content(empty_index)], None);
        session.enter_math_text_branch(path, 0);
        session.run(Command::SetMathTextSelection {
            anchor: 0,
            focus: 0,
        })?;
        session.run(Command::InsertText("x".to_owned()))?;
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert!(edit.source_projection.is_none());
        assert!(matches!(edit.branch, MathEditBranch::Text { .. }));
        let text = active_math_text_flow(&session)?;
        assert_eq!(
            text.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "x"
        );
    }
    Ok(())
}

#[test]
fn undo_and_redo_restore_equation_and_align_edit_targets() -> Result<(), Box<dyn std::error::Error>>
{
    for (query, align) in [("equation", false), ("align", true)] {
        let mut session = session_with_text("")?;
        accept(&mut session, query)?;
        let before_edit = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        assert!(before_edit.source_projection.is_none());
        assert!(matches!(before_edit.branch, MathEditBranch::Tree { .. }));
        session
            .run(Command::InsertText("x".to_owned()))
            .map_err(|error| format!("{query} insert failed: {error:?}"))?;
        let after_edit = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        assert!(matches!(
            after_edit.target,
            MathEditTarget::Block { align: value, .. } if value == align
        ));
        assert!(after_edit.source_projection.is_none());
        assert!(matches!(after_edit.branch, MathEditBranch::Tree { .. }));

        session
            .run(Command::Undo)
            .map_err(|error| format!("{query} undo failed: {error:?}"))?;
        let after_undo = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert_eq!(after_undo.target, after_edit.target);
        assert_eq!(after_undo.cursor, 0);
        assert!(after_undo.source_projection.is_none());
        assert!(matches!(after_undo.branch, MathEditBranch::Tree { .. }));

        session.run(Command::Redo)?;
        let after_redo = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert_eq!(after_redo.target, after_edit.target);
        assert!(after_redo.source_projection.is_none());
        assert!(matches!(after_redo.branch, MathEditBranch::Tree { .. }));
        assert!(inline_source(&session).is_some_and(|source| source.contains('x')));
        assert_eq!(after_redo.cursor, 1);
    }
    Ok(())
}

#[test]
fn undo_and_redo_restore_align_cursor_in_unicode_content_coordinates()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    let source = "αabcdefghijklmn";
    session.run(Command::InsertText(source.to_owned()))?;
    assert_eq!(session.math_edit().map(source_text), Some(source));
    assert_eq!(
        session.math_edit().map(|edit| edit.cursor),
        Some(source.len())
    );

    session.run(Command::Undo)?;
    let after_undo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(source_text(after_undo), "");
    assert_eq!(after_undo.cursor, 0);

    session.run(Command::Redo)?;
    let after_redo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(source_text(after_redo), source);
    assert_eq!(after_redo.cursor, source.len());
    assert!(source_projection(after_redo).align().is_some());
    Ok(())
}

#[test]
fn redo_formula_creation_reopens_the_same_edit_target() -> Result<(), Box<dyn std::error::Error>> {
    for query in ["math", "equation", "align"] {
        let mut session = session_with_text("")?;
        accept(&mut session, query)?;
        let created = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        assert!(created.source_projection.is_none());
        assert!(matches!(created.branch, MathEditBranch::Tree { .. }));

        session.run(Command::Undo)?;
        assert!(session.math_edit().is_none());
        session.run(Command::Redo)?;
        let restored = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert_eq!(restored.target, created.target);
        assert!(restored.source_projection.is_none());
        assert!(matches!(restored.branch, MathEditBranch::Tree { .. }));
        assert_eq!(restored.cursor, created.cursor);
    }
    Ok(())
}

#[test]
fn reentering_inline_math_uses_canonical_source_and_keeps_host_side()
-> Result<(), Box<dyn std::error::Error>> {
    let mut from_left = session_with_text("")?;
    from_left.run(Command::InsertInlineMath("$x$".to_owned()))?;
    let paragraph_id = from_left.selection().focus.paragraph_id.clone();
    let left = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    from_left.run(Command::SetSelection(Some(Selection {
        anchor: left.clone(),
        focus: left,
    })))?;
    from_left.run(Command::EnterMath(Direction::Forward))?;
    assert_eq!(from_left.math_edit().map(|edit| edit.cursor), Some(0));
    from_left.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(inline_source(&from_left).as_deref(), Some("yx"));

    let mut from_right = session_with_text("")?;
    from_right.run(Command::InsertInlineMath(r"\(x\)".to_owned()))?;
    from_right.run(Command::EnterMath(Direction::Backward))?;
    assert_eq!(from_right.math_edit().map(|edit| edit.cursor), Some(1));
    from_right.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(inline_source(&from_right).as_deref(), Some("xy"));
    from_right.run(Command::MoveCaret(Direction::Forward))?;
    assert!(from_right.math_edit().is_none());

    let mut from_left = session_with_text("")?;
    from_left.run(Command::InsertInlineMath(r"\(x\)".to_owned()))?;
    let paragraph_id = from_left.selection().focus.paragraph_id.clone();
    let left = Pos {
        story: StoryId::Body,
        paragraph_id,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    from_left.run(Command::SetSelection(Some(Selection {
        anchor: left.clone(),
        focus: left,
    })))?;
    from_left.run(Command::EnterMath(Direction::Forward))?;
    assert_eq!(from_left.math_edit().map(|edit| edit.cursor), Some(0));
    from_left.run(Command::MoveCaret(Direction::Backward))?;
    assert!(from_left.math_edit().is_none());
    Ok(())
}

#[test]
fn inline_math_navigation_ignores_text_flow_extension_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let caret_math::ParsedMath::Tree(math_tree) =
        caret_math::parse("x", caret_math::MathInputKind::Inline)?
    else {
        panic!("inline math should parse into a tree");
    };
    let mut paragraph = Paragraph::empty(NodeId::generate());
    let text = |value: &str| {
        let mut extra = std::collections::BTreeMap::new();
        extra.insert("futureTextFlowField".to_owned(), serde_json::json!(true));
        ParagraphContent::Text(caret_doc::TextFlow {
            runs: vec![Run {
                text: value.to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }],
            extra,
        })
    };
    paragraph.inlines = vec![
        text("before"),
        ParagraphContent::Math(InlineMath {
            id: NodeId::generate(),
            math_tree,
            extra: Default::default(),
        }),
        text("after"),
    ];
    let paragraph_id = paragraph.id.clone();
    let mut document = caret_doc::Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session = Session::new(document)?;
    let before_math = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: "before".len(),
        affinity: Affinity::Downstream,
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: before_math.clone(),
        focus: before_math,
    })))?;

    session.run(Command::EnterMath(Direction::Forward))?;
    assert!(session.math_edit().is_some());
    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.offset, "before".len() + 3);
    Ok(())
}

#[test]
fn backspace_deletes_an_empty_fenced_inline_formula() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("$ $".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    let before_delete_affinity = session.selection_visual_affinity();
    assert_eq!(before_delete_affinity, crate::VisualAffinity::Upstream);
    session.run(Command::Delete(Direction::Backward))?;
    let after_delete_affinity = session.selection_visual_affinity();
    assert_eq!(after_delete_affinity, crate::VisualAffinity::Downstream);

    assert!(inline_source(&session).is_none());
    assert!(session.math_edit().is_none());

    session.run(Command::Undo)?;
    assert!(inline_source(&session).is_some());
    assert!(session.math_edit().is_some());
    assert_eq!(session.selection_visual_affinity(), before_delete_affinity);
    session.run(Command::Redo)?;
    assert!(inline_source(&session).is_none());
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection_visual_affinity(), after_delete_affinity);
    Ok(())
}

#[test]
fn undo_and_redo_restore_focus_when_deleting_an_empty_display_formula()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "equation")?;
    assert!(session.math_edit().is_some());
    let before_delete_affinity = session.selection_visual_affinity();
    assert_eq!(before_delete_affinity, crate::VisualAffinity::Downstream);
    session.run(Command::Delete(Direction::Backward))?;
    let after_delete_affinity = session.selection_visual_affinity();
    assert_eq!(after_delete_affinity, crate::VisualAffinity::Upstream);
    assert!(session.math_edit().is_none());
    assert!(
        !session
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Equation(_)))
    );

    session.run(Command::Undo)?;
    assert!(session.math_edit().is_some());
    assert_eq!(session.selection_visual_affinity(), before_delete_affinity);
    assert!(
        session
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Equation(_)))
    );
    session.run(Command::Redo)?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection_visual_affinity(), after_delete_affinity);
    assert!(
        !session
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Equation(_)))
    );
    Ok(())
}

#[test]
fn rejecting_missing_block_selection_keeps_active_math_edit() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    accept(&mut session, "math")?;
    let before = session.math_edit().cloned();
    let generation = session.document_generation();

    assert_eq!(
        session.select_block_environment(NodeId::generate()),
        Err(EditError::InvalidPosition)
    );
    assert_eq!(session.math_edit(), before.as_ref());
    assert_eq!(session.document_generation(), generation);
    Ok(())
}

#[test]
fn incomplete_math_source_is_retained_and_has_local_undo_redo() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    let committed_document = session.document().clone();
    let committed_generation = session.document_generation();

    session.run(Command::InsertText("\\".to_owned()))?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x\\"));
    assert_eq!(session.document(), &committed_document);
    assert_eq!(session.document_generation(), committed_generation);
    assert!(session.can_undo());

    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x"));
    assert_eq!(session.document(), &committed_document);
    assert_eq!(session.document_generation(), committed_generation);
    assert!(session.can_redo());

    session.run(Command::Redo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x\\"));
    session.run(Command::InsertText("alpha".to_owned()))?;
    assert_eq!(
        session.math_edit().and_then(MathEdit::source),
        Some(r"x\alpha")
    );
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| !edit.source_draft_is_pending())
    );
    assert_ne!(session.document(), &committed_document);

    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x"));
    assert_eq!(session.document(), &committed_document);
    Ok(())
}

#[test]
fn incomplete_math_source_blocks_exit_until_repaired_or_cancelled() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::InsertText("\\".to_owned()))?;
    let document = session.document().clone();
    let edit = session.math_edit().cloned();

    assert!(matches!(
        session.run(Command::EditorKey(EditorKey::Enter)),
        Err(EditError::InvalidMath(_))
    ));
    assert!(matches!(
        session.run(Command::EditorKey(EditorKey::ModBackspace)),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &document);
    assert_eq!(session.math_edit(), edit.as_ref());

    session.run(Command::EditorKey(EditorKey::Escape))?;
    assert!(session.math_edit().is_none());
    assert_eq!(session.document(), &document);
    Ok(())
}

#[test]
fn incomplete_math_draft_blocks_document_settings_without_invalidating_its_revision()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::InsertText("\\".to_owned()))?;
    let document = session.document().clone();
    let generation = session.document_generation();
    let source = session
        .math_edit()
        .and_then(MathEdit::source)
        .map(str::to_owned);

    for command in [
        Command::SetPageGeometry(caret_doc::PageGeometry {
            paper_size: caret_doc::PaperSize::A3,
            orientation: caret_doc::Orientation::Landscape,
            margins: caret_doc::PageMargins {
                top: 36.0,
                right: 54.0,
                bottom: 72.0,
                left: 90.0,
            },
        }),
        Command::SetDefaultFontFamily(Some("Noto Sans".to_owned())),
        Command::SetDocumentFontFamily {
            target: caret_doc::DocumentFontTarget::Script("Latn".to_owned()),
            family: Some("Noto Sans".to_owned()),
        },
    ] {
        assert!(matches!(
            session.run(command),
            Err(EditError::InvalidMath(_))
        ));
        assert_eq!(session.document(), &document);
        assert_eq!(session.document_generation(), generation);
        assert_eq!(
            session.math_edit().and_then(MathEdit::source),
            source.as_deref()
        );
    }

    session.run(Command::InsertText("alpha".to_owned()))?;
    assert_eq!(
        session.math_edit().and_then(MathEdit::source),
        Some(r"x\alpha")
    );
    Ok(())
}

#[test]
fn incomplete_math_fence_can_be_removed_with_backspace() -> Result<(), EditError> {
    for fence in ["$", "\\("] {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        accept(&mut session, "math")?;
        enter_source_edit(&mut session, Direction::Forward)?;
        session.run(Command::InsertText(fence.to_owned()))?;
        assert_eq!(session.math_edit().and_then(MathEdit::source), Some(fence));
        assert!(session.has_uncommitted_math_source_draft());

        while session.has_uncommitted_math_source_draft() {
            session.run(Command::Delete(Direction::Backward))?;
        }
        assert_eq!(session.math_edit().and_then(MathEdit::source), Some(""));
    }
    Ok(())
}

#[test]
fn draft_undo_keeps_auto_paired_closing_braces() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("^".to_owned()))?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("^{}"));

    session.run(Command::InsertText("}\\".to_owned()))?;
    assert_eq!(
        session.math_edit().and_then(MathEdit::source),
        Some("^{}\\")
    );
    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("^{}"));
    session.run(Command::Redo)?;
    assert_eq!(
        session.math_edit().and_then(MathEdit::source),
        Some("^{}\\")
    );
    Ok(())
}

#[test]
fn incomplete_equation_and_align_source_is_kept_separate_from_the_committed_tree()
-> Result<(), EditError> {
    for kind in ["equation", "align"] {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        accept(&mut session, kind)?;
        enter_source_edit(&mut session, Direction::Forward)?;
        session.run(Command::InsertText("x".to_owned()))?;
        let committed = session.document().clone();

        session.run(Command::InsertText("\\".to_owned()))?;
        assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x\\"));
        assert_eq!(session.document(), &committed);
        assert!(session.has_uncommitted_math_source_draft());

        session.run(Command::Undo)?;
        assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x"));
        assert_eq!(session.document(), &committed);
        session.run(Command::Redo)?;
        assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x\\"));
    }
    Ok(())
}

#[test]
fn crossing_from_math_draft_history_to_document_history_discards_stale_redo()
-> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::InsertText("\\".to_owned()))?;

    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x"));
    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some(""));
    session.run(Command::Redo)?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x"));
    assert!(
        !session
            .math_edit()
            .is_some_and(|edit| edit.source_draft_is_pending())
    );
    Ok(())
}

#[test]
fn backspace_removes_empty_construct_and_then_the_object() -> Result<(), Box<dyn std::error::Error>>
{
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("sqrt".to_owned()))?;
    session.run(Command::AcceptCommandLine)?;
    assert_eq!(inline_source(&session).as_deref(), Some("\\sqrt{}"));
    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(inline_source(&session).as_deref(), Some(""));
    session.run(Command::Delete(Direction::Backward))?;
    assert!(session.math_edit().is_none());
    assert_eq!(inline_source(&session), None);
    Ok(())
}

#[test]
fn align_command_template_backspace_removes_empty_construct()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    let block_id = match &session
        .math_edit()
        .ok_or(EditError::InvalidPosition)?
        .target
    {
        MathEditTarget::Block {
            block_id,
            align: true,
        } => block_id.clone(),
        _ => return Err(EditError::InvalidPosition.into()),
    };
    session.run(Command::InsertText("x& ".to_owned()))?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("sqrt".to_owned()))?;
    session.run(Command::AcceptCommandLine)?;

    let inserted = session
        .math_edit()
        .map(source_text)
        .ok_or(EditError::InvalidPosition)?;
    assert!(inserted.contains(r"\sqrt{}"));
    assert!(inserted.starts_with("x& "));
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    assert!(!session.has_uncommitted_math_source_draft());
    let before_delete_generation = session.document_generation();
    let deletion = session.run(Command::Delete(Direction::Backward))?;
    assert!(deletion.document_changed);
    assert!(session.document_generation() > before_delete_generation);
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    let after_delete = session
        .math_edit()
        .map(source_text)
        .ok_or(EditError::InvalidPosition)?;
    assert!(!after_delete.contains(r"\sqrt"));
    assert!(after_delete.starts_with("x& "));
    let deleted_align = first_align(&session)?;
    assert!(!caret_math::serialize_tree(&deleted_align.rows[0].cells[1]).contains(r"\sqrt"));
    session.run(Command::Undo)?;
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| source_text(edit).contains(r"\sqrt{}"))
    );
    let restored_align = first_align(&session)?;
    assert!(caret_math::serialize_tree(&restored_align.rows[0].cells[1]).contains(r"\sqrt"));
    session.run(Command::Redo)?;
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    assert!(
        session
            .math_edit()
            .is_some_and(|edit| !source_text(edit).contains(r"\sqrt"))
    );
    let redone_align = first_align(&session)?;
    assert!(!caret_math::serialize_tree(&redone_align.rows[0].cells[1]).contains(r"\sqrt"));
    Ok(())
}

#[test]
fn backspace_removes_empty_construct_in_align_cell_and_history_restores_it()
-> Result<(), Box<dyn std::error::Error>> {
    let parsed = caret_math::parse_projection(
        "\\begin{aligned}x& % before\n \\sqrt{} % after\n\\end{aligned}",
        caret_math::MathInputKind::Align,
    )?;
    let (caret_math::ProjectedMath::Align(mut align), projection) = parsed.into_parts() else {
        return Err("Align source should produce an Align tree".into());
    };
    align
        .extra
        .insert("vendorAlign".to_owned(), serde_json::json!({"kept": true}));
    align.rows[0]
        .extra
        .insert("vendorRow".to_owned(), serde_json::json!("kept"));
    align.rows[0].cells[1]
        .extra
        .insert("vendorCell".to_owned(), serde_json::json!("kept"));
    align.rows[0].cells[1]
        .root
        .extra
        .insert("vendorFlow".to_owned(), serde_json::json!("kept"));
    let block_id = align.id.clone();
    let first_cell_id = align.rows[0].cells[0].root.id.clone();
    let second_cell_id = align.rows[0].cells[1].root.id.clone();
    let edit_range = projection.edit_range();
    let cell_range = projection
        .align()
        .and_then(|map| map.cell_range(0, 1))
        .ok_or(EditError::InvalidPosition)?;
    let source_base = cell_range
        .start
        .checked_sub(edit_range.start)
        .ok_or(EditError::InvalidPosition)?;
    let cursor = source_base + r"\sqrt{".len();

    let mut document = Document::blank();
    document.body.push(Block::Align(align));
    let mut session = Session::new(document)?;
    assert_eq!(
        first_align(&session)?.rows[0].cells[0].root.id,
        first_cell_id
    );
    assert_eq!(
        first_align(&session)?.rows[0].cells[1].root.id,
        second_cell_id
    );
    session.math_edit = Some(MathEdit {
        target: MathEditTarget::Block {
            block_id: block_id.clone(),
            align: true,
        },
        source_projection: Some(Arc::new(projection)),
        source_draft: None,
        revision: MathEditRevision {
            session_id: session.session_id.clone(),
            document_generation: session.document_generation,
        },
        cursor,
        branch: MathEditBranch::Source,
    });

    assert_eq!(
        session
            .math_edit()
            .and_then(MathEdit::align_cell_at_cursor)
            .map(|(row, cell, _)| (row, cell)),
        Some((0, 1))
    );
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    let generation_before_delete = session.document_generation();
    let deletion = session.run(Command::Delete(Direction::Backward))?;
    assert!(deletion.document_changed);
    assert!(session.document_generation() > generation_before_delete);
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    let deleted_align = first_align(&session)?;
    assert_eq!(deleted_align.rows[0].cells[0].root.id, first_cell_id);
    assert_eq!(deleted_align.rows[0].cells[1].root.id, second_cell_id);
    assert_eq!(
        deleted_align.extra["vendorAlign"],
        serde_json::json!({"kept": true})
    );
    assert_eq!(deleted_align.rows[0].extra["vendorRow"], "kept");
    assert_eq!(deleted_align.rows[0].cells[1].extra["vendorCell"], "kept");
    assert_eq!(
        deleted_align.rows[0].cells[1].root.extra["vendorFlow"],
        "kept"
    );
    assert!(!caret_math::serialize_tree(&deleted_align.rows[0].cells[1]).contains(r"\sqrt"));
    let after_delete = session
        .math_edit()
        .map(source_text)
        .ok_or(EditError::InvalidPosition)?;
    assert!(!after_delete.contains(r"\sqrt"));
    assert!(after_delete.contains("% before"));
    assert!(after_delete.contains("% after"));
    session.run(Command::Undo)?;
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    let restored_align = first_align(&session)?;
    assert_eq!(restored_align.rows[0].cells[0].root.id, first_cell_id);
    assert_eq!(restored_align.rows[0].cells[1].root.id, second_cell_id);
    assert_eq!(
        restored_align.extra["vendorAlign"],
        serde_json::json!({"kept": true})
    );
    assert_eq!(restored_align.rows[0].extra["vendorRow"], "kept");
    assert_eq!(restored_align.rows[0].cells[1].extra["vendorCell"], "kept");
    assert_eq!(
        restored_align.rows[0].cells[1].root.extra["vendorFlow"],
        "kept"
    );
    assert!(caret_math::serialize_tree(&restored_align.rows[0].cells[1]).contains(r"\sqrt"));
    assert_eq!(
        session.math_edit().map(source_text),
        Some("x& % before\n \\sqrt{} % after\n")
    );
    session.run(Command::Redo)?;
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 0, 1)?;
    let redone_align = first_align(&session)?;
    assert_eq!(redone_align.rows[0].cells[0].root.id, first_cell_id);
    assert_eq!(redone_align.rows[0].cells[1].root.id, second_cell_id);
    assert_eq!(
        redone_align.extra["vendorAlign"],
        serde_json::json!({"kept": true})
    );
    assert_eq!(redone_align.rows[0].extra["vendorRow"], "kept");
    assert_eq!(redone_align.rows[0].cells[1].extra["vendorCell"], "kept");
    assert_eq!(
        redone_align.rows[0].cells[1].root.extra["vendorFlow"],
        "kept"
    );
    assert!(!caret_math::serialize_tree(&redone_align.rows[0].cells[1]).contains(r"\sqrt"));
    let after_redo = session
        .math_edit()
        .map(source_text)
        .ok_or(EditError::InvalidPosition)?;
    assert!(!after_redo.contains(r"\sqrt"));
    Ok(())
}

#[test]
fn deleting_the_last_align_cell_content_preserves_the_trailing_row_through_history()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "\\begin{aligned}a\\\\ % before\n \\sqrt{} % after\n\\end{aligned}";
    let parsed = caret_math::parse_projection(source, caret_math::MathInputKind::Align)?;
    let (caret_math::ProjectedMath::Align(mut align), projection) = parsed.into_parts() else {
        return Err("Align source should produce an Align tree".into());
    };
    assert_eq!(align.rows.len(), 2);
    let block_id = align.id.clone();
    let row_id = align.rows[1].id.clone();
    let cell_id = align.rows[1].cells[0].root.id.clone();
    align.rows[1]
        .extra
        .insert("vendorRow".to_owned(), serde_json::json!("keep row"));
    align.rows[1].cells[0]
        .extra
        .insert("vendorCell".to_owned(), serde_json::json!("keep cell"));
    let edit_range = projection.edit_range();
    let cell_range = projection
        .align()
        .and_then(|map| map.cell_range(1, 0))
        .ok_or(EditError::InvalidPosition)?;
    let source_base = cell_range
        .start
        .checked_sub(edit_range.start)
        .ok_or(EditError::InvalidPosition)?;
    let cursor = source_base + r"\sqrt{".len();
    let mut document = Document::blank();
    document.body.push(Block::Align(align));
    let mut session = Session::new(document)?;
    session.math_edit = Some(MathEdit {
        target: MathEditTarget::Block {
            block_id: block_id.clone(),
            align: true,
        },
        source_projection: Some(Arc::new(projection)),
        source_draft: None,
        revision: MathEditRevision {
            session_id: session.session_id.clone(),
            document_generation: session.document_generation,
        },
        cursor,
        branch: MathEditBranch::Source,
    });

    let deletion = session.run(Command::Delete(Direction::Backward))?;
    assert!(deletion.document_changed);
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 1, 0)?;
    let deleted = first_align(&session)?;
    assert_eq!(deleted.rows.len(), 2);
    assert_eq!(deleted.rows[1].id, row_id);
    assert_eq!(deleted.rows[1].extra["vendorRow"], "keep row");
    assert_eq!(deleted.rows[1].cells[0].root.id, cell_id);
    assert_eq!(deleted.rows[1].cells[0].extra["vendorCell"], "keep cell");
    assert!(!caret_math::serialize_tree(&deleted.rows[1].cells[0]).contains(r"\sqrt"));

    session.run(Command::Undo)?;
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 1, 0)?;
    let restored = first_align(&session)?;
    assert_eq!(restored.rows.len(), 2);
    assert_eq!(restored.rows[1].id, row_id);
    assert_eq!(restored.rows[1].cells[0].root.id, cell_id);
    assert!(caret_math::serialize_tree(&restored.rows[1].cells[0]).contains(r"\sqrt"));

    session.run(Command::Redo)?;
    assert!(!session.has_uncommitted_math_source_draft());
    assert_align_edit_target(&session, &block_id, 1, 0)?;
    assert_eq!(first_align(&session)?.rows.len(), 2);
    Ok(())
}

#[test]
fn backspace_in_empty_fraction_argument_keeps_nonempty_sibling_recoverable()
-> Result<(), Box<dyn std::error::Error>> {
    let parsed = caret_math::parse_projection(r"\frac{a}{}", caret_math::MathInputKind::Inline)?;
    let (caret_math::ProjectedMath::Tree(tree), projection) = parsed.into_parts() else {
        return Err("inline source should produce a MathTree".into());
    };
    let mut document = Document::blank();
    let Some(Block::Paragraph(paragraph)) = document.body.first_mut() else {
        return Err("blank document should contain a paragraph".into());
    };
    let paragraph_id = paragraph.id.clone();
    let math_id = NodeId::generate();
    paragraph.inlines = vec![ParagraphContent::Math(InlineMath {
        id: math_id.clone(),
        math_tree: tree,
        extra: Default::default(),
    })];
    let mut session = Session::new(document)?;
    session.math_edit = Some(MathEdit {
        target: MathEditTarget::Inline {
            paragraph_id,
            math_id,
        },
        source_projection: Some(Arc::new(projection)),
        source_draft: None,
        revision: MathEditRevision {
            session_id: session.session_id.clone(),
            document_generation: session.document_generation,
        },
        cursor: r"\frac{a}{".len(),
        branch: MathEditBranch::Source,
    });
    let before_document = session.document.clone();
    let before_generation = session.document_generation;

    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(session.math_edit().map(source_text), Some(r"\frac{a}"));
    assert!(session.has_uncommitted_math_source_draft());
    assert_eq!(session.document, before_document);
    assert_eq!(session.document_generation, before_generation);
    session.run(Command::Undo)?;
    assert_eq!(session.math_edit().map(source_text), Some(r"\frac{a}{}"));
    assert!(!session.has_uncommitted_math_source_draft());
    session.run(Command::Redo)?;
    assert_eq!(session.math_edit().map(source_text), Some(r"\frac{a}"));
    assert_eq!(session.document, before_document);
    assert_eq!(session.document_generation, before_generation);
    Ok(())
}

#[test]
fn greek_sub_commands_are_case_sensitive() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    accept(&mut session, "Gamma")?;
    accept(&mut session, "gamma")?;
    assert_eq!(inline_source(&session).as_deref(), Some("\\Gamma\\gamma"));
    Ok(())
}

#[test]
fn repeated_control_word_commands_do_not_consume_tree_depth()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    for _ in 0..=caret_doc::MAX_MATH_TREE_DEPTH {
        accept(&mut session, "alpha")?;
    }
    session.document().validate()?;
    assert_eq!(
        inline_source(&session)
            .as_deref()
            .map(|source| source.matches(r"\alpha").count()),
        Some(caret_doc::MAX_MATH_TREE_DEPTH + 1)
    );
    Ok(())
}

#[test]
fn equation_command_inserts_block_after_text_and_round_trips()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("ab")?;
    accept(&mut session, "equation")?;
    let active_view = session.current_environment_view()?;
    assert_eq!(
        active_view.innermost_block().map(|entry| entry.kind),
        Some(crate::EnvironmentKind::Equation)
    );
    assert_eq!(session.document().body.len(), 3);
    session.run(Command::InsertText("a+b".to_owned()))?;
    let Some(Block::Equation(equation)) = session.document().body.get(1) else {
        panic!("expected equation block");
    };
    assert_eq!(caret_math::serialize_tree(&equation.math_tree), "a+b");
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::EditorKey(EditorKey::Enter))?;
    assert!(session.math_edit().is_none());
    assert_eq!(
        session
            .current_environment_view()?
            .innermost_block()
            .map(|entry| entry.kind),
        None
    );
    session.run(Command::MoveCaret(Direction::Backward))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    for _ in 0..4 {
        session.run(Command::Delete(Direction::Backward))?;
    }
    assert!(session.math_edit().is_none());
    assert_eq!(session.document().body.len(), 2);
    session.run(Command::Undo)?;
    assert_eq!(session.document().body.len(), 3);
    Ok(())
}

#[test]
fn edited_math_documents_remain_read_only_until_layout_support_is_enabled()
-> Result<(), Box<dyn std::error::Error>> {
    let mut inline = session_with_text("")?;
    accept(&mut inline, "math")?;
    inline.run(Command::InsertText("x".to_owned()))?;
    assert_math_document_is_not_yet_saveable(inline.document());

    let mut equation = session_with_text("")?;
    accept(&mut equation, "equation")?;
    equation.run(Command::InsertText("x+y".to_owned()))?;
    assert_math_document_is_not_yet_saveable(equation.document());

    let mut align = session_with_text("")?;
    accept(&mut align, "align")?;
    align.run(Command::InsertText("x&=y".to_owned()))?;
    assert_math_document_is_not_yet_saveable(align.document());
    Ok(())
}

#[test]
fn equation_command_splits_text_at_the_insertion_caret() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("abcd")?;
    let caret = Pos {
        offset: 2,
        ..session.selection().focus.clone()
    };
    session.run(Command::SetSelection(Some(Selection {
        anchor: caret.clone(),
        focus: caret,
    })))?;
    accept(&mut session, "equation")?;

    assert_eq!(session.document().body.len(), 3);
    let Some(Block::Paragraph(before)) = session.document().body.first() else {
        panic!("expected paragraph before equation");
    };
    let Some(Block::Equation(_)) = session.document().body.get(1) else {
        panic!("expected equation at caret");
    };
    let Some(Block::Paragraph(after)) = session.document().body.get(2) else {
        panic!("expected paragraph after equation");
    };
    assert_eq!(TextMap::new(before)?.text, "ab");
    assert_eq!(TextMap::new(after)?.text, "cd");

    session.run(Command::Undo)?;
    assert_eq!(session.document().body.len(), 1);
    let Some(Block::Paragraph(restored)) = session.document().body.first() else {
        panic!("undo should restore the original paragraph");
    };
    assert_eq!(TextMap::new(restored)?.text, "abcd");
    Ok(())
}

#[test]
fn align_enter_inserts_row_break() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    let active_view = session.current_environment_view()?;
    assert_eq!(
        active_view.innermost_block().map(|entry| entry.kind),
        Some(crate::EnvironmentKind::Align)
    );
    session.run(Command::InsertText("a&=b".to_owned()))?;
    session.run(Command::EditorKey(EditorKey::Enter))?;
    session.run(Command::InsertText("c&=d".to_owned()))?;
    let Some(Block::Align(value)) = session.document().body.first() else {
        panic!("expected align block");
    };
    assert_eq!(value.rows.len(), 2);
    assert_eq!(
        session.math_edit().and_then(MathEdit::align_cell_at_cursor),
        Some((1, 1, 2))
    );
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let source_map = source_projection(edit)
        .align()
        .ok_or(EditError::InvalidPosition)?;
    let content_range = source_map.content_range();
    assert_eq!(
        &source_projection(edit).source()[content_range],
        source_text(edit)
    );
    assert_eq!(source_text(edit), "a&=b \\\\ c&=d");
    Ok(())
}

#[test]
fn align_keeps_trailing_space_as_editable_content_for_the_next_input()
-> Result<(), Box<dyn std::error::Error>> {
    for (first, next, expected) in [
        ("x ", "y", "x y"),
        (" ", "y", " y"),
        (r"x\\ ", "y", r"x\\ y"),
    ] {
        let mut session = session_with_text("")?;
        accept(&mut session, "align")?;
        enter_source_edit(&mut session, Direction::Forward)?;
        session.run(Command::InsertText(first.to_owned()))?;
        assert_eq!(session.math_edit().map(source_text), Some(first));
        assert_eq!(
            session.math_edit().map(|edit| edit.cursor),
            Some(first.len())
        );

        session.run(Command::InsertText(next.to_owned()))?;
        assert_eq!(session.math_edit().map(source_text), Some(expected));
        assert_eq!(
            session.math_edit().map(|edit| edit.cursor),
            Some(expected.len())
        );
    }
    Ok(())
}

#[test]
fn incomplete_source_prefix_is_kept_out_of_the_committed_tree()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    let committed = session.document().clone();
    session.run(Command::InsertText("\\".to_owned()))?;
    assert_eq!(session.math_edit().and_then(MathEdit::source), Some("x\\"));
    assert_eq!(session.document(), &committed);
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
    Ok(())
}

#[test]
fn unmatched_closing_brace_is_not_silently_discarded() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    let generation = session.document_generation();
    let result = session.run(Command::PasteText("x}y".to_owned()));

    assert!(matches!(result, Err(EditError::InvalidMath(_))));
    assert_eq!(inline_source(&session).as_deref(), Some(""));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    assert_eq!(session.document_generation(), generation);
    Ok(())
}

#[test]
fn rejected_body_command_keeps_the_active_math_edit_target()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    let target_before = session.math_edit().cloned();
    let pending_style_before = session.pending_inline_style.clone();
    let result = session.run(Command::ToggleInlineFormat(InlineEnvironmentKind::Font));
    assert!(matches!(result, Err(EditError::UnsupportedInlineFormat)));
    assert_eq!(session.math_edit(), target_before.as_ref());
    assert_eq!(session.pending_inline_style, pending_style_before);
    assert!(matches!(
        session.run(Command::ToggleRunUnderline),
        Err(EditError::UnsupportedInlineFormat)
    ));
    assert!(matches!(
        session.run(Command::SetInlineFontFamily("Noto Serif".to_owned())),
        Err(EditError::UnsupportedInlineFormat)
    ));
    assert_eq!(session.math_edit(), target_before.as_ref());
    assert_eq!(session.pending_inline_style, pending_style_before);

    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("frac".to_owned()))?;
    let command_line_before = session.command_line().cloned();
    let document_before = session.document().clone();
    let history_state_before = session.history_state_id();
    assert_eq!(
        session.run(Command::ToggleRunUnderline),
        Err(EditError::UnsupportedInlineFormat)
    );
    assert_eq!(
        session.run(Command::InsertText("leak".to_owned())),
        Err(EditError::UnknownCommand)
    );
    assert_eq!(session.math_edit(), target_before.as_ref());
    assert_eq!(session.command_line(), command_line_before.as_ref());
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.pending_inline_style, pending_style_before);
    assert_eq!(session.history_state_id(), history_state_before);
    Ok(())
}

#[test]
fn caret_moves_between_adjacent_display_math_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        display_math_block(false, "a")?,
        display_math_block(true, "b&c")?,
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: false, .. })
    ));

    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: true, .. })
    ));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));

    session.run(Command::MoveCaret(Direction::Backward))?;
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: false, .. })
    ));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    Ok(())
}

#[test]
fn leaving_last_display_math_without_a_paragraph_keeps_formula_focus()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        display_math_block(false, "a")?,
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    let target = session.math_edit().cloned();
    let selection = session.selection().clone();

    session.run(Command::MoveCaret(Direction::Forward))?;

    assert_eq!(session.math_edit(), target.as_ref());
    assert_eq!(session.selection(), &selection);
    Ok(())
}

#[test]
fn escape_leaves_last_equation_and_align_at_the_nearest_body_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    for (align, source) in [(false, "a"), (true, "a&b")] {
        let mut document = caret_doc::Document::blank();
        document.body = vec![
            Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
            display_math_block(align, source)?,
        ];
        let mut session = Session::new(document)?;
        let paragraph_id = match session.document().body.first() {
            Some(Block::Paragraph(paragraph)) => paragraph.id.clone(),
            _ => panic!("document should start with a paragraph"),
        };
        session.run(Command::EnterMath(Direction::Forward))?;

        session.run(Command::EditorKey(EditorKey::Escape))?;

        assert!(session.math_edit().is_none());
        assert_eq!(session.selection().focus.paragraph_id, paragraph_id);
        assert_eq!(session.selection().focus.offset, 0);
    }
    Ok(())
}

#[test]
fn escape_skips_adjacent_formula_blocks_and_updates_paragraph_affinity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        display_math_block(false, "a")?,
        display_math_block(true, "b&c")?,
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    session.run(Command::EditorKey(EditorKey::Escape))?;

    let Some(Block::Paragraph(after)) = session.document().body.last() else {
        panic!("document should end with a paragraph");
    };
    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.paragraph_id, after.id);
    assert_eq!(session.selection().focus.affinity, Affinity::Upstream);
    assert_eq!(
        session.selection_visual_affinity(),
        crate::VisualAffinity::Upstream
    );
    Ok(())
}

#[test]
fn escape_uses_the_opposite_side_only_when_the_preferred_side_has_no_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let before = Paragraph::empty(NodeId::generate());
    let after = Paragraph::empty(NodeId::generate());
    let after_id = after.id.clone();
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        display_math_block(false, "x")?,
        Block::Paragraph(after),
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    let Block::Paragraph(before) = &mut session.document.body[0] else {
        panic!("document should start with the paragraph");
    };
    before.inlines = vec![ParagraphContent::Opaque(caret_doc::OpaqueNode {
        raw: serde_json::json!({"type": "future_inline", "payload": "preserve"}),
    })];

    session.run(Command::EditorKey(EditorKey::Escape))?;

    assert!(session.math_edit().is_none());
    assert_eq!(session.selection().focus.paragraph_id, after_id);
    Ok(())
}

#[test]
fn leaving_and_reentering_inline_math_splits_typing_undo_groups()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.begin_inline_math()?;
    enter_source_edit(&mut session, Direction::Forward)?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());
    enter_source_edit(&mut session, Direction::Backward)?;
    session.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("xy"));

    session.run(Command::Undo)?;

    assert_eq!(inline_source(&session).as_deref(), Some("x"));
    enter_source_edit(&mut session, Direction::Backward)?;
    assert_eq!(session.math_edit().map(source_text), Some("x"));
    Ok(())
}

#[test]
fn deleting_empty_align_after_a_formula_chain_is_undoable() -> Result<(), Box<dyn std::error::Error>>
{
    let mut document = caret_doc::Document::blank();
    document.body = vec![
        Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
        display_math_block(false, "a")?,
        display_math_block(true, "b")?,
    ];
    let mut session = Session::new(document)?;
    session.run(Command::EnterMath(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::Delete(Direction::Backward))?;
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    assert!(
        first_align(&session)?.rows[0].cells[0]
            .root
            .content
            .is_empty()
    );
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: true, .. })
    ));

    session.run(Command::Delete(Direction::Backward))?;
    assert_eq!(session.document().body.len(), 2);
    assert!(session.math_edit().is_none());

    session.run(Command::Undo)?;
    assert_eq!(session.document().body.len(), 3);
    assert!(matches!(
        session.math_edit().map(|edit| &edit.target),
        Some(MathEditTarget::Block { align: true, .. })
    ));
    assert!(session.math_edit().is_some_and(|edit| {
        edit.source_projection.is_none() && matches!(edit.branch, MathEditBranch::Tree { .. })
    }));
    session.run(Command::Redo)?;
    assert_eq!(session.document().body.len(), 2);
    assert!(session.math_edit().is_none());
    Ok(())
}

#[test]
fn invalid_structure_selection_keeps_the_active_math_edit_target()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    let target_before = session.math_edit().cloned();
    let position = session.selection().focus.clone();
    let result = session.select_block_environments_from_range(
        position.clone(),
        position,
        crate::VisualAffinity::Downstream,
    );
    assert!(matches!(result, Err(EditError::InvalidPosition)));
    assert_eq!(session.math_edit(), target_before.as_ref());
    Ok(())
}

#[test]
fn structure_selection_blocks_entering_adjacent_inline_math()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = caret_doc::Document::blank();
    let Some(Block::Paragraph(paragraph)) = document.body.first_mut() else {
        panic!("blank document should start with a paragraph");
    };
    let tree = match caret_math::parse("x", caret_math::MathInputKind::Inline)? {
        caret_math::ParsedMath::Tree(tree) => tree,
        caret_math::ParsedMath::Align(_) => panic!("expected inline formula"),
    };
    paragraph.inlines = vec![caret_doc::ParagraphContent::Math(caret_doc::InlineMath {
        id: NodeId::generate(),
        math_tree: tree,
        extra: Default::default(),
    })];
    let selected_environment_id = NodeId::generate();
    document
        .body
        .push(Block::Environment(caret_doc::BlockEnvironment {
            id: selected_environment_id.clone(),
            kind: caret_doc::BlockEnvironmentKind::Section,
            definition_id: None,
            props: Default::default(),
            title: None,
            children: vec![Block::Paragraph(caret_doc::Paragraph::empty(
                NodeId::generate(),
            ))],
            extra: Default::default(),
        }));
    let mut session = Session::new(document)?;
    session.select_block_environment(selected_environment_id.clone())?;
    let structure_selection_before = session
        .structure_selection()
        .map(|(a, f)| (a.clone(), f.clone()));
    let document_before = session.document().clone();

    session.run(Command::EnterMath(Direction::Forward))?;

    assert!(session.math_edit().is_none());
    assert_eq!(
        session.selected_block_environment(),
        Some(&selected_environment_id)
    );
    assert_eq!(
        session
            .structure_selection()
            .map(|(a, f)| (a.clone(), f.clone())),
        structure_selection_before
    );
    assert_eq!(session.document(), &document_before);
    Ok(())
}

#[test]
fn superscript_and_subscript_templates_use_previous_tree_content_as_their_base()
-> Result<(), EditError> {
    for (template, empty_script, filled_script) in
        [("^{}", "x^{}", "x^{2}"), ("_{}", "x_{}", "x_{2}")]
    {
        let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
        session.begin_inline_math()?;
        session.run(Command::InsertText("x".to_owned()))?;

        session.math_insert_template(template)?;
        assert_eq!(inline_source(&session).as_deref(), Some(empty_script));
        let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
        let MathEditBranch::Tree { path, .. } = &edit.branch else {
            return Err(EditError::InvalidPosition);
        };
        assert!(path.steps().iter().any(|step| matches!(
            step,
            MathSequenceStep::Operand {
                operand_index: 1,
                ..
            }
        )));

        session.run(Command::InsertText("2".to_owned()))?;
        assert_eq!(inline_source(&session).as_deref(), Some(filled_script));
        session.run(Command::Undo)?;
        assert_eq!(inline_source(&session).as_deref(), Some(empty_script));
        session.run(Command::Redo)?;
        assert_eq!(inline_source(&session).as_deref(), Some(filled_script));
    }
    Ok(())
}
