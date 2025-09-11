use criterion::{Criterion, black_box, criterion_group, criterion_main};
use oxc_sourcemap::SourceMapBuilder;

fn bench_add_name_with_duplicates(c: &mut Criterion) {
    c.bench_function("add_name_with_50%_duplicates", |b| {
        b.iter(|| {
            let mut builder = SourceMapBuilder::default();

            // Add names with 50% duplicates to test HashMap efficiency
            for i in 0..1000 {
                let name = if i % 2 == 0 {
                    format!("duplicate_name_{}", i % 100) // Many duplicates
                } else {
                    format!("unique_name_{}", i) // Unique names
                };
                black_box(builder.add_name(&name));
            }

            builder
        });
    });
}

fn bench_add_name_all_unique(c: &mut Criterion) {
    c.bench_function("add_name_all_unique", |b| {
        b.iter(|| {
            let mut builder = SourceMapBuilder::default();

            // All unique names - tests Arc creation efficiency
            for i in 0..1000 {
                let name = format!("unique_name_{}", i);
                black_box(builder.add_name(&name));
            }

            builder
        });
    });
}

fn bench_add_name_all_duplicates(c: &mut Criterion) {
    c.bench_function("add_name_all_duplicates", |b| {
        b.iter(|| {
            let mut builder = SourceMapBuilder::default();

            // All duplicates - tests lookup efficiency
            for i in 0..1000 {
                let name = format!("duplicate_name_{}", i % 10); // Only 10 unique names
                black_box(builder.add_name(&name));
            }

            builder
        });
    });
}

fn bench_add_source_and_content_with_duplicates(c: &mut Criterion) {
    c.bench_function("add_source_and_content_with_duplicates", |b| {
        b.iter(|| {
            let mut builder = SourceMapBuilder::default();

            // Mix of duplicate and unique sources
            for i in 0..500 {
                let source = if i % 3 == 0 {
                    format!("common_file_{}.js", i % 20) // Duplicates
                } else {
                    format!("unique_file_{}.js", i) // Unique
                };
                let content = format!("const var{} = {};", i, i);
                black_box(builder.add_source_and_content(&source, &content));
            }

            builder
        });
    });
}

fn bench_large_sourcemap_building(c: &mut Criterion) {
    c.bench_function("large_sourcemap_building", |b| {
        b.iter(|| {
            let mut builder = SourceMapBuilder::default();

            // Simulate building a large sourcemap with realistic patterns
            for i in 0..2000 {
                // Add source files (some duplicates, representing shared libraries)
                let source = if i % 10 == 0 {
                    format!("shared_lib_{}.js", i % 5)
                } else {
                    format!("module_{}.js", i)
                };
                let content = format!("// Content for {}", source);
                let source_id = builder.add_source_and_content(&source, &content);

                // Add variable names (high duplication, representing common names)
                let name = if i % 4 == 0 {
                    format!("common_var_{}", i % 50) // Common variable names
                } else {
                    format!("var_{}", i)
                };
                let name_id = builder.add_name(&name);

                // Add token
                builder.add_token(
                    i % 100,       // dst_line
                    (i * 7) % 80,  // dst_col
                    i % 50,        // src_line
                    (i * 3) % 100, // src_col
                    Some(source_id),
                    Some(name_id),
                );
            }

            black_box(builder.into_sourcemap())
        });
    });
}

criterion_group!(
    benches,
    bench_add_name_with_duplicates,
    bench_add_name_all_unique,
    bench_add_name_all_duplicates,
    bench_add_source_and_content_with_duplicates,
    bench_large_sourcemap_building
);
criterion_main!(benches);
