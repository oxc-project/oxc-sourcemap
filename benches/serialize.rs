use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use oxc_sourcemap::SourceMap;

mod common;

pub fn bench(c: &mut Criterion) {
    let fixtures = common::load_perf_fixtures();

    let mut serialize_group = c.benchmark_group("serialize");
    for fixture in &fixtures {
        let sourcemap = SourceMap::from_json_string(&fixture.json)
            .unwrap_or_else(|err| panic!("invalid perf fixture {}: {err}", fixture.name));
        serialize_group.throughput(Throughput::Bytes(fixture.json.len() as u64));
        serialize_group.bench_with_input(
            BenchmarkId::from_parameter(&fixture.name),
            &sourcemap,
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

criterion_group!(sourcemap, bench);
criterion_main!(sourcemap);
