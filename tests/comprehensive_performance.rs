use std::time::Instant;
use oxc_sourcemap::{SourceMap, SourceMapBuilder, ConcatSourceMapBuilder};

#[test]
fn comprehensive_performance_test() {
    // Test data - a realistic sourcemap
    let input = r#"{
        "version": 3,
        "sources": ["coolstuff.js"],
        "sourceRoot": "x",
        "names": ["x","alert"],
        "mappings": "AAAA,GAAIA,GAAI,EACR,IAAIA,GAAK,EAAG,CACVC,MAAM",
        "x_google_ignoreList": [0],
        "sourcesContent": ["var x = 1;\nif (x == 2) {\n  alert('test');\n}"]
    }"#;

    println!("=== Comprehensive Performance Test ===");
    
    // Test 1: Decoding performance
    let start = Instant::now();
    let mut sourcemaps = Vec::with_capacity(1000);
    for _ in 0..1000 {
        let sm = SourceMap::from_json_string(input).unwrap();
        sourcemaps.push(sm);
    }
    let decode_time = start.elapsed();
    println!("Decode 1000 sourcemaps: {:?}", decode_time);
    
    // Test 2: Encoding performance  
    let sm = &sourcemaps[0];
    let start = Instant::now();
    for _ in 0..1000 {
        let _json = sm.to_json_string();
    }
    let encode_time = start.elapsed();
    println!("Encode 1000 sourcemaps: {:?}", encode_time);
    
    // Test 3: Builder performance
    let start = Instant::now();
    for _ in 0..1000 {
        let mut builder = SourceMapBuilder::default();
        builder.add_name("x");
        builder.add_name("alert");
        builder.add_name("x"); // Duplicate to test HashMap efficiency
        builder.add_source_and_content("test.js", "var x = 1;");
        let _sm = builder.into_sourcemap();
    }
    let builder_time = start.elapsed();
    println!("Build 1000 sourcemaps with builder: {:?}", builder_time);
    
    // Test 4: Concat performance
    let start = Instant::now();
    for _ in 0..100 {
        let builder = ConcatSourceMapBuilder::from_sourcemaps(&[
            (&sourcemaps[0], 0),
            (&sourcemaps[1], 100),
        ]);
        let _sm = builder.into_sourcemap();
    }
    let concat_time = start.elapsed();
    println!("Concat 100 pairs of sourcemaps: {:?}", concat_time);
    
    // Test 5: Lookup table generation and lookup performance
    let start = Instant::now();
    for _ in 0..100 {
        let _lookup_table = sm.generate_lookup_table();
    }
    let lookup_gen_time = start.elapsed();
    println!("Generate 100 lookup tables: {:?}", lookup_gen_time);
    
    let lookup_table = sm.generate_lookup_table();
    let start = Instant::now();
    for _ in 0..10000 {
        let _token = sm.lookup_token(&lookup_table, 0, 5);
    }
    let lookup_time = start.elapsed();
    println!("Lookup 10000 tokens: {:?}", lookup_time);
    
    println!("=== Performance Test Complete ===");
}