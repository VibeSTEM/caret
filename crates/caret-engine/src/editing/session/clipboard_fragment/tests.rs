//! 覆盖剪贴板片段复制、粘贴事务与外部纯文本数学识别。

use super::*;
use crate::editing::text_map::TextMap;
use crate::editing::{
    Affinity, Command, Direction, DocumentRangeEndpoint, EditError, MathEditBranch, Pos, Selection,
    StoryId, StructureSelection,
};
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, InlineEnvironment,
    InlineEnvironmentKind, InlineMath, NodeId, Paragraph, ParagraphContent, Run, Table, TableCell,
    TableRow, TextFlow,
};
use caret_math::{MathInputKind, ParsedMath};

fn text_flow(text: &str) -> ParagraphContent {
    ParagraphContent::Text(TextFlow {
        runs: vec![Run {
            text: text.to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        }],
        extra: Default::default(),
    })
}

fn paragraph_with(id: NodeId, text: &str) -> Block {
    let mut paragraph = Paragraph::empty(id);
    paragraph.inlines.push(text_flow(text));
    Block::Paragraph(paragraph)
}

fn session_with_body(body: Vec<Block>) -> Session {
    let mut document = Document::blank();
    document.body = body;
    Session::new(document).unwrap_or_else(|error| panic!("invalid test document: {error}"))
}

fn pos(paragraph_id: &NodeId, offset: usize) -> Pos {
    Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset,
        affinity: Affinity::Downstream,
    }
}

fn select(session: &mut Session, anchor: Pos, focus: Pos) {
    session
        .run(Command::SetSelection(Some(Selection { anchor, focus })))
        .unwrap_or_else(|error| panic!("selection should be accepted: {error}"));
}

fn paragraph_text_of(paragraph: &Paragraph) -> String {
    match TextMap::new(paragraph) {
        Ok(map) => map.text,
        Err(error) => panic!("failed to map paragraph: {error}"),
    }
}

fn body_paragraph_texts(session: &Session) -> Vec<String> {
    let mut texts = Vec::new();
    collect_paragraph_texts(&session.document.body, &mut texts);
    texts
}

fn collect_paragraph_texts(blocks: &[Block], texts: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => texts.push(paragraph_text_of(paragraph)),
            Block::Table(_) => {}
            Block::Environment(environment) => {
                if let Some(title) = &environment.title {
                    texts.push(paragraph_text_of(title));
                }
                collect_paragraph_texts(&environment.children, texts);
            }
            Block::Equation(_) | Block::Align(_) | Block::ExplicitBreak(_) | Block::Opaque(_) => {}
        }
    }
}

fn parse_tree(source: &str, kind: MathInputKind) -> caret_doc::MathTree {
    match caret_math::parse(source, kind) {
        Ok(ParsedMath::Tree(tree)) => tree,
        Ok(ParsedMath::Align(_)) => panic!("expected tree for {source}"),
        Err(error) => panic!("test source should parse: {error}"),
    }
}

fn parse_align(source: &str) -> Align {
    match caret_math::parse(source, MathInputKind::Align) {
        Ok(ParsedMath::Align(align)) => align,
        Ok(ParsedMath::Tree(_)) => panic!("expected align for {source}"),
        Err(error) => panic!("test align should parse: {error}"),
    }
}

fn inline_math(source: &str) -> ParagraphContent {
    ParagraphContent::Math(InlineMath {
        id: NodeId::generate(),
        math_tree: parse_tree(source, MathInputKind::Inline),
        extra: Default::default(),
    })
}

fn table_cell(blocks: Vec<Block>) -> TableCell {
    TableCell {
        id: NodeId::generate(),
        blocks,
    }
}

fn select_block(session: &mut Session, block_id: &NodeId) {
    session
        .install_structure_selection(StructureSelection {
            anchor: DocumentRangeEndpoint::BlockEdge {
                block_id: block_id.clone(),
                side: Direction::Backward,
            },
            focus: DocumentRangeEndpoint::BlockEdge {
                block_id: block_id.clone(),
                side: Direction::Forward,
            },
        })
        .unwrap_or_else(|error| panic!("block selection should succeed: {error}"));
}

fn table_block(rows: Vec<Vec<Vec<Block>>>) -> (Block, NodeId, Vec<NodeId>, Vec<NodeId>) {
    let mut row_ids = Vec::with_capacity(rows.len());
    let mut cell_ids = Vec::new();
    let rows = rows
        .into_iter()
        .map(|cells| {
            let cells = cells
                .into_iter()
                .map(|blocks| {
                    let cell = table_cell(blocks);
                    cell_ids.push(cell.id.clone());
                    cell
                })
                .collect();
            let row = TableRow {
                id: NodeId::generate(),
                cells,
            };
            row_ids.push(row.id.clone());
            row
        })
        .collect();
    let table = Table {
        id: NodeId::generate(),
        rows,
    };
    let table_id = table.id.clone();
    (Block::Table(table), table_id, row_ids, cell_ids)
}

fn cell_position(table: &Table, paragraph: &Paragraph, offset: usize) -> Pos {
    let row = table
        .rows
        .iter()
        .find(|row| {
            row.cells.iter().any(|cell| {
                cell.blocks
                    .iter()
                    .any(|block| matches!(block, Block::Paragraph(p) if p.id == paragraph.id))
            })
        })
        .unwrap_or_else(|| panic!("paragraph should be inside a table cell"));
    let cell = row
        .cells
        .iter()
        .find(|cell| {
            cell.blocks
                .iter()
                .any(|block| matches!(block, Block::Paragraph(p) if p.id == paragraph.id))
        })
        .unwrap_or_else(|| panic!("paragraph should be inside a table cell"));
    Pos {
        story: StoryId::TableCell {
            table_id: table.id.clone(),
            row_id: row.id.clone(),
            cell_id: cell.id.clone(),
        },
        paragraph_id: paragraph.id.clone(),
        offset,
        affinity: Affinity::Downstream,
    }
}

fn select_between_blocks(session: &mut Session, first: &NodeId, last: &NodeId) {
    session
        .install_structure_selection(StructureSelection {
            anchor: DocumentRangeEndpoint::BlockEdge {
                block_id: first.clone(),
                side: Direction::Backward,
            },
            focus: DocumentRangeEndpoint::BlockEdge {
                block_id: last.clone(),
                side: Direction::Forward,
            },
        })
        .unwrap_or_else(|error| panic!("block range should succeed: {error}"));
}

