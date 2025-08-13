use oxc_sourcemap::SourceMap;

fn main() {
    let input = r#"{
        "version": 3,
        "sources": ["coolstuff.js"],
        "sourceRoot": "x",
        "names": ["x","alert"],
        "mappings": "AAAA,GAAIA,GAAI,EACR,IAAIA,GAAK,EAAG,CACVC,MAAM",
        "x_google_ignoreList": [0],
        "sourcesContent": ["var x = 1;\nif (x == 2) {\n  alert('test');\n}"]
    }"#;

    let sm = SourceMap::from_json_string(input).unwrap();
    
    println!("Sources:");
    for (i, source) in sm.get_sources().enumerate() {
        println!("  [{}]: '{}'", i, source);
        // Check if it needs escaping
        let needs_escaping = source.bytes().any(|b| match b {
            b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 0x08 | 0x0C | 0x00..=0x1F => true,
            _ => false,
        });
        println!("    needs escaping: {}", needs_escaping);
    }
    
    println!("\nNames:");
    for (i, name) in sm.get_names().enumerate() {
        println!("  [{}]: '{}'", i, name);
        // Check if it needs escaping
        let needs_escaping = name.bytes().any(|b| match b {
            b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 0x08 | 0x0C | 0x00..=0x1F => true,
            _ => false,
        });
        println!("    needs escaping: {}", needs_escaping);
    }
}