//! 公式编辑状态的行为测试。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::text_branch::{text_source_boundary_from_map, text_source_range_from_map};
use super::*;
use crate::editing::{
    DocumentCaretPosition, HostCaretKind, HostCaretLocalPosition, HostCaretPosition,
    MathCaretCoordinate, SessionCaretSnapshot,
};

fn test_error(error: impl std::fmt::Display) -> EditError {
    EditError::InvalidMath(format!("test prerequisite failed: {error}"))
}

fn math_edit_for_test(
    target: MathEditTarget,
    cursor: usize,
    branch: MathEditBranch,
    revision: MathEditRevision,
    source_projection: Option<Arc<caret_math::MathSourceMap<'static>>>,
) -> MathEdit {
    let mut edit = MathEdit::entering(target, cursor, branch, revision);
    edit.source_projection = source_projection;
    edit
}

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

fn assert_math_document_is_saveable(document: &caret_doc::Document) {
    caret_doc::CdocFile::new(document.clone())
        .unwrap_or_else(|error| panic!("supported math document should be saveable: {error}"));
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
                    .map_err(test_error)
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

fn active_math_text_bold_chars(session: &Session) -> Result<Vec<(char, bool)>, EditError> {
    let edit = session.math_edit().ok_or(EditError::InvalidPosition)?;
    let MathEditBranch::Text { path, .. } = &edit.branch else {
        return Err(EditError::InvalidPosition);
    };
    let tree = session.math_tree_for_edit(edit, path.align_cell())?;
    let island = path::contiguous_text_flow_text(tree, path).ok_or(EditError::InvalidPosition)?;
    let mut characters = Vec::new();
    for path in island.paths {
        let bold = super::text_inline_environment::inherited_environment_value(
            tree,
            &path,
            InlineEnvironmentKind::Bold,
        )
        .ok_or(EditError::InvalidPosition)?;
        let flow = path
            .resolve_structurally(tree)
            .ok_or(EditError::InvalidPosition)?;
        characters.extend(
            flow.runs
                .iter()
                .flat_map(|run| run.text.chars())
                .map(|character| (character, bold)),
        );
    }
    Ok(characters)
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

fn inline_math_text_session_with_selection(
    source: &str,
    anchor: usize,
    focus: usize,
) -> Result<Session, Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath(source.to_owned()))?;
    move_into_math_text(&mut session)?;
    session.run(Command::SetMathTextSelection { anchor, focus })?;
    Ok(session)
}

fn align_math_text_session_with_selection(
    anchor: usize,
    focus: usize,
) -> Result<Session, Box<dyn std::error::Error>> {
    let (parsed_align, projection) =
        display_math_block_with_projection(true, r"x+\text{abcd}", NodeId::generate())?;
    let Block::Align(align_block) = parsed_align else {
        return Err(EditError::InvalidPosition.into());
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
        anchor_visual_affinity: crate::VisualAffinity::Downstream,
        visual_affinity: crate::VisualAffinity::Downstream,
    };
    let mut align_document = Document::blank();
    align_document.body.push(Block::Align(align_block));
    let mut session = Session::new(align_document)?;
    session.math_edit = Some(math_edit_for_test(
        MathEditTarget::Block {
            block_id,
            align: true,
        },
        cursor,
        branch,
        session.current_math_edit_revision(),
        Some(Arc::new(projection)),
    ));
    session.run(Command::SetMathTextSelection { anchor, focus })?;
    Ok(session)
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
        Block::Table(_) => false,
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

fn paragraph_with_run_text(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::empty(NodeId::generate());
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
    paragraph
}

fn equation_with_inline_math_tree(source: &str) -> (Block, NodeId) {
    let id = NodeId::generate();
    (
        Block::Equation(Equation {
            id: id.clone(),
            math_tree: parsed_inline_tree(source),
            extra: Default::default(),
        }),
        id,
    )
}

fn shift_right(session: &mut Session) -> Result<Effect, EditError> {
    session.run(Command::EditorKey(EditorKey::RightArrow {
        candidate: None,
        extend: true,
    }))
}

fn shift_left(session: &mut Session) -> Result<Effect, EditError> {
    session.run(Command::EditorKey(EditorKey::LeftArrow {
        candidate: None,
        extend: true,
    }))
}

/// 断言扩选焦点停在锚点所在序列层，而不是下钻到嵌套结构。
fn assert_tree_extension_stays_on_anchor_sequence(
    session: &Session,
    expected_anchor_offset: usize,
    expected_cursor: usize,
) {
    let edit = session
        .math_edit()
        .unwrap_or_else(|| panic!("公式编辑状态应保留"));
    let MathEditBranch::Tree { path, anchor } = &edit.branch else {
        panic!("扩选焦点应停留在 MathTree 分支");
    };
    assert_eq!(
        anchor.offset, expected_anchor_offset,
        "扩选只移动焦点，锚点必须固定在起点"
    );
    assert_eq!(edit.cursor, expected_cursor, "扩选光标应原子步进");
    assert_ne!(anchor.offset, edit.cursor, "扩选范围不能坍缩成一个光标位置");
    assert_eq!(
        anchor.path, *path,
        "扩选焦点路径必须与锚点路径一致，否则历史恢复和后续按键无法消费该范围"
    );
}

fn assert_whole_equation_structure_range(
    session: &Session,
    equation_id: &NodeId,
    direction: Direction,
) {
    assert!(
        session.math_edit().is_none(),
        "转换为结构范围后应退出公式编辑"
    );
    let (anchor, focus) = session
        .structure_selection_range()
        .unwrap_or_else(|| panic!("缺少整公式结构范围"));
    let (anchor_side, focus_side) = match direction {
        Direction::Forward => (Direction::Backward, Direction::Forward),
        Direction::Backward => (Direction::Forward, Direction::Backward),
    };
    assert!(matches!(
        anchor,
        crate::editing::DocumentRangeEndpoint::BlockEdge { block_id, side }
            if block_id == equation_id && *side == anchor_side
    ));
    assert!(matches!(
        focus,
        crate::editing::DocumentRangeEndpoint::BlockEdge { block_id, side }
            if block_id == equation_id && *side == focus_side
    ));
}

fn inline_math_paragraph(inlines: Vec<ParagraphContent>) -> (Document, NodeId) {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines = inlines;
    let paragraph_id = paragraph.id.clone();
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    (document, paragraph_id)
}

fn inline_math_node(source: &str) -> ParagraphContent {
    ParagraphContent::Math(InlineMath {
        id: NodeId::generate(),
        math_tree: parsed_inline_tree(source),
        extra: Default::default(),
    })
}

fn text_content(text: &str) -> ParagraphContent {
    ParagraphContent::Text(caret_doc::TextFlow {
        runs: vec![Run {
            text: text.to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    })
}

fn enter_text_branch_at_align_cell(
    session: &mut Session,
    row: usize,
    cell: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    session.run(Command::EnterMath(Direction::Forward))?;
    for _ in 0..32 {
        if let Some(MathEditBranch::Text { path, .. }) =
            session.math_edit().map(|edit| &edit.branch)
            && path.align_cell() == Some((row, cell))
        {
            return Ok(());
        }
        session.run(Command::MoveCaret(Direction::Forward))?;
    }
    Err(format!("Align cell ({row}, {cell}) text island is unreachable").into())
}

#[path = "tests/ime.rs"]
mod ime;
#[path = "tests/selection_caret.rs"]
mod selection_caret;
#[path = "tests/source_edit.rs"]
mod source_edit;
#[path = "tests/symbol_input.rs"]
mod symbol_input;
#[path = "tests/tree_ops.rs"]
mod tree_ops;