#[test]
fn table_blocks_copy_to_tsv_and_paste_with_fresh_ids_and_one_undo() {
    let mut inline = Paragraph::empty(NodeId::generate());
    let inline_math = match inline_math("x") {
        ParagraphContent::Math(math) => math,
        _ => unreachable!(),
    };
    let inline_math_id = inline_math.id.clone();
    inline.inlines = vec![text_flow("one"), ParagraphContent::Math(inline_math)];
    let equation = equation_block("x");
    let equation_id = match &equation {
        Block::Equation(equation) => equation.id.clone(),
        _ => unreachable!(),
    };
    let first_para_id = inline.id.clone();
    let second = paragraph_with(NodeId::generate(), "multi1");
    let third = paragraph_with(NodeId::generate(), "multi2");
    let second_id = match &second {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let third_id = match &third {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => unreachable!(),
    };
    let align = Block::Align(parse_align(r"\begin{aligned}a&=b\end{aligned}"));
    let (table_block, table_id, row_ids, cell_ids) = table_block(vec![
        vec![vec![Block::Paragraph(inline)], vec![equation]],
        vec![vec![second, third], vec![align]],
    ]);
    let mut source = session_with_body(vec![table_block]);
    select_block(&mut source, &table_id);

    let copy = source
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("table copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("selected table should produce a fragment"));
    assert_eq!(copy.fragment.shape, ClipboardFragmentShape::Blocks);
    assert!(matches!(copy.fragment.blocks.as_slice(), [Block::Table(_)]));
    assert_eq!(
        copy.plain_text,
        "one\\(x\\)\t\\[x\\]\nmulti1\nmulti2\t\\[\\begin{aligned}a & =b\\end{aligned}\\]"
    );
    assert_eq!(
        source
            .selected_text()
            .unwrap_or_else(|error| panic!("selection projection should succeed: {error}")),
        "one\u{fffc}\t\u{fffc}\nmulti1\nmulti2\t\u{fffc}"
    );
    assert!(!copy.plain_text.contains('\u{fffc}'));

    let target_id = NodeId::generate();
    let mut target = session_with_body(vec![paragraph_with(target_id.clone(), "beforeafter")]);
    select(&mut target, pos(&target_id, 6), pos(&target_id, 6));
    let before = target.document().clone();
    target
        .paste_clipboard_fragment(copy.fragment)
        .unwrap_or_else(|error| panic!("table block paste should succeed: {error}"));
    let pasted = target.document().clone();
    assert!(matches!(
        target.document().body.as_slice(),
        [Block::Paragraph(_), Block::Table(_), Block::Paragraph(_)]
    ));
    let pasted_table = match &target.document().body[1] {
        Block::Table(table) => table,
        _ => unreachable!(),
    };
    for id in [
        table_id,
        row_ids[0].clone(),
        row_ids[1].clone(),
        cell_ids[0].clone(),
        cell_ids[1].clone(),
        cell_ids[2].clone(),
        cell_ids[3].clone(),
        first_para_id,
        second_id,
        third_id,
        inline_math_id,
        equation_id,
    ] {
        assert_ne!(pasted_table.id, id);
        assert!(!caret_doc::clipboard_fragment_contains_id(
            &target.document().body,
            &id
        ));
    }
    assert_eq!(
        body_paragraph_texts(&target),
        vec!["before".to_owned(), "after".to_owned()]
    );
    target
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("table paste should undo once: {error}"));
    assert_eq!(target.document(), &before);
    assert!(!target.can_undo());
    target
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("table paste should redo: {error}"));
    assert_eq!(target.document(), &pasted);
}

#[test]
fn same_cell_range_copies_paragraph_cuts_and_display_formula_then_pastes_in_cell() {
    let first = match paragraph_with(NodeId::generate(), "alpha") {
        Block::Paragraph(paragraph) => paragraph,
        _ => unreachable!(),
    };
    let last = match paragraph_with(NodeId::generate(), "omega") {
        Block::Paragraph(paragraph) => paragraph,
        _ => unreachable!(),
    };
    let equation = equation_block("x");
    let equation_id = match &equation {
        Block::Equation(equation) => equation.id.clone(),
        _ => unreachable!(),
    };
    let (source_table_block, _source_table_id, _, _) = table_block(vec![vec![vec![
        Block::Paragraph(first.clone()),
        equation,
        Block::Paragraph(last.clone()),
    ]]]);
    let source_table = match &source_table_block {
        Block::Table(table) => table.clone(),
        _ => unreachable!(),
    };
    let mut source = session_with_body(vec![source_table_block]);
    select(
        &mut source,
        cell_position(&source_table, &first, 2),
        cell_position(&source_table, &last, 3),
    );
    let copy = source
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("same-cell range copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("selected cell range should produce a fragment"));
    assert_eq!(copy.fragment.shape, ClipboardFragmentShape::Blocks);
    assert!(copy.fragment.start_clipped && copy.fragment.end_clipped);
    assert_eq!(copy.plain_text, "pha\n\\[x\\]\nome");
    assert_eq!(
        source
            .selected_text()
            .unwrap_or_else(|error| panic!("same-cell text projection should succeed: {error}")),
        "pha\n\u{fffc}\nome"
    );
    let Block::Table(source_table) = &source.document().body[0] else {
        unreachable!();
    };
    let source_cell = &source_table.rows[0].cells[0];

    let target_first = match paragraph_with(NodeId::generate(), "XY") {
        Block::Paragraph(paragraph) => paragraph,
        _ => unreachable!(),
    };
    let (target_table_block, _, _, _) =
        table_block(vec![vec![vec![Block::Paragraph(target_first.clone())]]]);
    let target_table = match &target_table_block {
        Block::Table(table) => table.clone(),
        _ => unreachable!(),
    };
    let mut target = session_with_body(vec![target_table_block]);
    let target_position = cell_position(&target_table, &target_first, 1);
    select(&mut target, target_position.clone(), target_position);
    let before = target.document().clone();
    target
        .paste_clipboard_fragment(copy.fragment)
        .unwrap_or_else(|error| panic!("ordinary block fragment should paste in a cell: {error}"));
    let Block::Table(pasted_table) = &target.document().body[0] else {
        unreachable!();
    };
    let pasted_cell = &pasted_table.rows[0].cells[0];
    assert_eq!(pasted_cell.blocks.len(), 3);
    assert!(
        matches!(&pasted_cell.blocks[0], Block::Paragraph(paragraph) if paragraph_text_of(paragraph) == "Xpha")
    );
    assert!(
        matches!(&pasted_cell.blocks[1], Block::Equation(equation) if equation.id != equation_id)
    );
    assert!(
        matches!(&pasted_cell.blocks[2], Block::Paragraph(paragraph) if paragraph_text_of(paragraph) == "omeY")
    );
    assert_eq!(source_cell.blocks.len(), 3);
    let pasted = target.document().clone();
    target
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("cell paste should undo once: {error}"));
    assert_eq!(target.document(), &before);
    target
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("cell paste should redo: {error}"));
    assert_eq!(target.document(), &pasted);
}

