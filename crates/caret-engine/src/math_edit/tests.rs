//! 验证数学树编辑及失败原子性。

use super::{MathEdit, MathEditError, apply_math_edit};
use crate::editing::MathPos;
use caret_doc::{
    Block, Document, Equation, MATH_TREE_SCHEMA_VERSION, MathNode, MathNodeKind, MathSourceRange,
    MathTree, NodeId,
};
use std::collections::BTreeMap;

fn empty_row() -> MathTree {
    MathTree {
        schema_version: MATH_TREE_SCHEMA_VERSION,
        source: String::new(),
        root: MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Row,
            text: None,
            attributes: BTreeMap::new(),
            source_range: None,
            children: Vec::new(),
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }
}

fn assert_tree_is_document_valid(tree: MathTree) {
    let mut document = Document::blank();
    document.body.push(Block::Equation(Equation {
        id: NodeId::generate(),
        math_tree: tree,
        extra: BTreeMap::new(),
    }));
    assert!(document.validate().is_ok());
}

#[test]
fn inserts_text_and_returns_a_resolvable_position_without_mutating_input() {
    let original = empty_row();
    let position = MathPos {
        path: vec![original.root.id.clone()],
        offset: 0,
    };
    let Ok(result) = apply_math_edit(&original, &position, MathEdit::InsertText("x2".to_owned()))
    else {
        panic!("ordinary math text should insert into an empty row");
    };
    assert!(original.root.children.is_empty());
    assert_eq!(result.tree.root.children.len(), 1);
    assert_eq!(result.tree.root.children[0].kind, MathNodeKind::Symbol);
    assert_eq!(result.tree.root.children[0].text.as_deref(), Some("x2"));
    assert!(result.position.resolve(&result.tree).is_some());
    assert_tree_is_document_valid(result.tree);
}

#[test]
fn math_text_uses_latex_rust_classification_for_operators_symbols_and_commands() {
    let original = empty_row();
    let position = MathPos {
        path: vec![original.root.id.clone()],
        offset: 0,
    };
    let operator = apply_math_edit(&original, &position, MathEdit::InsertText("+".to_owned()))
        .unwrap_or_else(|error| panic!("operator input should parse: {error}"));
    assert_eq!(operator.tree.root.children.len(), 1);
    assert_eq!(
        operator.tree.root.children[0]
            .attributes
            .get("atomClass")
            .map(String::as_str),
        Some("bin")
    );
    assert!(operator.position.resolve(&operator.tree).is_some());

    let named = apply_math_edit(
        &original,
        &position,
        MathEdit::InsertText("\\alpha".to_owned()),
    )
    .unwrap_or_else(|error| panic!("named symbol input should parse: {error}"));
    assert_eq!(named.tree.root.children[0].kind, MathNodeKind::Symbol);
    assert!(named.tree.root.children[0].text.is_some());
    assert!(named.position.resolve(&named.tree).is_some());

    let command = apply_math_edit(
        &original,
        &position,
        MathEdit::InsertText(r"\frac{x}{y}".to_owned()),
    )
    .unwrap_or_else(|error| panic!("fraction command input should parse: {error}"));
    assert_eq!(command.tree.root.children[0].kind, MathNodeKind::Fraction);
    assert!(command.position.resolve(&command.tree).is_some());
    assert_tree_is_document_valid(command.tree);
}

#[test]
fn fraction_insertion_enters_the_numerator_and_fills_its_slot() {
    let original = empty_row();
    let row_position = MathPos {
        path: vec![original.root.id.clone()],
        offset: 0,
    };
    let Ok(fraction) = apply_math_edit(&original, &row_position, MathEdit::InsertFraction) else {
        panic!("fraction should insert into an empty row");
    };
    assert_eq!(fraction.tree.root.children[0].kind, MathNodeKind::Fraction);
    assert_eq!(fraction.tree.root.children[0].children.len(), 2);
    let Ok(filled) = apply_math_edit(
        &fraction.tree,
        &fraction.position,
        MathEdit::InsertText("n".to_owned()),
    ) else {
        panic!("numerator placeholder should accept ordinary text");
    };
    assert_eq!(
        filled.tree.root.children[0].children[0].kind,
        MathNodeKind::Symbol
    );
    assert_eq!(
        filled.tree.root.children[0].children[0].text.as_deref(),
        Some("n")
    );
    assert_eq!(
        filled.tree.root.children[0].children[1].kind,
        MathNodeKind::Placeholder
    );
    assert_tree_is_document_valid(filled.tree);
}

