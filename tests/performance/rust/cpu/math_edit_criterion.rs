//! 测量数学源码投影与 Math TextFlow 单字符编辑的 CPU 基线。

use caret_doc::{Block, Document, Equation, NodeId};
use caret_engine::{Command, Direction, MathEditBranch, Session};
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

const TEXT_SIZES: [usize; 2] = [256, 4_096];

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
}

criterion_group!(benches, benchmark_math_projection_and_text_edit);
criterion_main!(benches);
