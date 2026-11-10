use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use oxc_sourcemap::{ConcatSourceMapBuilder, SourceMap};

mod common;

fn token_line_span(sm: &SourceMap) -> u32 {
    sm.get_tokens().last().map_or(1, |token| token.get_dst_line() + 1)
}

pub fn bench(c: &mut Criterion) {
    let fixtures = common::load_perf_fixtures();

    let sourcemaps: Vec<_> = fixtures
        .iter()
        .map(|fixture| {
            SourceMap::from_json_string(&fixture.json)
                .unwrap_or_else(|err| panic!("invalid perf fixture {}: {err}", fixture.name))
        })
        .collect();

    let mut line_offset = 0u32;
    let mut concat_inputs = Vec::with_capacity(sourcemaps.len());
    for sourcemap in &sourcemaps {
        concat_inputs.push((sourcemap, line_offset));
        line_offset = line_offset.saturating_add(token_line_span(sourcemap));
    }

    let mut concat_group = c.benchmark_group("concat");
    concat_group.throughput(Throughput::Bytes(
        fixtures.iter().map(|fixture| fixture.json.len() as u64).sum(),
    ));
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

criterion_group!(sourcemap, bench);
criterion_main!(sourcemap);
