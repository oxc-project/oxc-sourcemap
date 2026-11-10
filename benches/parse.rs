use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use oxc_sourcemap::SourceMap;

mod common;

pub fn bench(c: &mut Criterion) {
    let smoke_input = r#"{
        "version": 3,
        "sources": ["coolstuff.js"],
        "sourceRoot": "x",
        "names": ["x","alert"],
        "mappings": "AAAA,GAAIA,GAAI,EACR,IAAIA,GAAK,EAAG,CACVC,MAAM",
        "ignoreList": [0],
        "sourcesContent": ["var x = 1;\nif (x == 2) {\n  alert('test');\n}"]
    }"#;
    c.bench_function("smoke/SourceMap::from_json_string_inline", |b| {
        b.iter(|| {
            let parsed = SourceMap::from_json_string(black_box(smoke_input))
                .expect("inline fixture should parse");
            black_box(parsed);
        });
    });

    let fixtures = common::load_perf_fixtures();

    let mut parse_group = c.benchmark_group("parse");
    for fixture in &fixtures {
        parse_group.throughput(Throughput::Bytes(fixture.json.len() as u64));
        parse_group.bench_with_input(
            BenchmarkId::from_parameter(&fixture.name),
            fixture,
            |b, fixture| {
                b.iter(|| {
                    let parsed = SourceMap::from_json_string(black_box(&fixture.json))
                        .unwrap_or_else(|err| {
                            panic!("failed to parse fixture {}: {err}", fixture.name)
                        });
                    black_box(parsed);
                });
            },
        );
    }
    parse_group.finish();
}

criterion_group!(sourcemap, bench);
criterion_main!(sourcemap);