#[test]
fn copying_only_a_cell_paragraph_break_pastes_one_shared_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    for (left, right) in [("aa", "bb"), ("", "bb"), ("aa", ""), ("", "")] {
        for destination_text in ["xy", ""] {
            let Block::Paragraph(first) = paragraph_with(NodeId::generate(), left) else {
                unreachable!();
            };
            let Block::Paragraph(second) = paragraph_with(NodeId::generate(), right) else {
                unreachable!();
            };
            let Block::Paragraph(target) = paragraph_with(NodeId::generate(), destination_text)
            else {
                unreachable!();
            };
            let (block, ..) = table_block(vec![vec![vec![
                Block::Paragraph(first.clone()),
                Block::Paragraph(second.clone()),
                Block::Paragraph(target.clone()),
                paragraph_with(NodeId::generate(), "cc"),
            ]]]);
            let Block::Table(table) = &block else {
                unreachable!();
            };
            let first_end = cell_position(table, &first, left.len());
            let second_start = cell_position(table, &second, 0);
            let target_start = cell_position(table, &target, 0);
            let mut session = session_with_body(vec![block]);
            select(&mut session, first_end, second_start);
            assert_eq!(session.selected_text()?, "\n");
            let copied = session
                .copy_clipboard_fragment()?
                .ok_or(EditError::InvalidPosition)?;
            assert_eq!(copied.plain_text, "\n");
            assert_eq!(copied.fragment.shape, ClipboardFragmentShape::Blocks);
            assert!(copied.fragment.start_clipped && copied.fragment.end_clipped);
            assert_eq!(copied.fragment.blocks.len(), 2);
            let encoded = caret_doc::encode_clipboard_fragment_json(&copied.fragment)?;
            let fragment = caret_doc::decode_clipboard_fragment_json(&encoded)?;

            for offset in 0..=destination_text.len() {
                let destination = Pos {
                    offset,
                    ..target_start.clone()
                };
                select(&mut session, destination.clone(), destination);
                let before = session.document().clone();
                session.paste_clipboard_fragment(fragment.clone())?;
                let Block::Table(table) = &session.document().body[0] else {
                    return Err(EditError::InvalidPosition.into());
                };
                let text = table.rows[0].cells[0]
                    .blocks
                    .iter()
                    .map(|block| match block {
                        Block::Paragraph(paragraph) => Ok(paragraph_text_of(paragraph)),
                        _ => Err(EditError::InvalidPosition),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let (prefix, suffix) = destination_text.split_at(offset);
                assert_eq!(text, [left, right, prefix, suffix, "cc"]);
                session.run(Command::Undo)?;
                assert_eq!(session.document(), &before);
            }
        }
    }
    Ok(())
}

#[test]
fn cross_table_structure_range_copies_and_pastes_tables_in_document_order() {
    let (first_table, first_id, _, _) = table_block(vec![vec![vec![paragraph_with(
        NodeId::generate(),
        "first",
    )]]]);
    let (second_table, second_id, _, _) = table_block(vec![vec![vec![paragraph_with(
        NodeId::generate(),
        "second",
    )]]]);
    let middle_id = NodeId::generate();
    let mut source = session_with_body(vec![
        first_table,
        paragraph_with(middle_id.clone(), "middle"),
        second_table,
    ]);
    select_between_blocks(&mut source, &first_id, &second_id);
    let copy = source
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("cross-table copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("cross-table range should produce a fragment"));
    assert_eq!(copy.fragment.shape, ClipboardFragmentShape::Blocks);
    assert!(matches!(
        copy.fragment.blocks.as_slice(),
        [Block::Table(_), Block::Paragraph(_), Block::Table(_)]
    ));

    let target_id = NodeId::generate();
    let mut target = session_with_body(vec![paragraph_with(target_id.clone(), "delete")]);
    select(&mut target, pos(&target_id, 0), pos(&target_id, 6));
    target
        .paste_clipboard_fragment(copy.fragment)
        .unwrap_or_else(|error| panic!("cross-table blocks should paste: {error}"));
    assert!(matches!(
        target.document().body.as_slice(),
        [Block::Table(_), Block::Paragraph(_), Block::Table(_)]
    ));
    let Block::Table(first_pasted) = &target.document().body[0] else {
        unreachable!();
    };
    let Block::Table(second_pasted) = &target.document().body[2] else {
        unreachable!();
    };
    assert_ne!(first_pasted.id, first_id);
    assert_ne!(second_pasted.id, second_id);
    assert_eq!(body_paragraph_texts(&target), vec!["middle".to_owned()]);
    let table_text = |table: &Table| match &table.rows[0].cells[0].blocks[0] {
        Block::Paragraph(paragraph) => paragraph_text_of(paragraph),
        _ => panic!("copied cell should preserve its paragraph block"),
    };
    assert_eq!(table_text(first_pasted), "first");
    assert_eq!(table_text(second_pasted), "second");
}

#[test]
fn table_blocks_cannot_be_pasted_into_an_existing_cell_and_failure_is_atomic() {
    let (source_table, source_table_id, _, _) = table_block(vec![vec![vec![paragraph_with(
        NodeId::generate(),
        "copied",
    )]]]);
    let mut source = session_with_body(vec![source_table]);
    select_block(&mut source, &source_table_id);
    let copy = source
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("table copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("selected table should produce a fragment"));

    let paragraph = match paragraph_with(NodeId::generate(), "keep") {
        Block::Paragraph(paragraph) => paragraph,
        _ => unreachable!(),
    };
    let (target_table_block, _, _, _) =
        table_block(vec![vec![vec![Block::Paragraph(paragraph.clone())]]]);
    let target_table = match &target_table_block {
        Block::Table(table) => table.clone(),
        _ => unreachable!(),
    };
    let mut target = session_with_body(vec![target_table_block]);
    let position = cell_position(&target_table, &paragraph, 2);
    select(&mut target, position.clone(), position);
    let document_before = target.document().clone();
    let selection_before = target.selection().clone();
    let generation_before = target.document_generation();
    assert!(!target.can_undo());
    assert_eq!(
        target.paste_clipboard_fragment(copy.fragment),
        Err(EditError::ClipboardHostMismatch)
    );
    assert_eq!(target.document(), &document_before);
    assert_eq!(target.selection(), &selection_before);
    assert_eq!(target.document_generation(), generation_before);
    assert!(!target.can_undo());
}

#[test]
fn copy_inline_selection_returns_inline_fragment_without_fffc() {
    let id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(id.clone(), "hello world")]);
    select(&mut session, pos(&id, 0), pos(&id, 5));

    let copy = session
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("inline copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("non-empty selection should produce a copy"));
    assert_eq!(copy.fragment.shape, ClipboardFragmentShape::Inline);
    assert!(!copy.fragment.start_clipped);
    assert!(copy.fragment.end_clipped);
    assert_eq!(copy.fragment.blocks.len(), 1);
    let Block::Paragraph(paragraph) = &copy.fragment.blocks[0] else {
        panic!("inline fragment should contain one paragraph");
    };
    assert_eq!(paragraph_text_of(paragraph), "hello");
    assert_eq!(copy.plain_text, "hello");
    assert!(!copy.plain_text.contains('\u{FFFC}'));
}

#[test]
fn copy_empty_selection_returns_none() {
    let id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(id.clone(), "abc")]);
    select(&mut session, pos(&id, 1), pos(&id, 1));
    assert_eq!(session.copy_clipboard_fragment(), Ok(None));
}

