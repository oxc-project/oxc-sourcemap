use std::hint::black_box;

use oxc_sourcemap::SourceMap;

fn main() {
    let input =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/perf/real_small.map"));
    let map = SourceMap::from_json_string(input).unwrap();
    let table = map.generate_lookup_table();
    assert!(!table.is_empty());
    assert_eq!(table.iter().map(|line| line.len()).sum::<usize>(), map.get_tokens().len());
    let checksum = table.iter().flat_map(|line| line.iter()).fold(0u64, |value, token| {
        value
            .wrapping_mul(31)
            .wrapping_add(u64::from(token.get_dst_line()))
            .wrapping_mul(31)
            .wrapping_add(u64::from(token.get_dst_col()))
            .wrapping_mul(31)
            .wrapping_add(u64::from(token.get_src_line()))
            .wrapping_mul(31)
            .wrapping_add(u64::from(token.get_src_col()))
    });
    println!("tokens={} lines={} checksum={checksum}", map.get_tokens().len(), table.len());
    drop(table);
    for _ in 0..black_box(1_000_000) {
        black_box(black_box(&map).generate_lookup_table());
    }
}
