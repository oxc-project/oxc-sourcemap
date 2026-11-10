use std::time::Duration;

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use oxc_sourcemap::SourceMapBuilder;

#[cfg(feature = "codspeed")]
#[path = "common/allocator.rs"]
mod allocator;

pub fn bench(c: &mut Criterion) {
    c.bench_function("builder/SourceMapBuilder::build_single", |b| {
        b.iter(|| {
            let mut builder = SourceMapBuilder::default();
            let name_id = builder.add_name(black_box("foo"));
            let source_id =
                builder.add_source_and_content(black_box("test.js"), black_box("var x = 1;"));
            builder.add_token(0, 0, 0, 0, Some(source_id), Some(name_id));
            let sourcemap = builder.into_sourcemap();
            black_box(sourcemap);
        });
    });
}

criterion_group!(
    name = sourcemap;
    config = Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = bench
);
criterion_main!(sourcemap);