#[test]
fn fraction_can_be_inserted_at_either_edge_of_an_ordinary_symbol() {
    for (offset, insertion_index) in [(0, 0), (1, 1)] {
        let mut tree = empty_row();
        let symbol = MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Symbol,
            text: Some("x".to_owned()),
            attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
            source_range: None,
            children: Vec::new(),
            extra: BTreeMap::new(),
        };
        let symbol_id = symbol.id.clone();
        tree.root.children.push(symbol);
        let position = MathPos {
            path: vec![tree.root.id.clone(), symbol_id],
            offset,
        };

        let Ok(result) = apply_math_edit(&tree, &position, MathEdit::InsertFraction) else {
            panic!("fraction should insert at an ordinary symbol edge");
        };

        assert_eq!(
            result.tree.root.children[insertion_index].kind,
            MathNodeKind::Fraction
        );
        assert_eq!(result.tree.root.children.len(), 2);
        assert!(result.position.resolve(&result.tree).is_some());
        assert_tree_is_document_valid(result.tree);
    }
}

#[test]
fn nested_structure_insertion_wraps_filled_fraction_slots_in_rows() {
    let original = empty_row();
    let row_position = MathPos {
        path: vec![original.root.id.clone()],
        offset: 0,
    };
    let fraction = apply_math_edit(&original, &row_position, MathEdit::InsertFraction)
        .unwrap_or_else(|error| panic!("fraction insertion failed: {error}"));
    let numerator = apply_math_edit(
        &fraction.tree,
        &fraction.position,
        MathEdit::InsertText("n".to_owned()),
    )
    .unwrap_or_else(|error| panic!("numerator input failed: {error}"));
    let radical = apply_math_edit(
        &numerator.tree,
        &numerator.position,
        MathEdit::InsertRadical,
    )
    .unwrap_or_else(|error| panic!("radical after numerator text failed: {error}"));

    let numerator_row = &radical.tree.root.children[0].children[0];
    assert_eq!(numerator_row.kind, MathNodeKind::Row);
    assert_eq!(numerator_row.children[0].text.as_deref(), Some("n"));
    assert_eq!(numerator_row.children[1].kind, MathNodeKind::Radical);
    assert!(radical.position.resolve(&radical.tree).is_some());

    let radicand = apply_math_edit(
        &radical.tree,
        &radical.position,
        MathEdit::InsertText("r".to_owned()),
    )
    .unwrap_or_else(|error| panic!("radicand input failed: {error}"));
    let nested_fraction =
        apply_math_edit(&radicand.tree, &radicand.position, MathEdit::InsertFraction)
            .unwrap_or_else(|error| panic!("fraction inside radical text failed: {error}"));

    let radicand_row = &nested_fraction.tree.root.children[0].children[0].children[1].children[1];
    assert_eq!(radicand_row.kind, MathNodeKind::Row);
    assert_eq!(radicand_row.children[0].text.as_deref(), Some("r"));
    assert_eq!(radicand_row.children[1].kind, MathNodeKind::Fraction);
    assert!(
        nested_fraction
            .position
            .resolve(&nested_fraction.tree)
            .is_some()
    );
    assert_tree_is_document_valid(nested_fraction.tree);
}

