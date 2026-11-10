//! 验证结构索引的文档顺序、嵌套范围和标题派生范围。

use super::*;
use crate::{
    DocumentNodeEdge, DocumentNodeKind, block_list_by_environment_ids,
    block_list_by_environment_ids_mut, compare_document_node_edges, find_block_node,
    find_document_node,
};
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, InlineEnvironment,
    InlineEnvironmentKind, InlineMath, MathFlow, MathTree, NodeId, Paragraph, ParagraphContent,
    Run,
};
use std::collections::BTreeMap;

fn paragraph(text: &str, level: Option<u64>) -> Paragraph {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(text_flow(Run {
        text: text.to_owned(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }));
    if let Some(level) = level {
        paragraph
            .props
            .insert("outlineLevel".to_owned(), serde_json::json!(level));
    }
    paragraph
}

fn empty_inline_math(id: NodeId) -> ParagraphContent {
    ParagraphContent::Math(InlineMath {
        id,
        math_tree: MathTree {
            schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
            root: MathFlow {
                id: NodeId::generate(),
                content: Vec::new(),
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    })
}

#[test]
fn nested_environment_stack_is_stable_id_ordered() {
    let mut document = Document::blank();
    let section_id = NodeId::generate();
    let custom_id = NodeId::generate();
    let text = paragraph("inside", None);
    let text_id = text.id.clone();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: section_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: None,
        children: vec![Block::Environment(BlockEnvironment {
            id: custom_id.clone(),
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children: vec![Block::Paragraph(text)],
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    })];

    let index = StructureIndex::build(&document);
    let stack = index.at(&StructurePosition {
        paragraph_id: text_id.clone(),
        offset_utf8: 3,
    });
    assert!(stack.iter().any(|entry| entry.id == section_id));
    assert!(stack.iter().any(|entry| entry.id == custom_id));
    assert!(index.is_ancestor(&section_id, &custom_id));
    assert!(!index.is_ancestor(&custom_id, &section_id));
    assert_eq!(
        stack.last().and_then(|entry| entry.paragraph_id.as_ref()),
        Some(&text_id)
    );
}

#[test]
fn heading_scope_ends_before_next_same_or_higher_heading() {
    let mut document = Document::blank();
    let first = paragraph("first", Some(1));
    let child = paragraph("child", Some(2));
    let body = paragraph("body", None);
    let second = paragraph("second", Some(1));
    let first_id = first.id.clone();
    let child_id = child.id.clone();
    let body_id = body.id.clone();
    let second_id = second.id.clone();
    document.body = [first, child, body, second]
        .into_iter()
        .map(Block::Paragraph)
        .collect();

    let index = StructureIndex::build(&document);
    let first_scopes = index.structures().iter().find(|entry| {
        entry.id == first_id && matches!(entry.kind, StructureKind::Heading { level: 1 })
    });
    assert!(first_scopes.is_some());
    let Some(first_scope) = first_scopes else {
        return;
    };
    assert!(first_scope.range.paragraph_ordinals.contains(&0));
    assert!(first_scope.range.paragraph_ordinals.contains(&1));
    assert!(first_scope.range.paragraph_ordinals.contains(&2));
    assert!(!first_scope.range.paragraph_ordinals.contains(&3));
    assert_eq!(index.adjacent_paragraph(&first_id, true), Some(&child_id));
    assert_eq!(index.adjacent_paragraph(&body_id, true), Some(&second_id));
}

#[test]
fn structure_stack_orders_heading_and_nested_environment_by_document_tree() {
    let mut document = Document::blank();
    let heading = paragraph("heading", Some(1));
    let heading_id = heading.id.clone();
    let nested_id = NodeId::generate();
    let child = paragraph("child", None);
    let child_id = child.id.clone();
    document.body = vec![
        Block::Paragraph(heading),
        Block::Environment(BlockEnvironment {
            id: nested_id.clone(),
            kind: BlockEnvironmentKind::Custom,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children: vec![Block::Paragraph(child)],
            extra: BTreeMap::new(),
        }),
    ];

    let index = StructureIndex::build(&document);
    let stack = index.at(&StructurePosition {
        paragraph_id: child_id.clone(),
        offset_utf8: 0,
    });
    let ids = stack
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![heading_id, nested_id, child_id]);
}

#[test]
fn environment_title_is_a_navigation_paragraph_owned_by_its_environment() {
    let mut document = Document::blank();
    let environment_id = NodeId::generate();
    let title = paragraph("title", None);
    let title_id = title.id.clone();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(title),
        children: vec![Block::Paragraph(paragraph("body", None))],
        extra: BTreeMap::new(),
    })];

    let index = StructureIndex::build(&document);
    assert_eq!(index.title_environment_id(&title_id), Some(&environment_id));
    assert_eq!(index.paragraph_id_at(0), Some(&title_id));
    let stack = index.at(&StructurePosition {
        paragraph_id: title_id.clone(),
        offset_utf8: 0,
    });
    assert_eq!(stack.len(), 2);
    assert_eq!(stack[0].id, environment_id);
    assert_eq!(stack[1].id, title_id);
}

