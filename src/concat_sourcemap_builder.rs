use std::borrow::Cow;

use crate::{Mappings, SourceMap};

/// The `ConcatSourceMapBuilder` is a helper to concat sourcemaps.
///
/// Names/sources/sourcesContent are accumulated as [`Cow<'a, str>`], so the builder serves two
/// input shapes without ever copying string bytes during the join:
///
/// * [`add_sourcemap`](Self::add_sourcemap) / [`from_sourcemaps`](Self::from_sourcemaps) **borrow**
///   from input maps that outlive the builder (`Cow::Borrowed`).
/// * [`add_sourcemap_owned`](Self::add_sourcemap_owned) /
///   [`from_owned_sourcemaps`](Self::from_owned_sourcemaps) **consume** owned input maps and
///   **move** their strings in (`Cow::Owned`) — no copy, ideal when the inputs are discarded
///   afterwards (e.g. a bundler joining freshly-rendered modules).
///
/// The mappings are joined as bytes: see [`Mappings::append`]. Tokens are not decoded, and the
/// result is as compact as its inputs.
///
/// The ownership decision is deferred to the end:
/// * [`into_sourcemap`](Self::into_sourcemap) moves the accumulated entries straight into a
///   [`SourceMap<'a>`] — zero copy either way.
/// * [`into_owned_sourcemap`](Self::into_owned_sourcemap) detaches to `'static`: entries moved in
///   via `add_sourcemap_owned` are kept as-is, only borrowed ones are copied.
#[derive(Debug, Default)]
pub struct ConcatSourceMapBuilder<'a> {
    pub(crate) names: Vec<Cow<'a, str>>,
    pub(crate) sources: Vec<Option<Cow<'a, str>>>,
    pub(crate) source_contents: Vec<Option<Cow<'a, str>>>,
    pub(crate) mappings: Mappings,
}

impl<'a> ConcatSourceMapBuilder<'a> {
    /// Create new `ConcatSourceMapBuilder` with pre-allocated capacity.
    ///
    /// Allocating capacity before adding sourcemaps with `add_sourcemap` avoids memory copies
    /// and increases performance. `mappings_byte_len` is the sum of
    /// [`Mappings::byte_len`] of the maps to add.
    ///
    /// Alternatively, use `from_sourcemaps`.
    pub fn with_capacity(names_len: usize, sources_len: usize, mappings_byte_len: usize) -> Self {
        Self {
            names: Vec::with_capacity(names_len),
            sources: Vec::with_capacity(sources_len),
            source_contents: Vec::with_capacity(sources_len),
            // A token at the start of each map may be written again, a few bytes longer.
            mappings: Mappings::with_byte_capacity(mappings_byte_len + mappings_byte_len / 64),
        }
    }