#[test]
fn text_container_placeholder_becomes_text_content() {
    let mut original = empty_row();
    let text_id = NodeId::generate();
    let placeholder_id = NodeId::generate();
    original.root.children.push(MathNode {
        id: text_id.clone(),
        kind: MathNodeKind::Text,
        text: None,
        attributes: BTreeMap::from([("style".to_owned(), "rm".to_owned())]),
        source_range: None,
        children: vec![MathNode {
            id: placeholder_id.clone(),
            kind: MathNodeKind::Placeholder,
            text: None,
            attributes: BTreeMap::new(),
            source_range: None,
            children: Vec::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    });
    let position = MathPos {
        path: vec![original.root.id.clone(), text_id.clone(), placeholder_id],
        offset: 0,
    };
    let Ok(result) = apply_math_edit(
        &original,
        &position,
        MathEdit::InsertText("中文，。".to_owned()),
    ) else {
        panic!("text placeholders should accept ordinary text content");
    };
    let content = &result.tree.root.children[0].children[0];
    assert_eq!(content.kind, MathNodeKind::TextContent);
    assert_eq!(content.text.as_deref(), Some("中文，。"));
    assert_tree_is_document_valid(result.tree);
}

#[test]
fn direct_matrix_row_edit_is_rejected_without_mutating_the_tree() {
    let mut original = empty_row();
    let matrix_id = NodeId::generate();
    let row_id = NodeId::generate();
    original.root.children.push(MathNode {
        id: matrix_id.clone(),
        kind: MathNodeKind::Matrix,
        text: None,
        attributes: BTreeMap::from([
            ("style".to_owned(), "aligned".to_owned()),
            ("columns".to_owned(), "lc".to_owned()),
        ]),
        source_range: None,
        children: vec![MathNode {
            id: row_id.clone(),
            kind: MathNodeKind::Row,
            text: None,
            attributes: BTreeMap::new(),
            source_range: None,
            children: Vec::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    });
    let before = original.clone();
    let position = MathPos {
        path: vec![original.root.id.clone(), matrix_id, row_id],
        offset: 0,
    };
    assert_eq!(
        apply_math_edit(&original, &position, MathEdit::InsertText("x".to_owned())),
        Err(MathEditError::UnsupportedTarget)
    );
    assert_eq!(original, before);
}

#[test]
fn radical_insertion_enters_the_radicand_and_keeps_an_index_slot() {
    let original = empty_row();
    let position = MathPos {
        path: vec![original.root.id.clone()],
        offset: 0,
    };
    let Ok(result) = apply_math_edit(&original, &position, MathEdit::InsertRadical) else {
        panic!("radical should insert into an empty row");
    };
    let radical = &result.tree.root.children[0];
    assert_eq!(radical.kind, MathNodeKind::Radical);
    assert_eq!(radical.children[0].kind, MathNodeKind::Placeholder);
    assert_eq!(radical.children[1].kind, MathNodeKind::Placeholder);
    assert_eq!(result.position.path.last(), Some(&radical.children[1].id));
    assert!(result.position.resolve(&result.tree).is_some());
    assert_tree_is_document_valid(result.tree);
}

#[test]
fn unsupported_text_is_rejected_without_changing_the_original_tree() {
    let original = empty_row();
    let before = original.clone();
    let position = MathPos {
        path: vec![original.root.id.clone()],
        offset: 0,
    };
    assert_eq!(
        apply_math_edit(&original, &position, MathEdit::InsertText("\0".to_owned())),
        Err(MathEditError::InvalidText)
    );
    assert_eq!(original, before);
}

#[test]
fn parser_input_uses_latex_rust_structure_without_rewriting_import_provenance() {
    let original = empty_row();
    let before = original.clone();
    let position = MathPos {
        path: vec![original.root.id.clone()],
        offset: 0,
    };
    for input in ["$x$", "a b", r"\alpha"] {
        let result = apply_math_edit(&original, &position, MathEdit::InsertText(input.to_owned()))
            .unwrap_or_else(|error| panic!("parser input should be accepted: {error}"));
        assert_eq!(result.tree.source, original.source);
        assert!(result.position.resolve(&result.tree).is_some());
    }
    assert!(original.root.children.is_empty());
    assert_eq!(original, before);
}

#[test]
fn insertion_before_existing_structure_keeps_followup_input_at_the_new_cursor() {
    let original = empty_row();
    let row_position = MathPos {
        path: vec![original.root.id.clone()],
        offset: 0,
    };
    let with_fraction = apply_math_edit(&original, &row_position, MathEdit::InsertFraction)
        .unwrap_or_else(|error| panic!("fraction insertion failed: {error}"));
    let before_fraction = MathPos {
        path: vec![with_fraction.tree.root.id.clone()],
        offset: 0,
    };
    let inserted = apply_math_edit(
        &with_fraction.tree,
        &before_fraction,
        MathEdit::InsertText("x".to_owned()),
    )
    .unwrap_or_else(|error| panic!("text insertion before fraction failed: {error}"));
    let continued = apply_math_edit(
        &inserted.tree,
        &inserted.position,
        MathEdit::InsertText("y".to_owned()),
    )
    .unwrap_or_else(|error| panic!("continued input moved to the wrong slot: {error}"));
    assert_eq!(continued.tree.root.children.len(), 2);
    assert_eq!(continued.tree.root.children[0].text.as_deref(), Some("xy"));
    assert_eq!(continued.tree.root.children[1].kind, MathNodeKind::Fraction);
    assert_eq!(continued.tree.source, original.source);
    assert!(continued.position.resolve(&continued.tree).is_some());
}

#[test]
fn parser_chunks_insert_at_interior_ordinary_symbol_positions() {
    for (chunk, inserted_kind) in [
        ("+", MathNodeKind::Symbol),
        (r"\alpha", MathNodeKind::Symbol),
        (r"\frac{x}{y}", MathNodeKind::Fraction),
    ] {
        let mut original = empty_row();
        let symbol_id = NodeId::generate();
        original.root.children.push(MathNode {
            id: symbol_id.clone(),
            kind: MathNodeKind::Symbol,
            text: Some("ab".to_owned()),
            attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
            source_range: None,
            children: Vec::new(),
            extra: BTreeMap::new(),
        });
        let position = MathPos {
            path: vec![original.root.id.clone(), symbol_id.clone()],
            offset: 1,
        };

        let result = apply_math_edit(&original, &position, MathEdit::InsertText(chunk.to_owned()))
            .unwrap_or_else(|error| panic!("chunk should insert at an interior position: {error}"));

        assert_eq!(result.tree.root.children.len(), 3);
        assert_eq!(result.tree.root.children[0].id, symbol_id);
        assert_eq!(result.tree.root.children[0].text.as_deref(), Some("a"));
        assert_eq!(result.tree.root.children[1].kind, inserted_kind);
        if chunk == "+" {
            assert_eq!(
                result.tree.root.children[1]
                    .attributes
                    .get("atomClass")
                    .map(String::as_str),
                Some("bin")
            );
        }
        assert_eq!(result.tree.root.children[2].text.as_deref(), Some("b"));
        assert!(result.position.resolve(&result.tree).is_some());
        assert_eq!(result.tree.source, original.source);
    }
}

#[test]
fn interior_symbol_splits_reject_unknown_metadata_atomically() {
    for source_range_extra in [false, true] {
        let mut original = empty_row();
        original.source = "ab".to_owned();
        let symbol_id = NodeId::generate();
        original.root.children.push(MathNode {
            id: symbol_id.clone(),
            kind: MathNodeKind::Symbol,
            text: Some("ab".to_owned()),
            attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
            source_range: source_range_extra.then(|| MathSourceRange {
                start: 0,
                end: 2,
                extra: BTreeMap::from([(
                    "vendorRangeHint".to_owned(),
                    serde_json::Value::String("preserve-me".to_owned()),
                )]),
            }),
            children: Vec::new(),
            extra: if source_range_extra {
                BTreeMap::new()
            } else {
                BTreeMap::from([(
                    "vendorSymbolHint".to_owned(),
                    serde_json::Value::String("whole-symbol".to_owned()),
                )])
            },
        });
        let position = MathPos {
            path: vec![original.root.id.clone(), symbol_id],
            offset: 1,
        };
        let before = original.clone();

        assert_eq!(
            apply_math_edit(&original, &position, MathEdit::InsertText("+".to_owned())),
            Err(MathEditError::UnsupportedTarget)
        );
        assert_eq!(original, before);
    }
}

#[test]
fn fraction_and_radical_can_be_inserted_inside_ordinary_symbol_text() {
    for (edit, structure_kind, focus_slot) in [
        (MathEdit::InsertFraction, MathNodeKind::Fraction, 0usize),
        (MathEdit::InsertRadical, MathNodeKind::Radical, 1usize),
    ] {
        let mut original = empty_row();
        let symbol_id = NodeId::generate();
        original.root.children.push(MathNode {
            id: symbol_id.clone(),
            kind: MathNodeKind::Symbol,
            text: Some("ab".to_owned()),
            attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
            source_range: None,
            children: Vec::new(),
            extra: BTreeMap::new(),
        });
        let position = MathPos {
            path: vec![original.root.id.clone(), symbol_id.clone()],
            offset: 1,
        };

        let result = apply_math_edit(&original, &position, edit).unwrap_or_else(|error| {
            panic!("structure insertion failed at interior caret: {error}")
        });

        assert_eq!(result.tree.root.children.len(), 3);
        assert_eq!(result.tree.root.children[0].id, symbol_id);
        assert_eq!(result.tree.root.children[0].text.as_deref(), Some("a"));
        assert_eq!(result.tree.root.children[1].kind, structure_kind);
        assert_eq!(result.tree.root.children[2].text.as_deref(), Some("b"));
        assert_eq!(
            result.position.path.last(),
            Some(&result.tree.root.children[1].children[focus_slot].id)
        );
        assert!(result.position.resolve(&result.tree).is_some());
    }
}

#[test]
fn parser_chunk_can_split_an_atom_inside_fraction_or_radical_operand() {
    for structure_edit in [MathEdit::InsertFraction, MathEdit::InsertRadical] {
        let original = empty_row();
        let row_position = MathPos {
            path: vec![original.root.id.clone()],
            offset: 0,
        };
        let structure = apply_math_edit(&original, &row_position, structure_edit)
            .unwrap_or_else(|error| panic!("structure setup failed: {error}"));
        let structure_id = structure.tree.root.children[0].id.clone();
        let target_slot = if structure.tree.root.children[0].kind == MathNodeKind::Fraction {
            0
        } else {
            1
        };
        let slot_id = structure.tree.root.children[0].children[target_slot]
            .id
            .clone();
        let text_position = MathPos {
            path: vec![
                structure.tree.root.id.clone(),
                structure_id.clone(),
                slot_id,
            ],
            offset: 0,
        };
        let text = apply_math_edit(
            &structure.tree,
            &text_position,
            MathEdit::InsertText("ab".to_owned()),
        )
        .unwrap_or_else(|error| panic!("operand setup failed: {error}"));
        let atom_id = text.tree.root.children[0].children[target_slot].id.clone();
        let middle = MathPos {
            path: vec![text.tree.root.id.clone(), structure_id.clone(), atom_id],
            offset: 1,
        };

        let result = apply_math_edit(&text.tree, &middle, MathEdit::InsertText("+".to_owned()))
            .unwrap_or_else(|error| panic!("chunk insertion into fixed operand failed: {error}"));

        let operand = &result.tree.root.children[0].children[target_slot];
        assert_eq!(operand.kind, MathNodeKind::Row);
        assert_eq!(operand.children[0].text.as_deref(), Some("a"));
        assert_eq!(
            operand.children[1]
                .attributes
                .get("atomClass")
                .map(String::as_str),
            Some("bin")
        );
        assert_eq!(operand.children[2].text.as_deref(), Some("b"));
        assert!(result.position.resolve(&result.tree).is_some());
    }
}

#[test]
fn interior_parser_insertion_splits_only_at_grapheme_boundaries() {
    let mut original = empty_row();
    let symbol_id = NodeId::generate();
    original.root.children.push(MathNode {
        id: symbol_id.clone(),
        kind: MathNodeKind::Symbol,
        text: Some("a\u{301}b".to_owned()),
        attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
        source_range: None,
        children: Vec::new(),
        extra: BTreeMap::new(),
    });
    let middle = MathPos {
        path: vec![original.root.id.clone(), symbol_id.clone()],
        offset: "a\u{301}".len(),
    };

    let result = apply_math_edit(&original, &middle, MathEdit::InsertText("+".to_owned()))
        .unwrap_or_else(|error| panic!("grapheme-boundary insertion failed: {error}"));

    assert_eq!(result.tree.root.children[0].id, symbol_id);
    assert_eq!(
        result.tree.root.children[0].text.as_deref(),
        Some("a\u{301}")
    );
    assert_eq!(result.tree.root.children[2].text.as_deref(), Some("b"));
    assert!(result.position.resolve(&result.tree).is_some());
}

#[test]
fn invalid_position_and_unsupported_node_are_rejected_atomically() {
    let original = empty_row();
    let missing = MathPos {
        path: vec![NodeId::generate()],
        offset: 0,
    };
    assert_eq!(
        apply_math_edit(&original, &missing, MathEdit::InsertFraction),
        Err(MathEditError::InvalidPosition)
    );
    let outside_child_range = MathPos {
        path: vec![original.root.id.clone()],
        offset: 1,
    };
    assert_eq!(
        apply_math_edit(&original, &outside_child_range, MathEdit::InsertFraction),
        Err(MathEditError::InvalidPosition)
    );
    assert!(original.root.children.is_empty());
}

#[test]
fn text_insertion_snaps_past_a_combining_character_joined_to_the_right() {
    let mut original = empty_row();
    original.root.children.push(MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Symbol,
        text: Some("\u{301}x".to_owned()),
        attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
        source_range: None,
        children: Vec::new(),
        extra: BTreeMap::new(),
    });
    let position = MathPos {
        path: vec![
            original.root.id.clone(),
            original.root.children[0].id.clone(),
        ],
        offset: 0,
    };
    let Ok(result) = apply_math_edit(&original, &position, MathEdit::InsertText("e".to_owned()))
    else {
        panic!("text should insert before a combining sequence");
    };
    assert_eq!(
        result.tree.root.children[0].text.as_deref(),
        Some("e\u{301}x")
    );
    assert_eq!(result.position.offset, "e\u{301}".len());
    assert!(result.position.resolve(&result.tree).is_some());
}

#[test]
fn text_edit_rejects_single_and_cumulative_text_over_budget_atomically() {
    let original = empty_row();
    let position = MathPos {
        path: vec![original.root.id.clone()],
        offset: 0,
    };
    let oversized = "x".repeat(caret_doc::MAX_MATH_TREE_PAYLOAD_BYTES + 1);
    assert_eq!(
        apply_math_edit(&original, &position, MathEdit::InsertText(oversized)),
        Err(MathEditError::InvalidText)
    );
    assert!(original.root.children.is_empty());

    let mut full_tree = original.clone();
    let text_len = caret_doc::MAX_MATH_TREE_PAYLOAD_BYTES
        - full_tree.root.id.as_str().len()
        - "future".len()
        - 1;
    full_tree.root.extra.insert(
        "future".to_owned(),
        serde_json::Value::String("x".repeat(text_len)),
    );
    assert!(full_tree.validate().is_ok());
    assert_eq!(
        apply_math_edit(&full_tree, &position, MathEdit::InsertText("y".to_owned())),
        Err(MathEditError::InvalidTree)
    );
    assert!(full_tree.root.children.is_empty());

    let mut full_content = empty_row();
    let text_id = NodeId::generate();
    let current_text = "x".repeat(600 * 1024);
    full_content.root.children.push(MathNode {
        id: text_id.clone(),
        kind: MathNodeKind::TextContent,
        text: Some(current_text.clone()),
        attributes: BTreeMap::new(),
        source_range: None,
        children: Vec::new(),
        extra: BTreeMap::new(),
    });
    let text_position = MathPos {
        path: vec![full_content.root.id.clone(), text_id],
        offset: current_text.len(),
    };
    assert!(full_content.validate().is_ok());
    assert_eq!(
        apply_math_edit(
            &full_content,
            &text_position,
            MathEdit::InsertText("y".repeat(600 * 1024))
        ),
        Err(MathEditError::InvalidTree)
    );
    assert_eq!(
        full_content.root.children[0].text.as_deref(),
        Some(current_text.as_str())
    );
}

#[test]
fn text_edits_do_not_silently_drop_unknown_source_range_fields() {
    for kind in [MathNodeKind::Symbol, MathNodeKind::TextContent] {
        let mut original = empty_row();
        let mut attributes = BTreeMap::new();
        if kind == MathNodeKind::Symbol {
            attributes.insert("atomClass".to_owned(), "ord".to_owned());
        }
        original.source = "x".to_owned();
        original.root.children.push(MathNode {
            id: NodeId::generate(),
            kind,
            text: Some("x".to_owned()),
            attributes,
            source_range: Some(MathSourceRange {
                start: 0,
                end: 1,
                extra: BTreeMap::from([(
                    "vendorRangeHint".to_owned(),
                    serde_json::Value::String("preserve-me".to_owned()),
                )]),
            }),
            children: Vec::new(),
            extra: BTreeMap::new(),
        });
        let position = MathPos {
            path: vec![
                original.root.id.clone(),
                original.root.children[0].id.clone(),
            ],
            offset: 1,
        };
        let before = original.clone();

        assert_eq!(
            apply_math_edit(&original, &position, MathEdit::InsertText("y".to_owned())),
            Err(MathEditError::UnsupportedTarget)
        );
        assert_eq!(original, before);
    }
}

#[test]
fn merged_ordinary_text_cannot_exceed_the_parser_expansion_budget() {
    let mut original = empty_row();
    original.root.children.push(MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Symbol,
        text: Some("x".repeat(caret_math::MAX_MATH_SYNTAX_NODES - 1)),
        attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
        source_range: None,
        children: Vec::new(),
        extra: BTreeMap::new(),
    });
    let position = MathPos {
        path: vec![
            original.root.id.clone(),
            original.root.children[0].id.clone(),
        ],
        offset: caret_math::MAX_MATH_SYNTAX_NODES - 1,
    };
    assert_eq!(
        apply_math_edit(&original, &position, MathEdit::InsertText("x".to_owned())),
        Err(MathEditError::InvalidTree)
    );
    assert_eq!(
        original.root.children[0].text.as_deref().map(str::len),
        Some(caret_math::MAX_MATH_SYNTAX_NODES - 1)
    );
}

#[test]
fn root_ordinary_run_counts_the_parser_row_created_after_coalescing() {
    let original = MathTree {
        schema_version: MATH_TREE_SCHEMA_VERSION,
        source: "x".repeat(caret_math::MAX_MATH_SYNTAX_NODES - 1),
        root: MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Symbol,
            text: Some("x".repeat(caret_math::MAX_MATH_SYNTAX_NODES - 1)),
            attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
            source_range: None,
            children: Vec::new(),
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    assert!(original.validate().is_ok());
    let position = MathPos {
        path: vec![original.root.id.clone()],
        offset: caret_math::MAX_MATH_SYNTAX_NODES - 1,
    };
    assert_eq!(
        apply_math_edit(&original, &position, MathEdit::InsertText("x".to_owned())),
        Err(MathEditError::InvalidTree)
    );
}

#[test]
fn fixed_operand_slots_budget_the_parser_row_wrappers() {
    let symbol = |text: String| MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Symbol,
        text: Some(text),
        attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
        source_range: None,
        children: Vec::new(),
        extra: BTreeMap::new(),
    };
    let tree = MathTree {
        schema_version: MATH_TREE_SCHEMA_VERSION,
        source: String::new(),
        root: MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Fraction,
            text: None,
            attributes: BTreeMap::from([("style".to_owned(), "frac".to_owned())]),
            source_range: None,
            children: vec![
                symbol("x".repeat(caret_math::MAX_MATH_SYNTAX_NODES / 2)),
                symbol("y".repeat(caret_math::MAX_MATH_SYNTAX_NODES / 2)),
            ],
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    assert!(tree.validate().is_ok());
    let position = MathPos {
        path: vec![tree.root.id.clone(), tree.root.children[0].id.clone()],
        offset: 0,
    };
    assert_eq!(
        apply_math_edit(&tree, &position, MathEdit::InsertText("x".to_owned())),
        Err(MathEditError::InvalidTree)
    );
}

#[test]
fn matrix_cell_rows_budget_their_parser_wrapper() {
    let tree = MathTree {
        schema_version: MATH_TREE_SCHEMA_VERSION,
        source: String::new(),
        root: MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Matrix,
            text: None,
            attributes: BTreeMap::from([
                ("style".to_owned(), "matrix".to_owned()),
                ("columns".to_owned(), "l".to_owned()),
            ]),
            source_range: None,
            children: vec![MathNode {
                id: NodeId::generate(),
                kind: MathNodeKind::Row,
                text: None,
                attributes: BTreeMap::new(),
                source_range: None,
                children: vec![MathNode {
                    id: NodeId::generate(),
                    kind: MathNodeKind::Symbol,
                    text: Some("x".repeat(caret_math::MAX_MATH_SYNTAX_NODES - 3)),
                    attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
                    source_range: None,
                    children: Vec::new(),
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    assert!(tree.validate().is_ok());
    let position = MathPos {
        path: vec![
            tree.root.id.clone(),
            tree.root.children[0].id.clone(),
            tree.root.children[0].children[0].id.clone(),
        ],
        offset: 0,
    };
    assert_eq!(
        apply_math_edit(&tree, &position, MathEdit::InsertText("x".to_owned())),
        Err(MathEditError::InvalidTree)
    );
}

#[test]
fn existing_atom_class_is_trusted_while_new_input_uses_parser_classification() {
    let mut original = empty_row();
    original.root.children.push(MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Symbol,
        text: Some("+".to_owned()),
        attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
        source_range: None,
        children: Vec::new(),
        extra: BTreeMap::new(),
    });
    let position = MathPos {
        path: vec![
            original.root.id.clone(),
            original.root.children[0].id.clone(),
        ],
        offset: 1,
    };
    let Ok(result) = apply_math_edit(&original, &position, MathEdit::InsertText("x".to_owned()))
    else {
        panic!("the persisted atom class is authoritative for existing content");
    };
    assert_eq!(result.tree.root.children[0].text.as_deref(), Some("+x"));
}

#[test]
fn deeply_nested_unknown_extensions_are_rejected_before_tree_clone() {
    let mut original = empty_row();
    let mut extension = serde_json::Value::Null;
    for _ in 0..=caret_doc::MAX_DOCUMENT_NESTING_DEPTH {
        extension = serde_json::Value::Array(vec![extension]);
    }
    original.root.extra.insert("future".to_owned(), extension);
    let position = MathPos {
        path: vec![original.root.id.clone()],
        offset: 0,
    };
    assert_eq!(
        apply_math_edit(&original, &position, MathEdit::InsertText("x".to_owned())),
        Err(MathEditError::InvalidTree)
    );
    assert!(original.root.children.is_empty());
}

#[test]
fn valid_nested_fraction_slots_do_not_get_phantom_parser_rows() {
    let placeholder = || MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Placeholder,
        text: None,
        attributes: BTreeMap::new(),
        source_range: None,
        children: Vec::new(),
        extra: BTreeMap::new(),
    };
    let fraction = |numerator| MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Fraction,
        text: None,
        attributes: BTreeMap::from([("style".to_owned(), "frac".to_owned())]),
        source_range: None,
        children: vec![numerator, placeholder()],
        extra: BTreeMap::new(),
    };
    let mut nested = fraction(placeholder());
    for _ in 0..15 {
        nested = fraction(nested);
    }
    let mut original = empty_row();
    let mut path = vec![original.root.id.clone()];
    path.push(nested.id.clone());
    let mut current = &nested;
    for _ in 0..15 {
        current = &current.children[0];
        path.push(current.id.clone());
    }
    path.push(current.children[0].id.clone());
    original.root.children.push(nested);
    let position = MathPos { path, offset: 0 };
    let Ok(result) = apply_math_edit(&original, &position, MathEdit::InsertText("x".to_owned()))
    else {
        panic!("valid nested one-character operands should stay within backend depth");
    };
    assert!(result.position.resolve(&result.tree).is_some());
}
