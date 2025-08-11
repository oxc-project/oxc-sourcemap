#![allow(clippy::print_stdout)] // Allow prints for CLI output
#![allow(clippy::print_stderr)] // Allow error prints for CLI output

use oxc_sourcemap::{escape_json_string_fallback, SourceMap};
use std::time::Instant;

#[cfg(target_arch = "x86_64")]
use oxc_sourcemap::escape_json_string_avx2_if_available;

fn main() {
    println!("SIMD JSON String Escaping Benchmark");
    println!("====================================");

    // Check for required SIMD support
    #[cfg(target_arch = "x86_64")]
    {
        let has_avx512 =
            is_x86_feature_detected!("avx512f") && is_x86_feature_detected!("avx512bw");
        let has_avx2 = is_x86_feature_detected!("avx2");

        println!("Hardware Support:");
        println!("  AVX512F+BW: {}", has_avx512);
        println!("  AVX2:       {}", has_avx2);

        if !has_avx512 && !has_avx2 {
            eprintln!("Error: This benchmark requires at least AVX2 support.");
            eprintln!("Your system doesn't support AVX512 or AVX2.");
            std::process::exit(1);
        }

        println!();
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        eprintln!("Error: This benchmark is only supported on x86_64 architecture.");
        std::process::exit(1);
    }

    // Download and parse the real-world sourcemap
    println!("Downloading real-world sourcemap...");
    let sourcemap_url = "https://prod.affineassets.com/js/index.1dd8ba8c.js.map";

    let response = match ureq::get(sourcemap_url).call() {
        Ok(response) => response,
        Err(e) => {
            eprintln!("Failed to download sourcemap: {}", e);
            eprintln!("Falling back to generating a test sourcemap...");

            // Create a large test sourcemap for benchmarking
            let test_sourcemap = create_large_test_sourcemap();
            return run_benchmark_with_sourcemap(&test_sourcemap);
        }
    };

    let sourcemap_content = match response.into_string() {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Failed to read sourcemap content: {}", e);
            eprintln!("Falling back to generating a test sourcemap...");

            let test_sourcemap = create_large_test_sourcemap();
            return run_benchmark_with_sourcemap(&test_sourcemap);
        }
    };

    println!("Sourcemap size: {} bytes", sourcemap_content.len());

    // Parse the sourcemap
    let sourcemap = match SourceMap::from_json_string(&sourcemap_content) {
        Ok(sm) => sm,
        Err(e) => {
            eprintln!("Failed to parse sourcemap: {}", e);
            eprintln!("Falling back to generating a test sourcemap...");

            let test_sourcemap = create_large_test_sourcemap();
            return run_benchmark_with_sourcemap(&test_sourcemap);
        }
    };

    run_benchmark_with_sourcemap(&sourcemap);
}

fn create_large_test_sourcemap() -> SourceMap {
    use oxc_sourcemap::SourceMapBuilder;

    println!("Creating large test sourcemap for benchmarking...");

    let mut builder = SourceMapBuilder::default();

    // Add many names of varying lengths to test SIMD thresholds
    for i in 0..1000 {
        // Reduced from 10000 for faster CI
        // Short names (< 32 bytes)
        builder.add_name(&format!("fn{}", i));

        // Medium names (32-63 bytes)
        builder
            .add_name(&format!("very_long_function_name_that_exceeds_thirty_two_characters_{}", i));

        // Long names (≥ 64 bytes)
        builder.add_name(&format!("extremely_long_function_name_that_definitely_exceeds_sixty_four_characters_and_should_trigger_avx512_optimization_{}", i));
    }

    // Add many sources with varying lengths
    for i in 0..100 {
        // Reduced from 1000 for faster CI
        // Mix of different length sources
        builder.add_source_and_content(&format!("src/short_{}.js", i), "var x = 1;");
        builder.add_source_and_content(
            &format!("src/long_path_name_that_should_trigger_simd_optimizations_{}.js", i),
            &format!("// This is source content number {} with some characters that need escaping: \"quotes\", \\backslashes, \nand newlines\nlet variable = \"string with quotes\";\nfunction test() {{\n  return \"more content\"\n}}", i).repeat(10)  // Reduced from 100
        );
    }

    builder.into_sourcemap()
}

