use oxc_sourcemap::{SharedSourceMap, SourceMapBuilder, ThreadSafeSourceMap};
use std::thread;

#[test]
fn test_single_threaded_rc_performance() {
    // Test that single-threaded operations work with Rc
    let mut builder = SourceMapBuilder::default();
    builder.set_file("test.js");
    builder.set_source_and_content("source.js", "const x = 1;");
    builder.add_name("x");
    builder.add_token(0, 0, 0, 6, Some(0), Some(0));

    let sourcemap = builder.into_sourcemap();

    // Access data multiple times (this uses Rc internally)
    assert_eq!(sourcemap.get_file().map(|s| s.as_ref()), Some("test.js"));
    assert_eq!(sourcemap.get_source(0).map(|s| s.as_ref()), Some("source.js"));
    assert_eq!(sourcemap.get_name(0).map(|s| s.as_ref()), Some("x"));

    // Clone operations should be cheap with Rc
    let names: Vec<_> = sourcemap.get_names().collect();
    assert_eq!(names.len(), 1);
}

#[test]
fn test_thread_safe_sourcemap_creation() {
    let mut builder = SourceMapBuilder::default();
    builder.set_file("test.js");
    builder.set_source_and_content("source1.js", "const x = 1;");
    builder.set_source_and_content("source2.js", "const y = 2;");
    builder.add_name("x");
    builder.add_name("y");

    let sourcemap = builder.into_sourcemap();
    let thread_safe = ThreadSafeSourceMap::from(sourcemap);

    // Test that we can access data through the wrapper
    assert_eq!(thread_safe.get_file().map(|s| s.as_ref()), Some("test.js"));
    assert_eq!(thread_safe.get_source(0).map(|s| s.as_ref()), Some("source1.js"));
    assert_eq!(thread_safe.get_source(1).map(|s| s.as_ref()), Some("source2.js"));
}

#[test]
fn test_cross_thread_read() {
    let mut builder = SourceMapBuilder::default();
    builder.set_file("shared.js");
    builder.set_source_and_content("source1.js", "const x = 1;");
    builder.set_source_and_content("source2.js", "const y = 2;");
    builder.add_name("x");
    builder.add_name("y");
    builder.add_token(0, 0, 0, 6, Some(0), Some(0));
    builder.add_token(1, 0, 0, 6, Some(1), Some(1));

    let sourcemap = builder.into_sourcemap();
    let shared = SharedSourceMap::new(sourcemap);

    // Clone the Arc for each thread
    let ts1 = shared.clone();
    let ts2 = shared.clone();
    let ts3 = shared.clone();

    // Spawn multiple threads that read from the same sourcemap
    let handle1 = thread::spawn(move || {
        assert_eq!(ts1.get_file().map(|s| s.as_ref()), Some("shared.js"));
        assert_eq!(ts1.get_source(0).map(|s| s.as_ref()), Some("source1.js"));
        assert_eq!(ts1.get_name(0).map(|s| s.as_ref()), Some("x"));
        ts1.to_json_string()
    });

    let handle2 = thread::spawn(move || {
        assert_eq!(ts2.get_source(1).map(|s| s.as_ref()), Some("source2.js"));
        assert_eq!(ts2.get_name(1).map(|s| s.as_ref()), Some("y"));
        let tokens: Vec<_> = ts2.get_tokens().collect();
        assert_eq!(tokens.len(), 2);
        ts2.to_data_url()
    });

    let handle3 = thread::spawn(move || {
        let sources: Vec<_> = ts3.get_sources().collect();
        assert_eq!(sources.len(), 2);
        let names: Vec<_> = ts3.get_names().collect();
        assert_eq!(names.len(), 2);
        ts3.to_json()
    });

    // Wait for all threads and check results
    let json_string = handle1.join().unwrap();
    assert!(json_string.contains("shared.js"));

    let data_url = handle2.join().unwrap();
    assert!(data_url.starts_with("data:application/json"));

    let json = handle3.join().unwrap();
    assert_eq!(json.sources.len(), 2);
    assert_eq!(json.names.len(), 2);
}

