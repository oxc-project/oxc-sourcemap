use criterion::{Criterion, criterion_group, criterion_main};
use oxc_sourcemap::{SourceMap, SourceMapBuilder, escape_json_string, escape_json_string_fallback};

pub fn bench_json_escaping(c: &mut Criterion) {
    let long_clean = "abcdefghijklmnopqrstuvwxyz".repeat(50);
    let long_quotes = "\"test\"".repeat(100);

    let test_strings = vec![
        ("short_clean", "simple string without escapes"),
        ("short_quotes", "string with \"quotes\" and \\backslashes"),
        ("short_control", "string with\ncontrol\tchars\r"),
        ("long_clean", long_clean.as_str()), // Long string without escapes
        ("long_quotes", long_quotes.as_str()), // Many escapes
        ("mixed", "mixed: \"quotes\", \\backslashes, \ncontrol\tchars, and regular text"),
    ];

    for (name, test_str) in test_strings {
        c.bench_function(&format!("escape_fallback_{}", name), |b| {
            b.iter(|| escape_json_string_fallback(test_str));
        });

        c.bench_function(&format!("escape_avx512_{}", name), |b| {
            b.iter(|| escape_json_string(test_str));
        });
    }
}

pub fn bench(c: &mut Criterion) {
    let input = r#"{
        "version": 3,
        "sources": ["coolstuff.js"],
        "sourceRoot": "x",
        "names": ["x","alert"],
        "mappings": "AAAA,GAAIA,GAAI,EACR,IAAIA,GAAK,EAAG,CACVC,MAAM",
        "x_google_ignoreList": [0],
        "sourcesContent": ["var x = 1;\nif (x == 2) {\n  alert('test');\n}"]
    }"#;

    c.bench_function("SourceMap::from_json_string", |b| {
        b.iter(|| SourceMap::from_json_string(input).unwrap());
    });

    c.bench_function("SourceMap::to_json", |b| {
        let sm = SourceMap::from_json_string(input).unwrap();
        b.iter(|| sm.to_json());
    });

    c.bench_function("SourceMap::to_json_string", |b| {
        let sm = SourceMap::from_json_string(input).unwrap();
        b.iter(|| sm.to_json_string());
    });

    c.bench_function("SourceMapBuilder::add_name_add_source_and_content", |b| {
        let mut builder = SourceMapBuilder::default();
        b.iter(|| {
            builder.add_name("foo");
            builder.add_source_and_content("test.js", "var x = 1;");
        });
    });
}

criterion_group!(sourcemap, bench, bench_json_escaping);
criterion_main!(sourcemap);
