use oxc_sourcemap::SourceMap;
use std::hint::black_box;

fn main() {
    // Use a real sourcemap example to test
    let input = r#"{
        "version": 3,
        "sources": ["coolstuff.js", "other-file.js", "yet/another/file.js"],
        "sourceRoot": "x",
        "names": ["x","alert", "console", "log", "function", "var", "if", "else"],
        "mappings": "AAAA,GAAIA,GAAI,EACR,IAAIA,GAAK,EAAG,CACVC,MAAM",
        "x_google_ignoreList": [0],
        "sourcesContent": ["var x = 1;\nif (x == 2) {\n  alert('test');\n}", "console.log('hello world');", "function test() { return 42; }"]
    }"#;

    let sm = SourceMap::from_json_string(input).unwrap();
    
    let sources: Vec<_> = sm.get_sources().collect();
    let names: Vec<_> = sm.get_names().collect();
    
    println!("Source map contains:");
    println!("  {} sources", sources.len());
    println!("  {} names", names.len());
    
    for (i, source) in sources.iter().enumerate() {
        println!("  source[{}]: '{}'", i, source);
    }
    
    for (i, name) in names.iter().enumerate() {
        println!("  name[{}]: '{}'", i, name);
    }

    // Benchmark the to_json_string operation
    let iterations = 10000;
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        black_box(sm.to_json_string());
    }
    let time = start.elapsed();
    
    println!("\nto_json_string benchmark: {:?} for {} iterations", time, iterations);
    println!("Average per call: {:?}", time / iterations);
}