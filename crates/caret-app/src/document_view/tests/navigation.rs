//! 验证指针选区、命中图与视觉光标导航。

use super::*;
use caret_doc::{BlockEnvironment, BlockEnvironmentKind, NodeId, Paragraph, Run};
use std::collections::BTreeMap;

fn paragraph_with_text(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(text_flow(Run {
        text: text.to_owned(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }));
    paragraph
}

fn section(title: &str, body: &str) -> Block {
    Block::Environment(BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(paragraph_with_text(title)),
        children: vec![Block::Paragraph(paragraph_with_text(body))],
        extra: BTreeMap::new(),
    })
}

fn section_title_id(environment: &BlockEnvironment) -> Result<NodeId, std::io::Error> {
    environment
        .title
        .as_ref()
        .map(|title| title.id.clone())
        .ok_or_else(|| std::io::Error::other("Section title should exist"))
}

fn block_text(block: &Block) -> Option<&str> {
    let Block::Paragraph(paragraph) = block else {
        return None;
    };
    paragraph.inlines.iter().find_map(|inline| match inline {
        ParagraphContent::Text(flow) => flow.runs.first().map(|run| run.text.as_str()),
        _ => None,
    })
}

fn align_text_tree(text: &str) -> caret_doc::MathTree {
    caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![caret_doc::MathContent::Text(caret_doc::TextFlow {
                runs: vec![Run {
                    text: text.to_owned(),
                    language: None,
                    props: BTreeMap::new(),
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }
}

fn inline_math_tree() -> caret_doc::MathTree {
    caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![caret_doc::MathContent::Math(caret_doc::MathNode {
                id: NodeId::generate(),
                kind: caret_doc::MathNodeKind::Symbol,
                text: Some("x".to_owned()),
                attributes: BTreeMap::new(),
                children: Vec::new(),
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }
}

fn symbol_flow(text: &str) -> caret_doc::MathFlow {
    caret_doc::MathFlow {
        id: NodeId::generate(),
        content: vec![caret_doc::MathContent::Math(caret_doc::MathNode {
            id: NodeId::generate(),
            kind: caret_doc::MathNodeKind::Symbol,
            text: Some(text.to_owned()),
            attributes: BTreeMap::new(),
            children: Vec::new(),
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    }
}

fn symbol_content(text: &str) -> caret_doc::MathContent {
    symbol_flow_with_atom_class(text)
        .content
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("symbol flow should contain one symbol"))
}

fn symbol_flow_with_atom_class(text: &str) -> caret_doc::MathFlow {
    let mut flow = symbol_flow(text);
    if let Some(caret_doc::MathContent::Math(symbol)) = flow.content.first_mut() {
        symbol
            .attributes
            .insert("atomClass".to_owned(), "ord".to_owned());
    }
    flow
}

fn inline_subsup_document(superscript: Option<&str>) -> (Document, NodeId, NodeId) {
    let paragraph_id = NodeId::generate();
    let math_id = NodeId::generate();
    let mut root = caret_doc::MathFlow {
        id: NodeId::generate(),
        content: ["s", "a", "d"].into_iter().map(symbol_content).collect(),
        extra: BTreeMap::new(),
    };
    root.content
        .push(caret_doc::MathContent::Math(caret_doc::MathNode {
            id: NodeId::generate(),
            kind: caret_doc::MathNodeKind::SubSup,
            text: None,
            attributes: BTreeMap::new(),
            children: vec![
                symbol_flow_with_atom_class("a"),
                symbol_flow_with_atom_class("a"),
                superscript.map_or_else(
                    || caret_doc::MathFlow {
                        id: NodeId::generate(),
                        content: Vec::new(),
                        extra: BTreeMap::new(),
                    },
                    symbol_flow_with_atom_class,
                ),
            ],
            extra: BTreeMap::new(),
        }));
    root.content.push(symbol_content("a"));

    let mut paragraph = Paragraph::empty(paragraph_id.clone());
    paragraph
        .inlines
        .push(ParagraphContent::Math(caret_doc::InlineMath {
            id: math_id.clone(),
            math_tree: caret_doc::MathTree {
                schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                root,
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        }));
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    (document, paragraph_id, math_id)
}

fn inline_superscript_document() -> (Document, NodeId, NodeId) {
    let paragraph_id = NodeId::generate();
    let math_id = NodeId::generate();
    let root = caret_doc::MathFlow {
        id: NodeId::generate(),
        content: vec![caret_doc::MathContent::Math(caret_doc::MathNode {
            id: NodeId::generate(),
            kind: caret_doc::MathNodeKind::Superscript,
            text: None,
            attributes: BTreeMap::new(),
            children: vec![
                symbol_flow_with_atom_class("x"),
                symbol_flow_with_atom_class("a"),
            ],
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    };
    let mut paragraph = Paragraph::empty(paragraph_id.clone());
    paragraph
        .inlines
        .push(ParagraphContent::Math(caret_doc::InlineMath {
            id: math_id.clone(),
            math_tree: caret_doc::MathTree {
                schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                root,
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        }));
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    (document, paragraph_id, math_id)
}

fn placeholder_flow() -> caret_doc::MathFlow {
    caret_doc::MathFlow {
        id: NodeId::generate(),
        content: vec![caret_doc::MathContent::Math(caret_doc::MathNode {
            id: NodeId::generate(),
            kind: caret_doc::MathNodeKind::Placeholder,
            text: None,
            attributes: BTreeMap::new(),
            children: Vec::new(),
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    }
}

fn fraction_document() -> (Document, NodeId) {
    let block_id = NodeId::generate();
    let math_tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![caret_doc::MathContent::Math(caret_doc::MathNode {
                id: NodeId::generate(),
                kind: caret_doc::MathNodeKind::Fraction,
                text: None,
                attributes: BTreeMap::from([("style".to_owned(), "frac".to_owned())]),
                children: vec![symbol_flow("a"), symbol_flow("b")],
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    let mut document = Document::blank();
    document.body.push(Block::Equation(caret_doc::Equation {
        id: block_id.clone(),
        math_tree,
        extra: BTreeMap::new(),
    }));
    (document, block_id)
}

fn math_text_content(text: &str) -> caret_doc::MathContent {
    caret_doc::MathContent::Text(caret_doc::TextFlow {
        runs: vec![Run {
            text: text.to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    })
}

fn align_document(cells: &[&str]) -> Document {
    let align = caret_doc::Align {
        id: NodeId::generate(),
        rows: cells
            .iter()
            .map(|text| caret_doc::MathAlignRow {
                id: NodeId::generate(),
                cells: vec![align_text_tree(text)],
                extra: BTreeMap::new(),
            })
            .collect(),
        extra: BTreeMap::new(),
    };
    let mut document = Document::blank();
    document.body.push(Block::Align(align));
    document
}

fn formula_block(align: bool) -> (Block, NodeId) {
    let id = NodeId::generate();
    let tree = align_text_tree("x");
    let block = if align {
        Block::Align(caret_doc::Align {
            id: id.clone(),
            rows: vec![caret_doc::MathAlignRow {
                id: NodeId::generate(),
                cells: vec![tree],
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        })
    } else {
        Block::Equation(caret_doc::Equation {
            id: id.clone(),
            math_tree: tree,
            extra: BTreeMap::new(),
        })
    };
    (block, id)
}

fn set_body_caret(
    runtime: &mut DocumentViewRuntime,
    paragraph_id: NodeId,
    offset: usize,
) -> Result<(), caret_engine::EditError> {
    let position = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id,
        offset,
        affinity: if offset == 0 {
            caret_engine::Affinity::Downstream
        } else {
            caret_engine::Affinity::Upstream
        },
    };
    runtime
        .session
        .run(caret_engine::Command::SetSelection(Some(
            caret_engine::Selection {
                anchor: position.clone(),
                focus: position,
            },
        )))?;
    Ok(())
}

#[test]
fn horizontal_navigation_enters_adjacent_inline_and_display_math_from_both_sides()
-> Result<(), Box<dyn std::error::Error>> {
    for (formula_kind, align) in [("inline", false), ("equation", false), ("align", true)] {
        for forward in [false, true] {
            for affinity in [
                caret_engine::Affinity::Downstream,
                caret_engine::Affinity::Upstream,
            ] {
                let mut document = Document::blank();
                let mut before = paragraph_with_text("before");
                let before_id = before.id.clone();
                let after = paragraph_with_text("after");
                let after_id = after.id.clone();

                let target_id = if formula_kind == "inline" {
                    let math_id = NodeId::generate();
                    before
                        .inlines
                        .push(ParagraphContent::Math(caret_doc::InlineMath {
                            id: math_id.clone(),
                            math_tree: inline_math_tree(),
                            extra: BTreeMap::new(),
                        }));
                    document.body = vec![Block::Paragraph(before)];
                    math_id
                } else {
                    let (formula, formula_id) = formula_block(align);
                    document.body =
                        vec![Block::Paragraph(before), formula, Block::Paragraph(after)];
                    formula_id
                };

                let mut runtime = DocumentViewRuntime::new(document)?;
                let (paragraph_id, offset) = if forward {
                    (before_id, "before".len())
                } else if formula_kind == "inline" {
                    (before_id, "before".len() + '\u{fffc}'.len_utf8())
                } else {
                    (after_id, 0)
                };
                set_body_caret(&mut runtime, paragraph_id, offset)?;
                let mut position = runtime.session.selection().focus.clone();
                position.affinity = affinity;
                runtime
                    .session
                    .run(caret_engine::Command::SetSelection(Some(
                        caret_engine::Selection {
                            anchor: position.clone(),
                            focus: position,
                        },
                    )))?;
                runtime.move_visual_caret_json(forward, false)?;

                if formula_kind != "inline" {
                    let exit_direction = if forward {
                        caret_engine::Direction::Backward
                    } else {
                        caret_engine::Direction::Forward
                    };
                    assert!(runtime.session.math_edit().is_none());
                    assert_eq!(
                        runtime.session.block_boundary_exit(),
                        Some((target_id.clone(), exit_direction))
                    );
                    let caret: Value = serde_json::from_str(&runtime.current_caret_json()?)?;
                    let page_index = caret["pageIndex"].as_u64().unwrap_or_default() as usize;
                    let expected_x = runtime
                        .current_snapshot()?
                        .block_boundary_x(&target_id, page_index, exit_direction)
                        .ok_or_else(|| std::io::Error::other("math block edge is missing"))?;
                    assert_eq!(caret["xPt"].as_f64(), Some(expected_x));
                    runtime.move_visual_caret_json(forward, false)?;
                }

                let edit = runtime
                    .session
                    .math_edit()
                    .ok_or_else(|| std::io::Error::other("arrow did not enter adjacent math"))?;
                match (&edit.target, formula_kind) {
                    (caret_engine::MathEditTarget::Inline { math_id, .. }, "inline") => {
                        assert_eq!(math_id, &target_id);
                    }
                    (
                        caret_engine::MathEditTarget::Block { block_id, align },
                        "equation" | "align",
                    ) => {
                        assert_eq!(block_id, &target_id);
                        assert_eq!(*align, formula_kind == "align");
                    }
                    _ => return Err("navigation entered the wrong math host".into()),
                }
            }
        }
    }
    Ok(())
}

#[test]
fn vertical_navigation_enters_and_exits_equation_and_multiline_align_blocks()
-> Result<(), Box<dyn std::error::Error>> {
    for align in [false, true] {
        let mut document = Document::blank();
        let before = paragraph_with_text("before");
        let before_id = before.id.clone();
        let after = paragraph_with_text("after");
        let after_id = after.id.clone();
        let block_id = NodeId::generate();
        let formula = if align {
            Block::Align(caret_doc::Align {
                id: block_id.clone(),
                rows: ["x", "y"]
                    .into_iter()
                    .map(|text| caret_doc::MathAlignRow {
                        id: NodeId::generate(),
                        cells: vec![align_text_tree(text)],
                        extra: BTreeMap::new(),
                    })
                    .collect(),
                extra: BTreeMap::new(),
            })
        } else {
            Block::Equation(caret_doc::Equation {
                id: block_id.clone(),
                math_tree: align_text_tree("x"),
                extra: BTreeMap::new(),
            })
        };
        document.body = vec![Block::Paragraph(before), formula, Block::Paragraph(after)];

        let mut from_above = DocumentViewRuntime::new(document.clone())?;
        set_body_caret(&mut from_above, before_id.clone(), 2)?;
        from_above.move_vertical_caret_json(1, false)?;
        assert!(from_above.session.math_edit().is_none());
        assert_eq!(
            from_above.session.block_boundary_exit(),
            Some((block_id.clone(), caret_engine::Direction::Backward))
        );
        from_above.move_vertical_caret_json(1, false)?;
        assert!(matches!(
            from_above.session.math_edit().map(|edit| &edit.target),
            Some(caret_engine::MathEditTarget::Block {
                block_id: active,
                align: active_align,
            }) if active == &block_id && *active_align == align
        ));
        if align {
            from_above.move_vertical_caret_json(1, false)?;
            assert!(matches!(
                from_above.session.math_edit().map(|edit| &edit.target),
                Some(caret_engine::MathEditTarget::Block { block_id: active, align: true })
                    if active == &block_id
            ));
        }
        from_above.move_vertical_caret_json(1, false)?;
        assert!(from_above.session.math_edit().is_none());
        assert_eq!(from_above.session.selection().focus.paragraph_id, after_id);
        assert_eq!(
            from_above.session.block_boundary_exit(),
            Some((block_id.clone(), caret_engine::Direction::Forward))
        );
        from_above.move_vertical_caret_json(-1, false)?;
        assert!(matches!(
            from_above.session.math_edit().map(|edit| &edit.target),
            Some(caret_engine::MathEditTarget::Block { block_id: active, align: active_align })
                if active == &block_id && *active_align == align
        ));

        let mut from_below = DocumentViewRuntime::new(document)?;
        set_body_caret(&mut from_below, after_id.clone(), 2)?;
        from_below.move_vertical_caret_json(-1, false)?;
        assert!(from_below.session.math_edit().is_none());
        assert_eq!(
            from_below.session.block_boundary_exit(),
            Some((block_id.clone(), caret_engine::Direction::Forward))
        );
        from_below.move_vertical_caret_json(-1, false)?;
        assert!(matches!(
            from_below.session.math_edit().map(|edit| &edit.target),
            Some(caret_engine::MathEditTarget::Block {
                block_id: active,
                align: active_align,
            }) if active == &block_id && *active_align == align
        ));
        from_below.move_vertical_caret_json(1, false)?;
        assert!(from_below.session.math_edit().is_none());
        assert_eq!(from_below.session.selection().focus.paragraph_id, after_id);
    }
    Ok(())
}

#[test]
fn vertical_navigation_does_not_enter_adjacent_inline_math()
-> Result<(), Box<dyn std::error::Error>> {
    let mut paragraph = paragraph_with_text("x");
    paragraph
        .inlines
        .push(ParagraphContent::Math(caret_doc::InlineMath {
            id: NodeId::generate(),
            math_tree: inline_math_tree(),
            extra: BTreeMap::new(),
        }));
    let paragraph_id = paragraph.id.clone();
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];

    for (offset, line_delta) in [(1 + '\u{fffc}'.len_utf8(), -1), (1, 1)] {
        let mut runtime = DocumentViewRuntime::new(document.clone())?;
        set_body_caret(&mut runtime, paragraph_id.clone(), offset)?;
        runtime.move_vertical_caret_json(line_delta, false)?;
        assert!(runtime.session.math_edit().is_none());
    }
    Ok(())
}

#[test]
fn horizontal_navigation_enters_inline_math_in_section_title_from_both_sides()
-> Result<(), Box<dyn std::error::Error>> {
    for forward in [false, true] {
        let mut environment = match section("title", "body") {
            Block::Environment(environment) => environment,
            _ => unreachable!(),
        };
        let math_id = NodeId::generate();
        let title = environment
            .title
            .as_mut()
            .unwrap_or_else(|| panic!("section title should exist"));
        title
            .inlines
            .push(ParagraphContent::Math(caret_doc::InlineMath {
                id: math_id.clone(),
                math_tree: inline_math_tree(),
                extra: BTreeMap::new(),
            }));
        let mut document = Document::blank();
        let environment_id = environment.id.clone();
        let title_id = environment
            .title
            .as_ref()
            .unwrap_or_else(|| panic!("section title should exist"))
            .id
            .clone();
        document.body.push(Block::Environment(environment));
        let mut runtime = DocumentViewRuntime::new(document)?;
        let offset = if forward {
            "title".len()
        } else {
            "title".len() + '\u{fffc}'.len_utf8()
        };
        let position = caret_engine::Pos {
            story: caret_engine::StoryId::EnvironmentTitle(environment_id),
            paragraph_id: title_id,
            offset,
            affinity: if forward {
                caret_engine::Affinity::Upstream
            } else {
                caret_engine::Affinity::Downstream
            },
        };
        runtime
            .session
            .run(caret_engine::Command::SetSelection(Some(
                caret_engine::Selection {
                    anchor: position.clone(),
                    focus: position,
                },
            )))?;
        runtime.move_visual_caret_json(forward, false)?;
        assert!(matches!(
            runtime.session.math_edit().map(|edit| &edit.target),
            Some(caret_engine::MathEditTarget::Inline { math_id: active, .. }) if active == &math_id
        ));
    }
    Ok(())
}

#[test]
fn visual_line_edge_navigation_does_not_enter_inline_math_it_skips()
-> Result<(), Box<dyn std::error::Error>> {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph
        .inlines
        .push(ParagraphContent::Math(caret_doc::InlineMath {
            id: NodeId::generate(),
            math_tree: inline_math_tree(),
            extra: BTreeMap::new(),
        }));
    let mut document = Document::blank();
    document.body.push(Block::Paragraph(paragraph.clone()));
    let mut runtime = DocumentViewRuntime::new(document)?;
    set_body_caret(&mut runtime, paragraph.id, 0)?;

    runtime.move_to_visual_line_edge_json(true, false)?;

    assert!(runtime.session.math_edit().is_none());
    assert_eq!(
        runtime.session.selection().focus.offset,
        '\u{fffc}'.len_utf8()
    );
    Ok(())
}

#[test]
fn invalid_horizontal_candidate_does_not_enter_adjacent_display_math()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let before_id = before.id.clone();
    let after = paragraph_with_text("after");
    let after_id = after.id.clone();
    let (formula, _) = formula_block(false);
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(before), formula, Block::Paragraph(after)];
    let mut runtime = DocumentViewRuntime::new(document)?;
    set_body_caret(&mut runtime, before_id, "before".len())?;
    let document_before = runtime.session.document().clone();
    let selection_before = runtime.session.selection().clone();
    let can_undo_before = runtime.session.can_undo();
    let invalid = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id: after_id,
        offset: "after".len() + 1,
        affinity: caret_engine::Affinity::Downstream,
    };

    assert!(
        runtime
            .session
            .set_navigation_selection(
                caret_engine::Selection {
                    anchor: invalid.clone(),
                    focus: invalid,
                },
                caret_engine::VisualAffinity::Downstream,
                caret_engine::NavigationIntent::Horizontal,
            )
            .is_err()
    );

    assert_eq!(runtime.session.document(), &document_before);
    assert_eq!(runtime.session.selection(), &selection_before);
    assert!(runtime.session.math_edit().is_none());
    assert_eq!(runtime.session.can_undo(), can_undo_before);
    Ok(())
}

#[test]
fn horizontal_navigation_rejects_interior_candidate_across_display_math()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let before_id = before.id.clone();
    let after = paragraph_with_text("after");
    let after_id = after.id.clone();
    let (formula, _) = formula_block(false);
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(before), formula, Block::Paragraph(after)];
    let mut runtime = DocumentViewRuntime::new(document)?;
    set_body_caret(&mut runtime, before_id, 2)?;
    let document_before = runtime.session.document().clone();
    let selection_before = runtime.session.selection().clone();
    let interior = caret_engine::Pos {
        story: caret_engine::StoryId::Body,
        paragraph_id: after_id,
        offset: 2,
        affinity: caret_engine::Affinity::Downstream,
    };

    assert!(
        runtime
            .session
            .set_navigation_selection(
                caret_engine::Selection {
                    anchor: interior.clone(),
                    focus: interior,
                },
                caret_engine::VisualAffinity::Downstream,
                caret_engine::NavigationIntent::Horizontal,
            )
            .is_err()
    );

    assert_eq!(runtime.session.document(), &document_before);
    assert_eq!(runtime.session.selection(), &selection_before);
    assert!(runtime.session.math_edit().is_none());
    Ok(())
}

#[test]
fn horizontal_navigation_enters_display_math_at_document_edges()
-> Result<(), Box<dyn std::error::Error>> {
    for align in [false, true] {
        for forward in [false, true] {
            let expected_align = align;
            let mut document = Document::blank();
            let before = paragraph_with_text("before");
            let before_id = before.id.clone();
            let after = paragraph_with_text("after");
            let after_id = after.id.clone();
            let (formula, formula_id) = formula_block(align);
            let (paragraph, offset) = if forward {
                document.body = vec![Block::Paragraph(before), formula];
                (before_id, "before".len())
            } else {
                document.body = vec![formula, Block::Paragraph(after)];
                (after_id, 0)
            };
            let mut runtime = DocumentViewRuntime::new(document)?;
            set_body_caret(&mut runtime, paragraph, offset)?;
            runtime.move_visual_caret_json(forward, false)?;
            assert!(runtime.session.math_edit().is_none());
            assert_eq!(
                runtime.session.block_boundary_exit(),
                Some((
                    formula_id.clone(),
                    if forward {
                        caret_engine::Direction::Backward
                    } else {
                        caret_engine::Direction::Forward
                    }
                ))
            );
            runtime.move_visual_caret_json(forward, false)?;

            let Some(caret_engine::MathEditTarget::Block {
                block_id,
                align: actual_align,
            }) = runtime.session.math_edit().map(|edit| &edit.target)
            else {
                return Err("arrow at document edge did not enter adjacent display math".into());
            };
            assert_eq!(block_id, &formula_id);
            assert_eq!(*actual_align, expected_align);
        }
    }
    Ok(())
}

#[test]
fn display_math_has_a_shared_outer_stop_for_reentry_and_text_materialization()
-> Result<(), Box<dyn std::error::Error>> {
    for align in [false, true] {
        let mut document = Document::blank();
        let before = paragraph_with_text("before");
        let before_id = before.id.clone();
        let after = paragraph_with_text("after");
        let (formula, formula_id) = formula_block(align);
        document.body = vec![Block::Paragraph(before), formula, Block::Paragraph(after)];

        let mut runtime = DocumentViewRuntime::new(document)?;
        set_body_caret(&mut runtime, before_id, "before".len())?;
        runtime.move_visual_caret_json(true, false)?;
        runtime.move_visual_caret_json(true, false)?;
        runtime.move_visual_caret_json(true, false)?;
        runtime.move_visual_caret_json(true, false)?;

        assert!(runtime.session.math_edit().is_none());
        assert_eq!(
            runtime.session.block_boundary_exit(),
            Some((formula_id.clone(), caret_engine::Direction::Forward))
        );

        runtime.move_visual_caret_json(false, false)?;
        assert!(matches!(
            runtime.session.math_edit().map(|edit| &edit.target),
            Some(caret_engine::MathEditTarget::Block { block_id, align: actual_align })
                if block_id == &formula_id && *actual_align == align
        ));

        let mut pass_document = Document::blank();
        let before = paragraph_with_text("before");
        let before_id = before.id.clone();
        let after = paragraph_with_text("after");
        let after_id = after.id.clone();
        let (formula, pass_formula_id) = formula_block(align);
        pass_document.body = vec![Block::Paragraph(before), formula, Block::Paragraph(after)];
        let mut pass_runtime = DocumentViewRuntime::new(pass_document)?;
        set_body_caret(&mut pass_runtime, before_id, "before".len())?;
        pass_runtime.move_visual_caret_json(true, false)?;
        pass_runtime.move_visual_caret_json(true, false)?;
        pass_runtime.move_visual_caret_json(true, false)?;
        pass_runtime.move_visual_caret_json(true, false)?;
        pass_runtime.move_visual_caret_json(true, false)?;
        assert!(pass_runtime.session.block_boundary_exit().is_none());
        assert!(pass_runtime.session.math_edit().is_none());
        assert_eq!(
            pass_runtime.session.selection().focus.paragraph_id,
            after_id
        );
        assert_eq!(pass_runtime.session.selection().focus.offset, 0);
        assert_ne!(pass_formula_id, formula_id);

        let mut typing_document = Document::blank();
        let before = paragraph_with_text("before");
        let before_id = before.id.clone();
        let after = paragraph_with_text("after");
        let (formula, typing_formula_id) = formula_block(align);
        typing_document.body = vec![Block::Paragraph(before), formula, Block::Paragraph(after)];
        let mut typing_runtime = DocumentViewRuntime::new(typing_document)?;
        set_body_caret(&mut typing_runtime, before_id, "before".len())?;
        typing_runtime.move_visual_caret_json(true, false)?;
        typing_runtime.move_visual_caret_json(true, false)?;
        typing_runtime.move_visual_caret_json(true, false)?;
        typing_runtime.move_visual_caret_json(true, false)?;
        assert!(typing_runtime.session.block_boundary_exit().is_some());
        typing_runtime.insert_text_delta_json("X".to_owned())?;
        assert_eq!(typing_runtime.document().body.len(), 4);
        assert_eq!(block_text(&typing_runtime.document().body[2]), Some("X"));
        typing_runtime.undo_json()?;
        assert_eq!(typing_runtime.document().body.len(), 3);
        assert_eq!(
            typing_runtime.session.block_boundary_exit(),
            Some((typing_formula_id, caret_engine::Direction::Forward))
        );
        typing_runtime.redo_json()?;
        assert_eq!(typing_runtime.document().body.len(), 4);
        assert!(typing_runtime.session.block_boundary_exit().is_none());
    }
    Ok(())
}

#[test]
fn backspace_at_empty_line_after_equation_removes_line_and_enters_formula()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let (formula, _) = formula_block(false);
    let empty_line = Paragraph::empty(NodeId::generate());
    let empty_line_id = empty_line.id.clone();
    let after = paragraph_with_text("after");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        formula,
        Block::Paragraph(empty_line),
        Block::Paragraph(after),
    ];
    let before = document.clone();
    let mut runtime = DocumentViewRuntime::new(document)?;
    set_body_caret(&mut runtime, empty_line_id.clone(), 0)?;

    runtime.delete_backward_json()?;

    assert_eq!(runtime.document().body.len(), 3);
    assert_ne!(runtime.document(), &before);
    assert!(runtime.session.block_boundary_exit().is_none());
    assert!(matches!(
        runtime.session.math_edit().map(|edit| &edit.target),
        Some(caret_engine::MathEditTarget::Block { align: false, .. })
    ));
    runtime.undo_json()?;
    assert_eq!(runtime.document().body.len(), 4);
    assert!(runtime.session.math_edit().is_none());
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        empty_line_id
    );
    runtime.redo_json()?;
    assert_eq!(runtime.document().body.len(), 3);
    assert!(matches!(
        runtime.session.math_edit().map(|edit| &edit.target),
        Some(caret_engine::MathEditTarget::Block { align: false, .. })
    ));
    Ok(())
}

#[test]
fn backspace_after_consecutive_math_blocks_enters_the_last_block()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let (equation, _) = formula_block(false);
    let (align, align_id) = formula_block(true);
    let empty_line = Paragraph::empty(NodeId::generate());
    let empty_line_id = empty_line.id.clone();
    let after = paragraph_with_text("after");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        equation,
        align,
        Block::Paragraph(empty_line),
        Block::Paragraph(after),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    set_body_caret(&mut runtime, empty_line_id, 0)?;

    runtime.delete_backward_json()?;

    assert_eq!(runtime.document().body.len(), 4);
    assert!(matches!(
        runtime.session.math_edit().map(|edit| &edit.target),
        Some(caret_engine::MathEditTarget::Block {
            block_id,
            align: true,
        }) if block_id == &align_id
    ));
    runtime.undo_json()?;
    assert_eq!(runtime.document().body.len(), 5);
    assert!(runtime.session.math_edit().is_none());
    runtime.redo_json()?;
    assert_eq!(runtime.document().body.len(), 4);
    assert!(matches!(
        runtime.session.math_edit().map(|edit| &edit.target),
        Some(caret_engine::MathEditTarget::Block { align: true, .. })
    ));
    Ok(())
}

#[test]
fn backspace_after_nested_math_uses_a_surviving_body_anchor()
-> Result<(), Box<dyn std::error::Error>> {
    let outside = paragraph_with_text("outside");
    let (formula, formula_id) = formula_block(false);
    let empty_line = Paragraph::empty(NodeId::generate());
    let empty_line_id = empty_line.id.clone();
    let section = BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(paragraph_with_text("title")),
        children: vec![formula, Block::Paragraph(empty_line)],
        extra: BTreeMap::new(),
    };
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(outside), Block::Environment(section)];
    let mut runtime = DocumentViewRuntime::new(document)?;
    set_body_caret(&mut runtime, empty_line_id.clone(), 0)?;

    runtime.delete_backward_json()?;

    let Block::Environment(section) = &runtime.document().body[1] else {
        panic!("document should retain the Section")
    };
    assert_eq!(section.children.len(), 1);
    assert!(matches!(section.children.first(), Some(Block::Equation(_))));
    assert!(matches!(
        runtime.session.math_edit().map(|edit| &edit.target),
        Some(caret_engine::MathEditTarget::Block { block_id, .. }) if block_id == &formula_id
    ));

    runtime.undo_json()?;
    let Block::Environment(section) = &runtime.document().body[1] else {
        panic!("undo should restore the Section")
    };
    assert_eq!(section.children.len(), 2);
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        empty_line_id
    );
    runtime.redo_json()?;
    let Block::Environment(section) = &runtime.document().body[1] else {
        panic!("redo should retain the Section")
    };
    assert_eq!(section.children.len(), 1);
    assert!(runtime.session.math_edit().is_some());
    Ok(())
}

#[test]
fn backspace_at_a_virtual_math_boundary_does_not_delete_the_adjacent_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let before = paragraph_with_text("before");
    let before_id = before.id.clone();
    let (formula, formula_id) = formula_block(false);
    let empty_line = Paragraph::empty(NodeId::generate());
    let after = paragraph_with_text("after");
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(before),
        formula,
        Block::Paragraph(empty_line),
        Block::Paragraph(after),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    set_body_caret(&mut runtime, before_id, "before".len())?;
    runtime.session.run(caret_engine::Command::EnterMath(
        caret_engine::Direction::Forward,
    ))?;
    runtime.session.run(caret_engine::Command::MoveCaret(
        caret_engine::Direction::Forward,
    ))?;
    runtime.session.run(caret_engine::Command::MoveCaret(
        caret_engine::Direction::Forward,
    ))?;
    let stop = runtime
        .session
        .block_boundary_exit()
        .ok_or_else(|| std::io::Error::other("navigation should stop outside the formula"))?;
    assert_eq!(stop.0, formula_id);
    assert_eq!(stop.1, caret_engine::Direction::Forward);

    runtime.delete_backward_json()?;

    assert_eq!(runtime.document().body.len(), 4);
    assert_eq!(runtime.session.block_boundary_exit(), Some(stop));
    assert!(runtime.session.math_edit().is_none());
    Ok(())
}

#[test]
fn math_tree_vertical_navigation_moves_between_fraction_operands()
-> Result<(), Box<dyn std::error::Error>> {
    let (document, block_id) = fraction_document();
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Block {
        block_id,
        align: false,
    };
    let position = caret_engine::MathTreePosition {
        host: caret_engine::MathPositionHost::Equation {
            owner_id: match &target {
                caret_engine::MathEditTarget::Block { block_id, .. } => block_id.clone(),
                _ => unreachable!(),
            },
        },
        path: caret_engine::MathSequencePath::new(vec![caret_engine::MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }]),
        offset: 1,
        affinity: caret_engine::MathPositionAffinity::Upstream,
    };
    let snapshot = runtime.current_snapshot()?;
    let caret = snapshot
        .math_tree_caret_geometry(&target, &position)
        .ok_or_else(|| std::io::Error::other("fraction numerator caret is missing"))?;
    let hit = caret_engine::PageMathStructureHit {
        session_id: snapshot.session_id.clone(),
        document_generation: snapshot.document_generation,
        font_generation: snapshot.font_generation,
        target,
        position,
        caret,
    };
    runtime
        .session
        .focus_math_structure_hit(&hit, runtime.layout.font_generation())?;

    let original_document = runtime.session.document().clone();
    let original_history = runtime.session.history_state_id();
    let mut stale_hit = hit.clone();
    stale_hit.document_generation = stale_hit.document_generation.saturating_add(1);
    let stale_generation = caret_engine::NavigationGeneration {
        session_id: runtime.session.session_id().clone(),
        document_generation: stale_hit.document_generation,
        font_generation: stale_hit.font_generation,
    };
    assert!(
        runtime
            .session
            .run(caret_engine::Command::EditorKey(
                caret_engine::EditorKey::DownArrow {
                    candidate: Some(caret_engine::NavigationCandidate::MathTreeHit {
                        hit: stale_hit,
                        generation: stale_generation,
                    }),
                    extend: false,
                    structure: false,
                },
            ))
            .is_err()
    );
    assert_eq!(runtime.session.document(), &original_document);
    assert_eq!(
        runtime
            .session
            .math_tree_input_context()
            .map(|(_, pos)| pos),
        Some(hit.position.clone())
    );
    assert_eq!(runtime.session.history_state_id(), original_history);

    runtime.move_vertical_caret_json(1, true)?;
    assert_eq!(
        runtime
            .session
            .math_tree_input_context()
            .map(|(_, pos)| pos),
        Some(hit.position.clone())
    );

    runtime.move_vertical_caret_json(1, false)?;
    let (_, focus) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("Down left MathTree focus"))?;
    assert_eq!(
        focus.path.steps().last(),
        Some(&caret_engine::MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        })
    );

    runtime.move_vertical_caret_json(-1, false)?;
    let (_, focus) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("Up left MathTree focus"))?;
    assert_eq!(focus, hit.position);
    Ok(())
}

#[test]
fn subsup_pointer_focus_routes_backspace_and_mod_backspace_by_operand_path()
-> Result<(), Box<dyn std::error::Error>> {
    let (document, paragraph_id, math_id) = inline_subsup_document(Some("a"));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Inline {
        paragraph_id: paragraph_id.clone(),
        math_id,
    };
    let superscript_path =
        caret_engine::MathSequencePath::new(vec![caret_engine::MathSequenceStep::Operand {
            content_index: 3,
            operand_index: 2,
        }]);
    let superscript_position = caret_engine::MathTreePosition {
        host: caret_engine::MathPositionHost::InlineMath {
            owner_id: match &target {
                caret_engine::MathEditTarget::Inline { math_id, .. } => math_id.clone(),
                _ => unreachable!(),
            },
        },
        path: superscript_path.clone(),
        offset: 1,
        affinity: caret_engine::MathPositionAffinity::Upstream,
    };
    let superscript_hit = {
        let snapshot = runtime.current_snapshot()?;
        let caret = snapshot
            .math_tree_caret_geometry(&target, &superscript_position)
            .ok_or_else(|| std::io::Error::other("superscript caret geometry is missing"))?;
        snapshot
            .hit_test_math_structure(
                caret.page_index,
                caret.x_pt,
                (caret.y0_pt + caret.y1_pt) / 2.0,
            )
            .ok_or_else(|| std::io::Error::other("superscript hit is missing"))?
    };
    assert_eq!(superscript_hit.position.path, superscript_path);
    assert_eq!(superscript_hit.position.offset, 1);
    runtime
        .session
        .focus_math_structure_hit(&superscript_hit, runtime.layout.font_generation())?;
    assert_eq!(
        runtime
            .session
            .math_tree_input_context()
            .map(|(_, position)| position),
        Some(superscript_hit.position.clone())
    );

    runtime.handle_editor_key_json("backspace")?;
    let (_, after_script_backspace) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("Backspace left MathTree focus"))?;
    assert_eq!(after_script_backspace.path, superscript_path);
    assert_eq!(after_script_backspace.offset, 0);
    let Block::Paragraph(paragraph) = &runtime.session.document().body[0] else {
        return Err("inline math paragraph disappeared after script Backspace".into());
    };
    let ParagraphContent::Math(math) = &paragraph.inlines[0] else {
        return Err("SubSup inline math disappeared after script Backspace".into());
    };
    let caret_doc::MathContent::Math(subsup) = &math.math_tree.root.content[3] else {
        return Err("SubSup node disappeared after script Backspace".into());
    };
    assert!(subsup.children[2].content.is_empty());

    let before_empty_slot_backspace = runtime.session.document().clone();
    runtime.handle_editor_key_json("backspace")?;
    assert_eq!(runtime.session.document(), &before_empty_slot_backspace);
    assert!(
        runtime
            .session
            .math_tree_input_context()
            .is_some_and(|(_, position)| {
                position.path.steps().is_empty() && position.offset == 3
            })
    );
    runtime.handle_editor_key_json("backspace")?;
    let Block::Paragraph(paragraph) = &runtime.session.document().body[0] else {
        return Err("inline math paragraph disappeared after Backspace".into());
    };
    let ParagraphContent::Math(math) = &paragraph.inlines[0] else {
        return Err("SubSup inline math disappeared after Backspace".into());
    };
    assert_eq!(math.math_tree.root.content.len(), 4);
    assert!(matches!(
        &math.math_tree.root.content[2],
        caret_doc::MathContent::Math(node)
            if node.kind == caret_doc::MathNodeKind::SubSup
    ));

    let (document, paragraph_id, math_id) = inline_subsup_document(Some("a"));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Inline {
        paragraph_id,
        math_id: math_id.clone(),
    };
    let host = caret_engine::MathPositionHost::InlineMath { owner_id: math_id };
    let position = caret_engine::MathTreePosition {
        host: host.clone(),
        path: superscript_path.clone(),
        offset: 1,
        affinity: caret_engine::MathPositionAffinity::Upstream,
    };
    let hit = {
        let snapshot = runtime.current_snapshot()?;
        let caret = snapshot
            .math_tree_caret_geometry(&target, &position)
            .ok_or_else(|| std::io::Error::other("superscript caret geometry is missing"))?;
        snapshot
            .hit_test_math_structure(
                caret.page_index,
                caret.x_pt,
                (caret.y0_pt + caret.y1_pt) / 2.0,
            )
            .ok_or_else(|| std::io::Error::other("superscript hit is missing"))?
    };
    assert_eq!(hit.position.path, superscript_path);
    assert_eq!(hit.position.offset, 1);
    runtime
        .session
        .focus_math_structure_hit(&hit, runtime.layout.font_generation())?;
    runtime.handle_editor_key_json("mod-backspace")?;
    assert!(runtime.session.math_edit().is_none());
    assert_eq!(
        block_text(&runtime.session.document().body[0]),
        Some("sada_{a}^{a}a")
    );
    assert_eq!(
        runtime.session.selection().focus.offset,
        "sada_{a}^{a}a".find("^{a}").ok_or_else(|| {
            std::io::Error::other("serialized superscript operand is missing")
        })? + 3
    );
    runtime.handle_editor_key_json("mod-z")?;
    assert!(
        runtime
            .session
            .math_tree_input_context()
            .is_some_and(|(_, position)| {
                position.path == superscript_path && position.offset == 1
            })
    );
    runtime.handle_editor_key_json("mod-shift-z")?;
    assert!(runtime.session.math_edit().is_none());

    let (document, paragraph_id, math_id) = inline_subsup_document(Some("a"));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Inline {
        paragraph_id,
        math_id: math_id.clone(),
    };
    let host = caret_engine::MathPositionHost::InlineMath { owner_id: math_id };
    let root_position = caret_engine::MathTreePosition {
        host,
        path: caret_engine::MathSequencePath::root(),
        offset: 5,
        affinity: caret_engine::MathPositionAffinity::Upstream,
    };
    let root_hit = {
        let snapshot = runtime.current_snapshot()?;
        let caret = snapshot
            .math_tree_caret_geometry(&target, &root_position)
            .ok_or_else(|| std::io::Error::other("root-end caret geometry is missing"))?;
        snapshot
            .hit_test_math_structure(
                caret.page_index,
                caret.x_pt,
                (caret.y0_pt + caret.y1_pt) / 2.0,
            )
            .ok_or_else(|| std::io::Error::other("root-end structure hit is missing"))?
    };
    assert!(root_hit.position.path.steps().is_empty());
    assert_eq!(root_hit.position.offset, 5);
    runtime
        .session
        .focus_math_structure_hit(&root_hit, runtime.layout.font_generation())?;
    runtime.handle_editor_key_json("backspace")?;
    let Block::Paragraph(paragraph) = &runtime.session.document().body[0] else {
        return Err("inline math paragraph disappeared after root-end Backspace".into());
    };
    let ParagraphContent::Math(math) = &paragraph.inlines[0] else {
        return Err("SubSup inline math disappeared after root-end Backspace".into());
    };
    assert_eq!(math.math_tree.root.content.len(), 4);
    assert!(matches!(
        &math.math_tree.root.content[3],
        caret_doc::MathContent::Math(node)
            if node.kind == caret_doc::MathNodeKind::SubSup
    ));
    Ok(())
}

#[test]
fn subsup_base_start_pointer_focus_supports_mod_backspace() -> Result<(), Box<dyn std::error::Error>>
{
    let (document, paragraph_id, math_id) = inline_subsup_document(Some("a"));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Inline {
        paragraph_id,
        math_id: math_id.clone(),
    };
    let internal_position = caret_engine::MathTreePosition {
        host: caret_engine::MathPositionHost::InlineMath { owner_id: math_id },
        path: caret_engine::MathSequencePath::new(vec![caret_engine::MathSequenceStep::Operand {
            content_index: 3,
            operand_index: 0,
        }]),
        offset: 0,
        affinity: caret_engine::MathPositionAffinity::Downstream,
    };
    let caret = runtime
        .current_snapshot()?
        .math_tree_caret_geometry(&target, &internal_position)
        .ok_or_else(|| std::io::Error::other("base-start caret geometry is missing"))?;
    let placed: serde_json::Value = serde_json::from_str(&runtime.place_caret_at_page_json(
        caret.page_index as i32,
        caret.x_pt,
        (caret.y0_pt + caret.y1_pt) / 2.0,
    )?)?;
    assert_eq!(placed["mathTree"], true);
    assert!(
        runtime
            .session
            .math_tree_input_context()
            .is_some_and(|(_, position)| {
                position.path.steps()
                    == [caret_engine::MathSequenceStep::Operand {
                        content_index: 3,
                        operand_index: 0,
                    }]
                    && position.offset == 0
            })
    );
    runtime.handle_editor_key_json("mod-backspace")?;
    assert!(runtime.session.math_edit().is_none());
    assert_eq!(
        block_text(&runtime.session.document().body[0]),
        Some("sada_{a}^{a}a")
    );
    assert_eq!(runtime.session.selection().focus.offset, 3);
    Ok(())
}

#[test]
fn inserting_before_superscript_base_then_mod_backspace_succeeds()
-> Result<(), Box<dyn std::error::Error>> {
    let (document, paragraph_id, math_id) = inline_superscript_document();
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Inline {
        paragraph_id,
        math_id: math_id.clone(),
    };
    let position = caret_engine::MathTreePosition {
        host: caret_engine::MathPositionHost::InlineMath { owner_id: math_id },
        path: caret_engine::MathSequencePath::new(vec![caret_engine::MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }]),
        offset: 0,
        affinity: caret_engine::MathPositionAffinity::Downstream,
    };
    let caret = runtime
        .current_snapshot()?
        .math_tree_caret_geometry(&target, &position)
        .ok_or_else(|| std::io::Error::other("superscript base-start geometry is missing"))?;
    let placed: serde_json::Value = serde_json::from_str(&runtime.place_caret_at_page_json(
        caret.page_index as i32,
        caret.x_pt,
        (caret.y0_pt + caret.y1_pt) / 2.0,
    )?)?;
    assert_eq!(placed["mathTree"], true);
    let (_, before_insert) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("pointer did not enter MathTree"))?;
    assert_eq!(before_insert.path, position.path);
    assert_eq!(before_insert.offset, 0);
    runtime.insert_text_json("v".to_owned())?;
    let (_, after_insert) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("insert left MathTree focus"))?;
    assert!(after_insert.path.steps().is_empty());
    assert_eq!(after_insert.offset, 1);
    let result = runtime.handle_editor_key_json("mod-backspace");
    assert!(
        result.is_ok(),
        "Mod+Backspace failed after insertion: {result:?}"
    );
    assert!(runtime.session.math_edit().is_none());
    assert_eq!(
        block_text(&runtime.session.document().body[0]),
        Some("vx^{a}")
    );
    assert_eq!(runtime.session.selection().focus.offset, 1);
    Ok(())
}