#[test]
fn copy_blocks_preserve_text_format_math_and_environment_content() {
    let bold = ParagraphContent::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: Default::default(),
        children: vec![text_flow("bold")],
        extra: Default::default(),
    });
    let math_paragraph = Block::Paragraph({
        let mut paragraph = Paragraph::empty(NodeId::generate());
        paragraph.inlines = vec![bold, text_flow(" "), inline_math("x")];
        paragraph
    });
    let equation = Block::Equation(caret_doc::Equation {
        id: NodeId::generate(),
        math_tree: parse_tree("x^2", MathInputKind::Equation),
        extra: Default::default(),
    });
    let align = Block::Align(parse_align(r"\begin{aligned}a&=b\end{aligned}"));
    let environment = Block::Environment(BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Custom,
        definition_id: None,
        props: Default::default(),
        title: None,
        children: vec![paragraph_with(NodeId::generate(), "env child")],
        extra: Default::default(),
    });
    let first = NodeId::generate();
    let Block::Paragraph(math_paragraph_inner) = &math_paragraph else {
        unreachable!();
    };
    let second = math_paragraph_inner.id.clone();
    let env_child_id = match &environment {
        Block::Environment(environment) => match &environment.children[0] {
            Block::Paragraph(paragraph) => paragraph.id.clone(),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    let mut session = session_with_body(vec![
        paragraph_with(first.clone(), "alpha "),
        math_paragraph,
        equation,
        align,
        environment,
    ]);
    select(
        &mut session,
        pos(&first, 0),
        pos(&env_child_id, "env child".len()),
    );

    let copy = session
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("block copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("selection should produce a copy"));
    assert_eq!(copy.fragment.shape, ClipboardFragmentShape::Blocks);
    assert!(copy.fragment.blocks.iter().any(|block| matches!(
        block,
        Block::Paragraph(paragraph)
            if paragraph.inlines.iter().any(|content| matches!(content, ParagraphContent::Environment(environment) if environment.kind == InlineEnvironmentKind::Bold))
    )));
    assert!(
        copy.fragment
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Equation(_)))
    );
    assert!(
        copy.fragment
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Align(_)))
    );
    assert!(!copy.plain_text.contains('\u{FFFC}'));
    assert!(copy.plain_text.contains(r"\(x\)"), "{}", copy.plain_text);
    assert!(copy.plain_text.contains(r"\["), "{}", copy.plain_text);
    assert!(
        copy.plain_text.contains(r"\begin{aligned}"),
        "{}",
        copy.plain_text
    );
    assert!(copy.plain_text.contains("env child"), "{}", copy.plain_text);
    // 中间段落的 InlineMath 对象由同一片段保留原树，不输出替换字符。
    assert!(copy
        .fragment
        .blocks
        .iter()
        .any(|block| matches!(block, Block::Paragraph(paragraph)
            if paragraph.inlines.iter().any(|content| matches!(content, ParagraphContent::Math(_))))));
    let _ = second;
}

#[test]
fn copy_whole_environment_keeps_environment_block() {
    let environment_id = NodeId::generate();
    let environment = Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: Default::default(),
        title: Some(Paragraph::empty(NodeId::generate())),
        children: vec![paragraph_with(NodeId::generate(), "body")],
        extra: Default::default(),
    });
    let mut session = session_with_body(vec![
        environment,
        paragraph_with(NodeId::generate(), "tail"),
    ]);
    session
        .select_block_environment(environment_id)
        .unwrap_or_else(|error| panic!("environment selection should be accepted: {error}"));

    let copy = session
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("environment copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("environment selection should produce a copy"));
    assert_eq!(copy.fragment.shape, ClipboardFragmentShape::Blocks);
    assert!(matches!(
        copy.fragment.blocks.as_slice(),
        [Block::Environment(_)]
    ));
    assert!(copy.plain_text.contains("body"));
}

#[test]
fn copy_math_checked_loss_returns_stable_error_and_keeps_state() {
    let first = NodeId::generate();
    let mut tree = parse_tree("x", MathInputKind::Equation);
    tree.extra
        .insert("futureField".to_owned(), serde_json::json!(true));
    let equation = Block::Equation(caret_doc::Equation {
        id: NodeId::generate(),
        math_tree: tree,
        extra: Default::default(),
    });
    let last = NodeId::generate();
    let mut session = session_with_body(vec![
        paragraph_with(first.clone(), "before"),
        equation,
        paragraph_with(last.clone(), "after"),
    ]);
    select(&mut session, pos(&first, 0), pos(&last, 5));
    let before_document = session.document().clone();
    match session.copy_clipboard_fragment() {
        Err(EditError::ClipboardMathLoss(reason)) => {
            assert!(reason.contains("TeX form"), "unexpected reason: {reason}");
        }
        other => panic!("expected checked math loss, got {other:?}"),
    }
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.document_generation(), 0);
}

#[test]
fn paste_inline_fragment_merges_text_flows_and_undoes_as_one_generation() {
    let id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(id.clone(), "abcdef")]);
    select(&mut session, pos(&id, 0), pos(&id, 2));
    let copy = session
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("copy should produce a fragment"));
    let original = session.document().clone();
    let baseline_generation = session.document_generation();
    let baseline_history = session.history_state_id();

    select(&mut session, pos(&id, 4), pos(&id, 4));
    session
        .paste_clipboard_fragment(copy.fragment.clone())
        .unwrap_or_else(|error| panic!("inline paste should succeed: {error}"));
    assert_eq!(body_paragraph_texts(&session), vec!["abcdabef".to_owned()]);
    assert_eq!(session.document_generation(), baseline_generation + 1);
    assert_ne!(session.history_state_id(), baseline_history);
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        unreachable!();
    };
    assert_eq!(paragraph.inlines.len(), 1, "adjacent TextFlow should merge");
    assert_eq!(session.selection().focus.offset, 6);
    assert!(!session.has_structure_selection());

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo should succeed: {error}"));
    assert_eq!(session.document(), &original);
    assert_eq!(session.document_generation(), baseline_generation + 2);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo should succeed: {error}"));
    assert_eq!(body_paragraph_texts(&session), vec!["abcdabef".to_owned()]);
}

#[test]
fn paste_blocks_replaces_selection_and_merges_clipped_boundaries() {
    let paragraph_one = NodeId::generate();
    let paragraph_two = NodeId::generate();
    let mut session = session_with_body(vec![
        paragraph_with(paragraph_one.clone(), "one"),
        paragraph_with(paragraph_two.clone(), "two"),
    ]);
    select(&mut session, pos(&paragraph_one, 1), pos(&paragraph_two, 2));
    let copy = session
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("copy should produce a fragment"));
    assert_eq!(copy.fragment.shape, ClipboardFragmentShape::Blocks);
    assert!(copy.fragment.start_clipped);
    assert!(copy.fragment.end_clipped);

    let target = NodeId::generate();
    let mut target_session = session_with_body(vec![paragraph_with(target.clone(), "AAA BBB")]);
    select(&mut target_session, pos(&target, 2), pos(&target, 6));
    target_session
        .paste_clipboard_fragment(copy.fragment)
        .unwrap_or_else(|error| panic!("block paste should succeed: {error}"));
    let texts = body_paragraph_texts(&target_session);
    assert_eq!(texts, vec!["AAne".to_owned(), "twB".to_owned()]);
    assert!(target_session.selection().anchor == target_session.selection().focus);
    assert!(matches!(
        &target_session.document.body[0],
        Block::Paragraph(paragraph) if paragraph.id == target
    ));
}

