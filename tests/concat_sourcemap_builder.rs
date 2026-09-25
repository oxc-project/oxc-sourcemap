use std::fs;

use oxc_sourcemap::{ConcatSourceMapBuilder, SourceMap, SourcemapVisualizer};

#[test]
fn concat_sourcemap_builder_with_empty() {
    let dir = std::path::Path::new(file!())
        .parent()
        .unwrap()
        .join("fixtures_concat_sourcemap_builder/empty");

    // SourceMap borrows from its JSON input, so collect all js + js.map
    // strings into Vecs first to keep them alive for the whole builder
    // lifetime.
    let filenames = ["dep1.js", "dep2.js", "dep3.js"];
    let js_inputs: Vec<String> =
        filenames.iter().map(|f| fs::read_to_string(dir.join(f)).unwrap()).collect();
    let map_inputs: Vec<String> = filenames
        .iter()
        .map(|f| fs::read_to_string(dir.join(f).with_added_extension("map")).unwrap())
        .collect();
    let sourcemaps: Vec<SourceMap<'_>> =
        map_inputs.iter().map(|m| SourceMap::from_json_string(m).unwrap()).collect();

    let mut builder = ConcatSourceMapBuilder::default();
    let mut source = String::new();

    // dep2.js.map has { mappings: "" }
    for (sourcemap, js) in sourcemaps.iter().zip(&js_inputs) {
        builder.add_sourcemap(sourcemap, source.lines().count() as u32);
        source.push_str(js);
    }

    let sourcemap = builder.into_sourcemap();
    // encode and decode back to test token chunk serialization
    let encoded = sourcemap.to_json_string();
    let sourcemap = SourceMap::from_json_string(&encoded).unwrap();

    let visualizer_text = SourcemapVisualizer::new(&source, &sourcemap).get_text();
    insta::assert_snapshot!("empty", visualizer_text);
}
