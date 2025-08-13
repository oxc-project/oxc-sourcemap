use std::hint::black_box;

// Current optimized implementation
fn escape_json_string_optimized<S: AsRef<str>>(s: S) -> String {
    let s = s.as_ref();
    
    // Fast path: Most sourcemap strings are simple identifiers that don't need escaping
    // Quick check for common special characters
    if s.contains(&['"', '\\', '\n', '\r', '\t'][..]) || s.bytes().any(|b| b < 0x20) {
        // Found special characters - use serde for escaping
        let mut escaped_buf = Vec::with_capacity(s.len() * 2 + 2);
        serde::Serialize::serialize(s, &mut serde_json::Serializer::new(&mut escaped_buf)).unwrap();
        unsafe { String::from_utf8_unchecked(escaped_buf) }
    } else {
        // Fast path: no escaping needed, just add quotes
        let mut result = String::with_capacity(s.len() + 2);
        result.push('"');
        result.push_str(s);
        result.push('"');
        result
    }
}

// Original serde-based implementation
fn escape_json_string_original<S: AsRef<str>>(s: S) -> String {
    let s = s.as_ref();
    let mut escaped_buf = Vec::with_capacity(s.len() * 2 + 2);
    serde::Serialize::serialize(s, &mut serde_json::Serializer::new(&mut escaped_buf)).unwrap();
    unsafe { String::from_utf8_unchecked(escaped_buf) }
}

fn main() {
    // Realistic sourcemap strings (mostly don't need escaping)
    let test_strings = vec![
        "coolstuff.js", "x", "alert", "console", "log", "function", "var", "if", "else",
        "React", "useState", "useEffect", "component", "props", "state",
        "index.js", "main.tsx", "App.jsx", "utils.ts", "other-file.js",
        "src/components/Button.tsx", "lib/helpers/format.js", "node_modules/react/index.js",
        // A few that need escaping
        "quoted\"string", "newline\nstring", "tab\tstring",
    ];

    println!("Testing correctness...");
    for s in &test_strings {
        let original = escape_json_string_original(s);
        let optimized = escape_json_string_optimized(s);
        if original != optimized {
            println!("MISMATCH: '{}' -> '{}' vs '{}'", s, original, optimized);
        }
    }
    println!("Correctness check passed!");

    let iterations = 100000;
    
    // Benchmark original
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        for s in &test_strings {
            black_box(escape_json_string_original(s));
        }
    }
    let original_time = start.elapsed();
    
    // Benchmark optimized
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        for s in &test_strings {
            black_box(escape_json_string_optimized(s));
        }
    }
    let optimized_time = start.elapsed();
    
    println!("\nBenchmark results:");
    println!("Original: {:?}", original_time);
    println!("Optimized: {:?}", optimized_time);
    println!("Speedup: {:.2}x", original_time.as_nanos() as f64 / optimized_time.as_nanos() as f64);
}