    /// Sum the `names` / `sources` lengths and the mappings bytes across `maps`, to pre-size
    /// the builder.
    fn sum_lengths<'s, 'd: 's>(
        maps: impl Iterator<Item = &'s SourceMap<'d>>,
    ) -> (usize, usize, usize) {
        maps.fold((0, 0, 0), |(names, sources, bytes), map| {
            (
                names + map.names.len(),
                sources + map.sources.len(),
                bytes + map.mappings.byte_len() + 16,
            )
        })
    }

    /// Pad `source_contents` with `None` so it stays index-aligned with `sources`. An input map's
    /// `sourcesContent` may be absent or shorter than its `sources` (it is not normalized on
    /// decode), and contents are indexed by source id — without this, a later map's content would
    /// shift onto an earlier map's source. A no-op when the map's contents already cover its
    /// sources (the common case), so it costs nothing there.
    fn pad_source_contents(&mut self) {
        self.source_contents.resize(self.sources.len(), None);
    }

    /// Create new `ConcatSourceMapBuilder` from an array of `SourceMap`s and line offsets.
    ///
    /// This avoids memory copies versus creating builder with `ConcatSourceMapBuilder::default()`
    /// and then adding sourcemaps individually with `add_sourcemap`.
    ///
    /// # Example
    /// ```no_run
    /// # use oxc_sourcemap::ConcatSourceMapBuilder;
    /// # use oxc_sourcemap::SourceMap;
    /// # let sourcemap1 = SourceMap::default();
    /// # let sourcemap2 = SourceMap::default();
    /// let builder = ConcatSourceMapBuilder::from_sourcemaps(&[
    ///   (&sourcemap1, 0),
    ///   (&sourcemap2, 100),
    /// ]);
    /// let combined_sourcemap = builder.into_sourcemap();
    /// ```
    pub fn from_sourcemaps(sourcemap_and_line_offsets: &[(&'a SourceMap<'_>, u32)]) -> Self {
        let (names_len, sources_len, bytes_len) =
            Self::sum_lengths(sourcemap_and_line_offsets.iter().map(|(sourcemap, _)| *sourcemap));
        let mut builder = Self::with_capacity(names_len, sources_len, bytes_len);

        for &(sourcemap, line_offset) in sourcemap_and_line_offsets {
            builder.add_sourcemap(sourcemap, line_offset);
        }

        builder
    }

    /// Create a `ConcatSourceMapBuilder` from **owned** `SourceMap`s and line offsets, **moving**
    /// their strings in (no copy).
    ///
    /// The owned counterpart of [`from_sourcemaps`](Self::from_sourcemaps) — see
    /// [`add_sourcemap_owned`](Self::add_sourcemap_owned). This is the fastest way to join owned
    /// maps that are discarded afterwards (e.g. a bundler joining rendered modules): no
    /// name/source/sourcesContent string is copied.
    pub fn from_owned_sourcemaps(sourcemap_and_line_offsets: Vec<(SourceMap<'a>, u32)>) -> Self {
        let (names_len, sources_len, bytes_len) =
            Self::sum_lengths(sourcemap_and_line_offsets.iter().map(|(sourcemap, _)| sourcemap));
        let mut builder = Self::with_capacity(names_len, sources_len, bytes_len);

        for (sourcemap, line_offset) in sourcemap_and_line_offsets {
            builder.add_sourcemap_owned(sourcemap, line_offset);
        }

        builder
    }

    /// Add a **borrowed** `SourceMap`'s entries to the concatenation, offset by `line_offset`.
    ///
    /// Strings are borrowed from `sourcemap` for `'a` (no allocation), so the result cannot
    /// outlive it. Use [`add_sourcemap_owned`](Self::add_sourcemap_owned) when you own the map and
    /// want to move its strings in instead.
    pub fn add_sourcemap(&mut self, sourcemap: &'a SourceMap<'_>, line_offset: u32) {
        let source_offset = self.sources.len() as u32;
        let name_offset = self.names.len() as u32;

        // Borrow strings directly from the input map — no allocations. The output `SourceMap`'s
        // lifetime is tied to `'a`, so the borrow checker enforces that input maps outlive it.
        self.sources.extend(sourcemap.get_sources().map(|source| source.map(Cow::Borrowed)));
        self.source_contents
            .extend(sourcemap.get_source_contents().map(|content| content.map(Cow::Borrowed)));
        self.pad_source_contents();
        self.names.extend(sourcemap.get_names().map(Cow::Borrowed));

        self.mappings.append(&sourcemap.mappings, line_offset, source_offset, name_offset);
    }

    /// Add an **owned** `SourceMap` to the concatenation, **moving** its strings in (no copy),
    /// offset by `line_offset`.
    ///
    /// This is the fastest path when the inputs are owned and discarded after the join: the
    /// name/source/sourcesContent `Cow::Owned` heap buffers are moved across rather than copied,
    /// so the cost is independent of how much `sourcesContent` rides along. Pair with
    /// [`into_sourcemap`](Self::into_sourcemap), which then moves the entries straight into the
    /// result (and, for `'static` inputs, returns a `'static` map without any copy).
    pub fn add_sourcemap_owned(&mut self, sourcemap: SourceMap<'a>, line_offset: u32) {
        let source_offset = self.sources.len() as u32;
        let name_offset = self.names.len() as u32;

        let parts = sourcemap.into_parts();

        // Move the owned entries in — no string bytes are copied.
        self.sources.extend(parts.sources);
        self.source_contents.extend(parts.source_contents);
        self.pad_source_contents();
        self.names.extend(parts.names);

        self.mappings.append(&parts.mappings, line_offset, source_offset, name_offset);
    }

    /// Finish, moving the accumulated names/sources/contents straight into a [`SourceMap<'a>`]
    /// (zero copy — the `Cow` vectors are moved, not rebuilt).
    pub fn into_sourcemap(self) -> SourceMap<'a> {
        SourceMap::new(None, self.names, None, self.sources, self.source_contents, self.mappings)
    }

    /// Same as [`Self::into_sourcemap`], but detaches to a `'static` [`crate::OwnedSourceMap`].
    /// Entries moved in via [`add_sourcemap_owned`](Self::add_sourcemap_owned) are kept as-is
    /// (no copy); only borrowed entries (from [`add_sourcemap`](Self::add_sourcemap)) are copied.
    #[inline]
    pub fn into_owned_sourcemap(self) -> crate::OwnedSourceMap {
        self.into_sourcemap().into_owned_sourcemap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Token;

    #[test]
    fn chunk_baselines_find_ids_independently_and_keep_them_across_unmapped_maps() {
        let mapped = SourceMap::new(
            None,
            vec![Cow::Borrowed("name"); 4],
            None,
            vec![Some(Cow::Borrowed("input.js")); 3],
            vec![],
            vec![
                Token::new(0, 0, 0, 0, Some(0), Some(0)),
                Token::new(1, 0, 0, 0, Some(0), Some(3)),
                Token::new(2, 0, 0, 0, Some(2), None),
                Token::new(3, 0, 0, 0, None, None),
            ],
        );
        let unmapped = SourceMap::new(
            None,
            vec![],
            None,
            vec![],
            vec![],
            vec![Token::new(0, 0, 0, 0, None, None)],
        );
        let last = SourceMap::new(
            None,
            vec![Cow::Borrowed("last")],
            None,
            vec![Some(Cow::Borrowed("last.js"))],
            vec![],
            vec![Token::new(0, 0, 0, 0, Some(0), Some(0))],
        );
        let mut builder = ConcatSourceMapBuilder::default();
        builder.add_sourcemap_owned(mapped, 0);
        builder.add_sourcemap_owned(unmapped, 10);
        builder.add_sourcemap_owned(SourceMap::default(), 20);
        builder.add_sourcemap_owned(last, 30);
        let map = builder.into_sourcemap();
        assert_eq!(
            map.get_tokens().collect::<Vec<_>>(),
            [
                Token::new(0, 0, 0, 0, Some(0), Some(0)),
                Token::new(1, 0, 0, 0, Some(0), Some(3)),
                Token::new(2, 0, 0, 0, Some(2), None),
                Token::unmapped(3, 0),
                Token::unmapped(10, 0),
                Token::new(30, 0, 0, 0, Some(3), Some(4)),
            ]
        );
        let json = map.to_json_string();
        let decoded = SourceMap::from_json_string(&json).unwrap();
        assert_eq!(decoded.mappings, map.mappings);
    }

    fn build_test_inputs() -> [SourceMap<'static>; 3] {
        [
            (vec!["foo", "foo2"], "foo.js", Token::new(1, 1, 1, 1, Some(0), Some(0))),
            (vec!["bar"], "bar.js", Token::new(1, 1, 1, 1, Some(0), Some(0))),
            (vec!["abc"], "abc.js", Token::new(1, 2, 2, 2, Some(0), Some(0))),
        ]
        .map(|(names, source, token)| SourceMap {
            names: names.into_iter().map(Cow::Borrowed).collect(),
            sources: vec![Some(source.into())],
            mappings: Mappings::from_tokens([token]),
            ..SourceMap::default()
        })
    }

    fn assert_test_result(concat_sm: SourceMap<'_>) {
        let expected = SourceMap::new(
            None,
            vec![
                Cow::Borrowed("foo"),
                Cow::Borrowed("foo2"),
                Cow::Borrowed("bar"),
                Cow::Borrowed("abc"),
            ],
            None,
            vec![
                Some(Cow::Borrowed("foo.js")),
                Some(Cow::Borrowed("bar.js")),
                Some(Cow::Borrowed("abc.js")),
            ],
            vec![],
            vec![
                Token::new(1, 1, 1, 1, Some(0), Some(0)),
                Token::new(3, 1, 1, 1, Some(1), Some(2)),
                Token::new(3, 2, 2, 2, Some(2), Some(3)),
            ],
        );

        assert_eq!(concat_sm.mappings, expected.mappings);
        assert_eq!(concat_sm.sources, expected.sources);
        assert_eq!(concat_sm.names, expected.names);
        assert_eq!(expected.to_json().mappings, concat_sm.to_json().mappings);
    }

    #[test]
    fn add_sourcemap_in_loop() {
        let [sm1, sm2, sm3] = build_test_inputs();
        let inputs = [(&sm1, 0u32), (&sm2, 2), (&sm3, 2)];
        let mut builder = ConcatSourceMapBuilder::default();
        for (sourcemap, line_offset) in inputs {
            builder.add_sourcemap(sourcemap, line_offset);
        }
        assert_test_result(builder.into_sourcemap());
    }

    #[test]
    fn from_sourcemaps() {
        let [sm1, sm2, sm3] = build_test_inputs();
        let builder = ConcatSourceMapBuilder::from_sourcemaps(&[(&sm1, 0), (&sm2, 2), (&sm3, 2)]);
        assert_test_result(builder.into_sourcemap());
    }

    #[test]
    fn add_sourcemap_owned() {
        // Moving owned maps in must produce the same result as borrowing them.
        let [sm1, sm2, sm3] = build_test_inputs();
        let mut builder = ConcatSourceMapBuilder::default();
        builder.add_sourcemap_owned(sm1, 0);
        builder.add_sourcemap_owned(sm2, 2);
        builder.add_sourcemap_owned(sm3, 2);
        assert_test_result(builder.into_sourcemap());
    }

    #[test]
    fn from_owned_sourcemaps() {
        let [sm1, sm2, sm3] = build_test_inputs();
        let builder =
            ConcatSourceMapBuilder::from_owned_sourcemaps(vec![(sm1, 0), (sm2, 2), (sm3, 2)]);
        assert_test_result(builder.into_sourcemap());
    }

    #[test]
    fn serialization_after_unmapped_segments() {
        let mapped = |tokens: Vec<Token>| SourceMap {
            sources: vec![Some("input.js".into())],
            mappings: tokens.into(),
            ..SourceMap::default()
        };
        let first =
            mapped(vec![Token::new(0, 0, 3, 5, Some(0), None), Token::new(0, 8, 0, 0, None, None)]);
        let unmapped = SourceMap {
            mappings: Mappings::from_tokens([Token::new(0, 0, 0, 0, None, None)]),
            ..SourceMap::default()
        };
        let next = mapped(vec![Token::new(0, 0, 2, 3, Some(0), None)]);

        for unmapped_count in [0, 2] {
            let mut builder = ConcatSourceMapBuilder::default();
            builder.add_sourcemap(&first, 0);
            for line in 1..=unmapped_count {
                builder.add_sourcemap(&unmapped, line);
            }
            builder.add_sourcemap(&next, unmapped_count + 1);
            let map = builder.into_sourcemap();
            let expected = Token::new(unmapped_count + 1, 0, 2, 3, Some(1), None);
            assert_eq!(map.get_tokens().last(), Some(expected));

            let json = map.to_json_string();
            for decoded in [
                SourceMap::from_json_string(&json).unwrap(),
                SourceMap::from_json(map.to_json()).unwrap(),
            ] {
                assert_eq!(decoded.get_tokens().last(), Some(expected));
            }
        }
    }

    #[test]
    fn owned_moves_strings_into_owned_sourcemap() {
        // Two owned maps with owned content; the move-in path must preserve every string and
        // renumber `other`'s ids/lines, ending up as a `'static` map with no data lost.
        let a = SourceMap::new(
            None,
            vec![Cow::Owned("name_a".to_string())],
            None,
            vec![Some(Cow::Owned("a.js".to_string()))],
            vec![Some(Cow::Owned("a content".to_string()))],
            vec![Token::new(0, 0, 0, 0, Some(0), Some(0))],
        );
        let b = SourceMap::new(
            None,
            vec![Cow::Owned("name_b".to_string())],
            None,
            vec![Some(Cow::Owned("b.js".to_string()))],
            vec![Some(Cow::Owned("b content".to_string()))],
            vec![Token::new(0, 0, 0, 0, Some(0), Some(0))],
        );

        let owned = ConcatSourceMapBuilder::from_owned_sourcemaps(vec![(a, 0), (b, 5)])
            .into_owned_sourcemap();

        assert_eq!(owned.get_sources().collect::<Vec<_>>(), vec![Some("a.js"), Some("b.js")]);
        assert_eq!(owned.get_names().collect::<Vec<_>>(), vec!["name_a", "name_b"]);
        assert_eq!(owned.get_source_content(0), Some("a content"));
        assert_eq!(owned.get_source_content(1), Some("b content"));
        // `b`'s token is shifted by the line offset and renumbered onto the combined ids.
        assert_eq!(owned.get_token(1), Some(Token::new(5, 0, 0, 0, Some(1), Some(1))));
    }

    #[test]
    fn owned_translates_after_tokenless_map() {
        // A first map that contributes a source but no tokens leaves `self.tokens` empty while the
        // source offset advances; the next map must still renumber its ids (not take the memcpy path).
        let tokenless = SourceMap::new(
            None,
            vec![],
            None,
            vec![Some(Cow::Owned("a.js".to_string()))],
            vec![Some(Cow::Owned("a content".to_string()))],
            vec![],
        );
        let with_token = SourceMap::new(
            None,
            vec![],
            None,
            vec![Some(Cow::Owned("b.js".to_string()))],
            vec![Some(Cow::Owned("b content".to_string()))],
            // Source id 0 within its own map; after concat it must point at "b.js" (combined index 1).
            vec![Token::new(0, 0, 0, 0, Some(0), None)],
        );

        let map =
            ConcatSourceMapBuilder::from_owned_sourcemaps(vec![(tokenless, 0), (with_token, 0)])
                .into_sourcemap();

        assert_eq!(map.get_sources().collect::<Vec<_>>(), vec![Some("a.js"), Some("b.js")]);
        assert_eq!(map.get_token(0).unwrap().get_source_id(), Some(1));
        assert_eq!(map.get_source_content(1), Some("b content"));
    }

    #[test]
    fn owned_pads_missing_source_contents() {
        // First map has a source but no `sourcesContent`; the second map's content must stay attached
        // to its own source rather than shifting onto the first map's source id.
        let no_content = SourceMap::new(
            None,
            vec![],
            None,
            vec![Some(Cow::Owned("a.js".to_string()))],
            vec![],
            vec![Token::new(0, 0, 0, 0, Some(0), None)],
        );
        let with_content = SourceMap::new(
            None,
            vec![],
            None,
            vec![Some(Cow::Owned("b.js".to_string()))],
            vec![Some(Cow::Owned("b content".to_string()))],
            vec![Token::new(0, 0, 0, 0, Some(0), None)],
        );

        let map =
            ConcatSourceMapBuilder::from_owned_sourcemaps(vec![(no_content, 0), (with_content, 1)])
                .into_sourcemap();

        assert_eq!(map.get_sources().collect::<Vec<_>>(), vec![Some("a.js"), Some("b.js")]);
        assert_eq!(map.get_source_content(0), None);
        assert_eq!(map.get_source_content(1), Some("b content"));
    }

    #[test]
    fn keeps_distinct_boundary_tokens() {
        // Boundary dedup only drops a token when it is *identical* to the
        // previous map's last token (same dst/src position and source/name id
        // after offsetting). Here the seam tokens differ, so nothing is
        // dropped — the counterpart to `dedups_identical_boundary_token`.
        let sm1 = SourceMap::new(
            None,
            vec![Cow::Borrowed("name1")],
            None,
            vec![Some(Cow::Borrowed("file1.js"))],
            vec![],
            vec![
                Token::new(1, 1, 1, 1, Some(0), Some(0)),
                Token::new(2, 5, 2, 5, Some(0), Some(0)),
            ],
        );

        // sm2 has different source_id/name_id after offset, so won't deduplicate
        let sm2 = SourceMap::new(
            None,
            vec![Cow::Borrowed("name2")],
            None,
            vec![Some(Cow::Borrowed("file2.js"))],
            vec![],
            vec![
                Token::new(2, 5, 2, 5, Some(0), Some(0)), // Different source/name after offset
                Token::new(3, 10, 3, 10, Some(0), Some(0)),
            ],
        );

        let mut builder = ConcatSourceMapBuilder::default();
        builder.add_sourcemap(&sm1, 0);
        builder.add_sourcemap(&sm2, 0);

        let concat_sm = builder.into_sourcemap();

        // Should have 4 tokens (no deduplication because source_id/name_id differ)
        assert_eq!(concat_sm.token_count(), 4);
    }

    #[test]
    fn dedups_identical_boundary_token() {
        // When the second map contributes no sources/names, its first token can
        // translate to exactly the first map's last token. The boundary dedup
        // must then drop the duplicate rather than emit it twice.
        let map1 = SourceMap::new(
            None,
            vec![],
            None,
            vec![],
            vec![],
            vec![Token::new(0, 0, 0, 0, None, None), Token::new(2, 3, 0, 0, None, None)],
        );
        let map2 = SourceMap::new(
            None,
            vec![],
            None,
            vec![],
            vec![],
            vec![Token::new(0, 3, 0, 0, None, None), Token::new(1, 9, 0, 0, None, None)],
        );

        let mut builder = ConcatSourceMapBuilder::default();
        builder.add_sourcemap(&map1, 0);
        // `map2`'s first token shifted by `line_offset` 2 equals `map1`'s last token.
        builder.add_sourcemap(&map2, 2);
        let concat_sm = builder.into_sourcemap();

        // 2 from map1 + 1 from map2 (its first token was deduplicated at the seam).
        assert_eq!(concat_sm.token_count(), 3);
        assert_eq!(concat_sm.get_token(1), Some(Token::new(2, 3, 0, 0, None, None)));
        assert_eq!(concat_sm.get_token(2), Some(Token::new(3, 9, 0, 0, None, None)));
    }
}
