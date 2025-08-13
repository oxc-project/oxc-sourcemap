use std::hint::black_box;

// Current serde-based implementation
fn escape_json_string_current<S: AsRef<str>>(s: S) -> String {
    let s = s.as_ref();
    let mut escaped_buf = Vec::with_capacity(s.len() * 2 + 2);
    serde::Serialize::serialize(s, &mut serde_json::Serializer::new(&mut escaped_buf)).unwrap();
    unsafe { String::from_utf8_unchecked(escaped_buf) }
}

// Test just the fast path logic
fn escape_json_string_fast_path_only<S: AsRef<str>>(s: S) -> String {
    let s = s.as_ref();
    let mut result = String::with_capacity(s.len() + 2);
    result.push('"');
    result.push_str(s);
    result.push('"');
    result
}

// Test just the escape checking logic
fn needs_escaping<S: AsRef<str>>(s: S) -> bool {
    let s = s.as_ref();
    s.bytes().any(|b| match b {
        b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 0x08 | 0x0C | 0x00..=0x1F => true,
        _ => false,
    })
}

fn main() {
    // Test with realistic sourcemap strings
    let test_strings = vec![
        "x", "alert", "console", "log", "function", "var", "if", "else", "require", "exports",
        "React", "useState", "useEffect", "component", "props", "state",
        "index.js", "main.tsx", "App.jsx", "utils.ts", "coolstuff.js", "other-file.js",
        "src/components/Button.tsx", "lib/helpers/format.js", "node_modules/react/index.js",
    ];

    // Check if any of these actually need escaping
    for s in &test_strings {
        if needs_escaping(s) {
            println!("'{}' needs escaping", s);
        }
    }
    println!("None of the test strings need escaping!");

    let iterations = 100000;
    
    // Benchmark current implementation
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        for s in &test_strings {
            black_box(escape_json_string_current(s));
        }
    }
    let current_time = start.elapsed();
    
    // Benchmark fast path only
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        for s in &test_strings {
            black_box(escape_json_string_fast_path_only(s));
        }
    }
    let fast_path_time = start.elapsed();
    
    // Benchmark just the escape checking
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        for s in &test_strings {
            black_box(needs_escaping(s));
        }
    }
    let check_time = start.elapsed();
    
    println!("\nBenchmark results:");
    println!("Current implementation: {:?}", current_time);
    println!("Fast path only: {:?}", fast_path_time);
    println!("Escape checking only: {:?}", check_time);
    println!("Fast path speedup over current: {:.2}x", current_time.as_nanos() as f64 / fast_path_time.as_nanos() as f64);
}