#[test]
fn typing_at_empty_superscript_base_fills_its_placeholder() -> Result<(), Box<dyn std::error::Error>>
{
    let (mut document, paragraph_id, math_id) = inline_superscript_document();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        return Err("expected paragraph".into());
    };
    let ParagraphContent::Math(math) = &mut paragraph.inlines[0] else {
        return Err("expected inline math".into());
    };
    let Some(caret_doc::MathContent::Math(script)) = math.math_tree.root.content.first_mut() else {
        return Err("expected superscript".into());
    };
    script.children[0] = placeholder_flow();

    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Inline {
        paragraph_id,
        math_id: math_id.clone(),
    };
    let position = caret_engine::MathTreePosition {
        host: caret_engine::MathPositionHost::InlineMath { owner_id: math_id },
        path: caret_engine::MathSequencePath::new(vec![caret_engine::MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }]),
        offset: 0,
        affinity: caret_engine::MathPositionAffinity::Downstream,
    };
    let caret = runtime
        .current_snapshot()?
        .math_tree_caret_geometry(&target, &position)
        .ok_or_else(|| std::io::Error::other("empty-base caret geometry is missing"))?;
    let placed: serde_json::Value = serde_json::from_str(&runtime.place_caret_at_page_json(
        caret.page_index as i32,
        caret.x_pt,
        (caret.y0_pt + caret.y1_pt) / 2.0,
    )?)?;
    assert_eq!(placed["mathTree"], true);
    let (_, before_insert) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("pointer did not enter the empty MathTree slot"))?;
    assert_eq!(before_insert.path, position.path);
    assert_eq!(before_insert.offset, 0);
    runtime.insert_text_json("v".to_owned())?;
    let (_, after_insert) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("placeholder replacement left MathTree focus"))?;
    assert_eq!(after_insert.path, position.path);
    assert_eq!(after_insert.offset, 1);
    let Block::Paragraph(paragraph) = &runtime.session.document().body[0] else {
        return Err("paragraph disappeared after placeholder replacement".into());
    };
    let ParagraphContent::Math(math) = &paragraph.inlines[0] else {
        return Err("inline math disappeared after placeholder replacement".into());
    };
    let Some(caret_doc::MathContent::Math(script)) = math.math_tree.root.content.first() else {
        return Err("superscript disappeared after placeholder replacement".into());
    };
    assert!(matches!(
        script.children[0].content.first(),
        Some(caret_doc::MathContent::Math(node))
            if node.kind == caret_doc::MathNodeKind::Symbol && node.text.as_deref() == Some("v")
    ));
    Ok(())
}

