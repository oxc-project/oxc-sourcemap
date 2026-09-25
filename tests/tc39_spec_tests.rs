#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::{fs, path::PathBuf};

use oxc_sourcemap::SourceMap;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestCase {
    name: String,
    source_map_file: String,
    source_map_is_valid: bool,
    #[serde(default)]
    test_actions: Vec<TestAction>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "actionType", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum TestAction {
    CheckMapping {
        generated_line: u32,
        generated_column: u32,
        original_source: Option<String>,
        original_line: Option<u32>,
        original_column: Option<u32>,
        mapped_name: Option<String>,
    },
    CheckIgnoreList {
        present: Vec<String>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct TestSuite {
    tests: Vec<TestCase>,
}

#[test]
fn tc39_source_map_spec_tests() {
    let test_file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/source-map-tests/source-map-spec-tests.json");
    let resources_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/source-map-tests/resources");

    // Skip test if tc39 tests haven't been cloned yet
    if !test_file.exists() {
        eprintln!("Skipping tc39 tests - run 'just init' to clone the test suite");
        return;
    }

    let test_suite: TestSuite =
        serde_json::from_str(&fs::read_to_string(test_file).unwrap()).unwrap();

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    for test in test_suite.tests {
        // Skip tests with null sources - not supported in our API
        if test.name == "sourcesAndSourcesContentBothNull"
            || test.name == "sourcesNullSourcesContentNonNull"
        {
            skipped += 1;
            continue;
        }

        let source_map_path = resources_dir.join(&test.source_map_file);
        let Ok(source_map_content) = fs::read_to_string(&source_map_path) else {
            eprintln!("⊘ {}: skipped (file not found)", test.name);
            skipped += 1;
            continue;
        };

        let result = SourceMap::from_json_string(&source_map_content);

        // Check if parsing result matches expected validity
        let parse_result_valid = result.is_ok();
        if parse_result_valid != test.source_map_is_valid {
            eprintln!(
                "✗ {}: expected valid={}, got valid={}",
                test.name, test.source_map_is_valid, parse_result_valid
            );
            if let Err(e) = result {
                eprintln!("  Error: {e:?}");
            }
            failed += 1;
            continue;
        }

        // If the source map is valid, run test actions
        if let Ok(source_map) = result
            && !run_test_actions(&test, &source_map)
        {
            failed += 1;
            continue;
        }

        passed += 1;
    }

    println!("\n{passed} passed, {failed} failed, {skipped} skipped");

    // Don't panic on failures - some tests are expected to fail for unimplemented features
    // (index maps, sourceRoot resolution, etc.) and we skip tests with null sources
    assert!(passed >= 86, "Expected at least 86 tests to pass, but only {passed} passed");
}

fn run_test_actions(test: &TestCase, source_map: &SourceMap) -> bool {
    for action in &test.test_actions {
        match action {
            TestAction::CheckMapping {
                generated_line,
                generated_column,
                original_source,
                original_line,
                original_column,
                mapped_name,
            } => {
                let lookup_table = source_map.generate_lookup_table();
                let token = source_map.lookup_source_view_token(
                    &lookup_table,
                    *generated_line,
                    *generated_column,
                );

                if let Some(token) = token {
                    let (source, src_line, src_col, name) = token.to_tuple();

                    // Check source
                    if source != original_source.as_deref() {
                        eprintln!(
                            "✗ {}: mapping check failed - expected source {:?}, got {:?}",
                            test.name, original_source, source
                        );
                        return false;
                    }

                    // Check line and column
                    if let (Some(exp_line), Some(exp_col)) = (original_line, original_column)
                        && (src_line != *exp_line || src_col != *exp_col)
                    {
                        eprintln!(
                            "✗ {}: mapping check failed - expected {}:{}, got {}:{}",
                            test.name, exp_line, exp_col, src_line, src_col
                        );
                        return false;
                    }

                    // Check name
                    if name != mapped_name.as_deref() {
                        eprintln!(
                            "✗ {}: mapping check failed - expected name {:?}, got {:?}",
                            test.name, mapped_name, name
                        );
                        return false;
                    }
                } else {
                    eprintln!(
                        "✗ {}: mapping check failed - no token found at {}:{}",
                        test.name, generated_line, generated_column
                    );
                    return false;
                }
            }
            TestAction::CheckIgnoreList { present } => {
                let ignore_list = source_map.get_ignore_list();
                if let Some(indices) = ignore_list {
                    for source_name in present {
                        // Find the index of this source in the sources array
                        let source_index = source_map.get_sources().position(|s| s == source_name);

                        if let Some(idx) = source_index {
                            if !indices.contains(&(idx as u32)) {
                                eprintln!(
                                    "✗ {}: ignore list check failed - '{}' not in ignore list",
                                    test.name, source_name
                                );
                                return false;
                            }
                        } else {
                            eprintln!(
                                "✗ {}: ignore list check failed - source '{}' not found",
                                test.name, source_name
                            );
                            return false;
                        }
                    }
                } else if !present.is_empty() {
                    eprintln!("✗ {}: ignore list check failed - no ignore list found", test.name);
                    return false;
                }
            }
            TestAction::Unknown => {
                eprintln!("⊘ {}: unknown test action, skipping", test.name);
            }
        }
    }

    true
}
