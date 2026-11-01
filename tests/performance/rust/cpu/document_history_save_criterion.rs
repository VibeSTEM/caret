//! 测量 10 万与 100 万字符文档的撤销、重做和保存快照。

use caret_app::document_session::DocumentSessionService;
use caret_doc::{Block, CdocFile, Document, NodeId, Paragraph, ParagraphContent, Run, TextFlow};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::path::PathBuf;
use unicode_segmentation::UnicodeSegmentation;

const NOVEL_SOURCE: &str = include_str!("../../fixtures/novel-1m-multilingual.txt");
const PARAGRAPH_LANGUAGES: [&str; 7] =
    ["zh-Hans", "en-US", "fr-FR", "ar", "ru-RU", "el-GR", "ja-JP"];

struct Fixture {
    path: PathBuf,
}

impl Fixture {
    fn create(paragraph_count: usize) -> Self {
        let path = std::env::temp_dir().join(format!(
            "caret-history-save-{paragraph_count}-{}.cdoc",
            std::process::id()
        ));
        let paragraphs: Vec<_> = NOVEL_SOURCE.lines().take(paragraph_count).collect();
        assert_eq!(paragraphs.len(), paragraph_count);
        let mut document = Document::blank();
        document.global_settings.default_language = Some("en-US".to_owned());
        document.body = paragraphs
            .into_iter()
            .enumerate()
            .map(|(index, text)| {
                Block::Paragraph(Paragraph {
                    id: NodeId::generate(),
                    language: Some(
                        PARAGRAPH_LANGUAGES[index % PARAGRAPH_LANGUAGES.len()].to_owned(),
                    ),
                    layout: None,
                    inlines: vec![text_flow(Run {
                        text: text.to_owned(),
                        language: None,
                        props: BTreeMap::new(),
                        extra: BTreeMap::new(),
                    })],
                    mark_style: BTreeMap::new(),
                    props: BTreeMap::new(),
                    extra: BTreeMap::new(),
                })
            })
            .collect();
        let bytes = CdocFile::new(document)
            .and_then(|file| file.to_bytes())
            .unwrap_or_else(|error| panic!("无法生成长文档基准夹具：{error}"));
        std::fs::write(&path, bytes)
            .unwrap_or_else(|error| panic!("无法写入长文档基准夹具：{error}"));
        Self { path }
    }

    fn open(&self) -> DocumentSessionService {
        let bytes = std::fs::read(&self.path)
            .unwrap_or_else(|error| panic!("无法读取长文档基准夹具：{error}"));
        DocumentSessionService::prepare_open(&bytes, &self.path, 1)
            .unwrap_or_else(|error| panic!("无法准备长文档会话：{error}"))
            .0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn text_flow(run: Run) -> ParagraphContent {
    ParagraphContent::Text(TextFlow {
        runs: vec![run],
        source_range: None,
        extra: BTreeMap::new(),
    })
}

fn place_caret_in_middle(session: &mut DocumentSessionService, paragraph_index: usize) {
    let paragraph = match session.view_mut().document().body.get(paragraph_index) {
        Some(Block::Paragraph(paragraph)) => paragraph,
        _ => panic!("基准目标必须是段落"),
    };
    let (paragraph_id, offset) = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            ParagraphContent::Text(flow) => flow.runs.first().map(|run| {
                (
                    paragraph.id.to_string(),
                    run.text
                        .grapheme_indices(true)
                        .nth(100)
                        .map(|(offset, _)| offset)
                        .unwrap_or(run.text.len()),
                )
            }),
            _ => None,
        })
        .unwrap_or_else(|| panic!("基准段落必须包含文本"));
    let endpoint = serde_json::json!({
        "paragraphId": paragraph_id,
        "offsetUtf8": offset.to_string(),
        "affinity": "downstream",
        "visualAffinity": "downstream",
    });
    let selection = serde_json::json!({"anchor": endpoint, "focus": endpoint}).to_string();
    session
        .view_mut()
        .set_selection_json(&selection)
        .unwrap_or_else(|error| panic!("无法设置长文档基准光标：{error}"));
}

fn prepare_edited(fixture: &Fixture, paragraph_index: usize) -> DocumentSessionService {
    let mut session = fixture.open();
    place_caret_in_middle(&mut session, paragraph_index);
    session
        .view_mut()
        .insert_text_json("x".to_owned())
        .unwrap_or_else(|error| panic!("无法准备撤销基准状态：{error}"));
    session
}

fn benchmark_size(criterion: &mut Criterion, paragraph_count: usize, size_label: &str) {
    let fixture = Fixture::create(paragraph_count);
    let target_paragraph = paragraph_count / 2;
    let mut group = criterion.benchmark_group(format!("rust/history_save/{size_label}"));

    group.bench_function("undo_one_character_and_relayout", |bencher| {
        bencher.iter_batched_ref(
            || prepare_edited(&fixture, target_paragraph),
            |session| {
                let response = session
                    .view_mut()
                    .undo_json()
                    .unwrap_or_else(|error| panic!("长文档撤销失败：{error}"));
                black_box(response.len());
            },
            BatchSize::PerIteration,
        );
    });
    group.bench_function("redo_one_character_and_relayout", |bencher| {
        bencher.iter_batched_ref(
            || {
                let mut session = prepare_edited(&fixture, target_paragraph);
                session
                    .view_mut()
                    .undo_json()
                    .unwrap_or_else(|error| panic!("无法准备重做基准状态：{error}"));
                session
            },
            |session| {
                let response = session
                    .view_mut()
                    .redo_json()
                    .unwrap_or_else(|error| panic!("长文档重做失败：{error}"));
                black_box(response.len());
            },
            BatchSize::PerIteration,
        );
    });
    group.bench_function("serialize_save_snapshot", |bencher| {
        bencher.iter_batched_ref(
            || fixture.open(),
            |session| {
                let snapshot = session
                    .save_snapshot()
                    .unwrap_or_else(|error| panic!("无法序列化文档保存快照：{error}"));
                black_box((snapshot.bytes.len(), snapshot.content_digest));
            },
            BatchSize::PerIteration,
        );
    });

    group.finish();
}

fn benchmark_history_and_save(criterion: &mut Criterion) {
    benchmark_size(criterion, 500, "100k_chars");
    benchmark_size(criterion, 5_000, "1m_chars");
}

criterion_group!(benches, benchmark_history_and_save);
criterion_main!(benches);