#[test]
fn math_tree_vertical_navigation_enters_textflow_in_an_operand()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut document, block_id) = fraction_document();
    let Some(Block::Equation(equation)) = document
        .body
        .iter_mut()
        .find(|block| matches!(block, Block::Equation(_)))
    else {
        return Err("fraction fixture is not an equation".into());
    };
    let caret_doc::MathContent::Math(fraction) = &mut equation.math_tree.root.content[0] else {
        return Err("fraction fixture root has no fraction".into());
    };
    fraction.children[1].content = vec![math_text_content("denominator")];

    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let position = caret_engine::MathTreePosition {
        host: caret_engine::MathPositionHost::Equation { owner_id: block_id },
        path: caret_engine::MathSequencePath::new(vec![caret_engine::MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }]),
        offset: 1,
        affinity: caret_engine::MathPositionAffinity::Upstream,
    };
    let snapshot = runtime.current_snapshot()?;
    let caret = snapshot
        .math_tree_caret_geometry(&target, &position)
        .ok_or_else(|| std::io::Error::other("fraction numerator caret is missing"))?;
    let hit = caret_engine::PageMathStructureHit {
        session_id: snapshot.session_id.clone(),
        document_generation: snapshot.document_generation,
        font_generation: snapshot.font_generation,
        target,
        position,
        caret,
    };
    runtime
        .session
        .focus_math_structure_hit(&hit, runtime.layout.font_generation())?;

    runtime.move_vertical_caret_json(1, false)?;
    let context = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Down did not enter denominator TextFlow"))?;
    assert_eq!(context.surrounding_text, "denominator");
    Ok(())
}

#[test]
fn math_tree_vertical_navigation_keeps_empty_fraction_operand_insertable()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut document, block_id) = fraction_document();
    let Some(Block::Equation(equation)) = document
        .body
        .iter_mut()
        .find(|block| matches!(block, Block::Equation(_)))
    else {
        return Err("fraction fixture is not an equation".into());
    };
    let caret_doc::MathContent::Math(fraction) = &mut equation.math_tree.root.content[0] else {
        return Err("fraction fixture root has no fraction".into());
    };
    let mut denominator = symbol_flow("d");
    let mut empty_slot = placeholder_flow();
    denominator.content.push(empty_slot.content.remove(0));
    fraction.children = vec![placeholder_flow(), denominator];

    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let after_placeholder = caret_engine::MathTreePosition {
        host: caret_engine::MathPositionHost::Equation {
            owner_id: block_id.clone(),
        },
        path: caret_engine::MathSequencePath::new(vec![caret_engine::MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        }]),
        offset: 2,
        affinity: caret_engine::MathPositionAffinity::Upstream,
    };
    let hit = {
        let snapshot = runtime.current_snapshot()?;
        let caret = snapshot
            .math_tree_caret_geometry(&target, &after_placeholder)
            .ok_or_else(|| std::io::Error::other("post-placeholder caret geometry is missing"))?;
        snapshot
            .hit_test_math_structure(
                caret.page_index,
                caret.x_pt,
                (caret.y0_pt + caret.y1_pt) / 2.0,
            )
            .ok_or_else(|| std::io::Error::other("post-placeholder mouse hit is missing"))?
    };
    assert_eq!(hit.position.offset, 1);
    let position = caret_engine::MathTreePosition {
        host: caret_engine::MathPositionHost::Equation { owner_id: block_id },
        path: caret_engine::MathSequencePath::new(vec![caret_engine::MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 0,
        }]),
        offset: 0,
        affinity: caret_engine::MathPositionAffinity::Downstream,
    };
    let snapshot = runtime.current_snapshot()?;
    let caret = snapshot
        .math_tree_caret_geometry(&target, &position)
        .ok_or_else(|| std::io::Error::other("empty numerator caret is missing"))?;
    let initial_hit = caret_engine::PageMathStructureHit {
        session_id: snapshot.session_id.clone(),
        document_generation: snapshot.document_generation,
        font_generation: snapshot.font_generation,
        target: target.clone(),
        position,
        caret,
    };
    runtime
        .session
        .focus_math_structure_hit(&initial_hit, runtime.layout.font_generation())?;

    let next = runtime
        .current_snapshot()?
        .move_math_tree_vertical_target(&target, &initial_hit.position, 1, Some(10_000.0))
        .ok_or_else(|| std::io::Error::other("Down found no denominator slot"))?;
    let caret_engine::MathTreeVerticalTarget::Structure(next) = next else {
        return Err("empty fraction operand should remain a tree slot".into());
    };
    assert_eq!(
        next.position.path.steps().last(),
        Some(&caret_engine::MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        })
    );
    assert_eq!(next.position.offset, 1);
    runtime
        .session
        .focus_math_structure_hit(&next, runtime.layout.font_generation())?;
    runtime.insert_text_json("x".to_owned())?;

    let Some(Block::Equation(equation)) = runtime
        .session
        .document()
        .body
        .iter()
        .find(|block| matches!(block, Block::Equation(_)))
    else {
        return Err("fraction disappeared after inserting into denominator".into());
    };
    let caret_doc::MathContent::Math(fraction) = &equation.math_tree.root.content[0] else {
        return Err("fraction disappeared after inserting into denominator".into());
    };
    assert!(matches!(
        fraction.children[1].content.as_slice(),
        [
            caret_doc::MathContent::Math(existing),
            caret_doc::MathContent::Math(inserted),
        ] if existing.kind == caret_doc::MathNodeKind::Symbol
            && existing.text.as_deref() == Some("d")
            && inserted.kind == caret_doc::MathNodeKind::Symbol
            && inserted.text.as_deref() == Some("x")
    ));
    Ok(())
}

