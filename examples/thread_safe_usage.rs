use std::thread;
use oxc_sourcemap::{SourceMapBuilder, SharedSourceMap};

fn main() {
    // Build a sourcemap using the builder (uses Rc internally for performance)
    let mut builder = SourceMapBuilder::default();
    builder.set_file("output.js");
    builder.set_source_and_content("input.js", "const x = 1;");
    builder.add_name("x");
    builder.add_token(0, 0, 0, 6, Some(0), Some(0));
    
    let sourcemap = builder.into_sourcemap();
    
    // Single-threaded usage (fast with Rc)
    println!("Single-threaded access:");
    println!("  File: {:?}", sourcemap.get_file().map(|s| s.as_ref()));
    println!("  Source: {:?}", sourcemap.get_source(0).map(|s| s.as_ref()));
    
    // Convert to thread-safe version when needed (converts Rc to Arc)
    let shared = SharedSourceMap::new(sourcemap);
    
    // Can now share across threads
    let handles: Vec<_> = (0..3)
        .map(|i| {
            let shared_clone = shared.clone();
            thread::spawn(move || {
                println!("Thread {}: File = {:?}", i, shared_clone.get_file().map(|s| s.as_ref()));
            })
        })
        .collect();
    
    for handle in handles {
        handle.join().unwrap();
    }
    
    println!("\nConversion complete! The sourcemap is now thread-safe.");
}