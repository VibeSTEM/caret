//! 测量数学源码投影、TextFlow 编辑与混合长文档布局的 CPU 基线。

use caret_doc::{
    Block, Document, Equation, InlineMath, NodeId, Paragraph, ParagraphContent, Run, TextFlow,
};
use caret_engine::{Command, Direction, DocumentLayoutService, MathEditBranch, Session};
use caret_layout::{FontCatalog, ShapingStyle};
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::hint::black_box;

const TEXT_SIZES: [usize; 2] = [256, 4_096];
const MIXED_PARAGRAPHS: usize = 500;
const MIXED_CHARS_PER_PARAGRAPH: usize = 200;
const MIXED_SOURCE: &str = include_str!("../../fixtures/novel-1m-multilingual.txt");
const MIXED_LANGUAGES: [&str; 7] = ["zh-Hans", "en-US", "fr-FR", "ar", "ru-RU", "el-GR", "ja-JP"];

fn structured_formula_source() -> String {
    let rows = (0..4)
        .map(|row| {
            (0..4)
                .map(|column| {
                    format!(
                        r"\frac{{\text{{row {row} cell {column} numerator}}}}{{\frac{{\text{{denominator {row}-{column}}}}}{{x_{row}+y_{column}}}}}"
                    )
                })
                .collect::<Vec<_>>()
                .join(" & ")
        })
        .collect::<Vec<_>>()
        .join(r" \\");
    format!(r"\begin{{bmatrix}}{rows}\end{{bmatrix}}")
}

fn prepare_structured_formula_session() -> Session {
    let source = structured_formula_source();
    let parsed = caret_math::parse(&source, caret_math::MathInputKind::Equation)
        .unwrap_or_else(|error| panic!("结构化大公式基准源码无效：{error}"));
    let caret_math::ParsedMath::Tree(math_tree) = parsed else {
        panic!("结构化大公式基准必须解析为公式块");
    };
    let mut document = Document::blank();
    document.body.insert(
        0,
        Block::Equation(Equation {
            id: NodeId::generate(),
            math_tree,
            extra: Default::default(),
        }),
    );
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("无法创建结构化大公式基准会话：{error}"));
    session
        .run(Command::EnterMath(Direction::Backward))
        .unwrap_or_else(|error| panic!("无法进入结构化大公式基准公式：{error}"));
    for _ in 0..256 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            return session;
        }
        session
            .run(Command::MoveCaret(Direction::Backward))
            .unwrap_or_else(|error| panic!("无法进入结构化大公式 TextFlow：{error}"));
    }
    panic!("结构化大公式没有可编辑的数学 TextFlow");
}

fn math_text_source(text: &str) -> String {
    format!(r"\frac{{\text{{{text}}}}}{{\beta}}")
}

fn prepare_text_edit_session(text_size: usize) -> Session {
    let source = math_text_source(&"a".repeat(text_size));
    let projection = caret_math::parse_projection(&source, caret_math::MathInputKind::Equation)
        .unwrap_or_else(|error| panic!("数学文本基准源码必须有效：{error}"));
    let (caret_math::ProjectedMath::Tree(math_tree), _) = projection.into_parts() else {
        panic!("显示公式基准必须解析为 MathTree");
    };
    let mut document = Document::blank();
    document.body.insert(
        0,
        Block::Equation(Equation {
            id: NodeId::generate(),
            math_tree,
            extra: Default::default(),
        }),
    );
    let mut session =
        Session::new(document).unwrap_or_else(|error| panic!("无法创建数学编辑基准会话：{error}"));
    session
        .run(Command::EnterMath(Direction::Backward))
        .unwrap_or_else(|error| panic!("无法进入数学文本基准公式：{error}"));
    let mut found_text_flow = false;
    for _ in 0..64 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            found_text_flow = true;
            break;
        }
        session
            .run(Command::MoveCaret(Direction::Backward))
            .unwrap_or_else(|error| panic!("无法进入数学 TextFlow 基准：{error}"));
    }
    assert!(found_text_flow, "基准公式没有可编辑的数学 TextFlow");
    session
        .run(Command::SetMathTextSelection {
            anchor: text_size / 2,
            focus: text_size / 2,
        })
        .unwrap_or_else(|error| panic!("无法定位数学 TextFlow 基准光标：{error}"));
    session
}

