use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use oxc_sourcemap::SourceMap;

mod common;

pub fn bench(c: &mut Criterion) {
    let fixtures = common::load_perf_fixtures();

    let mut lookup_group = c.benchmark_group("lookup_table");
    for fixture in &fixtures {
        let sourcemap = SourceMap::from_json_string(&fixture.json)
            .unwrap_or_else(|err| panic!("invalid perf fixture {}: {err}", fixture.name));
        lookup_group.throughput(Throughput::Bytes(fixture.json.len() as u64));
        lookup_group.bench_with_input(
            BenchmarkId::from_parameter(&fixture.name),
            &sourcemap,
            |b, sourcemap| {
                b.iter(|| {
                    let table = black_box(sourcemap).generate_lookup_table();
                    black_box(table);
                });
            },
        );
    }
    lookup_group.finish();
}

criterion_group!(sourcemap, bench);
criterion_main!(sourcemap);
