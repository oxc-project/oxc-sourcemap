use std::time::Instant;
use oxc_sourcemap::SourceMap;

#[test]
fn bench_performance() {
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

    println!("Running simple performance benchmark...");
    
    // Test decoding performance
    let start = Instant::now();
    let mut sourcemaps = Vec::new();
    for _ in 0..100 {
        let sm = SourceMap::from_json_string(input).unwrap();
        sourcemaps.push(sm);
    }
    let decode_time = start.elapsed();
    println!("Decode 100 sourcemaps: {:?}", decode_time);
    
    // Test encoding performance
    let sm = &sourcemaps[0];
    let start = Instant::now();
    for _ in 0..100 {
        let _json = sm.to_json_string();
    }
    let encode_time = start.elapsed();
    println!("Encode 100 sourcemaps: {:?}", encode_time);
    
    // Test lookup performance
    let lookup_table = sm.generate_lookup_table();
    let start = Instant::now();
    for _ in 0..1000 {
        let _token = sm.lookup_token(&lookup_table, 0, 5);
    }
    let lookup_time = start.elapsed();
    println!("Lookup 1000 tokens: {:?}", lookup_time);
    
    println!("Benchmark complete");
}