#[test]
fn math_tree_focus_after_required_placeholder_is_normalized_before_typing()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut document, block_id) = fraction_document();
    let Some(Block::Equation(equation)) = document
        .body
        .iter_mut()
        .find(|block| matches!(block, Block::Equation(_)))
    else {
        return Err("fraction fixture is not an equation".into());
    };
    let caret_doc::MathContent::Math(fraction) = &mut equation.math_tree.root.content[0] else {
        return Err("fraction fixture root has no fraction".into());
    };
    let mut denominator = placeholder_flow();
    denominator.content.push(symbol_flow("d").content.remove(0));
    fraction.children = vec![placeholder_flow(), denominator];

    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let invalid_position = caret_engine::MathTreePosition {
        host: caret_engine::MathPositionHost::Equation {
            owner_id: block_id.clone(),
        },
        path: caret_engine::MathSequencePath::new(vec![caret_engine::MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        }]),
        offset: 1,
        affinity: caret_engine::MathPositionAffinity::Upstream,
    };
    let valid_after_content = caret_engine::MathTreePosition {
        offset: 2,
        ..invalid_position.clone()
    };
    let (page_index, x_pt, y0_pt, y1_pt) = {
        let snapshot = runtime.current_snapshot()?;
        let caret = snapshot
            .math_tree_caret_geometry(&target, &valid_after_content)
            .ok_or_else(|| std::io::Error::other("post-content caret geometry is missing"))?;
        (caret.page_index, caret.x_pt, caret.y0_pt, caret.y1_pt)
    };
    let placement: serde_json::Value = serde_json::from_str(&runtime.place_caret_at_page_json(
        page_index as i32,
        x_pt,
        (y0_pt + y1_pt) / 2.0,
    )?)?;
    assert_eq!(placement["mathTree"], true);
    assert_eq!(
        runtime
            .session
            .math_tree_input_context()
            .map(|(_, focus)| focus.offset),
        Some(2),
        "clicking after existing content must keep its legal boundary"
    );
    let (page_index, x_pt, y0_pt, y1_pt) = {
        let snapshot = runtime.current_snapshot()?;
        let caret = snapshot
            .math_tree_caret_geometry(&target, &invalid_position)
            .ok_or_else(|| std::io::Error::other("post-placeholder caret geometry is missing"))?;
        (caret.page_index, caret.x_pt, caret.y0_pt, caret.y1_pt)
    };
    let placement: serde_json::Value = serde_json::from_str(&runtime.place_caret_at_page_json(
        page_index as i32,
        x_pt,
        (y0_pt + y1_pt) / 2.0,
    )?)?;
    assert_eq!(
        placement["mathTree"], true,
        "click should stay in the math tree"
    );
    let (focused_target, focus) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("click did not focus MathTree"))?;
    assert_eq!(focused_target, target);
    assert_eq!(focus.path, invalid_position.path);
    assert_eq!(focus.offset, 0, "click must land before the Placeholder");

    runtime.insert_text_json("asdasd".to_owned())?;
    let Some(Block::Equation(equation)) = runtime
        .session
        .document()
        .body
        .iter()
        .find(|block| matches!(block, Block::Equation(_)))
    else {
        return Err("fraction disappeared after typing".into());
    };
    let caret_doc::MathContent::Math(fraction) = &equation.math_tree.root.content[0] else {
        return Err("fraction disappeared after typing".into());
    };
    assert!(fraction.children[1].content.iter().all(|content| {
        !matches!(content, caret_doc::MathContent::Math(node) if node.kind == caret_doc::MathNodeKind::Placeholder)
    }));
    let symbols = fraction.children[1]
        .content
        .iter()
        .filter_map(|content| match content {
            caret_doc::MathContent::Math(node) if node.kind == caret_doc::MathNodeKind::Symbol => {
                node.text.as_deref()
            }
            _ => None,
        })
        .collect::<String>();
    assert_eq!(
        symbols, "asdasdd",
        "typing must replace the slot and preserve following content"
    );
    Ok(())
}

#[test]
fn math_tree_focus_normalizes_a_stale_hit_after_required_placeholder()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut document, block_id) = fraction_document();
    let Some(Block::Equation(equation)) = document
        .body
        .iter_mut()
        .find(|block| matches!(block, Block::Equation(_)))
    else {
        return Err("fraction fixture is not an equation".into());
    };
    let caret_doc::MathContent::Math(fraction) = &mut equation.math_tree.root.content[0] else {
        return Err("fraction fixture root has no fraction".into());
    };
    let mut denominator = placeholder_flow();
    denominator.content.push(symbol_flow("d").content.remove(0));
    fraction.children = vec![placeholder_flow(), denominator];

    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let make_position = |offset| caret_engine::MathTreePosition {
        host: caret_engine::MathPositionHost::Equation {
            owner_id: block_id.clone(),
        },
        path: caret_engine::MathSequencePath::new(vec![caret_engine::MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        }]),
        offset,
        affinity: caret_engine::MathPositionAffinity::Upstream,
    };
    let make_hit = |runtime: &mut DocumentViewRuntime,
                    position|
     -> Result<_, Box<dyn std::error::Error>> {
        let snapshot = runtime.current_snapshot()?;
        let caret = snapshot
            .math_tree_caret_geometry(&target, &position)
            .ok_or_else(|| std::io::Error::other("post-placeholder caret geometry is missing"))?;
        Ok(caret_engine::PageMathStructureHit {
            session_id: snapshot.session_id.clone(),
            document_generation: snapshot.document_generation,
            font_generation: snapshot.font_generation,
            target: target.clone(),
            position,
            caret,
        })
    };
    let hit = make_hit(&mut runtime, make_position(2))?;
    runtime
        .session
        .focus_math_structure_hit(&hit, runtime.layout.font_generation())?;
    assert_eq!(
        runtime
            .session
            .math_tree_input_context()
            .map(|(_, focus)| focus.offset),
        Some(2),
        "content after a Placeholder must retain its caret positions"
    );

    runtime.insert_text_json("x".to_owned())?;
    let Block::Equation(equation) = runtime
        .session
        .document()
        .body
        .iter()
        .find(|block| matches!(block, Block::Equation(_)))
        .ok_or_else(|| std::io::Error::other("fraction disappeared after typing"))?
    else {
        return Err("fraction disappeared after typing".into());
    };
    let caret_doc::MathContent::Math(fraction) = &equation.math_tree.root.content[0] else {
        return Err("fraction disappeared after typing".into());
    };
    assert!(matches!(
        fraction.children[1].content.as_slice(),
        [
            caret_doc::MathContent::Math(placeholder),
            caret_doc::MathContent::Math(existing),
            caret_doc::MathContent::Math(inserted),
        ] if placeholder.kind == caret_doc::MathNodeKind::Placeholder
            && existing.text.as_deref() == Some("d")
            && inserted.text.as_deref() == Some("x")
    ));
    let hit = make_hit(&mut runtime, make_position(1))?;
    runtime
        .session
        .focus_math_structure_hit(&hit, runtime.layout.font_generation())?;
    assert_eq!(
        runtime
            .session
            .math_tree_input_context()
            .map(|(_, focus)| focus.offset),
        Some(0),
        "a required slot has no legal caret boundary after its Placeholder"
    );
    Ok(())
}

#[test]
fn math_tree_vertical_navigation_escapes_nested_fraction_to_outer_sibling()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut document, block_id) = fraction_document();
    let Some(Block::Equation(equation)) = document
        .body
        .iter_mut()
        .find(|block| matches!(block, Block::Equation(_)))
    else {
        return Err("fraction fixture is not an equation".into());
    };
    let caret_doc::MathContent::Math(outer_fraction) = &mut equation.math_tree.root.content[0]
    else {
        return Err("fraction fixture root has no fraction".into());
    };
    let inner_fraction = caret_doc::MathNode {
        id: NodeId::generate(),
        kind: caret_doc::MathNodeKind::Fraction,
        text: None,
        attributes: BTreeMap::from([("style".to_owned(), "frac".to_owned())]),
        children: vec![symbol_flow("a"), symbol_flow("b")],
        extra: BTreeMap::new(),
    };
    outer_fraction.children[0].content = vec![caret_doc::MathContent::Math(inner_fraction)];

    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Block {
        block_id: block_id.clone(),
        align: false,
    };
    let position = caret_engine::MathTreePosition {
        host: caret_engine::MathPositionHost::Equation { owner_id: block_id },
        path: caret_engine::MathSequencePath::new(vec![
            caret_engine::MathSequenceStep::Operand {
                content_index: 0,
                operand_index: 0,
            },
            caret_engine::MathSequenceStep::Operand {
                content_index: 0,
                operand_index: 1,
            },
        ]),
        offset: 1,
        affinity: caret_engine::MathPositionAffinity::Upstream,
    };
    let snapshot = runtime.current_snapshot()?;
    let caret = snapshot
        .math_tree_caret_geometry(&target, &position)
        .ok_or_else(|| std::io::Error::other("nested denominator caret is missing"))?;
    let hit = caret_engine::PageMathStructureHit {
        session_id: snapshot.session_id.clone(),
        document_generation: snapshot.document_generation,
        font_generation: snapshot.font_generation,
        target,
        position,
        caret,
    };
    runtime
        .session
        .focus_math_structure_hit(&hit, runtime.layout.font_generation())?;

    runtime.move_vertical_caret_json(1, false)?;
    let (_, focus) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("Down left nested MathTree focus"))?;
    assert_eq!(
        focus.path.steps(),
        &[caret_engine::MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        }]
    );
    Ok(())
}

#[test]
fn undo_restores_anchor_visual_side_at_a_soft_wrap_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    let original_text = "The quick brown fox jumps over the lazy dog. ".repeat(6);
    runtime.insert_text_json(original_text.clone())?;
    let paragraph_id = runtime.session.selection().focus.paragraph_id.to_string();
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let stops = hit_map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("paragraph has no visual caret stops"))?;
    let boundary = stops.iter().find_map(|upstream| {
        if upstream["position"]["paragraphId"] != paragraph_id
            || upstream["visualAffinity"] != "upstream"
        {
            return None;
        }
        let offset = upstream["position"]["offsetUtf8"].as_str()?;
        let downstream = stops.iter().find(|candidate| {
            candidate["position"]["paragraphId"] == paragraph_id
                && candidate["position"]["offsetUtf8"] == offset
                && candidate["position"]["affinity"] == upstream["position"]["affinity"]
                && candidate["visualAffinity"] == "downstream"
        })?;
        (upstream["y0Pt"] != downstream["y0Pt"]).then_some(upstream.clone())
    });
    let anchor_stop = boundary
        .ok_or_else(|| std::io::Error::other("paragraph has no distinct soft-wrap caret sides"))?;
    let boundary_offset = anchor_stop["position"]["offsetUtf8"]
        .as_str()
        .ok_or_else(|| std::io::Error::other("soft-wrap stop has no offset"))?
        .parse::<usize>()?;
    let next_offset = boundary_offset + 1;
    let focus_stop = stops
        .iter()
        .find(|stop| {
            stop["position"]["paragraphId"] == paragraph_id
                && stop["position"]["offsetUtf8"]
                    .as_str()
                    .and_then(|offset| offset.parse::<usize>().ok())
                    == Some(next_offset)
                && stop["visualAffinity"] == "downstream"
        })
        .ok_or_else(|| std::io::Error::other("caret after soft-wrap boundary is missing"))?;
    let anchor = serde_json::json!({
        "paragraphId": paragraph_id,
        "offsetUtf8": boundary_offset.to_string(),
        "affinity": anchor_stop["position"]["affinity"],
        "visualAffinity": "upstream"
    });
    let focus = serde_json::json!({
        "paragraphId": paragraph_id,
        "offsetUtf8": next_offset.to_string(),
        "affinity": focus_stop["position"]["affinity"],
        "visualAffinity": "downstream"
    });
    runtime
        .set_selection_json(&serde_json::json!({"anchor": anchor, "focus": focus}).to_string())?;

    runtime.insert_text_json("X".to_owned())?;
    let restored: Value = serde_json::from_str(&runtime.undo_json()?)?;
    assert_eq!(restored["paragraphs"][0]["text"], original_text);
    assert_ne!(
        runtime.session.selection().anchor,
        runtime.session.selection().focus
    );
    let collapsed: Value = serde_json::from_str(&runtime.move_visual_caret_json(false, false)?)?;

    assert_eq!(collapsed["caretGeometry"]["y0Pt"], anchor_stop["y0Pt"]);
    assert_eq!(collapsed["caretGeometry"]["xPt"], anchor_stop["xPt"]);
    Ok(())
}

#[test]
fn math_textflow_vertical_navigation_enters_a_math_tree_operand()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut document, block_id) = fraction_document();
    let Some(Block::Equation(equation)) = document
        .body
        .iter_mut()
        .find(|block| matches!(block, Block::Equation(_)))
    else {
        return Err("fraction fixture is not an equation".into());
    };
    let caret_doc::MathContent::Math(fraction) = &mut equation.math_tree.root.content[0] else {
        return Err("fraction fixture root has no fraction".into());
    };
    fraction.children[0].content = vec![math_text_content("numerator")];

    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Block {
        block_id,
        align: false,
    };
    let (token, position, order_utf8, visual_affinity) = {
        let snapshot = runtime.current_snapshot()?;
        snapshot
            .math_text_drag_stops(0)
            .into_iter()
            .find(|stop| stop.target == target && stop.order_utf8 == "numerator".len())
            .map(|stop| {
                (
                    stop.token,
                    stop.position,
                    stop.order_utf8,
                    stop.visual_affinity,
                )
            })
            .ok_or_else(|| std::io::Error::other("fraction numerator TextFlow caret is missing"))?
    };
    let hit = runtime
        .current_snapshot()?
        .math_text_drag_hit(0, token, &target, &position, order_utf8, visual_affinity)
        .ok_or_else(|| std::io::Error::other("fraction numerator TextFlow hit is missing"))?;
    let preferred_x = hit.caret.x_pt;
    runtime
        .session
        .focus_math_text_hit(&hit, runtime.layout.font_generation())?;

    runtime.move_vertical_caret_json(1, false)?;
    let (_, focus) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("Down did not enter denominator MathTree"))?;
    assert_eq!(
        focus.path.steps().last(),
        Some(&caret_engine::MathSequenceStep::Operand {
            content_index: 0,
            operand_index: 1,
        })
    );
    runtime.move_vertical_caret_json(-1, false)?;
    let context = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Up did not return to numerator TextFlow"))?;
    assert_eq!(context.focus, position);
    let restored_caret = runtime
        .current_snapshot()?
        .math_text_caret_geometry(&target, &context.focus, context.visual_affinity)
        .ok_or_else(|| std::io::Error::other("restored numerator caret geometry is missing"))?;
    assert!((restored_caret.x_pt - preferred_x).abs() < 0.01);
    Ok(())
}

#[test]
fn math_textflow_vertical_navigation_enters_next_align_row_tree_slot()
-> Result<(), Box<dyn std::error::Error>> {
    let align = caret_doc::Align {
        id: NodeId::generate(),
        rows: vec![
            caret_doc::MathAlignRow {
                id: NodeId::generate(),
                cells: vec![align_text_tree("ab")],
                extra: BTreeMap::new(),
            },
            caret_doc::MathAlignRow {
                id: NodeId::generate(),
                cells: vec![caret_doc::MathTree {
                    schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                    root: placeholder_flow(),
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            },
        ],
        extra: BTreeMap::new(),
    };
    let expected_row = align.rows[1].id.clone();
    let mut document = Document::blank();
    document.body.push(Block::Align(align.clone()));
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    let target = caret_engine::MathEditTarget::Block {
        block_id: align.id,
        align: true,
    };
    let hit = runtime
        .current_snapshot()?
        .math_text_drag_stops(0)
        .into_iter()
        .filter(|stop| stop.target == target && stop.position.path.align_cell() == Some((0, 0)))
        .filter_map(|stop| {
            runtime.current_snapshot().ok()?.math_text_drag_hit(
                0,
                stop.token,
                &target,
                &stop.position,
                stop.order_utf8,
                stop.visual_affinity,
            )
        })
        .find(|hit| hit.position.offset == "ab".len())
        .ok_or_else(|| std::io::Error::other("first Align row text caret is missing"))?;
    runtime
        .session
        .focus_math_text_hit(&hit, runtime.layout.font_generation())?;

    runtime.move_vertical_caret_json(1, false)?;
    let (_, focus) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("Down did not enter the next Align tree slot"))?;
    assert!(matches!(
        focus.host,
        caret_engine::MathPositionHost::AlignCell { row_id, .. } if row_id == expected_row
    ));
    assert_eq!(focus.offset, 0);
    Ok(())
}

#[test]
fn unsupported_line_edge_navigation_preserves_formula_focus()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime
        .session
        .run(caret_engine::Command::InsertInlineMath("x".to_owned()))?;
    runtime.session.run(caret_engine::Command::EnterMath(
        caret_engine::Direction::Backward,
    ))?;
    assert!(runtime.session.math_tree_input_context().is_some());
    let selection = runtime.session.selection().clone();

    runtime.move_to_visual_line_edge_json(true, false)?;
    assert!(runtime.session.math_tree_input_context().is_some());
    assert_eq!(runtime.session.selection(), &selection);
    Ok(())
}

fn place_caret_in_align_cell(
    runtime: &mut DocumentViewRuntime,
    row: usize,
    column: usize,
    offset: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let point = {
        let snapshot = runtime.current_snapshot()?;
        let mut point = None;
        'pages: for (page_index, page) in snapshot.pages.iter().enumerate() {
            for fragment in &page.layout.math_fragments {
                let left = page.margin_left_pt + f64::from(fragment.x_offset_pt);
                let top = page.margin_top_pt + f64::from(fragment.y_offset_pt);
                let right = left + f64::from(fragment.width_pt);
                let bottom = top + f64::from(fragment.height_pt);
                let mut y = top;
                while y <= bottom {
                    let mut x = left;
                    while x <= right {
                        if snapshot
                            .hit_test_math_text(page_index, x, y)
                            .is_some_and(|hit| {
                                hit.position.path.align_cell() == Some((row, column))
                                    && hit.position.offset == offset
                            })
                        {
                            point = Some((page_index, x, y));
                            break 'pages;
                        }
                        x += 0.5;
                    }
                    y += 0.5;
                }
            }
        }
        point.ok_or_else(|| {
            std::io::Error::other(format!(
                "Align cell ({row}, {column}) offset {offset} is not hittable"
            ))
        })?
    };
    runtime.place_caret_at_page_json(point.0 as i32, point.1, point.2)?;
    Ok(())
}

