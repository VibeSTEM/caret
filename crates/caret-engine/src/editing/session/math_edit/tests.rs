//! 公式编辑状态的行为测试。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

fn aligned_body_offset(align: &Align) -> usize {
    align
        .source
        .find("\\begin{aligned}")
        .unwrap_or_else(|| panic!("aligned test source must have an opening environment"))
        + "\\begin{aligned}".len()
}

fn assert_math_document_is_not_yet_saveable(document: &caret_doc::Document) {
    assert!(matches!(
        caret_doc::CdocFile::new(document.clone()),
        Err(caret_doc::DocumentError::ReadOnlyPreview)
    ));
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
fn arrow_treats_a_rendered_control_symbol_as_one_caret_stop() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath(r"\alpha".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(6));

    session.run(Command::MoveCaret(Direction::Backward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(0));
    Ok(())
}

#[test]
fn backspace_deletes_one_grapheme_from_formula_source() -> Result<(), EditError> {
    let mut session = session_with_text("").map_err(|_| EditError::InvalidPosition)?;
    session.run(Command::InsertInlineMath("abc".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::Delete(Direction::Backward))?;

    assert_eq!(inline_source(&session).as_deref(), Some("ab"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(2));
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
    session.run(Command::PasteText("x^{2}".to_owned()))?;

    assert_eq!(inline_source(&session).as_deref(), Some("x^{2}"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(5));
    Ok(())
}

#[test]
fn align_row_break_is_one_navigation_and_deletion_unit() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "align")?;
    session.run(Command::InsertText(r"a\\abc".to_owned()))?;

    for _ in 0..4 {
        session.run(Command::MoveCaret(Direction::Backward))?;
    }
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(3));
    session.run(Command::Delete(Direction::Backward))?;

    let Some(Block::Align(align)) = session.document().body.first() else {
        panic!("expected Align block");
    };
    assert_eq!(align.source, r"\begin{aligned}aabc\end{aligned}");
    Ok(())
}

#[test]
fn math_source_mode_rejects_ime_without_changing_focus_or_document()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    session.run(Command::InsertInlineMath("abc".to_owned()))?;
    session.run(Command::EnterMath(Direction::Backward))?;
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
    assert_eq!(align_cell_at("a&=b \\\\ c& =d", 12), Some((1, 1, 1)));
    assert_eq!(align_cell_at("a&=b", 1), Some((0, 0, 1)));
}

#[test]
fn align_cell_mapping_ignores_nested_environment_separators() {
    let source = r"\begin{matrix}a&b\\c&d\end{matrix}&=z";
    let nested_cell = source
        .find("c&d")
        .unwrap_or_else(|| panic!("missing nested cell"))
        + 2;
    assert_eq!(
        align_cell_at(source, nested_cell),
        Some((0, 0, nested_cell))
    );

    let outer_cell = source
        .rfind('&')
        .unwrap_or_else(|| panic!("missing outer separator"))
        + 1;
    assert_eq!(align_cell_at(source, outer_cell), Some((0, 1, 0)));
}

