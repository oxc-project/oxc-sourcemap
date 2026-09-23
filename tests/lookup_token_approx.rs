use oxc_sourcemap::SourceMapBuilder;

/// End-to-end model of the composition scenario `lookup_token_approx` exists
/// for (rolldown/rolldown#10070): an indented line whose mapping survives
/// collapsing a two-map chain only because the lookup clamps instead of
/// returning `None`.
#[test]
fn compose_sourcemaps_with_approx_lookup_keeps_indented_lines() {
    // Stage 1 — codegen wraps the source in a function, indenting it one tab:
    //
    //   a.js (original):        intermediate.js (generated):
    //   globalThis.side = 1;    function wrap() {
    //                           \tglobalThis.side = 1;
    //                           }
    //
    // Codegen anchors at real tokens, so the indented line's first mapping
    // sits *after* the tab, at column 1. Nothing maps column 0.
    let mut codegen = SourceMapBuilder::default();
    let src = codegen.add_source_and_content("a.js", "globalThis.side = 1;");
    let name = codegen.add_name("side");
    codegen.add_token(1, 1, 0, 0, Some(src), None); // `globalThis`, after the tab
    codegen.add_token(1, 12, 0, 11, Some(src), Some(name)); // `side`
    let codegen_map = codegen.into_sourcemap();

    // Stage 2 — the bundler concatenates chunks, landing the wrapper at line 3
    // of the bundle, and samples the moved line at its *start* (column 0).
    let mut bundler = SourceMapBuilder::default();
    let chunk = bundler
        .add_source_and_content("intermediate.js", "function wrap() {\n\tglobalThis.side = 1;\n}");
    bundler.add_token(3, 0, 1, 0, Some(chunk), None); // bundle (3,0) -> intermediate (1,0)
    let bundler_map = bundler.into_sourcemap();

    // Stage 3 — verify strict lookup misses, then compose the chain.
    let table = codegen_map.generate_lookup_table();
    assert!(codegen_map.lookup_token(&table, 1, 0).is_none());
    let composed = bundler_map.compose(codegen_map);
    let origin = composed.get_source_view_token(0).unwrap();

    assert_eq!(origin.get_source(), Some("a.js"));
    assert_eq!(origin.get_source_content(), Some("globalThis.side = 1;"));
    assert_eq!((origin.get_src_line(), origin.get_src_col()), (0, 0));
}
