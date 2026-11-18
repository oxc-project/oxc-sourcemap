use oxc_sourcemap::{ConcatSourceMapBuilder, Mappings, SourceMap, Token};

const FIXTURES: [&str; 3] = [
    include_str!("fixtures/perf/real_large.map"),
    include_str!("fixtures/perf/real_medium.map"),
    include_str!("fixtures/perf/real_small.map"),
];

fn mappings_field(json: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(json).unwrap();
    value["mappings"].as_str().unwrap().to_owned()
}

#[test]
fn fixtures_round_trip() {
    for json in FIXTURES {
        let map = SourceMap::from_json_string(json).unwrap();
        assert_eq!(map.token_count(), map.get_tokens().len());
        assert!(map.mappings().byte_len() < map.token_count() * 4);

        // Encoding again gives the same `mappings` field from both encoders, and decoding
        // that gives the same mappings.
        let encoded = map.to_json_string();
        assert_eq!(mappings_field(&encoded), map.to_json().mappings);
        assert_eq!(SourceMap::from_json_string(&encoded).unwrap().mappings(), map.mappings());

        // Encoding the decoded tokens gives the same bytes as decoding the JSON.
        let tokens: Vec<Token> = map.get_tokens().collect();
        assert_eq!(&Mappings::from_tokens(tokens.iter().copied()), map.mappings());
        assert_eq!(map.mappings().last(), tokens.last().copied());

        // The lookup table has every token, by line.
        let table = map.generate_lookup_table();
        assert_eq!(table.tokens(), tokens);
        for token in &tokens {
            let line = table.get(token.get_dst_line() as usize).unwrap();
            assert!(line.contains(token));
        }
    }
}

/// Joining maps as bytes must equal adding every translated token in order.
#[test]
fn concat_matches_translated_tokens() {
    let maps: Vec<_> =
        FIXTURES.iter().map(|json| SourceMap::from_json_string(json).unwrap()).collect();

    let mut builder = ConcatSourceMapBuilder::default();
    let mut expected = Mappings::default();
    let (mut line, mut sources, mut names) = (0, 0, 0);
    // Add every map twice. The second round starts on the last line of the first.
    for map in maps.iter().chain(&maps) {
        builder.add_sourcemap(map, line);
        for (index, token) in map.get_tokens().enumerate() {
            let token = Token::new(
                token.get_dst_line() + line,
                token.get_dst_col(),
                token.get_src_line(),
                token.get_src_col(),
                token.get_source_id().map(|id| id + sources),
                token.get_name_id().map(|id| id + names),
            );
            if index == 0 && expected.last() == Some(token) {
                continue;
            }
            expected.push(token);
        }
        line += map.get_tokens().last().map_or(0, |token| token.get_dst_line());
        sources += map.get_sources().len() as u32;
        names += map.get_names().len() as u32;
    }

    let concat = builder.into_sourcemap();
    assert_eq!(concat.mappings(), &expected);
    let json = concat.to_json_string();
    let decoded = SourceMap::from_json_string(&json).unwrap();
    assert_eq!(decoded.mappings(), &expected);
}

#[test]
fn unmapped_tokens_keep_only_their_generated_position() {
    let token = Token::new(1, 2, 3, 4, None, None);
    assert_eq!(token, Token::unmapped(1, 2));
    assert_eq!((token.get_src_line(), token.get_src_col()), (0, 0));
}