#[test]
fn align_cell_mapping_ignores_separators_in_comments() {
    let source = "a % & ignored \\\\ ignored\n+b&c";
    let in_comment = source
        .find("ignored")
        .unwrap_or_else(|| panic!("missing comment"))
        + 3;
    assert_eq!(align_cell_at(source, in_comment), Some((0, 0, in_comment)));
    let after_outer_separator = source
        .rfind('&')
        .unwrap_or_else(|| panic!("missing separator"))
        + 2;
    assert_eq!(
        align_cell_at(source, after_outer_separator),
        Some((0, 1, 1))
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
        .insert("enabled".to_owned(), serde_json::json!(false));
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
    if let Some(range) = &mut text.source_range {
        range
            .extra
            .insert("vendorRange".to_owned(), serde_json::json!(5));
    }
    text.source_map[0]
        .extra
        .insert("vendorMap".to_owned(), serde_json::json!(6));
    text.source_map[0]
        .source_range
        .extra
        .insert("vendorMapRange".to_owned(), serde_json::json!(7));

    let updated_source = source.replace("+x", "+y");
    let mut updated = parsed_inline_tree(&updated_source);
    let edit = (source.len() - 1)..source.len();
    assert!(preserve_math_tree_metadata_for_edit(
        &previous,
        &mut updated,
        &edit,
        &edit,
    ));
    let environment = first_environment(&mut updated.root)
        .unwrap_or_else(|| panic!("mapped environment should remain"));
    assert_eq!(environment.id, environment_id);
    assert_eq!(environment.definition_id.as_deref(), Some("template.bold"));
    assert_eq!(environment.props["enabled"], false);
    assert_eq!(environment.extra["vendorEnvironment"], 3);
    let text = first_text(&mut updated.root.content)
        .unwrap_or_else(|| panic!("mapped TextFlow should remain"));
    assert_eq!(text.runs[0].language.as_deref(), Some("en"));
    assert_eq!(text.runs[0].props["underline"], true);
    assert_eq!(text.runs[0].extra["vendorRun"], 4);
    assert_eq!(text.source_map[0].extra["vendorMap"], 6);
    assert_eq!(text.source_map[0].source_range.extra["vendorMapRange"], 7);
    let Some(range) = &text.source_range else {
        panic!("mapped TextFlow should retain its source range");
    };
    assert_eq!(range.extra["vendorRange"], 5);
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
fn duplicate_text_source_map_targets_refuse_ambiguous_metadata_transfer() {
    let mut previous = parsed_inline_tree(r"\text{x}");
    let Some(MathContent::Text(previous_text)) = previous.root.content.first_mut() else {
        panic!("text formula should start with a TextFlow");
    };
    previous_text.source_map[0]
        .extra
        .insert("vendorMapping".to_owned(), serde_json::json!(true));
    let mut updated = parsed_inline_tree(r"\text{x}");
    let Some(MathContent::Text(updated_text)) = updated.root.content.first_mut() else {
        panic!("text formula should start with a TextFlow");
    };
    updated_text
        .source_map
        .push(updated_text.source_map[0].clone());

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

    assert!(preserve_math_tree_metadata_for_edit(
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
    session.run(Command::EnterMath(Direction::Backward))?;

    let mut refused = false;
    for _ in 0..5 {
        let generation_before = session.document_generation();
        let document_before = session.document().clone();
        let edit_before = session.math_edit().cloned();
        match session.run(Command::Delete(Direction::Backward)) {
            Ok(_) => {}
            Err(EditError::InvalidMath(_)) => {
                assert_eq!(session.document(), &document_before);
                assert_eq!(session.math_edit(), edit_before.as_ref());
                assert_eq!(session.document_generation(), generation_before);
                refused = true;
                break;
            }
            Err(error) => return Err(error),
        }
    }
    assert!(refused, "metadata-bearing node deletion should be rejected");
    Ok(())
}

#[test]
fn deleting_mapped_math_text_with_mapping_metadata_is_atomic() -> Result<(), EditError> {
    let mut math_tree = parsed_inline_tree(r"\text{x}");
    let Some(MathContent::Text(text)) = math_tree.root.content.first_mut() else {
        panic!("text formula should start with a TextFlow");
    };
    text.source_map[0]
        .extra
        .insert("vendorMapping".to_owned(), serde_json::json!(true));
    text.source_map[0]
        .source_range
        .extra
        .insert("vendorRange".to_owned(), serde_json::json!(true));

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
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::MoveCaret(Direction::Backward))?;
    let document_before = session.document().clone();
    let edit_before = session.math_edit().cloned();
    let generation_before = session.document_generation();

    assert!(matches!(
        session.run(Command::Delete(Direction::Backward)),
        Err(EditError::InvalidMath(_))
    ));
    assert_eq!(session.document(), &document_before);
    assert_eq!(session.math_edit(), edit_before.as_ref());
    assert_eq!(session.document_generation(), generation_before);
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
    let metadata_preserved = preserve_align_metadata(
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
    let Block::Align(mut previous) = display_math_block(true, "a+b&b")? else {
        panic!("expected Align block");
    };
    let row_id = previous.rows[0].id.clone();
    let tree_id = previous.rows[0].cells[0].root.id.clone();
    previous.rows[0].cells[0]
        .extra
        .insert("futureTreeValue".to_owned(), serde_json::json!("kept"));
    previous.rows[0].cells[0]
        .root
        .extra
        .insert("futureNodeValue".to_owned(), serde_json::json!("kept"));
    let Block::Align(mut updated) = display_math_block_with_id(true, "a+x&b", previous.id.clone())?
    else {
        panic!("expected parsed Align block");
    };
    let old_offset = aligned_body_offset(&previous);
    let new_offset = aligned_body_offset(&updated);
    preserve_align_metadata(
        &previous,
        &mut updated,
        &(old_offset + 2..old_offset + 3),
        &(new_offset + 2..new_offset + 3),
    );

    assert_eq!(updated.rows[0].id, row_id);
    assert_eq!(updated.rows[0].cells[0].extra["futureTreeValue"], "kept");
    assert_eq!(updated.rows[0].cells[0].root.id, tree_id);
    assert_eq!(
        updated.rows[0].cells[0].root.extra["futureNodeValue"],
        "kept"
    );
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
    preserve_align_metadata(
        &previous,
        &mut updated,
        &(old_offset + 3..old_offset + 3),
        &(new_offset + 3..new_offset + 5),
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
    preserve_align_metadata(
        &previous,
        &mut updated,
        &(old_offset..old_offset),
        &(new_offset..new_offset + 2),
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
    let old_offset = aligned_body_offset(&previous);
    let new_offset = aligned_body_offset(&updated);
    preserve_align_metadata(
        &previous,
        &mut updated,
        &(old_offset + 2..old_offset + 2),
        &(new_offset + 2..new_offset + 3),
    );

    assert_eq!(updated.rows[0].cells[1].root.id, cell_id);
    assert_eq!(updated.rows[0].cells[1].extra["futureCellValue"], "keep");
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
    crate::validate_document_math_sources(session.document()).map_err(EditError::InvalidDocument)
}

#[test]
fn preserving_math_metadata_keeps_the_parent_of_an_edited_subtree() {
    let old_source = "\\frac{ab}{c}";
    let new_source = "\\frac{axb}{c}";
    let mut previous = parsed_inline_tree(old_source);
    let (prefix_end, previous_tail, _) = unchanged_source_boundaries(old_source, new_source);
    let Some(changed_parent) = math_node_mut(&mut previous.root, 0)
        .children
        .iter_mut()
        .find(|flow| {
            flow.source_range.as_ref().is_some_and(|range| {
                range.start as usize <= prefix_end && range.end as usize >= previous_tail
            })
        })
    else {
        panic!("fraction operand should contain the edit");
    };
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
    match caret_math::parse(source, caret_math::MathInputKind::Inline) {
        Ok(caret_math::ParsedMath::Tree(tree)) => tree,
        Ok(caret_math::ParsedMath::Align(_)) => panic!("expected inline math tree"),
        Err(error) => panic!("test formula should parse: {error}"),
    }
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
                source_range: None,
                source_map: Vec::new(),
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
    let Some(Block::Paragraph(paragraph)) = session.document().body.first() else {
        return None;
    };
    paragraph.inlines.iter().find_map(|node| match node {
        ParagraphContent::Math(math) => Some(math.math_tree.source.clone()),
        _ => None,
    })
}

#[test]
fn math_command_inserts_inline_math_and_sub_commands_edit_it()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("a")?;
    accept(&mut session, "math")?;
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
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::BeginCommandLine)?;
    session.run(Command::TypeCommandLine("frac".to_owned()))?;

    session.run(Command::Undo)?;
    assert!(session.command_line().is_none());
    assert_eq!(
        session.math_edit().map(|edit| edit.source.as_str()),
        Some("")
    );
    assert_eq!(inline_source(&session).as_deref(), Some(""));

    session.run(Command::Redo)?;
    assert!(session.command_line().is_none());
    assert_eq!(
        session.math_edit().map(|edit| edit.source.as_str()),
        Some("x")
    );
    assert_eq!(inline_source(&session).as_deref(), Some("x"));
    Ok(())
}

#[test]
fn undo_and_redo_restore_formula_target_and_source_cursor() -> Result<(), Box<dyn std::error::Error>>
{
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::InsertText("y".to_owned()))?;
    let after_edit = session
        .math_edit()
        .cloned()
        .ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_edit.source, "xy");
    assert_eq!(after_edit.cursor, 2);

    session.run(Command::Undo)?;
    let after_undo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_undo.target, after_edit.target);
    assert_eq!(after_undo.source, "");
    assert_eq!(after_undo.cursor, 0);
    assert_eq!(inline_source(&session).as_deref(), Some(""));

    session.run(Command::Redo)?;
    let after_redo = session.math_edit().ok_or(EditError::InvalidPosition)?;
    assert_eq!(after_redo.target, after_edit.target);
    assert_eq!(after_redo.source, "xy");
    assert_eq!(after_redo.cursor, 2);
    assert_eq!(inline_source(&session).as_deref(), Some("xy"));
    Ok(())
}

#[test]
fn undo_and_redo_restore_equation_and_align_edit_targets() -> Result<(), Box<dyn std::error::Error>>
{
    for (query, align) in [("equation", false), ("align", true)] {
        let mut session = session_with_text("")?;
        accept(&mut session, query)?;
        session.run(Command::InsertText("x".to_owned()))?;
        let after_edit = session
            .math_edit()
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        assert!(matches!(
            after_edit.target,
            MathEditTarget::Block { align: value, .. } if value == align
        ));

        session.run(Command::Undo)?;
        let after_undo = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert_eq!(after_undo.target, after_edit.target);
        assert_eq!(after_undo.cursor, 0);

        session.run(Command::Redo)?;
        let after_redo = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert_eq!(after_redo.target, after_edit.target);
        assert_eq!(after_redo.source, "x");
        assert_eq!(after_redo.cursor, 1);
    }
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

        session.run(Command::Undo)?;
        assert!(session.math_edit().is_none());
        session.run(Command::Redo)?;
        let restored = session.math_edit().ok_or(EditError::InvalidPosition)?;
        assert_eq!(restored.target, created.target);
        assert_eq!(restored.source, created.source);
        assert_eq!(restored.cursor, created.cursor);
    }
    Ok(())
}

