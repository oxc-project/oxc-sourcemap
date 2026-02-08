use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use oxc_sourcemap::{ConcatSourceMapBuilder, SourceMap, SourceMapBuilder};

#[derive(Debug)]
struct Fixture {
    name: String,
    json: String,
}

impl Fixture {
    fn bytes(&self) -> u64 {
        self.json.len() as u64
    }
}

fn load_perf_fixtures() -> Vec<Fixture> {
    let perf_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/perf");
    let entries = fs::read_dir(&perf_dir).unwrap_or_else(|err| {
        panic!("failed to read perf fixtures at {}: {err}", perf_dir.display());
    });

    let mut fixture_paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("map"))
        .collect::<Vec<PathBuf>>();
    fixture_paths.sort_unstable();
    assert!(!fixture_paths.is_empty(), "no perf fixtures found at {}", perf_dir.display());

    fixture_paths
        .into_iter()
        .map(|path| {
            let json = fs::read_to_string(&path).unwrap_or_else(|err| {
                panic!("failed to read fixture {}: {err}", path.display());
            });
            let name =
                path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("unnamed").to_string();
            Fixture { name, json }
        })
        .collect()
}

fn token_line_span(sm: &SourceMap) -> u32 {
    sm.get_tokens().last().map_or(1, |token| token.get_dst_line() + 1)
}

pub fn bench(c: &mut Criterion) {
    let smoke_input = r#"{
        "version": 3,
        "sources": ["coolstuff.js"],
        "sourceRoot": "x",
        "names": ["x","alert"],
        "mappings": "AAAA,GAAIA,GAAI,EACR,IAAIA,GAAK,EAAG,CACVC,MAAM",
        "x_google_ignoreList": [0],
        "sourcesContent": ["var x = 1;\nif (x == 2) {\n  alert('test');\n}"]
    }"#;
    c.bench_function("smoke/SourceMap::from_json_string_inline", |b| {
        b.iter(|| {
            let parsed = SourceMap::from_json_string(black_box(smoke_input))
                .expect("inline fixture should parse");
            black_box(parsed);
        });
    });

    let fixtures = load_perf_fixtures();

    let mut parse_group = c.benchmark_group("parse");
    for fixture in &fixtures {
        parse_group.throughput(Throughput::Bytes(fixture.bytes()));
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

    let parsed_fixtures = fixtures
        .into_iter()
        .map(|fixture| {
            let bytes = fixture.bytes();
            let sourcemap = SourceMap::from_json_string(&fixture.json)
                .unwrap_or_else(|err| panic!("invalid perf fixture {}: {err}", fixture.name));
            (fixture.name, bytes, sourcemap)
        })
        .collect::<Vec<_>>();

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

    let mut lookup_group = c.benchmark_group("lookup_table");
    for (name, bytes, sourcemap) in &parsed_fixtures {
        lookup_group.throughput(Throughput::Bytes(*bytes));
        lookup_group.bench_with_input(
            BenchmarkId::from_parameter(name),
            sourcemap,
            |b, sourcemap| {
                b.iter(|| {
                    let table = black_box(sourcemap).generate_lookup_table();
                    black_box(table);
                });
            },
        );
    }
    lookup_group.finish();

    c.bench_function("builder/SourceMapBuilder::build_single", |b| {
        b.iter(|| {
            let mut builder = SourceMapBuilder::default();
            let name_id = builder.add_name(black_box("foo"));
            let source_id =
                builder.add_source_and_content(black_box("test.js"), black_box("var x = 1;"));
            builder.add_token(0, 0, 0, 0, Some(source_id), Some(name_id));
            let sourcemap = builder.into_sourcemap();
            black_box(sourcemap);
        });
    });

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
