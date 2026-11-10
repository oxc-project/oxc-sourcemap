use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(feature = "codspeed")]
mod allocator;

#[derive(Debug, Clone)]
pub struct Fixture {
    pub name: String,
    pub json: String,
}

impl Fixture {
    pub fn bytes(&self) -> u64 {
        self.json.len() as u64
    }
}

pub fn load_perf_fixtures() -> Vec<Fixture> {
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
