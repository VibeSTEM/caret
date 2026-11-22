//! 测量 50×8 表格的整表布局与单元格内容重排基线。

use caret_doc::{
    Block, Document, NodeId, Paragraph, ParagraphContent, Run, Table, TableCell, TableRow, TextFlow,
};
use caret_engine::DocumentLayoutService;
use caret_layout::{FontCatalog, ShapingStyle};
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use std::collections::BTreeMap;
use std::hint::black_box;

const ROW_COUNT: usize = 50;
const COLUMN_COUNT: usize = 8;
const CELL_TEXT: &str = "Caret table cell content wraps inside the measured column.";
const FONT_SIZE_CENTIPOINTS: u16 = 1200;

fn paragraph(text: &str) -> Paragraph {
    Paragraph {
        id: NodeId::generate(),
        language: Some("en-US".to_owned()),
        layout: None,
        inlines: vec![ParagraphContent::Text(TextFlow {
            runs: vec![Run {
                text: text.to_owned(),
                language: None,
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        })],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }
}

fn table_document(rows: usize, columns: usize) -> Document {
    let rows = (0..rows)
        .map(|row_index| TableRow {
            id: NodeId::generate(),
            cells: (0..columns)
                .map(|column_index| TableCell {
                    id: NodeId::generate(),
                    blocks: vec![Block::Paragraph(paragraph(&format!(
                        "{CELL_TEXT} r{row_index} c{column_index}"
                    )))],
                })
                .collect(),
        })
        .collect();
    let mut document = Document::blank();
    document.body = vec![Block::Table(Table {
        id: NodeId::generate(),
        rows,
    })];
    document
}

fn benchmark_table_layout(criterion: &mut Criterion) {
    let document = table_document(ROW_COUNT, COLUMN_COUNT);
    let cell_count = ROW_COUNT * COLUMN_COUNT;
    let mut group = criterion.benchmark_group("rust/table_layout");
    group.throughput(Throughput::Elements(cell_count as u64));
    group.bench_function("fifty_by_eight_full_layout", |bencher| {
        bencher.iter_batched(
            || {
                (
                    DocumentLayoutService::new(),
                    FontCatalog::system().unwrap_or_else(|error| panic!("font catalog: {error}")),
                )
            },
            |(mut service, mut catalog)| {
                let snapshot = service
                    .layout(
                        &document,
                        1,
                        &mut catalog,
                        &ShapingStyle::new(FONT_SIZE_CENTIPOINTS),
                    )
                    .unwrap_or_else(|error| panic!("table layout failed: {error}"));
                black_box(snapshot.pages.len());
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

criterion_group!(benches, benchmark_table_layout);
criterion_main!(benches);