#[test]
fn physical_locations_keep_real_slots_and_order_titles_before_children() {
    let mut document = Document::blank();
    let environment_id = NodeId::generate();
    let title = paragraph("title", None);
    let title_id = title.id.clone();
    let child = paragraph("child", None);
    let child_id = child.id.clone();
    let after = paragraph("after", None);
    let after_id = after.id.clone();
    let equation_id = NodeId::generate();
    document.body = vec![
        Block::Environment(BlockEnvironment {
            id: environment_id.clone(),
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: BTreeMap::new(),
            title: Some(title),
            children: vec![
                Block::ExplicitBreak(caret_doc::ExplicitBreak::default()),
                Block::Opaque(caret_doc::OpaqueNode {
                    raw: serde_json::json!({"type": "future"}),
                }),
                Block::Equation(caret_doc::Equation {
                    id: equation_id.clone(),
                    math_tree: caret_doc::MathTree {
                        schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                        root: caret_doc::MathFlow {
                            id: NodeId::generate(),
                            content: Vec::new(),
                            extra: BTreeMap::new(),
                        },
                        extra: BTreeMap::new(),
                    },
                    extra: BTreeMap::new(),
                }),
                Block::Paragraph(child),
            ],
            extra: BTreeMap::new(),
        }),
        Block::Paragraph(after),
    ];

    let Some(title_location) = find_document_node(&document, &title_id) else {
        panic!("title is indexed");
    };
    let Some(equation_location) = find_document_node(&document, &equation_id) else {
        panic!("equation is indexed");
    };
    let Some(child_location) = find_document_node(&document, &child_id) else {
        panic!("paragraph is indexed");
    };

    assert_eq!(title_location.kind, DocumentNodeKind::EnvironmentTitle);
    assert_eq!(
        title_location.parent_environment_ids,
        vec![environment_id.clone()]
    );
    assert_eq!(title_location.sibling_index, None);
    assert_eq!(
        equation_location.parent_environment_ids,
        vec![environment_id.clone()]
    );
    assert_eq!(equation_location.sibling_index, Some(2));
    assert_eq!(child_location.sibling_index, Some(3));
    assert_eq!(
        compare_document_node_edges(
            &document,
            &title_id,
            DocumentNodeEdge::Before,
            &equation_id,
            DocumentNodeEdge::Before
        ),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &equation_id,
            DocumentNodeEdge::Before,
            &child_id,
            DocumentNodeEdge::Before
        ),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &child_id,
            DocumentNodeEdge::Before,
            &after_id,
            DocumentNodeEdge::Before
        ),
        Some(std::cmp::Ordering::Less)
    );
}

