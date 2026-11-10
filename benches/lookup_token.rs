use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use oxc_sourcemap::SourceMap;

mod common;

pub fn bench(c: &mut Criterion) {
    let fixtures = common::load_perf_fixtures();

    // Probe every generated position with the lookup table already built.
    let mut token_lookup_group = c.benchmark_group("lookup_token");
    for fixture in &fixtures {
        let sourcemap = SourceMap::from_json_string(&fixture.json)
            .unwrap_or_else(|err| panic!("invalid perf fixture {}: {err}", fixture.name));
        let table = sourcemap.generate_lookup_table();
        let keys: Vec<(u32, u32)> =
            sourcemap.get_tokens().map(|t| (t.get_dst_line(), t.get_dst_col())).collect();
        token_lookup_group.throughput(Throughput::Elements(keys.len() as u64));
        token_lookup_group.bench_with_input(
            BenchmarkId::from_parameter(&fixture.name),
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

criterion_group!(sourcemap, bench);
criterion_main!(sourcemap);
