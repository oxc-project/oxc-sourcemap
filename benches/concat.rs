use std::time::Duration;

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use oxc_sourcemap::{ConcatSourceMapBuilder, SourceMap};

mod common;

fn token_line_span(sm: &SourceMap) -> u32 {
    sm.get_tokens().last().map_or(1, |token| token.get_dst_line() + 1)
}

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

    let mut line_offset = 0u32;
    let mut concat_input_size = 0u64;
    let mut concat_inputs = Vec::with_capacity(parsed_fixtures.len());
    for (_, bytes, sourcemap) in &parsed_fixtures {
        concat_inputs.push((sourcemap, line_offset));
        line_offset = line_offset.saturating_add(token_line_span(sourcemap));
        concat_input_size += *bytes;
    }

    let mut concat_group = c.benchmark_group("concat");
    concat_group.throughput(Throughput::Bytes(concat_input_size));
    concat_group.bench_function("from_sourcemaps", |b| {
        b.iter(|| {
            let concat_sm =
                ConcatSourceMapBuilder::from_sourcemaps(black_box(&concat_inputs)).into_sourcemap();
            black_box(concat_sm);
        });
    });
    concat_group.bench_function("add_sourcemap_loop", |b| {
        b.iter(|| {
            let mut builder = ConcatSourceMapBuilder::default();
            for &(sourcemap, line_offset) in black_box(&concat_inputs) {
                builder.add_sourcemap(sourcemap, line_offset);
            }
            let concat_sm = builder.into_sourcemap();
            black_box(concat_sm);
        });
    });
    concat_group.finish();
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
