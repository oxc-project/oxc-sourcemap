use std::hint::black_box;

// Current serde-based implementation
fn escape_json_string_current<S: AsRef<str>>(s: S) -> String {
    let s = s.as_ref();
    let mut escaped_buf = Vec::with_capacity(s.len() * 2 + 2);
    serde::Serialize::serialize(s, &mut serde_json::Serializer::new(&mut escaped_buf)).unwrap();
    unsafe { String::from_utf8_unchecked(escaped_buf) }
}

// Fast path optimization: check if we need escaping at all
fn escape_json_string_optimized<S: AsRef<str>>(s: S) -> String {
    let s = s.as_ref();
    
    // Fast path: check if we need escaping at all
    // Most sourcemap names and sources are simple identifiers that don't need escaping
    let needs_escaping = s.bytes().any(|b| match b {
        b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 0x08 | 0x0C | 0x00..=0x1F => true,
        _ => false,
    });
    
    if !needs_escaping {
        // Fast path: no escaping needed, just add quotes
        let mut result = String::with_capacity(s.len() + 2);
        result.push('"');
        result.push_str(s);
        result.push('"');
        return result;
    }
    
    // Slow path: use serde for complex escaping (same as current implementation)
    let mut escaped_buf = Vec::with_capacity(s.len() * 2 + 2);
    serde::Serialize::serialize(s, &mut serde_json::Serializer::new(&mut escaped_buf)).unwrap();
    unsafe { String::from_utf8_unchecked(escaped_buf) }
}

fn main() {
    let test_strings = vec![
        // Common sourcemap names (no escaping needed)
        "x", "alert", "console", "log", "function", "var", "if", "else", "require", "exports",
        "React", "useState", "useEffect", "component", "props", "state",
        // Common sourcemap sources (no escaping needed)  
        "index.js", "main.tsx", "App.jsx", "utils.ts", "coolstuff.js", "other-file.js",
        "src/components/Button.tsx", "lib/helpers/format.js", "node_modules/react/index.js",
        // Strings that need escaping (uncommon)
        "hello \"quoted\" world",
        "hello\\world",
        "hello\nworld\ttab",
        "mixed content with \"quotes\" and \\backslashes\n",
        "控制字符\x0B测试",
        "虎🐯",
    ];

    // Verify correctness first
    println!("Verifying correctness...");
    for s in &test_strings {
        let current_result = escape_json_string_current(s);
        let optimized_result = escape_json_string_optimized(s);
        if current_result != optimized_result {
            println!("MISMATCH for '{}': '{}' vs '{}'", s, current_result, optimized_result);
        } else {
            println!("OK: '{}' -> '{}'", s, current_result);
        }
    }

    // Warmup
    for _ in 0..1000 {
        for s in &test_strings {
            black_box(escape_json_string_current(s));
            black_box(escape_json_string_optimized(s));
        }
    }

    let iterations = 100000;
    
    // Benchmark current implementation
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        for s in &test_strings {
            black_box(escape_json_string_current(s));
        }
    }
    let current_time = start.elapsed();
    
    // Benchmark optimized implementation  
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        for s in &test_strings {
            black_box(escape_json_string_optimized(s));
        }
    }
    let optimized_time = start.elapsed();
    
    println!("\nBenchmark results:");
    println!("Current implementation: {:?}", current_time);
    println!("Optimized implementation: {:?}", optimized_time);
    println!("Speedup: {:.2}x", current_time.as_nanos() as f64 / optimized_time.as_nanos() as f64);
}