#[test]
fn test_concurrent_reads_stress() {
    // Create a larger sourcemap for stress testing
    let mut builder = SourceMapBuilder::default();
    builder.set_file("stress.js");

    for i in 0..100 {
        builder.set_source_and_content(
            &format!("source{}.js", i),
            &format!("const var{} = {};", i, i),
        );
        builder.add_name(&format!("var{}", i));
        builder.add_token(i, 0, 0, 6, Some(i), Some(i));
    }

    let sourcemap = builder.into_sourcemap();
    let shared = SharedSourceMap::new(sourcemap);

    // Spawn many threads that all read concurrently
    let mut handles = vec![];

    for thread_id in 0..10 {
        let ts = shared.clone();
        let handle = thread::spawn(move || {
            for i in 0..100 {
                // Each thread reads different parts of the sourcemap
                let idx = (thread_id * 10 + i % 10) % 100;
                let source = ts.get_source(idx);
                assert!(source.is_some());
                let name = ts.get_name(idx);
                assert!(name.is_some());
            }
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_thread_safe_json_round_trip() {
    let json_str = r#"{
        "version": 3,
        "sources": ["foo.js", "bar.js"],
        "names": ["x", "y"],
        "mappings": "AAAA,GAAIA;AAAA,GAAIC"
    }"#;

    let thread_safe = ThreadSafeSourceMap::from_json_string(json_str).unwrap();
    let shared = SharedSourceMap::from_thread_safe(thread_safe);

    // Clone and send to another thread
    let ts = shared.clone();
    let handle = thread::spawn(move || {
        let sources: Vec<_> = ts.get_sources().map(|s| s.as_ref()).collect();
        assert_eq!(sources, vec!["foo.js", "bar.js"]);

        let names: Vec<_> = ts.get_names().map(|s| s.as_ref()).collect();
        assert_eq!(names, vec!["x", "y"]);

        ts.to_json_string()
    });

    let result = handle.join().unwrap();
    assert!(result.contains("foo.js"));
    assert!(result.contains("bar.js"));
}

#[test]
fn test_modification_and_sharing() {
    // Create initial sourcemap
    let mut builder = SourceMapBuilder::default();
    builder.set_file("initial.js");
    builder.set_source_and_content("source.js", "const x = 1;");
    builder.add_name("x");

    let mut sourcemap = builder.into_sourcemap();

    // Modify the sourcemap
    sourcemap.set_file("modified.js");
    sourcemap.set_x_google_ignore_list(vec![0]);
    sourcemap.set_debug_id("debug-123");

    // Convert to thread-safe version
    let shared = SharedSourceMap::new(sourcemap);

    // Share across threads
    let ts1 = shared.clone();
    let ts2 = shared.clone();

    let handle1 = thread::spawn(move || {
        assert_eq!(ts1.get_file().map(|s| s.as_ref()), Some("modified.js"));
        assert_eq!(ts1.get_debug_id(), Some("debug-123"));
        assert_eq!(ts1.get_x_google_ignore_list(), Some(&[0][..]));
    });

    let handle2 = thread::spawn(move || {
        // Verify modifications are visible in other threads
        assert_eq!(ts2.get_file().map(|s| s.as_ref()), Some("modified.js"));
        assert_eq!(ts2.get_debug_id(), Some("debug-123"));
    });

    handle1.join().unwrap();
    handle2.join().unwrap();
}

#[test]
fn test_rc_to_arc_conversion() {
    // Test that conversion from Rc-based SourceMap to Arc-based ThreadSafeSourceMap works
    let mut builder = SourceMapBuilder::default();
    builder.set_file("conversion.js");
    builder.set_source_and_content("source.js", "const x = 1;");
    builder.add_name("variable");

    let sourcemap = builder.into_sourcemap();

    // Single-threaded access with Rc
    assert_eq!(sourcemap.get_file().map(|s| s.as_ref()), Some("conversion.js"));

    // Convert to thread-safe
    let thread_safe = ThreadSafeSourceMap::from(sourcemap);

    // Thread-safe access with Arc
    assert_eq!(thread_safe.get_file().map(|s| s.as_ref()), Some("conversion.js"));
    assert_eq!(thread_safe.get_source(0).map(|s| s.as_ref()), Some("source.js"));
    assert_eq!(thread_safe.get_name(0).map(|s| s.as_ref()), Some("variable"));

    // Convert back to regular SourceMap
    let sourcemap2 = thread_safe.to_sourcemap();
    assert_eq!(sourcemap2.get_file().map(|s| s.as_ref()), Some("conversion.js"));
}
