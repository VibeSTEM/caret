//! 测量共享物理父链查询与深层文档 caret 顺序比较。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{Block, BlockEnvironment, BlockEnvironmentKind, Document, NodeId, Paragraph};
use caret_engine::{Affinity, Command, Pos, Selection, Session};
use criterion::{Criterion, criterion_group, criterion_main};
use std::collections::BTreeMap;
use std::hint::black_box;

const NESTED_DEPTH: usize = 16;
const ROOT_PARAGRAPH_COUNT: usize = 512;

struct PositionFixture {
    document: Document,
    environment_path: Vec<NodeId>,
    nested_paragraph_id: NodeId,
    root_tail_paragraph_id: NodeId,
}

fn position_fixture() -> PositionFixture {
    let nested_paragraph = Paragraph::empty(NodeId::generate());
    let nested_paragraph_id = nested_paragraph.id.clone();
    let mut children = vec![Block::Paragraph(nested_paragraph)];
    let mut environment_path = Vec::with_capacity(NESTED_DEPTH);

    for _ in 0..NESTED_DEPTH {
        let environment_id = NodeId::generate();
        environment_path.push(environment_id.clone());
        children = vec![Block::Environment(BlockEnvironment {
            id: environment_id,
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children,
            extra: BTreeMap::new(),
        })];
    }
    environment_path.reverse();

    let mut document = Document::blank();
    document.body.extend(
        (0..ROOT_PARAGRAPH_COUNT).map(|_| Block::Paragraph(Paragraph::empty(NodeId::generate()))),
    );
    let root_tail_paragraph_id = match document.body.last() {
        Some(Block::Paragraph(paragraph)) => paragraph.id.clone(),
        _ => unreachable!("基准前缀以段落结尾"),
    };
    document.body.extend(children);

    PositionFixture {
        document,
        environment_path,
        nested_paragraph_id,
        root_tail_paragraph_id,
    }
}

fn text_position(session: &Session, paragraph_id: NodeId) -> Pos {
    Pos {
        story: session
            .story_for_paragraph(&paragraph_id)
            .unwrap_or_else(|| panic!("基准段落 {paragraph_id} 未编入文档索引")),
        paragraph_id,
        offset: 0,
        affinity: Affinity::Downstream,
    }
}

fn prepare_comparison(
    fixture: &PositionFixture,
) -> (
    Session,
    caret_engine::SessionCaretSnapshot,
    caret_engine::SessionCaretSnapshot,
) {
    let mut session = Session::new(fixture.document.clone())
        .unwrap_or_else(|error| panic!("无法创建位置比较基准会话：{error}"));
    let root_tail = text_position(&session, fixture.root_tail_paragraph_id.clone());
    let nested = text_position(&session, fixture.nested_paragraph_id.clone());
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: root_tail.clone(),
            focus: root_tail,
        })))
        .unwrap_or_else(|error| panic!("无法定位根层末端基准段落：{error}"));
    let first_snapshot = session
        .current_caret_snapshot()
        .unwrap_or_else(|error| panic!("无法创建根层末端位置快照：{error}"));
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: nested.clone(),
            focus: nested,
        })))
        .unwrap_or_else(|error| panic!("无法定位深层基准段落：{error}"));
    let last_snapshot = session
        .current_caret_snapshot()
        .unwrap_or_else(|error| panic!("无法创建深层位置快照：{error}"));
    (session, first_snapshot, last_snapshot)
}

fn benchmark_document_position(criterion: &mut Criterion) {
    let fixture = position_fixture();
    let (session, first, last) = prepare_comparison(&fixture);
    assert_eq!(
        session.compare_caret_snapshots(&first, &last),
        Ok(std::cmp::Ordering::Less)
    );

    let mut group = criterion.benchmark_group("rust/document_position");
    group.bench_function(
        "physical_parent_lookup_after_root_512_depth_16",
        |bencher| {
            bencher.iter(|| {
                black_box(caret_structure::block_list_by_environment_ids(
                    &fixture.document.body,
                    black_box(&fixture.environment_path),
                ))
            });
        },
    );
    group.bench_function("caret_compare_root_tail_to_nested_16", |bencher| {
        bencher.iter(|| {
            black_box(
                session
                    .compare_caret_snapshots(black_box(&first), black_box(&last))
                    .unwrap_or_else(|error| panic!("基准位置比较失败：{error}")),
            )
        });
    });
    group.finish();
}

criterion_group!(benches, benchmark_document_position);
criterion_main!(benches);
