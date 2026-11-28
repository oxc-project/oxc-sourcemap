use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use oxc_sourcemap::{PackedConcatSourceMapBuilder, PackedSourceMap, SourceMap};

mod common;

pub fn bench(c: &mut Criterion) {
    let fixtures = common::load_perf_fixtures();
    let packed: Vec<_> = fixtures
        .iter()
        .map(|fixture| {
            PackedSourceMap::from_json_string(&fixture.json)
                .unwrap_or_else(|err| panic!("invalid perf fixture {}: {err}", fixture.name))
        })
        .collect();

    let mut group = c.benchmark_group("packed");
    for (fixture, packed) in fixtures.iter().zip(&packed) {
        group.throughput(Throughput::Bytes(fixture.json.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("parse", &fixture.name),
            &fixture.json,
            |b, json| {
                b.iter(|| black_box(PackedSourceMap::from_json_string(black_box(json)).unwrap()));
            },
        );
        group.bench_with_input(BenchmarkId::new("serialize", &fixture.name), packed, |b, map| {
            b.iter(|| black_box(black_box(map).to_json_string()));
        });
        group.bench_with_input(BenchmarkId::new("unpack", &fixture.name), packed, |b, map| {
            b.iter(|| black_box(black_box(map).unpack()));
        });
        let map = SourceMap::from_json_string(&fixture.json).unwrap();
        group.bench_with_input(BenchmarkId::new("pack", &fixture.name), &map, |b, map| {
            b.iter_batched(
                || map.clone(),
                |map| black_box(map.into_packed()),
                criterion::BatchSize::LargeInput,
            );
        });
    }

    let mut line_offset = 0u32;
    let mut inputs = Vec::with_capacity(packed.len());
    for map in &packed {
        inputs.push((map, line_offset));
        let lines = map.mappings().last().map_or(1, |token| token.get_dst_line() + 1);
        line_offset = line_offset.saturating_add(lines);
    }
    group.throughput(Throughput::Bytes(fixtures.iter().map(|f| f.json.len() as u64).sum()));
    group.bench_function("concat", |b| {
        b.iter(|| {
            black_box(PackedConcatSourceMapBuilder::from_sourcemaps(black_box(&inputs)))
                .into_sourcemap()
        });
    });
    group.finish();
}

criterion_group!(sourcemap, bench);
criterion_main!(sourcemap);
