from pathlib import Path
import os
if os.environ['BENCH_MODE'] == 'walltime':
    p = Path('Cargo.toml')
    t = p.read_text().replace(
        'criterion2 = { version = "3.0.4", default-features = false }',
        'criterion = { package = "codspeed-criterion-compat", version = "=6.0.0", default-features = false }',
    ).replace('codspeed = ["criterion2/codspeed"]', 'codspeed = []')
    p.write_text(t)
p = Path('benches/simple.rs')
t = p.read_text().replace(
    'pub fn bench(c: &mut Criterion) {',
    'pub fn bench(c: &mut Criterion) {\n    let label = std::env::var("BENCH_LABEL").expect("paired validation label");',
)
for group in ['parse', 'serialize', 'lookup_table', 'lookup_token', 'concat']:
    t = t.replace(f'c.benchmark_group("{group}")', f'c.benchmark_group(format!("{{label}}/{group}"))')
for name in ['smoke/SourceMap::from_json_string_inline', 'builder/SourceMapBuilder::build_single']:
    t = t.replace(f'c.bench_function("{name}"', f'c.bench_function(&format!("{{label}}/{name}")')
p.write_text(t)
# Supplement the unchanged upstream cases with the nameless shape emitted by Oxc codegen.
p = Path('benches/simple.rs')
t = p.read_text()
extra = r'''
    // Validation-only variant: retain the real fixture coordinates and source ids,
    // while omitting optional name annotations. Setup is outside the measured loops.
    let nameless_maps: Vec<_> = parsed_fixtures.iter().map(|(_, _, map)| {
        let mut parts = map.clone().into_parts();
        parts.names.clear();
        parts.tokens = parts.tokens.iter().map(|token| oxc_sourcemap::Token::new(
            token.get_dst_line(), token.get_dst_col(), token.get_src_line(), token.get_src_col(),
            token.get_source_id(), None,
        )).collect();
        parts.token_chunks = None;
        SourceMap::from_parts(parts)
    }).collect();
    let mut offset = 0;
    let nameless_inputs: Vec<_> = nameless_maps.iter().map(|map| {
        let entry = (map, offset);
        offset += token_line_span(map);
        entry
    }).collect();
    let mut nameless_group = c.benchmark_group(format!("{label}/concat_nameless"));
    nameless_group.throughput(Throughput::Elements(nameless_maps.iter().map(|map| map.get_tokens().len() as u64).sum()));
    nameless_group.bench_function("from_sourcemaps", |b| {
        b.iter(|| {
            black_box(ConcatSourceMapBuilder::from_sourcemaps(black_box(&nameless_inputs)).into_sourcemap());
        });
    });
    nameless_group.bench_function("add_sourcemap_loop", |b| {
        b.iter(|| {
            let mut builder = ConcatSourceMapBuilder::default();
            for &(map, offset) in black_box(&nameless_inputs) { builder.add_sourcemap(map, offset); }
            black_box(builder.into_sourcemap());
        });
    });
    nameless_group.finish();
'''
assert t.count('    concat_group.finish();') == 1
t = t.replace('    concat_group.finish();', '    concat_group.finish();\n' + extra)
p.write_text(t)
p = Path('benches/simple.rs')
t = p.read_text()
extra = r'''
    // Worst-case guard: neither source nor name ids occur anywhere in a chunk.
    let unmapped_maps: Vec<_> = nameless_maps.iter().map(|map| {
        let mut parts = map.clone().into_parts();
        parts.tokens = parts.tokens.iter().map(|token| oxc_sourcemap::Token::new(
            token.get_dst_line(), token.get_dst_col(), token.get_src_line(), token.get_src_col(),
            None, None,
        )).collect();
        parts.token_chunks = None;
        SourceMap::from_parts(parts)
    }).collect();
    let mut offset = 0;
    let unmapped_inputs: Vec<_> = unmapped_maps.iter().map(|map| {
        let entry = (map, offset);
        offset += token_line_span(map);
        entry
    }).collect();
    let mut unmapped_group = c.benchmark_group(format!("{label}/concat_unmapped"));
    unmapped_group.throughput(Throughput::Elements(unmapped_maps.iter().map(|map| map.get_tokens().len() as u64).sum()));
    unmapped_group.bench_function("from_sourcemaps", |b| {
        b.iter(|| black_box(ConcatSourceMapBuilder::from_sourcemaps(black_box(&unmapped_inputs)).into_sourcemap()));
    });
    unmapped_group.bench_function("add_sourcemap_loop", |b| {
        b.iter(|| {
            let mut builder = ConcatSourceMapBuilder::default();
            for &(map, offset) in black_box(&unmapped_inputs) { builder.add_sourcemap(map, offset); }
            black_box(builder.into_sourcemap());
        });
    });
    unmapped_group.finish();
'''
assert t.count('    nameless_group.finish();') == 1
t = t.replace('    nameless_group.finish();', '    nameless_group.finish();\n' + extra)
p.write_text(t)
