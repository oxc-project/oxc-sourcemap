// Tight loop around the parse hot path so a sampling profiler
// (xcrun xctrace, Instruments, perf, etc.) gets stable hits inside
// the decoder rather than the harness.
//
// Build the bench-profile binary:
//   cargo build --release --example profile_parse
// Then profile:
//   xcrun xctrace record --template 'Time Profile' --output trace.trace \
//       --launch -- target/release/examples/profile_parse
#![allow(clippy::print_stdout, clippy::disallowed_methods)]

use std::{fs, path::Path};

use oxc_sourcemap::{ConcatSourceMapBuilder, SourceMap};

fn main() {
    let perf_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/perf");
    let large =
        fs::read_to_string(perf_dir.join("real_large.map")).expect("read real_large fixture");

    // Match `benches/simple.rs`'s `real_xlarge`: 40 copies of the large
    // fixture, names/sources inflated to keep VLQ deltas valid.
    let xlarge_json = synth_xlarge(&large, 40);

    // Warm up before the long-running loop so JITs / page mappings settle.
    for _ in 0..50 {
        let _ = SourceMap::from_json_string(&xlarge_json).unwrap();
    }

    let mut total_tokens: u64 = 0;
    let iters = std::env::args().nth(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(2000);

    for _ in 0..iters {
        let sm = SourceMap::from_json_string(&xlarge_json).unwrap();
        // Force the result to escape so the parse can't be DCE'd.
        total_tokens = total_tokens.wrapping_add(sm.get_tokens().count() as u64);
        let serialized = sm.to_json_string();
        total_tokens = total_tokens.wrapping_add(serialized.len() as u64);
        let cat = ConcatSourceMapBuilder::from_sourcemaps(&[(&sm, 0), (&sm, 10000)]);
        let cat_sm = cat.into_sourcemap();
        total_tokens = total_tokens.wrapping_add(cat_sm.get_tokens().count() as u64);
    }

    // Print so the optimizer can't dead-strip the loop body.
    println!("done, total_tokens={total_tokens}, iters={iters}");
}

fn synth_xlarge(base_json: &str, repeats: usize) -> String {
    let base: serde_json::Value = serde_json::from_str(base_json).expect("base fixture json");
    let base = base.as_object().expect("base fixture object");
    let base_sources = base["sources"].as_array().unwrap();
    let base_sources_content = base["sourcesContent"].as_array().unwrap();
    let base_names = base["names"].as_array().unwrap();
    let base_mappings = base["mappings"].as_str().unwrap();

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
    serde_json::to_string(&serde_json::json!({
        "version": 3,
        "sources": sources,
        "sourcesContent": sources_content,
        "names": names,
        "mappings": mappings,
    }))
    .unwrap()
}
