<div align="center">

[![Crates.io][crates-badge]][crates-url]
[![Docs.rs][docs-badge]][docs-url]
[![BSD-3-Clause licensed][license-badge]][license-url]
[![Build Status][ci-badge]][ci-url]
[![Discord chat][discord-badge]][discord-url]

</div>

# oxc-sourcemap

Forked version of [rust-sourcemap](https://github.com/getsentry/rust-sourcemap), modified for Oxc.

## Migrating to v10

Source names now preserve JSON `null` entries as `None`. Wrap known names in `Some` when using
`SourceMap::new`, `SourceMapParts::sources`, `JSONSourceMap::sources`, or `napi::SourceMap::sources`.
For both `SourceMap` and `OwnedSourceMap`, `set_sources` accepts optional names and `get_sources`
returns an iterator of `Option<&str>`:

```rust
use oxc_sourcemap::SourceMap;

let mut map = SourceMap::default();
map.set_sources([Some("input.js"), None]);
let sources: Vec<Option<&str>> = map.get_sources().collect();
```

Preserve `None` entries when rebuilding the source list: their positions are source IDs.
A mapped token can have an unknown source name; use `get_source_id()` to distinguish it from an unmapped token.

## Benchmarking

Run benchmarks with:

```bash
cargo bench --benches
# Run one operation group:
cargo bench --bench lookup_token
```

Operation groups run in separate binaries to reduce benchmark interference. Fixtures are in
`tests/fixtures/perf/*.map`.

[discord-badge]: https://img.shields.io/discord/1079625926024900739?logo=discord&label=Discord
[discord-url]: https://discord.gg/9uXCAwqQZW
[license-badge]: https://img.shields.io/badge/license-BSD--3--Clause-blue.svg
[license-url]: https://github.com/oxc-project/oxc-sourcemap/blob/main/LICENSE
[ci-badge]: https://github.com/oxc-project/oxc-sourcemap/actions/workflows/ci.yml/badge.svg?event=push&branch=main
[ci-url]: https://github.com/oxc-project/oxc-sourcemap/actions/workflows/ci.yml?query=event%3Apush+branch%3Amain
[crates-badge]: https://img.shields.io/crates/d/oxc_sourcemap?label=crates.io
[crates-url]: https://crates.io/crates/oxc_sourcemap
[docs-badge]: https://img.shields.io/docsrs/oxc_sourcemap
[docs-url]: https://docs.rs/oxc_sourcemap

# [Sponsored By](https://oxc.rs/sponsor)

<p align="center">
  <a href="https://oxc.rs/sponsor">
    <img src="https://raw.githubusercontent.com/oxc-project/sponsors/main/sponsors.svg" alt="Our sponsors" />
  </a>
</p>
