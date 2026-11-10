use oxc_sourcemap::{ConcatSourceMapBuilder, JSONSourceMap, OwnedSourceMap, SourceMap, Token};

#[test]
fn null_sources_roundtrip() {
    let input = r#"{
        "version": 3,
        "names": ["name"],
        "sources": [null, "", "a\\b.js", null],
        "sourcesContent": [null, "", null, "anonymous content"],
        "mappings": "AAAAA,CCAA,CCAA,CCAA",
        "ignoreList": [0, 3]
    }"#;
    let value: serde_json::Value = serde_json::from_str(input).unwrap();
    let maps = [
        SourceMap::from_json_string(input).unwrap(),
        SourceMap::from_json(serde_json::from_str::<JSONSourceMap>(input).unwrap()).unwrap(),
        SourceMap::from_json(serde_json::from_reader(input.as_bytes()).unwrap()).unwrap(),
        SourceMap::from_json(serde_json::from_value(value.clone()).unwrap()).unwrap(),
    ];

    for map in maps {
        assert_eq!(map.get_sources().len(), 4);
        assert_eq!(map.get_sources().collect::<Vec<_>>(), [None, Some(""), Some("a\\b.js"), None]);
        assert_eq!(map.get_source(0), None);
        assert_eq!(map.get_source(1), Some(""));
        assert_eq!(map.get_source(2), Some("a\\b.js"));
        assert_eq!(map.get_source(3), None);
        assert_eq!(map.get_source_content(3), Some("anonymous content"));
        assert_eq!(map.get_ignore_list(), Some(&[0, 3][..]));
        assert_eq!(map.get_source_view_token(0).unwrap().to_tuple(), (None, 0, 0, Some("name")));
        for (index, token) in map.get_source_view_tokens().enumerate() {
            assert_eq!(token.get_source_id(), Some(index as u32));
        }

        let encoded = map.to_json_string();
        let encoded_value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(encoded_value, value);
        let reparsed = SourceMap::from_json_string(&encoded).unwrap();
        assert!(map.get_tokens().eq(reparsed.get_tokens()));
        assert_eq!(SourceMap::from_json(map.to_json()).unwrap().to_json_string(), encoded);
        let owned = SourceMap::from_parts(map.into_parts()).into_owned();
        assert_eq!(owned.to_json_string(), encoded);
    }
}

#[test]
fn all_null_sources() {
    for contents in ["", r#", "sourcesContent": [null]"#, r#", "sourcesContent": ["content"]"#] {
        let input = format!(r#"{{"version":3,"sources":[null],"mappings":"AAAA"{contents}}}"#);
        let map = OwnedSourceMap::from_json_string(&input).unwrap();
        assert_eq!(map.get_sources().collect::<Vec<_>>(), [None]);
        assert_eq!(map.get_token(0).unwrap().get_source_id(), Some(0));
        assert_eq!(map.to_json().sources, [None]);
        let encoded: serde_json::Value = serde_json::from_str(&map.to_json_string()).unwrap();
        assert_eq!(encoded["sources"], serde_json::json!([null]));
    }
}

#[test]
fn concat_preserves_null_source_indices() {
    let first = SourceMap::from_json_string(
        r#"{"version":3,"sources":[null,"a.js"],"sourcesContent":["anonymous a"],"mappings":"AAAA,CCAA"}"#,
    )
    .unwrap();
    let second = SourceMap::from_json_string(
        r#"{"version":3,"sources":[null,"b.js"],"sourcesContent":["anonymous b","b"],"mappings":"AAAA,CCAA"}"#,
    )
    .unwrap();
    let borrowed =
        ConcatSourceMapBuilder::from_sourcemaps(&[(&first, 0), (&second, 1)]).into_sourcemap();
    let owned = ConcatSourceMapBuilder::from_owned_sourcemaps(vec![
        (first.clone().into_owned(), 0),
        (second.clone().into_owned(), 1),
    ])
    .into_owned_sourcemap();

    for map in [&borrowed, owned.as_source_map()] {
        assert_eq!(map.get_sources().collect::<Vec<_>>(), [None, Some("a.js"), None, Some("b.js")]);
        assert_eq!(
            map.get_source_contents().collect::<Vec<_>>(),
            [Some("anonymous a"), None, Some("anonymous b"), Some("b")]
        );
        assert_eq!(
            map.get_tokens().collect::<Vec<_>>(),
            [
                Token::new(0, 0, 0, 0, Some(0), None),
                Token::new(0, 1, 0, 0, Some(1), None),
                Token::new(1, 0, 0, 0, Some(2), None),
                Token::new(1, 1, 0, 0, Some(3), None),
            ]
        );
        let encoded = map.to_json_string();
        let reparsed = SourceMap::from_json_string(&encoded).unwrap();
        assert!(map.get_sources().eq(reparsed.get_sources()));
        assert!(map.get_tokens().eq(reparsed.get_tokens()));
    }
}

#[test]
fn compose_preserves_null_sources() {
    let input = SourceMap::from_json_string(
        r#"{"version":3,"sources":[null,"original.js"],"sourcesContent":["anonymous","original"],"mappings":"AAAA,CCAA"}"#,
    )
    .unwrap();
    let generated =
        SourceMap::from_json_string(r#"{"version":3,"sources":[null],"mappings":"AAAA,CAAC"}"#)
            .unwrap();
    let expected_tokens = input.get_tokens().collect::<Vec<_>>();
    let composed = generated.compose(input);
    assert_eq!(composed.get_sources().collect::<Vec<_>>(), [None, Some("original.js")]);
    assert_eq!(composed.get_source_content(0), Some("anonymous"));
    assert_eq!(composed.get_source_content(1), Some("original"));
    assert_eq!(composed.get_tokens().collect::<Vec<_>>(), expected_tokens);
}
