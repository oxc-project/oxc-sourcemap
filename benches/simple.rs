use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use std::borrow::Cow;

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use oxc_sourcemap::{ConcatSourceMapBuilder, SourceMap, SourceMapBuilder, Token};

#[derive(Debug, Clone)]
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

    let mut fixtures: Vec<Fixture> = fixture_paths
        .into_iter()
        .map(|path| {
            let json = fs::read_to_string(&path).unwrap_or_else(|err| {
                panic!("failed to read fixture {}: {err}", path.display());
            });
            let name =
                path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("unnamed").to_string();
            Fixture { name, json }
        })
        .collect();

    // The on-disk fixtures top out at ~3 KB which leaves benchmarks dominated
    // by per-iteration overhead. Append a synthesized "real_xlarge" by
    // concatenating the largest fixture's source/mapping data so we exercise
    // the parser on a workload closer to real bundler output.
    if let Some(large) = fixtures.iter().find(|f| f.name == "real_large").cloned() {
        const REPEATS: usize = 40;
        let xlarge_json = synthesize_xlarge(&large.json, REPEATS);
        fixtures.push(Fixture { name: "real_xlarge".to_string(), json: xlarge_json });
    }

    fixtures
}

fn synthesize_xlarge(base_json: &str, repeats: usize) -> String {
    let base: serde_json::Value =
        serde_json::from_str(base_json).expect("base fixture must be valid JSON");
    let base = base.as_object().expect("base fixture must be an object");

    let base_sources = base["sources"].as_array().unwrap();
    let base_sources_content = base["sourcesContent"].as_array().unwrap();
    let base_names = base["names"].as_array().unwrap();
    let base_mappings = base["mappings"].as_str().unwrap();

    // Mappings are delta-encoded; repeating them naively makes the cumulative
    // name_id / source_id grow with each chunk. Inflate names / sources /
    // sources_content arrays accordingly so the bigger deltas remain valid.
    let mut sources = Vec::with_capacity(base_sources.len() * repeats);
    let mut sources_content = Vec::with_capacity(base_sources_content.len() * repeats);
    let mut names = Vec::with_capacity(base_names.len() * repeats);
    for chunk in 0..repeats {
        for (i, src) in base_sources.iter().enumerate() {
            sources.push(serde_json::Value::String(format!(
                "chunk_{chunk}/{}",
                src.as_str().unwrap_or("source.js")
            )));
            sources_content
                .push(base_sources_content.get(i).cloned().unwrap_or(serde_json::Value::Null));
        }
        for (i, n) in base_names.iter().enumerate() {
            names.push(serde_json::Value::String(format!(
                "c{chunk}_{}",
                n.as_str().unwrap_or(&format!("name_{i}"))
            )));
        }
    }

    let mut mappings = String::with_capacity(base_mappings.len() * repeats + repeats);
    for chunk in 0..repeats {
        if chunk > 0 {
            mappings.push(';');
        }
        mappings.push_str(base_mappings);
    }

    let obj = serde_json::json!({
        "version": 3,
        "sources": sources,
        "sourcesContent": sources_content,
        "names": names,
        "mappings": mappings,
    });
    serde_json::to_string(&obj).unwrap()
}

/// Synthesize a chunk's worth of owned module sourcemaps, each carrying its full source text as
/// `sourcesContent` — modelling rolldown's `SourceJoiner` inputs (the dominant concat workload).
fn make_owned_module_maps(
    count: usize,
    content_bytes: usize,
    tokens_per_map: u32,
) -> Vec<(SourceMap<'static>, u32)> {
    let line = "const value = computeSomething(alpha, beta, gamma); // source line\n";
    let mut content = String::with_capacity(content_bytes + line.len());
    while content.len() < content_bytes {
        content.push_str(line);
    }

    let mut maps = Vec::with_capacity(count);
    let mut line_offset = 0u32;
    for i in 0..count {
        let names = vec![Cow::Owned(format!("ident_a_{i}")), Cow::Owned(format!("ident_b_{i}"))];
        let sources = vec![Cow::Owned(format!("src/module_{i}.js"))];
        let source_contents = vec![Some(Cow::Owned(content.clone()))];
        let tokens: Vec<Token> =
            (0..tokens_per_map).map(|t| Token::new(t, t * 2, t, t, Some(0), Some(t % 2))).collect();
        let map = SourceMap::new(
            None,
            names,
            None,
            sources,
            source_contents,
            tokens.into_boxed_slice(),
            None,
        );
        maps.push((map, line_offset));
        line_offset += tokens_per_map + 1;
    }
    maps
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

    // A single representative concat workload: a chunk of 200 modules, each carrying ~2 KB of
    // `sourcesContent`, concatenated into one owned `SourceMap` (rolldown's `SourceJoiner` shape).
    let owned_maps = make_owned_module_maps(200, 2048, 100);
    let content_bytes: u64 = owned_maps
        .iter()
        .map(|(map, _)| map.get_source_contents().flatten().map(str::len).sum::<usize>())
        .sum::<usize>() as u64;
    let concat_inputs: Vec<(&SourceMap, u32)> =
        owned_maps.iter().map(|(map, offset)| (map, *offset)).collect();

    let mut concat_group = c.benchmark_group("concat");
    concat_group.throughput(Throughput::Bytes(content_bytes));
    // Bulk constructor.
    concat_group.bench_function("ConcatSourceMapBuilder::from_sourcemaps", |b| {
        b.iter(|| {
            let concat_sm = ConcatSourceMapBuilder::from_sourcemaps(black_box(&concat_inputs))
                .into_owned_sourcemap();
            black_box(concat_sm);
        });
    });
    // Incremental adder (rolldown's `SourceJoiner` calls this per source).
    concat_group.bench_function("ConcatSourceMapBuilder::add_sourcemap", |b| {
        b.iter(|| {
            let mut builder = ConcatSourceMapBuilder::default();
            for &(map, offset) in black_box(&concat_inputs) {
                builder.add_sourcemap(map, offset);
            }
            black_box(builder.into_owned_sourcemap());
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
