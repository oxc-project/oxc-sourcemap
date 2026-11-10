#[cfg(feature = "codspeed")]
mod allocator;

pub struct Fixture {
    pub name: String,
    pub json: String,
}

pub fn load_perf_fixtures() -> Vec<Fixture> {
    let mut fixtures: Vec<_> = [
        ("real_large", include_str!("../../tests/fixtures/perf/real_large.map")),
        ("real_medium", include_str!("../../tests/fixtures/perf/real_medium.map")),
        ("real_small", include_str!("../../tests/fixtures/perf/real_small.map")),
    ]
    .into_iter()
    .map(|(name, json)| Fixture { name: name.into(), json: json.into() })
    .collect();
    let json = synthesize_xlarge(&fixtures[0].json, 40);
    fixtures.push(Fixture { name: "real_xlarge".into(), json });
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