fn text_flow(text: String) -> ParagraphContent {
    ParagraphContent::Text(TextFlow {
        runs: vec![Run {
            text,
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: BTreeMap::new(),
    })
}

fn parsed_equation(source: &str) -> Equation {
    let parsed = caret_math::parse(source, caret_math::MathInputKind::Equation)
        .unwrap_or_else(|error| panic!("混合长文档 Equation 夹具无效：{error}"));
    let caret_math::ParsedMath::Tree(math_tree) = parsed else {
        panic!("Equation 夹具必须解析为公式块");
    };
    Equation {
        id: NodeId::generate(),
        math_tree,
        extra: Default::default(),
    }
}

fn mixed_long_document() -> Document {
    let prose: Vec<_> = MIXED_SOURCE.lines().take(MIXED_PARAGRAPHS).collect();
    assert_eq!(prose.len(), MIXED_PARAGRAPHS, "混合长文档语料段落数量不符");
    assert_eq!(
        prose.iter().map(|line| line.chars().count()).sum::<usize>(),
        MIXED_PARAGRAPHS * MIXED_CHARS_PER_PARAGRAPH,
        "混合长文档正文应有约 100k 个 Unicode 字符"
    );

    let mut document = Document::blank();
    document.global_settings.default_language = Some("en-US".to_owned());
    for (index, source) in prose.into_iter().enumerate() {
        let mut paragraph = Paragraph::empty(NodeId::generate());
        paragraph.language = Some(MIXED_LANGUAGES[index % MIXED_LANGUAGES.len()].to_owned());
        let split = source
            .chars()
            .take(MIXED_CHARS_PER_PARAGRAPH / 2)
            .collect::<String>();
        let suffix = source
            .chars()
            .skip(MIXED_CHARS_PER_PARAGRAPH / 2)
            .collect::<String>();
        paragraph.inlines.push(text_flow(split));
        if index % 10 == 0 {
            let parsed = caret_math::parse(
                r"x_i + \frac{\text{mixed math}}{\beta}",
                caret_math::MathInputKind::Inline,
            )
            .unwrap_or_else(|error| panic!("混合长文档 InlineMath 夹具无效：{error}"));
            let caret_math::ParsedMath::Tree(math_tree) = parsed else {
                panic!("InlineMath 夹具必须解析为行内公式");
            };
            paragraph.inlines.push(ParagraphContent::Math(InlineMath {
                id: NodeId::generate(),
                math_tree,
                extra: Default::default(),
            }));
        }
        paragraph.inlines.push(text_flow(suffix));
        document.body.push(Block::Paragraph(paragraph));

        if index % 100 == 99 {
            document.body.push(Block::Equation(parsed_equation(
                r"\frac{\text{published snapshot}}{\beta} + x^2",
            )));
        }
        if index % 100 == 49 {
            let parsed = caret_math::parse(
                r"\begin{aligned}\text{left}&=x^2\\\text{right}&=\frac{a}{b}\end{aligned}",
                caret_math::MathInputKind::Align,
            )
            .unwrap_or_else(|error| panic!("混合长文档 Align 夹具无效：{error}"));
            let caret_math::ParsedMath::Align(align) = parsed else {
                panic!("Align 夹具必须解析为多行对齐公式");
            };
            document.body.push(Block::Align(align));
        }
    }
    document
}

fn mixed_session(document: &Document) -> Session {
    Session::new(document.clone()).unwrap_or_else(|error| panic!("混合长文档会话无效：{error}"))
}

fn mixed_math_text_session(document: &Document) -> Session {
    let mut session = mixed_session(document);
    session
        .run(Command::InsertInlineMath(
            r"x_i + \frac{\text{mixed document math}}{\beta}".to_owned(),
        ))
        .unwrap_or_else(|error| panic!("无法准备长文档数学 TextFlow：{error}"));
    session
        .run(Command::EnterMath(Direction::Backward))
        .unwrap_or_else(|error| panic!("无法进入长文档行内公式：{error}"));
    for _ in 0..64 {
        if matches!(
            session.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Text { .. })
        ) {
            return session;
        }
        session
            .run(Command::MoveCaret(Direction::Backward))
            .unwrap_or_else(|error| panic!("无法进入公式 TextFlow：{error}"));
    }
    panic!(
        "混合长文档行内公式没有可编辑的数学 TextFlow：{:?}",
        session.math_edit().map(|edit| &edit.branch)
    );
}

fn math_drag_stops(
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> (
    caret_engine::PageMathTextDragStop,
    caret_engine::PageMathTextDragStop,
) {
    let stops = snapshot.math_text_drag_stops(0);
    for anchor in &stops {
        for focus in &stops {
            if anchor.token == focus.token
                && anchor.target == focus.target
                && anchor.order_utf8 < focus.order_utf8
            {
                return (anchor.clone(), focus.clone());
            }
        }
    }
    panic!("混合长文档公式未发布可拖选的数学文本停靠点");
}

fn math_drag_hits(
    snapshot: &caret_engine::DocumentLayoutSnapshot,
    anchor: &caret_engine::PageMathTextDragStop,
    focus: &caret_engine::PageMathTextDragStop,
) -> (caret_engine::PageMathTextHit, caret_engine::PageMathTextHit) {
    let hit = |stop: &caret_engine::PageMathTextDragStop| {
        snapshot
            .math_text_drag_hit(
                stop.caret.page_index,
                stop.token,
                &stop.target,
                &stop.position,
                stop.order_utf8,
                stop.visual_affinity,
            )
            .unwrap_or_else(|| panic!("数学文本拖选端点未通过快照校验"))
    };
    (hit(anchor), hit(focus))
}

fn benchmark_math_projection_and_text_edit(criterion: &mut Criterion) {
    let mut projection_group = criterion.benchmark_group("rust/math_projection");
    for text_size in TEXT_SIZES {
        let source = math_text_source(&"a".repeat(text_size));
        projection_group.throughput(Throughput::Bytes(source.len() as u64));
        projection_group.bench_function(format!("textflow_{text_size}_bytes"), |bencher| {
            bencher.iter_batched_ref(
                || source.clone(),
                |source| {
                    caret_math::parse_projection(
                        black_box(source.as_str()),
                        caret_math::MathInputKind::Equation,
                    )
                    .unwrap_or_else(|error| panic!("数学投影基准源码必须有效：{error}"))
                },
                BatchSize::SmallInput,
            )
        });
    }
    projection_group.finish();

    let mut edit_group = criterion.benchmark_group("rust/math_textflow_edit");
    for text_size in TEXT_SIZES {
        edit_group.bench_function(
            format!("insert_one_character_{text_size}_bytes"),
            |bencher| {
                bencher.iter_batched_ref(
                    || prepare_text_edit_session(text_size),
                    |session| {
                        black_box(
                            session
                                .run(Command::InsertText("x".to_owned()))
                                .unwrap_or_else(|error| {
                                    panic!("数学 TextFlow 编辑基准失败：{error}")
                                }),
                        )
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }
    edit_group.finish();

    benchmark_math_layout_and_document_edit(criterion);
    benchmark_structured_formula(criterion);
    benchmark_math_command_candidates(criterion);
}

fn benchmark_structured_formula(criterion: &mut Criterion) {
    let catalog = RefCell::new(
        FontCatalog::system()
            .unwrap_or_else(|error| panic!("无法初始化结构化公式基准字体目录：{error}")),
    );
    let mut group = criterion.benchmark_group("rust/math_structured_formula");
    group.bench_function("initial_4x4_nested_fraction_matrix_layout", |bencher| {
        bencher.iter_batched(
            || {
                (
                    prepare_structured_formula_session(),
                    DocumentLayoutService::new(),
                )
            },
            |(session, mut service)| {
                let base_style = ShapingStyle::new(
                    session
                        .document()
                        .global_settings
                        .default_font_size_centipoints,
                );
                black_box(
                    service
                        .layout_session(&session, &mut catalog.borrow_mut(), &base_style)
                        .unwrap_or_else(|error| panic!("结构化公式首次布局失败：{error}")),
                )
            },
            BatchSize::SmallInput,
        );
    });
    group.bench_function("textflow_edit_and_full_relayout", |bencher| {
        bencher.iter_batched(
            || {
                let session = prepare_structured_formula_session();
                let mut service = DocumentLayoutService::new();
                let base_style = ShapingStyle::new(
                    session
                        .document()
                        .global_settings
                        .default_font_size_centipoints,
                );
                service
                    .layout_session(&session, &mut catalog.borrow_mut(), &base_style)
                    .unwrap_or_else(|error| panic!("结构化公式编辑前布局失败：{error}"));
                (session, service, base_style)
            },
            |(mut session, mut service, base_style)| {
                session
                    .run(Command::InsertText("x".to_owned()))
                    .unwrap_or_else(|error| panic!("结构化公式 TextFlow 编辑失败：{error}"));
                black_box(
                    service
                        .layout_session(&session, &mut catalog.borrow_mut(), &base_style)
                        .unwrap_or_else(|error| panic!("结构化公式编辑后重排失败：{error}")),
                )
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn benchmark_math_command_candidates(criterion: &mut Criterion) {
    let document = mixed_long_document();
    let mut prose_session = mixed_session(&document);
    prose_session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("无法准备数学命令候选基准：{error}"));
    prose_session
        .run(Command::TypeCommandLine("math".to_owned()))
        .unwrap_or_else(|error| panic!("无法输入数学命令候选查询：{error}"));
    let candidates = prose_session.command_line_candidates();
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.alias == r"\math" && candidate.available),
        "长文档中的行内公式候选必须可用"
    );

    criterion.bench_function("rust/math_insert_candidate_query_100k_prose", |bencher| {
        bencher.iter(|| black_box(prose_session.command_line_candidates()));
    });

    let mut math_session = mixed_math_text_session(&document);
    math_session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("无法准备数学拆分候选基准：{error}"));
    math_session
        .run(Command::TypeCommandLine("math".to_owned()))
        .unwrap_or_else(|error| panic!("无法输入数学拆分候选查询：{error}"));
    let candidates = math_session.command_line_candidates();
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.alias == r"\math" && candidate.available),
        "长文档中的 Math TextFlow 拆分候选必须可用"
    );
    criterion.bench_function(
        "rust/math_split_inline_candidate_query_100k_prose",
        |bencher| {
            bencher.iter(|| black_box(math_session.command_line_candidates()));
        },
    );
}

fn benchmark_math_layout_and_document_edit(criterion: &mut Criterion) {
    let catalog = RefCell::new(
        FontCatalog::system()
            .unwrap_or_else(|error| panic!("无法初始化数学布局性能基准字体目录：{error}")),
    );
    let mut layout_group = criterion.benchmark_group("rust/math_document_layout");
    for text_size in TEXT_SIZES {
        layout_group.bench_function(format!("initial_equation_layout_{text_size}_bytes"), |b| {
            b.iter_batched(
                || {
                    (
                        prepare_text_edit_session(text_size),
                        DocumentLayoutService::new(),
                    )
                },
                |(session, mut service)| {
                    let base_style = ShapingStyle::new(
                        session
                            .document()
                            .global_settings
                            .default_font_size_centipoints,
                    );
                    let mut catalog = catalog.borrow_mut();
                    black_box(
                        service
                            .layout_session(&session, &mut catalog, &base_style)
                            .unwrap_or_else(|error| panic!("公式布局性能基准失败：{error}")),
                    )
                },
                BatchSize::SmallInput,
            )
        });

        layout_group.bench_function(
            format!("math_text_edit_and_relayout_{text_size}_bytes"),
            |b| {
                b.iter_batched(
                    || {
                        let session = prepare_text_edit_session(text_size);
                        let mut service = DocumentLayoutService::new();
                        let base_style = ShapingStyle::new(
                            session
                                .document()
                                .global_settings
                                .default_font_size_centipoints,
                        );
                        let mut catalog = catalog.borrow_mut();
                        let snapshot = service
                            .layout_session(&session, &mut catalog, &base_style)
                            .unwrap_or_else(|error| panic!("公式编辑基准初始布局失败：{error}"));
                        (session, service, snapshot, base_style)
                    },
                    |(mut session, mut service, _snapshot, base_style)| {
                        session
                            .run(Command::InsertText("x".to_owned()))
                            .unwrap_or_else(|error| panic!("公式编辑基准输入失败：{error}"));
                        let mut catalog = catalog.borrow_mut();
                        black_box(
                            service
                                .layout_session(&session, &mut catalog, &base_style)
                                .unwrap_or_else(|error| {
                                    panic!("公式编辑后的完整布局基准失败：{error}")
                                }),
                        )
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }
    layout_group.finish();
    benchmark_mixed_long_document(criterion);
}

fn benchmark_mixed_long_document(criterion: &mut Criterion) {
    let document = mixed_long_document();
    let math_host_paragraph_id = match &document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => panic!("混合长文档首节点必须为段落"),
    };
    let catalog = RefCell::new(
        FontCatalog::system()
            .unwrap_or_else(|error| panic!("无法读取混合公式基准字体目录：{error}")),
    );
    let base_style = ShapingStyle::new(1200);
    let service = RefCell::new(DocumentLayoutService::new());

    let mut group = criterion.benchmark_group("rust/math_mixed_document_100k");
    group.bench_function("initial_layout_and_snapshot_publish", |bencher| {
        bencher.iter_batched_ref(
            || mixed_session(&document),
            |session| {
                black_box(
                    service
                        .borrow_mut()
                        .layout_session(session, &mut catalog.borrow_mut(), &base_style)
                        .unwrap_or_else(|error| panic!("混合长文档快照发布失败：{error}")),
                )
            },
            BatchSize::SmallInput,
        );
    });
    group.bench_function(
        "math_textflow_edit_relayout_reusing_unchanged_paragraphs",
        |bencher| {
            bencher.iter_batched_ref(
                || {
                    let session = mixed_math_text_session(&document);
                    let baseline = service
                        .borrow_mut()
                        .layout_session(&session, &mut catalog.borrow_mut(), &base_style)
                        .unwrap_or_else(|error| panic!("数学 TextFlow 编辑前布局失败：{error}"));
                    (session, baseline)
                },
                |(session, baseline)| {
                    session
                        .run(Command::InsertText("x".to_owned()))
                        .unwrap_or_else(|error| panic!("长文档数学 TextFlow 编辑失败：{error}"));
                    black_box(
                        service
                            .borrow_mut()
                            .layout_session_reusing(
                                session,
                                baseline,
                                &math_host_paragraph_id,
                                &mut catalog.borrow_mut(),
                                &base_style,
                            )
                            .unwrap_or_else(|error| {
                                panic!("数学 TextFlow 缓存复用重排失败：{error}")
                            }),
                    )
                },
                BatchSize::SmallInput,
            );
        },
    );
    group.bench_function("math_textflow_edit_forced_full_relayout", |bencher| {
        bencher.iter_batched_ref(
            || {
                let session = mixed_math_text_session(&document);
                let baseline = service
                    .borrow_mut()
                    .layout_session(&session, &mut catalog.borrow_mut(), &base_style)
                    .unwrap_or_else(|error| panic!("数学 TextFlow 全量对照基线布局失败：{error}"));
                (session, baseline)
            },
            |(session, _baseline)| {
                session
                    .run(Command::InsertText("x".to_owned()))
                    .unwrap_or_else(|error| panic!("长文档数学 TextFlow 编辑失败：{error}"));
                black_box(
                    service
                        .borrow_mut()
                        .layout_session(session, &mut catalog.borrow_mut(), &base_style)
                        .unwrap_or_else(|error| panic!("数学 TextFlow 强制全量重排失败：{error}")),
                )
            },
            BatchSize::SmallInput,
        );
    });

    let preview_session = mixed_math_text_session(&document);
    let preview_snapshot = service
        .borrow_mut()
        .layout_session(&preview_session, &mut catalog.borrow_mut(), &base_style)
        .unwrap_or_else(|error| panic!("数学拖选预览基准布局失败：{error}"));
    let (anchor_stop, focus_stop) = math_drag_stops(&preview_snapshot);
    let (anchor_hit, focus_hit) = math_drag_hits(&preview_snapshot, &anchor_stop, &focus_stop);
    let preview_geometry = preview_snapshot
        .math_text_selection_geometry(
            &anchor_hit.target,
            &anchor_hit.position,
            &focus_hit.position,
        )
        .unwrap_or_else(|| panic!("数学拖选预览几何未生成"));
    assert!(!preview_geometry.is_empty(), "数学拖选预览必须产生可见几何");
    group.bench_function("math_text_drag_preview_hit_and_geometry", |bencher| {
        bencher.iter(|| {
            let (anchor, focus) = math_drag_hits(&preview_snapshot, &anchor_stop, &focus_stop);
            black_box(preview_snapshot.math_text_selection_geometry(
                &anchor.target,
                &anchor.position,
                &focus.position,
            ))
        });
    });
    group.bench_function("math_text_drag_selection_commit", |bencher| {
        bencher.iter_batched_ref(
            || {
                let session = mixed_math_text_session(&document);
                let snapshot = service
                    .borrow_mut()
                    .layout_session(&session, &mut catalog.borrow_mut(), &base_style)
                    .unwrap_or_else(|error| panic!("数学拖选提交基准布局失败：{error}"));
                let (anchor, focus) = math_drag_stops(&snapshot);
                let (anchor, focus) = math_drag_hits(&snapshot, &anchor, &focus);
                (session, snapshot, anchor, focus)
            },
            |(session, _snapshot, anchor, focus)| {
                black_box(
                    session
                        .set_math_text_selection_from_hits(anchor, focus, anchor.font_generation)
                        .unwrap_or_else(|error| panic!("数学拖选提交失败：{error}")),
                )
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, benchmark_math_projection_and_text_edit);
criterion_main!(benches);
