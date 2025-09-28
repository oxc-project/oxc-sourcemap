use std::fs;

use oxc_sourcemap::{SourceMap, SourcemapVisualizer, Token, Tokens};

#[test]
fn snapshot_sourcemap_visualizer() {
    insta::glob!("fixtures/**/*.js", |path| {
        let js = fs::read_to_string(path).unwrap();
        let js_map = fs::read_to_string(path.with_extension("js.map")).unwrap();
        let sourcemap = SourceMap::from_json_string(&js_map).unwrap();
        let visualizer = SourcemapVisualizer::new(&js, &sourcemap);
        let visualizer_text = visualizer.get_text();
        insta::with_settings!({ snapshot_path => path.parent().unwrap(), prepend_module_to_snapshot => false, snapshot_suffix => "", omit_expression => true }, {
            insta::assert_snapshot!("visualizer", visualizer_text);
        });
    });
}

#[test]
fn invalid_token_position() {
    let mut tokens = Tokens::new();
    tokens.push(Token::new(0, 0, 0, 0, Some(0), None));
    tokens.push(Token::new(0, 10, 0, 0, Some(0), None));
    tokens.push(Token::new(0, 0, 0, 10, Some(0), None));

    let sourcemap = SourceMap::new(
        None,
        vec![],
        None,
        vec!["src.js".into()],
        vec![Some("abc\ndef".into())],
        tokens,
        None,
    );
    let js = "abc\ndef\n";
    let visualizer = SourcemapVisualizer::new(js, &sourcemap);
    let visualizer_text = visualizer.get_text();
    insta::assert_snapshot!("invalid_token_position", visualizer_text);
}
