use oxc_sourcemap::{OwnedSourceMap, SourceMapBuilder};

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

    // Stage 3 — compose the chain the way a bundler collapses it: resolve each
    // bundle token's intermediate position through the codegen map.
    let table = codegen_map.generate_lookup_table();
    let mut composed = SourceMapBuilder::default();
    for token in bundler_map.get_tokens() {
        let (line, col) = (token.get_src_line(), token.get_src_col());

        // The strict lookup lands before the line's first token (column 0 < 1)
        // and returns `None` — composing with it would silently drop the whole
        // statement from the final map.
        assert!(codegen_map.lookup_token(&table, line, col).is_none());

        // The approximating lookup clamps to the line's first token instead.
        let origin = codegen_map
            .lookup_source_view_token_approx(&table, line, col)
            .expect("approx lookup must resolve the indented line");
        let src_id = composed.add_source_and_content(
            origin.get_source().unwrap(),
            origin.get_source_content().unwrap(),
        );
        composed.add_token(
            token.get_dst_line(),
            token.get_dst_col(),
            origin.get_src_line(),
            origin.get_src_col(),
            Some(src_id),
            None,
        );
    }

    // Stage 4 — the composed map round-trips through JSON (encode + decode)
    // and still resolves the bundle position back to the original source.
    let json = composed.into_sourcemap().to_json_string();
    let composed = OwnedSourceMap::from_json_string(&json).unwrap();
    let table = composed.generate_lookup_table();
    let origin = composed.lookup_source_view_token_approx(&table, 3, 0).unwrap();
    assert_eq!(origin.get_source(), Some("a.js"));
    assert_eq!(origin.get_source_content(), Some("globalThis.side = 1;"));
    assert_eq!((origin.get_src_line(), origin.get_src_col()), (0, 0));
}