#[test]
fn paste_blocks_at_paragraph_boundaries_inserts_without_overlap() {
    let first = NodeId::generate();
    let second = NodeId::generate();
    let mut source = session_with_body(vec![
        paragraph_with(first.clone(), "one"),
        paragraph_with(second.clone(), "two"),
    ]);
    select(&mut source, pos(&first, 0), pos(&second, 3));
    let copy = source
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("copy should produce a fragment"));
    assert_eq!(copy.fragment.shape, ClipboardFragmentShape::Blocks);
    assert!(!copy.fragment.start_clipped);
    assert!(!copy.fragment.end_clipped);

    // 段首：目标段落整体保留在片段之后。
    let target = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(target.clone(), "tail")]);
    select(&mut session, pos(&target, 0), pos(&target, 0));
    session
        .paste_clipboard_fragment(copy.fragment.clone())
        .unwrap_or_else(|error| panic!("paste at paragraph start should succeed: {error}"));
    assert_eq!(
        body_paragraph_texts(&session),
        vec!["one".to_owned(), "two".to_owned(), "tail".to_owned()]
    );
    assert!(matches!(
        session.document().body.last(),
        Some(Block::Paragraph(paragraph)) if paragraph.id == target
    ));

    // 段尾：目标段落整体保留在片段之前。
    let target = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(target.clone(), "head")]);
    select(&mut session, pos(&target, 4), pos(&target, 4));
    session
        .paste_clipboard_fragment(copy.fragment)
        .unwrap_or_else(|error| panic!("paste at paragraph end should succeed: {error}"));
    assert_eq!(
        body_paragraph_texts(&session),
        vec!["head".to_owned(), "one".to_owned(), "two".to_owned()]
    );
    assert!(matches!(
        session.document().body.first(),
        Some(Block::Paragraph(paragraph)) if paragraph.id == target
    ));
}

#[test]
fn paste_whole_environment_block_replaces_caret_paragraph_content() {
    let environment_id = NodeId::generate();
    let environment = Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: Default::default(),
        title: Some({
            let mut title = Paragraph::empty(NodeId::generate());
            title.inlines.push(text_flow("Title"));
            title
        }),
        children: vec![paragraph_with(NodeId::generate(), "body")],
        extra: Default::default(),
    });
    let mut source = session_with_body(vec![
        environment,
        paragraph_with(NodeId::generate(), "tail"),
    ]);
    source
        .select_block_environment(environment_id)
        .unwrap_or_else(|error| panic!("environment selection should be accepted: {error}"));
    let copy = source
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("environment copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("environment selection should produce a copy"));

    let target = NodeId::generate();
    let mut target_session = session_with_body(vec![paragraph_with(target.clone(), "start end")]);
    select(&mut target_session, pos(&target, 5), pos(&target, 5));
    target_session
        .paste_clipboard_fragment(copy.fragment)
        .unwrap_or_else(|error| panic!("environment paste should succeed: {error}"));
    target_session
        .document()
        .validate()
        .unwrap_or_else(|error| panic!("pasted document must validate: {error}"));
    assert!(
        target_session
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Environment(environment) if environment.kind == BlockEnvironmentKind::Section)),
        "pasted environment should be preserved as a block"
    );
    assert!(target_session.selection().anchor == target_session.selection().focus);
    let focus = target_session.selection().focus.clone();
    let location = target_session
        .paragraph_index
        .get(&focus.paragraph_id)
        .cloned()
        .unwrap_or_else(|| panic!("pasted caret should point at a live paragraph"));
    let paragraph = paragraph_at_location(&target_session.document.body, &location)
        .unwrap_or_else(|| panic!("caret paragraph should exist"));
    assert!(focus.offset <= paragraph_text_of(paragraph).len());
}

#[test]
fn paste_fragment_twice_regenerates_unique_ids_and_validates_document() {
    let id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(id.clone(), "abcdef")]);
    select(&mut session, pos(&id, 0), pos(&id, 2));
    let copy = session
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("copy should produce a fragment"));
    for _ in 0..2 {
        select(&mut session, pos(&id, 1), pos(&id, 1));
        session
            .paste_clipboard_fragment(copy.fragment.clone())
            .unwrap_or_else(|error| panic!("repeated paste should succeed: {error}"));
    }
    session
        .document()
        .validate()
        .unwrap_or_else(|error| panic!("document must stay valid: {error}"));
    assert_eq!(session.document_generation(), 2);
}

#[test]
fn paste_into_title_is_rejected_without_state_change() {
    let title_id = NodeId::generate();
    let environment_id = NodeId::generate();
    let environment = Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: Default::default(),
        title: Some(Paragraph::empty(title_id.clone())),
        children: vec![paragraph_with(NodeId::generate(), "body")],
        extra: Default::default(),
    });
    let mut session = session_with_body(vec![environment]);
    let title_position = Pos {
        story: StoryId::EnvironmentTitle(environment_id),
        paragraph_id: title_id,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    select(&mut session, title_position.clone(), title_position);
    let fragment = ClipboardFragment {
        shape: ClipboardFragmentShape::Blocks,
        start_clipped: false,
        end_clipped: false,
        blocks: vec![paragraph_with(NodeId::generate(), "pasted")],
    };
    let before_document = session.document().clone();
    let before_selection = session.selection().clone();
    let before_history = session.history_state_id();
    assert_eq!(
        session.paste_clipboard_fragment(fragment),
        Err(EditError::ClipboardHostMismatch)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.selection(), &before_selection);
    assert_eq!(session.document_generation(), 0);
    assert_eq!(session.history_state_id(), before_history);
    assert!(!session.can_undo());
}

#[test]
fn external_whole_display_math_pastes_equation_and_align_as_single_transaction() {
    for (source, align_expected) in [
        (r"\[x^2\]", false),
        (
            r"\[ \begin{aligned} a &= b \\ c &= d \end{aligned} \]",
            true,
        ),
    ] {
        let id = NodeId::generate();
        let mut session = session_with_body(vec![paragraph_with(id.clone(), "")]);
        let before_generation = session.document_generation();
        session
            .paste_external_clipboard_text(source)
            .unwrap_or_else(|error| panic!("display paste should succeed: {error}"));
        assert_eq!(session.document_generation(), before_generation + 1);
        assert!(session.can_undo());
        let block = session
            .document()
            .body
            .iter()
            .find(|block| matches!(block, Block::Equation(_) | Block::Align(_)))
            .unwrap_or_else(|| panic!("display formula block should be inserted for {source}"));
        match (block, align_expected) {
            (Block::Equation(equation), false) => {
                assert_eq!(
                    caret_math::serialize_tree(&equation.math_tree),
                    caret_math::serialize_tree(&parse_tree("x^2", MathInputKind::Equation))
                );
            }
            (Block::Align(align), true) => {
                assert_eq!(
                    caret_math::serialize_align(align),
                    caret_math::serialize_align(&parse_align(
                        r"\begin{aligned} a &= b \\ c &= d \end{aligned}",
                    ))
                );
            }
            (block, _) => panic!("unexpected display block: {block:?}"),
        }
        session
            .run(Command::Undo)
            .unwrap_or_else(|error| panic!("undo should succeed: {error}"));
        assert_eq!(session.document_generation(), before_generation + 2);
        assert_eq!(body_paragraph_texts(&session), vec![String::new()]);
    }
}

