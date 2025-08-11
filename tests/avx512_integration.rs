use oxc_sourcemap::{escape_json_string, escape_json_string_fallback};

#[test]
fn test_integration_escape_functions() {
    let long_test = "test".repeat(100);
    let test_cases = vec!["simple", "with \"quotes\"", "with\ncontrol\tchars", long_test.as_str()];

    for test in test_cases {
        let result1 = escape_json_string(test);
        let result2 = escape_json_string_fallback(test);

        assert_eq!(result1, result2, "Results should match for: {:?}", test);

        // Verify basic JSON escaping behavior
        assert!(result1.starts_with('"'));
        assert!(result1.ends_with('"'));

        if test.contains('"') {
            assert!(result1.contains("\\\""));
        }
        if test.contains('\n') {
            assert!(result1.contains("\\n"));
        }
    }
}

#[test]
#[cfg(target_arch = "x86_64")]
fn test_feature_detection() {
    // This test just verifies that feature detection works
    let has_avx512f = std::is_x86_feature_detected!("avx512f");
    let has_avx512bw = std::is_x86_feature_detected!("avx512bw");

    println!("AVX512F detected: {}", has_avx512f);
    println!("AVX512BW detected: {}", has_avx512bw);

    // Test should always pass regardless of CPU features
    assert!(true);
}
