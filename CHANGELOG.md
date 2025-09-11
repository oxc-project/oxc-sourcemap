# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [4.1.1](https://github.com/oxc-project/oxc-sourcemap/compare/v4.1.0...v4.1.1) - 2025-09-11

### Other

- _(sourcemap)_ optimize escape_json_string to avoid serde overhead ([#141](https://github.com/oxc-project/oxc-sourcemap/pull/141))

## [4.1.0](https://github.com/oxc-project/oxc-sourcemap/compare/v4.0.5...v4.1.0) - 2025-08-18

### Added

- add `SourcemapVisualizer::get_url` method ([#126](https://github.com/oxc-project/oxc-sourcemap/pull/126))

## [4.0.5](https://github.com/oxc-project/oxc-sourcemap/compare/v4.0.4...v4.0.5) - 2025-08-03

### Other

- make `Token` Copy ([#108](https://github.com/oxc-project/oxc-sourcemap/pull/108))
- change some APIs to return `&Arc<str>` ([#107](https://github.com/oxc-project/oxc-sourcemap/pull/107))

## [4.0.4](https://github.com/oxc-project/oxc-sourcemap/compare/v4.0.3...v4.0.4) - 2025-07-31

### Fixed

- fix

### Other

- change some APIs to return `&Arc<str>` ([#105](https://github.com/oxc-project/oxc-sourcemap/pull/105))
- avoid string allocation in `SourceMapBuilder::add_name` ([#103](https://github.com/oxc-project/oxc-sourcemap/pull/103))
- add [bench] to Cargo.toml
- add benchmark ([#100](https://github.com/oxc-project/oxc-sourcemap/pull/100))

## [4.0.3](https://github.com/oxc-project/oxc-sourcemap/compare/v4.0.2...v4.0.3) - 2025-07-28

### Other

- remove outdated text from README ([#95](https://github.com/oxc-project/oxc-sourcemap/pull/95))
- _(deps)_ lock file maintenance ([#97](https://github.com/oxc-project/oxc-sourcemap/pull/97))
- _(justfile)_ add `dprint` ([#94](https://github.com/oxc-project/oxc-sourcemap/pull/94))
- add auto format ([#92](https://github.com/oxc-project/oxc-sourcemap/pull/92))

# Changelog

All notable changes to this package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project does not adhere to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) until v1.0.0.

## [4.0.2](https://github.com/oxc-project/oxc-sourcemap/compare/v4.0.1...v4.0.2) - 2025-07-18

### Other

- return `&Arc<str>` instead `&str` for source content ([#88](https://github.com/oxc-project/oxc-sourcemap/pull/88))
- reduce size of token from 32 bytes to 24 ([#86](https://github.com/oxc-project/oxc-sourcemap/pull/86))

## [4.0.1](https://github.com/oxc-project/oxc-sourcemap/compare/v4.0.0...v4.0.1) - 2025-07-18

### Other

- _(deps)_ napi v3 ([#84](https://github.com/oxc-project/oxc-sourcemap/pull/84))

## [4.0.0](https://github.com/oxc-project/oxc-sourcemap/compare/v3.0.2...v4.0.0) - 2025-07-17

### Other

- _(deps)_ bump deps ([#83](https://github.com/oxc-project/oxc-sourcemap/pull/83))
- remove `rayon` feature ([#81](https://github.com/oxc-project/oxc-sourcemap/pull/81))

## [3.0.2](https://github.com/oxc-project/oxc-sourcemap/compare/v3.0.1...v3.0.2) - 2025-05-19

### Other

- napi beta

## [3.0.1](https://github.com/oxc-project/oxc-sourcemap/compare/v3.0.0...v3.0.1) - 2025-05-10

### Fixed

- sources_content should be Vec<Option<Arc<str>>> ([#50](https://github.com/oxc-project/oxc-sourcemap/pull/50))

## [3.0.0](https://github.com/oxc-project/oxc-sourcemap/compare/v2.0.2...v3.0.0) - 2025-03-03

### Added

- support `x_google_ignoreList` in more places ([#30](https://github.com/oxc-project/oxc-sourcemap/pull/30))

## [2.0.2](https://github.com/oxc-project/oxc-sourcemap/compare/v2.0.1...v2.0.2) - 2025-02-22

### Other

- Rust Edition 2024 ([#24](https://github.com/oxc-project/oxc-sourcemap/pull/24))

## [2.0.1](https://github.com/oxc-project/oxc-sourcemap/compare/v2.0.0...v2.0.1) - 2025-02-21

### Other

- include build.rs

## [2.0.0](https://github.com/oxc-project/oxc-sourcemap/compare/v1.0.7...v2.0.0) - 2025-02-21

### Fixed

- broken cargo features

## [1.0.7](https://github.com/oxc-project/oxc-sourcemap/compare/v1.0.6...v1.0.7) - 2025-02-11

### Fixed

- add napi build.rs (#19)

## [1.0.6](https://github.com/oxc-project/oxc-sourcemap/compare/v1.0.5...v1.0.6) - 2024-12-15

### Fixed

- handle non existing token position in visualizer (#14)

## [1.0.5](https://github.com/oxc-project/oxc-sourcemap/compare/v1.0.4...v1.0.5) - 2024-12-11

### Fixed

- _(lookup_token)_ should be None if original tokens hasn't the line (#9)

## [1.0.4](https://github.com/oxc-project/oxc-sourcemap/compare/v1.0.3...v1.0.4) - 2024-12-10

### Fixed

- fix wrong source id when concatenating empty source map (#7)

### Other

- update README

## [1.0.3](https://github.com/oxc-project/oxc-sourcemap/compare/v1.0.2...v1.0.3) - 2024-12-03

### Other

- rename feature `concurrent` to `rayon`

## [1.0.2](https://github.com/oxc-project/oxc-sourcemap/compare/v1.0.1...v1.0.2) - 2024-12-03

### Other

- `pub mod napi`

## [1.0.1](https://github.com/oxc-project/oxc-sourcemap/compare/v1.0.0...v1.0.1) - 2024-12-03

### Other

- remove unused lint
- add `napi` feature

## [0.37.0] - 2024-11-21

### Bug Fixes

- 3d66929 sourcemap: Improve source map visualizer (#7386) (Hiroshi Ogawa)

## [0.31.0] - 2024-10-08

### Features

- f6e42b6 sourcemap: Add support for sourcemap debug IDs (#6221) (Tim Fish)

## [0.30.4] - 2024-09-28

### Bug Fixes

- 6f98aad sourcemap: Align sourcemap type with Rollup (#6133) (Boshen)

## [0.29.0] - 2024-09-13

### Performance

- d18c896 rust: Use `cow_utils` instead (#5664) (dalaoshu)

## [0.28.0] - 2024-09-11

### Documentation

- fefbbc1 sourcemap: Add trailing newline to README (#5539) (overlookmotel)

## [0.24.3] - 2024-08-18

### Refactor

- 5fd1701 sourcemap: Lower the `msrv`. (#4873) (rzvxa)

## [0.24.0] - 2024-08-08

### Features

- e42ac3a sourcemap: Add `ConcatSourceMapBuilder::from_sourcemaps` (#4639) (overlookmotel)

### Performance

- ff43dff sourcemap: Speed up VLQ encoding (#4633) (overlookmotel)
- a330773 sourcemap: Reduce string copying in `ConcatSourceMapBuilder` (#4638) (overlookmotel)
- 372316b sourcemap: `ConcatSourceMapBuilder` extend `source_contents` in separate loop (#4634) (overlookmotel)
- c7f1d48 sourcemap: Keep local copy of previous token in VLQ encode (#4596) (overlookmotel)
- 590d795 sourcemap: Shorten main loop encoding VLQ (#4586) (overlookmotel)

## [0.23.1] - 2024-08-06

### Features

- e42ac3a sourcemap: Add `ConcatSourceMapBuilder::from_sourcemaps` (#4639) (overlookmotel)

### Performance

- ff43dff sourcemap: Speed up VLQ encoding (#4633) (overlookmotel)
- a330773 sourcemap: Reduce string copying in `ConcatSourceMapBuilder` (#4638) (overlookmotel)
- 372316b sourcemap: `ConcatSourceMapBuilder` extend `source_contents` in separate loop (#4634) (overlookmotel)
- c7f1d48 sourcemap: Keep local copy of previous token in VLQ encode (#4596) (overlookmotel)
- 590d795 sourcemap: Shorten main loop encoding VLQ (#4586) (overlookmotel)

## [0.23.0] - 2024-08-01

- 27fd062 sourcemap: [**BREAKING**] Avoid passing `Result`s (#4541) (overlookmotel)

### Performance

- d00014e sourcemap: Elide bounds checks in VLQ encoding (#4583) (overlookmotel)
- 1fd9dd0 sourcemap: Use simd to escape JSON string (#4487) (Brooooooklyn)

### Refactor

- 7c42ffc sourcemap: Align Base64 chars lookup table to cache line (#4535) (overlookmotel)

## [0.22.1] - 2024-07-27

### Bug Fixes

- 5db7bed sourcemap: Fix pre-calculation of required segments for building JSON (#4490) (overlookmotel)

### Performance

- 705e19f sourcemap: Reduce memory copies encoding JSON (#4489) (overlookmotel)
- 4d10c6c sourcemap: Pre allocate String buf while encoding (#4476) (Brooooooklyn)

### Refactor

- c958a55 sourcemap: `push_list` method for building JSON (#4486) (overlookmotel)

## [0.22.0] - 2024-07-23

### Bug Fixes

- 4cd5df0 sourcemap: Avoid negative line if token_chunks has same prev_dst_line (#4348) (underfin)

## [0.21.0] - 2024-07-18

### Features

- 205c259 sourcemap: Support SourceMapBuilder#token_chunks (#4220) (underfin)

## [0.16.0] - 2024-06-26

### Features

- 01572f0 sourcemap: Impl `std::fmt::Display` for `Error` (#3902) (DonIsaac)- d3cd3ea Oxc transform binding (#3896) (underfin)

## [0.13.1] - 2024-05-22

### Features

- 90d2d09 sourcemap: Add Sourcemap#from_json method (#3361) (underfin)

### Bug Fixes

- 899a52b Fix some nightly warnings (Boshen)

## [0.13.0] - 2024-05-14

### Features

- f6daf0b sourcemap: Add feature "sourcemap_concurrent" (Boshen)
- 7363e14 sourcemap: Add "rayon" feature (#3198) (Boshen)

## [0.12.3] - 2024-04-11

### Features

- 8662f4f sourcemap: Add x_google_ignoreList (#2928) (underfin)
- 5cb3991 sourcemap: Add sourceRoot (#2926) (underfin)

## [0.12.2] - 2024-04-08

### Features

- 96f02e6 sourcemap: Optional JSONSourceMap fields (#2910) (underfin)
- d87cf17 sourcemap: Add methods to mutate SourceMap (#2909) (underfin)
- 74aca1c sourcemap: Add SourceMapBuilder file (#2908) (underfin)

## [0.12.1] - 2024-04-03

### Bug Fixes

- 28fae2e sourcemap: Using serde_json::to_string to quote sourcemap string (#2889) (underfin)

## [0.11.0] - 2024-03-30

### Features

- b199cb8 Add oxc sourcemap crate (#2825) (underfin)

### Bug Fixes

- 6177c2f codegen: Sourcemap token name should be original name (#2843) (underfin)