fn align_caret_x(runtime: &mut DocumentViewRuntime) -> Result<f64, Box<dyn std::error::Error>> {
    let context = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Align text focus is unavailable"))?;
    let snapshot = runtime.current_snapshot()?;
    snapshot
        .math_text_caret_geometry(&context.target, &context.focus, context.visual_affinity)
        .map(|caret| caret.x_pt)
        .ok_or_else(|| std::io::Error::other("Align caret geometry is unavailable").into())
}

fn assert_align_caret_is_nearest_to_x(
    runtime: &mut DocumentViewRuntime,
    target_x: f64,
) -> Result<(), Box<dyn std::error::Error>> {
    let context = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Align text focus is unavailable"))?;
    let snapshot = runtime.current_snapshot()?;
    let actual = snapshot
        .math_text_caret_geometry(&context.target, &context.focus, context.visual_affinity)
        .ok_or_else(|| std::io::Error::other("Align caret geometry is unavailable"))?;
    let mut candidate_x = context
        .surrounding_text
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(context.surrounding_text.len()))
        .map(|offset| {
            let mut position = context.focus.clone();
            position.offset = offset;
            snapshot
                .math_text_caret_geometry(&context.target, &position, context.visual_affinity)
                .map(|caret| caret.x_pt)
                .ok_or_else(|| {
                    std::io::Error::other("Align candidate caret geometry is unavailable")
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    candidate_x.sort_by(f64::total_cmp);
    candidate_x.dedup_by(|left, right| (*left - *right).abs() < 1e-6);
    assert!(
        candidate_x.len() > 1,
        "Align fixture must have distinct caret positions"
    );
    let actual_distance = (actual.x_pt - target_x).abs();
    let nearest_distance = candidate_x
        .iter()
        .map(|x| (x - target_x).abs())
        .fold(f64::INFINITY, f64::min);
    assert!((actual_distance - nearest_distance).abs() < 1e-6);
    let nearest_count = candidate_x
        .iter()
        .filter(|x| ((**x - target_x).abs() - nearest_distance).abs() < 1e-6)
        .count();
    assert_eq!(
        nearest_count, 1,
        "Align fixture must not have tied nearest carets"
    );
    Ok(())
}

#[test]
fn drag_hit_map_keeps_soft_break_carets_but_omits_its_highlight_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("你好".to_owned())?;
    runtime.insert_soft_break_json()?;
    runtime.insert_text_json("你好".to_owned())?;

    let snapshot: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    let text = snapshot["paragraphs"][0]["text"]
        .as_str()
        .ok_or_else(|| std::io::Error::other("paragraph text was omitted"))?;
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let segments = hit_map["segments"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("drag segments were omitted"))?;
    assert_eq!(segments.len(), 4);
    for segment in segments {
        let start = segment["startUtf8"]
            .as_str()
            .ok_or_else(|| std::io::Error::other("segment start was omitted"))?
            .parse::<usize>()?;
        let end = segment["endUtf8"]
            .as_str()
            .ok_or_else(|| std::io::Error::other("segment end was omitted"))?
            .parse::<usize>()?;
        assert_ne!(&text[start..end], "\n");
    }
    let stops = hit_map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("drag stops were omitted"))?;
    for offset in ["6", "7"] {
        assert!(
            stops
                .iter()
                .any(|stop| stop["position"]["offsetUtf8"] == offset)
        );
    }
    Ok(())
}

#[test]
fn visual_navigation_keeps_selection_and_vertical_line_target()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("alpha".to_owned())?;
    runtime.move_to_visual_line_edge_json(false, false)?;
    assert_eq!(runtime.session.selection().focus.offset, 0);

    let moved = runtime.move_visual_caret_json(true, false)?;
    let moved_snapshot: Value = serde_json::from_str(&moved)?;
    assert!(
        moved_snapshot["inputMethodContext"]
            .get("surroundingText")
            .is_none()
    );
    let selected = runtime.move_visual_caret_json(true, true)?;
    assert_eq!(runtime.session.selection().anchor.offset, 1);
    assert_eq!(runtime.session.selection().focus.offset, 2);
    let selected_snapshot: Value = serde_json::from_str(&selected)?;
    assert_eq!(
        selected_snapshot["selectionRects"].as_array().map(Vec::len),
        Some(1)
    );
    let replaced = runtime.insert_text_json("XY".to_owned())?;
    assert_eq!(
        serde_json::from_str::<Value>(&replaced)?["paragraphs"][0]["text"],
        "aXYpha"
    );

    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("ab".to_owned())?;
    runtime.insert_soft_break_json()?;
    runtime.insert_text_json("cd".to_owned())?;
    runtime.move_to_visual_line_edge_json(false, false)?;
    let before = serde_json::from_str::<Value>(&runtime.current_caret_json()?)?;
    runtime.move_vertical_caret_json(-1, false)?;
    let after = serde_json::from_str::<Value>(&runtime.current_caret_json()?)?;
    assert_eq!(runtime.session.selection().focus.offset, 0);
    let after_y = after["y0Pt"]
        .as_f64()
        .ok_or_else(|| std::io::Error::other("vertical navigation omitted caret y"))?;
    let before_y = before["y0Pt"]
        .as_f64()
        .ok_or_else(|| std::io::Error::other("caret geometry omitted y"))?;
    assert!(after_y < before_y);
    assert_eq!(runtime.session.document_generation(), 3);
    Ok(())
}

#[test]
fn visual_navigation_crosses_paragraph_boundaries() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("first".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("second".to_owned())?;
    let paragraph_ids: Vec<_> = runtime
        .current_snapshot()?
        .paragraphs
        .iter()
        .map(|paragraph| paragraph.id.clone())
        .collect();
    assert_eq!(paragraph_ids.len(), 2);

    runtime.move_to_visual_line_edge_json(false, false)?;
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        paragraph_ids[1]
    );
    assert_eq!(runtime.session.selection().focus.offset, 0);
    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(false, false)?)?;
    assert!(interaction.get("paragraphs").is_none());
    assert!(interaction.get("selectionRects").is_some());
    assert_eq!(
        interaction["inputMethodContext"]["surroundingText"],
        "first"
    );
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        paragraph_ids[0]
    );
    assert_eq!(runtime.session.selection().focus.offset, "first".len());
    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        paragraph_ids[1]
    );
    assert_eq!(runtime.session.selection().focus.offset, 0);
    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(false, false)?)?;
    assert_eq!(
        interaction["inputMethodContext"]["surroundingText"],
        "first"
    );

    runtime.move_to_visual_line_edge_json(false, false)?;
    runtime.move_vertical_caret_json(-1, false)?;
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        paragraph_ids[0]
    );
    Ok(())
}

#[test]
fn align_vertical_navigation_moves_between_row_and_neighboring_paragraph_edges()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = align_document(&["first", "second"]);
    document.body.remove(0);
    let align_id = match &document.body[0] {
        Block::Align(align) => align.id.clone(),
        _ => unreachable!(),
    };
    let before = paragraph_with_text("before");
    let before_id = before.id.clone();
    document.body.insert(0, Block::Paragraph(before));
    let mut runtime = DocumentViewRuntime::new(document)?;
    place_caret_in_align_cell(&mut runtime, 1, 0, 3)?;
    let target_x = align_caret_x(&mut runtime)?;
    let generation = runtime.session.document_generation();

    runtime.move_vertical_caret_json(-1, false)?;
    let context = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Up left Align text focus"))?;
    assert_eq!(context.focus.path.align_cell(), Some((0, 0)));
    assert_eq!(context.focus.offset, 0);
    assert_align_caret_is_nearest_to_x(&mut runtime, target_x)?;
    assert!(context.surrounding_text.contains("first"));

    runtime.move_vertical_caret_json(-1, false)?;
    assert!(runtime.session.math_edit().is_none());
    assert_eq!(runtime.session.selection().focus.paragraph_id, before_id);
    assert_eq!(runtime.session.selection().focus.offset, "before".len());

    runtime.move_vertical_caret_json(1, false)?;
    let reentered = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Down did not reenter Align"))?;
    assert_eq!(reentered.focus.path.align_cell(), Some((0, 0)));
    assert_eq!(reentered.focus.offset, 0);

    runtime.move_vertical_caret_json(1, false)?;
    let returned = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Down left Align text focus"))?;
    assert_eq!(returned.focus.path.align_cell(), Some((1, 0)));
    assert_eq!(returned.focus.offset, 3);
    assert_align_caret_is_nearest_to_x(&mut runtime, target_x)?;
    runtime.move_vertical_caret_json(1, false)?;
    assert!(runtime.session.math_edit().is_none());
    assert_eq!(
        runtime.session.block_boundary_exit(),
        Some((align_id, caret_engine::Direction::Forward))
    );
    runtime.move_vertical_caret_json(-1, false)?;
    let reentered_bottom = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Up did not reenter Align at its bottom edge"))?;
    assert_eq!(reentered_bottom.focus.path.align_cell(), Some((1, 0)));
    assert_eq!(reentered_bottom.focus.offset, "second".len());
    assert_eq!(runtime.session.document_generation(), generation);
    Ok(())
}

#[test]
fn align_vertical_navigation_preserves_target_x_across_short_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new(align_document(&["abcdefghij", "x", "abcdefghij"]))?;
    place_caret_in_align_cell(&mut runtime, 0, 0, 8)?;
    let target_x = align_caret_x(&mut runtime)?;

    runtime.move_vertical_caret_json(1, false)?;
    let short_row = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Down left Align text focus"))?;
    assert_eq!(short_row.focus.path.align_cell(), Some((1, 0)));
    assert_eq!(short_row.focus.offset, 0);
    assert_align_caret_is_nearest_to_x(&mut runtime, target_x)?;

    runtime.move_vertical_caret_json(1, false)?;
    let lower_row = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("second Down left Align text focus"))?;
    assert_eq!(lower_row.focus.path.align_cell(), Some((2, 0)));
    assert_eq!(lower_row.focus.offset, 8);
    assert_align_caret_is_nearest_to_x(&mut runtime, target_x)?;

    runtime.move_vertical_caret_json(-1, false)?;
    let short_row_again = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Up left Align text focus"))?;
    assert_eq!(short_row_again.focus.path.align_cell(), Some((1, 0)));
    assert_eq!(short_row_again.focus.offset, 0);
    assert_align_caret_is_nearest_to_x(&mut runtime, target_x)?;
    runtime.move_vertical_caret_json(-1, false)?;
    let upper_row = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("second Up left Align text focus"))?;
    assert_eq!(upper_row.focus.path.align_cell(), Some((0, 0)));
    assert_eq!(upper_row.focus.offset, 8);
    assert_align_caret_is_nearest_to_x(&mut runtime, target_x)?;
    Ok(())
}

#[test]
fn align_vertical_navigation_enters_an_empty_cell_and_recovers_text_focus()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new(align_document(&["top", "", "bottom"]))?;
    place_caret_in_align_cell(&mut runtime, 0, 0, 2)?;

    runtime.move_vertical_caret_json(1, false)?;
    let empty_cell = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("empty Align cell did not receive text focus"))?;
    assert_eq!(empty_cell.focus.path.align_cell(), Some((1, 0)));
    assert_eq!(empty_cell.focus.offset, 0);
    assert_eq!(empty_cell.surrounding_text, "");

    runtime.insert_text_json("z".to_owned())?;
    let edited_cell = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("typing left the empty Align cell"))?;
    assert_eq!(edited_cell.focus.path.align_cell(), Some((1, 0)));
    assert!(edited_cell.surrounding_text.contains('z'));

    runtime.move_vertical_caret_json(1, false)?;
    let lower_text = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Down from empty cell lost Align text focus"))?;
    assert_eq!(lower_text.focus.path.align_cell(), Some((2, 0)));
    assert!(lower_text.surrounding_text.contains("bottom"));
    Ok(())
}

#[test]
fn shift_down_at_an_align_text_island_boundary_keeps_selection_stable()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new(align_document(&["first", "second"]))?;
    place_caret_in_align_cell(&mut runtime, 0, 0, 2)?;

    runtime.move_vertical_caret_json(1, true)?;
    let first_attempt = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Shift+Down lost Align text focus"))?;
    assert_eq!(first_attempt.anchor, first_attempt.focus);
    assert_eq!(first_attempt.focus.path.align_cell(), Some((0, 0)));
    assert_eq!(first_attempt.focus.offset, 2);

    runtime.move_vertical_caret_json(1, true)?;
    let repeated_attempt = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("repeated Shift+Down lost Align text focus"))?;
    assert_eq!(repeated_attempt.anchor, first_attempt.anchor);
    assert_eq!(repeated_attempt.focus, first_attempt.focus);

    runtime.move_vertical_caret_json(1, false)?;
    let resumed = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("Down after Shift+Down lost Align text focus"))?;
    assert_eq!(resumed.focus.path.align_cell(), Some((1, 0)));
    Ok(())
}

#[test]
fn shift_navigation_selects_an_environment_across_its_title_story_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Title", "body");
    let (environment_id, title_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            environment
                .title
                .as_ref()
                .ok_or_else(|| std::io::Error::other("Section title is missing"))?
                .id
                .clone(),
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![section_block];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let title_end = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "5",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": title_end.clone(), "focus": title_end}).to_string(),
    )?;

    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, true)?)?;

    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    let selection_rects = interaction["selectionRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("environment selection geometry is missing"))?;
    let environment_ranges = &runtime.current_snapshot()?.environment_ranges;
    assert_eq!(selection_rects.len(), environment_ranges.len());
    assert!(!selection_rects.is_empty());
    assert_eq!(
        interaction["blockEnvironmentHighlightRects"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );

    let section_block = section("Title", "body");
    let (environment_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => return Err("Section body should begin with a paragraph".into()),
            },
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        section_block,
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let body_end = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "4",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": body_end.clone(), "focus": body_end}).to_string(),
    )?;
    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, true)?)?;
    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    assert!(
        !interaction["selectionRects"]
            .as_array()
            .ok_or_else(|| std::io::Error::other("environment selection geometry is missing"))?
            .is_empty()
    );
    Ok(())
}

#[test]
fn pointer_selection_rejects_invalid_utf8_positions_before_selecting_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("标题", "正文");
    let (environment_id, title_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            section_title_id(environment)?,
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => return Err("Section body should be a paragraph".into()),
            },
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![section_block];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let anchor = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let invalid_focus = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "1",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });

    let result = runtime.set_selection_json(
        &serde_json::json!({"anchor": anchor, "focus": invalid_focus}).to_string(),
    );

    assert!(result.is_err());
    assert_eq!(runtime.session.selected_block_environment(), None);
    assert_ne!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    Ok(())
}

#[test]
fn pointer_selection_preserves_text_before_a_section_and_selects_the_section()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Section", "body");
    let (environment_id, title_id) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("outside")),
        section_block,
    ];
    let outside_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    let anchor = serde_json::json!({
        "paragraphId": outside_id.to_string(),
        "offsetUtf8": "2",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let focus = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "7",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });

    let interaction: Value =
        serde_json::from_str(&runtime.set_selection_json(
            &serde_json::json!({"anchor": anchor, "focus": focus}).to_string(),
        )?)?;

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[environment_id]
    );
    let environment_range_count = runtime.current_snapshot()?.environment_ranges.len();
    assert!(
        interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| rectangles.len() > environment_range_count)
    );
    assert_eq!(
        runtime
            .session
            .structure_selection_anchor()
            .ok_or_else(|| std::io::Error::other("structure selection anchor is missing"))?
            .paragraph_id,
        outside_id
    );
    Ok(())
}

#[test]
fn pointer_selection_can_cover_two_adjacent_block_environments()
-> Result<(), Box<dyn std::error::Error>> {
    let first = section("First", "one");
    let second = section("Second", "two");
    let (first_id, first_title_id) = match &first {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let (second_id, second_title_id) = match &second {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        first,
        Block::Paragraph(paragraph_with_text("middle")),
        second,
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let anchor = serde_json::json!({
        "paragraphId": first_title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let focus = serde_json::json!({
        "paragraphId": second_title_id.to_string(),
        "offsetUtf8": "6",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });

    let interaction: Value =
        serde_json::from_str(&runtime.set_selection_json(
            &serde_json::json!({"anchor": anchor, "focus": focus}).to_string(),
        )?)?;

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[first_id, second_id]
    );
    let selection_rects = interaction["selectionRects"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("selection rectangles are missing"))?;
    assert!(selection_rects.len() > runtime.current_snapshot()?.environment_ranges.len());
    Ok(())
}

#[test]
fn pointer_selection_from_outside_to_outside_includes_the_intervening_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Section", "body");
    let (environment_id, _) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        section_block,
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let before_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let after_id = match &document.body[2] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    let anchor = serde_json::json!({
        "paragraphId": before_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let focus = serde_json::json!({
        "paragraphId": after_id.to_string(),
        "offsetUtf8": "5",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });

    let interaction: Value =
        serde_json::from_str(&runtime.set_selection_json(
            &serde_json::json!({"anchor": anchor, "focus": focus}).to_string(),
        )?)?;

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[environment_id]
    );
    let environment_range_count = runtime.current_snapshot()?.environment_ranges.len();
    assert!(
        interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| { rectangles.len() >= environment_range_count + 2 })
    );
    Ok(())
}

#[test]
fn shift_right_keeps_a_contiguous_selection_across_paragraph_section_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Section", "inside");
    let (environment_id, _) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        section_block,
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let before_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let after_id = match &document.body[2] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    let start = serde_json::json!({
        "paragraphId": before_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": start.clone(), "focus": start}).to_string(),
    )?;

    for _ in 0..32 {
        runtime.move_visual_caret_json(true, true)?;
        if let Some((_, focus)) = runtime.session.structure_selection()
            && focus.paragraph_id == after_id
            && focus.offset == 5
        {
            break;
        }
    }

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[environment_id]
    );
    assert_eq!(
        runtime
            .session
            .structure_selection()
            .ok_or_else(|| std::io::Error::other("structure selection was lost"))?
            .1
            .paragraph_id,
        after_id
    );
    let copied: Value = serde_json::from_str(&runtime.selected_text_json()?)?;
    assert_eq!(copied["text"], "before\nSection\ninside\nafter");
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    let environment_range_count = runtime.current_snapshot()?.environment_ranges.len();
    assert!(
        interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| { rectangles.len() >= environment_range_count + 2 })
    );
    Ok(())
}

