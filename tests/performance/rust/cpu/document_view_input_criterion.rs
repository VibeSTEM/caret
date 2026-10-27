//! 测量长文档中一次普通输入命令及其 UI 快照生成。

use caret_app::document_view::DocumentViewRuntime;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
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
    let mut ime_probe = prepare_runtime(&text).0;
    ime_probe
        .input_method_event_json("ni".to_owned(), String::new(), 0, 0, 2)
        .unwrap_or_else(|error| panic!("无法准备长文档输入法预编辑：{error}"));
    let ime_delta_bytes = ime_probe
        .input_method_event_json(String::new(), "你好".to_owned(), 0, 0, 0)
        .unwrap_or_else(|error| panic!("长文档输入法候选提交失败：{error}"))
        .len();
    let mut preedit_probe = prepare_runtime(&text).0;
    preedit_probe
        .input_method_event_json("ni".to_owned(), String::new(), 0, 0, 2)
        .unwrap_or_else(|error| panic!("无法准备长文档输入法预编辑：{error}"));
    let preedit_delta_bytes = preedit_probe
        .input_method_event_json("ni hao".to_owned(), String::new(), 0, 0, 5)
        .unwrap_or_else(|error| panic!("长文档输入法预编辑更新失败：{error}"))
        .len();
    eprintln!(
        "100k snapshot payload: full={full_snapshot_bytes} bytes, text_delta={delta_snapshot_bytes} bytes, ime_preedit_delta={preedit_delta_bytes} bytes, ime_commit_delta={ime_delta_bytes} bytes"
    );
    let mut group = criterion.benchmark_group("rust/document_view_input_100k");
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
    group.bench_function("ime_candidate_commit_and_ui_snapshot", |bencher| {
        bencher.iter_batched_ref(
            || {
                let mut runtime = prepare_runtime(&text).0;
                runtime
                    .input_method_event_json("ni".to_owned(), String::new(), 0, 0, 2)
                    .unwrap_or_else(|error| panic!("无法准备长文档输入法预编辑：{error}"));
                runtime
            },
            |runtime| {
                let response = runtime
                    .input_method_event_json(String::new(), "你好".to_owned(), 0, 0, 0)
                    .unwrap_or_else(|error| panic!("长文档输入法候选提交失败：{error}"));
                black_box(response.len())
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("ime_fast_preedit_update_and_ui_snapshot", |bencher| {
        bencher.iter_batched_ref(
            || {
                let mut runtime = prepare_runtime(&text).0;
                runtime
                    .input_method_event_json("ni".to_owned(), String::new(), 0, 0, 2)
                    .unwrap_or_else(|error| panic!("无法准备长文档输入法预编辑：{error}"));
                runtime
            },
            |runtime| {
                let response = runtime
                    .input_method_event_json("ni hao".to_owned(), String::new(), 0, 0, 5)
                    .unwrap_or_else(|error| panic!("长文档输入法预编辑更新失败：{error}"));
                black_box(response.len())
            },
            BatchSize::LargeInput,
        );
    });
    let large_paste = "多语言输入法与粘贴性能验证。".repeat(715);
    group.bench_function("paste_10010_chars_and_full_snapshot", |bencher| {
        bencher.iter_batched_ref(
            || (prepare_runtime(&text).0, large_paste.clone()),
            |(runtime, paste)| {
                let response = runtime
                    .paste_text_json(std::mem::take(paste))
                    .unwrap_or_else(|error| panic!("长文档粘贴失败：{error}"));
                black_box(response.len())
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("return_split_and_full_snapshot", |bencher| {
        bencher.iter_batched_ref(
            || prepare_runtime(&text).0,
            |runtime| {
                let response = runtime
                    .split_paragraph_json()
                    .unwrap_or_else(|error| panic!("长文档拆分段落失败：{error}"));
                black_box(response.len())
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

criterion_group!(benches, long_document_input);
criterion_main!(benches);
