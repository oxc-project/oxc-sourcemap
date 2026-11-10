use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use oxc_sourcemap::SourceMap;

mod common;

pub fn bench(c: &mut Criterion) {
    let fixtures = common::load_perf_fixtures();

    // Keep `fixtures` alive: parsed `SourceMap`s borrow from each fixture's
    // JSON string, so we can't consume `fixtures` with `into_iter()`.
    let parsed_fixtures: Vec<(&str, u64, SourceMap<'_>)> = fixtures
        .iter()
        .map(|fixture| {
            let bytes = fixture.bytes();
            let sourcemap = SourceMap::from_json_string(&fixture.json)
                .unwrap_or_else(|err| panic!("invalid perf fixture {}: {err}", fixture.name));
            (fixture.name.as_str(), bytes, sourcemap)
        })
        .collect();

    let mut serialize_group = c.benchmark_group("serialize");
    for (name, bytes, sourcemap) in &parsed_fixtures {
        serialize_group.throughput(Throughput::Bytes(*bytes));
        serialize_group.bench_with_input(
            BenchmarkId::from_parameter(name),
            sourcemap,
            |b, sourcemap| {
                b.iter(|| {
                    let encoded = black_box(sourcemap).to_json_string();
                    black_box(encoded);
                });
            },
        );
    }
    serialize_group.finish();
}

criterion_group!(
    name = sourcemap;
    config = Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = bench
);
criterion_main!(sourcemap);
