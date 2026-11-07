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