#[test]
fn shift_down_keeps_a_contiguous_selection_across_paragraph_section_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Section", "inside");
    let (environment_id, _) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        section_block,
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let before_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let after_id = match &document.body[2] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    let start = serde_json::json!({
        "paragraphId": before_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": start.clone(), "focus": start}).to_string(),
    )?;

    for _ in 0..8 {
        runtime.move_vertical_caret_json(1, true)?;
        if runtime
            .session
            .structure_selection()
            .is_some_and(|(_, focus)| focus.paragraph_id == after_id)
        {
            break;
        }
    }

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[environment_id]
    );
    assert_eq!(
        runtime
            .session
            .structure_selection()
            .ok_or_else(|| std::io::Error::other("structure selection was lost"))?
            .1
            .paragraph_id,
        after_id
    );
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    let environment_range_count = runtime.current_snapshot()?.environment_ranges.len();
    assert!(
        interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| { rectangles.len() > environment_range_count })
    );
    Ok(())
}

#[test]
fn shift_vertical_selects_section_across_its_title_story_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Title", "body");
    let (environment_id, title_id) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![section_block];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let title_start = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": title_start.clone(), "focus": title_start}).to_string(),
    )?;

    runtime.move_vertical_caret_json(1, true)?;

    assert_eq!(
        runtime.session.selected_block_environments(),
        &[environment_id]
    );
    Ok(())
}

#[test]
fn shift_horizontal_keeps_the_original_paragraph_anchor_when_entering_a_section()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Section", "body");
    let (environment_id, title_id) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("outside")),
        section_block,
    ];
    let outside_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let mut runtime = DocumentViewRuntime::new(document)?;
    let outside_end = serde_json::json!({
        "paragraphId": outside_id.to_string(),
        "offsetUtf8": "7",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": outside_end.clone(), "focus": outside_end}).to_string(),
    )?;

    for _ in 0..4 {
        runtime.move_visual_caret_json(true, true)?;
        if !runtime.session.selected_block_environments().is_empty() {
            break;
        }
    }

    assert_eq!(
        runtime.session.selected_block_environments(),
        std::slice::from_ref(&environment_id)
    );
    assert!(runtime.session.has_structure_selection());
    // 环境完整覆盖时，环境外的文本锚点保持原段落位置；焦点停到环境外后边界。
    let (anchor, focus) = runtime
        .session
        .structure_selection_range()
        .ok_or_else(|| std::io::Error::other("structure selection range is missing"))?;
    assert!(
        matches!(
            anchor,
            caret_engine::DocumentRangeEndpoint::Text(position)
                if position.paragraph_id == outside_id
                    && position.offset == 7
                    && position.affinity == caret_engine::Affinity::Upstream
        ),
        "the original paragraph anchor must survive environment promotion"
    );
    assert!(
        matches!(
            focus,
            caret_engine::DocumentRangeEndpoint::BlockEdge {
                block_id,
                side: caret_engine::Direction::Forward,
            } if block_id == &environment_id
        ),
        "the structure focus must be the environment forward edge"
    );
    // 停靠光标位于环境标题起点，不属于结构端点本身。
    let parked = runtime.session.selection();
    assert_eq!(parked.anchor, parked.focus);
    assert_eq!(parked.focus.paragraph_id, title_id);
    assert_eq!(parked.focus.offset, 0);
    Ok(())
}

#[test]
fn visual_navigation_stops_between_adjacent_environments_without_creating_a_line()
-> Result<(), Box<dyn std::error::Error>> {
    let first = section("first", "body");
    let second = section("second", "body");
    let first_body_id = match &first {
        Block::Environment(environment) => match &environment.children[0] {
            Block::Paragraph(paragraph) => paragraph.id.clone(),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![first, second];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let endpoint = serde_json::json!({
        "paragraphId": first_body_id.to_string(),
        "offsetUtf8": "4",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": endpoint.clone(), "focus": endpoint}).to_string(),
    )?;

    let outside: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, false)?)?;
    assert_eq!(runtime.document().body.len(), 2);
    assert!(runtime.session.block_boundary_exit().is_some());
    assert_eq!(
        outside["currentStructure"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(
        outside["blockEnvironmentHighlightRects"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert!(matches!(runtime.document().body[0], Block::Environment(_)));
    assert!(matches!(runtime.document().body[1], Block::Environment(_)));
    Ok(())
}

#[test]
fn horizontal_navigation_keeps_the_boundary_stop_for_an_empty_environment_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let first = section("First", "");
    let second = section("Second", "");
    let (first_environment_id, first_body_id) = match &first {
        Block::Environment(environment) => (
            environment.id.clone(),
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => unreachable!(),
            },
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![first, second];
    let mut runtime = DocumentViewRuntime::new(document)?;
    set_body_caret(&mut runtime, first_body_id.clone(), 0)?;
    let generation = runtime.document_generation();

    runtime.move_visual_caret_json(true, false)?;

    assert_eq!(runtime.document_generation(), generation);
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        first_body_id
    );
    assert_eq!(
        runtime.session.block_boundary_exit(),
        Some((first_environment_id, caret_engine::Direction::Forward))
    );
    Ok(())
}

#[test]
fn repeated_arrow_from_outer_stop_enters_the_adjacent_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let first = section("First", "one");
    let second = section("Second", "two");
    let (first_environment_id, first_body_id) = match &first {
        Block::Environment(environment) => (
            environment.id.clone(),
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => unreachable!(),
            },
        ),
        _ => unreachable!(),
    };
    let (second_environment_id, second_title_id) = match &second {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![first, second];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let first_body_end = serde_json::json!({
        "paragraphId": first_body_id.to_string(),
        "offsetUtf8": "3",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": first_body_end.clone(), "focus": first_body_end}).to_string(),
    )?;
    let generation = runtime.document_generation();

    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(
        runtime.session.block_boundary_exit(),
        Some((first_environment_id, caret_engine::Direction::Forward))
    );
    runtime.move_visual_caret_json(true, false)?;

    assert_eq!(runtime.document_generation(), generation);
    assert!(runtime.session.block_boundary_exit().is_none());
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        second_title_id
    );
    let interaction: Value = serde_json::from_str(&runtime.interaction_state_json()?)?;
    assert_eq!(
        interaction["currentStructure"][0]["id"],
        second_environment_id.to_string()
    );
    assert_eq!(interaction["currentStructure"][0]["kind"], "section");

    let first = section("First", "one");
    let second = section("Second", "two");
    let (first_body_id, second_environment_id, second_title_id) = match (&first, &second) {
        (Block::Environment(first), Block::Environment(second)) => (
            match &first.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => unreachable!(),
            },
            second.id.clone(),
            section_title_id(second)?,
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![first, second];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let second_title_start = serde_json::json!({
        "paragraphId": second_title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": second_title_start.clone(), "focus": second_title_start})
            .to_string(),
    )?;
    runtime.move_visual_caret_json(false, false)?;
    assert_eq!(
        runtime.session.block_boundary_exit(),
        Some((second_environment_id, caret_engine::Direction::Backward))
    );
    runtime.move_visual_caret_json(false, false)?;
    assert!(runtime.session.block_boundary_exit().is_none());
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        first_body_id
    );
    assert_eq!(runtime.session.selection().focus.offset, "one".len());
    Ok(())
}

#[test]
fn typing_after_structural_navigation_uses_the_visible_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Title", "body");
    let (section_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => return Err("Section body should be a paragraph".into()),
            },
        ),
        _ => unreachable!(),
    };
    let after = paragraph_with_text("after");
    let after_paragraph_id = after.id.clone();
    let mut document = Document::blank();
    document.body = vec![section_block, Block::Paragraph(after)];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let end_of_body = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "4",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": end_of_body.clone(), "focus": end_of_body}).to_string(),
    )?;

    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(
        runtime.session.block_boundary_exit(),
        Some((section_id, caret_engine::Direction::Forward))
    );
    runtime.move_structure_json("next")?;

    let snapshot: Value = serde_json::from_str(&runtime.insert_text_delta_json("X".to_owned())?)?;
    assert_eq!(snapshot["partial"], true);
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        after_paragraph_id
    );
    assert_eq!(block_text(&runtime.document().body[1]), Some("Xafter"));
    Ok(())
}

#[test]
fn typing_at_environment_exit_uses_a_full_snapshot_for_the_new_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Title", "body");
    let (section_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => return Err("Section body should be a paragraph".into()),
            },
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        section_block,
        Block::Paragraph(paragraph_with_text("after")),
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let end_of_body = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "4",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": end_of_body.clone(), "focus": end_of_body}).to_string(),
    )?;
    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(
        runtime.session.block_boundary_exit(),
        Some((section_id, caret_engine::Direction::Forward))
    );

    let snapshot: Value = serde_json::from_str(&runtime.insert_text_delta_json("X".to_owned())?)?;

    assert!(snapshot.get("partial").is_none());
    assert_eq!(runtime.document().body.len(), 3);
    assert_eq!(block_text(&runtime.document().body[1]), Some("X"));
    assert_eq!(runtime.session.block_boundary_exit(), None);
    Ok(())
}

#[test]
fn pointer_selection_across_section_title_and_body_selects_the_whole_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Title", "body");
    let (environment_id, title_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            section_title_id(environment)?,
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => return Err("Section body should be a paragraph".into()),
            },
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![section_block];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let title_start = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let body_end = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "4",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });

    let interaction: Value = serde_json::from_str(&runtime.set_selection_json(
        &serde_json::json!({"anchor": title_start, "focus": body_end}).to_string(),
    )?)?;

    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    assert_eq!(runtime.session.selection().focus.paragraph_id, body_id);
    assert_eq!(runtime.session.selection().focus.offset, 4);
    assert!(
        interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    let body_end = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "4",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    let title_start = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": body_end, "focus": title_start}).to_string(),
    )?;
    assert_eq!(runtime.session.selection().focus.paragraph_id, title_id);
    assert_eq!(runtime.session.selection().focus.offset, 0);
    Ok(())
}

