//! 测量百万字符 `.cdoc` 的打开准备和单字素删除。

use caret_app::document_session::DocumentSessionService;
use caret_app::document_storage::DocumentStorage;
use caret_doc::{
    Block, CdocFile, Document, InlineNode, NodeId, OpenedDocument, Paragraph, Run, open,
};
use criterion::{Criterion, criterion_group, criterion_main};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;

const NOVEL_SOURCE: &str = include_str!("../../fixtures/novel-1m-multilingual.txt");
const PARAGRAPH_LANGUAGES: [&str; 7] =
    ["zh-Hans", "en-US", "fr-FR", "ar", "ru-RU", "el-GR", "ja-JP"];
const TARGET_PARAGRAPH_INDEX: usize = 2_500;
const PROFILE_DOCUMENT_ENV: &str = "CARET_PERF_CDOC";

struct BenchmarkDocument {
    path: PathBuf,
    temporary: bool,
}

impl BenchmarkDocument {
    fn load() -> Self {
        if let Some(path) = std::env::var_os(PROFILE_DOCUMENT_ENV) {
            return Self {
                path: PathBuf::from(path),
                temporary: false,
            };
        }

        let path = std::env::temp_dir().join(format!(
            "caret-performance-novel-{}.cdoc",
            std::process::id()
        ));
        let bytes = CdocFile::new(novel_document())
            .and_then(|file| file.to_bytes())
            .unwrap_or_else(|error| panic!("无法创建长文档性能夹具：{error}"));
        std::fs::write(&path, bytes)
            .unwrap_or_else(|error| panic!("无法写入长文档性能夹具：{error}"));
        Self {
            path,
            temporary: true,
        }
    }
}

impl Drop for BenchmarkDocument {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn novel_document() -> Document {
    let paragraphs: Vec<_> = NOVEL_SOURCE.lines().collect();
    assert_eq!(paragraphs.len(), 5_000, "百万字语料段落数量不符");
    let mut document = Document::blank();
    document.global_settings.default_language = Some("en-US".to_owned());
    document.body = paragraphs
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            Block::Paragraph(Paragraph {
                id: NodeId::generate(),
                language: Some(PARAGRAPH_LANGUAGES[index % PARAGRAPH_LANGUAGES.len()].to_owned()),
                layout: None,
                inlines: vec![InlineNode::Run(Run {
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

fn prepare_session(path: &Path) -> DocumentSessionService {
    let bytes = DocumentStorage
        .read_file_bytes(path)
        .unwrap_or_else(|error| panic!("无法读取性能文档：{error}"));
    DocumentSessionService::prepare_open(&bytes, path, 1)
        .unwrap_or_else(|error| panic!("无法打开性能文档：{error}"))
        .0
}

fn place_caret_after_first_grapheme(session: &mut DocumentSessionService) -> String {
    let paragraph = match session
        .view_mut()
        .document()
        .body
        .get(TARGET_PARAGRAPH_INDEX)
    {
        Some(Block::Paragraph(paragraph)) => paragraph,
        _ => panic!("性能文档目标必须是段落"),
    };
    let text = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            InlineNode::Run(run) => Some(run.text.as_str()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("性能文档目标段落必须包含文本"));
    let (offset, grapheme) = text
        .grapheme_indices(true)
        .next()
        .map(|(offset, grapheme)| (offset + grapheme.len(), grapheme.to_owned()))
        .unwrap_or_else(|| panic!("性能文档目标段落不能为空"));
    let paragraph_id = paragraph.id.to_string();
    let endpoint = serde_json::json!({
        "paragraphId": paragraph_id,
        "offsetUtf8": offset.to_string(),
        "affinity": "downstream",
        "visualAffinity": "downstream",
    });
    let selection = serde_json::json!({ "anchor": endpoint, "focus": endpoint }).to_string();
    session
        .view_mut()
        .set_selection_json(&selection)
        .unwrap_or_else(|error| panic!("无法放置性能测试光标：{error}"));
    grapheme
}

fn benchmark_open_and_delete(criterion: &mut Criterion) {
    let document = BenchmarkDocument::load();
    let document_bytes = DocumentStorage
        .read_file_bytes(&document.path)
        .unwrap_or_else(|error| panic!("无法读取性能文档：{error}"));
    let mut group = criterion.benchmark_group("rust/cdoc_1m_open_delete");
    group.throughput(criterion::Throughput::Elements(1_000_000));
    group.bench_function("decode_validate_cdoc", |bencher| {
        bencher.iter(|| {
            let opened = open(black_box(&document_bytes))
                .unwrap_or_else(|error| panic!("无法解析性能文档：{error}"));
            black_box(matches!(opened, OpenedDocument::Editable(_)));
        });
    });
    group.bench_function("open_read_validate_layout_and_snapshot", |bencher| {
        bencher.iter_custom(|iterations| {
            let mut elapsed = Duration::ZERO;
            for _ in 0..iterations {
                let start = Instant::now();
                let session = prepare_session(&document.path);
                black_box(session.document_generation());
                elapsed += start.elapsed();
            }
            elapsed
        });
    });
    group.bench_function("delete_one_grapheme_and_build_snapshot", |bencher| {
        bencher.iter_custom(|iterations| {
            let mut session = prepare_session(&document.path);
            let grapheme = place_caret_after_first_grapheme(&mut session);
            let mut elapsed = Duration::ZERO;
            for _ in 0..iterations {
                let start = Instant::now();
                let response = session
                    .view_mut()
                    .delete_backward_json()
                    .unwrap_or_else(|error| panic!("删除长文档字符失败：{error}"));
                elapsed += start.elapsed();
                black_box(response.len());
                session
                    .view_mut()
                    .insert_text_json(grapheme.clone())
                    .unwrap_or_else(|error| panic!("恢复性能文档字符失败：{error}"));
            }
            elapsed
        });
    });
    group.finish();
}

criterion_group!(benches, benchmark_open_and_delete);
criterion_main!(benches);
