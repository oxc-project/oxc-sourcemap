use oxc_sourcemap::SourceMapBuilder;
use std::time::Instant;

fn main() {
    println!("HashMap Optimization Demo");
    println!("========================");

    // Test scenario with many duplicate names and sources
    let names = vec![
        "React",
        "useState",
        "useEffect",
        "props",
        "state",
        "component",
        "render",
        "onClick",
        "onChange",
        "className",
        "style",
        "children",
    ];

    let sources = vec!["react.js", "index.js", "components.js", "utils.js", "hooks.js"];

    let iterations = 10000;

    println!("\nBuilding sourcemap with {} operations...", iterations);
    println!("Names pool: {} unique names (will create many duplicates)", names.len());
    println!("Sources pool: {} unique sources (will create many duplicates)", sources.len());

    let start = Instant::now();
    let mut builder = SourceMapBuilder::default();

    for i in 0..iterations {
        // Add names with high duplication rate
        let name = &names[i % names.len()];
        let name_id = builder.add_name(name);

        // Add sources with high duplication rate
        let source = &sources[i % sources.len()];
        let content = format!("// Content for {}", source);
        let source_id = builder.add_source_and_content(source, &content);

        // Add a token
        builder.add_token(
            i as u32 % 100,       // dst_line
            (i as u32 * 7) % 80,  // dst_col
            i as u32 % 50,        // src_line
            (i as u32 * 3) % 100, // src_col
            Some(source_id),
            Some(name_id),
        );
    }

    let sourcemap = builder.into_sourcemap();
    let duration = start.elapsed();

    println!("\n✅ Sourcemap built successfully!");
    println!("⏱️  Time taken: {:?}", duration);
    println!("📊 Final stats:");
    println!("   - Unique names stored: {}", sourcemap.get_names().count());
    println!("   - Unique sources stored: {}", sourcemap.get_sources().count());
    println!("   - Total tokens: {}", sourcemap.get_tokens().count());

    println!("\n🚀 Optimization benefits:");
    println!("   - Single HashMap lookup per add_name/add_source operation");
    println!("   - Reduced Arc allocations (no Arc::clone for HashMap keys)");
    println!("   - Entry API eliminates double hashing");
    println!("   - Better memory locality from fewer allocations");

    // Calculate theoretical savings
    let duplicate_name_ops = iterations - names.len();
    let duplicate_source_ops = iterations - sources.len();

    println!("\n📈 Theoretical savings for this workload:");
    println!("   - Avoided {} duplicate name Arc allocations", duplicate_name_ops);
    println!("   - Avoided {} duplicate source Arc allocations", duplicate_source_ops);
    println!("   - Eliminated {} redundant HashMap lookups", iterations);
}