#[test]
fn shift_left_from_section_start_selects_the_whole_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Title", "body");
    let (environment_id, title_id) = match &section_block {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        section_block,
    ];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let title_start = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": title_start.clone(), "focus": title_start}).to_string(),
    )?;

    let interaction: Value = serde_json::from_str(&runtime.move_visual_caret_json(false, true)?)?;

    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    assert!(
        interaction["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    Ok(())
}

#[test]
fn escape_cancels_environment_selection_before_leaving_it() -> Result<(), Box<dyn std::error::Error>>
{
    let section = section("Heading", "body");
    let (environment_id, title_id) = match &section {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![section];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let title_end = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "7",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": title_end.clone(), "focus": title_end}).to_string(),
    )?;
    runtime.move_visual_caret_json(true, true)?;
    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    let generation = runtime.document_generation();

    let cancelled: Value = serde_json::from_str(&runtime.handle_editor_key_json("escape")?)?;

    assert!(runtime.session.selected_block_environment().is_none());
    assert_eq!(runtime.document_generation(), generation);
    assert_eq!(runtime.document().body.len(), 1);
    assert_eq!(
        cancelled["currentStructure"][0]["id"],
        environment_id.to_string()
    );
    Ok(())
}

#[test]
fn deleting_selected_environment_through_runtime_is_undoable()
-> Result<(), Box<dyn std::error::Error>> {
    let section = section("Heading", "body");
    let (environment_id, title_id) = match &section {
        Block::Environment(environment) => (environment.id.clone(), section_title_id(environment)?),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![section];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let title_end = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "7",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": title_end.clone(), "focus": title_end}).to_string(),
    )?;
    runtime.move_visual_caret_json(true, true)?;
    assert_eq!(
        runtime.session.selected_block_environment(),
        Some(&environment_id)
    );
    let generation = runtime.document_generation();

    runtime.delete_backward_json()?;

    assert!(runtime.session.selected_block_environment().is_none());
    assert_eq!(runtime.document_generation(), generation + 1);
    assert!(
        runtime
            .document()
            .body
            .iter()
            .all(|block| !matches!(block, Block::Environment(_)))
    );
    runtime.undo_json()?;
    assert!(
        matches!(&runtime.document().body[0], Block::Environment(environment) if environment.id == environment_id)
    );
    Ok(())
}

#[test]
fn environment_virtual_stops_use_outer_geometry_and_insert_outside_paragraphs()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Heading", "inside");
    let (environment_id, title_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            section_title_id(environment)?,
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => unreachable!(),
            },
        ),
        _ => unreachable!(),
    };

    let mut document = Document::blank();
    document.body = vec![section_block.clone()];
    let mut before_runtime = DocumentViewRuntime::new(document)?;
    let title_start = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    before_runtime.set_selection_json(
        &serde_json::json!({"anchor": title_start.clone(), "focus": title_start}).to_string(),
    )?;
    let outside_before: Value =
        serde_json::from_str(&before_runtime.move_visual_caret_json(false, false)?)?;
    assert_eq!(
        before_runtime.session.block_boundary_exit(),
        Some((environment_id.clone(), caret_engine::Direction::Backward))
    );
    assert_eq!(
        outside_before["blockEnvironmentHighlightRects"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    let range_x0 = before_runtime.current_snapshot()?.environment_ranges[0].x0_pt;
    assert_eq!(
        outside_before["caretGeometry"]["pageIndex"],
        before_runtime.current_snapshot()?.environment_ranges[0].page_index
    );
    assert_eq!(
        outside_before["caretGeometry"]["xPt"].as_f64(),
        Some(range_x0)
    );
    let complete_caret: Value = serde_json::from_str(&before_runtime.current_caret_json()?)?;
    assert_eq!(complete_caret["xPt"].as_f64(), Some(range_x0));

    before_runtime.insert_text_json("before".to_owned())?;
    assert_eq!(before_runtime.document().body.len(), 2);
    assert_eq!(
        block_text(&before_runtime.document().body[0]),
        Some("before")
    );
    assert_eq!(
        before_runtime.session.selection().focus.offset,
        "before".len()
    );
    assert_ne!(
        before_runtime.session.selection().focus.paragraph_id,
        title_id
    );
    assert!(matches!(
        &before_runtime.document().body[1],
        Block::Environment(environment) if environment.id == environment_id
    ));

    let mut document = Document::blank();
    document.body = vec![section_block];
    let mut after_runtime = DocumentViewRuntime::new(document)?;
    let body_end = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "6",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    after_runtime.set_selection_json(
        &serde_json::json!({"anchor": body_end.clone(), "focus": body_end}).to_string(),
    )?;
    let outside_after: Value =
        serde_json::from_str(&after_runtime.move_visual_caret_json(true, false)?)?;
    assert_eq!(
        after_runtime.session.block_boundary_exit(),
        Some((environment_id.clone(), caret_engine::Direction::Forward))
    );
    let range = after_runtime
        .current_snapshot()?
        .environment_ranges
        .iter()
        .find(|range| range.environment_id == environment_id)
        .ok_or_else(|| std::io::Error::other("Section layout range is missing"))?;
    assert_eq!(
        outside_after["caretGeometry"]["pageIndex"],
        range.page_index
    );
    assert_eq!(
        outside_after["caretGeometry"]["xPt"].as_f64(),
        Some(range.x1_pt)
    );

    after_runtime.insert_text_json("after".to_owned())?;
    assert_eq!(after_runtime.document().body.len(), 2);
    assert!(matches!(
        &after_runtime.document().body[0],
        Block::Environment(environment) if environment.id == environment_id
    ));
    assert_eq!(block_text(&after_runtime.document().body[1]), Some("after"));
    assert_eq!(
        after_runtime.session.selection().focus.offset,
        "after".len()
    );
    assert_ne!(
        after_runtime.session.selection().focus.paragraph_id,
        body_id
    );
    Ok(())
}

#[test]
fn existing_paragraphs_do_not_skip_environment_outer_stops()
-> Result<(), Box<dyn std::error::Error>> {
    let section_block = section("Heading", "inside");
    let (environment_id, title_id, body_id) = match &section_block {
        Block::Environment(environment) => (
            environment.id.clone(),
            section_title_id(environment)?,
            match &environment.children[0] {
                Block::Paragraph(paragraph) => paragraph.id.clone(),
                _ => unreachable!(),
            },
        ),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph_with_text("before")),
        section_block,
        Block::Paragraph(paragraph_with_text("after")),
    ];

    let mut before_runtime = DocumentViewRuntime::new(document.clone())?;
    let start = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    before_runtime.set_selection_json(
        &serde_json::json!({"anchor": start.clone(), "focus": start}).to_string(),
    )?;
    let outside: Value =
        serde_json::from_str(&before_runtime.move_visual_caret_json(false, false)?)?;
    assert_eq!(
        before_runtime.session.block_boundary_exit(),
        Some((environment_id.clone(), caret_engine::Direction::Backward))
    );
    assert_eq!(
        outside["blockEnvironmentHighlightRects"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    before_runtime.insert_text_json("inserted before".to_owned())?;
    assert_eq!(before_runtime.document().body.len(), 4);
    assert_eq!(
        block_text(&before_runtime.document().body[1]),
        Some("inserted before")
    );
    assert!(matches!(
        before_runtime.document().body[2],
        Block::Environment(_)
    ));

    let mut after_runtime = DocumentViewRuntime::new(document)?;
    let end = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "6",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    after_runtime.set_selection_json(
        &serde_json::json!({"anchor": end.clone(), "focus": end}).to_string(),
    )?;
    let outside: Value = serde_json::from_str(&after_runtime.move_visual_caret_json(true, false)?)?;
    assert_eq!(
        after_runtime.session.block_boundary_exit(),
        Some((environment_id.clone(), caret_engine::Direction::Forward))
    );
    assert_eq!(
        outside["blockEnvironmentHighlightRects"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    let environment_range_x1 = after_runtime
        .current_snapshot()?
        .environment_ranges
        .iter()
        .find(|range| range.environment_id == environment_id)
        .ok_or_else(|| std::io::Error::other("Section outer geometry is missing"))?
        .x1_pt;
    let caret: Value = serde_json::from_str(&after_runtime.current_caret_json()?)?;
    assert_eq!(caret["xPt"].as_f64(), Some(environment_range_x1));
    after_runtime.insert_text_json("inserted after".to_owned())?;
    assert_eq!(after_runtime.document().body.len(), 4);
    assert!(matches!(
        after_runtime.document().body[1],
        Block::Environment(_)
    ));
    assert_eq!(
        block_text(&after_runtime.document().body[2]),
        Some("inserted after")
    );
    Ok(())
}

#[test]
fn rtl_visual_right_exit_uses_logical_start_boundary() -> Result<(), Box<dyn std::error::Error>> {
    let before = section("before", "body");
    let rtl = section("مرحبا", "متن");
    let title_id = match &rtl {
        Block::Environment(environment) => environment
            .title
            .as_ref()
            .ok_or_else(|| std::io::Error::other("RTL title is missing"))?
            .id
            .clone(),
        _ => unreachable!(),
    };
    let mut document = Document::blank();
    document.body = vec![before, rtl];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let endpoint = serde_json::json!({
        "paragraphId": title_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": endpoint.clone(), "focus": endpoint}).to_string(),
    )?;

    let outside: Value = serde_json::from_str(&runtime.move_visual_caret_json(true, false)?)?;
    assert_eq!(runtime.document().body.len(), 2);
    assert!(runtime.session.block_boundary_exit().is_some());
    assert_eq!(
        outside["currentStructure"].as_array().map(Vec::len),
        Some(0)
    );
    assert!(matches!(runtime.document().body[0], Block::Environment(_)));
    assert!(matches!(runtime.document().body[1], Block::Environment(_)));
    Ok(())
}

#[test]
fn exiting_nested_environment_reuses_existing_parent_paragraph()
-> Result<(), Box<dyn std::error::Error>> {
    let outer_title = paragraph_with_text("outer");
    let inner_paragraph = paragraph_with_text("inner");
    let inner_id = inner_paragraph.id.clone();
    let parent_paragraph = paragraph_with_text("parent");
    let parent_id = parent_paragraph.id.clone();
    let inner = Block::Environment(BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Custom,
        definition_id: None,
        props: BTreeMap::new(),
        title: None,
        children: vec![Block::Paragraph(inner_paragraph)],
        extra: BTreeMap::new(),
    });
    let outer = Block::Environment(BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(outer_title),
        children: vec![inner, Block::Paragraph(parent_paragraph)],
        extra: BTreeMap::new(),
    });
    let mut document = Document::blank();
    document.body = vec![outer];
    let mut runtime = DocumentViewRuntime::new(document)?;
    let endpoint = serde_json::json!({
        "paragraphId": inner_id.to_string(),
        "offsetUtf8": "5",
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": endpoint.clone(), "focus": endpoint}).to_string(),
    )?;
    let generation = runtime.document_generation();

    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(runtime.document_generation(), generation);
    assert!(runtime.session.block_boundary_exit().is_some());
    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(runtime.document().body.len(), 1);
    assert_eq!(runtime.document_generation(), generation);
    assert!(runtime.session.block_boundary_exit().is_none());
    assert_eq!(runtime.session.selection().focus.paragraph_id, parent_id);
    Ok(())
}

#[test]
fn section_title_navigation_and_backspace_keep_story_and_layout_in_sync()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("Heading".to_owned())?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "section".to_owned())?;
    runtime.command_line_command_json("accept", String::new())?;
    runtime.handle_editor_key_json("enter")?;
    runtime.insert_text_json("body".to_owned())?;

    let section_snapshot: Value = serde_json::from_str(&runtime.layout_snapshot_json()?)?;
    assert_eq!(
        section_snapshot["currentStructure"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(section_snapshot["currentStructure"][0]["kind"], "section");

    let generation = runtime.document_generation();
    runtime.move_visual_caret_json(true, true)?;
    assert_eq!(runtime.document_generation(), generation);
    assert!(runtime.session.selected_block_environment().is_some());
    assert_eq!(runtime.document().body.len(), 2);

    runtime.move_visual_caret_json(true, false)?;
    assert_eq!(runtime.document().body.len(), 2);
    assert!(matches!(runtime.document().body[1], Block::Environment(_)));
    assert!(runtime.session.block_boundary_exit().is_some());

    let Block::Environment(section) = &runtime.document().body[1] else {
        return Err("预期文档第二项为 Section".into());
    };
    let body_id = match &section.children[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => return Err("预期 Section 正文为段落".into()),
    };
    assert_eq!(
        runtime.session.selection().focus.story,
        caret_engine::StoryId::Body
    );
    let body_start = serde_json::json!({
        "paragraphId": body_id.to_string(),
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    runtime.set_selection_json(
        &serde_json::json!({"anchor": body_start.clone(), "focus": body_start}).to_string(),
    )?;
    let deleted: Value = serde_json::from_str(&runtime.delete_backward_json()?)?;
    assert_eq!(
        deleted["documentGeneration"],
        runtime.session.document_generation().to_string()
    );
    let Block::Environment(section) = &runtime.document().body[1] else {
        return Err("退格后 Section 应保留".into());
    };
    assert!(section.children.is_empty());
    assert_eq!(
        section.title.as_ref().map(|title| {
            title
                .inlines
                .iter()
                .filter_map(|inline| match inline {
                    ParagraphContent::Text(flow) => flow.runs.first().map(|run| run.text.as_str()),
                    _ => None,
                })
                .collect::<String>()
        }),
        Some("body".to_owned())
    );

    let restored: Value = serde_json::from_str(&runtime.undo_json()?)?;
    assert_eq!(
        restored["documentGeneration"],
        runtime.session.document_generation().to_string()
    );
    let Block::Environment(section) = &runtime.document().body[1] else {
        return Err("撤销后 Section 应保留".into());
    };
    assert_eq!(section.children.len(), 1);
    Ok(())
}

#[test]
fn section_title_drag_selection_uses_the_title_story() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("Heading".to_owned())?;
    runtime.command_line_command_json("begin", String::new())?;
    runtime.command_line_command_json("type", "section".to_owned())?;
    runtime.command_line_command_json("accept", String::new())?;

    let section_id = match &runtime.document().body[1] {
        Block::Environment(section) => section.id.clone(),
        _ => return Err("预期文档首项为 Section".into()),
    };
    let title_id = match &runtime.document().body[1] {
        Block::Environment(section) => section
            .title
            .as_ref()
            .ok_or_else(|| std::io::Error::other("Section 标题缺失"))?
            .id
            .clone(),
        _ => return Err("预期 Section 有标题".into()),
    };
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let stops = hit_map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("标题没有拖选停靠点"))?;
    let first = stops
        .iter()
        .find(|stop| stop["position"]["paragraphId"] == title_id.to_string())
        .ok_or_else(|| std::io::Error::other("标题首停靠点不存在"))?;
    let last = stops
        .iter()
        .filter(|stop| stop["position"]["paragraphId"] == title_id.to_string())
        .max_by_key(|stop| {
            stop["position"]["offsetUtf8"]
                .as_str()
                .and_then(|offset| offset.parse::<usize>().ok())
                .unwrap_or(0)
        })
        .ok_or_else(|| std::io::Error::other("标题末停靠点不存在"))?;
    let endpoint = |stop: &Value| {
        serde_json::json!({
            "paragraphId": stop["position"]["paragraphId"],
            "offsetUtf8": stop["position"]["offsetUtf8"],
            "affinity": stop["position"]["affinity"],
            "visualAffinity": stop["visualAffinity"],
        })
    };
    runtime.set_selection_json(
        &serde_json::json!({"anchor": endpoint(first), "focus": endpoint(last)}).to_string(),
    )?;
    let selected_text: Value = serde_json::from_str(&runtime.selected_text_json()?)?;
    assert_eq!(selected_text["text"], "");
    assert_eq!(
        runtime.session.selection().focus.story,
        caret_engine::StoryId::EnvironmentTitle(section_id)
    );
    Ok(())
}

#[test]
fn structure_navigation_commands_update_interaction_snapshot_without_editing_document()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("first".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("second".to_owned())?;
    let generation = runtime.session.document_generation();

    let previous: Value = serde_json::from_str(&runtime.move_structure_json("previous")?)?;
    assert_eq!(runtime.session.selection().focus.offset, "first".len());
    assert!(previous.get("caretGeometry").is_some());
    assert_eq!(runtime.session.document_generation(), generation);

    let next: Value = serde_json::from_str(&runtime.move_structure_json("next")?)?;
    assert_eq!(runtime.session.selection().focus.offset, 0);
    assert!(next.get("caretGeometry").is_some());
    assert_eq!(runtime.session.document_generation(), generation);
    Ok(())
}

#[test]
fn structure_selection_action_expands_through_the_public_runtime_entry_point()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("alpha".to_owned())?;
    runtime.split_paragraph_json()?;
    runtime.insert_text_json("beta".to_owned())?;
    runtime.move_to_visual_line_edge_json(false, false)?;

    let first: Value = serde_json::from_str(&runtime.move_structure_json("expand-selection")?)?;
    assert!(first["selectionRects"].is_array());
    assert_eq!(runtime.session.selection().anchor.offset, 0);
    assert_eq!(runtime.session.selection().focus.offset, "beta".len());
    Ok(())
}

#[test]
fn drag_hit_map_is_page_local_and_release_commits_one_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.insert_text_json("ab😀cd".to_owned())?;
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    assert_eq!(hit_map["pageIndex"], 0);
    assert_eq!(hit_map["documentGeneration"], "1");
    let stops = hit_map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("drag map omitted caret stops"))?;
    assert!(!stops.is_empty());
    let text = "ab😀cd";
    let mut grapheme_boundaries = text
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    grapheme_boundaries.push(text.len());
    for stop in stops {
        assert_eq!(stop["pageIndex"], 0);
        let offset = stop["position"]["offsetUtf8"]
            .as_str()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| std::io::Error::other("drag stop omitted a valid UTF-8 offset"))?;
        assert!(grapheme_boundaries.contains(&offset));
    }
    assert!(
        hit_map["segments"]
            .as_array()
            .is_some_and(|segments| !segments.is_empty())
    );
    let mut ordered = stops.to_vec();
    ordered.sort_by_key(|stop| {
        stop["position"]["offsetUtf8"]
            .as_str()
            .unwrap_or_default()
            .parse::<usize>()
            .unwrap_or(usize::MAX)
    });
    let to_endpoint = |stop: &Value| {
        serde_json::json!({
            "paragraphId": stop["position"]["paragraphId"],
            "offsetUtf8": stop["position"]["offsetUtf8"],
            "affinity": stop["position"]["affinity"],
            "visualAffinity": stop["visualAffinity"],
        })
    };
    let selection = serde_json::json!({
        "anchor": to_endpoint(&ordered[0]),
        "focus": to_endpoint(ordered.last().ok_or_else(|| std::io::Error::other("drag map has no stops"))?),
    });
    let response = runtime.set_selection_json(&selection.to_string())?;
    let snapshot: Value = serde_json::from_str(&response)?;
    assert_eq!(snapshot["documentGeneration"], "1");
    assert!(
        snapshot["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    assert_eq!(runtime.session.selection().anchor.offset, 0);
    assert_eq!(runtime.session.selection().focus.offset, "ab😀cd".len());
    let selected_text: Value = serde_json::from_str(&runtime.selected_text_json()?)?;
    assert_eq!(selected_text["text"], "ab😀cd");
    Ok(())
}

fn math_text_drag_runtime() -> Result<DocumentViewRuntime, String> {
    let math_tree = caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![caret_doc::MathContent::Text(caret_doc::TextFlow {
                runs: vec![Run {
                    text: "ab".to_owned(),
                    language: None,
                    props: BTreeMap::new(),
                    extra: BTreeMap::new(),
                }],
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    let mut document = Document::blank();
    document.body.push(Block::Equation(caret_doc::Equation {
        id: NodeId::generate(),
        math_tree,
        extra: BTreeMap::new(),
    }));
    DocumentViewRuntime::new(document)
}

#[test]
fn math_text_drag_reuses_the_shared_hit_map_and_replaces_the_selected_grapheme()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = math_text_drag_runtime()?;
    runtime.insert_text_json("q".to_owned())?;
    runtime.layout_snapshot_json()?;
    let session_id = runtime.session.session_id().clone();
    let generation = runtime.document_generation();
    assert!(generation > 0);
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let stops = hit_map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("drag map omitted stops"))?;
    let anchor = stops
        .iter()
        .find(|stop| {
            stop["position"]["kind"] == "math"
                && stop["position"]["orderUtf8"] == "0"
                && stop["visualAffinity"] == "downstream"
        })
        .ok_or_else(|| std::io::Error::other("math start stop is missing"))?;
    let focus = stops
        .iter()
        .find(|stop| {
            stop["position"]["kind"] == "math"
                && stop["position"]["orderUtf8"] == "1"
                && stop["visualAffinity"] == "downstream"
                && stop["domainId"] == anchor["domainId"]
        })
        .ok_or_else(|| std::io::Error::other("math focus stop is missing"))?;
    let math_segment = hit_map["segments"]
        .as_array()
        .and_then(|segments| {
            segments
                .iter()
                .find(|segment| segment["domainId"] == anchor["domainId"])
        })
        .ok_or_else(|| std::io::Error::other("math drag segment is missing"))?;
    assert_eq!(math_segment["startPosition"]["kind"], "math");
    assert!(math_segment["endPosition"]["orderUtf8"].is_string());
    let selection = serde_json::json!({
        "anchor": anchor["position"],
        "focus": focus["position"],
    });
    let snapshot: Value =
        serde_json::from_str(&runtime.set_selection_json(&selection.to_string())?)?;
    assert_eq!(runtime.session.session_id(), &session_id);
    assert_eq!(runtime.document_generation(), generation);
    assert!(
        snapshot["selectionRects"]
            .as_array()
            .is_some_and(|rectangles| !rectangles.is_empty())
    );
    let copied: Value = serde_json::from_str(&runtime.selected_text_json()?)?;
    assert_eq!(copied["text"], "a");
    runtime.insert_text_json("Z".to_owned())?;
    let context = runtime
        .session
        .math_text_input_context()
        .ok_or_else(|| std::io::Error::other("math drag lost formula focus"))?;
    assert_eq!(context.surrounding_text, "Zb");
    Ok(())
}

#[test]
fn stale_math_drag_stops_do_not_change_the_session() -> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = math_text_drag_runtime()?;
    runtime.layout_snapshot_json()?;
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let stop = hit_map["stops"]
        .as_array()
        .and_then(|stops| stops.iter().find(|stop| stop["position"]["kind"] == "math"))
        .ok_or_else(|| std::io::Error::other("math drag stop is missing"))?
        .clone();
    runtime.insert_text_json("body".to_owned())?;
    let before = runtime.session.document().clone();
    let generation = runtime.document_generation();
    let selection = serde_json::json!({
        "anchor": stop["position"],
        "focus": stop["position"],
    });
    assert_eq!(
        runtime
            .set_selection_json(&selection.to_string())
            .err()
            .as_deref(),
        Some("selection.stale_math_hit")
    );
    assert_eq!(runtime.document_generation(), generation);
    assert_eq!(runtime.session.document(), &before);
    assert!(runtime.session.math_text_input_context().is_none());
    Ok(())
}

#[test]
fn math_drag_stops_from_a_reopened_session_are_rejected() -> Result<(), Box<dyn std::error::Error>>
{
    let mut original = math_text_drag_runtime()?;
    original.layout_snapshot_json()?;
    let map: Value = serde_json::from_str(&original.drag_hit_map_json(0)?)?;
    let math_stops = map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("drag map omitted stops"))?
        .iter()
        .filter(|stop| stop["position"]["kind"] == "math")
        .collect::<Vec<_>>();
    let selection = serde_json::json!({
        "anchor": math_stops[0]["position"],
        "focus": math_stops[1]["position"],
    });
    let mut reopened = DocumentViewRuntime::new(original.session.document().clone())?;
    reopened.layout_snapshot_json()?;
    let before = reopened.session.document().clone();
    let result = reopened.set_selection_json(&selection.to_string());
    assert!(matches!(result, Err(error) if error == "selection.stale_math_hit"));
    assert_eq!(reopened.session.document(), &before);
    assert!(reopened.session.math_text_input_context().is_none());
    Ok(())
}

#[test]
fn math_drag_cancels_body_preedit_and_moves_focus_atomically()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = math_text_drag_runtime()?;
    runtime.input_method_event_json("候補".to_owned(), String::new(), 0, 0, 2)?;
    assert!(runtime.composition_id.is_some());
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let stops = hit_map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("drag map omitted stops"))?;
    let math_stops = stops
        .iter()
        .filter(|stop| stop["position"]["kind"] == "math")
        .collect::<Vec<_>>();
    let selection = serde_json::json!({
        "anchor": math_stops[0]["position"],
        "focus": math_stops[1]["position"],
    });
    let generation = runtime.document_generation();
    runtime.set_selection_json(&selection.to_string())?;
    assert!(runtime.composition_id.is_none());
    assert_eq!(runtime.document_generation(), generation);
    assert_eq!(
        runtime
            .session
            .math_text_input_context()
            .map(|context| context.surrounding_text),
        Some("ab".to_owned())
    );
    Ok(())
}

#[test]
fn zero_length_math_drag_enters_formula_text_without_changing_generation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = math_text_drag_runtime()?;
    runtime.layout_snapshot_json()?;
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let stop = hit_map["stops"]
        .as_array()
        .and_then(|stops| stops.iter().find(|stop| stop["position"]["kind"] == "math"))
        .ok_or_else(|| std::io::Error::other("math drag stop is missing"))?;
    let position = stop["position"].clone();
    let generation = runtime.document_generation();
    runtime.set_selection_json(
        &serde_json::json!({ "anchor": position, "focus": position }).to_string(),
    )?;
    assert!(runtime.session.math_text_input_context().is_some());
    assert_eq!(runtime.document_generation(), generation);
    Ok(())
}

