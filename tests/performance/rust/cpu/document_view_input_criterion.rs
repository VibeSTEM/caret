//! 测量长文档中一次普通输入命令及其 UI 快照生成。

use caret_app::document_view::DocumentViewRuntime;
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

const NOVEL_SOURCE: &str = include_str!("../../fixtures/novel-1m-multilingual.txt");
const TEST_PARAGRAPH_COUNT: usize = 500;

fn novel_excerpt() -> String {
    let text = NOVEL_SOURCE
        .lines()
        .take(TEST_PARAGRAPH_COUNT)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(text.chars().count(), 100_499);
    text
}

fn prepare_runtime(text: &str) -> (DocumentViewRuntime, usize) {
    let mut runtime = DocumentViewRuntime::new_blank()
        .unwrap_or_else(|error| panic!("无法创建长文档基准运行时：{error}"));
    let initial_snapshot = runtime
        .paste_text_json(text.to_owned())
        .unwrap_or_else(|error| panic!("无法准备 10 万字符基准文档：{error}"));
    (runtime, initial_snapshot.len())
}

fn long_document_input(criterion: &mut Criterion) {
    let text = novel_excerpt();
    let (mut response_probe, full_snapshot_bytes) = prepare_runtime(&text);
    let delta_snapshot_bytes = response_probe
        .insert_text_delta_json("x".to_owned())
        .unwrap_or_else(|error| panic!("无法测量布局增量大小：{error}"))
        .len();
    eprintln!(
        "100k snapshot payload: full={full_snapshot_bytes} bytes, delta={delta_snapshot_bytes} bytes"
    );
    let mut group = criterion.benchmark_group("rust/document_view_input_100k");
    group.throughput(Throughput::Bytes(delta_snapshot_bytes as u64));
    group.bench_function("one_character_edit_and_ui_delta", |bencher| {
        bencher.iter_batched_ref(
            || prepare_runtime(&text).0,
            |runtime| {
                let response = runtime
                    .insert_text_delta_json(black_box("x".to_owned()))
                    .unwrap_or_else(|error| panic!("长文档输入失败：{error}"));
                black_box(response.len())
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

criterion_group!(benches, long_document_input);
criterion_main!(benches);