#[test]
fn external_inline_and_mixed_math_create_inline_math_at_selection() {
    let id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(id.clone(), "prefix  suffix")]);
    select(&mut session, pos(&id, 7), pos(&id, 8));
    session
        .paste_external_clipboard_text(r"\begin{aligned}") // 非闭合，按纯文本
        .unwrap_or_else(|error| panic!("plain fallback should succeed: {error}"));
    assert!(body_paragraph_texts(&session)[0].contains(r"\begin{aligned}"));

    let generation = session.document_generation();
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo should succeed: {error}"));
    assert_eq!(
        body_paragraph_texts(&session),
        vec!["prefix  suffix".to_owned()]
    );
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo should succeed: {error}"));
    assert_eq!(session.document_generation(), generation + 2);

    let id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(id.clone(), "ab")]);
    select(&mut session, pos(&id, 1), pos(&id, 2));
    session
        .paste_external_clipboard_text("a \\(x\\) b")
        .unwrap_or_else(|error| panic!("mixed paste should succeed: {error}"));
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        unreachable!();
    };
    assert!(
        paragraph
            .inlines
            .iter()
            .any(|content| matches!(content, ParagraphContent::Math(_))),
        "mixed text should create InlineMath"
    );
    assert_eq!(paragraph_text_of(paragraph), "aa \u{FFFC} b");
}

#[test]
fn external_dollar_formula_only_recognized_as_whole_text() {
    let id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(id.clone(), "")]);
    session
        .paste_external_clipboard_text("$5 and $6")
        .unwrap_or_else(|error| panic!("plain fallback should succeed: {error}"));
    assert_eq!(body_paragraph_texts(&session), vec!["$5 and $6".to_owned()]);

    let id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(id.clone(), "")]);
    session
        .paste_external_clipboard_text("$x$")
        .unwrap_or_else(|error| panic!("whole inline should succeed: {error}"));
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        unreachable!();
    };
    assert!(matches!(
        paragraph.inlines.as_slice(),
        [ParagraphContent::Math(_)]
    ));
    assert_eq!(paragraph_text_of(paragraph), "\u{FFFC}");
}

#[test]
fn external_paste_without_target_falls_back_to_plain_text() {
    let title_id = NodeId::generate();
    let environment_id = NodeId::generate();
    let environment = Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: Default::default(),
        title: Some(Paragraph::empty(title_id.clone())),
        children: vec![paragraph_with(NodeId::generate(), "body")],
        extra: Default::default(),
    });
    let mut session = session_with_body(vec![environment]);
    let title_position = Pos {
        story: StoryId::EnvironmentTitle(environment_id),
        paragraph_id: title_id,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    select(&mut session, title_position.clone(), title_position);
    session
        .paste_external_clipboard_text(r"\[x^2\]")
        .unwrap_or_else(|error| panic!("title fallback should succeed: {error}"));
    assert!(body_paragraph_texts(&session)[0].contains(r"\[x^2\]"));
    assert!(!matches!(
        session.document().body.first(),
        Some(Block::Equation(_))
    ));
}

#[test]
fn inline_fragment_paste_rejects_non_paragraph_hosts_without_state_change() {
    let id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(id.clone(), "abc")]);
    select(&mut session, pos(&id, 0), pos(&id, 1));
    let copy = session
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("copy should produce a fragment"));
    // 跨段普通选区不是 Inline 片段的合法宿主。
    let first = NodeId::generate();
    let second = NodeId::generate();
    let mut target = session_with_body(vec![
        paragraph_with(first.clone(), "a"),
        paragraph_with(second.clone(), "b"),
    ]);
    let anchor = pos(&first, 0);
    let focus = pos(&second, 1);
    select(&mut target, anchor, focus);
    let before_document = target.document().clone();
    assert_eq!(
        target.paste_clipboard_fragment(copy.fragment),
        Err(EditError::ClipboardHostMismatch)
    );
    assert_eq!(target.document(), &before_document);
    assert_eq!(target.document_generation(), 0);
    assert!(!target.can_undo());
    let _ = id;
}

#[test]
fn command_paste_paths_share_the_single_transaction_behaviour() {
    let id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(id.clone(), "abcdef")]);
    select(&mut session, pos(&id, 0), pos(&id, 2));
    let copy = session
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("copy should produce a fragment"));
    select(&mut session, pos(&id, 5), pos(&id, 5));
    let baseline = session.document_generation();
    let effect = session
        .run(Command::PasteClipboardFragment(Box::new(copy.fragment)))
        .unwrap_or_else(|error| panic!("command paste should succeed: {error}"));
    assert!(effect.document_changed && effect.selection_changed);
    assert_eq!(session.document_generation(), baseline + 1);
    assert_eq!(body_paragraph_texts(&session), vec!["abcdeabf".to_owned()]);

    let id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(id.clone(), "")]);
    let effect = session
        .run(Command::PasteExternalClipboardText("$x$".to_owned()))
        .unwrap_or_else(|error| panic!("command external paste should succeed: {error}"));
    assert!(effect.document_changed && effect.selection_changed);
    assert_eq!(session.document_generation(), 1);
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        unreachable!();
    };
    assert!(
        paragraph
            .inlines
            .iter()
            .any(|content| matches!(content, ParagraphContent::Math(_)))
    );
}

#[test]
fn paste_blocks_undoes_and_redoes_the_full_body_snapshot() {
    let environment_id = NodeId::generate();
    let environment = Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Custom,
        definition_id: None,
        props: Default::default(),
        title: None,
        children: vec![paragraph_with(NodeId::generate(), "env body")],
        extra: Default::default(),
    });
    let mut source = session_with_body(vec![
        environment,
        paragraph_with(NodeId::generate(), "tail"),
    ]);
    source
        .select_block_environment(environment_id)
        .unwrap_or_else(|error| panic!("environment selection should be accepted: {error}"));
    let copy = source
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("environment copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("environment selection should produce a copy"));

    let target_id = NodeId::generate();
    let mut target = session_with_body(vec![paragraph_with(target_id.clone(), "start end")]);
    let original = target.document().clone();
    select(&mut target, pos(&target_id, 5), pos(&target_id, 5));
    let baseline = target.document_generation();
    target
        .paste_clipboard_fragment(copy.fragment)
        .unwrap_or_else(|error| panic!("environment paste should succeed: {error}"));
    let pasted = target.document().clone();
    assert_ne!(pasted, original);
    assert_eq!(target.document_generation(), baseline + 1);
    target
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo should succeed: {error}"));
    assert_eq!(target.document(), &original);
    assert_eq!(target.document_generation(), baseline + 2);
    target
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo should succeed: {error}"));
    assert_eq!(target.document(), &pasted);
}

