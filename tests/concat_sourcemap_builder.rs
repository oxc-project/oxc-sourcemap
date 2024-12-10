use std::fs;

use oxc_sourcemap::{ConcatSourceMapBuilder, SourceMap, SourcemapVisualizer};

#[test]
fn concat_sourcemap_builder_basic() {
    let dir = std::path::Path::new(file!())
        .parent()
        .unwrap()
        .join("fixtures_concat_sourcemap_builder/basic");

    let mut builder = ConcatSourceMapBuilder::default();
    let mut source = String::new();
    {
        let js = fs::read_to_string(dir.join("dep1.js")).unwrap();
        let js_map = fs::read_to_string(dir.join("dep1.js.map")).unwrap();
        let sourcemap = SourceMap::from_json_string(&js_map).unwrap();
        builder.add_sourcemap(&sourcemap, source.lines().count() as u32);
        source.push_str(&js);
    }
    {
        let js = fs::read_to_string(dir.join("dep2.js")).unwrap();
        let js_map = fs::read_to_string(dir.join("dep2.js.map")).unwrap();
        let sourcemap = SourceMap::from_json_string(&js_map).unwrap();
        builder.add_sourcemap(&sourcemap, source.lines().count() as u32);
        source.push_str(&js);
    }
    {
        let js = fs::read_to_string(dir.join("dep3.js")).unwrap();
        let js_map = fs::read_to_string(dir.join("dep3.js.map")).unwrap();
        let sourcemap = SourceMap::from_json_string(&js_map).unwrap();
        builder.add_sourcemap(&sourcemap, source.lines().count() as u32);
        source.push_str(&js);
    }

    let sourcemap = builder.into_sourcemap();
    // encode and decode to test token chunk serialization
    let sourcemap = SourceMap::from_json(sourcemap.to_json()).unwrap();
    let visualizer = SourcemapVisualizer::new(&source, &sourcemap);
    let visualizer_text = visualizer.into_visualizer_text();
    insta::assert_snapshot!("concat_sourcemap_builder_basic", visualizer_text);
}