#[test]
fn entering_fenced_inline_math_edits_inside_the_fences_from_both_sides()
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
    assert_eq!(from_left.math_edit().map(|edit| edit.cursor), Some(1));
    from_left.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(inline_source(&from_left).as_deref(), Some("$yx$"));

    let mut from_right = session_with_text("")?;
    from_right.run(Command::InsertInlineMath(r"\(x\)".to_owned()))?;
    from_right.run(Command::EnterMath(Direction::Backward))?;
    assert_eq!(from_right.math_edit().map(|edit| edit.cursor), Some(3));
    from_right.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(inline_source(&from_right).as_deref(), Some(r"\(xy\)"));
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
    assert_eq!(from_left.math_edit().map(|edit| edit.cursor), Some(2));
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
            source_range: None,
            source_map: Vec::new(),
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
fn backspace_removes_empty_construct_and_then_the_object() -> Result<(), Box<dyn std::error::Error>>
{
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
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
fn greek_sub_commands_are_case_sensitive() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    accept(&mut session, "Gamma")?;
    accept(&mut session, "gamma")?;
    assert_eq!(inline_source(&session).as_deref(), Some("\\Gamma\\gamma"));
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
    assert_eq!(equation.math_tree.source, "a+b");
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
    Ok(())
}

#[test]
fn invalid_source_is_rejected_without_changes() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = session_with_text("")?;
    accept(&mut session, "math")?;
    session.run(Command::InsertText("x".to_owned()))?;
    let result = session.run(Command::InsertText("\\".to_owned()));
    assert!(matches!(result, Err(EditError::InvalidMath(_))));
    assert_eq!(inline_source(&session).as_deref(), Some("x"));
    assert_eq!(session.math_edit().map(|edit| edit.cursor), Some(1));
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
    session.run(Command::InsertText("x".to_owned()))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    session.run(Command::MoveCaret(Direction::Forward))?;
    assert!(session.math_edit().is_none());
    session.run(Command::EnterMath(Direction::Backward))?;
    session.run(Command::InsertText("y".to_owned()))?;
    assert_eq!(inline_source(&session).as_deref(), Some("xy"));

    session.run(Command::Undo)?;

    assert_eq!(inline_source(&session).as_deref(), Some("x"));
    assert_eq!(
        session.math_edit().map(|edit| edit.source.as_str()),
        Some("x")
    );
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
    assert_eq!(
        session.math_edit().map(|edit| edit.source.as_str()),
        Some("")
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
