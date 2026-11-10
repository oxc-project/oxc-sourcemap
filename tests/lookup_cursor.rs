//! Differential tests: `LookupCursor` must return exactly what `SourceMap::lookup_token` and
//! `SourceMap::lookup_token_approx` return, whatever order the queries come in.

use oxc_sourcemap::{SourceMap, SourceMapBuilder, Token};

/// Builds a map from per-line generated columns. Each line's columns must be sorted. Every token
/// gets a distinct original column so duplicate generated columns are told apart.
fn map_from_lines(lines: &[Vec<u32>]) -> SourceMap<'static> {
    let mut tokens = Vec::new();
    for (line, cols) in lines.iter().enumerate() {
        for &col in cols {
            let id = tokens.len() as u32;
            tokens.push(Token::new(line as u32, col, 0, id, Some(0), None));
        }
    }
    SourceMap::new(None, vec![], None, vec!["a.js".into()], vec![], tokens.into_boxed_slice(), None)
}

/// Runs `queries` in order through one cursor per lookup kind, and through a third cursor that
/// alternates between the kinds, comparing every answer with the stateless lookups.
fn check(map: &SourceMap, queries: &[(u32, u32)]) {
    let table = map.generate_lookup_table();
    let mut approx = map.lookup_cursor();
    let mut exact = map.lookup_cursor();
    let mut mixed = map.lookup_cursor();
    for (i, &(line, col)) in queries.iter().enumerate() {
        let expected_approx = map.lookup_token_approx(&table, line, col);
        let expected_exact = map.lookup_token(&table, line, col);
        assert_eq!(
            approx.lookup_token_approx(line, col),
            expected_approx,
            "lookup_token_approx query #{i} ({line}, {col})"
        );
        assert_eq!(
            exact.lookup_token(line, col),
            expected_exact,
            "lookup_token query #{i} ({line}, {col})"
        );
        let (actual, expected) = if i % 2 == 0 {
            (mixed.lookup_token_approx(line, col), expected_approx)
        } else {
            (mixed.lookup_token(line, col), expected_exact)
        };
        assert_eq!(actual, expected, "mixed query #{i} ({line}, {col})");
    }
}

/// Queries every column from `from` to `to` on `line`, ascending or descending.
fn sweep(line: u32, from: u32, to: u32) -> Vec<(u32, u32)> {
    if from <= to {
        (from..=to).map(|col| (line, col)).collect()
    } else {
        (to..=from).rev().map(|col| (line, col)).collect()
    }
}

fn fixture() -> SourceMap<'static> {
    map_from_lines(&[
        // Duplicate columns, including a run longer than the forward scan.
        vec![3, 3, 3, 5, 7, 7, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 12, 20],
        // Empty line.
        vec![],
        // A single token at column 0.
        vec![0],
        // Columns up to `u32::MAX`.
        vec![1, u32::MAX - 1, u32::MAX, u32::MAX],
        // Many evenly spaced tokens, for jumps past the forward scan.
        (0..64).map(|i| 4 + i * 3).collect(),
        // Trailing empty line.
        vec![],
        // A line that starts late.
        vec![100, 101, 101, 200],
    ])
}

#[test]
fn sequential_queries() {
    let map = fixture();
    for line in 0..8 {
        check(&map, &sweep(line, 0, 220));
    }
}

#[test]
fn backward_queries() {
    let map = fixture();
    for line in 0..8 {
        check(&map, &sweep(line, 220, 0));
    }
}

#[test]
fn repeated_queries() {
    let map = fixture();
    let queries: Vec<_> =
        [(0, 3), (0, 10), (0, 11), (0, 0), (4, 50), (3, u32::MAX), (6, 101), (6, 99)]
            .into_iter()
            .flat_map(|query| [query; 4])
            .collect();
    check(&map, &queries);
}

#[test]
fn duplicate_columns() {
    let map = fixture();
    // Land inside a run of duplicates from before, inside and after it.
    check(&map, &[(0, 10), (0, 10), (0, 9), (0, 10), (0, 11), (0, 10), (0, 4), (0, 3), (0, 3)]);
    check(&map, &[(0, 12), (0, 10), (0, 7), (0, 3), (0, 7), (0, 10), (0, 12)]);
    check(&map, &[(6, 101), (6, 101), (6, 100), (6, 101), (6, 150), (6, 101)]);
}

#[test]
fn before_first_token() {
    let map = fixture();
    check(&map, &[(0, 0), (0, 2), (0, 3), (0, 0), (6, 0), (6, 99), (6, 100), (6, 50)]);
    check(&map, &[(4, 0), (4, 3), (4, 4), (4, 1), (4, 200), (4, 0)]);
}

#[test]
fn empty_and_missing_lines() {
    let map = fixture();
    check(&map, &[(1, 0), (1, 5), (0, 5), (1, 5), (5, 0), (7, 0), (8, 0), (u32::MAX, 0)]);
    check(&map, &[(0, 7), (u32::MAX, u32::MAX), (0, 7), (100, 3), (0, 8)]);

    let empty = SourceMap::default();
    let mut cursor = empty.lookup_cursor();
    assert_eq!(cursor.lookup_token_approx(0, 0), None);
    assert_eq!(cursor.lookup_token(0, 0), None);
    assert_eq!(cursor.lookup_token_approx(u32::MAX, u32::MAX), None);
}

