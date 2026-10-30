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

fn prepare_formatting_runtime(text: &str) -> DocumentViewRuntime {
    let mut runtime = prepare_runtime(text).0;
    let snapshot: serde_json::Value = serde_json::from_str(
        &runtime
            .layout_snapshot_json()
            .unwrap_or_else(|error| panic!("无法读取格式性能基准文档快照：{error}")),
    )
    .unwrap_or_else(|error| panic!("无法解析格式性能基准文档快照：{error}"));
    let paragraphs = snapshot["paragraphs"]
        .as_array()
        .unwrap_or_else(|| panic!("格式性能基准文档没有段落：{snapshot}"));
    let paragraph = paragraphs
        .get(paragraphs.len() / 2)
        .unwrap_or_else(|| panic!("格式性能基准文档中间段落不存在：{}", paragraphs.len()));
    let paragraph_id = paragraph["paragraphId"]
        .as_str()
        .unwrap_or_else(|| panic!("格式性能基准段落缺少 ID：{paragraph}"));
    let text_length = paragraph["text"]
        .as_str()
        .unwrap_or_else(|| panic!("格式性能基准段落缺少文本：{paragraph}"))
        .len();
    let anchor = serde_json::json!({
        "paragraphId": paragraph_id,
        "offsetUtf8": "0",
        "affinity": "downstream",
        "visualAffinity": "downstream"
    });
    let focus = serde_json::json!({
        "paragraphId": paragraph_id,
        "offsetUtf8": text_length.to_string(),
        "affinity": "upstream",
        "visualAffinity": "upstream"
    });
    runtime
        .set_selection_json(&serde_json::json!({"anchor": anchor, "focus": focus}).to_string())
        .unwrap_or_else(|error| panic!("无法选择格式性能基准段落：{error}"));
    runtime
}

fn prepare_ime_runtime(text: &str) -> DocumentViewRuntime {
    let mut runtime = prepare_runtime(text).0;
    let page_index = current_caret_page_index(&mut runtime);
    runtime
        .rasterize_page_glyphs(page_index, 96.0 / 72.0, [0.0, 0.0], [32, 32, 32, 255])
        .unwrap_or_else(|error| panic!("无法预热可视页字形缓存：{error}"));
    runtime
        .input_method_event_json("ni".to_owned(), String::new(), 0, 0, 2)
        .unwrap_or_else(|error| panic!("无法准备长文档输入法预编辑：{error}"));
    let composition_page_index = current_caret_page_index(&mut runtime);
    runtime
        .rasterize_page_glyphs(
            composition_page_index,
            96.0 / 72.0,
            [0.0, 0.0],
            [32, 32, 32, 255],
        )
        .unwrap_or_else(|error| panic!("无法预热输入法预览字形缓存：{error}"));
    runtime
}

fn current_caret_page_index(runtime: &mut DocumentViewRuntime) -> usize {
    let caret: serde_json::Value = serde_json::from_str(
        &runtime
            .current_caret_json()
            .unwrap_or_else(|error| panic!("无法获取长文档光标位置：{error}")),
    )
    .unwrap_or_else(|error| panic!("无法解析长文档光标位置：{error}"));
    caret["pageIndex"]
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
        .unwrap_or_else(|| panic!("光标页面索引无效：{caret}"))
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
    group.bench_function(
        "ime_preedit_update_and_visible_page_raster_warm_cache",
        |bencher| {
            bencher.iter_batched_ref(
                || prepare_ime_runtime(&text),
                |runtime| {
                    let response = runtime
                        .input_method_event_json("ni hao".to_owned(), String::new(), 0, 0, 5)
                        .unwrap_or_else(|error| panic!("长文档输入法预编辑更新失败：{error}"));
                    let active_page_index = current_caret_page_index(runtime);
                    let page = runtime
                        .rasterize_page_glyphs(
                            active_page_index,
                            96.0 / 72.0,
                            [0.0, 0.0],
                            [32, 32, 32, 255],
                        )
                        .unwrap_or_else(|error| panic!("输入法更新后的可视页栅格化失败：{error}"));
                    black_box((response.len(), page.images.len(), page.placements.len()));
                },
                BatchSize::LargeInput,
            );
        },
    );
    group.bench_function("ime_candidate_commit_and_visible_page_raster", |bencher| {
        bencher.iter_batched_ref(
            || prepare_ime_runtime(&text),
            |runtime| {
                let response = runtime
                    .input_method_event_json(String::new(), "你好".to_owned(), 0, 0, 0)
                    .unwrap_or_else(|error| panic!("输入法候选提交失败：{error}"));
                let active_page_index = current_caret_page_index(runtime);
                let page = runtime
                    .rasterize_page_glyphs(
                        active_page_index,
                        96.0 / 72.0,
                        [0.0, 0.0],
                        [32, 32, 32, 255],
                    )
                    .unwrap_or_else(|error| panic!("候选提交后的可视页栅格化失败：{error}"));
                black_box((response.len(), page.images.len(), page.placements.len()));
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
    group.bench_function("underline_one_paragraph_in_100k_document", |bencher| {
        bencher.iter_batched_ref(
            || prepare_formatting_runtime(&text),
            |runtime| {
                runtime
                    .command_line_command_json("begin", String::new())
                    .unwrap_or_else(|error| panic!("无法打开格式命令：{error}"));
                runtime
                    .command_line_command_json("type", "underline".to_owned())
                    .unwrap_or_else(|error| panic!("无法查询下划线命令：{error}"));
                let response = runtime
                    .command_line_command_json("accept", String::new())
                    .unwrap_or_else(|error| panic!("应用长文档下划线格式失败：{error}"));
                black_box(response.len())
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

criterion_group!(benches, long_document_input);
criterion_main!(benches);