#[test]
fn paste_blocks_over_document_structure_selection_replaces_covered_blocks() {
    let source_id = NodeId::generate();
    let mut source = session_with_body(vec![paragraph_with(source_id.clone(), "replacement")]);
    select(&mut source, pos(&source_id, 0), pos(&source_id, 3));
    let copy = source
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("copy should produce a fragment"));
    assert_eq!(copy.fragment.shape, ClipboardFragmentShape::Inline);

    // Blocks 片段：复制跨段选区得到 Blocks 形状后，粘贴到覆盖整个环境的结构选区。
    let first = NodeId::generate();
    let second = NodeId::generate();
    let mut source = session_with_body(vec![
        paragraph_with(first.clone(), "one"),
        paragraph_with(second.clone(), "two"),
    ]);
    select(&mut source, pos(&first, 0), pos(&second, 3));
    let blocks = source
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("cross-paragraph copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("cross-paragraph copy should produce a fragment"));
    assert_eq!(blocks.fragment.shape, ClipboardFragmentShape::Blocks);

    let environment_id = NodeId::generate();
    let environment = Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Custom,
        definition_id: None,
        props: Default::default(),
        title: None,
        children: vec![paragraph_with(NodeId::generate(), "covered")],
        extra: Default::default(),
    });
    let mut target = session_with_body(vec![
        environment,
        paragraph_with(NodeId::generate(), "survivor"),
    ]);
    target
        .select_block_environment(environment_id)
        .unwrap_or_else(|error| panic!("environment selection should be accepted: {error}"));
    target
        .paste_clipboard_fragment(blocks.fragment)
        .unwrap_or_else(|error| panic!("structure selection paste should succeed: {error}"));
    target
        .document()
        .validate()
        .unwrap_or_else(|error| panic!("pasted document must validate: {error}"));
    assert!(
        !target
            .document()
            .body
            .iter()
            .any(|block| matches!(block, Block::Environment(_))),
        "covered environment should be replaced by the fragment"
    );
    let texts = body_paragraph_texts(&target);
    assert!(texts.contains(&"one".to_owned()), "texts: {texts:?}");
    assert!(texts.contains(&"two".to_owned()), "texts: {texts:?}");
    assert!(texts.contains(&"survivor".to_owned()), "texts: {texts:?}");
}

#[test]
fn paste_blocks_inside_environment_stays_in_environment_children() {
    let first = NodeId::generate();
    let second = NodeId::generate();
    let mut source = session_with_body(vec![
        paragraph_with(first.clone(), "one"),
        paragraph_with(second.clone(), "two"),
    ]);
    select(&mut source, pos(&first, 0), pos(&second, 3));
    let copy = source
        .copy_clipboard_fragment()
        .unwrap_or_else(|error| panic!("copy should succeed: {error}"))
        .unwrap_or_else(|| panic!("copy should produce a fragment"));

    let child_id = NodeId::generate();
    let environment_id = NodeId::generate();
    let environment = Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Custom,
        definition_id: None,
        props: Default::default(),
        title: None,
        children: vec![paragraph_with(child_id.clone(), "hello world")],
        extra: Default::default(),
    });
    let mut target = session_with_body(vec![
        environment,
        paragraph_with(NodeId::generate(), "tail"),
    ]);
    select(&mut target, pos(&child_id, 5), pos(&child_id, 5));
    target
        .paste_clipboard_fragment(copy.fragment)
        .unwrap_or_else(|error| panic!("nested paste should succeed: {error}"));
    target
        .document()
        .validate()
        .unwrap_or_else(|error| panic!("pasted document must validate: {error}"));
    let Block::Environment(environment) = &target.document().body[0] else {
        panic!("paste target environment should survive");
    };
    assert_eq!(environment.id, environment_id);
    let child_texts = environment
        .children
        .iter()
        .map(|block| match block {
            Block::Paragraph(paragraph) => paragraph_text_of(paragraph),
            _ => panic!("pasted content should stay inside environment children"),
        })
        .collect::<Vec<_>>();
    assert!(
        child_texts.contains(&"hello".to_owned()),
        "children: {child_texts:?}"
    );
    assert!(
        child_texts.contains(&"one".to_owned()),
        "children: {child_texts:?}"
    );
    assert!(
        child_texts.contains(&"two".to_owned()),
        "children: {child_texts:?}"
    );
    assert!(
        child_texts.contains(&" world".to_owned()),
        "children: {child_texts:?}"
    );
}

#[test]
fn copy_with_non_empty_math_text_selection_reports_host_mismatch() {
    let mut session = session_with_body(vec![paragraph_with(NodeId::generate(), "")]);
    session
        .run(Command::InsertInlineMath(r"\frac{1}{\text{AB}}".to_owned()))
        .unwrap_or_else(|error| panic!("inline math insertion should succeed: {error}"));
    session
        .run(Command::EnterMath(crate::editing::Direction::Backward))
        .unwrap_or_else(|error| panic!("math entry should succeed: {error}"));
    for _ in 0..64 {
        if session
            .math_edit()
            .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Text { .. }))
        {
            break;
        }
        session
            .run(Command::MoveCaret(crate::editing::Direction::Backward))
            .unwrap_or_else(|error| panic!("math navigation should succeed: {error}"));
    }
    session
        .run(Command::SetMathTextSelection {
            anchor: "AB".len(),
            focus: 0,
        })
        .unwrap_or_else(|error| panic!("math text selection should succeed: {error}"));
    assert_eq!(
        session.copy_clipboard_fragment(),
        Err(EditError::ClipboardHostMismatch)
    );
}

#[test]
fn paste_fragment_during_math_edit_is_rejected_without_state_change() {
    let id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(id.clone(), "abcdef")]);
    session
        .begin_inline_math()
        .unwrap_or_else(|error| panic!("inline math entry should succeed: {error}"));
    assert!(session.math_edit().is_some());
    let fragment = ClipboardFragment {
        shape: ClipboardFragmentShape::Inline,
        start_clipped: false,
        end_clipped: false,
        blocks: vec![paragraph_with(NodeId::generate(), "pasted")],
    };
    let before_document = session.document().clone();
    assert_eq!(
        session.paste_clipboard_fragment(fragment),
        Err(EditError::ClipboardHostMismatch)
    );
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.document_generation(), 1);
}

fn equation_block(source: &str) -> Block {
    Block::Equation(caret_doc::Equation {
        id: NodeId::generate(),
        math_tree: parse_tree(source, MathInputKind::Equation),
        extra: Default::default(),
    })
}

fn clipped_paragraph_fragment(
    text: &str,
    start_clipped: bool,
    end_clipped: bool,
) -> ClipboardFragment {
    ClipboardFragment {
        shape: ClipboardFragmentShape::Blocks,
        start_clipped,
        end_clipped,
        blocks: vec![paragraph_with(NodeId::generate(), text)],
    }
}

