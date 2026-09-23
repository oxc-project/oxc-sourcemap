use std::borrow::Cow;

use oxc_sourcemap::{Error, OwnedSourceMap, SourceMap, SourceMapBuilder, TokenChunk};

#[test]
fn compose_traces_mappings_and_preserves_metadata() {
    let mut input = SourceMapBuilder::default();
    let source_id = input.add_source_and_content("original.ts", "let originalName = 1;");
    let name_id = input.add_name("originalName");
    input.add_token(1, 1, 4, 5, Some(source_id), Some(name_id));
    let mut input = input.into_sourcemap().into_parts();
    input.file = Some(Cow::Borrowed("intermediate.js"));
    input.source_root = Some(Cow::Borrowed("../src"));
    input.source_contents = vec![None];
    input.debug_id = Some(Cow::Borrowed("input-debug-id"));

    let mut generated = SourceMapBuilder::default();
    generated.set_file("bundle.js");
    generated.add_source_and_content("intermediate.js", "\n compiled();");
    let name_id = generated.add_name("compiledName");
    generated.add_token(3, 7, 1, 1, Some(0), Some(name_id));
    generated.set_token_chunks(vec![TokenChunk::default()]);
    let mut generated = generated.into_sourcemap();
    generated.set_debug_id("generated-debug-id");

    let composed = generated.compose(SourceMap::from_parts(input)).unwrap();
    let token = composed.get_source_view_token(0).unwrap();

    assert_eq!((token.get_dst_line(), token.get_dst_col()), (3, 7));
    assert_eq!((token.get_src_line(), token.get_src_col()), (4, 5));
    assert_eq!(token.get_source(), Some("original.ts"));
    assert_eq!(token.get_source_content(), None);
    assert_eq!(token.get_name(), Some("originalName"));
    assert_eq!(composed.get_file(), Some("bundle.js"));
    assert_eq!(composed.get_source_root(), Some("../src"));
    assert_eq!(composed.get_debug_id(), Some("generated-debug-id"));
    assert!(composed.into_parts().token_chunks.is_none());
}

#[test]
fn compose_uses_generated_names_as_deduplicated_fallbacks() {
    let mut input = SourceMapBuilder::default();
    input.add_name("shared");
    input.add_source_and_content("original.js", "let a = b;");
    input.add_token(0, 0, 0, 0, Some(0), None);
    input.add_token(0, 10, 0, 8, Some(0), None);

    let mut generated = SourceMapBuilder::default();
    generated.add_source_and_content("intermediate.js", "let a = b;");
    let shared = generated.add_name("shared");
    let fallback = generated.add_name("fallback");
    generated.add_token(0, 0, 0, 0, Some(0), Some(shared));
    generated.add_token(0, 10, 0, 10, Some(0), Some(fallback));

    let composed = generated.into_sourcemap().compose(input.into_sourcemap()).unwrap();

    assert_eq!(composed.get_names().collect::<Vec<_>>(), ["shared", "fallback"]);
    assert_eq!(composed.get_source_view_token(0).unwrap().get_name(), Some("shared"));
    assert_eq!(composed.get_source_view_token(1).unwrap().get_name(), Some("fallback"));
}

#[test]
fn compose_preserves_unmapped_segments() {
    let mut input = SourceMapBuilder::default();
    input.add_source_and_content("original.js", "    mapped();");
    input.add_token(0, 4, 0, 4, Some(0), None);

    let mut generated = SourceMapBuilder::default();
    generated.add_source_and_content("intermediate.js", "unmapped(); mapped();");
    generated.add_token(0, 0, 0, 0, Some(0), None);
    generated.add_token(0, 12, 0, 4, Some(0), None);
    generated.add_token(0, 22, 0, 0, None, None);

    let composed = generated.into_sourcemap().compose(input.into_sourcemap()).unwrap();
    let tokens = composed.get_tokens().collect::<Vec<_>>();

    assert_eq!(tokens[0].get_source_id(), None);
    assert_eq!(tokens[1].get_source_id(), Some(0));
    assert_eq!(tokens[2].get_source_id(), None);
}

#[test]
fn compose_rejects_multiple_transformation_sources() {
    let mut generated = SourceMapBuilder::default();
    generated.add_source_and_content("first.js", "");
    generated.add_source_and_content("second.js", "");

    let error = generated.into_sourcemap().compose(SourceMap::default()).unwrap_err();

    assert!(matches!(error, Error::MultipleSourcesInComposition(2)));
}

#[test]
fn owned_source_map_compose_delegates_to_source_map() {
    let input = OwnedSourceMap::from_json_string(
        r#"{"version":3,"sources":["original.js"],"names":[],"mappings":"AAAA"}"#,
    )
    .unwrap();
    let generated = OwnedSourceMap::from_json_string(
        r#"{"version":3,"file":"out.js","sources":["intermediate.js"],"names":[],"mappings":"AAAA"}"#,
    )
    .unwrap();

    let composed = generated.compose(input).unwrap();
    assert_eq!(composed.get_file(), Some("out.js"));
    assert_eq!(composed.get_source(0), Some("original.js"));
}