#[test]
fn math_drag_during_math_preedit_rehits_original_text_positions()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = math_text_drag_runtime()?;
    runtime.layout_snapshot_json()?;
    let initial_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let initial_stop = initial_map["stops"]
        .as_array()
        .and_then(|stops| {
            stops.iter().find(|stop| {
                stop["position"]["kind"] == "math" && stop["position"]["orderUtf8"] == "1"
            })
        })
        .ok_or_else(|| std::io::Error::other("middle math drag stop is missing"))?;
    let position = initial_stop["position"].clone();
    runtime.set_selection_json(
        &serde_json::json!({ "anchor": position, "focus": position }).to_string(),
    )?;
    runtime.input_method_event_json("X".to_owned(), String::new(), 0, 0, 1)?;
    assert!(runtime.composition_id.is_some());
    let preview_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let math_stops = preview_map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("preview drag map omitted stops"))?
        .iter()
        .filter(|stop| stop["position"]["kind"] == "math")
        .collect::<Vec<_>>();
    let first = math_stops
        .iter()
        .find(|stop| stop["position"]["orderUtf8"] == "0")
        .ok_or_else(|| std::io::Error::other("preview start stop is missing"))?;
    let last = math_stops
        .iter()
        .max_by_key(|stop| {
            stop["position"]["orderUtf8"]
                .as_str()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0)
        })
        .ok_or_else(|| std::io::Error::other("preview end stop is missing"))?;
    let mut selection = serde_json::json!({
        "anchor": first["position"],
        "focus": last["position"],
    });
    // Preedit 文本可能将整块公式推到相邻页；原文重命中按 MathPath 和页面几何定位。
    selection["anchor"]["pageIndex"] =
        serde_json::json!(first["position"]["pageIndex"].as_u64().unwrap_or(0) + 1);
    selection["focus"]["pageIndex"] =
        serde_json::json!(last["position"]["pageIndex"].as_u64().unwrap_or(0) + 1);
    runtime.set_selection_json(&selection.to_string())?;
    assert!(runtime.composition_id.is_none());
    assert_eq!(
        runtime
            .session
            .math_text_input_context()
            .map(|context| context.surrounding_text),
        Some("ab".to_owned())
    );
    let selected: Value = serde_json::from_str(&runtime.selected_text_json()?)?;
    assert_eq!(selected["text"], "ab");
    Ok(())
}

#[test]
fn cross_paragraph_cjk_drag_stops_commit_a_valid_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    let Block::Paragraph(first) = &mut document.body[0] else {
        unreachable!();
    };
    first.inlines = vec![text_flow(Run {
        text: "你好".to_owned(),
        language: Some("zh".to_owned()),
        props: Default::default(),
        extra: Default::default(),
    })];
    let mut second = first.clone();
    second.id = caret_doc::NodeId::generate();
    second.inlines = vec![text_flow(Run {
        text: "世界".to_owned(),
        language: Some("zh".to_owned()),
        props: Default::default(),
        extra: Default::default(),
    })];
    document.body.push(Block::Paragraph(second));
    let mut runtime = DocumentViewRuntime::new(document)?;
    let hit_map: Value = serde_json::from_str(&runtime.drag_hit_map_json(0)?)?;
    let stops = hit_map["stops"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("drag map omitted caret stops"))?;
    let first_stop = stops
        .iter()
        .filter(|stop| stop["position"]["paragraphIndex"] == 0)
        .min_by_key(|stop| {
            stop["position"]["offsetUtf8"]
                .as_str()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(usize::MAX)
        })
        .ok_or_else(|| std::io::Error::other("first paragraph has no stop"))?;
    let last_stop = stops
        .iter()
        .filter(|stop| stop["position"]["paragraphIndex"] == 1)
        .max_by_key(|stop| {
            stop["position"]["offsetUtf8"]
                .as_str()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0)
        })
        .ok_or_else(|| std::io::Error::other("second paragraph has no stop"))?;
    let endpoint = |stop: &Value| {
        serde_json::json!({
            "paragraphId": stop["position"]["paragraphId"],
            "offsetUtf8": stop["position"]["offsetUtf8"],
            "affinity": stop["position"]["affinity"],
            "visualAffinity": stop["visualAffinity"],
        })
    };
    let result = runtime.set_selection_json(
        &serde_json::json!({
            "anchor": endpoint(first_stop),
            "focus": endpoint(last_stop),
        })
        .to_string(),
    )?;
    let snapshot: Value = serde_json::from_str(&result)?;
    assert_eq!(runtime.session.selection().anchor.offset, 0);
    assert_eq!(runtime.session.selection().focus.offset, "世界".len());
    assert!(
        snapshot["selectionRects"]
            .as_array()
            .is_some_and(|rects| rects.len() >= 2)
    );
    runtime.delete_backward_json()?;
    assert_eq!(runtime.session.document().body.len(), 1);
    Ok(())
}

#[test]
fn page_click_updates_the_rust_session_selection() -> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::blank();
    if let Some(Block::Paragraph(paragraph)) = document.body.first_mut() {
        paragraph.inlines.push(text_flow(Run {
            text: "a".to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        }));
    } else {
        panic!("blank document should start with a paragraph");
    }
    let mut runtime = DocumentViewRuntime::new(document)?;

    let result = runtime.place_caret_at_page_json(0, 550.0, 73.0)?;
    let hit: Value = serde_json::from_str(&result)?;
    assert_eq!(hit["position"]["offsetUtf8"], "1");
    assert_eq!(runtime.session.selection().anchor.offset, 1);
    assert_eq!(runtime.session.selection().focus.offset, 1);
    assert_eq!(
        runtime.session.selection_visual_affinity(),
        caret_engine::VisualAffinity::Upstream
    );
    assert_eq!(runtime.session.document_generation(), 0);

    assert_eq!(runtime.place_caret_at_page_json(-1, 550.0, 73.0)?, "null");
    assert_eq!(runtime.session.selection().focus.offset, 1);

    runtime.set_selection_json(
        &serde_json::json!({
            "anchor": {
                "paragraphId": runtime.session.selection().focus.paragraph_id.to_string(),
                "offsetUtf8": "0",
                "affinity": "downstream",
                "visualAffinity": "downstream"
            },
            "focus": {
                "paragraphId": runtime.session.selection().focus.paragraph_id.to_string(),
                "offsetUtf8": "1",
                "affinity": "upstream",
                "visualAffinity": "upstream"
            }
        })
        .to_string(),
    )?;
    assert_ne!(
        runtime.session.selection().anchor,
        runtime.session.selection().focus
    );
    runtime.clear_selection_json()?;
    assert_eq!(
        runtime.session.selection().anchor,
        runtime.session.selection().focus
    );
    assert_eq!(runtime.session.selection().focus.offset, 1);
    Ok(())
}

#[test]
fn current_structure_exposes_heading_kind_and_level() -> Result<(), Box<dyn std::error::Error>> {
    let mut document = caret_doc::Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        return Err("blank document must contain a paragraph".into());
    };
    paragraph
        .props
        .insert("outlineLevel".into(), serde_json::json!(2));
    let session = caret_engine::Session::new(document)?;
    let state = super::super::snapshot::current_structure_state(&session);
    assert_eq!(state[0]["kind"], "heading");
    assert_eq!(state[0]["level"], 2);
    assert!(state[0].get("label").is_none());
    Ok(())
}

fn paragraph_with_inline_math(
    before: &str,
    math_tree: caret_doc::MathTree,
    after: &str,
) -> (Paragraph, NodeId) {
    let mut paragraph = paragraph_with_text(before);
    let math_id = NodeId::generate();
    paragraph
        .inlines
        .push(ParagraphContent::Math(caret_doc::InlineMath {
            id: math_id.clone(),
            math_tree,
            extra: BTreeMap::new(),
        }));
    paragraph.inlines.push(text_flow(Run {
        text: after.to_owned(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }));
    (paragraph, math_id)
}

fn document_with_middle_inline_math(
    math_tree: caret_doc::MathTree,
) -> (Document, NodeId, NodeId, NodeId) {
    let above = paragraph_with_text("above");
    let above_id = above.id.clone();
    let (middle, _) = paragraph_with_inline_math("ab", math_tree, "cd");
    let middle_id = middle.id.clone();
    let below = paragraph_with_text("below");
    let below_id = below.id.clone();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(above),
        Block::Paragraph(middle),
        Block::Paragraph(below),
    ];
    (document, above_id, middle_id, below_id)
}

fn inline_fraction_tree() -> caret_doc::MathTree {
    caret_doc::MathTree {
        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
        root: caret_doc::MathFlow {
            id: NodeId::generate(),
            content: vec![caret_doc::MathContent::Math(caret_doc::MathNode {
                id: NodeId::generate(),
                kind: caret_doc::MathNodeKind::Fraction,
                text: None,
                attributes: BTreeMap::from([("style".to_owned(), "frac".to_owned())]),
                children: vec![symbol_flow("a"), symbol_flow("b")],
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }
}

fn enter_inline_math_tree(
    runtime: &mut DocumentViewRuntime,
    paragraph_id: &NodeId,
    offset: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    set_body_caret(runtime, paragraph_id.clone(), offset)?;
    runtime.session.run(caret_engine::Command::EnterMath(
        caret_engine::Direction::Forward,
    ))?;
    assert!(
        runtime.session.math_tree_input_context().is_some(),
        "inline formula should enter MathTree focus"
    );
    Ok(())
}

fn enter_inline_math_text_island(
    runtime: &mut DocumentViewRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    runtime
        .session
        .run(caret_engine::Command::InsertInlineMath(
            r"\frac{1}{\text{ab}}".to_owned(),
        ))?;
    runtime.session.run(caret_engine::Command::EnterMath(
        caret_engine::Direction::Backward,
    ))?;
    for _ in 0..16 {
        if runtime.session.math_text_input_context().is_some() {
            return Ok(());
        }
        runtime.session.run(caret_engine::Command::MoveCaret(
            caret_engine::Direction::Backward,
        ))?;
    }
    Err("inline math text island focus was not reached".into())
}

#[test]
fn inline_math_vertical_navigation_exits_and_moves_to_adjacent_visual_line()
-> Result<(), Box<dyn std::error::Error>> {
    for (line_delta, target_is_above) in [(-1isize, true), (1, false)] {
        let (document, above_id, middle_id, below_id) =
            document_with_middle_inline_math(inline_math_tree());
        let mut runtime = DocumentViewRuntime::new(document)?;
        runtime.layout_snapshot_json()?;
        enter_inline_math_tree(&mut runtime, &middle_id, 2)?;

        runtime.move_vertical_caret_json(line_delta, false)?;

        assert!(
            runtime.session.math_edit().is_none(),
            "Up/Down from InlineMath must leave the formula editor"
        );
        assert!(!runtime.session.has_structure_selection());
        let expected = if target_is_above {
            &above_id
        } else {
            &below_id
        };
        assert_eq!(
            &runtime.session.selection().focus.paragraph_id,
            expected,
            "one key press should reach the adjacent visual line"
        );
        assert_ne!(
            runtime.session.selection().focus.paragraph_id,
            middle_id,
            "focus should leave the formula paragraph"
        );
    }
    Ok(())
}

#[test]
fn inline_math_shift_vertical_navigation_extends_document_selection()
-> Result<(), Box<dyn std::error::Error>> {
    for (line_delta, anchor_offset, anchor_affinity) in [
        (-1isize, 2usize, caret_engine::Affinity::Downstream),
        (1, 5, caret_engine::Affinity::Upstream),
    ] {
        let (document, _above_id, middle_id, _below_id) =
            document_with_middle_inline_math(inline_math_tree());
        let mut runtime = DocumentViewRuntime::new(document)?;
        runtime.layout_snapshot_json()?;
        enter_inline_math_tree(&mut runtime, &middle_id, 2)?;

        runtime.move_vertical_caret_json(line_delta, true)?;

        assert!(runtime.session.math_edit().is_none());
        assert!(!runtime.session.has_structure_selection());
        let selection = runtime.session.selection().clone();
        assert_ne!(
            selection.anchor, selection.focus,
            "Shift+vertical must extend the document selection instead of being swallowed"
        );
        assert_eq!(selection.anchor.paragraph_id, middle_id);
        assert_eq!(selection.anchor.offset, anchor_offset);
        assert_eq!(selection.anchor.affinity, anchor_affinity);
    }
    Ok(())
}

#[test]
fn inline_math_fraction_vertical_navigation_moves_inside_then_exits()
-> Result<(), Box<dyn std::error::Error>> {
    let (document, _above_id, middle_id, below_id) =
        document_with_middle_inline_math(inline_fraction_tree());
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    enter_inline_math_tree(&mut runtime, &middle_id, 2)?;

    // 第一次 Down 仍使用公式内部垂直候选，进入分母。
    runtime.move_vertical_caret_json(1, false)?;
    let (target, position) = runtime
        .session
        .math_tree_input_context()
        .ok_or_else(|| std::io::Error::other("fraction Down should stay in MathTree"))?;
    assert!(matches!(
        target,
        caret_engine::MathEditTarget::Inline { .. }
    ));
    assert!(
        matches!(
            position.path.steps().last(),
            Some(caret_engine::MathSequenceStep::Operand {
                operand_index: 1,
                ..
            })
        ),
        "first Down should focus the denominator operand: {position:?}"
    );

    // 第二次 Down 到达公式底边，退出并继续到下一段落视觉行。
    runtime.move_vertical_caret_json(1, false)?;
    assert!(runtime.session.math_edit().is_none());
    assert_eq!(
        runtime.session.selection().focus.paragraph_id,
        below_id,
        "second Down should leave the fraction and reach the next paragraph"
    );
    Ok(())
}

#[test]
fn inline_math_text_island_vertical_edge_exits_at_the_document_edge()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.layout_snapshot_json()?;
    enter_inline_math_text_island(&mut runtime)?;

    // 文档只有这一行：分母文本岛底边没有更下方的候选，按键仍应退出公式编辑。
    runtime.move_vertical_caret_json(1, false)?;

    assert!(runtime.session.math_edit().is_none());
    assert!(!runtime.session.has_structure_selection());
    assert_eq!(runtime.session.selection().focus.offset, 3);
    assert_eq!(
        runtime.session.selection().focus.affinity,
        caret_engine::Affinity::Upstream
    );
    Ok(())
}

#[test]
fn inline_math_vertical_navigation_at_document_edges_only_exits_the_formula()
-> Result<(), Box<dyn std::error::Error>> {
    for (line_delta, expected_offset, expected_affinity) in [
        (-1isize, 2usize, caret_engine::Affinity::Downstream),
        (1, 5, caret_engine::Affinity::Upstream),
    ] {
        let (middle, _) = paragraph_with_inline_math("ab", inline_math_tree(), "cd");
        let middle_id = middle.id.clone();
        let mut document = Document::blank();
        document.body = vec![Block::Paragraph(middle)];
        let mut runtime = DocumentViewRuntime::new(document)?;
        runtime.layout_snapshot_json()?;
        enter_inline_math_tree(&mut runtime, &middle_id, 2)?;

        runtime.move_vertical_caret_json(line_delta, false)?;

        assert!(runtime.session.math_edit().is_none());
        assert_eq!(runtime.session.selection().focus.offset, expected_offset);
        assert_eq!(
            runtime.session.selection().focus.affinity,
            expected_affinity
        );
        assert!(!runtime.session.has_structure_selection());
    }
    Ok(())
}

#[test]
fn command_query_vertical_navigation_does_not_leave_inline_math()
-> Result<(), Box<dyn std::error::Error>> {
    let (document, _above_id, middle_id, _below_id) =
        document_with_middle_inline_math(inline_math_tree());
    let mut runtime = DocumentViewRuntime::new(document)?;
    runtime.layout_snapshot_json()?;
    enter_inline_math_tree(&mut runtime, &middle_id, 2)?;
    runtime
        .session
        .run(caret_engine::Command::BeginCommandLine)?;
    runtime
        .session
        .run(caret_engine::Command::TypeCommandLine("abc".to_owned()))?;

    runtime.move_vertical_caret_json(-1, false)?;

    assert!(
        runtime.session.command_line().is_some(),
        "command query keeps priority over formula exit"
    );
    assert!(runtime.session.math_edit().is_some());
    Ok(())
}

#[test]
fn ime_composition_vertical_navigation_does_not_leave_inline_math()
-> Result<(), Box<dyn std::error::Error>> {
    let mut runtime = DocumentViewRuntime::new_blank()?;
    runtime.layout_snapshot_json()?;
    enter_inline_math_text_island(&mut runtime)?;
    runtime
        .session
        .run(caret_engine::Command::UpdateComposition {
            id: 902,
            preedit: "中".to_owned(),
        })?;
    let document_before = runtime.session.document().clone();

    runtime.move_vertical_caret_json(-1, false)?;

    assert!(runtime.session.math_edit().is_some());
    assert_eq!(runtime.session.document(), &document_before);
    assert!(runtime.session.composition_preview_with_range(0).is_ok());
    Ok(())
}

#[test]
fn empty_inline_math_vertical_navigation_exits_the_formula()
-> Result<(), Box<dyn std::error::Error>> {
    for (line_delta, expected_offset, expected_affinity) in [
        (-1isize, 0usize, caret_engine::Affinity::Downstream),
        (1, 3, caret_engine::Affinity::Upstream),
    ] {
        let mut runtime = DocumentViewRuntime::new_blank()?;
        runtime
            .session
            .run(caret_engine::Command::InsertInlineMath(String::new()))?;
        runtime.layout_snapshot_json()?;
        runtime.session.run(caret_engine::Command::EnterMath(
            caret_engine::Direction::Backward,
        ))?;
        assert!(runtime.session.math_tree_input_context().is_some());

        runtime.move_vertical_caret_json(line_delta, false)?;

        assert!(runtime.session.math_edit().is_none());
        assert_eq!(runtime.session.selection().focus.offset, expected_offset);
        assert_eq!(
            runtime.session.selection().focus.affinity,
            expected_affinity
        );
    }
    Ok(())
}
