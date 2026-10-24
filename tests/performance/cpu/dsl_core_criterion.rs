use caret_dsl_core::{format_source, parse};
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

const SOURCE: &str = include_str!("../../../tests/fixtures/dsl/artifacts.pa");

fn benchmark_dsl_core(criterion: &mut Criterion) {
    let parsed = match parse(SOURCE) {
        Ok(document) => document,
        Err(error) => panic!("benchmark fixture must parse: {error}"),
    };

    criterion.bench_function("dsl/parse_artifacts_fixture", |bencher| {
        bencher.iter(|| parse(black_box(SOURCE)))
    });
    criterion.bench_function("dsl/format_source_artifacts_fixture", |bencher| {
        bencher.iter(|| format_source(black_box(SOURCE)))
    });
    criterion.bench_function("dsl/format_parsed_artifacts_document", |bencher| {
        bencher.iter(|| caret_dsl_core::format_document(black_box(&parsed)))
    });
}

criterion_group!(benches, benchmark_dsl_core);
criterion_main!(benches);