fn run_benchmark_with_sourcemap(sourcemap: &SourceMap) {
    println!("Parsed sourcemap with:");
    println!("  Sources: {}", sourcemap.get_sources().count());
    println!("  Names: {}", sourcemap.get_names().count());
    println!("  Source contents: {}", sourcemap.get_source_contents().count());
    println!();

    // Analyze string lengths to understand SIMD impact
    let mut total_string_bytes = 0;
    let mut long_strings = 0; // >= 64 bytes (AVX512 threshold)
    let mut medium_strings = 0; // >= 32 bytes (AVX2 threshold)
    let mut short_strings = 0; // < 32 bytes (fallback)

    for name in sourcemap.get_names() {
        total_string_bytes += name.len();
        if name.len() >= 64 {
            long_strings += 1;
        } else if name.len() >= 32 {
            medium_strings += 1;
        } else {
            short_strings += 1;
        }
    }

    for source in sourcemap.get_sources() {
        total_string_bytes += source.len();
        if source.len() >= 64 {
            long_strings += 1;
        } else if source.len() >= 32 {
            medium_strings += 1;
        } else {
            short_strings += 1;
        }
    }

    for content in sourcemap.get_source_contents().flatten() {
        total_string_bytes += content.len();
        if content.len() >= 64 {
            long_strings += 1;
        } else if content.len() >= 32 {
            medium_strings += 1;
        } else {
            short_strings += 1;
        }
    }

    println!("String analysis:");
    println!("  Total string bytes: {}", total_string_bytes);
    println!("  Long strings (≥64 bytes, AVX512): {}", long_strings);
    println!("  Medium strings (≥32 bytes, AVX2): {}", medium_strings);
    println!("  Short strings (<32 bytes, fallback): {}", short_strings);
    println!();

    // Warm up
    println!("Warming up...");
    for _ in 0..10 {
        let _ = sourcemap.to_json_string();
    }

    // Run benchmarks
    const ITERATIONS: usize = 10; // Reduced from 100 for faster CI
    println!("Running {} iterations for each implementation:", ITERATIONS);
    println!();

    // Benchmark 1: Main implementation (with SIMD dispatch)
    {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let _ = sourcemap.to_json_string();
        }
        let duration = start.elapsed();
        let avg_ms = duration.as_nanos() as f64 / ITERATIONS as f64 / 1_000_000.0;
        println!(
            "Main (SIMD dispatch): {:.3}ms avg ({:.3}s total)",
            avg_ms,
            duration.as_secs_f64()
        );
    }

    // Benchmark 2: Fallback implementation only
    // For simplicity, let's just measure string escaping directly on the biggest strings
    {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            // Escape all strings using fallback only
            for name in sourcemap.get_names() {
                let _ = escape_json_string_fallback(name.as_ref());
            }
            for source in sourcemap.get_sources() {
                let _ = escape_json_string_fallback(source.as_ref());
            }
            for content in sourcemap.get_source_contents().flatten() {
                let _ = escape_json_string_fallback(content.as_ref());
            }
        }
        let duration = start.elapsed();
        let avg_ms = duration.as_nanos() as f64 / ITERATIONS as f64 / 1_000_000.0;
        println!(
            "Fallback (strings only): {:.3}ms avg ({:.3}s total)",
            avg_ms,
            duration.as_secs_f64()
        );
    }

    // Benchmark 3: AVX2 only (if available)
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("avx2") {
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            // Escape all strings using AVX2 when possible
            for name in sourcemap.get_names() {
                let _ = escape_json_string_avx2_if_available(name.as_ref())
                    .unwrap_or_else(|| escape_json_string_fallback(name.as_ref()));
            }
            for source in sourcemap.get_sources() {
                let _ = escape_json_string_avx2_if_available(source.as_ref())
                    .unwrap_or_else(|| escape_json_string_fallback(source.as_ref()));
            }
            for content in sourcemap.get_source_contents().flatten() {
                let _ = escape_json_string_avx2_if_available(content.as_ref())
                    .unwrap_or_else(|| escape_json_string_fallback(content.as_ref()));
            }
        }
        let duration = start.elapsed();
        let avg_ms = duration.as_nanos() as f64 / ITERATIONS as f64 / 1_000_000.0;
        println!(
            "AVX2 (strings only):     {:.3}ms avg ({:.3}s total)",
            avg_ms,
            duration.as_secs_f64()
        );
    }

    // Show speedup calculations
    println!();
    println!("Note: Main implementation uses:");
    println!("  - AVX512 for strings ≥64 bytes (if available)");
    println!("  - AVX2 for strings ≥32 bytes (if available)");
    println!("  - Fallback for smaller strings or non-SIMD hardware");
    println!();
    println!("String-only benchmarks show the pure escaping performance difference.");
    println!("Full to_json_string includes other processing (mappings, structure, etc.)");
    println!();
    println!("Benchmark completed successfully!");
}