#[test]
fn physical_node_edges_order_nested_content_and_share_adjacent_gaps() {
    let mut document = Document::blank();
    let environment_id = NodeId::generate();
    let inline_environment_id = NodeId::generate();
    let inline_math_id = NodeId::generate();
    let mut title = paragraph("title", None);
    title.inlines = vec![ParagraphContent::Environment(InlineEnvironment {
        id: inline_environment_id.clone(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::new(),
        children: vec![ParagraphContent::Math(InlineMath {
            id: inline_math_id.clone(),
            math_tree: MathTree {
                schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                root: MathFlow {
                    id: NodeId::generate(),
                    content: Vec::new(),
                    extra: BTreeMap::new(),
                },
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    })];
    let title_id = title.id.clone();
    let child = paragraph("child", None);
    let child_id = child.id.clone();
    let next = paragraph("next", None);
    let next_id = next.id.clone();
    document.body = vec![
        Block::Environment(BlockEnvironment {
            id: environment_id.clone(),
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: BTreeMap::new(),
            title: Some(title),
            children: vec![Block::Paragraph(child)],
            extra: BTreeMap::new(),
        }),
        Block::Paragraph(next),
    ];

    assert_eq!(
        compare_document_node_edges(
            &document,
            &environment_id,
            DocumentNodeEdge::Before,
            &title_id,
            DocumentNodeEdge::Before,
        ),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &title_id,
            DocumentNodeEdge::After,
            &child_id,
            DocumentNodeEdge::Before,
        ),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &title_id,
            DocumentNodeEdge::Before,
            &inline_environment_id,
            DocumentNodeEdge::Before,
        ),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &inline_environment_id,
            DocumentNodeEdge::After,
            &inline_math_id,
            DocumentNodeEdge::Before,
        ),
        Some(std::cmp::Ordering::Greater)
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &inline_math_id,
            DocumentNodeEdge::After,
            &title_id,
            DocumentNodeEdge::After,
        ),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &child_id,
            DocumentNodeEdge::After,
            &environment_id,
            DocumentNodeEdge::After,
        ),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &environment_id,
            DocumentNodeEdge::After,
            &next_id,
            DocumentNodeEdge::Before,
        ),
        Some(std::cmp::Ordering::Equal)
    );
}

#[test]
fn inline_node_edges_preserve_gaps_around_unidentified_content() {
    let first_id = NodeId::generate();
    let after_opaque_id = NodeId::generate();
    let adjacent_id = NodeId::generate();
    let paragraph = Paragraph {
        id: NodeId::generate(),
        inlines: vec![
            empty_inline_math(first_id.clone()),
            ParagraphContent::Opaque(caret_doc::OpaqueNode {
                raw: serde_json::json!({"type": "futureInline"}),
            }),
            empty_inline_math(after_opaque_id.clone()),
            empty_inline_math(adjacent_id.clone()),
        ],
        ..Paragraph::empty(NodeId::generate())
    };
    let document = Document {
        body: vec![Block::Paragraph(paragraph)],
        ..Document::blank()
    };

    assert_eq!(
        compare_document_node_edges(
            &document,
            &first_id,
            DocumentNodeEdge::After,
            &after_opaque_id,
            DocumentNodeEdge::Before,
        ),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &after_opaque_id,
            DocumentNodeEdge::After,
            &adjacent_id,
            DocumentNodeEdge::Before,
        ),
        Some(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &adjacent_id,
            DocumentNodeEdge::After,
            &after_opaque_id,
            DocumentNodeEdge::Before,
        ),
        Some(std::cmp::Ordering::Greater)
    );
}

#[test]
fn semantic_stack_can_be_resolved_from_a_block_without_paragraph_anchor() {
    let mut document = Document::blank();
    let section_id = NodeId::generate();
    let equation_id = NodeId::generate();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: section_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: None,
        children: vec![Block::Equation(caret_doc::Equation {
            id: equation_id.clone(),
            math_tree: caret_doc::MathTree {
                schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                root: caret_doc::MathFlow {
                    id: NodeId::generate(),
                    content: Vec::new(),
                    extra: BTreeMap::new(),
                },
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    })];

    let index = StructureIndex::build(&document);
    let ids = index
        .stack_for_id(&equation_id)
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![section_id, equation_id]);
}