#[test]
fn max_columns() {
    let map = fixture();
    check(
        &map,
        &[
            (3, u32::MAX),
            (3, u32::MAX - 1),
            (3, u32::MAX - 2),
            (3, u32::MAX),
            (3, 0),
            (3, u32::MAX),
            (0, u32::MAX),
            (4, u32::MAX),
            (6, u32::MAX),
        ],
    );
}

#[test]
fn distant_jumps() {
    let map = fixture();
    // Line 4 has tokens at 4, 7, 10, ..., 193. Steps of 30 columns skip 10 tokens.
    let forward: Vec<_> = (0..8).map(|i| (4, i * 30)).collect();
    check(&map, &forward);
    let backward: Vec<_> = forward.iter().rev().copied().collect();
    check(&map, &backward);
    // Alternate between the ends of the line and across lines.
    check(&map, &[(4, 4), (4, 193), (4, 5), (4, 192), (0, 20), (4, 100), (4, 31), (0, 3)]);
    // Jumps of exactly the forward scan length and one more.
    check(&map, &[(4, 4), (4, 4 + 8 * 3), (4, 4 + 16 * 3 + 1), (4, 4 + 25 * 3), (4, 4 + 33 * 3)]);
}

/// A small xorshift generator, so the randomized test is deterministic without a dependency.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A value in `0..n`.
    fn below(&mut self, n: u32) -> u32 {
        (self.next() % u64::from(n)) as u32
    }
}

fn random_map(rng: &mut XorShift) -> Vec<Vec<u32>> {
    let line_count = rng.below(6);
    (0..line_count)
        .map(|_| {
            // Narrow column ranges force duplicates; wide ones force long jumps.
            let token_count = rng.below(40);
            let col_range = [4, 40, 1000][rng.below(3) as usize];
            let mut cols: Vec<u32> = (0..token_count)
                .map(|_| match rng.below(20) {
                    0 => u32::MAX,
                    1 => u32::MAX - 1,
                    _ => rng.below(col_range),
                })
                .collect();
            cols.sort_unstable();
            cols
        })
        .collect()
}

fn random_queries(rng: &mut XorShift, lines: &[Vec<u32>], count: usize) -> Vec<(u32, u32)> {
    let line_count = lines.len() as u32;
    let mut queries = Vec::with_capacity(count);
    let (mut line, mut col) = (0u32, 0u32);
    for _ in 0..count {
        match rng.below(10) {
            // Small forward step, the common case.
            0..=3 => col = col.saturating_add(rng.below(4)),
            // Repeat the previous query.
            4 => {}
            // Small backward step.
            5 => col = col.saturating_sub(rng.below(4)),
            // Distant jump within the line.
            6 => col = rng.below(1100),
            // Land on, just before or just after an existing token.
            7 => {
                if let Some(cols) = lines.get(line as usize).filter(|cols| !cols.is_empty()) {
                    let target = cols[rng.below(cols.len() as u32) as usize];
                    col = match rng.below(3) {
                        0 => target.saturating_sub(1),
                        1 => target,
                        _ => target.saturating_add(1),
                    };
                }
            }
            // Extreme columns.
            8 => col = if rng.below(2) == 0 { 0 } else { u32::MAX - rng.below(2) },
            // Another line, possibly missing.
            _ => {
                line = rng.below(line_count + 2);
                col = rng.below(1100);
            }
        }
        queries.push((line, col));
    }
    queries
}

#[test]
fn randomized_queries_match_stateless_lookups() {
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    for _ in 0..2_000 {
        let lines = random_map(&mut rng);
        let map = map_from_lines(&lines);
        let queries = random_queries(&mut rng, &lines, 300);
        check(&map, &queries);
    }
}

#[test]
fn randomized_compose_matches_stateless_lookups() {
    let mut rng = XorShift(0x2545_F491_4F6C_DD1D);
    for _ in 0..500 {
        let lines = random_map(&mut rng);
        let input = map_from_lines(&lines);
        let queries = random_queries(&mut rng, &lines, 100);

        let mut generated = SourceMapBuilder::default();
        let source = generated.add_source_and_content("intermediate.js", "");
        for (i, &(line, col)) in queries.iter().enumerate() {
            let source = (rng.below(8) != 0).then_some(source);
            generated.add_token(0, i as u32, line, col, source, None);
        }
        let generated = generated.into_sourcemap();

        let table = input.generate_lookup_table();
        let expected: Vec<Token> = generated
            .get_tokens()
            .map(|token| {
                let original = token
                    .get_source_id()
                    .and_then(|_| {
                        input.lookup_token_approx(&table, token.get_src_line(), token.get_src_col())
                    })
                    .filter(|original| original.get_source_id().is_some());
                match original {
                    Some(original) => Token::new(
                        token.get_dst_line(),
                        token.get_dst_col(),
                        original.get_src_line(),
                        original.get_src_col(),
                        original.get_source_id(),
                        None,
                    ),
                    None => Token::new(token.get_dst_line(), token.get_dst_col(), 0, 0, None, None),
                }
            })
            .collect();

        let composed = generated.compose(input.clone());
        assert_eq!(composed.get_tokens().collect::<Vec<_>>(), expected);
    }
}
