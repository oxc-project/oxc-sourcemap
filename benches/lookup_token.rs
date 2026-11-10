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

    // Exercise the `lookup_token` remapping hot path: with the lookup table already
    // built, probe every token's generated position. This mirrors how bundlers
    // collapse a sourcemap chain (one lookup per token, per chain link). Throughput
    // is measured in lookups (elements), not bytes.
    let mut token_lookup_group = c.benchmark_group("lookup_token");
    for (name, _bytes, sourcemap) in &parsed_fixtures {
        let table = sourcemap.generate_lookup_table();
        let keys: Vec<(u32, u32)> =
            sourcemap.get_tokens().map(|t| (t.get_dst_line(), t.get_dst_col())).collect();
        token_lookup_group.throughput(Throughput::Elements(keys.len() as u64));
        token_lookup_group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &(table, keys),
            |b, (table, keys)| {
                b.iter(|| {
                    let table = black_box(table.as_slice());
                    let keys = black_box(keys.as_slice());
                    for &(line, col) in keys {
                        black_box(sourcemap.lookup_token(table, line, col));
                    }
                });
            },
        );
    }
    token_lookup_group.finish();
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
