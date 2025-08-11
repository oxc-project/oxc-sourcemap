#[cfg(target_arch = "x86_64")]
use oxc_sourcemap::escape_json_string_avx2_if_available;
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
    // This test verifies that feature detection functions work correctly and don't panic
    let _has_avx512f = std::is_x86_feature_detected!("avx512f");
    let _has_avx512bw = std::is_x86_feature_detected!("avx512bw");
    let _has_avx2 = std::is_x86_feature_detected!("avx2");

    // Test the main escape function which uses feature detection internally
    let test_input = "simple test";
    let result = oxc_sourcemap::escape_json_string(test_input);

    // Verify it returns a properly formatted JSON string
    assert!(result.starts_with('"') && result.ends_with('"'));
}

#[test]
#[cfg(target_arch = "x86_64")]
fn test_avx2_integration() {
    let long_test = "test".repeat(100);
    let test_cases = vec!["simple", "with \"quotes\"", "with\ncontrol\tchars", long_test.as_str()];

    for test in test_cases {
        let fallback_result = escape_json_string_fallback(test);
        let main_result = escape_json_string(test);

        // Test AVX2 specifically if available
        if let Some(avx2_result) = escape_json_string_avx2_if_available(test) {
            assert_eq!(
                avx2_result, fallback_result,
                "AVX2 result differs from fallback for: {:?}",
                test
            );
            assert_eq!(
                avx2_result, main_result,
                "AVX2 result differs from main function for: {:?}",
                test
            );
        }

        assert_eq!(
            main_result, fallback_result,
            "Main result differs from fallback for: {:?}",
            test
        );
    }
}