#[test]
fn inline_math_and_format_environments_share_the_paragraph_order() {
    let paragraph_id = NodeId::generate();
    let inline_environment_id = NodeId::generate();
    let math_id = NodeId::generate();
    let inline_paragraph = Paragraph {
        id: paragraph_id.clone(),
        inlines: vec![ParagraphContent::Environment(InlineEnvironment {
            id: inline_environment_id.clone(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: BTreeMap::new(),
            children: vec![ParagraphContent::Math(InlineMath {
                id: math_id.clone(),
                math_tree: MathTree {
                    schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                    root: MathFlow {
                        id: NodeId::generate(),
                        content: Vec::new(),
                        extra: BTreeMap::new(),
                    },
                    extra: BTreeMap::new(),
                },
                extra: BTreeMap::new(),
            })],
            extra: BTreeMap::new(),
        })],
        ..Paragraph::empty(paragraph_id.clone())
    };
    let after = paragraph("after", None);
    let after_id = after.id.clone();
    let document = Document {
        body: vec![Block::Paragraph(inline_paragraph), Block::Paragraph(after)],
        ..Document::blank()
    };

    let Some(environment_location) = find_document_node(&document, &inline_environment_id) else {
        panic!("inline environment is indexed");
    };
    let Some(math_location) = find_document_node(&document, &math_id) else {
        panic!("inline math is indexed");
    };
    assert_eq!(
        environment_location.kind,
        DocumentNodeKind::InlineEnvironment
    );
    assert_eq!(math_location.kind, DocumentNodeKind::InlineMath);
    assert_eq!(math_location.paragraph_id, Some(paragraph_id.clone()));
    assert!(!math_location.is_title);
    assert_eq!(
        math_location.parent_inline_environment_ids,
        vec![inline_environment_id.clone()]
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &paragraph_id,
            DocumentNodeEdge::Before,
            &inline_environment_id,
            DocumentNodeEdge::Before
        ),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &inline_environment_id,
            DocumentNodeEdge::Before,
            &math_id,
            DocumentNodeEdge::Before
        ),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        compare_document_node_edges(
            &document,
            &math_id,
            DocumentNodeEdge::Before,
            &after_id,
            DocumentNodeEdge::Before
        ),
        Some(std::cmp::Ordering::Less)
    );
}

#[test]
fn equation_and_align_blocks_are_indexed_without_navigation_stops() {
    let mut document = Document::blank();
    let before = paragraph("before", None);
    let after = paragraph("after", None);
    let blocks: Vec<Block> = serde_json::from_value(serde_json::json!([
        {"type": "equation", "id": "eq", "mathTree": {
            "schemaVersion": caret_doc::MATH_TREE_SCHEMA_VERSION,
            "root": {"id": "eq-root", "content": [
                {"type": "math", "id": "eq-x", "kind": "symbol", "text": "x"}
            ]}
        }},
        {"type": "align", "id": "al", "rows": []}
    ]))
    .unwrap_or_else(|error| panic!("math blocks should decode: {error}"));
    document.body = vec![Block::Paragraph(before.clone())];
    document.body.extend(blocks);
    document.body.push(Block::Paragraph(after.clone()));
    document
        .validate()
        .unwrap_or_else(|error| panic!("fixture should satisfy document invariants: {error}"));
    let index = StructureIndex::build(&document);

    let equation = index
        .get(&NodeId::parse("eq").unwrap_or_else(|error| panic!("id: {error}")))
        .unwrap_or_else(|| panic!("equation should be indexed"));
    assert_eq!(equation.kind, StructureKind::Equation);
    assert_eq!(equation.kind.class(), StructureClass::SingleLine);
    assert!(equation.paragraph_id.is_none());
    assert_eq!(equation.range.paragraph_ordinals, 1..1);
    let align = index
        .get(&NodeId::parse("al").unwrap_or_else(|error| panic!("id: {error}")))
        .unwrap_or_else(|| panic!("align should be indexed"));
    assert_eq!(align.kind, StructureKind::Align);
    assert!(align.kind.is_math_block());
    assert!(!align.kind.is_block_environment());
    assert_eq!(index.navigation_paragraphs(), &[before.id, after.id]);
}