fn caret_paragraph_text(session: &Session) -> (String, usize) {
    let focus = session.selection().focus.clone();
    let paragraph = session
        .document
        .body
        .iter()
        .find_map(|block| match block {
            Block::Paragraph(paragraph) if paragraph.id == focus.paragraph_id => Some(paragraph),
            Block::Paragraph(_) => None,
            Block::Environment(environment) => {
                find_paragraph(&environment.children, &focus.paragraph_id)
            }
            Block::Table(_)
            | Block::Equation(_)
            | Block::Align(_)
            | Block::ExplicitBreak(_)
            | Block::Opaque(_) => None,
        })
        .unwrap_or_else(|| panic!("caret paragraph {} is missing", focus.paragraph_id));
    (paragraph_text_of(paragraph), focus.offset)
}

fn find_paragraph<'a>(blocks: &'a [Block], id: &NodeId) -> Option<&'a Paragraph> {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) if &paragraph.id == id => return Some(paragraph),
            Block::Paragraph(_) => {}
            Block::Table(_) => {}
            Block::Environment(environment) => {
                if let Some(found) = find_paragraph(&environment.children, id) {
                    return Some(found);
                }
                if let Some(title) = &environment.title
                    && &title.id == id
                {
                    return Some(title);
                }
            }
            Block::Equation(_) | Block::Align(_) | Block::ExplicitBreak(_) | Block::Opaque(_) => {}
        }
    }
    None
}

#[test]
fn block_paste_into_body_without_paragraphs_adds_a_caret_home() {
    let paragraph_id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(paragraph_id.clone(), "abc")]);
    select(&mut session, pos(&paragraph_id, 0), pos(&paragraph_id, 3));
    session
        .paste_clipboard_fragment(ClipboardFragment {
            shape: ClipboardFragmentShape::Blocks,
            start_clipped: false,
            end_clipped: false,
            blocks: vec![equation_block("x")],
        })
        .unwrap_or_else(|error| panic!("equation fragment paste should succeed: {error}"));
    assert!(matches!(
        session.document.body.first(),
        Some(Block::Equation(_))
    ));
    let Block::Paragraph(paragraph) = session
        .document
        .body
        .last()
        .unwrap_or_else(|| panic!("caret home paragraph missing"))
    else {
        panic!("caret home should be a paragraph");
    };
    assert_eq!(paragraph_text_of(paragraph), "");
    assert_eq!(session.selection().focus.paragraph_id, paragraph.id);
    assert_eq!(session.selection().focus.offset, 0);
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo should succeed: {error}"));
    assert_eq!(body_paragraph_texts(&session), vec!["abc"]);
}

#[test]
fn block_paste_caret_lands_in_the_last_inserted_paragraph() {
    let target_id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(target_id.clone(), "abc")]);
    select(&mut session, pos(&target_id, 1), pos(&target_id, 1));
    session
        .paste_clipboard_fragment(ClipboardFragment {
            shape: ClipboardFragmentShape::Blocks,
            start_clipped: true,
            end_clipped: false,
            blocks: vec![
                paragraph_with(NodeId::generate(), "ne"),
                paragraph_with(NodeId::generate(), "two"),
            ],
        })
        .unwrap_or_else(|error| panic!("clipped block fragment should paste: {error}"));
    assert_eq!(body_paragraph_texts(&session), vec!["ane", "two", "bc"]);
    assert_eq!(caret_paragraph_text(&session), ("two".to_owned(), 3));
}

#[test]
fn block_paste_ending_in_explicit_break_parks_caret_after_the_break() {
    let target_id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(target_id.clone(), "abc")]);
    select(&mut session, pos(&target_id, 1), pos(&target_id, 1));
    let mut fragment = clipped_paragraph_fragment("ne", true, false);
    fragment
        .blocks
        .push(Block::ExplicitBreak(caret_doc::ExplicitBreak::default()));
    session
        .paste_clipboard_fragment(fragment)
        .unwrap_or_else(|error| panic!("explicit break fragment should paste: {error}"));
    assert_eq!(body_paragraph_texts(&session), vec!["ane", "bc"]);
    assert_eq!(caret_paragraph_text(&session), ("bc".to_owned(), 0));
}

#[test]
fn external_mixed_text_with_newlines_falls_back_to_plain_paste() {
    let target_id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(target_id.clone(), "abc")]);
    select(&mut session, pos(&target_id, 0), pos(&target_id, 3));
    session
        .paste_external_clipboard_text("line1\n\\(x\\)")
        .unwrap_or_else(|error| panic!("mixed text with newlines should fall back: {error}"));
    assert_eq!(body_paragraph_texts(&session), vec!["line1", "\\(x\\)"]);
    for block in &session.document.body {
        let Block::Paragraph(paragraph) = block else {
            continue;
        };
        for content in &paragraph.inlines {
            if let ParagraphContent::Text(flow) = content {
                for run in &flow.runs {
                    assert!(!run.text.contains('\n') && !run.text.contains('\r'));
                }
            }
        }
    }
    session
        .document
        .validate()
        .unwrap_or_else(|error| panic!("document should stay valid: {error}"));
}

#[test]
fn end_clipped_block_paste_keeps_caret_at_the_insertion_seam() {
    let target_id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(target_id.clone(), "ABCDEF")]);
    select(&mut session, pos(&target_id, 3), pos(&target_id, 3));
    session
        .paste_clipboard_fragment(ClipboardFragment {
            shape: ClipboardFragmentShape::Blocks,
            start_clipped: true,
            end_clipped: true,
            blocks: vec![
                paragraph_with(NodeId::generate(), "ne"),
                paragraph_with(NodeId::generate(), "tw"),
            ],
        })
        .unwrap_or_else(|error| panic!("both-clipped fragment should paste: {error}"));
    assert_eq!(body_paragraph_texts(&session), vec!["ABCne", "twDEF"]);
    assert_eq!(caret_paragraph_text(&session), ("twDEF".to_owned(), 2));
}

#[test]
fn block_paste_without_paragraphs_redo_restores_the_same_structure() {
    let paragraph_id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(paragraph_id.clone(), "abc")]);
    select(&mut session, pos(&paragraph_id, 0), pos(&paragraph_id, 3));
    session
        .paste_clipboard_fragment(ClipboardFragment {
            shape: ClipboardFragmentShape::Blocks,
            start_clipped: false,
            end_clipped: false,
            blocks: vec![equation_block("x")],
        })
        .unwrap_or_else(|error| panic!("equation fragment paste should succeed: {error}"));
    let pasted = session.document.clone();
    let pasted_caret = session.selection().focus.clone();
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo should succeed: {error}"));
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo should succeed: {error}"));
    assert_eq!(session.document, pasted);
    assert_eq!(session.selection().focus, pasted_caret);
}

#[test]
fn external_mixed_text_newline_fallback_is_a_single_undo_step() {
    let target_id = NodeId::generate();
    let mut session = session_with_body(vec![paragraph_with(target_id.clone(), "abc")]);
    select(&mut session, pos(&target_id, 0), pos(&target_id, 3));
    let generation = session.document_generation();
    session
        .paste_external_clipboard_text("line1\n\\(x\\)")
        .unwrap_or_else(|error| panic!("mixed text with newlines should fall back: {error}"));
    assert_ne!(session.document_generation(), generation);
    assert!(session.can_undo());
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo should succeed: {error}"));
    assert_eq!(body_paragraph_texts(&session), vec!["abc"]);
    assert!(!session.can_undo());
}
