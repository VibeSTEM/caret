//! 测量百万字中文长篇的整篇排版与单段编辑重排。

use caret_doc::{Block, Document, NodeId, Paragraph, ParagraphContent, Run, TextFlow};
use caret_engine::{Affinity, Command, DocumentLayoutService, Pos, Selection, Session, StoryId};
use caret_layout::{FontCatalog, ShapingStyle};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::collections::BTreeMap;
use std::hint::black_box;

const PARAGRAPH_COUNT: usize = 5_000;
const CHARACTERS_PER_PARAGRAPH: usize = 200;
const NOVEL_CHARACTERS: usize = PARAGRAPH_COUNT * CHARACTERS_PER_PARAGRAPH;
const NOVEL_SOURCE: &str = include_str!("../../fixtures/novel-1m-multilingual.txt");
const PARAGRAPH_LANGUAGES: [&str; 7] =
    ["zh-Hans", "en-US", "fr-FR", "ar", "ru-RU", "el-GR", "ja-JP"];
const FONT_SIZE_CENTIPOINTS: u16 = 1200;
const EDIT_OFFSET_BYTES: usize = 120;

fn text_flow(run: Run) -> ParagraphContent {
    ParagraphContent::Text(TextFlow {
        runs: vec![run],
        source_range: None,
        source_map: Vec::new(),
        extra: BTreeMap::new(),
    })
}

fn novel_document() -> Document {
    let mut document = Document::blank();
    document.global_settings.default_language = Some("en-US".to_owned());
    let paragraphs: Vec<_> = NOVEL_SOURCE.lines().collect();
    assert_eq!(paragraphs.len(), PARAGRAPH_COUNT, "百万字语料段落数量不符");
    assert_eq!(
        paragraphs
            .iter()
            .map(|text| text.chars().count())
            .sum::<usize>(),
        NOVEL_CHARACTERS,
        "百万字语料字符数量不符"
    );
    document.body = paragraphs
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            Block::Paragraph(Paragraph {
                id: NodeId::generate(),
                language: Some(PARAGRAPH_LANGUAGES[index % PARAGRAPH_LANGUAGES.len()].to_owned()),
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
    document
}

fn make_session(document: &Document, paragraph_id: &NodeId) -> Session {
    let mut session =
        Session::new(document.clone()).unwrap_or_else(|error| panic!("长篇会话无效：{error}"));
    let position = Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset: EDIT_OFFSET_BYTES,
        affinity: Affinity::Downstream,
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: position.clone(),
            focus: position,
        })))
        .unwrap_or_else(|error| panic!("设置长篇基准光标失败：{error}"));
    session
}

fn benchmark_novel_document_layout(criterion: &mut Criterion) {
    let document = novel_document();
    let changed_paragraph_id = match &document.body[PARAGRAPH_COUNT / 2] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => panic!("长篇基准必须由段落组成"),
    };
    let mut catalog =
        FontCatalog::system().unwrap_or_else(|error| panic!("无法读取系统字体目录：{error}"));
    let mut service = DocumentLayoutService::new();
    let base_style = ShapingStyle::new(FONT_SIZE_CENTIPOINTS);
    let initial_session = make_session(&document, &changed_paragraph_id);
    let baseline = service
        .layout_session(&initial_session, &mut catalog, &base_style)
        .unwrap_or_else(|error| panic!("百万字长篇首次排版失败：{error}"));
    let first_selected_paragraph = match &document.body[PARAGRAPH_COUNT / 2] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("长篇基准必须由段落组成"),
    };
    let last_selected_paragraph = match &document.body[PARAGRAPH_COUNT / 2 + 9] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("长篇基准必须由段落组成"),
    };
    let selection = Selection {
        anchor: Pos {
            story: StoryId::Body,
            paragraph_id: first_selected_paragraph.id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        },
        focus: Pos {
            story: StoryId::Body,
            paragraph_id: last_selected_paragraph.id.clone(),
            offset: NOVEL_SOURCE
                .lines()
                .nth(PARAGRAPH_COUNT / 2 + 9)
                .map(str::len)
                .unwrap_or_else(|| panic!("长篇基准末段缺失")),
            affinity: Affinity::Downstream,
        },
    };

    let mut group = criterion.benchmark_group("rust/novel_1m_mixed_language");
    group.throughput(criterion::Throughput::Elements(NOVEL_CHARACTERS as u64));
    group.bench_function("full_document_layout", |bencher| {
        bencher.iter(|| {
            let snapshot = service
                .layout(black_box(&document), 1, &mut catalog, &base_style)
                .unwrap_or_else(|error| panic!("百万字长篇排版失败：{error}"));
            black_box((snapshot.paragraphs.len(), snapshot.pages.len()))
        });
    });
    group.bench_function("single_paragraph_edit_and_relayout", |bencher| {
        bencher.iter_batched(
            || make_session(&document, &changed_paragraph_id),
            |mut session| {
                session
                    .run(Command::InsertText("新".to_owned()))
                    .unwrap_or_else(|error| panic!("修改长篇基准段落失败：{error}"));
                let snapshot = service
                    .layout_session_reusing(
                        &session,
                        &baseline,
                        &changed_paragraph_id,
                        &mut catalog,
                        &base_style,
                    )
                    .unwrap_or_else(|error| panic!("长篇单段重排失败：{error}"));
                black_box((snapshot.paragraphs.len(), snapshot.pages.len()))
            },
            BatchSize::SmallInput,
        );
    });
    group.bench_function("selection_geometry_10_paragraph_range", |bencher| {
        bencher.iter(|| black_box(baseline.selection_geometry(black_box(&selection))));
    });
    group.finish();
}

criterion_group!(benches, benchmark_novel_document_layout);
criterion_main!(benches);