#[test]
fn environment_parent_paths_share_read_and_write_block_list_resolution() {
    let outer_id = NodeId::generate();
    let inner_id = NodeId::generate();
    let paragraph_id = NodeId::generate();
    let blocks = vec![Block::Environment(BlockEnvironment {
        id: outer_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: None,
        children: vec![Block::Environment(BlockEnvironment {
            id: inner_id.clone(),
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children: vec![Block::Paragraph(Paragraph::empty(paragraph_id.clone()))],
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    })];
    assert_eq!(
        block_list_by_environment_ids(&blocks, &[]),
        Some(blocks.as_slice())
    );
    let nested = block_list_by_environment_ids(&blocks, &[outer_id.clone(), inner_id.clone()])
        .unwrap_or_else(|| panic!("nested environment path should resolve"));
    assert!(matches!(nested, [Block::Paragraph(paragraph)] if paragraph.id == paragraph_id));

    let mut edited = blocks.clone();
    let nested =
        block_list_by_environment_ids_mut(&mut edited, &[outer_id.clone(), inner_id.clone()])
            .unwrap_or_else(|| panic!("mutable nested environment path should resolve"));
    nested.push(Block::Paragraph(Paragraph::empty(NodeId::generate())));
    assert_eq!(
        block_list_by_environment_ids(&edited, &[outer_id, inner_id]).map(<[Block]>::len),
        Some(2)
    );
    assert_eq!(blocks.len(), 1);
}

#[test]
fn environment_parent_paths_reject_wrong_missing_and_ambiguous_ids() {
    let environment = BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: None,
        children: Vec::new(),
        extra: BTreeMap::new(),
    };
    let paragraph = Paragraph::empty(NodeId::generate());
    let wrong_kind_id = paragraph.id.clone();
    let environment_id = environment.id.clone();
    let missing_id = NodeId::generate();
    let blocks = vec![
        Block::Environment(environment.clone()),
        Block::Paragraph(paragraph),
    ];
    assert!(block_list_by_environment_ids(&blocks, &[wrong_kind_id]).is_none());
    assert!(block_list_by_environment_ids(&blocks, &[missing_id]).is_none());
    assert!(
        block_list_by_environment_ids(&blocks, &[environment_id.clone(), environment_id.clone()])
            .is_none()
    );

    let duplicate_blocks = vec![
        Block::Environment(environment.clone()),
        Block::Environment(environment),
    ];
    assert!(
        block_list_by_environment_ids(&duplicate_blocks, std::slice::from_ref(&environment_id))
            .is_none()
    );
    let mut duplicate_blocks = duplicate_blocks;
    assert!(block_list_by_environment_ids_mut(&mut duplicate_blocks, &[environment_id]).is_none());
    assert_eq!(duplicate_blocks.len(), 2);
}

#[test]
fn environment_parent_location_uses_physical_parent_path_and_real_slot() {
    let outer_id = NodeId::generate();
    let target_id = NodeId::generate();
    let blocks = vec![Block::Environment(BlockEnvironment {
        id: outer_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: None,
        children: vec![
            Block::Paragraph(Paragraph::empty(NodeId::generate())),
            Block::Environment(BlockEnvironment {
                id: target_id.clone(),
                kind: BlockEnvironmentKind::Section,
                definition_id: None,
                props: BTreeMap::new(),
                title: None,
                children: Vec::new(),
                extra: BTreeMap::new(),
            }),
        ],
        extra: BTreeMap::new(),
    })];
    let location = find_block_node(&blocks, &target_id)
        .unwrap_or_else(|| panic!("target environment should be indexed"));
    assert_eq!(location.kind, DocumentNodeKind::Environment);
    assert_eq!(location.parent_environment_ids, vec![outer_id]);
    assert_eq!(location.sibling_index, Some(1));
}

fn text_flow(run: Run) -> caret_doc::ParagraphContent {
    caret_doc::ParagraphContent::Text(caret_doc::TextFlow {
        runs: vec![run],
        extra: Default::default(),
    })
}
