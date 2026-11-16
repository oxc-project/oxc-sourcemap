use oxc_sourcemap::{
    ConcatSourceMapBuilder, Mappings, PackedConcatSourceMapBuilder, PackedSourceMap, SourceMap,
    Token,
};

const FIXTURES: [&str; 3] = [
    include_str!("fixtures/perf/real_large.map"),
    include_str!("fixtures/perf/real_medium.map"),
    include_str!("fixtures/perf/real_small.map"),
];

/// A token as [`Mappings`] holds it: an unmapped token keeps only its generated position.
fn normalized(token: Token) -> Token {
    if token.get_source_id().is_some() {
        token
    } else {
        Token::unmapped(token.get_dst_line(), token.get_dst_col())
    }
}

fn line_span(map: &SourceMap) -> u32 {
    map.get_tokens().last().map_or(1, |token| token.get_dst_line() + 1)
}

#[test]
fn fixtures_round_trip() {
    for json in FIXTURES {
        let map = SourceMap::from_json_string(json).unwrap();
        let packed = PackedSourceMap::from_json_string(json).unwrap();
        let tokens: Vec<Token> = map.get_tokens().map(normalized).collect();

        // Same tokens, in less than four bytes each.
        assert_eq!(packed.token_count(), tokens.len());
        assert_eq!(packed.tokens().len(), tokens.len());
        assert!(packed.tokens().eq(tokens.iter().copied()));
        assert_eq!(packed.mappings().last(), tokens.last().copied());
        assert!(packed.mappings().byte_len() < tokens.len() * 4);

        // Same JSON, written without decoding the tokens.
        let encoded = map.to_json_string();
        assert_eq!(packed.to_json_string(), encoded);

        // Packing the decoded map gives the same bytes as decoding the JSON.
        assert_eq!(map.clone().into_packed().mappings(), packed.mappings());
        assert_eq!(&Mappings::from_tokens(tokens.iter().copied()), packed.mappings());

        // Unpacking gives the map back.
        assert_eq!(packed.unpack().to_json_string(), encoded);
        assert_eq!(packed.clone().into_sourcemap().to_json_string(), encoded);
        assert_eq!(SourceMap::from(packed.clone().into_owned()).to_json_string(), encoded);
        assert_eq!(PackedSourceMap::from(map).to_json_string(), encoded);
    }
}

/// Joining packed maps as bytes must equal joining the decoded maps.
#[test]
fn concat_matches_concat_sourcemap_builder() {
    let maps: Vec<_> =
        FIXTURES.iter().map(|json| SourceMap::from_json_string(json).unwrap()).collect();
    let packed: Vec<_> =
        FIXTURES.iter().map(|json| PackedSourceMap::from_json_string(json).unwrap()).collect();

    // Add every map twice. The second round starts on the last line of the first.
    let mut inputs = vec![];
    let mut line = 0;
    for index in (0..maps.len()).chain(0..maps.len()) {
        inputs.push((index, line));
        line += line_span(&maps[index]) - u32::from(index == maps.len() - 1);
    }

    let mut expected = ConcatSourceMapBuilder::default();
    let mut borrowed = PackedConcatSourceMapBuilder::default();
    let mut owned = PackedConcatSourceMapBuilder::default();
    for &(index, line) in &inputs {
        expected.add_sourcemap(&maps[index], line);
        borrowed.add_sourcemap(&packed[index], line);
        owned.add_sourcemap_owned(packed[index].clone(), line);
    }
    let presized = PackedConcatSourceMapBuilder::from_sourcemaps(
        &inputs.iter().map(|&(index, line)| (&packed[index], line)).collect::<Vec<_>>(),
    );

    let expected = expected.into_sourcemap();
    let json = expected.to_json_string();
    for builder in [borrowed, owned, presized] {
        let joined = builder.into_sourcemap();
        assert_eq!(joined.token_count(), expected.get_tokens().len());
        assert!(joined.tokens().eq(expected.get_tokens().map(normalized)));
        assert_eq!(joined.to_json_string(), json);
    }
}

#[test]
fn concat_drops_duplicate_boundary_token() {
    let map = |tokens: Vec<Token>| {
        SourceMap::new(
            None,
            vec![],
            None,
            vec![Some("a.js".into())],
            vec![],
            tokens.into_boxed_slice(),
            None,
        )
    };
    let first = map(vec![Token::unmapped(0, 0), Token::unmapped(1, 0)]);
    let second = map(vec![Token::unmapped(0, 0), Token::new(0, 4, 2, 0, Some(0), None)]);

    let mut expected = ConcatSourceMapBuilder::default();
    expected.add_sourcemap(&first, 0);
    expected.add_sourcemap(&second, 1);
    let expected = expected.into_sourcemap();
    assert_eq!(expected.get_tokens().len(), 3);

    let mut builder = PackedConcatSourceMapBuilder::default();
    builder.add_sourcemap_owned(first.clone().into_packed(), 0);
    builder.add_sourcemap_owned(second.clone().into_packed(), 1);
    let joined = builder.into_sourcemap();
    assert!(joined.tokens().eq(expected.get_tokens()));
    assert_eq!(joined.to_json_string(), expected.to_json_string());
}

#[test]
fn strings_stay_editable() {
    let mut packed = PackedSourceMap::from_json_string(FIXTURES[2]).unwrap();
    let mut map = SourceMap::from_json_string(FIXTURES[2]).unwrap();
    let sources: Vec<String> = (0..map.get_sources().len()).map(|i| format!("{i}.js")).collect();

    packed.set_file("out.js");
    packed.set_sources(sources.iter().map(Some));
    packed.set_source_contents(vec![None; sources.len()]);
    packed.set_ignore_list(vec![0]);
    packed.set_debug_id("56431d54-c0a6-451d-8ea2-ba5de5d8ca2e");
    map.set_file("out.js");
    map.set_sources(sources.iter().map(Some));
    map.set_source_contents(vec![None; sources.len()]);
    map.set_ignore_list(vec![0]);
    map.set_debug_id("56431d54-c0a6-451d-8ea2-ba5de5d8ca2e");

    assert_eq!(packed.strings().get_file(), Some("out.js"));
    assert_eq!(packed.to_json_string(), map.to_json_string());
}

#[test]
fn empty_and_invalid() {
    let empty = PackedSourceMap::default();
    assert_eq!(empty.token_count(), 0);
    assert_eq!(empty.to_json_string(), SourceMap::default().to_json_string());
    assert_eq!(empty.mappings().last(), None);

    for mappings in ["AAAA;;;", ";;;AAAA,C", ""] {
        let json =
            format!(r#"{{"version":3,"sources":["a.js"],"names":[],"mappings":"{mappings}"}}"#);
        let packed = PackedSourceMap::from_json_string(&json).unwrap();
        let map = SourceMap::from_json_string(&json).unwrap();
        assert_eq!(packed.to_json_string(), map.to_json_string());
        assert!(packed.tokens().eq(map.get_tokens().map(normalized)));
    }

    // A source, a name and a column that do not exist.
    for mappings in ["ACAA", "AAAAC", "D"] {
        let json =
            format!(r#"{{"version":3,"sources":["a.js"],"names":[],"mappings":"{mappings}"}}"#);
        assert!(SourceMap::from_json_string(&json).is_err());
        assert!(PackedSourceMap::from_json_string(&json).is_err());
    }
}
